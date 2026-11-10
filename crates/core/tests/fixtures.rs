//! Regression tests against the private fixture corpus (fixtures/private, git-ignored).
//! Each test is skipped with a message when the corpus is not provisioned.

use ocb_core::contracts::*;
use ocb_core::engine::{RecipeState, Session, decode_recipe, edit, inspect, migrate, verify};
use ocb_core::error::Error;
use ocb_core::{catalog, firmware, io};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

fn root() -> PathBuf {
    // Writes need the reviewed approvals file shipped in the repo; install it before any check.
    static APPROVALS: OnceLock<()> = OnceLock::new();
    APPROVALS.get_or_init(|| {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/approved-images.toml");
        ocb_core::approvals::install_from(&p);
        // Writing is experimental and off by default; these tests exercise it.
        ocb_core::settings::install(ocb_core::settings::Settings {
            experimental: ocb_core::settings::Experimental { writing: true },
            ..Default::default()
        });
    });
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/private")
}

fn have_fixtures() -> bool {
    let ok = root().join("bios/E7D91IMS.HJ0").exists();
    if !ok {
        eprintln!("skipping: private fixture corpus not provisioned");
    }
    ok
}

fn schema(rev: &str) -> Arc<firmware::BiosSchema> {
    static HI0: OnceLock<Arc<firmware::BiosSchema>> = OnceLock::new();
    static HJ0: OnceLock<Arc<firmware::BiosSchema>> = OnceLock::new();
    let cell = if rev == "HI0" { &HI0 } else { &HJ0 };
    cell.get_or_init(|| {
        Arc::new(io::load_bios(&root().join(format!("bios/E7D91IMS.{rev}"))).expect("parse bios"))
    })
    .clone()
}

fn session(rev: &str, profile: &str) -> Session {
    let bytes = std::fs::read(root().join(format!("profiles/{profile}"))).expect("read profile");
    Session::pair(schema(rev), bytes).expect("pair")
}

fn value(s: &Session, id: &str) -> Value {
    match decode_recipe(s, catalog::recipe(id).expect("recipe")) {
        RecipeState::Decoded(d) => d.value,
        other => panic!("{id}: {other:?}"),
    }
}

fn int(v: i64, unit: &str) -> Value {
    Value::Integer {
        value: v,
        unit: unit.into(),
    }
}

#[test]
fn bios_identity_and_schema() {
    if !have_fixtures() {
        return;
    }
    for rev in ["HI0", "HJ0"] {
        let s = schema(rev);
        assert_eq!(s.identity.bios_id, format!("E7D91IMS.{rev}"));
        assert_eq!(s.identity.board, "7D91");
        assert_eq!(s.store("Setup").map(|v| v.size), Some(5081));
        assert_eq!(s.store("CpuSetup").map(|v| v.size), Some(961));
        assert!(ocb_core::approvals::active().is_approved(
            &s.identity.bios_id,
            &s.sha256,
            &s.identity.board
        ));
        // Callback + reset-required flags on the tuning controls (SPEC §5 evidence).
        let q = s
            .questions
            .iter()
            .find(|q| q.prompt == "CPU Core Voltage Mode")
            .expect("question");
        assert_eq!(q.flags, 0x14);
    }
    // Unique (store, offset, width, prompt) counts reproduce the exploratory extractor.
    let unique = |rev| {
        let s = schema(rev);
        let set: std::collections::BTreeSet<_> = s
            .decoded_questions()
            .map(|q| (q.store.clone(), q.offset, q.width_bits, q.prompt.clone()))
            .collect();
        set.len()
    };
    assert_eq!(unique("HI0"), 3264);
    assert_eq!(unique("HJ0"), 3265);
}

#[test]
fn fixture_anchors_decode() {
    if !have_fixtures() {
        return;
    }
    let a = session("HJ0", "OC-Just-Minus-100.ocb");
    assert_eq!(value(&a, "cpu.core.voltage_offset"), int(-100, "mV"));
    assert_eq!(value(&a, "cpu.vr_voltage_limit"), int(1400, "mV"));
    assert_eq!(value(&a, "cpu.current_limit"), int(307, "A"));
    assert!(matches!(value(&a, "cpu.cep"), Value::Enum { ref symbol, .. } if symbol == "enabled"));
    assert!(
        matches!(value(&a, "cpu.enhanced_turbo"), Value::Enum { ref symbol, .. } if symbol == "enabled")
    );
    assert!(matches!(
        value(&a, "cpu.core.voltage_mode"),
        Value::Enum { raw: Some(4), .. }
    ));

    // The filename says -100; the profile stores -105 mV and 235/235 W.
    let b = session("HJ0", "Base_-100_+2P.ocb");
    assert_eq!(value(&b, "cpu.core.voltage_offset"), int(-105, "mV"));
    assert_eq!(value(&b, "cpu.power.pl1"), int(235, "W"));
    assert_eq!(value(&b, "cpu.power.pl2"), int(235, "W"));
    assert_eq!(value(&b, "cpu.pcore.turbo_ratio_offset"), int(2, "ratio"));

    let c = session("HI0", "BeforeUpgrade.ocb");
    assert_eq!(value(&c, "cpu.core.voltage_offset"), Value::Auto);
    assert_eq!(value(&c, "cpu.ecore.turbo_ratio_offset"), int(2, "ratio"));
}

#[test]
fn identity_mismatch_and_corruption_are_rejected() {
    if !have_fixtures() {
        return;
    }
    let bytes = std::fs::read(root().join("profiles/BeforeUpgrade.ocb")).unwrap();
    let e = Session::pair(schema("HJ0"), bytes.clone()).unwrap_err();
    assert!(matches!(e, Error::IdentityMismatch(_)), "{e}");
    assert_eq!(e.exit_code(), 4);

    let mut corrupt = bytes.clone();
    corrupt[0x200] ^= 1;
    let e = Session::pair(schema("HI0"), corrupt).unwrap_err();
    assert_eq!(e.exit_code(), 4, "{e}");

    // A renamed/forged header is caught by the $OCI$ cross-check and checksum.
    let mut forged = bytes;
    forged[14..17].copy_from_slice(b"HJ0");
    assert!(Session::pair(schema("HJ0"), forged).is_err());

    // Truncation at every structural boundary fails cleanly.
    let full = std::fs::read(root().join("profiles/Base-100mv-NoOC.ocb")).unwrap();
    for cut in [
        0,
        4,
        0x11,
        0x15,
        0x19,
        0x1e,
        0x15d0,
        0x2d2d,
        0x2e2d,
        full.len() - 1,
    ] {
        assert!(
            Session::pair(schema("HJ0"), full[..cut].to_vec()).is_err(),
            "cut at {cut:#x}"
        );
    }
}

#[test]
fn noop_roundtrip_is_byte_identical() {
    if !have_fixtures() {
        return;
    }
    for (rev, p) in [
        ("HJ0", "Base-100mv-NoOC.ocb"),
        ("HJ0", "Base_-100_+2P.ocb"),
        ("HJ0", "OC-Just-Minus-100.ocb"),
        ("HI0", "BeforeUpgrade.ocb"),
    ] {
        let s = session(rev, p);
        let original = std::fs::read(root().join(format!("profiles/{p}"))).unwrap();
        assert_eq!(s.profile.bytes(), original.as_slice());
        let (bytes, checks, _) = edit::apply_and_verify(&s, &[]).expect("no-op");
        assert_eq!(bytes, original, "{p}");
        assert!(verify::all_pass(&checks));
        assert_eq!(verify::verify_profile(&s).payload.status, "valid");
    }
}

fn edit_patch(s: &Session, ops: Vec<Operation>) -> Patch {
    Patch {
        kind: KIND_PATCH.into(),
        schema_version: SCHEMA_VERSION.into(),
        producer: Producer {
            name: "test".into(),
            version: "1".into(),
        },
        mode: PatchMode::Edit,
        payload_version: PAYLOAD_VERSION.into(),
        base: Some(s.binding()),
        source: None,
        target: None,
        operations: ops,
        migration: None,
        policy: Policy {
            unresolved: "error".into(),
        },
        extensions: None,
        diagnostics: vec![],
    }
}

#[test]
fn m0_controlled_edit_matches_spec() {
    if !have_fixtures() {
        return;
    }
    let s = session("HJ0", "Base-100mv-NoOC.ocb");
    let p = edit_patch(
        &s,
        vec![Operation {
            op: "set".into(),
            setting_id: "cpu.core.voltage_offset".into(),
            expected: int(-100, "mV"),
            value: int(-105, "mV"),
        }],
    );
    let out = edit::build_edit(&s, &p, "m0.ocb").expect("build");
    let changed = ocb_core::format::diff_offsets(s.profile.bytes(), &out.bytes);
    // SPEC §14 M0: only 0xCD6 (magnitude) and the checksum change; checksum becomes -113326.
    assert!(
        changed
            .iter()
            .all(|&o| o == 0xCD6 || (0x11..0x15).contains(&o)),
        "{changed:x?}"
    );
    assert_eq!(
        i32::from_le_bytes(out.bytes[0x11..0x15].try_into().unwrap()),
        -113326
    );
}

#[test]
fn edit_preconditions_and_dependencies() {
    if !have_fixtures() {
        return;
    }
    let s = session("HJ0", "Base-100mv-NoOC.ocb");
    // Stale expected value.
    let p = edit_patch(
        &s,
        vec![Operation {
            op: "set".into(),
            setting_id: "cpu.core.voltage_offset".into(),
            expected: int(-105, "mV"),
            value: int(-90, "mV"),
        }],
    );
    assert_eq!(edit::plan_edit(&s, &p).unwrap_err().exit_code(), 6);
    // Ratio offset without its apply mode violates the dependency rule.
    let p = edit_patch(
        &s,
        vec![Operation {
            op: "set".into(),
            setting_id: "cpu.pcore.turbo_ratio_offset".into(),
            expected: Value::Auto,
            value: int(2, "ratio"),
        }],
    );
    let w = edit::plan_edit(&s, &p).unwrap();
    assert_eq!(edit::apply_and_verify(&s, &w).unwrap_err().exit_code(), 6);
    // Same change together with the apply mode succeeds.
    let mode = Value::Enum {
        symbol: "turbo_ratio_offset".into(),
        raw: None,
        label: None,
    };
    let all_core = Value::Enum {
        symbol: "all_core".into(),
        raw: None,
        label: None,
    };
    let p = edit_patch(
        &s,
        vec![
            Operation {
                op: "set".into(),
                setting_id: "cpu.pcore.ratio_apply_mode".into(),
                expected: all_core,
                value: mode,
            },
            Operation {
                op: "set".into(),
                setting_id: "cpu.pcore.turbo_ratio_offset".into(),
                expected: Value::Auto,
                value: int(2, "ratio"),
            },
        ],
    );
    assert!(edit::build_edit(&s, &p, "x.ocb").is_ok());
    // Off-step and out-of-range values are rejected.
    for v in [-103, -2000] {
        let p = edit_patch(
            &s,
            vec![Operation {
                op: "set".into(),
                setting_id: "cpu.core.voltage_offset".into(),
                expected: int(-100, "mV"),
                value: int(v, "mV"),
            }],
        );
        assert_eq!(edit::plan_edit(&s, &p).unwrap_err().exit_code(), 6, "{v}");
    }
    // Auto-only and read-only recipes.
    let p = edit_patch(
        &s,
        vec![Operation {
            op: "set".into(),
            setting_id: "cpu.ac_loadline".into(),
            expected: Value::Auto,
            value: int(50, "raw"),
        }],
    );
    assert!(edit::plan_edit(&s, &p).is_err());
    // Changed input hash.
    let mut p = edit_patch(&s, vec![]);
    p.base.as_mut().unwrap().profile_sha256 = "0".repeat(64);
    assert_eq!(edit::plan_edit(&s, &p).unwrap_err().exit_code(), 9);
}

fn plan_for(src: &Session, tgt: &Session, groups: &[&str]) -> Patch {
    migrate::plan(
        src,
        tgt,
        Selection {
            groups: groups.iter().map(|s| s.to_string()).collect(),
            settings: vec![],
        },
        None,
    )
    .unwrap()
    .0
}

#[test]
fn upgrade_hi0_to_hj0() {
    if !have_fixtures() {
        return;
    }
    let src = session("HI0", "BeforeUpgrade.ocb");
    let tgt = session("HJ0", "Base-100mv-NoOC.ocb");
    let plan = plan_for(&src, &tgt, &["all"]);
    let m = plan.migration.as_ref().unwrap();
    assert!(m.can_build);
    assert!(
        m.target_only
            .iter()
            .any(|t| t.setting_id == "platform.high_efficiency_mode")
    );
    let out = migrate::build(&src, &tgt, &plan, "out.ocb").expect("build");
    assert!(verify::all_pass(&out.manifest.checks));
    let o = Session::pair(schema("HJ0"), out.bytes.clone()).unwrap();
    for id in [
        "cpu.pcore.turbo_ratio_offset",
        "cpu.ecore.turbo_ratio_offset",
        "cpu.pcore.ratio_apply_mode",
        "cpu.core.voltage_offset",
    ] {
        assert!(value(&o, id).same_intent(&value(&src, id)), "{id}");
    }
    // Opaque/trailer data comes from the target baseline.
    assert_eq!(
        &out.bytes[o.profile.cmos_offset..],
        &tgt.profile.bytes()[tgt.profile.cmos_offset..]
    );
}

#[test]
fn downgrade_conflicts_require_resolution() {
    if !have_fixtures() {
        return;
    }
    // OC-Just-Minus-100 stores IOMMU raw 2 (Compatible Mode), absent in HI0.
    let src = session("HJ0", "OC-Just-Minus-100.ocb");
    let tgt = session("HI0", "BeforeUpgrade.ocb");
    let mut plan = plan_for(&src, &tgt, &["all"]);
    let m = plan.migration.as_ref().unwrap();
    let iommu = m
        .entries
        .iter()
        .find(|e| e.setting_id == "platform.iommu")
        .unwrap();
    assert_eq!(iommu.final_action, Some(FinalAction::Blocked));
    assert_eq!(
        iommu.assessment.as_ref().unwrap().reason,
        Some(Reason::UnsupportedValue)
    );
    assert!(!m.can_build);
    assert_eq!(
        migrate::build(&src, &tgt, &plan, "x")
            .unwrap_err()
            .exit_code(),
        7
    );

    // Resolve with an explicit replacement: the destination gets 1, never the source's 2.
    let entries = &mut plan.migration.as_mut().unwrap().entries;
    let e = entries
        .iter_mut()
        .find(|e| e.setting_id == "platform.iommu")
        .unwrap();
    e.resolution = Resolution {
        kind: ResolutionKind::SetValue,
        value: Some(Value::Enum {
            symbol: "enable_iommu_during_boot".into(),
            raw: None,
            label: None,
        }),
    };
    // Forged engine fields are ignored.
    e.final_action = Some(FinalAction::Transfer);
    // High Efficiency Mode exists only in HJ0: missing_setting until explicitly omitted.
    let hem = entries
        .iter_mut()
        .find(|e| e.setting_id == "platform.high_efficiency_mode")
        .unwrap();
    assert_eq!(
        hem.assessment.as_ref().unwrap().reason,
        Some(Reason::MissingSetting)
    );
    assert_eq!(
        migrate::build(&src, &tgt, &plan_for(&src, &tgt, &["platform"]), "x")
            .unwrap_err()
            .exit_code(),
        7
    );
    let entries = &mut plan.migration.as_mut().unwrap().entries;
    let hem = entries
        .iter_mut()
        .find(|e| e.setting_id == "platform.high_efficiency_mode")
        .unwrap();
    hem.resolution = Resolution {
        kind: ResolutionKind::Omit,
        value: None,
    };
    let out = migrate::build(&src, &tgt, &plan, "x").expect("resolved build");
    let o = Session::pair(schema("HI0"), out.bytes).unwrap();
    assert!(matches!(
        value(&o, "platform.iommu"),
        Value::Enum { raw: Some(1), .. }
    ));
    assert_eq!(value(&o, "cpu.core.voltage_offset"), int(-100, "mV"));

    // keep_target is also permitted; unpermitted resolutions block.
    let mut p2 = plan_for(&src, &tgt, &["platform"]);
    let e = p2
        .migration
        .as_mut()
        .unwrap()
        .entries
        .iter_mut()
        .find(|e| e.setting_id == "platform.iommu")
        .unwrap();
    e.resolution = Resolution {
        kind: ResolutionKind::KeepTarget,
        value: None,
    };
    let hem = p2
        .migration
        .as_mut()
        .unwrap()
        .entries
        .iter_mut()
        .find(|e| e.setting_id == "platform.high_efficiency_mode")
        .unwrap();
    hem.resolution = Resolution {
        kind: ResolutionKind::SetValue,
        value: Some(Value::Auto),
    };
    assert_eq!(
        migrate::build(&src, &tgt, &p2, "x")
            .unwrap_err()
            .exit_code(),
        7,
        "set_value is not permitted for missing_setting"
    );
    let hem = p2
        .migration
        .as_mut()
        .unwrap()
        .entries
        .iter_mut()
        .find(|e| e.setting_id == "platform.high_efficiency_mode")
        .unwrap();
    hem.resolution = Resolution {
        kind: ResolutionKind::KeepTarget,
        value: None,
    };
    assert!(migrate::build(&src, &tgt, &p2, "x").is_ok());
}

#[test]
fn compound_omission_is_validated() {
    if !have_fixtures() {
        return;
    }
    // Carry the -100 mV offset but keep the target's Auto voltage mode: the final state is invalid.
    let src = session("HJ0", "Base-100mv-NoOC.ocb");
    let tgt = session("HI0", "BeforeUpgrade.ocb");
    let mut plan = plan_for(&src, &tgt, &["cpu.voltage"]);
    let e = plan
        .migration
        .as_mut()
        .unwrap()
        .entries
        .iter_mut()
        .find(|e| e.setting_id == "cpu.core.voltage_mode")
        .unwrap();
    e.resolution = Resolution {
        kind: ResolutionKind::Omit,
        value: None,
    };
    assert_eq!(
        migrate::build(&src, &tgt, &plan, "x")
            .unwrap_err()
            .exit_code(),
        6
    );
}

#[test]
fn diff_reports_filename_independent_values() {
    if !have_fixtures() {
        return;
    }
    let l = session("HJ0", "Base-100mv-NoOC.ocb");
    let r = session("HJ0", "Base_-100_+2P.ocb");
    let d = inspect::diff(&l, &r).payload;
    let changed: Vec<&str> = d
        .settings
        .iter()
        .filter(|s| !s.equal)
        .map(|s| s.setting_id.as_str())
        .collect();
    assert!(changed.contains(&"cpu.core.voltage_offset"));
    assert!(changed.contains(&"cpu.power.pl1"));
    assert!(!d.other_field_differences.is_empty());
}

#[test]
fn originals_keep_their_digests() {
    if !have_fixtures() {
        return;
    }
    let sums = std::fs::read_to_string(root().join("../SHA256SUMS")).unwrap();
    for line in sums.lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        let bytes = std::fs::read(root().join(name)).unwrap();
        assert_eq!(ocb_core::bytes::sha256_hex(&bytes), hash, "{name}");
    }
    // The wider BIOS corpus is optional: check whichever images are provisioned.
    let corpus = std::fs::read_to_string(root().join("../SHA256SUMS.corpus")).unwrap_or_default();
    for line in corpus.lines() {
        let (hash, name) = line.split_once("  ").unwrap();
        if let Ok(bytes) = std::fs::read(root().join(name)) {
            assert_eq!(ocb_core::bytes::sha256_hex(&bytes), hash, "{name}");
        }
    }
}

fn setting<'a>(r: &'a Report<InspectPayload>, id: &str) -> &'a DecodedSetting {
    r.payload
        .settings
        .iter()
        .find(|s| s.setting_id == id)
        .expect("setting")
}

#[test]
fn menu_locations_follow_the_bios() {
    if !have_fixtures() {
        return;
    }
    let r = inspect::inspect(&session("HJ0", "Base-100mv-NoOC.ocb"), false);
    let m = |id: &str| setting(&r, id).menu.clone().expect("menu");
    let ratio = m("cpu.pcore.ratio_apply_mode");
    assert_eq!(ratio.path, ["Overclocking"]);
    assert_eq!(ratio.section.as_deref(), Some("CPU Setting"));
    assert_eq!(ratio.prompt, "P-Core Ratio Apply Mode");
    let volt = m("cpu.core.voltage_offset");
    assert_eq!(volt.section.as_deref(), Some("Voltage Setting"));
    assert_eq!(volt.prompt, "CPU Core Voltage Offset");
    assert_eq!(
        m("cpu.current_limit").path,
        ["Overclocking", "Advanced CPU Configuration"]
    );
    // AC load line also exists on CPU Features under `suppressif TRUE`; the shown copy wins.
    let ac = m("cpu.ac_loadline");
    assert_eq!(ac.path, ["Overclocking", "Advanced CPU Configuration"]);
    assert!(ac.also_on.is_empty());
    assert_eq!(m("platform.iommu").path, ["Overclocking", "CPU Features"]);
    assert_eq!(r.payload_version, INSPECT_PAYLOAD_VERSION);
}

#[test]
fn optimized_defaults_are_read_from_std_defaults() {
    if !have_fixtures() {
        return;
    }
    for rev in ["HI0", "HJ0"] {
        let d = schema(rev).std_defaults.clone().expect("StdDefaults");
        assert_eq!(d.copies.len(), 3);
        assert_eq!(d.stores["Setup"].len(), 5081);
        assert_eq!(d.stores["CpuSetup"].len(), 961);
    }
    let r = inspect::inspect(&session("HJ0", "OC-Just-Minus-100.ocb"), true);
    let s = |id: &str| setting(&r, id);
    assert_eq!(
        s("cpu.core.voltage_offset").differs_from_default,
        Some(true)
    );
    assert_eq!(
        s("cpu.core.voltage_offset").default_value,
        Some(Value::Auto)
    );
    assert_eq!(s("cpu.current_limit").differs_from_default, Some(true));
    assert_eq!(
        s("cpu.pcore.ratio_apply_mode").differs_from_default,
        Some(false)
    );
    // HJ0 IOMMU default is raw 2 (Compatible Mode); HI0 has no raw 2.
    let iommu = s("platform.iommu").default_value.clone();
    assert!(matches!(iommu, Some(Value::Enum { raw: Some(2), .. })));
    let raw_diff = r
        .payload
        .raw_fields
        .iter()
        .filter(|f| f.default_raw.is_some_and(|d| d != f.raw))
        .count();
    assert!(raw_diff > 0 && raw_diff < 200, "{raw_diff}");
    // A signed offset whose sign is Auto equals the default even if its magnitude is stale.
    let hi = inspect::inspect(&session("HI0", "BeforeUpgrade.ocb"), false);
    assert_eq!(
        setting(&hi, "cpu.core.voltage_offset").differs_from_default,
        Some(false)
    );
}

/// AMD images are rejected as unsupported with a clear reason. 5A33's Setup module contains machine
/// code that matches the forms-package byte pattern; it must be skipped, not abort the load.
#[test]
fn amd_images_are_unsupported_not_malformed() {
    for name in ["E7E48AMSI.2A35", "E7E49AMSI.1AA7", "E7E59AMSI.5A33"] {
        let path = root().join("bios").join(name);
        if !path.exists() {
            eprintln!("skipping {name}: not provisioned");
            continue;
        }
        match io::load_bios(&path) {
            Err(Error::Unsupported(m)) => assert!(m.contains("AMD"), "{name}: {m}"),
            other => panic!("{name}: expected Unsupported, got {:?}", other.map(|_| ())),
        }
    }
}

/// Every image in the corpus opens for browsing with the parser for its family. Intel decode
/// schemas keep their fingerprints; AMD formsets are paired with their own strings.
#[test]
fn every_board_family_is_browsable() {
    use ocb_core::firmware::platform::Vendor;
    // (file, vendor, version label, schema fingerprint prefix for Intel, minimum shown fields)
    let cases = [
        (
            "E7D91IMS.H85",
            Vendor::Intel,
            "V17.85",
            Some("aecefe991044"),
            5000,
        ),
        (
            "E7D91IMS.HI0",
            Vendor::Intel,
            "V17.I",
            Some("bd25c97b65b9"),
            5000,
        ),
        (
            "E7D91IMS.HJ0",
            Vendor::Intel,
            "V17.J",
            Some("284c9c67e2be"),
            5000,
        ),
        (
            "E7E34IMSI.1AF0",
            Vendor::Intel,
            "V1.AF0",
            Some("a3273d9225ac"),
            5000,
        ),
        ("E7E48AMSI.2A35", Vendor::Amd, "V2.A35", None, 5000),
        ("E7E49AMSI.1AA7", Vendor::Amd, "V1.AA7", None, 5000),
        ("E7E59AMSI.5A33", Vendor::Amd, "V5.A33", None, 5000),
    ];
    for (name, vendor, version, fp, min_fields) in cases {
        let path = root().join("bios").join(name);
        if !path.exists() {
            eprintln!("skipping {name}: not provisioned");
            continue;
        }
        let a = io::load_analysis(&path).expect(name);
        assert_eq!(a.image.platform.vendor, vendor, "{name}");
        assert_eq!(a.image.platform.version, version, "{name}");
        match (fp, &a.schema) {
            (Some(fp), Ok(s)) => {
                assert!(s.fingerprint.starts_with(fp), "{name}: {}", s.fingerprint)
            }
            (None, Err(Error::Unsupported(m))) => assert!(m.contains("AMD"), "{name}: {m}"),
            (_, other) => panic!(
                "{name}: unexpected schema result {:?}",
                other.as_ref().map(|_| ())
            ),
        }
        let report = ocb_core::engine::browse::browse(&a.image, a.schema.as_ref().ok(), None, 0);
        let shown = report.payload.fields.iter().filter(|f| !f.hidden).count();
        assert!(shown >= min_fields, "{name}: only {shown} fields");
        // The main setup formset must decode (5A33 once lost it behind a false package match).
        let main = &a.image.formsets[0];
        assert!(
            main.title.is_empty() && main.questions.len() > 2000,
            "{name}: main formset"
        );
        if vendor == Vendor::Amd {
            let titles: Vec<&str> = a.image.formsets.iter().map(|f| f.title.as_str()).collect();
            for t in [
                "AMD CBS (RPL)",
                "AMD CBS (PHX)",
                "AMD PBS",
                "AMD Overclocking (STX)",
                "AMD Overclocking (PHX)",
            ] {
                assert!(titles.contains(&t), "{name}: missing {t} in {titles:?}");
            }
            // AodDxe carries four formsets with four string tables; each must use its own.
            let aod = a
                .image
                .formsets
                .iter()
                .find(|f| f.title == "AMD Overclocking (RPL)")
                .unwrap();
            assert!(
                aod.questions
                    .iter()
                    .any(|q| q.prompt.contains("Curve Optimizer")),
                "{name}"
            );
            assert!(
                aod.questions
                    .iter()
                    .all(|q| q.prompt.is_empty() || q.prompt.is_ascii()),
                "{name}"
            );
        }
    }
}

/// One image per structural family of the wider MSI corpus (skipped when not provisioned).
/// Checks the classification, browse coverage and that profiles stay closed outside the
/// examined CpuSetup layout.
#[test]
fn corpus_families_classify_and_browse() {
    use ocb_core::firmware::platform::Vendor;
    // (path under bios/, vendor, family label, schema decodes, profiles offered, formset titles)
    type Case<'a> = (&'a str, Vendor, &'a str, bool, bool, &'a [&'a str]);
    let cases: [Case; 7] = [
        (
            "H310M-PRO-C/E7D02IMS.130",
            Vendor::Intel,
            "Intel LGA1151, 300 series",
            false,
            false,
            &[],
        ),
        (
            "X299-A-PRO/E7A94IMS.A20",
            Vendor::Intel,
            "Intel LGA2066, X299 HEDT",
            false,
            false,
            &["Socket Configuration", "Platform Configuration"],
        ),
        (
            "Z490-PLUS/E7C98IMS.1A2",
            Vendor::Intel,
            "Intel LGA1200, 400 series",
            true,
            false,
            &[],
        ),
        (
            "Z590-PLUS/E7D11IMS.1A1",
            Vendor::Intel,
            "Intel LGA1200, 500 series",
            true,
            false,
            &[],
        ),
        (
            "Creator-TRX40/E7C59AMS.198",
            Vendor::Amd,
            "AMD sTRX4, TRX40 HEDT",
            false,
            false,
            &["AMD CBS", "AMD PBS"],
        ),
        (
            "B450-TOMAHAWK-MAX-II/E7C02AMS.HG4",
            Vendor::Amd,
            "AMD AM4",
            false,
            false,
            &["AMD CBS (RN)", "AMD CBS (SSP)", "AMD Overclocking"],
        ),
        (
            "A320M-PRO-VH/E7C52AMS.1B8",
            Vendor::Amd,
            "AMD AM4",
            false,
            false,
            &["AMD CBS (ZP)", "AMD CBS (RV)", "AMD Overclocking"],
        ),
    ];
    for (rel, vendor, family, decodes, profiles, titles) in cases {
        let path = root().join("bios").join(rel);
        if !path.exists() {
            eprintln!("skipping {rel}: not provisioned");
            continue;
        }
        let a = io::load_analysis(&path).expect(rel);
        let p = &a.image.platform;
        assert_eq!(p.vendor, vendor, "{rel}");
        assert_eq!(p.family, family, "{rel}");
        assert_eq!(a.schema.is_ok(), decodes, "{rel}");
        assert_eq!(p.profiles_supported, profiles, "{rel}");
        if let Ok(s) = &a.schema {
            assert!(!s.platform.profiles_supported, "{rel}: schema platform");
            // The engine refuses pairing too, before looking at the profile.
            let e = Session::pair(Arc::new(s.clone()), vec![0u8; 16]).unwrap_err();
            assert!(matches!(e, Error::Unsupported(_)), "{rel}: {e}");
        }
        let report = ocb_core::engine::browse::browse(&a.image, a.schema.as_ref().ok(), None, 0);
        let have: Vec<&str> = report
            .payload
            .formsets
            .iter()
            .map(|f| f.title.as_str())
            .collect();
        for t in titles {
            assert!(have.contains(t), "{rel}: missing {t} in {have:?}");
        }
        // Each title once per view: identical AMD copies are merged and a second AM4 firmware
        // set is a separate view.
        let mut uniq = have.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), have.len(), "{rel}: duplicate titles {have:?}");
        let shown = report.payload.fields.iter().filter(|f| !f.hidden).count();
        assert!(shown >= 1000, "{rel}: only {shown} fields");
    }
}

/// Recipes refuse an image that reuses a prompt with other raw meanings; enum-only LLC accepts
/// the width the image declares and decodes by its own labels.
#[test]
fn reviewed_option_meanings_guard_other_generations() {
    use ocb_core::catalog::recipe;
    use ocb_core::engine::codec::{Resolution, resolve};
    let load = |rel: &str| {
        let path = root().join("bios").join(rel);
        path.exists()
            .then(|| io::load_analysis(&path).expect(rel).schema.expect(rel))
    };
    let mode = recipe("cpu.core.voltage_mode").unwrap();
    if let Some(hj0) = load("E7D91IMS.HJ0") {
        assert!(matches!(resolve(&hj0, mode), Resolution::Found(_)));
    }
    // Z890: raw 4 (Adaptive + Offset) is not an option there.
    if let Some(z890) = load("E7E34IMSI.1AF0") {
        match resolve(&z890, mode) {
            Resolution::Ambiguous(m) => assert!(m.contains("option meanings differ"), "{m}"),
            _ => panic!("Z890 voltage mode must not resolve"),
        }
    }
    // H670: LLC is 8-bit with Auto = 0 and Mode N = N.
    if let Some(h670) = load("MAG-H670-TOMAHAWK-WIFI-DDR4/E7D25IMS.HO0") {
        match resolve(&h670, recipe("cpu.llc").unwrap()) {
            Resolution::Found(f) => {
                assert_eq!(f[0].width, 1);
                assert_eq!(f[0].options.get(&0).map(String::as_str), Some("Auto"));
                assert_eq!(f[0].options.get(&1).map(String::as_str), Some("Mode 1"));
            }
            _ => panic!("H670 LLC must resolve at its own width"),
        }
    }
}

/// 32 MiB AM4 images: two complete firmware sets, each its own view with its own defaults, AMD
/// pages placed under the Setup pages of the same title.
#[test]
fn am4_firmware_sets_are_separate_views() {
    let path = root().join("bios/B450-TOMAHAWK-MAX-II/E7C02AMS.HG4");
    if !path.exists() {
        eprintln!("skipping: not provisioned");
        return;
    }
    let a = io::load_analysis(&path).unwrap();
    let labels: Vec<&str> = a.image.sets.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["CBS RN + SSP", "CBS ZP + RV"]);
    for (set, cbs) in [(0, "AMD CBS (SSP)"), (1, "AMD CBS (ZP)")] {
        let r = ocb_core::engine::browse::browse(&a.image, None, None, set);
        let p = &r.payload;
        assert_eq!(p.set, set);
        assert!(
            p.bios.defaults.source.contains("covers"),
            "{}",
            p.bios.defaults.source
        );
        let f = p.formsets.iter().find(|f| f.title == cbs).expect(cbs);
        assert_eq!(f.placement, "title_match");
        assert_eq!(f.placed_under.last().map(String::as_str), Some("AMD CBS"));
        let aod = p
            .formsets
            .iter()
            .find(|f| f.title == "AMD Overclocking")
            .unwrap();
        assert!(aod.note.is_some());
        let shown = p.fields.iter().filter(|f| !f.hidden).count();
        assert!(shown > 1500, "set {set}: {shown}");
    }
    let r = ocb_core::engine::browse::browse(&a.image, None, None, 7);
    assert_eq!(r.payload.set, 0);
    assert!(
        r.diagnostics
            .iter()
            .any(|d| d.code == "firmware_set_unknown")
    );
}

#[test]
fn bios_settings_list_every_shown_field_read_only() {
    if !have_fixtures() {
        return;
    }
    let s = session("HJ0", "Base_-100_+2P.ocb");
    let generic = inspect::bios_settings(&s);
    assert!(
        generic.len() > 3000,
        "only {} generic settings",
        generic.len()
    );
    assert!(
        generic
            .iter()
            .all(|g| !g.editability.writable && g.group == "bios")
    );
    // No generic row duplicates a reviewed recipe's bytes.
    let report = inspect::inspect(&s, false);
    let recipe_spans: Vec<(String, u16, u16)> = report
        .payload
        .settings
        .iter()
        .flat_map(|r| r.fields.iter())
        .map(|f| {
            (
                f.store.clone(),
                f.store_offset,
                f.store_offset + f.width_bits / 8,
            )
        })
        .collect();
    for g in &generic {
        let f = &g.fields[0];
        let (a, b) = (f.store_offset, f.store_offset + f.width_bits / 8);
        assert!(
            !recipe_spans
                .iter()
                .any(|(st, x, y)| *st == f.store && a < *y && b > *x),
            "{} overlaps a reviewed recipe",
            g.label
        );
    }
    // A known firmware-filled difference decodes with the BIOS's own labels.
    let dmi = generic
        .iter()
        .find(|g| g.label == "DMI Gen4 EQ Mode")
        .expect("DMI Gen4 EQ Mode");
    assert_eq!(
        dmi.value.as_ref().map(Value::display).as_deref(),
        Some("Fixed EQ")
    );
    assert_eq!(dmi.differs_from_default, Some(true));
}

#[test]
fn full_compare_finds_the_known_differences() {
    if !have_fixtures() {
        return;
    }
    // Same release: Base_-100_+2P really stores -105 mV and 235 W (not what its name says).
    let d = inspect::diff_with(
        &session("HJ0", "Base-100mv-NoOC.ocb"),
        &session("HJ0", "Base_-100_+2P.ocb"),
        true,
    )
    .payload;
    assert!(d.compared > 3000);
    let off = d
        .all_differences
        .iter()
        .find(|e| e.key == "cpu.core.voltage_offset")
        .expect("voltage offset differs");
    assert_eq!(
        off.left.as_ref().map(Value::display).as_deref(),
        Some("-100 mV")
    );
    assert_eq!(
        off.right.as_ref().map(Value::display).as_deref(),
        Some("-105 mV")
    );
    assert!(d.all_differences.iter().all(|e| e.only.is_none()));

    // Across releases: High Efficiency Mode exists only in HJ0.
    let x = inspect::diff_with(
        &session("HI0", "BeforeUpgrade.ocb"),
        &session("HJ0", "Base-100mv-NoOC.ocb"),
        true,
    )
    .payload;
    assert!(
        x.all_differences
            .iter()
            .any(|e| e.label == "High Efficiency Mode" && e.only.as_deref() == Some("right"))
    );
}

/// The experimental analyzer, on the profiles the validated reader supports, must bind Setup
/// and CpuSetup to the very records `Session::pair` binds, mark them confirmed, and never
/// mark any other record confirmed.
#[test]
fn experimental_mapping_agrees_with_the_validated_binding() {
    use ocb_core::engine::experimental::{AnalyzeOptions, BiosRef, analyze_profile};
    if !have_fixtures() {
        return;
    }
    for rev in ["HI0", "HJ0"] {
        let a = io::load_analysis(&root().join(format!("bios/E7D91IMS.{rev}"))).expect("bios");
        let profiles: &[&str] = if rev == "HI0" {
            &["BeforeUpgrade.ocb"]
        } else {
            &[
                "Base-100mv-NoOC.ocb",
                "Base_-100_+2P.ocb",
                "OC-Just-Minus-100.ocb",
            ]
        };
        for name in profiles {
            let bytes = std::fs::read(root().join(format!("profiles/{name}"))).expect("profile");
            let s = session(rev, name);
            let bios = BiosRef {
                image: &a.image,
                profiles_supported: true,
                schema: None,
            };
            let r = analyze_profile(&bytes, Some(bios), &AnalyzeOptions::default())
                .expect("analysis")
                .payload;
            assert_eq!(r.container, "established", "{name}");
            assert_eq!(r.integrity.status, "verified", "{name}");
            let bound = |store: &str| {
                let st = r
                    .stores
                    .iter()
                    .find(|x| x.name == store && x.mapping == "confirmed");
                let key = st.and_then(|x| x.mapped_record.clone()).expect(store);
                r.records
                    .iter()
                    .find(|x| x.key == key)
                    .expect("record")
                    .payload
                    .start
            };
            assert_eq!(bound("Setup"), s.setup.offset, "{name} Setup");
            assert_eq!(bound("CpuSetup"), s.cpu_setup.offset, "{name} CpuSetup");
            assert_eq!(r.coverage.records_confirmed, 2, "{name}");
            assert!(r.fields.iter().all(|f| f.mapping != "confirmed"
                || f.store.contains(":Setup:")
                || f.store.contains(":CpuSetup:")));

            // amitse_variable_index/1: the BIOS's own variable list names every record, and
            // agrees with the reviewed binding and with every isolated size match.
            assert!(r.records.iter().all(|x| x.variable.is_some()), "{name}");
            assert_eq!(r.coverage.records_ambiguous, 0, "{name}");
            let named = |key: &str| {
                r.records
                    .iter()
                    .find(|x| x.key == key)
                    .and_then(|x| x.variable.as_ref())
                    .map(|v| v.name.as_str())
            };
            assert_eq!(named("r0"), Some("Setup"));
            assert_eq!(named("r57"), Some("CpuSetup"));
            for (key, store) in [
                ("r3", "AMITSESetup"),
                ("r58", "SetupCpuFeatures"),
                ("r59", "CpuSetupVolatileData"),
                ("r60", "SaSetup"),
                ("r61", "TbtSetupVolatileData"),
                ("r62", "PchSetup"),
                ("r64", "MeSetup"),
                ("r65", "MeSetupStorage"),
                ("r71", "LpeSetup"),
                ("r74", "PcieSataSetupVar"),
                ("r104", "MemoryZSetupData"),
                ("r107", "SetupVolatileData"),
            ] {
                assert_eq!(named(key), Some(store), "{name} {key}");
                let rec = r.records.iter().find(|x| x.key == key).expect("record");
                assert_eq!(rec.mapping, "inferred", "{name} {key}");
            }
            // The four variables sized at run time are named but not decoded.
            for key in ["r1", "r12", "r13", "r14"] {
                let rec = r.records.iter().find(|x| x.key == key).expect("record");
                assert_eq!(rec.mapping, "unmapped", "{name} {key}");
                assert!(rec.reason.contains("sized at run time"), "{name} {key}");
            }
            // The profile's ids never use the entries the firmware skips.
            for x in &r.records {
                assert!(
                    ![3, 4, 7, 8, 11, 12, 58, 90].contains(&x.id),
                    "{name} id {}",
                    x.id
                );
            }
        }
    }
}

/// The profile format a BIOS reports without any profile: on each fixture pair, the list
/// positions it says the traced save writes are exactly the profile's record ids.
#[test]
fn the_profile_format_predicts_every_record_id_of_the_real_profiles() {
    use ocb_core::engine::experimental::profile_format;
    if !have_fixtures() {
        return;
    }
    for (rev, names) in [
        ("HI0", &["BeforeUpgrade.ocb"][..]),
        (
            "HJ0",
            &[
                "Base-100mv-NoOC.ocb",
                "Base_-100_+2P.ocb",
                "OC-Just-Minus-100.ocb",
            ][..],
        ),
    ] {
        let a = io::load_analysis(&root().join(format!("bios/E7D91IMS.{rev}"))).expect("bios");
        let p = profile_format(&a.image, true, None)
            .expect("format")
            .payload;
        assert!(p.traced, "{rev}");
        let trailer = p
            .layout
            .iter()
            .find(|f| f.part == "trailer")
            .expect("trailer");
        assert_eq!(trailer.basis, "firmware_code", "{rev}");
        let list = p.variable_list.expect("list");
        let saved: Vec<u32> = list
            .entries
            .iter()
            .filter(|e| e.saved == "saved")
            .map(|e| e.index)
            .collect();
        assert_eq!(list.entries.len(), 124, "{rev}");
        assert_eq!(saved.len(), 116, "{rev}");
        for name in names {
            let bytes = std::fs::read(root().join(format!("profiles/{name}"))).expect("profile");
            let f = ocb_core::format::OcbFile::parse(bytes).expect("parse");
            let ids: Vec<u32> = f.records.iter().map(|r| r.id).collect();
            assert_eq!(ids, saved, "{rev} {name}");
        }
    }
}

/// Every provisioned image carries the setup browser's variable list (`$SPF`), exactly one per
/// firmware set, with the entries the profile save code relies on (research survey, 126 images).
#[test]
fn every_image_has_one_setup_variable_list_per_firmware_set() {
    if !have_fixtures() {
        return;
    }
    let lists = std::fs::read_to_string(root().join("../SHA256SUMS")).unwrap()
        + &std::fs::read_to_string(root().join("../SHA256SUMS.corpus")).unwrap_or_default();
    let (mut checked, mut with_trailer) = (0, 0);
    for line in lists.lines() {
        let (_, name) = line.split_once("  ").unwrap();
        if name.ends_with(".ocb") || !root().join(name).exists() {
            continue;
        }
        let img = io::load_analysis(&root().join(name)).expect(name).image;
        assert!(!img.variable_lists.is_empty(), "{name}");
        for set in 0..img.sets.len().max(1) {
            let roots: std::collections::BTreeSet<&str> = img
                .formsets
                .iter()
                .filter(|f| f.set == set)
                .map(|f| {
                    f.module
                        .container_path
                        .split('/')
                        .next()
                        .unwrap_or_default()
                })
                .collect();
            let digests: std::collections::BTreeSet<&str> = img
                .variable_lists
                .iter()
                .filter(|l| img.sets.len() <= 1 || roots.contains(l.root.as_str()))
                .map(|l| l.sha256.as_str())
                .collect();
            assert_eq!(digests.len(), 1, "{name} set {set}");
        }
        let first = &img.variable_lists[0].entries;
        assert!(first.len() > 58, "{name}: {} entries", first.len());
        assert!(
            first.iter().any(|e| e.name == "Setup" && e.size > 0),
            "{name}"
        );
        assert!(
            img.profile_modules.has_profile_code_in(None),
            "{name}: no $MOS$ module"
        );
        for set in 0..img.sets.len().max(1) {
            let f = ocb_core::engine::experimental::profile_format(&img, false, Some(set))
                .unwrap_or_else(|e| panic!("{name} set {set}: {e}"))
                .payload;
            assert!(f.variable_list.is_some(), "{name} set {set}");
        }
        if img.profile_modules.has_trailer_in(None) {
            with_trailer += 1;
        }
        checked += 1;
    }
    eprintln!("{checked} images checked, {with_trailer} with $OCI$ code");
    if checked == 126 {
        // Research survey: 86 images build $OCI$ in OcProfileInfo, 36 (AM5, Intel 800) in the
        // setup browser; only the four E7C52 releases hold no $OCI$ at all.
        assert_eq!(with_trailer, 122);
    }
}

/// A shareable report holds neither the profile's CPU brand nor its save time, no preview
/// bytes, and no local path; the settings stay. A baseline from the same BIOS marks the
/// core voltage offset (100 -> 105 mV) as differing.
#[test]
fn experimental_shareable_report_and_baseline_on_real_profiles() {
    use ocb_core::engine::experimental::{
        AnalyzeOptions, BaselineSource, BiosRef, analyze_with_baseline_on, make_shareable,
        to_json_bounded,
    };
    if !have_fixtures() {
        return;
    }
    let a = io::load_analysis(&root().join("bios/E7D91IMS.HJ0")).expect("bios");
    let bios = BiosRef {
        image: &a.image,
        profiles_supported: true,
        schema: None,
    };
    let read = |n: &str| std::fs::read(root().join(format!("profiles/{n}"))).expect("profile");
    let (main, base) = (read("Base_-100_+2P.ocb"), read("Base-100mv-NoOC.ocb"));
    let mut r = analyze_with_baseline_on(
        &main,
        Some(bios),
        Some(BaselineSource::same_image(&base)),
        &AnalyzeOptions::default(),
    )
    .expect("analysis")
    .payload;
    let b = r.inputs.baseline.as_ref().expect("baseline");
    assert_eq!(b.status, "applied", "{}", b.reason);
    let offset = r
        .fields
        .iter()
        .find(|f| f.label == "CPU Core Voltage Offset")
        .expect("offset field");
    assert_eq!(offset.differs_from_baseline, Some(true));
    assert_eq!(
        (
            offset.raw.as_ref().map(|x| x.dec.as_str()),
            offset.baseline_raw.as_ref().map(|x| x.dec.as_str())
        ),
        (Some("105"), Some("100"))
    );
    assert!(r.coverage.fields_differing_from_baseline.unwrap_or(0) > 0);

    let trailer = r.container_candidates[0].trailers[0].clone();
    let (cpu, time) = (trailer.strings[2].clone(), trailer.strings[3].clone());
    assert!(!cpu.is_empty() && time.len() == 14);
    let before = String::from_utf8(to_json_bounded(&r, 32 << 20).unwrap()).unwrap();
    assert!(before.contains(&cpu) && before.contains(&time));
    make_shareable(&mut r);
    let json = String::from_utf8(to_json_bounded(&r, 32 << 20).unwrap()).unwrap();
    assert!(!json.contains(&cpu), "CPU brand left in the report");
    assert!(!json.contains(&time), "save time left in the report");
    assert!(!json.contains("fixtures"), "a local path is in the report");
    assert!(r.records.iter().all(|x| x.preview_hex.is_empty()));
    assert!(r.analysis.shareable && !r.analysis.redactions.is_empty());
    assert!(r.coverage.fields_decoded > 5000);
}

/// A profile saved on HI0, with a defaults profile saved on the newer HJ0 as the baseline: the
/// comparison applies by menu place, and the BIOS pairing is recorded.
#[test]
fn experimental_cross_version_baseline_on_real_profiles() {
    fn r(a: &ocb_core::firmware::Analysis) -> BiosRef<'_> {
        BiosRef {
            image: &a.image,
            profiles_supported: true,
            schema: None,
        }
    }
    use ocb_core::engine::experimental::{
        AnalyzeOptions, BaselineSource, BiosRef, analyze_with_baseline_on,
    };
    if !have_fixtures() {
        return;
    }
    let a1 = io::load_analysis(&root().join("bios/E7D91IMS.HI0")).expect("bios");
    let a2 = io::load_analysis(&root().join("bios/E7D91IMS.HJ0")).expect("bios");
    let read = |n: &str| std::fs::read(root().join(format!("profiles/{n}"))).expect("profile");
    let (main, base) = (read("BeforeUpgrade.ocb"), read("Base-100mv-NoOC.ocb"));
    let p = analyze_with_baseline_on(
        &main,
        Some(r(&a1)),
        Some(BaselineSource {
            profile: &base,
            bios: Some(r(&a2)),
            set: None,
        }),
        &AnalyzeOptions::default(),
    )
    .expect("analysis")
    .payload;
    let b = p.inputs.baseline.as_ref().expect("baseline");
    assert_eq!(b.status, "applied", "{}", b.reason);
    assert_eq!(b.mode, "cross_version");
    assert_eq!(
        b.bios.as_ref().map(|x| x.bios_id.as_str()),
        Some("E7D91IMS.HJ0")
    );
    assert!(p.coverage.fields_compared_with_baseline.unwrap_or(0) > 1000);
    assert!(p.coverage.fields_differing_from_baseline.unwrap_or(0) > 0);
    assert!(p.coverage.fields_not_compared_with_baseline.is_some());
    // The split counts add up to the differing fields, and the saved baseline is partly off its
    // own optimized default (values the firmware fills in).
    let differing = p.coverage.fields_differing_from_baseline.unwrap_or(0);
    let (at, off, none) = (
        p.coverage.baseline_differing_at_default.expect("at"),
        p.coverage.baseline_differing_off_default.expect("off"),
        p.coverage.baseline_differing_no_default.expect("none"),
    );
    assert_eq!(at + off + none, differing);
    assert!(at > 0 && off > 0, "{at} {off} {none}");
    assert!(
        p.fields
            .iter()
            .filter(|f| f.differs_from_baseline.is_some())
            .all(|f| f.baseline_match.as_deref() == Some("menu_place"))
    );
}

/// A profile saved by another release says which BIOS to open; the analyzer says the same, and
/// without a BIOS names the one the profile was saved on.
#[test]
fn identity_messages_name_the_bios_to_open() {
    use ocb_core::engine::experimental::{AnalyzeOptions, BiosRef, analyze_profile};
    if !have_fixtures() {
        return;
    }
    let bytes = std::fs::read(root().join("profiles/BeforeUpgrade.ocb")).expect("profile");
    let e = Session::pair(schema("HJ0"), bytes.clone()).expect_err("mismatch");
    assert!(matches!(e, Error::IdentityMismatch(_)));
    assert!(
        e.to_string().contains(
            "profile was saved by E7D91IMS.HI0 but the BIOS image is E7D91IMS.HJ0. Open the BIOS image E7D91IMS.HI0 (MSI version V17.I) instead."
        ),
        "{e}"
    );

    let a = io::load_analysis(&root().join("bios/E7D91IMS.HJ0")).expect("bios");
    let bios = BiosRef {
        image: &a.image,
        profiles_supported: true,
        schema: None,
    };
    let r = analyze_profile(&bytes, Some(bios), &AnalyzeOptions::default()).expect("analysis");
    assert_eq!(r.payload.identity.status, "mismatch");
    assert!(
        r.payload
            .identity
            .reason
            .contains("Open the BIOS image E7D91IMS.HI0 (MSI version V17.I) instead.")
    );

    let r = analyze_profile(&bytes, None, &AnalyzeOptions::default()).expect("analysis");
    assert_eq!(r.payload.identity.status, "unknown");
    assert!(
        r.payload.identity.reason.contains(
            "profile was saved on E7D91IMS.HI0; no BIOS image supplied. Open that BIOS image (MSI version V17.I) to read the settings."
        ),
        "{}",
        r.payload.identity.reason
    );
}

/// Real profiles exist only for MS-7D91: its images are `examined`, another Intel board with the
/// same layout is `untested`, and an image without profile support carries no marker.
#[test]
fn profile_evidence_in_reports() {
    if !have_fixtures() {
        return;
    }
    let ev = |rel: &str| {
        let a = io::load_analysis(&root().join(rel)).expect(rel);
        let info = ocb_core::engine::session::image_summary(&a.image, a.schema.as_ref().ok(), None)
            .platform
            .expect("platform");
        let json = serde_json::to_value(&info).expect("json");
        (info.profile_evidence, json)
    };
    let (e, json) = ev("bios/E7D91IMS.HJ0");
    assert_eq!(e, Some(ProfileEvidence::Examined));
    assert_eq!(json["profile_evidence"], "examined");

    let z690 = root().join("bios/PRO-Z690-P-WIFI/E7D33IMS.1M0");
    if z690.exists() {
        let (e, json) = ev("bios/PRO-Z690-P-WIFI/E7D33IMS.1M0");
        assert_eq!(e, Some(ProfileEvidence::Untested));
        assert_eq!(json["profile_evidence"], "untested");
    } else {
        eprintln!("skipping the Z690 check: image not present");
    }

    let amd = root().join("bios/E7E48AMSI.2A35");
    if amd.exists() {
        let (e, json) = ev("bios/E7E48AMSI.2A35");
        assert_eq!(e, None);
        assert!(json.get("profile_evidence").is_none());
    }
}

#[test]
fn experimental_reviewed_readings_preserve_generic_values() {
    use ocb_core::engine::experimental::{AnalyzeOptions, BiosRef, analyze_profile};
    if !have_fixtures() {
        return;
    }
    let a = io::load_analysis(&root().join("bios/E7D91IMS.HJ0")).unwrap();
    for (profile, id, expected) in [
        (
            "OC-Just-Minus-100.ocb",
            "cpu.core.voltage_offset",
            int(-100, "mV"),
        ),
        ("Base_-100_+2P.ocb", "cpu.power.pl1", int(235, "W")),
    ] {
        let bytes = std::fs::read(root().join("profiles").join(profile)).unwrap();
        let enriched = analyze_profile(
            &bytes,
            Some(BiosRef::new(&a.image, a.schema.as_ref().ok())),
            &AnalyzeOptions::default(),
        )
        .unwrap()
        .payload;
        let generic = analyze_profile(
            &bytes,
            Some(BiosRef::new::<firmware::BiosSchema>(&a.image, None)),
            &AnalyzeOptions::default(),
        )
        .unwrap()
        .payload;
        assert!(
            enriched
                .fields
                .iter()
                .any(|f| f.reviewed.as_ref().is_some_and(|r| r.setting_id == id
                    && r.value == expected
                    && r.board_evidence == ocb_core::contracts::ProfileEvidence::Examined)),
            "missing {id}"
        );
        assert_eq!(enriched.fields.len(), generic.fields.len());
        for (a, b) in enriched.fields.iter().zip(&generic.fields) {
            assert_eq!(a.raw, b.raw);
            assert_eq!(a.differs_from_default, b.differs_from_default);
        }
    }
}

#[test]
fn compatible_board_recipe_application_preserves_raw_and_incompatible_has_no_readings() {
    use ocb_core::engine::experimental::{AnalyzeOptions, BiosRef, analyze_profile};
    for (board, expected) in [("E7E02IMS", Some(int(50, "mV"))), ("E7C77IMS", None)] {
        let dir = root().join("new-bios-profiles").join(board);
        let bios_path = dir.join(format!("{board}.1C0"));
        if !bios_path.exists() {
            continue;
        }
        let a = io::load_analysis(&bios_path).unwrap();
        let bytes = std::fs::read(dir.join(format!("{board}.1C0.ocb"))).unwrap();
        let p = analyze_profile(
            &bytes,
            Some(BiosRef::new(&a.image, a.schema.as_ref().ok())),
            &AnalyzeOptions::default(),
        )
        .unwrap()
        .payload;
        let generic = analyze_profile(
            &bytes,
            Some(BiosRef::new::<firmware::BiosSchema>(&a.image, None)),
            &AnalyzeOptions::default(),
        )
        .unwrap()
        .payload;
        assert_eq!(p.fields.len(), generic.fields.len());
        for (enriched, generic) in p.fields.iter().zip(&generic.fields) {
            assert_eq!(enriched.raw, generic.raw);
        }
        let readings: Vec<_> = p
            .fields
            .iter()
            .filter_map(|f| f.reviewed.as_ref())
            .collect();
        if let Some(v) = expected {
            assert!(
                readings
                    .iter()
                    .any(|r| r.setting_id == "cpu.core.voltage_offset"
                        && r.value == v
                        && r.board_evidence == ocb_core::contracts::ProfileEvidence::Untested)
            );
        } else {
            assert!(readings.is_empty());
        }
    }
}

#[test]
fn reviewed_values_survive_baseline_and_shareable_reports() {
    use ocb_core::engine::experimental::{
        AnalyzeOptions, BaselineSource, BiosRef, analyze_with_baseline_on, make_shareable,
    };
    if !have_fixtures() {
        return;
    }
    let a = io::load_analysis(&root().join("bios/E7D91IMS.HJ0")).unwrap();
    let main = std::fs::read(root().join("profiles/OC-Just-Minus-100.ocb")).unwrap();
    let base = std::fs::read(root().join("profiles/Base_-100_+2P.ocb")).unwrap();
    let mut p = analyze_with_baseline_on(
        &main,
        Some(BiosRef::new(&a.image, a.schema.as_ref().ok())),
        Some(BaselineSource::same_image(&base)),
        &AnalyzeOptions::default(),
    )
    .unwrap()
    .payload;
    let check = |p: &ExperimentalInspectPayload| {
        assert!(p.fields.iter().any(|f| {
            f.reviewed.as_ref().is_some_and(|r| {
                r.setting_id == "cpu.core.voltage_offset"
                    && r.value == int(-100, "mV")
                    && r.baseline_value == Some(int(-105, "mV"))
            })
        }))
    };
    check(&p);
    make_shareable(&mut p);
    check(&p);
    let text = serde_json::to_string(&p).unwrap();
    let _: ExperimentalInspectPayload = serde_json::from_str(&text).unwrap();
    let mut legacy: serde_json::Value = serde_json::from_str(&text).unwrap();
    for field in legacy["fields"].as_array_mut().unwrap() {
        if let Some(reading) = field.get_mut("reviewed").and_then(|r| r.as_object_mut()) {
            reading.remove("board_evidence");
        }
    }
    let legacy: ExperimentalInspectPayload = serde_json::from_value(legacy).unwrap();
    assert!(
        legacy
            .fields
            .iter()
            .filter_map(|f| f.reviewed.as_ref())
            .all(|r| { r.board_evidence == ocb_core::contracts::ProfileEvidence::Untested })
    );
}
