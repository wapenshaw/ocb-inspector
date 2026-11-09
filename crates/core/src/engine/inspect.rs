//! Read-only operations: BIOS catalog, profile inspection and diff.

use super::{RecipeState, Session, activity, codec, decode_all, writability_reason};
use crate::catalog::GROUPS;
use crate::contracts::*;
use crate::firmware::{BiosSchema, DECODED_STORES};
use crate::format::{CHECKSUM_AT, COUNT_AT, RECORDS_AT, diff_offsets, spans};
use std::collections::BTreeMap;

pub fn bios_catalog(schema: &BiosSchema, path: Option<String>) -> Report<CatalogPayload> {
    let questions = schema
        .decoded_questions()
        .map(|q| serde_json::to_value(q).unwrap_or(serde_json::Value::Null))
        .collect();
    let stores = schema
        .stores
        .iter()
        .map(|s| serde_json::to_value(s).unwrap_or(serde_json::Value::Null))
        .collect();
    let diags = schema
        .diagnostics
        .iter()
        .map(|d| {
            Diagnostic::new(
                "firmware_parse_note",
                Severity::Info,
                "bios_parse",
                d.clone(),
            )
        })
        .collect();
    Report::new(
        "bios_catalog",
        CatalogPayload {
            bios: super::session::bios_summary(schema, path),
            stores,
            questions,
        },
        diags,
    )
}

/// Where a recipe's primary field appears in the BIOS menus. Display only.
///
/// Copies under `suppressif TRUE` are never shown by the BIOS and are skipped when another copy
/// exists. Among the rest, a page under "Overclocking" wins, then IFR order.
fn menu_location(
    schema: &BiosSchema,
    recipe: &crate::catalog::Recipe,
    fields: &[codec::ResolvedField],
) -> (Option<MenuLocation>, Option<String>) {
    // The signed-offset codec's first field is the sign; the magnitude carries the menu prompt.
    let idx = match recipe.codec {
        crate::catalog::Codec::SignedOffset { .. } => 1,
        _ => 0,
    };
    let (Some(f), Some(fr)) = (fields.get(idx), recipe.codec.fields().get(idx).copied()) else {
        return (None, None);
    };
    let named: Vec<&crate::firmware::ifr::Question> = f
        .questions
        .iter()
        .filter(|q| fr.prompts.contains(&q.prompt.as_str()))
        .collect();
    let mut qs: Vec<&crate::firmware::ifr::Question> = if named.is_empty() {
        f.questions
            .iter()
            .filter(|q| !q.prompt.trim().is_empty())
            .collect()
    } else {
        named
    };
    if qs.iter().any(|q| !q.never_shown()) {
        qs.retain(|q| !q.never_shown());
    }
    let primary = qs
        .iter()
        .find(|q| {
            schema
                .menu_path(q)
                .first()
                .is_some_and(|p| p == "Overclocking")
        })
        .or(qs.first())
        .copied();
    let Some(primary) = primary else {
        return (None, None);
    };
    let path = schema.menu_path(primary);
    let mut also_on: Vec<Vec<String>> = Vec::new();
    for q in &qs {
        let p = schema.menu_path(q);
        if p != path && !p.is_empty() && !also_on.contains(&p) {
            also_on.push(p);
        }
    }
    let help = Some(primary.help.trim().to_string()).filter(|h| !h.is_empty());
    (
        Some(MenuLocation {
            path,
            section: primary.section.clone(),
            prompt: primary.prompt.trim().to_string(),
            order: schema.menu_order(primary),
            also_on,
        }),
        help,
    )
}

/// Decode a recipe's fields from the BIOS's optimized defaults.
fn default_value(
    schema: &BiosSchema,
    recipe: &crate::catalog::Recipe,
    fields: &[codec::ResolvedField],
) -> Option<Value> {
    let raws: Option<Vec<u64>> = fields
        .iter()
        .map(|f| schema.default_raw(f.store, f.offset, f.width))
        .collect();
    Some(codec::decode(&recipe.codec, fields, &raws?))
}

pub fn decoded_settings(session: &Session) -> Vec<DecodedSetting> {
    let states = decode_all(session);
    states
        .iter()
        .map(|(recipe, state)| {
            let (writable, auto_only, reason) = writability_reason(session, recipe, state);
            let (act, act_reason) = activity(&states, recipe.id);
            let (available, value, constraints, fields) = match state {
                RecipeState::Decoded(d) => (
                    true,
                    Some(d.value.clone()),
                    Some(codec::constraints(recipe, &d.fields)),
                    codec::evidence(session, &d.fields),
                ),
                _ => (false, None, None, vec![]),
            };
            let (menu, bios_help, default) = match state {
                RecipeState::Decoded(d) => {
                    let (m, h) = menu_location(&session.schema, recipe, &d.fields);
                    (m, h, default_value(&session.schema, recipe, &d.fields))
                }
                _ => (None, None, None),
            };
            let differs_from_default = match (&value, &default) {
                (Some(v), Some(dv)) => Some(v != dv),
                _ => None,
            };
            DecodedSetting {
                setting_id: recipe.id.into(),
                group: recipe.group.into(),
                label: recipe.label.into(),
                summary: recipe.summary.into(),
                available,
                value,
                activity: if available { act } else { Activity::Unknown },
                activity_reason: if available {
                    act_reason
                } else {
                    "Not present in this BIOS.".into()
                },
                editability: Editability {
                    writable,
                    auto_only,
                    reason,
                },
                constraints,
                fields,
                evidence: recipe.evidence.iter().map(|s| s.to_string()).collect(),
                menu,
                bios_help,
                default_value: default,
                differs_from_default,
            }
        })
        .collect()
}

pub fn raw_fields(session: &Session) -> Vec<RawField> {
    // One row per (store, offset, prompt); prefer a copy the BIOS actually displays.
    type Key = (String, u16, String);
    let mut best: BTreeMap<Key, &crate::firmware::ifr::Question> = BTreeMap::new();
    let mut order: Vec<Key> = Vec::new();
    for q in session.schema.decoded_questions() {
        let key = (q.store.clone(), q.offset, q.prompt.clone());
        match best.get(&key) {
            None => {
                order.push(key.clone());
                best.insert(key, q);
            }
            Some(prev) if prev.never_shown() && !q.never_shown() => {
                best.insert(key, q);
            }
            Some(_) => {}
        }
    }
    let mut out = Vec::new();
    for key in &order {
        let Some(q) = best.get(key).copied() else {
            continue;
        };
        let Some(rec) = session.record_for(&q.store) else {
            continue;
        };
        let w = usize::from(q.width_bits / 8);
        if usize::from(q.offset) + w > rec.size {
            continue;
        }
        let Ok(raw) = crate::bytes::uint_at(
            session.profile.bytes(),
            rec.offset + usize::from(q.offset),
            w,
            "field",
        ) else {
            continue;
        };
        let label_of = |v: u64| {
            q.options
                .iter()
                .find(|o| o.value == v)
                .map(|o| o.label.trim().to_string())
        };
        let label = label_of(raw);
        let default_raw = session.schema.default_raw(&q.store, q.offset, w);
        out.push(RawField {
            store: q.store.clone(),
            offset: q.offset,
            width_bits: q.width_bits,
            prompt: q.prompt.clone(),
            raw,
            label,
            kind: format!("{:?}", q.kind).to_lowercase(),
            callback: q.is_callback(),
            menu_path: session.schema.menu_path(q),
            menu_order: session.schema.menu_order(q),
            section: q.section.clone(),
            default_raw,
            default_label: default_raw.and_then(label_of),
        });
    }
    out
}

/// Setting ID of a generic (unreviewed) BIOS field.
pub fn bios_setting_id(store: &str, offset: u16) -> String {
    format!("bios.{store}.{offset:04x}")
}

/// Generic value of one stored field: the BIOS's own option label, a plain number, or a
/// checkbox. A raw value with no matching option is kept as unknown, never coerced.
fn generic_value(q: &crate::firmware::ifr::Question, raw: u64) -> Value {
    use crate::firmware::ifr::QuestionKind;
    match q.kind {
        QuestionKind::Numeric => Value::Integer {
            value: i64::try_from(raw).unwrap_or(i64::MAX),
            unit: String::new(),
        },
        QuestionKind::Checkbox if q.options.is_empty() => Value::Boolean { value: raw != 0 },
        _ => match q.options.iter().find(|o| o.value == raw) {
            Some(o) => Value::Enum {
                symbol: raw.to_string(),
                raw: Some(raw),
                label: Some(o.label.trim().to_string()),
            },
            None => Value::Unknown { raw },
        },
    }
}

/// Every setting the BIOS menus show in Setup/CpuSetup that no reviewed recipe covers, with this
/// profile's value and the optimized default. Read-only: these are display, never write paths.
/// One row per stored field; copies on several pages are listed as `also_on`. Fields the BIOS
/// never shows (`suppressif TRUE` on every copy) stay in `raw_fields` only.
pub fn bios_settings(session: &Session) -> Vec<DecodedSetting> {
    use crate::firmware::ifr::Question;
    let schema = &session.schema;
    // Bytes owned by reviewed recipes: those settings are shown by their recipe instead.
    let mut owned: Vec<(String, u16, u16)> = Vec::new();
    for (_, state) in decode_all(session) {
        if let RecipeState::Decoded(d) = state {
            for f in &d.fields {
                owned.push((f.store.to_string(), f.offset, f.offset + f.width as u16));
            }
        }
    }
    let covered = |store: &str, off: u16, w: u16| {
        owned
            .iter()
            .any(|(s, a, b)| s == store && off < *b && off + w > *a)
    };

    // Group copies by stored field, in IFR order.
    let mut groups: BTreeMap<(String, u16, u16), Vec<&Question>> = BTreeMap::new();
    let mut order: Vec<(String, u16, u16)> = Vec::new();
    for q in schema.decoded_questions() {
        if q.prompt.trim().is_empty() || q.width_bits == 0 || q.width_bits % 8 != 0 {
            continue;
        }
        let key = (q.store.clone(), q.offset, q.width_bits);
        groups
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key.clone());
                Vec::new()
            })
            .push(q);
    }

    let mut out = Vec::new();
    for key in &order {
        let Some(copies) = groups.get(key) else {
            continue;
        };
        let shown: Vec<&Question> = copies
            .iter()
            .copied()
            .filter(|q| !q.never_shown())
            .collect();
        let Some(primary) = shown.first().copied() else {
            continue;
        };
        let (store, offset, width_bits) = key;
        let w = width_bits / 8;
        if covered(store, *offset, w) {
            continue;
        }
        let Some(static_store) = DECODED_STORES.iter().find(|s| **s == store.as_str()) else {
            continue;
        };
        let rf = codec::ResolvedField {
            store: static_store,
            offset: *offset,
            width: usize::from(w),
            options: primary
                .options
                .iter()
                .map(|o| (o.value, o.label.trim().to_string()))
                .collect(),
            primary_kind: primary.kind,
            min: primary.min,
            max: primary.max,
            step: primary.step,
            questions: copies.iter().map(|q| (*q).clone()).collect(),
            label_conflicts: vec![],
        };
        let Some((_, raw)) = codec::read_raw(session, &rf) else {
            continue;
        };
        let value = generic_value(primary, raw);
        let default_raw = schema.default_raw(store, *offset, usize::from(w));
        let default_value = default_raw.map(|d| generic_value(primary, d));
        let path = schema.menu_path(primary);
        let mut also_on: Vec<Vec<String>> = Vec::new();
        for q in &shown {
            let p = schema.menu_path(q);
            if p != path && !p.is_empty() && !also_on.contains(&p) {
                also_on.push(p);
            }
        }
        let options: Vec<EnumOption> = primary
            .options
            .iter()
            .map(|o| EnumOption {
                symbol: o.value.to_string(),
                raw: o.value,
                label: o.label.trim().to_string(),
            })
            .collect();
        let as_i64 = |v: Option<u64>| v.and_then(|x| i64::try_from(x).ok());
        out.push(DecodedSetting {
            setting_id: bios_setting_id(store, *offset),
            group: "bios".into(),
            label: primary.prompt.trim().to_string(),
            summary: String::new(),
            available: true,
            value: Some(value.clone()),
            activity: Activity::Unknown,
            activity_reason: "Applicability not established: the BIOS's own show/hide conditions \
                              are not evaluated."
                .into(),
            editability: Editability {
                writable: false,
                auto_only: false,
                reason: "Not a reviewed setting: shown from the BIOS's own definition, read only."
                    .into(),
            },
            constraints: Some(Constraints {
                unit: None,
                min: as_i64(primary.min),
                max: as_i64(primary.max),
                step: as_i64(primary.step),
                auto_allowed: false,
                options,
            }),
            fields: codec::evidence(session, std::slice::from_ref(&rf)),
            evidence: vec![
                "Decoded from the BIOS image's own setup definitions (IFR); not reviewed.".into(),
            ],
            menu: Some(MenuLocation {
                path,
                section: primary.section.clone(),
                prompt: primary.prompt.trim().to_string(),
                order: schema.menu_order(primary),
                also_on,
            }),
            bios_help: Some(primary.help.trim().to_string()).filter(|h| !h.is_empty()),
            differs_from_default: default_value.as_ref().map(|d| !d.same_intent(&value)),
            default_value,
        });
    }
    out
}

pub fn inspect(session: &Session, include_raw: bool) -> Report<InspectPayload> {
    let settings = decoded_settings(session);
    let mut diags = session.diagnostics.clone();
    for s in &settings {
        if !s.available {
            diags.push(
                Diagnostic::new(
                    "setting_absent",
                    Severity::Info,
                    "decode",
                    format!("{} is not present in this BIOS", s.label),
                )
                .setting(&s.setting_id),
            );
        }
        if matches!(s.value, Some(Value::Unknown { .. })) {
            diags.push(
                Diagnostic::new(
                    "unknown_raw_value",
                    Severity::Warning,
                    "decode",
                    format!("{} stores a value no option describes", s.label),
                )
                .setting(&s.setting_id),
            );
        }
    }
    let payload = InspectPayload {
        bios: session.bios_summary(),
        profile: session.profile_summary(),
        binding: session.binding(),
        capabilities: session.capabilities(),
        groups: GROUPS
            .iter()
            .map(|(id, l)| GroupInfo {
                id: (*id).into(),
                label: (*l).into(),
            })
            .collect(),
        settings,
        raw_fields: if include_raw {
            raw_fields(session)
        } else {
            vec![]
        },
        bios_settings: if include_raw {
            bios_settings(session)
        } else {
            vec![]
        },
        opaque_records: session.profile.records.len().saturating_sub(2),
    };
    let mut report = Report::new("inspect", payload, diags);
    report.payload_version = INSPECT_PAYLOAD_VERSION.into();
    report
}

/// Name the region a file offset belongs to.
pub fn region_name(session: &Session, offset: usize) -> String {
    let p = &session.profile;
    if offset < 5 {
        return "magic".into();
    }
    if offset < CHECKSUM_AT {
        return "header.bios_id".into();
    }
    if offset < COUNT_AT {
        return "header.checksum".into();
    }
    if offset < RECORDS_AT {
        return "header.record_count".into();
    }
    if offset >= p.oci.offset {
        return "oci_metadata".into();
    }
    if offset >= p.cmos_offset {
        return "rtc_cmos_block".into();
    }
    for r in &p.records {
        if offset >= r.offset && offset < r.offset + r.size {
            let name = if r.offset == session.setup.offset {
                "Setup".to_string()
            } else if r.offset == session.cpu_setup.offset {
                "CpuSetup".to_string()
            } else {
                format!("record {}", r.id)
            };
            return format!("{name}+0x{:x}", offset - r.offset);
        }
        if offset + 6 >= r.offset && offset < r.offset {
            return format!("record {} header", r.id);
        }
    }
    "unclassified".into()
}

pub fn labelled_spans(session: &Session, offsets: &[usize]) -> Vec<ByteSpan> {
    spans(offsets)
        .into_iter()
        .map(|(start, end)| ByteSpan {
            start,
            end,
            region: region_name(session, start),
        })
        .collect()
}

/// Matching key for comparing settings across two profiles, possibly on different releases:
/// reviewed settings by recipe ID; others by store, menu page and prompt (offsets can move
/// between releases), numbered when one page repeats a prompt.
fn compare_keys(list: &[DecodedSetting]) -> Vec<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    list.iter()
        .map(|s| {
            if is_reviewed(s) {
                return s.setting_id.clone();
            }
            let store = s.fields.first().map(|f| f.store.as_str()).unwrap_or("");
            let path = s
                .menu
                .as_ref()
                .map(|m| m.path.join(" > "))
                .unwrap_or_default();
            let base = format!("{store}|{path}|{}", s.label);
            let n = seen.entry(base.clone()).or_insert(0);
            *n += 1;
            if *n == 1 { base } else { format!("{base}#{n}") }
        })
        .collect()
}

/// Same setting value? Reviewed settings compare by meaning; BIOS-defined ones by what the BIOS
/// displays (option label or number), since raw meanings are not reviewed across releases.
fn same_value(reviewed: bool, a: &Option<Value>, b: &Option<Value>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) if reviewed => x.same_intent(y),
        (Some(x), Some(y)) => x.display() == y.display(),
        (None, None) => true,
        _ => false,
    }
}

/// Reviewed settings come from a recipe; generic ones (group "bios") straight from the IFR.
fn is_reviewed(s: &DecodedSetting) -> bool {
    s.group != "bios"
}

/// One compared setting, present on either or both sides.
fn compare_entry(
    key: &str,
    l: Option<&DecodedSetting>,
    r: Option<&DecodedSetting>,
) -> Option<CompareEntry> {
    let any = l.or(r)?;
    let reviewed = is_reviewed(any);
    let (lv, rv) = (
        l.and_then(|x| x.value.clone()),
        r.and_then(|x| x.value.clone()),
    );
    if l.is_some() && r.is_some() && same_value(reviewed, &lv, &rv) {
        return None;
    }
    Some(CompareEntry {
        key: key.to_string(),
        label: any
            .menu
            .as_ref()
            .map_or_else(|| any.label.clone(), |m| m.prompt.clone()),
        reviewed,
        menu: l
            .and_then(|x| x.menu.clone())
            .or_else(|| r.and_then(|x| x.menu.clone())),
        left: lv,
        right: rv,
        left_default: l.and_then(|x| x.default_value.clone()),
        right_default: r.and_then(|x| x.default_value.clone()),
        only: match (l, r) {
            (Some(_), None) => Some("left".into()),
            (None, Some(_)) => Some("right".into()),
            _ => None,
        },
    })
}

/// Every setting of two profiles, reviewed or not, matched by name: the ones that differ or
/// exist on one side only, and how many were compared. Read-only.
pub fn compare_all(left: &Session, right: &Session) -> (Vec<CompareEntry>, usize) {
    let side = |s: &Session| {
        let mut v = decoded_settings(s);
        v.retain(|x| x.available);
        v.extend(bios_settings(s));
        v
    };
    let (l, r) = (side(left), side(right));
    let (lk, rk) = (compare_keys(&l), compare_keys(&r));
    let lmap: BTreeMap<&str, &DecodedSetting> = lk.iter().map(String::as_str).zip(&l).collect();
    let rmap: BTreeMap<&str, &DecodedSetting> = rk.iter().map(String::as_str).zip(&r).collect();
    let right_only = rk.iter().filter(|k| !lmap.contains_key(k.as_str()));
    let compared = lk.len() + right_only.clone().count();
    let out = lk
        .iter()
        .chain(right_only)
        .filter_map(|k| {
            compare_entry(
                k,
                lmap.get(k.as_str()).copied(),
                rmap.get(k.as_str()).copied(),
            )
        })
        .collect();
    (out, compared)
}

pub fn diff(left: &Session, right: &Session) -> Report<DiffPayload> {
    diff_with(left, right, false)
}

/// `diff`, optionally with the full comparison of every setting (`all_differences`).
pub fn diff_with(left: &Session, right: &Session, all: bool) -> Report<DiffPayload> {
    let l = decode_all(left);
    let r = decode_all(right);
    let settings = l
        .iter()
        .zip(&r)
        .map(|((rec, ls), (_, rs))| {
            let lv = match ls {
                RecipeState::Decoded(d) => Some(d.value.clone()),
                _ => None,
            };
            let rv = match rs {
                RecipeState::Decoded(d) => Some(d.value.clone()),
                _ => None,
            };
            let equal = match (&lv, &rv) {
                (Some(a), Some(b)) => a.same_intent(b),
                (None, None) => true,
                _ => false,
            };
            SettingDiff {
                setting_id: rec.id.into(),
                label: rec.label.into(),
                left: lv,
                right: rv,
                equal,
            }
        })
        .collect();

    // Other named fields that exist at the same store/offset/prompt in both schemas.
    let recipe_locs: std::collections::BTreeSet<(String, u16)> = l
        .iter()
        .chain(&r)
        .filter_map(|(_, s)| match s {
            RecipeState::Decoded(d) => Some(
                d.fields
                    .iter()
                    .map(|f| (f.store.to_string(), f.offset))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect();
    let rf: BTreeMap<(String, u16, String), u64> = raw_fields(right)
        .into_iter()
        .map(|f| ((f.store, f.offset, f.prompt), f.raw))
        .collect();
    let mut other = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for f in raw_fields(left) {
        if recipe_locs.contains(&(f.store.clone(), f.offset))
            || !DECODED_STORES.contains(&f.store.as_str())
        {
            continue;
        }
        if let Some(rv) = rf.get(&(f.store.clone(), f.offset, f.prompt.clone()))
            && *rv != f.raw
            && seen.insert((f.store.clone(), f.offset))
        {
            other.push(RawFieldDiff {
                store: f.store,
                offset: f.offset,
                prompt: f.prompt,
                left: f.raw,
                right: *rv,
            });
        }
    }

    let offsets = diff_offsets(left.profile.bytes(), right.profile.bytes());
    let (all_differences, compared) = if all {
        compare_all(left, right)
    } else {
        (vec![], 0)
    };
    let payload = DiffPayload {
        left: left.binding(),
        right: right.binding(),
        same_bios: left.schema.sha256 == right.schema.sha256,
        settings,
        other_field_differences: other,
        byte_spans: labelled_spans(left, &offsets),
        all_differences,
        compared,
    };
    Report::new("diff", payload, vec![])
}
