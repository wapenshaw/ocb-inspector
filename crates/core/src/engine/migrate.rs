//! Bidirectional migration: target baseline plus reviewed setting transfer (SPEC §7).
//!
//! Every setting has selection, an engine-computed assessment, a user resolution and a derived
//! final action. Resolution decides the action; reassessment only decides whether it is permitted.

use super::edit::{
    BuildOutput, ManifestContext, PlannedWrite, apply_and_verify, build_manifest, check_header,
    normalize, plan_write,
};
use super::{RecipeState, Session, codec, decode_recipe};
use crate::catalog::{self, RECIPES, Writability};
use crate::contracts::*;
use crate::error::{Error, Result};
use std::collections::BTreeMap;

fn permitted(a: &Assessment) -> Vec<ResolutionKind> {
    use ResolutionKind::*;
    match (&a.status, a.reason) {
        (AssessmentStatus::Equivalent, _) => vec![None, KeepTarget, Omit, SetValue],
        (_, Some(Reason::UnsupportedValue)) => vec![SetValue, KeepTarget, Omit],
        _ => vec![KeepTarget, Omit],
    }
}

fn assessment(
    status: AssessmentStatus,
    reason: Option<Reason>,
    detail: impl Into<String>,
) -> Assessment {
    let mut a = Assessment {
        status,
        reason,
        detail: detail.into(),
        permitted_resolutions: vec![],
    };
    a.permitted_resolutions = permitted(&a);
    a
}

fn unavailable(reason: Reason, detail: impl Into<String>) -> Assessment {
    assessment(AssessmentStatus::Unavailable, Some(reason), detail)
}

/// Assess carrying the source value into the target. Returns the assessment and translated value.
fn assess_source(
    src: &Session,
    tgt: &Session,
    recipe: &catalog::Recipe,
    s: &RecipeState,
    t: &RecipeState,
) -> (Assessment, Option<Value>) {
    let RecipeState::Decoded(sd) = s else {
        return (
            unavailable(Reason::AmbiguousSemantics, "source location is ambiguous"),
            None,
        );
    };
    let td = match t {
        RecipeState::Missing => {
            return (
                unavailable(
                    Reason::MissingSetting,
                    format!("{} has no {}", tgt.schema.identity.bios_id, recipe.label),
                ),
                None,
            );
        }
        RecipeState::Ambiguous(e) => {
            return (unavailable(Reason::AmbiguousSemantics, e.clone()), None);
        }
        RecipeState::Decoded(d) => d,
    };
    if !tgt.approved() {
        return (
            unavailable(
                Reason::RecipeUnavailable,
                format!("target: {}", tgt.blocked_reason()),
            ),
            None,
        );
    }
    if let Writability::ReadOnly(why) = recipe.writability {
        return (unavailable(Reason::RecipeUnavailable, why), None);
    }
    let translated = match codec::translate(recipe, &sd.fields, &td.fields, &sd.value) {
        Ok(v) => v,
        Err(e) => return (unavailable(Reason::UnsupportedValue, e), None),
    };
    if let Err(e) = codec::encode(recipe, &td.fields, &translated, &td.raw_values()) {
        return (unavailable(Reason::UnsupportedValue, e), None);
    }
    let _ = src;
    let detail = if matches!(translated, Value::Auto) {
        "Transfers the Auto choice; the destination firmware may resolve Auto differently."
            .to_string()
    } else {
        format!(
            "Reviewed equivalent encoding in {}",
            tgt.schema.identity.bios_id
        )
    };
    (
        assessment(AssessmentStatus::Equivalent, None, detail),
        Some(translated),
    )
}

fn is_selected(recipe: &catalog::Recipe, sel: &Selection) -> bool {
    sel.settings.iter().any(|s| s == recipe.id)
        || sel
            .groups
            .iter()
            .any(|g| recipe.group == g || recipe.group.starts_with(&format!("{g}.")) || g == "all")
}

fn state_value(s: &RecipeState) -> Option<Value> {
    match s {
        RecipeState::Decoded(d) => Some(d.value.clone()),
        _ => None,
    }
}

/// Compute a full migration plan. `prior` carries user selection overrides and resolutions; its
/// assessments become unverified history and never influence execution.
pub fn plan(
    src: &Session,
    tgt: &Session,
    selection: Selection,
    prior: Option<&Patch>,
) -> Result<(Patch, Vec<PlannedWrite>)> {
    if src.schema.identity.board != tgt.schema.identity.board
        || src.schema.identity.flavor != tgt.schema.identity.flavor
    {
        return Err(Error::IdentityMismatch(format!(
            "cross-board/flavor migration is not supported ({} → {})",
            src.schema.identity.bios_id, tgt.schema.identity.bios_id
        )));
    }
    if src.profile.oci.cpu_brand != tgt.profile.oci.cpu_brand {
        return Err(Error::Unsupported(format!(
            "source and target baselines were saved with different CPUs ('{}' vs '{}'); CPU context is not supported",
            src.profile.oci.cpu_brand, tgt.profile.oci.cpu_brand
        )));
    }
    let prior_entries: BTreeMap<String, &MigrationEntry> = prior
        .and_then(|p| p.migration.as_ref())
        .map(|m| {
            m.entries
                .iter()
                .map(|e| (e.setting_id.clone(), e))
                .collect()
        })
        .unwrap_or_default();
    for id in prior_entries.keys() {
        if catalog::recipe(id).is_none() {
            return Err(Error::Request(format!(
                "plan references unknown setting_id '{id}'"
            )));
        }
    }

    let mut entries = Vec::new();
    let mut target_only = Vec::new();
    let mut writes = Vec::new();
    let mut diagnostics = Vec::new();

    for recipe in RECIPES {
        let s = decode_recipe(src, recipe);
        let t = decode_recipe(tgt, recipe);
        let in_scope = is_selected(recipe, &selection);
        if matches!(s, RecipeState::Missing) {
            if in_scope && !matches!(t, RecipeState::Missing) {
                target_only.push(TargetOnlyEntry {
                    setting_id: recipe.id.into(),
                    label: recipe.label.into(),
                    retained_value: state_value(&t),
                    note: format!(
                        "Exists only in {}; the target baseline value is retained.",
                        tgt.schema.identity.bios_id
                    ),
                });
            }
            continue;
        }
        let prior_e = prior_entries.get(recipe.id);
        let selected = prior_e
            .map(|e| e.selection == SelectionState::Selected)
            .unwrap_or(in_scope);
        let resolution = prior_e.map(|e| e.resolution.clone()).unwrap_or(Resolution {
            kind: ResolutionKind::None,
            value: None,
        });
        let history: Vec<HistoryEntry> = prior_e
            .map(|e| {
                let mut h: Vec<HistoryEntry> = e
                    .history
                    .iter()
                    .cloned()
                    .map(|mut x| {
                        x.provenance = "supplied_unverified".into();
                        x
                    })
                    .collect();
                if let Some(a) = &e.assessment {
                    h.push(HistoryEntry {
                        provenance: "supplied_unverified".into(),
                        assessment: a.clone(),
                    });
                }
                h
            })
            .unwrap_or_default();

        let source_value = state_value(&s);
        let target_value = state_value(&t);
        let mut entry = MigrationEntry {
            setting_id: recipe.id.into(),
            label: recipe.label.into(),
            group: recipe.group.into(),
            selection: if selected {
                SelectionState::Selected
            } else {
                SelectionState::Unselected
            },
            source_value: source_value.clone(),
            target_baseline_value: target_value.clone(),
            assessment: None,
            history,
            resolution: resolution.clone(),
            final_action: None,
            proposed_value: target_value.clone(),
            notes: vec![],
        };

        if !selected {
            entry.final_action = Some(FinalAction::RetainTarget);
            entries.push(entry);
            continue;
        }

        let (source_assessment, translated) = assess_source(src, tgt, recipe, &s, &t);
        if !source_assessment
            .permitted_resolutions
            .contains(&resolution.kind)
        {
            entry.notes.push(format!(
                "Resolution {:?} is not permitted for this assessment.",
                resolution.kind
            ));
            entry.assessment = Some(source_assessment);
            entry.final_action = Some(FinalAction::Blocked);
            entries.push(entry);
            continue;
        }
        match resolution.kind {
            ResolutionKind::SetValue => {
                let Some(v) = resolution.value.clone() else {
                    return Err(Error::Request(format!(
                        "{}: set_value resolution requires a value",
                        recipe.id
                    )));
                };
                entry.notes.push(format!(
                    "Source value assessment: {}",
                    source_assessment.detail
                ));
                // Reassess against the explicit replacement; the action stays `set` either way.
                match plan_write(tgt, recipe, &v) {
                    Ok(w) => {
                        let to = normalize(tgt, recipe, &v);
                        entry.assessment = Some(assessment(
                            AssessmentStatus::Equivalent,
                            None,
                            "Explicit replacement is representable in the target",
                        ));
                        entry.final_action = Some(FinalAction::Set);
                        entry.proposed_value = Some(to.clone());
                        writes.push(PlannedWrite {
                            setting_id: recipe.id.into(),
                            label: recipe.label.into(),
                            action: FinalAction::Set,
                            from: target_value,
                            to,
                            writes: w,
                        });
                    }
                    Err(e) => {
                        entry.assessment = Some(unavailable(
                            Reason::UnsupportedValue,
                            format!("replacement: {e}"),
                        ));
                        entry.final_action = Some(FinalAction::Blocked);
                    }
                }
            }
            ResolutionKind::KeepTarget | ResolutionKind::Omit => {
                entry.assessment = Some(source_assessment);
                entry.final_action = Some(FinalAction::RetainTarget);
            }
            ResolutionKind::None => {
                let equivalent = source_assessment.status == AssessmentStatus::Equivalent;
                entry.assessment = Some(source_assessment);
                match (equivalent, translated) {
                    (true, Some(v)) => {
                        let to = normalize(tgt, recipe, &v);
                        match plan_write(tgt, recipe, &to) {
                            Ok(w) => {
                                entry.final_action = Some(FinalAction::Transfer);
                                entry.proposed_value = Some(to.clone());
                                writes.push(PlannedWrite {
                                    setting_id: recipe.id.into(),
                                    label: recipe.label.into(),
                                    action: FinalAction::Transfer,
                                    from: target_value,
                                    to,
                                    writes: w,
                                });
                            }
                            Err(e) => {
                                entry.assessment = Some(unavailable(Reason::UnsupportedValue, e));
                                entry.final_action = Some(FinalAction::Blocked);
                            }
                        }
                    }
                    _ => entry.final_action = Some(FinalAction::Blocked),
                }
            }
        }
        if entry.final_action == Some(FinalAction::Blocked) {
            diagnostics.push(
                Diagnostic::new(
                    "unresolved_conflict",
                    Severity::Error,
                    "migration",
                    format!(
                        "{}: {}",
                        recipe.label,
                        entry
                            .assessment
                            .as_ref()
                            .map(|a| a.detail.clone())
                            .unwrap_or_default()
                    ),
                )
                .setting(recipe.id),
            );
        }
        entries.push(entry);
    }

    let blocked = entries
        .iter()
        .filter(|e| e.final_action == Some(FinalAction::Blocked))
        .count();
    let mut can_build = blocked == 0 && tgt.approved();
    // Final-state validation of the complete destination configuration.
    if blocked == 0
        && let Err(e) = apply_and_verify(tgt, &writes)
    {
        can_build = false;
        diagnostics.push(Diagnostic::new(
            e.code(),
            Severity::Error,
            "final_state",
            e.to_string(),
        ));
    }
    if !tgt.approved() {
        diagnostics.push(Diagnostic::new(
            "container_writer_unavailable",
            Severity::Error,
            "writer",
            format!(
                "The plan can be saved but not built: {}.",
                tgt.blocked_reason()
            ),
        ));
    }
    let other = super::inspect::diff(src, tgt)
        .payload
        .other_field_differences
        .len();
    let accounting = Accounting {
        selected: entries
            .iter()
            .filter(|e| e.selection == SelectionState::Selected)
            .count(),
        unselected: entries
            .iter()
            .filter(|e| e.selection == SelectionState::Unselected)
            .count(),
        blocked,
        other_differing_fields: other,
        opaque_records_retained_from_target: tgt.profile.records.len(),
    };
    if other > 0 {
        diagnostics.push(Diagnostic::new(
            "unselected_differences",
            Severity::Info,
            "migration",
            format!("{other} other named BIOS fields differ between source and target baseline; the target baseline values are kept."),
        ));
    }
    let patch = Patch {
        kind: KIND_PATCH.into(),
        schema_version: SCHEMA_VERSION.into(),
        producer: Producer::this(),
        mode: PatchMode::Migrate,
        payload_version: PAYLOAD_VERSION.into(),
        base: None,
        source: Some(src.binding()),
        target: Some(tgt.binding()),
        operations: vec![],
        migration: Some(MigrationPlan {
            selection,
            entries,
            target_only,
            accounting,
            can_build,
        }),
        policy: Policy {
            unresolved: "error".into(),
        },
        extensions: None,
        diagnostics,
    };
    Ok((patch, writes))
}

/// Revalidate a supplied plan against fresh inputs and build the destination profile.
pub fn build(
    src: &Session,
    tgt: &Session,
    supplied: &Patch,
    output_name: &str,
) -> Result<BuildOutput> {
    check_header(supplied, PatchMode::Migrate)?;
    let m = supplied
        .migration
        .as_ref()
        .ok_or_else(|| Error::Request("migrate patch has no migration plan".into()))?;
    for (b, s, role) in [
        (&supplied.source, src, "source"),
        (&supplied.target, tgt, "target"),
    ] {
        match b {
            Some(b) if *b == s.binding() => {}
            Some(_) => {
                return Err(Error::BindingChanged(format!(
                    "{role} inputs differ from the plan's binding"
                )));
            }
            None => return Err(Error::Request(format!("plan has no {role} binding"))),
        }
    }
    // Approval first: without it every entry is blocked, which would hide the real reason.
    if !tgt.approved() {
        return Err(Error::Unsupported(format!(
            "target {}: {}",
            tgt.schema.identity.bios_id,
            tgt.blocked_reason()
        )));
    }
    let (plan, writes) = plan(src, tgt, m.selection.clone(), Some(supplied))?;
    let pm = plan
        .migration
        .as_ref()
        .ok_or_else(|| Error::Internal("plan lost".into()))?;
    let blocked: Vec<&str> = pm
        .entries
        .iter()
        .filter(|e| e.final_action == Some(FinalAction::Blocked))
        .map(|e| e.setting_id.as_str())
        .collect();
    if !blocked.is_empty() {
        return Err(Error::Unresolved(blocked.join(", ")));
    }
    let (bytes, checks, spans) = apply_and_verify(tgt, &writes)?;
    let retained = pm
        .entries
        .iter()
        .filter(|e| e.final_action == Some(FinalAction::RetainTarget))
        .map(|e| {
            format!(
                "{} ({})",
                e.setting_id,
                if e.selection == SelectionState::Unselected {
                    "unselected"
                } else {
                    "resolved: keep target"
                }
            )
        })
        .chain(
            pm.target_only
                .iter()
                .map(|t| format!("{} (target-only)", t.setting_id)),
        )
        .collect();
    let history: Vec<serde_json::Value> = pm
        .entries
        .iter()
        .filter(|e| !e.history.is_empty())
        .map(|e| serde_json::json!({"setting_id": e.setting_id, "provenance": "supplied_unverified", "history": e.history}))
        .collect();
    let ctx = ManifestContext {
        operation: PatchMode::Migrate,
        source: Some(src.binding()),
        retained,
        output_name: output_name.into(),
        supplied_history: history,
    };
    let manifest = build_manifest(ctx, tgt, &writes, (&bytes, checks, spans));
    Ok(BuildOutput { bytes, manifest })
}
