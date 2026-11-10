//! Pairing, decoding, validation, editing, migration and verification (SPEC §6–§8).
//!
//! Every client (CLI, desktop) calls these functions; there is no other decision path.

pub mod browse;
pub mod codec;
pub mod edit;
pub mod experimental;
pub mod inspect;
pub mod migrate;
pub mod session;
pub mod verify;

use crate::catalog::{Codec, RECIPES, Recipe, Writability};
use crate::contracts::{Activity, Value};
use codec::{Resolution, ResolvedField};
pub use session::Session;

/// One recipe decoded against one session.
#[derive(Debug, Clone)]
pub enum RecipeState {
    Missing,
    Ambiguous(String),
    Decoded(Decoded),
}

#[derive(Debug, Clone)]
pub struct Decoded {
    pub fields: Vec<ResolvedField>,
    /// (file offset, raw) per field.
    pub raws: Vec<(usize, u64)>,
    pub value: Value,
}

impl Decoded {
    pub fn raw_values(&self) -> Vec<u64> {
        self.raws.iter().map(|r| r.1).collect()
    }

    /// File byte spans covered by this recipe's fields.
    pub fn spans(&self) -> Vec<(usize, usize)> {
        self.fields
            .iter()
            .zip(&self.raws)
            .map(|(f, (at, _))| (*at, at + f.width))
            .collect()
    }
}

pub fn decode_recipe(session: &Session, recipe: &Recipe) -> RecipeState {
    match codec::resolve(&session.schema, recipe) {
        Resolution::Missing => RecipeState::Missing,
        Resolution::Ambiguous(e) => RecipeState::Ambiguous(e),
        Resolution::Found(fields) => {
            let mut raws = Vec::new();
            for f in &fields {
                match codec::read_raw(session, f) {
                    Some(r) => raws.push(r),
                    None => {
                        return RecipeState::Ambiguous(format!(
                            "{} offset {} lies outside its record",
                            f.store, f.offset
                        ));
                    }
                }
            }
            let values: Vec<u64> = raws.iter().map(|r| r.1).collect();
            let value = codec::decode(&recipe.codec, &fields, &values);
            RecipeState::Decoded(Decoded {
                fields,
                raws,
                value,
            })
        }
    }
}

pub fn decode_all(session: &Session) -> Vec<(&'static Recipe, RecipeState)> {
    RECIPES
        .iter()
        .map(|r| (r, decode_recipe(session, r)))
        .collect()
}

fn raw_of(states: &[(&'static Recipe, RecipeState)], id: &str) -> Option<u64> {
    states
        .iter()
        .find(|(r, _)| r.id == id)
        .and_then(|(_, s)| match s {
            RecipeState::Decoded(d) => d.raws.first().map(|r| r.1),
            _ => None,
        })
}

fn value_of<'a>(states: &'a [(&'static Recipe, RecipeState)], id: &str) -> Option<&'a Value> {
    states
        .iter()
        .find(|(r, _)| r.id == id)
        .and_then(|(_, s)| match s {
            RecipeState::Decoded(d) => Some(&d.value),
            _ => None,
        })
}

/// Reviewed dependency rules: (dependent setting, controlling setting, allowed controller raws, explanation).
const DEPENDENCIES: &[(&str, &str, &[u64], &str)] = &[
    (
        "cpu.pcore.turbo_ratio_offset",
        "cpu.pcore.ratio_apply_mode",
        &[4],
        "requires P-core ratio apply mode = Turbo Ratio Offset",
    ),
    (
        "cpu.ecore.turbo_ratio_offset",
        "cpu.ecore.ratio_apply_mode",
        &[4],
        "requires E-core ratio apply mode = Turbo Ratio Offset",
    ),
    (
        "cpu.core.voltage_offset",
        "cpu.core.voltage_mode",
        &[3, 4],
        "requires core voltage mode = Offset or Adaptive + Offset",
    ),
];

/// Activity of a setting from reviewed dependency rules. IFR conditions are not evaluated.
pub fn activity(states: &[(&'static Recipe, RecipeState)], id: &str) -> (Activity, String) {
    for (dep, ctl, allowed, why) in DEPENDENCIES {
        if *dep == id {
            return match raw_of(states, ctl) {
                Some(raw) if allowed.contains(&raw) => {
                    (Activity::Active, format!("Active: {why}."))
                }
                Some(_) => (Activity::Inactive, format!("Stored but inactive: {why}.")),
                None => (Activity::Unknown, format!("Controller {ctl} unavailable.")),
            };
        }
    }
    (
        Activity::Unknown,
        "Applicability not established: IFR visibility conditions are not evaluated and these are firmware callback questions.".into(),
    )
}

/// Final-state rule violations: a non-Auto dependent value whose controller does not allow it.
pub fn rule_violations(states: &[(&'static Recipe, RecipeState)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (dep, ctl, allowed, why) in DEPENDENCIES {
        let Some(v) = value_of(states, dep) else {
            continue;
        };
        if matches!(v, Value::Auto) {
            continue;
        }
        match raw_of(states, ctl) {
            Some(raw) if allowed.contains(&raw) => {}
            _ => out.push((dep.to_string(), format!("{} = {} {why}", dep, v.display()))),
        }
    }
    out
}

pub fn writability_reason(
    session: &Session,
    recipe: &Recipe,
    state: &RecipeState,
) -> (bool, bool, String) {
    if !session.approved() {
        return (false, false, sentence(&session.blocked_reason()));
    }
    match state {
        RecipeState::Missing => (false, false, "Setting does not exist in this BIOS.".into()),
        RecipeState::Ambiguous(e) => (false, false, format!("Ambiguous location: {e}")),
        RecipeState::Decoded(_) => match recipe.writability {
            Writability::Writable => (
                true,
                false,
                "Reviewed recipe for this approved image.".into(),
            ),
            Writability::AutoOnly(why) => (true, true, format!("Only Auto can be written: {why}.")),
            Writability::ReadOnly(why) => (false, false, format!("Inspect-only: {why}.")),
        },
    }
}

/// Capitalise and end with a full stop, for UI reasons.
fn sentence(s: &str) -> String {
    let mut c = s.chars();
    let head: String = c
        .next()
        .map(|f| f.to_uppercase().collect())
        .unwrap_or_default();
    format!("{head}{}.", c.as_str().trim_end_matches('.'))
}

pub fn is_compound(recipe: &Recipe) -> bool {
    matches!(recipe.codec, Codec::SignedOffset { .. })
}
