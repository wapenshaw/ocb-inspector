//! Recipe field resolution, decoding, encoding and cross-release translation.

use super::session::Session;
use crate::bytes::uint_at;
use crate::catalog::{Codec, FieldRef, Recipe, Writability};
use crate::contracts::{Constraints, EnumOption, FieldEvidence, Value};
use crate::firmware::ifr::{Question, QuestionKind};
use crate::firmware::{BiosSchema, DECODED_STORES};
use std::collections::BTreeMap;

/// A recipe field located in one schema.
#[derive(Debug, Clone)]
pub struct ResolvedField {
    pub store: &'static str,
    pub offset: u16,
    pub width: usize,
    /// raw -> label from the highest-priority anchor prompt present.
    pub options: BTreeMap<u64, String>,
    pub primary_kind: QuestionKind,
    pub min: Option<u64>,
    pub max: Option<u64>,
    pub step: Option<u64>,
    pub questions: Vec<Question>,
    /// Raw values that carry different labels under different (conditional) options.
    pub label_conflicts: Vec<u64>,
}

pub enum Resolution {
    Missing,
    Ambiguous(String),
    Found(Vec<ResolvedField>),
}

pub fn resolve_field(schema: &BiosSchema, f: &FieldRef) -> Result<Option<ResolvedField>, String> {
    let mut chosen: Option<(&str, Vec<&Question>)> = None;
    for prompt in f.prompts {
        let qs: Vec<&Question> = schema
            .questions
            .iter()
            .filter(|q| q.store == f.store && q.prompt == *prompt)
            .collect();
        if !qs.is_empty() {
            chosen = Some((prompt, qs));
            break;
        }
    }
    let Some((prompt, qs)) = chosen else {
        return Ok(None);
    };
    let mut locs: Vec<(u16, u16)> = qs.iter().map(|q| (q.offset, q.width_bits)).collect();
    locs.sort();
    locs.dedup();
    let [(offset, width_bits)] = locs.as_slice() else {
        return Err(format!(
            "'{prompt}' appears at {} different locations in {}",
            locs.len(),
            f.store
        ));
    };
    if *width_bits != f.width_bits && !(f.any_width && matches!(*width_bits, 8 | 16)) {
        return Err(format!(
            "'{prompt}' is {width_bits} bits; recipe expects {}",
            f.width_bits
        ));
    }
    let mut options: BTreeMap<u64, String> = BTreeMap::new();
    let mut label_conflicts = Vec::new();
    for q in &qs {
        for o in &q.options {
            let label = o.label.trim().to_string();
            match options.get(&o.value) {
                Some(prev) if *prev != label => label_conflicts.push(o.value),
                Some(_) => {}
                None => {
                    options.insert(o.value, label);
                }
            }
        }
    }
    label_conflicts.sort();
    label_conflicts.dedup();
    for (raw, accepted) in f.reviewed {
        let ok = options
            .get(raw)
            .is_some_and(|l| accepted.iter().any(|a| a.eq_ignore_ascii_case(l.trim())));
        if !ok {
            return Err(format!(
                "'{prompt}' raw {raw} is {} here, but the reviewed releases use it for {}; option meanings differ",
                options
                    .get(raw)
                    .map_or_else(|| "not an option".to_string(), |l| format!("'{l}'")),
                accepted.join(" / ")
            ));
        }
    }
    let first = qs[0];
    let questions: Vec<Question> = schema
        .questions
        .iter()
        .filter(|q| q.store == f.store && q.offset == *offset)
        .cloned()
        .collect();
    Ok(Some(ResolvedField {
        store: f.store,
        offset: *offset,
        width: usize::from(*width_bits / 8),
        options,
        primary_kind: first.kind,
        min: first.min,
        max: first.max,
        step: first.step,
        questions,
        label_conflicts,
    }))
}

pub fn resolve(schema: &BiosSchema, recipe: &Recipe) -> Resolution {
    let mut out = Vec::new();
    for f in recipe.codec.fields() {
        match resolve_field(schema, &f) {
            // Enum labels drive meaning, so conditional label conflicts make an enum ambiguous.
            // The signed-offset sign codec uses reviewed raw meanings (0/1/2) and is unaffected.
            Ok(Some(r))
                if matches!(recipe.codec, Codec::Enum { .. }) && !r.label_conflicts.is_empty() =>
            {
                return Resolution::Ambiguous(format!(
                    "conditional options give raw {:?} different labels",
                    r.label_conflicts
                ));
            }
            Ok(Some(r)) => out.push(r),
            Ok(None) => return Resolution::Missing,
            Err(e) => return Resolution::Ambiguous(e),
        }
    }
    Resolution::Found(out)
}

pub fn slug(label: &str) -> String {
    let mut s = String::new();
    for c in label.trim().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if c == '+' {
            s.push_str("plus");
        } else if c == '-' && s.is_empty() {
            s.push_str("minus");
        } else if !s.ends_with('_') && !s.is_empty() {
            s.push('_');
        }
    }
    s.trim_end_matches('_').to_string()
}

pub fn read_raw(session: &Session, f: &ResolvedField) -> Option<(usize, u64)> {
    let rec = session.record_for(f.store)?;
    let at = rec.offset + usize::from(f.offset);
    if usize::from(f.offset) + f.width > rec.size {
        return None;
    }
    uint_at(session.profile.bytes(), at, f.width, "field")
        .ok()
        .map(|v| (at, v))
}

pub fn decode(codec: &Codec, fields: &[ResolvedField], raws: &[u64]) -> Value {
    match codec {
        Codec::Enum { .. } => enum_value(&fields[0], raws[0]),
        Codec::AutoZero { unit, .. } => {
            if raws[0] == 0 {
                Value::Auto
            } else {
                Value::Integer {
                    value: raws[0] as i64,
                    unit: (*unit).into(),
                }
            }
        }
        Codec::RatioOffset { .. } => {
            if raws[0] == 0 {
                Value::Auto
            } else if fields[0].options.contains_key(&raws[0]) {
                Value::Integer {
                    value: i64::from(raws[0] as u8 as i8),
                    unit: "ratio".into(),
                }
            } else {
                Value::Unknown { raw: raws[0] }
            }
        }
        Codec::SignedOffset { unit, .. } => match raws[0] {
            0 => Value::Auto,
            1 => Value::Integer {
                value: raws[1] as i64,
                unit: (*unit).into(),
            },
            2 => Value::Integer {
                value: -(raws[1] as i64),
                unit: (*unit).into(),
            },
            other => Value::Unknown { raw: other },
        },
    }
}

fn enum_value(f: &ResolvedField, raw: u64) -> Value {
    match f.options.get(&raw) {
        Some(label) => Value::Enum {
            symbol: slug(label),
            raw: Some(raw),
            label: Some(label.clone()),
        },
        None => Value::Unknown { raw },
    }
}

pub fn constraints(recipe: &Recipe, fields: &[ResolvedField]) -> Constraints {
    let auto_only = matches!(recipe.writability, Writability::AutoOnly(_));
    match recipe.codec {
        Codec::Enum { .. } => Constraints {
            unit: None,
            min: None,
            max: None,
            step: None,
            auto_allowed: false,
            options: fields[0]
                .options
                .iter()
                .map(|(raw, label)| EnumOption {
                    symbol: slug(label),
                    raw: *raw,
                    label: label.clone(),
                })
                .collect(),
        },
        Codec::AutoZero { unit, .. } => {
            let f = &fields[0];
            Constraints {
                unit: Some(unit.into()),
                min: if auto_only {
                    None
                } else {
                    f.min.map(|v| v.max(1) as i64)
                },
                max: if auto_only {
                    None
                } else {
                    f.max.map(|v| v as i64)
                },
                step: if auto_only {
                    None
                } else {
                    f.step.map(|v| v.max(1) as i64)
                },
                auto_allowed: true,
                options: vec![],
            }
        }
        Codec::RatioOffset { .. } => {
            let vals: Vec<i64> = fields[0]
                .options
                .keys()
                .filter(|&&r| r != 0)
                .map(|&r| i64::from(r as u8 as i8))
                .collect();
            Constraints {
                unit: Some("ratio".into()),
                min: vals.iter().min().copied(),
                max: vals.iter().max().copied(),
                step: Some(1),
                auto_allowed: true,
                options: vec![],
            }
        }
        Codec::SignedOffset { unit, .. } => {
            let m = &fields[1];
            let max = m.max.map(|v| v as i64);
            Constraints {
                unit: Some(unit.into()),
                min: max.map(|v| -v),
                max,
                step: m.step.map(|v| v.max(1) as i64),
                auto_allowed: true,
                options: vec![],
            }
        }
    }
}

/// Encode a value for a resolved recipe. Returns per-field raw values, or why it cannot be written.
/// `baseline_raws` supplies values for fields a value leaves unchanged (e.g. inactive magnitude).
pub fn encode(
    recipe: &Recipe,
    fields: &[ResolvedField],
    value: &Value,
    baseline_raws: &[u64],
) -> Result<Vec<u64>, String> {
    if let Writability::ReadOnly(why) = recipe.writability {
        return Err(format!("read-only: {why}"));
    }
    if let (Writability::AutoOnly(why), false) = (recipe.writability, matches!(value, Value::Auto))
    {
        return Err(format!("only Auto can be written: {why}"));
    }
    match (recipe.codec, value) {
        (Codec::Enum { .. }, Value::Enum { symbol, raw, .. }) => {
            let f = &fields[0];
            let hit: Vec<u64> = f
                .options
                .iter()
                .filter(|(_, l)| slug(l) == *symbol)
                .map(|(r, _)| *r)
                .collect();
            match (hit.as_slice(), raw) {
                ([r], None) => Ok(vec![*r]),
                ([r], Some(want)) if r == want => Ok(vec![*r]),
                ([_], Some(want)) => Err(format!(
                    "symbol '{symbol}' does not have raw value {want} in this BIOS"
                )),
                ([], _) => Err(format!("'{symbol}' is not an option in this BIOS")),
                _ => Err(format!("'{symbol}' matches several options; ambiguous")),
            }
        }
        (Codec::AutoZero { .. }, Value::Auto) | (Codec::RatioOffset { .. }, Value::Auto) => {
            Ok(vec![0])
        }
        (Codec::AutoZero { unit, .. }, Value::Integer { value: v, unit: u }) => {
            if u != unit {
                return Err(format!("unit must be {unit}, got {u}"));
            }
            let f = &fields[0];
            if *v <= 0 {
                return Err("use Auto instead of zero or a negative value".into());
            }
            check_numeric(f, *v as u64)?;
            Ok(vec![*v as u64])
        }
        (Codec::RatioOffset { .. }, Value::Integer { value: v, unit: u }) => {
            if u != "ratio" {
                return Err(format!("unit must be ratio, got {u}"));
            }
            if *v == 0 || *v < -128 || *v > 127 {
                return Err("ratio offset must be a non-zero option; use Auto for 0".into());
            }
            let raw = u64::from(*v as i8 as u8);
            if !fields[0].options.contains_key(&raw) {
                return Err(format!("{v:+} is not an offered ratio offset"));
            }
            Ok(vec![raw])
        }
        (Codec::SignedOffset { .. }, Value::Auto) => {
            Ok(vec![0, baseline_raws.get(1).copied().unwrap_or(0)])
        }
        (Codec::SignedOffset { unit, .. }, Value::Integer { value: v, unit: u }) => {
            if u != unit {
                return Err(format!("unit must be {unit}, got {u}"));
            }
            if *v == 0 {
                return Err("use Auto instead of a zero offset".into());
            }
            let mag = v.unsigned_abs();
            check_numeric(&fields[1], mag)?;
            let sign = if *v > 0 { 1 } else { 2 };
            if !fields[0].options.contains_key(&sign) {
                return Err("offset sign option missing in this BIOS".into());
            }
            Ok(vec![sign, mag])
        }
        (_, Value::Unknown { raw }) => Err(format!("unknown raw value {raw} cannot be written")),
        _ => Err(format!(
            "value kind does not match this setting's codec: {}",
            value.display()
        )),
    }
}

fn check_numeric(f: &ResolvedField, v: u64) -> Result<(), String> {
    if f.primary_kind != QuestionKind::Numeric {
        return Err("field is not numeric".into());
    }
    if let Some(min) = f.min
        && v < min
    {
        return Err(format!("{v} is below the BIOS minimum {min}"));
    }
    if let Some(max) = f.max
        && v > max
    {
        return Err(format!("{v} is above the BIOS maximum {max}"));
    }
    if let (Some(step), Some(min)) = (f.step, f.min)
        && step > 1
        && !(v - min).is_multiple_of(step)
    {
        return Err(format!("{v} is not on the BIOS step {step}"));
    }
    let limit = if f.width >= 8 {
        u64::MAX
    } else {
        (1u64 << (f.width * 8)) - 1
    };
    if v > limit {
        return Err(format!("{v} does not fit {} bytes", f.width));
    }
    Ok(())
}

/// Translate a source value into the target release's representation, without guessing.
pub fn translate(
    recipe: &Recipe,
    src: &[ResolvedField],
    tgt: &[ResolvedField],
    value: &Value,
) -> Result<Value, String> {
    match (recipe.codec, value) {
        (_, Value::Unknown { raw }) => {
            Err(format!("source raw value {raw} has no reviewed meaning"))
        }
        (
            Codec::Enum { cross_map, .. },
            Value::Enum {
                raw: Some(r),
                label,
                ..
            },
        ) => {
            let target_raw = match cross_map {
                Some(map) => map
                    .iter()
                    .find(|(s, _)| s == r)
                    .map(|(_, t)| *t)
                    .ok_or_else(|| {
                        format!(
                            "raw {r} ({}) has no reviewed equivalent in the target",
                            label.clone().unwrap_or_default()
                        )
                    })?,
                None => {
                    let src_label = src[0].options.get(r).cloned().unwrap_or_default();
                    match tgt[0].options.get(r) {
                        Some(l) if *l == src_label => *r,
                        Some(l) => {
                            return Err(format!(
                                "raw {r} means '{src_label}' in the source but '{l}' in the target"
                            ));
                        }
                        None => {
                            return Err(format!(
                                "'{src_label}' (raw {r}) does not exist in the target BIOS"
                            ));
                        }
                    }
                }
            };
            let label = tgt[0]
                .options
                .get(&target_raw)
                .cloned()
                .ok_or_else(|| format!("target has no raw {target_raw}"))?;
            Ok(Value::Enum {
                symbol: slug(&label),
                raw: Some(target_raw),
                label: Some(label),
            })
        }
        (Codec::Enum { .. }, _) => Err("enum value without raw representation".into()),
        // Numeric codecs: same unit, re-checked against the target's constraints by `encode`.
        _ => Ok(value.clone()),
    }
}

pub fn evidence(session: &Session, fields: &[ResolvedField]) -> Vec<FieldEvidence> {
    fields
        .iter()
        .map(|f| {
            let (file_offset, raw) = read_raw(session, f).unwrap_or((0, 0));
            let bytes = session
                .profile
                .bytes()
                .get(file_offset..file_offset + f.width)
                .unwrap_or(&[]);
            let mut prompts: Vec<String> = f
                .questions
                .iter()
                .map(|q| q.prompt.clone())
                .filter(|p| !p.is_empty())
                .collect();
            prompts.dedup();
            let schema_qids: std::collections::BTreeSet<u16> = session
                .schema
                .questions
                .iter()
                .filter(|q| DECODED_STORES.contains(&q.store.as_str()))
                .map(|q| q.question_id)
                .collect();
            let conds: Vec<_> = f
                .questions
                .iter()
                .flat_map(|q| q.conditions.iter())
                .collect();
            FieldEvidence {
                store: f.store.into(),
                store_offset: f.offset,
                file_offset,
                width_bits: (f.width * 8) as u16,
                raw,
                bytes_hex: bytes
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(" "),
                prompts,
                question_ids: f.questions.iter().map(|q| q.question_id).collect(),
                callback: f.questions.iter().any(|q| q.is_callback()),
                reset_required: f.questions.iter().any(|q| q.flags & 0x10 != 0),
                condition_count: conds.len(),
                conditions_reference_undecoded_stores: conds
                    .iter()
                    .any(|c| c.question_refs.iter().any(|r| !schema_qids.contains(r))),
            }
        })
        .collect()
}
