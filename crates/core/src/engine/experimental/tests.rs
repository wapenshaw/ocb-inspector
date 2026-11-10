//! Synthetic tests for the experimental analyzer. Profiles come from `probe::build` and BIOS
//! images are assembled by hand here, with expected offsets and values written out
//! independently of the analyzer.

use super::*;
use crate::firmware::ifr::{OptionItem, Question, QuestionKind, StoreKind, VarStore};
use crate::firmware::platform::{Platform, Vendor};
use crate::firmware::{BiosIdentity, FirmwareSet, Formset, Placement, SetupModule};
use crate::format::probe::build::{Rec, Spec, build};

const SETUP_GUID: &str = "EC87D643-EBA4-4BB5-A1E5-3F3E36B20DA9";
const CPU_GUID: &str = "B08F97FF-E6E8-4193-A997-5E9E9B0ADB32";
const PCH_GUID: &str = "4570B7F1-ADE8-4943-8DC3-406472842384";

fn store(id: u16, guid: &str, name: &str, size: u16) -> VarStore {
    VarStore {
        id,
        guid: guid.into(),
        name: name.into(),
        size,
        kind: StoreKind::Buffer,
    }
}

fn q(
    store_id: u16,
    store: &str,
    offset: u16,
    width_bits: u16,
    prompt: &str,
    kind: QuestionKind,
) -> Question {
    Question {
        question_id: offset,
        kind,
        prompt: prompt.into(),
        help: String::new(),
        store_id,
        store: store.into(),
        offset,
        width_bits,
        flags: 0,
        min: None,
        max: None,
        step: None,
        options: if kind == QuestionKind::OneOf {
            vec![opt(0, "Disabled"), opt(1, "Enabled"), opt(2, "Auto")]
        } else {
            Vec::new()
        },
        defaults: Vec::new(),
        conditions: Vec::new(),
        ifr_offset: u32::from(offset) + 1000,
        form_id: None,
        section: None,
    }
}

fn opt(value: u64, label: &str) -> OptionItem {
    OptionItem {
        value,
        label: label.into(),
        default: false,
    }
}

fn formset(guid: &str, stores: Vec<VarStore>, questions: Vec<Question>, set: usize) -> Formset {
    Formset {
        title: guid.into(),
        guid: guid.into(),
        module: SetupModule {
            file_guid: "00000000-0000-0000-0000-000000000000".into(),
            name: Some(format!("Module{guid}")),
            container_path: "fv@0x0".into(),
            pe32_sha256: "ab".repeat(32),
            forms_package_offset: 0,
            strings_package_offset: 0,
            language: "en-US".into(),
        },
        placed_under: Vec::new(),
        stores,
        questions,
        menus: Default::default(),
        defaults: Default::default(),
        defaults_missing: Vec::new(),
        placement: Placement::Main,
        set,
        note: None,
    }
}

fn image(board: &str, flavor: &str, rev: &str, formsets: Vec<Formset>) -> BiosImage {
    BiosImage {
        sha256: "cd".repeat(32),
        size: 1 << 20,
        identity: BiosIdentity {
            bios_id: format!("E{board}{flavor}.{rev}"),
            board: board.into(),
            flavor: flavor.into(),
            revision: rev.into(),
            build_date: "01/01/2026".into(),
        },
        platform: Platform {
            vendor: Vendor::Intel,
            family: "Intel test".into(),
            chipset: None,
            models: None,
            version: "V1".into(),
            parser: "test",
            profiles_supported: false,
            profiles_note: String::new(),
        },
        formsets,
        defaults_note: String::new(),
        diagnostics: Vec::new(),
        sets: Vec::new(),
        variable_lists: Vec::new(),
        profile_modules: Default::default(),
    }
}

/// Setup (40 bytes), CpuSetup (24) and PchSetup (9), each with a few questions.
fn standard_formsets(set: usize) -> Vec<Formset> {
    vec![formset(
        "FS-MAIN",
        vec![
            store(1, SETUP_GUID, "Setup", 40),
            store(2, CPU_GUID, "CpuSetup", 24),
            store(3, PCH_GUID, "PchSetup", 9),
        ],
        vec![
            q(1, "Setup", 2, 8, "Setup switch", QuestionKind::OneOf),
            q(1, "Setup", 8, 64, "Setup big number", QuestionKind::Numeric),
            q(2, "CpuSetup", 0, 8, "Cpu switch", QuestionKind::Checkbox),
            q(3, "PchSetup", 4, 16, "Pch number", QuestionKind::Numeric),
        ],
        set,
    )]
}

/// Payloads with known values: Setup[2] = 1 (Enabled), Setup[8..16] = u64::MAX,
/// CpuSetup[0] = 1, PchSetup[4..6] = 0x1234.
fn standard_records() -> Vec<Rec> {
    let mut setup = vec![0u8; 40];
    setup[2] = 1;
    setup[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    let mut cpu = vec![0u8; 24];
    cpu[0] = 1;
    let mut pch = vec![0u8; 9];
    pch[4..6].copy_from_slice(&0x1234u16.to_le_bytes());
    vec![
        Rec {
            id: 0,
            payload: setup,
        },
        Rec {
            id: 1,
            payload: vec![],
        },
        Rec {
            id: 5,
            payload: cpu,
        },
        Rec {
            id: 7,
            payload: pch,
        },
    ]
}

fn run(
    profile: &[u8],
    img: Option<&BiosImage>,
    set: Option<usize>,
) -> Result<ExperimentalInspectPayload> {
    let bios = img.map(|image| BiosRef {
        image,
        profiles_supported: false,
    });
    let opts = AnalyzeOptions {
        set,
        ..Default::default()
    };
    analyze_profile(profile, bios, &opts).map(|r| r.payload)
}

fn rec<'a>(p: &'a ExperimentalInspectPayload, key: &str) -> &'a RecordInfo {
    p.records.iter().find(|r| r.key == key).expect("record")
}

fn field<'a>(p: &'a ExperimentalInspectPayload, label: &str) -> &'a ExperimentalField {
    p.fields.iter().find(|f| f.label == label).expect("field")
}

#[test]
fn the_established_layout_confirms_the_validated_binding_and_infers_the_rest() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let b = build(&Spec::known("E7D91IMS", "HJ0", standard_records()));
    let p = run(&b, Some(&img), None).unwrap();
    assert_eq!(p.container, "established");
    assert_eq!(p.identity.status, "match");
    assert_eq!(p.integrity.status, "verified");
    assert_eq!(rec(&p, "r0").mapping, "confirmed"); // Setup, first record
    assert_eq!(rec(&p, "r2").mapping, "confirmed"); // CpuSetup
    assert_eq!(rec(&p, "r3").mapping, "inferred"); // PchSetup: unique size, no reviewed rule
    assert_eq!(rec(&p, "r1").mapping, "unmapped"); // empty
    // Setup payload starts at 0x19 + 6; the switch is its byte 2.
    let f = field(&p, "Setup switch");
    assert_eq!(f.span, Some(Range::new(0x19 + 6 + 2, 0x19 + 6 + 3)));
    assert_eq!(
        (f.state.as_str(), f.value_label.as_deref()),
        ("decoded", Some("Enabled"))
    );
    assert_eq!(
        field(&p, "Cpu switch").value_label.as_deref(),
        Some("Enabled")
    );
    let pch = field(&p, "Pch number");
    assert_eq!(pch.raw, Some(RawInt::new(0x1234)));
    assert_eq!(pch.mapping, "inferred");
    assert_eq!(p.capabilities, ExperimentalCapabilities::READ_ONLY);
}

#[test]
fn values_above_the_javascript_safe_integer_round_trip_as_exact_strings() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let b = build(&Spec::known("E7D91IMS", "HJ0", standard_records()));
    let p = run(&b, Some(&img), None).unwrap();
    let raw = field(&p, "Setup big number").raw.clone().unwrap();
    assert_eq!(raw.dec, "18446744073709551615");
    assert_eq!(raw.hex, "0xFFFFFFFFFFFFFFFF");
    let json = to_json_bounded(&p, 1 << 20).unwrap();
    let back: ExperimentalInspectPayload = serde_json::from_slice(&json).unwrap();
    assert_eq!(
        back.fields
            .iter()
            .find(|f| f.label == "Setup big number")
            .unwrap()
            .raw,
        Some(raw)
    );
}

#[test]
fn another_board_is_a_candidate_whose_mappings_are_only_inferred() {
    let img = image("7E26", "AMSI", "1A10", standard_formsets(0));
    let b = build(&Spec::known("E7E26AMSI", "1A10", standard_records()));
    let p = run(&b, Some(&img), None).unwrap();
    assert_eq!(p.container, "candidate");
    assert_eq!(p.selected_candidate.as_deref(), Some("variable_header/14"));
    assert_eq!(p.identity.status, "match");
    // The builder wrote the additive checksum at 5 + 14 = 19, so the trial matches.
    assert_eq!(p.integrity.status, "candidate_match");
    assert_eq!(p.integrity.checksum_at, Some(19));
    for r in ["r0", "r2", "r3"] {
        assert_eq!(rec(&p, r).mapping, "inferred", "{r}");
    }
    assert!(p.fields.iter().all(|f| f.mapping == "inferred"));
}

#[test]
fn an_unknown_checksum_still_shows_inferred_fields_with_a_warning() {
    let img = image("7E26", "AMSI", "1A10", standard_formsets(0));
    let mut s = Spec::known("E7E26AMSI", "1A10", standard_records());
    s.checksum = false;
    let p = run(&build(&s), Some(&img), None).unwrap();
    assert_eq!(p.integrity.status, "unknown");
    assert!(p.coverage.fields_decoded > 0);
    assert!(
        p.status_reasons
            .iter()
            .any(|r| r.contains("Integrity not established"))
    );
}

#[test]
fn a_failed_established_checksum_blocks_every_field() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let mut b = build(&Spec::known("E7D91IMS", "HJ0", standard_records()));
    b[0x19 + 6 + 2] ^= 0xFF; // change a Setup byte without fixing the checksum
    let p = run(&b, Some(&img), None).unwrap();
    assert_eq!(p.integrity.status, "failed");
    assert!(p.fields.is_empty());
    assert!(p.records.iter().all(|r| r.mapping == "unmapped"));
    // The alternate hypotheses cannot unlock it: none is eligible for a 12-byte identifier.
    assert_eq!(p.selected_candidate.as_deref(), Some("known_layout"));
}

#[test]
fn same_size_records_or_stores_stay_ambiguous() {
    // Two 9-byte records, one 9-byte store.
    let img = image("7E26", "AMSI", "1A10", standard_formsets(0));
    let mut records = standard_records();
    records.push(Rec {
        id: 9,
        payload: vec![0; 9],
    });
    let p = run(
        &build(&Spec::known("E7E26AMSI", "1A10", records)),
        Some(&img),
        None,
    )
    .unwrap();
    assert_eq!(rec(&p, "r3").mapping, "ambiguous");
    assert_eq!(rec(&p, "r4").mapping, "ambiguous");
    assert!(p.fields.iter().all(|f| f.label != "Pch number"));
    let pch = p.stores.iter().find(|s| s.name == "PchSetup").unwrap();
    assert_eq!(pch.mapping, "ambiguous");
    assert_eq!(pch.candidate_records, vec!["r3", "r4"]);

    // One 9-byte record, two 9-byte stores with different names.
    let mut fs = standard_formsets(0);
    fs[0].stores.push(store(4, PCH_GUID, "OtherSetup", 9));
    let img = image("7E26", "AMSI", "1A10", fs);
    let p = run(
        &build(&Spec::known("E7E26AMSI", "1A10", standard_records())),
        Some(&img),
        None,
    )
    .unwrap();
    assert_eq!(rec(&p, "r3").mapping, "ambiguous");
    assert_eq!(rec(&p, "r3").candidate_stores.len(), 2);
}

#[test]
fn one_variable_declared_by_two_formsets_is_one_store_and_a_size_conflict_is_not_mapped() {
    let mut fs = standard_formsets(0);
    fs.push(formset(
        "FS-OTHER",
        vec![store(7, PCH_GUID, "PchSetup", 9)],
        vec![q(7, "PchSetup", 0, 8, "Pch switch", QuestionKind::OneOf)],
        0,
    ));
    let img = image("7E26", "AMSI", "1A10", fs);
    let b = build(&Spec::known("E7E26AMSI", "1A10", standard_records()));
    let p = run(&b, Some(&img), None).unwrap();
    let pch: Vec<_> = p.stores.iter().filter(|s| s.name == "PchSetup").collect();
    assert_eq!(pch.len(), 1);
    assert_eq!(pch[0].declared_by.len(), 2);
    assert_eq!(pch[0].mapping, "inferred");
    assert!(p.fields.iter().any(|f| f.label == "Pch switch"));

    // Same GUID and name, different size: neither declaration is mapped.
    let mut fs = standard_formsets(0);
    fs.push(formset(
        "FS-OTHER",
        vec![store(7, PCH_GUID, "PchSetup", 10)],
        Vec::new(),
        0,
    ));
    let img = image("7E26", "AMSI", "1A10", fs);
    let p = run(&b, Some(&img), None).unwrap();
    assert!(
        p.stores
            .iter()
            .filter(|s| s.name == "PchSetup")
            .all(|s| s.support.starts_with("declared with different sizes")
                && s.mapping == "unmapped")
    );
    assert_eq!(rec(&p, "r3").mapping, "unmapped");
}

#[test]
fn wrong_revision_wrong_board_conflicts_and_missing_identity_block_decoding() {
    let b = build(&Spec::known("E7D91IMS", "HJ0", standard_records()));
    for (board, rev) in [("7D91", "HI0"), ("7D92", "HJ0")] {
        let img = image(board, "IMS", rev, standard_formsets(0));
        let p = run(&b, Some(&img), None).unwrap();
        assert_eq!(p.identity.status, "mismatch");
        assert!(p.fields.is_empty() && p.stores.is_empty());
    }
    // Header and $OCI$ disagree.
    let mut s = Spec::known("E7D91IMS", "HJ0", standard_records());
    s.oci = Some(["E7D91IMS", "HI0", "Test CPU", "20261001120000"]);
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let p = run(&build(&s), Some(&img), None).unwrap();
    assert!(p.selected_candidate.is_none());
    assert!(p.fields.is_empty());
    assert!(
        p.container_candidates[0]
            .contradictions
            .iter()
            .any(|c| c.contains("header says"))
    );
    // No identifier anywhere.
    let mut s = Spec::known("E7D91IMS", "HJ0", standard_records());
    s.id = b"NOT-AN-ID-XX".to_vec();
    s.oci = Some(["nothing", "here", "Test CPU", "20261001120000"]);
    let p = run(&build(&s), Some(&img), None).unwrap();
    assert!(p.fields.is_empty());
}

#[test]
fn profile_only_analysis_shows_structure_and_no_fields() {
    let b = build(&Spec::known("E7D91IMS", "HJ0", standard_records()));
    let p = run(&b, None, None).unwrap();
    assert_eq!(p.records.len(), 4);
    assert!(p.fields.is_empty());
    assert_eq!(p.identity.status, "unknown");
    assert_eq!(p.identity.profile_bios_id.as_deref(), Some("E7D91IMS.HJ0"));
    assert!(p.inputs.bios.is_none());
}

#[test]
fn a_multi_set_image_needs_an_explicit_set() {
    let mut fs = standard_formsets(0);
    fs.extend(standard_formsets(1));
    let mut img = image("7E26", "AMSI", "1A10", fs);
    let set = |label: &str| FirmwareSet {
        label: label.into(),
        note: String::new(),
        defaults_note: String::new(),
    };
    img.sets = vec![set("A"), set("B")];
    let b = build(&Spec::known("E7E26AMSI", "1A10", standard_records()));
    let p = run(&b, Some(&img), None).unwrap();
    assert!(p.fields.is_empty());
    assert_eq!(p.inputs.bios.as_ref().unwrap().set, None);
    assert!(
        p.status_reasons
            .iter()
            .any(|r| r.contains("several firmware sets"))
    );
    // Choosing a set analyses that set only: no store appears twice.
    let p = run(&b, Some(&img), Some(1)).unwrap();
    assert!(
        p.stores
            .iter()
            .all(|s| s.set == 1 && s.key.starts_with("set1:"))
    );
    assert_eq!(p.stores.iter().filter(|s| s.name == "Setup").count(), 1);
    assert!(matches!(
        run(&b, Some(&img), Some(2)),
        Err(Error::Request(_))
    ));
    assert!(matches!(run(&b, None, Some(0)), Err(Error::Request(_))));
}

#[test]
fn unknown_values_contradictory_labels_and_unsupported_encodings_stay_explicit() {
    let mut fs = standard_formsets(0);
    let qs = &mut fs[0].questions;
    qs.push(q(1, "Setup", 3, 8, "Unknown raw", QuestionKind::OneOf)); // byte 3 = 7
    let mut dup = q(1, "Setup", 4, 8, "Two labels", QuestionKind::OneOf); // byte 4 = 0
    dup.options.push(opt(0, "Off"));
    qs.push(dup);
    qs.push(q(1, "Setup", 5, 4, "Bit field", QuestionKind::Numeric));
    qs.push(q(1, "Setup", 38, 32, "Past the end", QuestionKind::Numeric));
    let img = image("7E26", "AMSI", "1A10", fs);
    let mut records = standard_records();
    records[0].payload[3] = 7;
    let p = run(
        &build(&Spec::known("E7E26AMSI", "1A10", records)),
        Some(&img),
        None,
    )
    .unwrap();
    // A question past the declared size means the mapping is refused, not truncated.
    assert_eq!(rec(&p, "r0").mapping, "unmapped");
    assert!(rec(&p, "r0").reason.contains("reach past"));

    let mut fs = standard_formsets(0);
    let qs = &mut fs[0].questions;
    qs.push(q(1, "Setup", 3, 8, "Unknown raw", QuestionKind::OneOf));
    let mut dup = q(1, "Setup", 4, 8, "Two labels", QuestionKind::OneOf);
    dup.options.push(opt(0, "Off"));
    qs.push(dup);
    qs.push(q(1, "Setup", 5, 4, "Bit field", QuestionKind::Numeric));
    let img = image("7E26", "AMSI", "1A10", fs);
    let mut records = standard_records();
    records[0].payload[3] = 7;
    let p = run(
        &build(&Spec::known("E7E26AMSI", "1A10", records)),
        Some(&img),
        None,
    )
    .unwrap();
    let u = field(&p, "Unknown raw");
    assert_eq!(
        (u.state.as_str(), u.raw.clone()),
        ("unknown_value", Some(RawInt::new(7)))
    );
    let t = field(&p, "Two labels");
    assert_eq!(t.state, "unknown_value");
    assert!(t.note.as_deref().unwrap().contains("Disabled / Off"));
    assert_eq!(field(&p, "Bit field").state, "unsupported_encoding");
    assert!(field(&p, "Bit field").raw.is_none());
}

#[test]
fn efi_and_name_value_stores_are_listed_but_never_mapped() {
    let mut fs = standard_formsets(0);
    fs[0].stores[2].kind = StoreKind::Efi;
    let img = image("7E26", "AMSI", "1A10", fs);
    let p = run(
        &build(&Spec::known("E7E26AMSI", "1A10", standard_records())),
        Some(&img),
        None,
    )
    .unwrap();
    let pch = p.stores.iter().find(|s| s.name == "PchSetup").unwrap();
    assert_eq!(pch.kind, "efi");
    assert_eq!(pch.mapping, "unmapped");
    assert_eq!(rec(&p, "r3").mapping, "unmapped");
}

#[test]
fn analysis_is_deterministic_and_leaves_the_input_unchanged() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let b = build(&Spec::known("E7D91IMS", "HJ0", standard_records()));
    let before = b.clone();
    let one = to_json_bounded(&run(&b, Some(&img), None).unwrap(), 1 << 20).unwrap();
    let two = to_json_bounded(&run(&b, Some(&img), None).unwrap(), 1 << 20).unwrap();
    assert_eq!(one, two);
    assert_eq!(b, before);
}

#[test]
fn limits_end_the_analysis_with_a_typed_error() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let b = build(&Spec::known("E7D91IMS", "HJ0", standard_records()));
    let opts = AnalyzeOptions {
        set: None,
        limits: ProbeLimits {
            max_fields: 1,
            ..ProbeLimits::default()
        },
    };
    let bios = BiosRef {
        image: &img,
        profiles_supported: false,
    };
    assert!(matches!(
        analyze_profile(&b, Some(bios), &opts),
        Err(Error::Limit(_))
    ));
    let p = run(&b, Some(&img), None).unwrap();
    assert!(matches!(to_json_bounded(&p, 100), Err(Error::Limit(_))));
    // Not an OCB at all: a complete, unrecognized analysis.
    let p = run(b"hello world", Some(&img), None).unwrap();
    assert_eq!(p.container, "unrecognized");
    assert!(p.records.is_empty());
}

#[test]
fn writing_enabled_and_an_approved_image_grant_nothing() {
    crate::settings::install(crate::settings::Settings {
        experimental: crate::settings::Experimental { writing: true },
        ..Default::default()
    });
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let b = build(&Spec::known("E7D91IMS", "HJ0", standard_records()));
    let p = run(&b, Some(&img), None).unwrap();
    assert_eq!(p.capabilities, ExperimentalCapabilities::READ_ONLY);
    assert!(!p.capabilities.profile_write && !p.capabilities.change_planning);
}

// ---------------------------------------------------- amitse_variable_index/1 (firmware list)

/// EFI mixed-endian bytes of a GUID string (the inverse of `guid_string`, written out here).
fn guid_bytes(s: &str) -> [u8; 16] {
    let h: String = s.chars().filter(|c| *c != '-').collect();
    let byte = |i: usize| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap();
    let mut g = [0u8; 16];
    for (k, i) in [3, 2, 1, 0, 5, 4, 7, 6].into_iter().enumerate() {
        g[k] = byte(i);
    }
    for (k, b) in g.iter_mut().enumerate().skip(8) {
        *b = byte(k);
    }
    g
}

fn with_list(mut img: BiosImage, entries: &[(&str, &str, u32)]) -> BiosImage {
    let raw: Vec<([u8; 16], &str, u32)> = entries
        .iter()
        .map(|(g, n, s)| (guid_bytes(g), *n, *s))
        .collect();
    let (entries, sha256) =
        crate::firmware::spf::parse_list(&crate::firmware::spf::build::blob(&raw)).unwrap();
    img.variable_lists.push(VariableList {
        root: "fv@0x0".into(),
        container_path: "fv@0x0/test".into(),
        sha256,
        entries,
    });
    img
}

const OTHER_GUID: &str = "11111111-2222-3333-4444-555555555555";

/// Two 9-byte stores (Alpha, Beta) that size alone cannot tell apart.
fn twin_formsets() -> Vec<Formset> {
    vec![formset(
        "FS-MAIN",
        vec![
            store(1, SETUP_GUID, "Setup", 40),
            store(2, CPU_GUID, "CpuSetup", 24),
            store(3, PCH_GUID, "Alpha", 9),
            store(4, PCH_GUID, "Beta", 9),
        ],
        vec![
            q(1, "Setup", 2, 8, "Setup switch", QuestionKind::OneOf),
            q(3, "Alpha", 4, 16, "Alpha number", QuestionKind::Numeric),
            q(4, "Beta", 0, 8, "Beta switch", QuestionKind::Checkbox),
        ],
        0,
    )]
}

const TWIN_LIST: &[(&str, &str, u32)] = &[
    (SETUP_GUID, "Setup", 40),
    (OTHER_GUID, "Empty", 0),
    (OTHER_GUID, "Lang", 6),
    (PCH_GUID, "Alpha", 9),
    (PCH_GUID, "Beta", 9),
    (CPU_GUID, "CpuSetup", 24),
];

fn twin_records() -> Vec<Rec> {
    let mut alpha = vec![0u8; 9];
    alpha[4..6].copy_from_slice(&0x1234u16.to_le_bytes());
    let mut beta = vec![0u8; 9];
    beta[0] = 1;
    vec![
        Rec {
            id: 0,
            payload: vec![0; 40],
        },
        Rec {
            id: 1,
            payload: vec![],
        },
        Rec {
            id: 2,
            payload: vec![b'x'; 7],
        },
        Rec {
            id: 3,
            payload: alpha,
        },
        Rec {
            id: 4,
            payload: beta,
        },
        Rec {
            id: 5,
            payload: vec![0; 24],
        },
    ]
}

/// A 14-character id board whose image holds no $OCI$ code: the file ends after the CMOS block.
fn no_trailer_profile() -> Vec<u8> {
    let mut s = Spec::known("E7E34IMSI", "1AF0", twin_records());
    s.oci = None;
    s.tail = 0;
    build(&s)
}

#[test]
fn the_variable_list_names_records_that_size_cannot_tell_apart() {
    let img = with_list(image("7E34", "IMSI", "1AF0", twin_formsets()), TWIN_LIST);
    let p = run(&no_trailer_profile(), Some(&img), None).unwrap();
    assert_eq!(p.selected_candidate.as_deref(), Some("variable_header/14"));
    assert_eq!(p.identity.status, "match");
    assert!(p.analysis.rules.iter().any(|r| r == RULE_VARIABLE_INDEX));
    // Record index 3 (id 3) is Alpha and index 4 (id 4) is Beta, by the list, not by size.
    assert_eq!(rec(&p, "r3").mapping, "inferred");
    assert_eq!(rec(&p, "r3").variable.as_ref().unwrap().name, "Alpha");
    assert_eq!(rec(&p, "r4").variable.as_ref().unwrap().name, "Beta");
    assert_eq!(field(&p, "Alpha number").raw.as_ref().unwrap().dec, "4660");
    assert_eq!(
        field(&p, "Beta switch").value_label.as_deref(),
        Some("Enabled")
    );
    // Named but not decodable: an empty record and a variable no menu reads.
    assert_eq!(rec(&p, "r1").mapping, "unmapped");
    assert!(rec(&p, "r1").reason.contains("Empty"));
    assert!(rec(&p, "r2").reason.contains("no BIOS menu reads"));
    assert!(p.records.iter().all(|r| r.variable.is_some()));
    // Firmware-derived basis; this image is not a traced release.
    let ev = p
        .evidence
        .iter()
        .find(|e| e.id == format!("ev:rule:{RULE_VARIABLE_INDEX}"))
        .unwrap();
    assert_eq!(ev.basis, "firmware_code");
    assert!(ev.limitations.iter().any(|l| l.contains("not traced")));
    let m = p.evidence.iter().find(|e| e.id == "ev:map:r3").unwrap();
    assert_eq!(m.basis, "firmware_code");
    // Without the list the same file is ambiguous by size.
    let bare = image("7E34", "IMSI", "1AF0", twin_formsets());
    let p = run(&no_trailer_profile(), Some(&bare), None).unwrap();
    assert_eq!(rec(&p, "r3").mapping, "ambiguous");
    assert!(rec(&p, "r3").variable.is_none());
}

#[test]
fn a_list_that_disagrees_with_the_stores_is_refused_not_fitted() {
    // Shifted by one (as a list from another release would be): sizes disagree almost everywhere.
    let shifted: Vec<(&str, &str, u32)> = std::iter::once((OTHER_GUID, "Extra", 1))
        .chain(TWIN_LIST.iter().copied())
        .collect();
    let img = with_list(image("7E34", "IMSI", "1AF0", twin_formsets()), &shifted);
    let p = run(&no_trailer_profile(), Some(&img), None).unwrap();
    assert!(p.records.iter().all(|r| r.variable.is_none()));
    assert!(
        p.status_reasons
            .iter()
            .any(|r| r.contains("variable list was not used"))
    );
    assert_eq!(rec(&p, "r3").mapping, "ambiguous");
}

#[test]
fn an_id_outside_the_list_or_two_different_lists_refuse_the_rule() {
    let short = with_list(
        image("7E34", "IMSI", "1AF0", twin_formsets()),
        &TWIN_LIST[..4],
    );
    let p = run(&no_trailer_profile(), Some(&short), None).unwrap();
    let ev = p
        .evidence
        .iter()
        .find(|e| e.id == format!("ev:rule:{RULE_VARIABLE_INDEX}"))
        .unwrap();
    assert!(
        ev.contradictions
            .iter()
            .any(|c| c.contains("outside the 4-entry"))
    );
    assert!(p.records.iter().all(|r| r.variable.is_none()));

    let two = with_list(
        with_list(image("7E34", "IMSI", "1AF0", twin_formsets()), TWIN_LIST),
        &TWIN_LIST[..5],
    );
    let p = run(&no_trailer_profile(), Some(&two), None).unwrap();
    assert!(
        p.status_reasons
            .iter()
            .any(|r| r.contains("2 different variable lists"))
    );
}

#[test]
fn without_a_trailer_the_file_must_end_after_the_cmos_block() {
    let mut s = Spec::known("E7E34IMSI", "1AF0", twin_records());
    s.oci = None;
    s.tail = 5;
    let img = with_list(image("7E34", "IMSI", "1AF0", twin_formsets()), TWIN_LIST);
    let p = run(&build(&s), Some(&img), None).unwrap();
    assert!(p.selected_candidate.is_none());
    assert!(p.records.is_empty());
}

#[test]
fn two_records_named_the_same_variable_stay_ambiguous() {
    // A list that names one variable at two positions: both records are "Alpha".
    let mut list = TWIN_LIST.to_vec();
    list[4] = (PCH_GUID, "Alpha", 9);
    let img = with_list(image("7E34", "IMSI", "1AF0", twin_formsets()), &list);
    let p = run(&no_trailer_profile(), Some(&img), None).unwrap();
    assert!(p.analysis.rules.iter().any(|r| r == RULE_VARIABLE_INDEX));
    for key in ["r3", "r4"] {
        assert_eq!(rec(&p, key).mapping, "ambiguous", "{key}");
        assert!(
            rec(&p, key).reason.contains("2 records are named Alpha"),
            "{key}"
        );
    }
    assert!(p.fields.iter().all(|f| f.label != "Alpha number"));
    assert_eq!(
        rec(&p, "r5").mapping,
        "inferred",
        "CpuSetup is still mapped"
    );
}

#[test]
fn record_ids_out_of_list_order_refuse_the_rule() {
    let mut recs = twin_records();
    recs.swap(3, 4); // ids 0, 1, 2, 4, 3, 5
    let mut s = Spec::known("E7E34IMSI", "1AF0", recs);
    s.oci = None;
    s.tail = 0;
    let img = with_list(image("7E34", "IMSI", "1AF0", twin_formsets()), TWIN_LIST);
    let p = run(&build(&s), Some(&img), None).unwrap();
    let ev = p
        .evidence
        .iter()
        .find(|e| e.id == format!("ev:rule:{RULE_VARIABLE_INDEX}"))
        .unwrap();
    assert!(
        ev.contradictions
            .iter()
            .any(|c| c.contains("do not strictly increase"))
    );
    assert!(p.records.iter().all(|r| r.variable.is_none()));
}

#[test]
fn a_file_without_its_trailer_is_truncated_when_the_bios_writes_one() {
    let mut s = Spec::known("E7D91IMS", "HJ0", standard_records());
    s.oci = None;
    s.tail = 0;
    let b = build(&s);
    let mut img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    img.profile_modules
        .modules
        .push(crate::firmware::profile_code::ModuleRef {
            name: "OcProfileInfo".into(),
            guid: "A526D2DC-0000-0000-0000-000000000000".into(),
            root: "fv@0x0".into(),
            sha256: "ef".repeat(32),
            mos: true,
            oci: true,
        });
    let p = run(&b, Some(&img), None).unwrap();
    assert!(
        p.status_reasons
            .iter()
            .any(|r| r.contains("looks truncated")),
        "{:?}",
        p.status_reasons
    );
    assert!(p.fields.is_empty());
    assert!(p.records.iter().all(|r| r.mapping == "unmapped"));
    // The same file is read when the image has no trailer module.
    img.profile_modules.modules.clear();
    let p = run(&b, Some(&img), None).unwrap();
    assert!(!p.fields.is_empty());
}

#[test]
fn a_traced_release_stops_at_a_checksum_its_loader_rejects() {
    let (sha, _) = crate::firmware::profile_code::TRACED_IMAGES[0]; // E7D26IMS.1Q0
    let mut img = image("7D26", "IMS", "1Q0", standard_formsets(0));
    img.sha256 = sha.into();
    let mut s = Spec::known("E7D26IMS", "1Q0", standard_records());
    s.checksum = false;
    let p = run(&build(&s), Some(&img), None).unwrap();
    assert_eq!(p.integrity.status, "unknown");
    assert!(
        p.status_reasons
            .iter()
            .any(|r| r.contains("traced profile loader rejects")),
        "{:?}",
        p.status_reasons
    );
    assert!(p.fields.is_empty());
    // With a matching checksum the same release is interpreted.
    s.checksum = true;
    let p = run(&build(&s), Some(&img), None).unwrap();
    assert_eq!(p.integrity.status, "candidate_match");
    assert!(!p.fields.is_empty());
}

#[test]
fn a_list_that_contradicts_the_validated_binding_is_refused() {
    let mut fs = standard_formsets(0);
    fs[0].stores.push(store(4, SETUP_GUID, "SetupAlt", 40));
    let list: &[(&str, &str, u32)] = &[
        (SETUP_GUID, "SetupAlt", 40),
        (OTHER_GUID, "Empty", 0),
        (OTHER_GUID, "F2", 1),
        (OTHER_GUID, "F3", 1),
        (OTHER_GUID, "F4", 1),
        (CPU_GUID, "CpuSetup", 24),
        (OTHER_GUID, "F6", 1),
        (PCH_GUID, "PchSetup", 9),
    ];
    let img = with_list(image("7D91", "IMS", "HJ0", fs), list);
    let b = build(&Spec::known("E7D91IMS", "HJ0", standard_records()));
    let p = run(&b, Some(&img), None).unwrap();
    let ev = p
        .evidence
        .iter()
        .find(|e| e.id == format!("ev:rule:{RULE_VARIABLE_INDEX}"))
        .unwrap();
    assert!(
        ev.contradictions
            .iter()
            .any(|c| c.contains("validated binding puts Setup")),
        "{:?}",
        ev.contradictions
    );
    assert!(p.records.iter().all(|r| r.variable.is_none()));
    assert_eq!(rec(&p, "r0").mapping, "confirmed");
}

#[test]
fn the_profile_format_lists_the_variables_and_labels_each_part() {
    let mut list: Vec<(&str, &str, u32)> = TWIN_LIST.to_vec();
    list.push((OTHER_GUID, "OcInfoPassToSetup", 4));
    let img = with_list(image("7E34", "IMSI", "1AF0", twin_formsets()), &list);
    let r = profile_format(&img, false, None).unwrap();
    assert_eq!(r.operation, FORMAT_OPERATION);
    let p = r.payload;
    assert!(!p.traced);
    let l = p.variable_list.as_ref().unwrap();
    assert_eq!(l.entries.len(), 7);
    let alpha = &l.entries[3];
    assert_eq!((alpha.index, alpha.name.as_str()), (3, "Alpha"));
    assert_eq!((alpha.store_size, alpha.field_count), (Some(9), 1));
    assert_eq!(l.entries[2].store_size, None, "no menu reads Lang");
    assert!(l.entries.iter().all(|e| e.saved == "unknown"));
    let part = |n: &str| p.layout.iter().find(|f| f.part == n).unwrap();
    assert_eq!(part("header").basis, "expected");
    assert!(part("header").detail.contains("14 bytes: E7E34IMSI.1AF0"));
    // No module holds $OCI$ in this synthetic image.
    assert_eq!(part("trailer").basis, "expected");
    assert!(p.layout.iter().all(|f| f.part != "skipped"));
    assert!(p.layout.iter().any(|f| f.part == "profile_code"));

    // A traced release: which positions are saved comes from its code.
    let mut traced = with_list(image("7D26", "IMS", "1Q0", twin_formsets()), &list);
    traced.sha256 = crate::firmware::profile_code::TRACED_IMAGES[0].0.into();
    traced
        .profile_modules
        .modules
        .push(crate::firmware::profile_code::ModuleRef {
            name: "OcProfileInfo".into(),
            guid: "A526D2DC-0000-0000-0000-000000000000".into(),
            root: "fv@0x0".into(),
            sha256: "ef".repeat(32),
            mos: true,
            oci: true,
        });
    let p = profile_format(&traced, false, None).unwrap().payload;
    assert!(p.traced);
    let l = p.variable_list.as_ref().unwrap();
    let saved: Vec<&str> = l.entries.iter().map(|e| e.saved.as_str()).collect();
    assert_eq!(
        saved,
        [
            "saved", "saved", "saved", "skipped", "skipped", "saved", "skipped"
        ]
    );
    assert!(
        l.entries[1]
            .note
            .as_deref()
            .unwrap()
            .contains("different size")
    );
    // E7D26 shares HJ0's save/load code but builds $OCI$ with other code.
    let trailer = |p: &ProfileFormatPayload| {
        p.layout
            .iter()
            .find(|f| f.part == "trailer")
            .unwrap()
            .basis
            .clone()
    };
    assert_eq!(trailer(&p), "expected");
    assert!(p.layout.iter().any(|f| f.part == "skipped"));
    let (hj0, _) = crate::firmware::profile_code::TRACED_IMAGES
        .iter()
        .find(|(_, id)| *id == "E7D91IMS.HJ0")
        .unwrap();
    traced.sha256 = (*hj0).into();
    let p = profile_format(&traced, false, None).unwrap().payload;
    assert_eq!(trailer(&p), "firmware_code");
}

#[test]
fn the_profile_format_reports_a_missing_list_and_a_bad_set() {
    let img = image("7E34", "IMSI", "1AF0", twin_formsets());
    let p = profile_format(&img, false, None).unwrap().payload;
    assert!(p.variable_list.is_none());
    assert!(
        p.list_problem
            .as_deref()
            .unwrap()
            .contains("no setup-browser variable list")
    );
    assert!(matches!(
        profile_format(&img, false, Some(1)),
        Err(Error::Request(_))
    ));
}

// ---- baseline comparison and shareable reports

fn run_with_baseline(
    profile: &[u8],
    baseline: &[u8],
    img: &BiosImage,
) -> Result<ExperimentalInspectPayload> {
    let bios = BiosRef {
        image: img,
        profiles_supported: false,
    };
    analyze_with_baseline_on(
        profile,
        Some(bios),
        Some(BaselineSource::same_image(baseline)),
        &AnalyzeOptions::default(),
    )
    .map(|r| r.payload)
}

fn known(records: Vec<Rec>) -> Vec<u8> {
    build(&Spec::known("E7D91IMS", "HJ0", records))
}

#[test]
fn an_applied_baseline_marks_exactly_the_field_whose_bytes_differ() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let main = known(standard_records());
    let mut recs = standard_records();
    recs[0].payload[2] = 0; // Setup switch: Enabled in the profile, Disabled in the baseline
    let base = known(recs);
    let p = run_with_baseline(&main, &base, &img).unwrap();
    let b = p.inputs.baseline.as_ref().unwrap();
    assert_eq!(b.status, "applied");
    assert_eq!(b.sha256, sha256_hex(&base));
    assert_eq!(b.size, base.len());
    let changed: Vec<&str> = p
        .fields
        .iter()
        .filter(|f| f.differs_from_baseline == Some(true))
        .map(|f| f.label.as_str())
        .collect();
    assert_eq!(changed, ["Setup switch"]);
    let f = field(&p, "Setup switch");
    assert_eq!(f.baseline_raw, Some(RawInt::new(0)));
    assert_eq!(f.baseline_label.as_deref(), Some("Disabled"));
    assert_eq!(f.value_label.as_deref(), Some("Enabled"));
    assert_eq!(field(&p, "Cpu switch").differs_from_baseline, Some(false));
    assert_eq!(p.coverage.fields_differing_from_baseline, Some(1));
    assert_eq!(p.coverage.fields_compared_with_baseline, Some(4));
    assert!(p.analysis.rules.iter().any(|r| r == RULE_BASELINE));
    // The main analysis itself is untouched.
    let plain = run(&main, Some(&img), None).unwrap();
    for (a, b) in plain.fields.iter().zip(&p.fields) {
        assert_eq!(
            (&a.key, &a.raw, &a.value_label),
            (&b.key, &b.raw, &b.value_label)
        );
    }
}

#[test]
fn differences_are_split_by_whether_the_baseline_is_at_its_own_default() {
    let mut fs = standard_formsets(0);
    let mut setup_default = vec![0u8; 40]; // switch default Disabled, big number default u64::MAX
    setup_default[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    fs[0].defaults.insert("Setup".into(), setup_default); // no default for PchSetup
    let img = image("7D91", "IMS", "HJ0", fs);
    let main = known(standard_records());
    let mut recs = standard_records();
    recs[0].payload[2] = 0; // switch: baseline at its default
    recs[0].payload[8..16].copy_from_slice(&5u64.to_le_bytes()); // number: baseline off its default
    recs[3].payload[4] = 0x35; // Pch number: the baseline's BIOS has no default
    let p = run_with_baseline(&main, &known(recs), &img).unwrap();
    assert_eq!(field(&p, "Setup switch").baseline_off_default, Some(false));
    assert_eq!(
        field(&p, "Setup big number").baseline_off_default,
        Some(true)
    );
    assert_eq!(field(&p, "Pch number").baseline_off_default, None);
    assert_eq!(field(&p, "Cpu switch").differs_from_baseline, Some(false));
    assert_eq!(p.coverage.fields_differing_from_baseline, Some(3));
    assert_eq!(p.coverage.baseline_differing_at_default, Some(1));
    assert_eq!(p.coverage.baseline_differing_off_default, Some(1));
    assert_eq!(p.coverage.baseline_differing_no_default, Some(1));
    assert!(p.analysis.rules.iter().any(|r| r == RULE_BASELINE_FILLED));
    let ev = p
        .evidence
        .iter()
        .find(|e| e.id.contains(RULE_BASELINE_FILLED))
        .unwrap();
    assert!(ev.limitations[0].contains("saved with the BIOS at its defaults"));
    // Without a baseline there is nothing to split.
    let plain = run(&main, Some(&img), None).unwrap();
    assert!(plain.coverage.baseline_differing_at_default.is_none());
    assert!(
        plain
            .fields
            .iter()
            .all(|f| f.baseline_off_default.is_none())
    );
}

#[test]
fn a_field_present_on_one_side_only_is_not_compared() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let main = known(standard_records());
    let mut recs = standard_records();
    recs.retain(|r| r.id != 7); // no PchSetup record in the baseline
    let p = run_with_baseline(&main, &known(recs), &img).unwrap();
    assert_eq!(p.inputs.baseline.as_ref().unwrap().status, "applied");
    let f = field(&p, "Pch number");
    assert_eq!(f.differs_from_baseline, None);
    assert_eq!(f.baseline_raw, None);
    assert_eq!(p.coverage.fields_compared_with_baseline, Some(3));
    assert_eq!(p.coverage.fields_differing_from_baseline, Some(0));
}

#[test]
fn an_unusable_baseline_is_blocked_and_the_main_result_is_unchanged() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let main = known(standard_records());
    let plain = serde_json::to_value(run(&main, Some(&img), None).unwrap()).unwrap();
    let wrong_id = build(&Spec::known("E7D91IMS", "HI0", standard_records()));
    let mut bad_sum = known(standard_records());
    bad_sum[0x19 + 6 + 2] ^= 0xFF;
    for base in [wrong_id, bad_sum, b"not a profile".to_vec()] {
        let p = run_with_baseline(&main, &base, &img).unwrap();
        let b = p.inputs.baseline.as_ref().unwrap();
        assert_eq!(b.status, "blocked", "{}", b.reason);
        assert!(!b.reason.is_empty());
        assert!(
            p.status_reasons
                .iter()
                .any(|r| r.starts_with("Baseline not applied"))
        );
        assert_eq!(p.coverage.fields_compared_with_baseline, None);
        assert!(p.fields.iter().all(|f| {
            f.baseline_raw.is_none()
                && f.baseline_label.is_none()
                && f.differs_from_baseline.is_none()
        }));
        let mut now = serde_json::to_value(&p).unwrap();
        for key in [
            "fields", "records", "stores", "evidence", "coverage", "identity",
        ] {
            assert_eq!(now[key], plain[key], "{key}");
        }
        now["inputs"].as_object_mut().unwrap().remove("baseline");
        assert_eq!(now["inputs"], plain["inputs"]);
    }
}

#[test]
fn a_baseline_needs_a_bios() {
    let main = known(standard_records());
    let r = analyze_with_baseline_on(
        &main,
        None,
        Some(BaselineSource::same_image(&main)),
        &AnalyzeOptions::default(),
    );
    assert!(matches!(r, Err(Error::Request(_))));
}

#[test]
fn without_a_baseline_no_baseline_key_appears() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let p = run(&known(standard_records()), Some(&img), None).unwrap();
    let json = String::from_utf8(to_json_bounded(&p, 1 << 22).unwrap()).unwrap();
    assert!(!json.contains("baseline"));
    assert!(!json.contains("shareable"));
    assert!(!json.contains("preview_omitted"));
}

#[test]
fn a_shareable_report_drops_the_cpu_brand_the_save_time_and_every_preview() {
    let img = image("7D91", "IMS", "HJ0", standard_formsets(0));
    let mut p = run(&known(standard_records()), Some(&img), None).unwrap();
    let before = String::from_utf8(to_json_bounded(&p, 1 << 22).unwrap()).unwrap();
    assert!(before.contains("Test CPU") && before.contains("20261001120000"));
    let settings = p.fields.clone();
    make_shareable(&mut p);
    let json = String::from_utf8(to_json_bounded(&p, 1 << 22).unwrap()).unwrap();
    assert!(!json.contains("Test CPU"), "{json}");
    assert!(!json.contains("20261001120000"));
    assert!(p.analysis.shareable && !p.analysis.redactions.is_empty());
    assert!(
        p.records
            .iter()
            .all(|r| r.preview_hex.is_empty() && r.preview_omitted)
    );
    // Board and revision stay, and so do the interpreted settings.
    assert_eq!(p.container_candidates[0].trailers[0].strings[0], "E7D91IMS");
    assert_eq!(p.fields.len(), settings.len());
    assert_eq!(
        field(&p, "Setup switch").value_label.as_deref(),
        Some("Enabled")
    );
}

// ---- baseline saved on another release of the same board

/// One release: the Setup store is `setup_size` bytes with the switch at `at` (options as given)
/// and a number 8 bytes later; CpuSetup holds a checkbox. Offsets and option numbers differ
/// between releases the way they do in real firmware.
fn release(
    rev: &'static str,
    setup_size: u16,
    at: u16,
    opts: Vec<OptionItem>,
    switch: u8,
) -> (BiosImage, Vec<u8>) {
    let mut sw = q(1, "Setup", at, 8, "Setup switch", QuestionKind::OneOf);
    sw.options = opts;
    let fs = formset(
        "FS-MAIN",
        vec![
            store(1, SETUP_GUID, "Setup", setup_size),
            store(2, CPU_GUID, "CpuSetup", 24),
        ],
        vec![
            sw,
            q(
                1,
                "Setup",
                at + 8,
                16,
                "Setup number",
                QuestionKind::Numeric,
            ),
            q(2, "CpuSetup", 0, 8, "Cpu switch", QuestionKind::Checkbox),
        ],
        0,
    );
    let mut img = image("7D91", "IMS", rev, vec![fs]);
    img.sha256 = format!("{:02x}", rev.bytes().map(u32::from).sum::<u32>() % 256).repeat(32);
    let mut setup = vec![0u8; usize::from(setup_size)];
    setup[usize::from(at)] = switch;
    setup[usize::from(at) + 8..usize::from(at) + 10].copy_from_slice(&0x0777u16.to_le_bytes());
    let mut cpu = vec![0u8; 24];
    cpu[0] = 1;
    let profile = build(&Spec::known(
        "E7D91IMS",
        rev,
        vec![
            Rec {
                id: 0,
                payload: setup,
            },
            Rec {
                id: 1,
                payload: vec![],
            },
            Rec {
                id: 5,
                payload: cpu,
            },
        ],
    ));
    (img, profile)
}

fn std_opts() -> Vec<OptionItem> {
    vec![opt(0, "Disabled"), opt(1, "Enabled"), opt(2, "Auto")]
}

fn cross(
    main: (&BiosImage, &[u8]),
    base: (&BiosImage, &[u8]),
    base_set: Option<usize>,
) -> ExperimentalInspectPayload {
    let r = |image| BiosRef {
        image,
        profiles_supported: false,
    };
    analyze_with_baseline_on(
        main.1,
        Some(r(main.0)),
        Some(BaselineSource {
            profile: base.1,
            bios: Some(r(base.0)),
            set: base_set,
        }),
        &AnalyzeOptions::default(),
    )
    .unwrap()
    .payload
}

#[test]
fn a_cross_version_baseline_matches_by_menu_place_when_offsets_differ() {
    let (i1, p1) = release("HI0", 40, 2, std_opts(), 1); // Enabled
    let (i2, p2) = release("HJ0", 56, 18, std_opts(), 0); // Disabled, every offset moved
    let p = cross((&i1, &p1), (&i2, &p2), None);
    let b = p.inputs.baseline.as_ref().unwrap();
    assert_eq!(
        (b.status.as_str(), b.mode.as_str()),
        ("applied", "cross_version")
    );
    let bb = b.bios.as_ref().unwrap();
    assert_eq!(
        (bb.bios_id.as_str(), bb.sha256.as_str()),
        ("E7D91IMS.HJ0", i2.sha256.as_str())
    );
    assert_eq!(bb.size, i2.size);
    assert!(b.reason.contains("menu place and name") && b.reason.contains("only one version"));
    let differing: Vec<&str> = p
        .fields
        .iter()
        .filter(|f| f.differs_from_baseline == Some(true))
        .map(|f| f.label.as_str())
        .collect();
    assert_eq!(differing, ["Setup switch"]);
    let f = field(&p, "Setup switch");
    assert_eq!(f.baseline_match.as_deref(), Some("menu_place"));
    assert_eq!(f.baseline_raw, Some(RawInt::new(0)));
    assert_eq!(f.baseline_label.as_deref(), Some("Disabled"));
    assert_eq!(field(&p, "Setup number").differs_from_baseline, Some(false));
    assert_eq!(p.coverage.fields_compared_with_baseline, Some(3));
    assert_eq!(p.coverage.fields_differing_from_baseline, Some(1));
    assert_eq!(p.coverage.fields_not_compared_with_baseline, Some(0));
    assert!(p.analysis.rules.iter().any(|r| r == RULE_BASELINE_CROSS));
    assert!(p.analysis.rules.iter().any(|r| r == RULE_BASELINE_FILLED));
    // The synthetic images carry no defaults: nothing is known about the baseline side.
    assert_eq!(f.baseline_off_default, None);
    assert_eq!(p.coverage.baseline_differing_no_default, Some(1));
    assert_eq!(p.coverage.baseline_differing_at_default, Some(0));
    assert!(
        p.evidence
            .iter()
            .any(|e| e.id.contains(RULE_BASELINE_CROSS) && e.basis == "file_comparison")
    );
    assert!(!p.analysis.rules.iter().any(|r| r == RULE_BASELINE));
    assert!(
        p.evidence
            .iter()
            .any(|e| e.id.contains(RULE_BASELINE_CROSS))
    );
    assert!(
        p.status_reasons
            .iter()
            .any(|r| r.contains("differ from the baseline"))
    );
    // Cross-version reports carry no CPU brand or save time once shareable.
    let mut shared = p.clone();
    make_shareable(&mut shared);
    let json = String::from_utf8(to_json_bounded(&shared, 1 << 22).unwrap()).unwrap();
    assert!(!json.contains("Test CPU") && !json.contains("20261001120000"));
}

#[test]
fn a_renumbered_option_with_the_same_label_is_not_a_difference() {
    let (i1, p1) = release("HI0", 40, 2, std_opts(), 1); // Enabled = 1
    let renumbered = vec![opt(5, "Disabled"), opt(6, "Enabled"), opt(7, "Auto")];
    let (i2, p2) = release("HJ0", 40, 2, renumbered, 6); // Enabled = 6
    let p = cross((&i1, &p1), (&i2, &p2), None);
    let f = field(&p, "Setup switch");
    assert_eq!(f.raw, Some(RawInt::new(1)));
    assert_eq!(f.baseline_raw, Some(RawInt::new(6)));
    assert_eq!(f.differs_from_baseline, Some(false));
    assert_eq!(p.coverage.fields_differing_from_baseline, Some(0));
}

#[test]
fn a_repeated_menu_place_is_not_compared() {
    let (i1, p1) = release("HI0", 40, 2, std_opts(), 1);
    let (mut i2, p2) = release("HJ0", 56, 18, std_opts(), 0);
    // The baseline's BIOS shows a second setting under the same place and name.
    let mut twin = i2.formsets[0].questions[0].clone();
    twin.offset = 30;
    twin.question_id = 30;
    twin.ifr_offset = 1030;
    i2.formsets[0].questions.push(twin);
    let p = cross((&i1, &p1), (&i2, &p2), None);
    let f = field(&p, "Setup switch");
    assert_eq!(f.differs_from_baseline, None);
    assert_eq!(f.baseline_raw, None);
    assert_eq!(f.baseline_match, None);
    assert_eq!(p.coverage.fields_compared_with_baseline, Some(2));
    assert_eq!(p.coverage.fields_not_compared_with_baseline, Some(1));
    // The same on the profile's side.
    let (mut j1, q1) = release("HI0", 40, 2, std_opts(), 1);
    let (j2, q2) = release("HJ0", 56, 18, std_opts(), 0);
    let mut twin = j1.formsets[0].questions[0].clone();
    twin.offset = 20;
    twin.question_id = 20;
    twin.ifr_offset = 1020;
    j1.formsets[0].questions.push(twin);
    let p = cross((&j1, &q1), (&j2, &q2), None);
    assert!(
        p.fields
            .iter()
            .filter(|f| f.label == "Setup switch")
            .all(|f| f.differs_from_baseline.is_none())
    );
    assert_eq!(p.coverage.fields_not_compared_with_baseline, Some(2));
}

#[test]
fn a_baseline_from_another_board_or_family_is_blocked_and_the_main_result_is_unchanged() {
    let (i1, p1) = release("HI0", 40, 2, std_opts(), 1);
    let plain = serde_json::to_value(run(&p1, Some(&i1), None).unwrap()).unwrap();
    let (same, _) = release("HJ0", 40, 2, std_opts(), 0);
    let mut other_board = image("7E34", "IMS", "HJ0", same.formsets.clone());
    other_board.sha256 = "77".repeat(32);
    // The profile names the other board too.
    let mut recs = standard_records();
    recs.truncate(1);
    let pb = build(&Spec::known("E7E34IMS", "HJ0", recs));
    let (mut other_family, pf) = release("HJ0", 40, 2, std_opts(), 0);
    other_family.platform.family = "Another family".into();
    for (img, prof, why) in [
        (&other_board, &pb, "not the same board"),
        (&other_family, &pf, "platform family"),
    ] {
        let p = cross((&i1, &p1), (img, prof), None);
        let b = p.inputs.baseline.as_ref().unwrap();
        assert_eq!(b.status, "blocked", "{}", b.reason);
        assert!(b.reason.contains(why), "{}", b.reason);
        assert_eq!(b.mode, "cross_version");
        assert_eq!(p.coverage.fields_compared_with_baseline, None);
        assert!(
            p.fields
                .iter()
                .all(|f| f.baseline_match.is_none() && f.differs_from_baseline.is_none())
        );
        let mut now = serde_json::to_value(&p).unwrap();
        for key in [
            "fields", "records", "stores", "evidence", "coverage", "identity",
        ] {
            assert_eq!(now[key], plain[key], "{key}");
        }
        now["inputs"].as_object_mut().unwrap().remove("baseline");
        assert_eq!(now["inputs"], plain["inputs"]);
    }
}

#[test]
fn a_baseline_that_fails_its_own_gates_on_its_image_is_blocked() {
    let (i1, p1) = release("HI0", 40, 2, std_opts(), 1);
    let (i2, p2) = release("HJ0", 56, 18, std_opts(), 0);
    let mut bad_sum = p2.clone();
    bad_sum[0x19 + 6 + 18] ^= 0xFF;
    // Saved on HI0, read against the HJ0 image: identity does not match.
    let (_, wrong_id) = release("HI0", 40, 2, std_opts(), 0);
    for base in [bad_sum, wrong_id, b"not a profile".to_vec()] {
        let p = cross((&i1, &p1), (&i2, &base), None);
        let b = p.inputs.baseline.as_ref().unwrap();
        assert_eq!(b.status, "blocked", "{}", b.reason);
        assert!(p.fields.iter().all(|f| f.differs_from_baseline.is_none()));
    }
    // A firmware set the baseline's image does not have is a blocked baseline too.
    let p = cross((&i1, &p1), (&i2, &p2), Some(3));
    assert_eq!(p.inputs.baseline.as_ref().unwrap().status, "blocked");
}

#[test]
fn the_same_image_digest_keeps_the_same_image_mode() {
    let (i1, p1) = release("HI0", 40, 2, std_opts(), 1);
    let (_, p2) = release("HI0", 40, 2, std_opts(), 0);
    let p = cross((&i1, &p1), (&i1.clone(), &p2), None);
    let b = p.inputs.baseline.as_ref().unwrap();
    assert_eq!(
        (b.status.as_str(), b.mode.as_str()),
        ("applied", "same_image")
    );
    assert!(b.bios.is_none());
    assert_eq!(p.coverage.fields_not_compared_with_baseline, None);
    assert!(p.analysis.rules.iter().any(|r| r == RULE_BASELINE));
    let f = field(&p, "Setup switch");
    assert_eq!(f.differs_from_baseline, Some(true));
    assert_eq!(f.baseline_match.as_deref(), Some("key"));
}
