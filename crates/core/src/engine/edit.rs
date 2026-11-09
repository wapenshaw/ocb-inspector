//! Same-BIOS typed edits (`ocb.patch`, mode `edit`) and the shared transactional apply.

use super::verify::{Intended, all_pass, verify_output};
use super::{RecipeState, Session, codec, decode_all, decode_recipe, rule_violations};
use crate::bytes::sha256_hex;
use crate::catalog::{self, ADAPTER_ID, ADAPTER_VERSION, RECIPE_SET_VERSION};
use crate::contracts::*;
use crate::error::{Error, Result};
use crate::format::{Mutation, diff_offsets};

/// One planned write against a baseline session.
#[derive(Debug, Clone)]
pub struct PlannedWrite {
    pub setting_id: String,
    pub label: String,
    pub action: FinalAction,
    pub from: Option<Value>,
    pub to: Value,
    /// (file offset, width, raw)
    pub writes: Vec<(usize, usize, u64)>,
}

/// Plan the raw writes for setting `value` on `session`; Err explains why it is not writable.
pub fn plan_write(
    session: &Session,
    recipe: &catalog::Recipe,
    value: &Value,
) -> std::result::Result<Vec<(usize, usize, u64)>, String> {
    if !session.approved() {
        return Err(session.blocked_reason());
    }
    match decode_recipe(session, recipe) {
        RecipeState::Missing => Err("setting does not exist in this BIOS".into()),
        RecipeState::Ambiguous(e) => Err(format!("ambiguous location: {e}")),
        RecipeState::Decoded(d) => {
            let raws = codec::encode(recipe, &d.fields, value, &d.raw_values())?;
            Ok(d.fields
                .iter()
                .zip(&d.raws)
                .zip(raws)
                .map(|((f, (at, _)), raw)| (*at, f.width, raw))
                .collect())
        }
    }
}

/// Apply writes as one transaction, then verify the result independently. Nothing is published here.
pub fn apply_and_verify(
    baseline: &Session,
    writes: &[PlannedWrite],
) -> Result<(Vec<u8>, Vec<Check>, Vec<ByteSpan>)> {
    let mut m = Mutation::from_baseline(&baseline.profile);
    let mut claimed: Vec<(usize, u8, &str)> = Vec::new();
    for w in writes {
        for &(at, width, raw) in &w.writes {
            let bytes = raw.to_le_bytes();
            for (i, &byte) in bytes.iter().enumerate().take(width) {
                // Overlapping writes must agree at the byte level.
                if let Some((_, prev, who)) = claimed.iter().find(|(o, _, _)| *o == at + i)
                    && *prev != byte
                {
                    return Err(Error::Validation(format!(
                        "{} and {} write different values to 0x{:x}",
                        who,
                        w.setting_id,
                        at + i
                    )));
                }
                claimed.push((at + i, byte, &w.setting_id));
            }
            m.write_uint(at, width, raw)?;
        }
    }
    let out = m.finish()?;
    let bytes = out.into_bytes();

    // Final-state validation on the complete resulting configuration.
    let staged = Session::pair(baseline.schema.clone(), bytes.clone())?;
    let violations = rule_violations(&decode_all(&staged));
    if !violations.is_empty() {
        return Err(Error::Validation(
            violations
                .into_iter()
                .map(|v| v.1)
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }

    let intended: Vec<Intended> = writes
        .iter()
        .map(|w| Intended {
            setting_id: w.setting_id.clone(),
            value: w.to.clone(),
            spans: w.writes.iter().map(|(a, wd, _)| (*a, a + wd)).collect(),
        })
        .collect();
    let checks = verify_output(baseline, &bytes, &intended);
    if !all_pass(&checks) {
        let failed: Vec<String> = checks
            .iter()
            .filter(|c| c.status == CheckStatus::Fail)
            .map(|c| format!("{}: {}", c.name, c.detail))
            .collect();
        return Err(Error::Verification(failed.join("; ")));
    }
    let spans =
        super::inspect::labelled_spans(baseline, &diff_offsets(baseline.profile.bytes(), &bytes));
    Ok((bytes, checks, spans))
}

pub fn check_header(p: &Patch, mode: PatchMode) -> Result<()> {
    if p.kind != KIND_PATCH {
        return Err(Error::Request(format!(
            "expected kind {KIND_PATCH}, got {}",
            p.kind
        )));
    }
    if p.schema_version != SCHEMA_VERSION {
        return Err(Error::Request(format!(
            "unsupported schema_version {}; supported: {SCHEMA_VERSION}",
            p.schema_version
        )));
    }
    if p.mode != mode {
        return Err(Error::Request(format!(
            "patch mode is {:?}; this command needs {mode:?}",
            p.mode
        )));
    }
    if p.payload_version != PAYLOAD_VERSION {
        return Err(Error::Request(format!(
            "unsupported payload_version {}; supported: {PAYLOAD_VERSION}",
            p.payload_version
        )));
    }
    if p.policy.unresolved != "error" {
        return Err(Error::Request("policy.unresolved must be \"error\"".into()));
    }
    Ok(())
}

fn check_binding(expected: &Option<Binding>, actual: &Binding, role: &str) -> Result<()> {
    match expected {
        None => Err(Error::Request(format!("patch has no {role} binding"))),
        Some(b) if b != actual => Err(Error::BindingChanged(format!(
            "{role} binding differs from the supplied inputs (profile {}…, BIOS {}…)",
            &actual.profile_sha256[..12],
            &actual.bios_sha256[..12]
        ))),
        Some(_) => Ok(()),
    }
}

/// Validate an edit patch. Returns planned writes; any failure is an error with a stable exit code.
pub fn plan_edit(session: &Session, patch: &Patch) -> Result<Vec<PlannedWrite>> {
    check_header(patch, PatchMode::Edit)?;
    check_binding(&patch.base, &session.binding(), "base")?;
    if patch.operations.is_empty() {
        return Err(Error::Request("patch has no operations".into()));
    }
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for op in &patch.operations {
        if op.op != "set" {
            return Err(Error::Request(format!("unknown operation '{}'", op.op)));
        }
        if !seen.insert(op.setting_id.clone()) {
            return Err(Error::Request(format!(
                "{} is set more than once",
                op.setting_id
            )));
        }
        let recipe = catalog::recipe(&op.setting_id)
            .ok_or_else(|| Error::Request(format!("unknown setting_id '{}'", op.setting_id)))?;
        let current = match decode_recipe(session, recipe) {
            RecipeState::Decoded(d) => d.value,
            RecipeState::Missing => {
                return Err(Error::Unsupported(format!(
                    "{} does not exist in this BIOS",
                    op.setting_id
                )));
            }
            RecipeState::Ambiguous(e) => {
                return Err(Error::Unsupported(format!("{}: {e}", op.setting_id)));
            }
        };
        if !op.expected.same_intent(&current) {
            return Err(Error::Validation(format!(
                "precondition failed for {}: expected {}, profile has {}",
                op.setting_id,
                op.expected.display(),
                current.display()
            )));
        }
        let writes = plan_write(session, recipe, &op.value)
            .map_err(|e| Error::Validation(format!("{}: {e}", op.setting_id)))?;
        out.push(PlannedWrite {
            setting_id: recipe.id.into(),
            label: recipe.label.into(),
            action: FinalAction::Set,
            from: Some(current),
            to: normalize(session, recipe, &op.value),
            writes,
        });
    }
    Ok(out)
}

/// Fill display fields (raw/label) of an enum value from the target schema.
pub fn normalize(session: &Session, recipe: &catalog::Recipe, v: &Value) -> Value {
    if let (Value::Enum { symbol, .. }, RecipeState::Decoded(d)) =
        (v, decode_recipe(session, recipe))
        && let Some((raw, label)) = d.fields[0]
            .options
            .iter()
            .find(|(_, l)| codec::slug(l) == *symbol)
    {
        return Value::Enum {
            symbol: symbol.clone(),
            raw: Some(*raw),
            label: Some(label.clone()),
        };
    }
    v.clone()
}

#[derive(Debug)]
pub struct BuildOutput {
    pub bytes: Vec<u8>,
    pub manifest: ExportManifest,
}

/// Context of a build, beyond the baseline and its writes.
pub struct ManifestContext {
    pub operation: PatchMode,
    pub source: Option<Binding>,
    pub retained: Vec<String>,
    pub output_name: String,
    pub supplied_history: Vec<serde_json::Value>,
}

pub fn build_manifest(
    ctx: ManifestContext,
    base: &Session,
    writes: &[PlannedWrite],
    (bytes, checks, spans): (&[u8], Vec<Check>, Vec<ByteSpan>),
) -> ExportManifest {
    // Only writes that change the decoded value are changes; equal transfers are recorded as retained.
    let ManifestContext {
        operation,
        source,
        retained,
        output_name,
        supplied_history,
    } = ctx;
    let mut retained = retained;
    retained.extend(
        writes
            .iter()
            .filter(|w| w.from.as_ref().is_some_and(|f| f.same_intent(&w.to)))
            .map(|w| {
                format!(
                    "{} (transferred value already equal to baseline: {})",
                    w.setting_id,
                    w.to.display()
                )
            }),
    );
    let changes = writes
        .iter()
        .filter(|w| !w.from.as_ref().is_some_and(|f| f.same_intent(&w.to)))
        .map(|w| ChangeRecord {
            setting_id: w.setting_id.clone(),
            label: w.label.clone(),
            action: w.action,
            from: w.from.clone(),
            to: Some(w.to.clone()),
            file_spans: w
                .writes
                .iter()
                .map(|(a, wd, _)| ByteSpan {
                    start: *a,
                    end: a + wd,
                    region: super::inspect::region_name(base, *a),
                })
                .collect(),
        })
        .collect();
    ExportManifest {
        kind: KIND_MANIFEST.into(),
        schema_version: SCHEMA_VERSION.into(),
        producer: Producer::this(),
        operation,
        payload_version: PAYLOAD_VERSION.into(),
        adapter_id: ADAPTER_ID.into(),
        adapter_version: ADAPTER_VERSION.into(),
        recipe_set_version: RECIPE_SET_VERSION.into(),
        source,
        base: base.binding(),
        output: OutputFile { file_name: output_name, sha256: sha256_hex(bytes), size: bytes.len() },
        capabilities: base.capabilities(),
        changes,
        retained,
        binary_spans: spans,
        checks,
        supplied_history_unverified: supplied_history,
        validation: "valid".into(),
        limitations: vec![
            "Binary checks establish consistency with the supported layout, not BIOS import acceptance.".into(),
            "Import acceptance has not been demonstrated for generated files; import manually on matching firmware and re-save to confirm.".into(),
            "Validation does not establish electrical safety or stability.".into(),
            "Auto values delegate to the destination firmware, which may resolve them differently.".into(),
            "Writing Auto to the core voltage offset keeps the baseline's inactive magnitude bytes (SPEC I-04); BIOS-saved Auto profiles may store 0 there. Whether firmware treats these differently cannot be established offline.".into(),
        ],
    }
}

pub fn build_edit(session: &Session, patch: &Patch, output_name: &str) -> Result<BuildOutput> {
    let writes = plan_edit(session, patch)?;
    let (bytes, checks, spans) = apply_and_verify(session, &writes)?;
    let ctx = ManifestContext {
        operation: PatchMode::Edit,
        source: None,
        retained: vec![],
        output_name: output_name.into(),
        supplied_history: vec![],
    };
    let manifest = build_manifest(ctx, session, &writes, (&bytes, checks, spans));
    Ok(BuildOutput { bytes, manifest })
}
