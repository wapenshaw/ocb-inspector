//! Independent re-verification of generated output and standalone profile verification (SPEC §8 stages 9–10).

use super::{RecipeState, Session, decode_all, rule_violations};
use crate::contracts::*;
use crate::format::{CHECKSUM_AT, COUNT_AT, diff_offsets};

/// A change the build intended to make.
#[derive(Debug, Clone)]
pub struct Intended {
    pub setting_id: String,
    pub value: Value,
    pub spans: Vec<(usize, usize)>,
}

fn check(name: &str, ok: bool, detail: impl Into<String>) -> Check {
    Check {
        name: name.into(),
        status: if ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        detail: detail.into(),
    }
}

/// Re-parse `output` from bytes, pair it with the baseline's BIOS and compare against intent.
pub fn verify_output(baseline: &Session, output: &[u8], intended: &[Intended]) -> Vec<Check> {
    let mut checks = Vec::new();
    let out = match Session::pair(baseline.schema.clone(), output.to_vec()) {
        Ok(s) => {
            checks.push(check(
                "reparse_and_pair",
                true,
                format!(
                    "Output parses and pairs with {}",
                    baseline.schema.identity.bios_id
                ),
            ));
            s
        }
        Err(e) => {
            checks.push(check("reparse_and_pair", false, e.to_string()));
            return checks;
        }
    };
    let p = &out.profile;
    checks.push(check(
        "checksum",
        p.checksum_valid(),
        format!(
            "stored {} / computed {}",
            p.stored_checksum,
            p.computed_checksum().unwrap_or_default()
        ),
    ));
    let base = &baseline.profile;
    let layout_same =
        p.records == base.records && p.records_end == base.records_end && p.oci == base.oci;
    checks.push(check(
        "layout_preserved",
        layout_same,
        "Record ids, sizes and offsets and $OCI$ metadata match the baseline",
    ));
    let tail_same = output.get(p.cmos_offset..) == base.bytes().get(base.cmos_offset..);
    checks.push(check(
        "trailer_preserved",
        tail_same,
        "RTC/CMOS-like block and $OCI$ bytes are byte-identical to the baseline",
    ));

    let mut allowed: Vec<(usize, usize)> = vec![(CHECKSUM_AT, COUNT_AT)];
    for i in intended {
        allowed.extend(i.spans.iter().copied());
    }
    let diffs = diff_offsets(base.bytes(), output);
    let stray: Vec<usize> = diffs
        .iter()
        .copied()
        .filter(|o| !allowed.iter().any(|(s, e)| o >= s && o < e))
        .collect();
    checks.push(check(
        "mutation_audit",
        stray.is_empty(),
        if stray.is_empty() {
            format!(
                "{} changed byte(s), all inside {} allowed span(s)",
                diffs.len(),
                allowed.len()
            )
        } else {
            format!(
                "{} byte(s) changed outside allowed spans, first at 0x{:x}",
                stray.len(),
                stray[0]
            )
        },
    ));

    let out_states = decode_all(&out);
    let base_states = decode_all(baseline);
    let mut bad = Vec::new();
    for i in intended {
        let got = out_states
            .iter()
            .find(|(r, _)| r.id == i.setting_id)
            .and_then(|(_, s)| match s {
                RecipeState::Decoded(d) => Some(d.value.clone()),
                _ => None,
            });
        if !got.as_ref().is_some_and(|g| g.same_intent(&i.value)) {
            bad.push(format!(
                "{}: wanted {}, decoded {}",
                i.setting_id,
                i.value.display(),
                got.map(|g| g.display()).unwrap_or("nothing".into())
            ));
        }
    }
    checks.push(check(
        "intended_values",
        bad.is_empty(),
        if bad.is_empty() {
            format!(
                "{} setting(s) decode to the intended values",
                intended.len()
            )
        } else {
            bad.join("; ")
        },
    ));
    let mut drift = Vec::new();
    for ((r, o), (_, b)) in out_states.iter().zip(&base_states) {
        if intended.iter().any(|i| i.setting_id == r.id) {
            continue;
        }
        if let (RecipeState::Decoded(o), RecipeState::Decoded(b)) = (o, b)
            && !o.value.same_intent(&b.value)
        {
            drift.push(r.id);
        }
    }
    checks.push(check(
        "retained_values",
        drift.is_empty(),
        if drift.is_empty() {
            "Every other reviewed setting equals the baseline".to_string()
        } else {
            format!("changed unexpectedly: {}", drift.join(", "))
        },
    ));
    let v = rule_violations(&out_states);
    checks.push(check(
        "dependency_rules",
        v.is_empty(),
        if v.is_empty() {
            "Compound and dependency rules hold".to_string()
        } else {
            v.into_iter().map(|x| x.1).collect::<Vec<_>>().join("; ")
        },
    ));
    checks.push(Check {
        name: "import_acceptance".into(),
        status: CheckStatus::NotEvaluated,
        detail:
            "Requires a user-operated import on matching firmware; never performed automatically"
                .into(),
    });
    checks
}

pub fn all_pass(checks: &[Check]) -> bool {
    checks.iter().all(|c| c.status != CheckStatus::Fail)
}

/// Standalone verification of a BIOS/profile pair (`ocbi profile verify`).
pub fn verify_profile(session: &Session) -> Report<VerifyPayload> {
    let p = &session.profile;
    let mut checks = vec![
        check(
            "pair_identity",
            true,
            format!(
                "Profile and image are both {}",
                session.schema.identity.bios_id
            ),
        ),
        check(
            "checksum",
            p.checksum_valid(),
            format!(
                "stored {} / computed {}",
                p.stored_checksum,
                p.computed_checksum().unwrap_or_default()
            ),
        ),
        check(
            "store_binding",
            true,
            format!(
                "Setup @0x{:x} ({} B), CpuSetup @0x{:x} ({} B)",
                session.setup.offset,
                session.setup.size,
                session.cpu_setup.offset,
                session.cpu_setup.size
            ),
        ),
        check(
            "lossless_roundtrip",
            crate::format::Mutation::from_baseline(p)
                .finish()
                .map(|f| f.bytes() == p.bytes())
                .unwrap_or(false),
            "Re-serializing with a recomputed checksum reproduces the input byte-for-byte",
        ),
    ];
    let states = decode_all(session);
    let undecodable: Vec<&str> = states
        .iter()
        .filter_map(|(r, s)| match s {
            RecipeState::Ambiguous(_) => Some(r.id),
            RecipeState::Decoded(d) if matches!(d.value, Value::Unknown { .. }) => Some(r.id),
            _ => None,
        })
        .collect();
    checks.push(check(
        "reviewed_settings_decode",
        undecodable.is_empty(),
        if undecodable.is_empty() {
            "All reviewed settings decode to known values".into()
        } else {
            format!("undecodable: {}", undecodable.join(", "))
        },
    ));
    let v = rule_violations(&states);
    checks.push(check(
        "dependency_rules",
        v.is_empty(),
        if v.is_empty() {
            "Compound and dependency rules hold".to_string()
        } else {
            v.into_iter().map(|x| x.1).collect::<Vec<_>>().join("; ")
        },
    ));
    checks.push(Check {
        name: "import_acceptance".into(),
        status: CheckStatus::NotEvaluated,
        detail: "Not observable offline".into(),
    });
    let status = if all_pass(&checks) {
        "valid"
    } else {
        "invalid"
    };
    Report::new(
        "verify",
        VerifyPayload {
            bios: session.bios_summary(),
            profile: session.profile_summary(),
            checks,
            status: status.into(),
        },
        session.diagnostics.clone(),
    )
}
