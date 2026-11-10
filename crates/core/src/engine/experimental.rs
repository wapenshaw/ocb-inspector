//! Experimental, read-only profile analysis (docs/EXPERIMENTAL_PROFILE_INSPECTION.md).
//!
//! Interprets an OCB file that the validated reader does not support, using only the file's own
//! structure and the BIOS's own variable stores and questions. Everything here is display only:
//! the result is an immutable report, never a `Session`, patch, plan or output file, and no
//! function in this module can reach a writer. Inferred mappings stay labelled as inferred.

use crate::bytes::{hex, sha256_hex, uint_at};
use crate::contracts::*;
use crate::error::{Error, Result};
use crate::firmware::ifr::{Question, QuestionKind, StoreKind};
use crate::firmware::profile_code;
use crate::firmware::spf::{ListedVariable, VariableList};
use crate::firmware::{BiosImage, Formset, parse_bios_id, root_of};
use crate::format::probe::{
    self, Budget, ContainerCandidate, KNOWN_ID_LEN, ProbeLimits, RULE_KNOWN,
};
use std::collections::{BTreeMap, BTreeSet};

pub const ANALYZER: &str = "ocb-experimental";
pub const ANALYZER_VERSION: &str = "1";
pub const OPERATION: &str = "experimental_inspect";

/// UEFI identifies a variable by vendor GUID + name (UEFI 2.x, GetVariable). Within one
/// firmware set, buffer stores with the same GUID, name and size declared by different
/// formsets are the same variable; the same GUID and name with different sizes is a conflict.
pub const RULE_UEFI_IDENTITY: &str = "uefi_variable_identity/1";
/// Additive checksum: the i32 at the checksum offset equals minus the sum of every other byte.
/// Established for the known layout on MS-7D91; elsewhere a trial only.
pub const RULE_CHECKSUM: &str = "additive_checksum/1";
/// The validated MS-7D91 binding (`Session::pair`): Setup and CpuSetup each match exactly one
/// record by size, and Setup is the first record. Confirmed only within that established scope.
pub const RULE_7D91_BINDING: &str = "adapter_7d91_binding/1";
/// Generic inference: a record and a buffer store whose sizes match nothing else.
pub const RULE_UNIQUE_SIZE: &str = "unique_size_pair/1";
/// Firmware-derived: a record id is the position of its variable in the setup browser's
/// variable list (`$SPF` setup data) of the release that saved the file. Recovered from that
/// browser's OC-profile save and load code (docs/FIRMWARE_PROFILE_FORMAT_RESEARCH.md); applied
/// only when the list and the file check against each other.
pub const RULE_VARIABLE_INDEX: &str = "amitse_variable_index/1";
/// Baseline comparison: the same pipeline on a second profile; a field differs when the two
/// files store different bytes for it. Says nothing about who changed what.
pub const RULE_BASELINE: &str = "baseline_compare/1";
pub const RULE_BASELINE_CROSS: &str = "baseline_compare_cross/1";
pub const RULE_BASELINE_FILLED: &str = "baseline_firmware_filled/1";
/// Board whose profiles established the known layout, checksum and binding.
const ESTABLISHED_BOARD: &str = "7D91";
const PREVIEW_BYTES: usize = 64;

#[derive(Debug, Clone, Default)]
pub struct AnalyzeOptions {
    /// Firmware set to analyse. Required (for BIOS interpretation) when the image has several.
    pub set: Option<usize>,
    pub limits: ProbeLimits,
}

/// A BIOS image for the analysis, with whether the validated reader supports it (unchanged).
#[derive(Clone, Copy)]
pub struct BiosRef<'a> {
    pub image: &'a BiosImage,
    pub profiles_supported: bool,
}

impl<'a> BiosRef<'a> {
    /// An image with the support flag its decode schema (if it has one) reports.
    pub fn new<S: std::borrow::Borrow<crate::firmware::BiosSchema>>(
        image: &'a BiosImage,
        schema: Option<&S>,
    ) -> Self {
        Self {
            image,
            profiles_supported: schema.is_some_and(|s| s.borrow().platform.profiles_supported),
        }
    }
}

/// The profile's identity as its contents give it.
#[derive(Debug, Clone)]
struct ProfileId {
    status: &'static str, // resolved | unknown | conflicting
    id: Option<(String, String, String)>,
    reason: String,
    observed: Vec<IdentifierObservation>,
    limitations: Vec<String>,
}

fn observation(source: &str, text: &str, spans: Vec<Range>) -> IdentifierObservation {
    let parts = parse_bios_id(text);
    IdentifierObservation {
        source: source.into(),
        text: text.into(),
        spans,
        board: parts.as_ref().map(|p| p.0.clone()),
        flavor: parts.as_ref().map(|p| p.1.clone()),
        revision: parts.map(|p| p.2),
    }
}

fn full_id(p: &(String, String, String)) -> String {
    format!("E{}{}.{}", p.0, p.1, p.2)
}

/// Identity of one container candidate from its header identifier and `$OCI$` strings.
fn candidate_identity(c: &ContainerCandidate) -> ProfileId {
    let mut observed = vec![observation(
        "header",
        &c.header_id,
        vec![Range::new(c.id_span.0, c.id_span.1)],
    )];
    let header = parse_bios_id(&c.header_id);
    let oci = c.trailer().and_then(|t| {
        let text = format!("{}.{}", t.strings[0], t.strings[1]);
        observed.push(observation(
            "oci",
            &text,
            vec![
                Range::new(t.spans[0].0, t.spans[0].1),
                Range::new(t.spans[1].0, t.spans[1].1),
            ],
        ));
        parse_bios_id(&text).map(|p| (p, text))
    });
    let mut limitations = vec![
        "Identifiers say which BIOS saved the file; they are not a cryptographic proof".into(),
    ];
    let (status, id, reason) = match (header, oci) {
        (Some(h), Some((o, _))) if h == o => ("resolved", Some(h), "header and $OCI$ agree".into()),
        (Some(h), Some((o, _))) => (
            "conflicting",
            None,
            format!("header says {} but $OCI$ says {}", full_id(&h), full_id(&o)),
        ),
        // A 12-byte header can only hold the first 12 characters of a longer identifier.
        (None, Some((o, text)))
            if c.header_id.len() == KNOWN_ID_LEN
                && text.len() > KNOWN_ID_LEN
                && text.starts_with(&c.header_id) =>
        {
            limitations.push(format!(
                "The header holds only the first {KNOWN_ID_LEN} characters of {text}; identity rests on $OCI$"
            ));
            (
                "resolved",
                Some(o),
                "$OCI$ gives the identifier; the header holds its first 12 characters".into(),
            )
        }
        (None, Some((o, _))) => (
            "conflicting",
            None,
            format!(
                "header text {:?} is not an identifier and does not agree with $OCI$ {}",
                c.header_id,
                full_id(&o)
            ),
        ),
        (Some(h), None) => {
            limitations.push("No $OCI$ identifier to cross-check the header".into());
            ("resolved", Some(h), "header identifier only".into())
        }
        (None, None) => (
            "unknown",
            None,
            "no identifier in the header or $OCI$ parses as an MSI BIOS id".into(),
        ),
    };
    ProfileId {
        status,
        id,
        reason,
        observed,
        limitations,
    }
}

fn established(c: &ContainerCandidate, id: &ProfileId) -> bool {
    c.rule == RULE_KNOWN
        && c.all_checks_pass()
        && id.id.as_ref().is_some_and(|p| p.0 == ESTABLISHED_BOARD)
}

fn integrity(
    bytes: &[u8],
    c: &ContainerCandidate,
    established: bool,
    traced: bool,
) -> IntegrityFinding {
    let stored = probe::stored_i32(bytes, c.checksum_at);
    let computed = probe::additive_checksum(bytes, c.checksum_at);
    let same = stored.is_some() && stored == computed;
    let (status, mut limitations) = if established {
        (
            if same { "verified" } else { "failed" },
            vec!["Consistency with the established rule; not proof of BIOS acceptance".to_string()],
        )
    } else {
        (
            if same { "candidate_match" } else { "unknown" },
            vec![if same {
                "The known additive relation holds at this offset; weak evidence, not authenticity"
                    .to_string()
            } else if traced {
                "The checksum does not match; this release's traced profile loader rejects such a file".to_string()
            } else {
                "The known additive relation does not hold here; this board may use another algorithm, so this is not a sign of corruption".to_string()
            }],
        )
    };
    if !established {
        limitations.push(format!(
            "The rule is established only for MS-{ESTABLISHED_BOARD} profiles in the known layout"
        ));
    }
    IntegrityFinding {
        status: status.into(),
        rule: Some(RULE_CHECKSUM.into()),
        scope: format!("known layout, MS-{ESTABLISHED_BOARD}"),
        checksum_at: Some(c.checksum_at),
        stored: stored.map(|v| v.to_string()),
        computed: computed.map(|v| v.to_string()),
        limitations,
    }
}

/// Top-level volumes of one firmware set; `None` when the image has a single set (every volume).
pub(crate) fn set_roots(img: &BiosImage, set: usize) -> Option<BTreeSet<&str>> {
    (img.sets.len() > 1).then(|| {
        img.formsets
            .iter()
            .filter(|f| f.set == set)
            .map(|f| root_of(&f.module.container_path))
            .collect()
    })
}

/// Whether the image's code for this set builds the `$OCI$` trailer.
fn writes_trailer(img: &BiosImage, set: usize) -> bool {
    img.profile_modules
        .has_trailer_in(set_roots(img, set).as_ref())
}

/// The setup browser's variable list for one firmware set: the lists in the set's top-level
/// volumes (any list when the image has one set), which must all be the same list.
pub(crate) fn variable_list(
    img: &BiosImage,
    set: usize,
) -> std::result::Result<&VariableList, String> {
    let roots = set_roots(img, set);
    let lists: Vec<&VariableList> = img
        .variable_lists
        .iter()
        .filter(|l| roots.as_ref().is_none_or(|r| r.contains(l.root.as_str())))
        .collect();
    match lists.as_slice() {
        [] => Err("this BIOS image has no setup-browser variable list for the set".into()),
        [first, rest @ ..] if rest.iter().all(|l| l.sha256 == first.sha256) => Ok(first),
        many => Err(format!(
            "{} different variable lists apply to this set; none is chosen",
            many.len()
        )),
    }
}

/// Outcome of `amitse_variable_index/1` for the selected records.
struct IndexRule<'a> {
    /// The list entry per record, when the rule applies.
    entries: Vec<Option<&'a ListedVariable>>,
    observations: Vec<String>,
    contradictions: Vec<String>,
    limitations: Vec<String>,
}

impl IndexRule<'_> {
    fn applied(&self) -> bool {
        self.contradictions.is_empty() && self.entries.iter().any(Option::is_some)
    }
}

/// Checks the list against the file and the BIOS's stores: every id must be in the list, and
/// most records whose listed variable the menus declare as a buffer store must have that
/// store's size (records that differ are left undecoded). Otherwise the whole rule is refused;
/// nothing is adjusted to fit.
fn index_rule<'a>(
    img: &'a BiosImage,
    set: usize,
    records: &[probe::ChainRecord],
    vars: &[Var],
    bound: &[(usize, usize)],
) -> IndexRule<'a> {
    let traced = profile_code::traced(&img.sha256);
    let mut r = IndexRule {
        entries: vec![None; records.len()],
        observations: Vec::new(),
        contradictions: Vec::new(),
        limitations: vec![
            if traced {
                "The OC-profile save and load code of this release was traced, or is the same code as a traced release"
            } else {
                "Applied because this image has the same setup-data variable list; the OC-profile save and load code of this release was not traced"
            }
            .into(),
            "Exported profiles have been checked against this rule only on MS-7D91".into(),
        ],
    };
    let list = match variable_list(img, set) {
        Ok(l) => l,
        Err(e) => {
            r.contradictions.push(e);
            return r;
        }
    };
    // The traced save writes records in list order, so ids strictly increase.
    if !records.windows(2).all(|w| w[0].id < w[1].id) {
        r.contradictions.push(
            "record ids do not strictly increase, unlike the list order the save writes".into(),
        );
    }
    r.observations.push(format!(
        "{} variables in the setup data at {} (list sha256 {})",
        list.entries.len(),
        list.container_path,
        &list.sha256[..16]
    ));
    let mut agree = 0usize;
    let mut differ = Vec::new();
    let mut store_checked = 0usize;
    let mut store_differ = Vec::new();
    for (ri, rec) in records.iter().enumerate() {
        let Some(e) = list.entries.get(rec.id as usize) else {
            r.contradictions.push(format!(
                "record id {} is outside the {}-entry variable list",
                rec.id,
                list.entries.len()
            ));
            continue;
        };
        r.entries[ri] = Some(e);
        if e.size > 0 && rec.size > 0 {
            if e.size as usize == rec.size {
                agree += 1;
            } else {
                differ.push(format!(
                    "{} (declared {}, record {})",
                    e.name, e.size, rec.size
                ));
            }
        }
        let stores: Vec<&Var> = vars
            .iter()
            .filter(|v| v.kind == StoreKind::Buffer && v.guid == e.guid && v.name == e.name)
            .collect();
        if rec.size > 0 && !stores.is_empty() {
            store_checked += 1;
            if !stores.iter().any(|v| usize::from(v.size) == rec.size) {
                store_differ.push(format!(
                    "{} (store {}, record {})",
                    e.name,
                    stores
                        .iter()
                        .map(|v| v.size.to_string())
                        .collect::<Vec<_>>()
                        .join("/"),
                    rec.size
                ));
            }
        }
    }
    // A reviewed binding and the list must name the same variable.
    for &(ri, vi) in bound {
        if let (Some(Some(e)), Some(v)) = (r.entries.get(ri), vars.get(vi))
            && (e.guid != v.guid || e.name != v.name)
        {
            r.contradictions.push(format!(
                "the validated binding puts {} at record {} but the variable list names {}",
                v.name, records[ri].id, e.name
            ));
        }
    }
    // Some variables are sized at run time (the save writes the live size), so a few records
    // may differ from their store. A wrong or shifted list disagrees on most of them.
    let store_agree = store_checked - store_differ.len();
    if r.contradictions.is_empty() {
        if store_checked == 0 {
            r.contradictions
                .push("no record could be checked against a store the BIOS menus declare".into());
        } else if store_agree <= store_differ.len() {
            r.contradictions.push(format!(
                "only {store_agree} of {store_checked} records have the size of the BIOS store the list names"
            ));
        }
    }
    r.observations.push(format!(
        "{store_agree} of {store_checked} records have the size of the BIOS store the list names{}{}",
        if store_differ.is_empty() { "" } else { "; not decoded: " },
        store_differ.join(", ")
    ));
    r.observations.push(format!(
        "declared sizes agree for {agree} records; {} differ ({}): {}",
        differ.len(),
        if traced {
            "variables sized at run time"
        } else {
            "expected for variables sized at run time; not traced for this release"
        },
        differ.join(", ")
    ));
    if !r.contradictions.is_empty() {
        r.entries = vec![None; records.len()];
    }
    r
}

/// One UEFI variable in the selected set: every formset's declaration of it.
#[derive(Debug)]
struct Var {
    key: String,
    name: String,
    guid: String,
    kind: StoreKind,
    size: u16,
    /// (formset index in image, local store id)
    decls: Vec<(usize, u16)>,
    support: String,
}

impl Var {
    fn supported(&self) -> bool {
        self.support == "supported"
    }
}

fn kind_name(k: &StoreKind) -> &'static str {
    match k {
        StoreKind::Buffer => "buffer",
        StoreKind::Efi => "efi",
        StoreKind::NameValue => "name_value",
    }
}

/// Variables of one set, merged by the UEFI identity rule. Deterministic order.
fn inventory(img: &BiosImage, set: usize) -> Vec<Var> {
    let mut by_identity: BTreeMap<(String, String, &'static str, u16), Var> = BTreeMap::new();
    for (fi, fs) in img
        .formsets
        .iter()
        .enumerate()
        .filter(|(_, f)| f.set == set)
    {
        for st in &fs.stores {
            let guid = st.guid.to_uppercase();
            let kind = kind_name(&st.kind);
            let id = (guid.clone(), st.name.clone(), kind, st.size);
            by_identity
                .entry(id)
                .or_insert_with(|| Var {
                    key: format!("set{set}:{guid}:{}:{}", st.name, st.size),
                    name: st.name.clone(),
                    guid: guid.clone(),
                    kind: st.kind.clone(),
                    size: st.size,
                    decls: Vec::new(),
                    support: String::new(),
                })
                .decls
                .push((fi, st.id));
        }
    }
    let mut sizes: BTreeMap<(String, String), BTreeSet<u16>> = BTreeMap::new();
    for v in by_identity.values() {
        sizes
            .entry((v.guid.clone(), v.name.clone()))
            .or_default()
            .insert(v.size);
    }
    let mut out: Vec<Var> = by_identity.into_values().collect();
    for v in &mut out {
        v.support = if v.kind != StoreKind::Buffer {
            format!(
                "{} stores are not mapped by this analysis",
                kind_name(&v.kind)
            )
        } else if v.name.is_empty() || v.size == 0 {
            "no name or no size".into()
        } else if sizes[&(v.guid.clone(), v.name.clone())].len() > 1 {
            format!(
                "declared with different sizes ({}) in this set",
                sizes[&(v.guid.clone(), v.name.clone())]
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            "supported".into()
        };
    }
    out
}

/// Named, deduplicated questions of a variable: one per (offset, width, prompt), preferring a
/// copy the BIOS shows.
fn questions<'a>(img: &'a BiosImage, v: &Var) -> Vec<(&'a Formset, &'a Question)> {
    let mut rows: BTreeMap<(u16, u16, String), (&Formset, &Question)> = BTreeMap::new();
    for &(fi, local) in &v.decls {
        let fs = &img.formsets[fi];
        for q in fs.questions.iter().filter(|q| q.store_id == local) {
            let prompt = q.prompt.trim();
            if prompt.is_empty() {
                continue;
            }
            let key = (q.offset, q.width_bits, prompt.to_string());
            match rows.get(&key) {
                Some((_, prev)) if !(prev.never_shown() && !q.never_shown()) => {}
                _ => {
                    rows.insert(key, (fs, q));
                }
            }
        }
    }
    rows.into_values().collect()
}

/// Optimized defaults of a variable: the copies its formsets carry must agree.
fn var_defaults(img: &BiosImage, v: &Var) -> (Option<Vec<u8>>, Option<String>) {
    let copies: BTreeSet<&Vec<u8>> = v
        .decls
        .iter()
        .filter_map(|(fi, _)| img.formsets[*fi].defaults.get(&v.name))
        .filter(|d| d.len() == usize::from(v.size))
        .collect();
    match copies.len() {
        0 => (None, None),
        1 => (copies.into_iter().next().cloned(), None),
        _ => (
            None,
            Some("formsets carry different optimized defaults for this variable".into()),
        ),
    }
}

#[derive(Debug, Default, Clone)]
struct EdgeStats {
    fields: usize,
    fits: usize,
    enum_known: usize,
    enum_unknown: usize,
    default_matches: usize,
    with_default: usize,
}

fn edge_stats(
    bytes: &[u8],
    payload_at: usize,
    v: &Var,
    qs: &[(&Formset, &Question)],
    defaults: Option<&[u8]>,
    budget: &mut Budget,
) -> Result<EdgeStats> {
    let mut s = EdgeStats::default();
    let mut seen: BTreeSet<(u16, u16)> = BTreeSet::new();
    for (_, q) in qs {
        budget.field()?;
        if !seen.insert((q.offset, q.width_bits)) {
            continue; // aliases of one span count once
        }
        s.fields += 1;
        let w = usize::from(q.width_bits / 8);
        if !q.width_bits.is_multiple_of(8) || !matches!(w, 1 | 2 | 4 | 8) {
            continue;
        }
        let off = usize::from(q.offset);
        if off + w > usize::from(v.size) {
            continue;
        }
        s.fits += 1;
        let Ok(raw) = uint_at(bytes, payload_at + off, w, "field") else {
            continue;
        };
        if q.kind == QuestionKind::OneOf {
            if q.options.iter().any(|o| o.value == raw) {
                s.enum_known += 1;
            } else {
                s.enum_unknown += 1;
            }
        }
        if let Some(d) = defaults.and_then(|d| uint_at(d, off, w, "default").ok()) {
            s.with_default += 1;
            s.default_matches += usize::from(d == raw);
        }
    }
    Ok(s)
}

fn stats_text(s: &EdgeStats) -> String {
    format!(
        "{} fields, {} fit; {} one-of values match an option, {} do not; {} of {} equal the optimized default",
        s.fields, s.fits, s.enum_known, s.enum_unknown, s.default_matches, s.with_default
    )
}

/// Write a report as JSON, refusing to grow past `limit` bytes.
pub fn to_json_bounded<T: serde::Serialize>(value: &T, limit: usize) -> Result<Vec<u8>> {
    struct Capped {
        buf: Vec<u8>,
        limit: usize,
    }
    impl std::io::Write for Capped {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.buf.len() + b.len() > self.limit {
                return Err(std::io::Error::other("limit"));
            }
            self.buf.extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut w = Capped {
        buf: Vec::new(),
        limit,
    };
    serde_json::to_writer_pretty(&mut w, value)
        .map_err(|_| Error::Limit(format!("analysis JSON would exceed {limit} bytes")))?;
    w.buf.push(b'\n');
    Ok(w.buf)
}

fn candidate_info(
    c: &ContainerCandidate,
    eligible: bool,
    contradictions: Vec<String>,
    integrity: IntegrityFinding,
) -> ContainerCandidateInfo {
    ContainerCandidateInfo {
        id: c.id.clone(),
        rule: format!("{}/{}", c.rule, c.rule_version),
        assumptions: c.assumptions.clone(),
        header_id: c.header_id.clone(),
        header_id_span: Range::new(c.id_span.0, c.id_span.1),
        checksum_at: c.checksum_at,
        count_at: c.count_at,
        records_at: c.records_at,
        record_count: c.count,
        records_span: c.chain_end.map(|e| Range::new(c.records_at, e)),
        chain_error: c.chain_error.clone(),
        trailers: c
            .trailers
            .iter()
            .map(|t| TrailerInfo {
                at: t.at,
                gap: t.gap,
                strings: t.strings.clone(),
                spans: t.spans.iter().map(|s| Range::new(s.0, s.1)).collect(),
                tail_len: t.tail_len,
                timestamp_valid: t.timestamp_valid,
            })
            .collect(),
        checks: c.checks.clone(),
        eligible,
        contradictions,
        integrity,
    }
}

/// Analyse `profile` (and, when given, interpret it against `bios`). Never fails on an
/// unfamiliar or mismatched file: those are findings. Fails on invalid options and limits.
pub fn analyze_profile(
    profile: &[u8],
    bios: Option<BiosRef<'_>>,
    opts: &AnalyzeOptions,
) -> Result<Report<ExperimentalInspectPayload>> {
    analyze_with_baseline_on(profile, bios, None, opts)
}

/// A baseline profile and, when it was saved on another BIOS release, that release's image.
pub struct BaselineSource<'a> {
    pub profile: &'a [u8],
    /// `None`: the baseline was saved on the same image as the profile.
    pub bios: Option<BiosRef<'a>>,
    /// Firmware set of `bios` (0-based); `None` lets a single-set image choose itself.
    pub set: Option<usize>,
}

impl<'a> BaselineSource<'a> {
    /// A baseline saved on the same image (and firmware set) as the profile.
    pub fn same_image(profile: &'a [u8]) -> Self {
        Self {
            profile,
            bios: None,
            set: None,
        }
    }
}

/// `analyze_profile`, plus a comparison with a baseline profile. A baseline saved on the same BIOS
/// right after loading optimized defaults goes through the same pipeline, against the same image
/// and firmware set, and must pass the same gates; when it does not, the main analysis is
/// unchanged and the report says why the baseline was not applied. With a separate BIOS for the
/// baseline whose image has another digest (or another firmware set), the baseline is analysed
/// against it and must be the same board and platform family; fields then match by where the BIOS
/// shows them.
pub fn analyze_with_baseline_on(
    profile: &[u8],
    bios: Option<BiosRef<'_>>,
    baseline: Option<BaselineSource<'_>>,
    opts: &AnalyzeOptions,
) -> Result<Report<ExperimentalInspectPayload>> {
    let Some(src) = baseline else {
        return analyze_one(profile, bios, opts);
    };
    let Some(main_bios) = bios else {
        return Err(Error::Request("a baseline needs a BIOS image".into()));
    };
    let mut main = analyze_one(profile, bios, opts)?;
    let main_set = main.payload.inputs.bios.as_ref().and_then(|b| b.set);
    let cross_bios = src.bios.filter(|b| {
        b.image.sha256 != main_bios.image.sha256 || src.set.is_some_and(|s| Some(s) != main_set)
    });
    let cross = cross_bios.is_some();
    let base = match cross_bios {
        Some(b) => analyze_one(
            src.profile,
            Some(b),
            &AnalyzeOptions {
                set: src.set,
                limits: opts.limits,
            },
        ),
        None => analyze_one(src.profile, bios, opts),
    };
    let base_bios = cross_bios.map(|b| {
        let set = match &base {
            Ok(r) => r.payload.inputs.bios.as_ref().and_then(|x| x.set),
            Err(_) => src.set,
        };
        bios_input(&b, set)
    });
    let (status, reason, base) = match base {
        Err(e) => ("blocked", e.to_string(), None),
        Ok(b) => match baseline_gate(&b.payload, &main.payload, cross) {
            Ok(()) => (
                "applied",
                if cross {
                    CROSS_NOTE.to_string()
                } else {
                    "the baseline passed the same identity, integrity and mapping gates".to_string()
                },
                Some(b.payload),
            ),
            Err(why) => ("blocked", why, None),
        },
    };
    let p = &mut main.payload;
    p.inputs.baseline = Some(BaselineInput {
        sha256: sha256_hex(src.profile),
        size: src.profile.len(),
        status: status.into(),
        reason: reason.clone(),
        mode: if cross { "cross_version" } else { "same_image" }.into(),
        bios: base_bios,
    });
    match base {
        None => p
            .status_reasons
            .push(format!("Baseline not applied: {reason}")),
        Some(b) if cross => {
            let t = compare_cross(p, &b);
            p.coverage.fields_compared_with_baseline = Some(t.compared);
            p.coverage.fields_differing_from_baseline = Some(t.differing);
            p.coverage.fields_not_compared_with_baseline = Some(t.not_compared());
            add_rule(p, RULE_BASELINE_CROSS);
            p.evidence.push(Evidence {
                id: format!("ev:rule:{RULE_BASELINE_CROSS}"),
                kind: "rule".into(),
                summary:
                    "Fields matched with a baseline from another BIOS release by menu place and name"
                        .into(),
                observations: vec![
                    format!(
                        "{} fields compared, {} differ from the baseline",
                        t.compared, t.differing
                    ),
                    format!("{} fields with no counterpart in the baseline", t.missing),
                    format!(
                        "{} fields whose place and name is not unique in the profile's BIOS (a setting repeated per item, such as per camera or lane)",
                        t.duplicate_main
                    ),
                    format!(
                        "{} further fields whose place and name is unique there but not in the baseline's BIOS",
                        t.duplicate_base
                    ),
                    format!(
                        "{} fields with a counterpart that has no stored value",
                        t.no_value
                    ),
                ],
                contradictions: Vec::new(),
                limitations: vec![CROSS_NOTE.into()],
                basis: "file_comparison".into(),
            });
            p.status_reasons.push(format!(
                "Baseline applied across two BIOS versions: {} of {} compared fields differ from the baseline; {} fields not compared. {CROSS_NOTE}",
                t.differing,
                t.compared,
                t.not_compared()
            ));
        }
        Some(b) => {
            let by_key: BTreeMap<&str, &ExperimentalField> =
                b.fields.iter().map(|f| (f.key.as_str(), f)).collect();
            let (mut compared, mut differing) = (0usize, 0usize);
            for f in &mut p.fields {
                let Some(other) = by_key.get(f.key.as_str()) else {
                    continue;
                };
                let d = match (&f.raw, &other.raw) {
                    (Some(a), Some(b)) => Some(a != b),
                    _ => None,
                };
                annotate_baseline(f, other, "key", d);
                if let Some(d) = d {
                    compared += 1;
                    differing += usize::from(d);
                }
            }
            p.coverage.fields_compared_with_baseline = Some(compared);
            p.coverage.fields_differing_from_baseline = Some(differing);
            add_rule(p, RULE_BASELINE);
            p.status_reasons.push(format!(
                "Baseline applied: {differing} of {compared} fields store different bytes than the baseline (a difference does not say which file was changed)"
            ));
        }
    }
    split_baseline_differences(&mut main.payload);
    Ok(main)
}

/// Record the baseline's counterpart `other` on `f`; `how` names the matching rule and `differs`
/// is whether the stored values differ (`None` when either side has no stored value).
fn annotate_baseline(
    f: &mut ExperimentalField,
    other: &ExperimentalField,
    how: &str,
    differs: Option<bool>,
) {
    f.baseline_match = Some(how.into());
    f.baseline_raw = other.raw.clone();
    f.baseline_label = other.value_label.clone();
    f.baseline_off_default = other.differs_from_default;
    f.differs_from_baseline = differs;
}

/// Add `rule` to the analysis' sorted rule list.
fn add_rule(p: &mut ExperimentalInspectPayload, rule: &str) {
    p.analysis.rules.push(rule.into());
    p.analysis.rules.sort();
}

/// Rule `baseline_firmware_filled/1`: display classification of the fields that differ from an
/// applied baseline, by whether the baseline itself is at its own BIOS's optimized default.
fn split_baseline_differences(p: &mut ExperimentalInspectPayload) {
    if p.coverage.fields_differing_from_baseline.is_none() {
        return;
    }
    let (mut at, mut off, mut unknown) = (0usize, 0usize, 0usize);
    for f in p
        .fields
        .iter()
        .filter(|f| f.differs_from_baseline == Some(true))
    {
        match f.baseline_off_default {
            Some(false) => at += 1,
            Some(true) => off += 1,
            None => unknown += 1,
        }
    }
    p.coverage.baseline_differing_at_default = Some(at);
    p.coverage.baseline_differing_off_default = Some(off);
    p.coverage.baseline_differing_no_default = Some(unknown);
    add_rule(p, RULE_BASELINE_FILLED);
    p.evidence.push(Evidence {
        id: format!("ev:rule:{RULE_BASELINE_FILLED}"),
        kind: "rule".into(),
        summary: "Fields differing from the baseline, split by whether the baseline is at its own BIOS default".into(),
        observations: vec![
            format!("{at} differ with the baseline at its optimized default (most likely settings made in the BIOS menus)"),
            format!("{off} differ with the baseline itself off its optimized default (often filled in by the firmware, such as fan type detection or link equalisation)"),
            format!("{unknown} differ where the baseline's BIOS has no optimized default, so nothing is known"),
        ],
        contradictions: Vec::new(),
        limitations: vec![
            "Display classification only. It means something only when the baseline was saved with the BIOS at its defaults; a baseline saved after tuning counts its own settings as off default too.".into(),
        ],
        basis: "file_comparison".into(),
    });
}

const CROSS_NOTE: &str = "Settings were matched by menu place and name across two BIOS versions; a difference can also come from a default or a meaning that changed between versions; settings present in only one version are not compared.";

#[derive(Default)]
struct CrossTally {
    compared: usize,
    differing: usize,
    missing: usize,
    duplicate_main: usize,
    duplicate_base: usize,
    no_value: usize,
}

impl CrossTally {
    fn not_compared(&self) -> usize {
        self.missing + self.duplicate_main + self.duplicate_base + self.no_value
    }
}

/// The place the BIOS shows a setting, without offsets or module digests: store name and GUID,
/// menu path, section, label, kind and width.
fn place_keys(p: &ExperimentalInspectPayload) -> Vec<Option<String>> {
    let stores: BTreeMap<&str, (&str, &str)> = p
        .stores
        .iter()
        .map(|s| (s.key.as_str(), (s.name.as_str(), s.guid.as_str())))
        .collect();
    p.fields
        .iter()
        .map(|f| {
            let (name, guid) = stores.get(f.store.as_str())?;
            Some(format!(
                "{name}\u{1}{guid}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
                f.menu_path.join("\u{2}"),
                f.section
                    .as_deref()
                    .map_or("-".to_string(), |s| format!("+{s}")),
                f.label,
                f.kind,
                f.width_bits
            ))
        })
        .collect()
}

/// Annotates `p` from `b`, a baseline read against another release of the same board. A field is
/// matched only when its place and name is unique on both sides.
fn compare_cross(p: &mut ExperimentalInspectPayload, b: &ExperimentalInspectPayload) -> CrossTally {
    let mkeys = place_keys(p);
    let bkeys = place_keys(b);
    let mut main_count: BTreeMap<&str, usize> = BTreeMap::new();
    for k in mkeys.iter().flatten() {
        *main_count.entry(k.as_str()).or_default() += 1;
    }
    let mut base_at: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, k) in bkeys.iter().enumerate() {
        if let Some(k) = k {
            base_at.entry(k.as_str()).or_default().push(i);
        }
    }
    let mut t = CrossTally::default();
    for (f, key) in p.fields.iter_mut().zip(&mkeys) {
        if f.raw.is_none() {
            continue;
        }
        let Some(key) = key else {
            t.missing += 1;
            continue;
        };
        if main_count.get(key.as_str()).copied().unwrap_or(0) > 1 {
            t.duplicate_main += 1;
            continue;
        }
        let other = match base_at.get(key.as_str()).map(Vec::as_slice) {
            None | Some([]) => {
                t.missing += 1;
                continue;
            }
            Some([i]) => &b.fields[*i],
            Some(_) => {
                t.duplicate_base += 1;
                continue;
            }
        };
        let Some(y) = &other.raw else {
            t.no_value += 1;
            continue;
        };
        // Option raws can be renumbered between releases: compare what the BIOS displays.
        let d = match (f.kind.as_str(), &f.value_label, &other.value_label) {
            ("one_of" | "checkbox", Some(l), Some(r)) => l != r,
            _ => f.raw.as_ref() != Some(y),
        };
        annotate_baseline(f, other, "menu_place", Some(d));
        t.compared += 1;
        t.differing += usize::from(d);
    }
    t
}

/// Whether a baseline analysis may annotate the main one.
fn baseline_gate(
    b: &ExperimentalInspectPayload,
    main: &ExperimentalInspectPayload,
    cross: bool,
) -> std::result::Result<(), String> {
    if main.fields.is_empty() {
        return Err("the profile itself was not interpreted".into());
    }
    if b.selected_candidate.is_none() {
        return Err("the baseline has no eligible container".into());
    }
    if b.identity.status != "match" {
        return Err(format!("baseline identity: {}", b.identity.reason));
    }
    if b.integrity.status == "failed" {
        return Err("the baseline's established checksum does not match".into());
    }
    if b.fields.is_empty() {
        return Err(format!(
            "the baseline was not interpreted: {}",
            b.status_reasons.first().cloned().unwrap_or_default()
        ));
    }
    let (Some(mb), Some(bb)) = (&main.inputs.bios, &b.inputs.bios) else {
        return Err("the baseline was read against another BIOS image or firmware set".into());
    };
    if cross {
        let board = |id: &str| id.split('.').next().unwrap_or_default().to_string();
        if board(&mb.bios_id).is_empty() || board(&mb.bios_id) != board(&bb.bios_id) {
            return Err(format!(
                "the baseline's BIOS ({}) is not the same board as this BIOS ({})",
                bb.bios_id, mb.bios_id
            ));
        }
        if mb.family != bb.family {
            return Err(format!(
                "the baseline's BIOS is another platform family ({} instead of {})",
                bb.family, mb.family
            ));
        }
    } else if (&bb.sha256, bb.set) != (&mb.sha256, mb.set) {
        return Err("the baseline was read against another BIOS image or firmware set".into());
    }
    Ok(())
}

fn analyze_one(
    profile: &[u8],
    bios: Option<BiosRef<'_>>,
    opts: &AnalyzeOptions,
) -> Result<Report<ExperimentalInspectPayload>> {
    let mut budget = Budget::new(opts.limits);
    if profile.len() > crate::format::MAX_PROFILE_BYTES {
        return Err(Error::Limit(crate::format::profile_too_large(
            profile,
            profile.len() as u64,
        )));
    }
    let set = match (&bios, opts.set) {
        (None, Some(_)) => {
            return Err(Error::Request("a firmware set needs a BIOS image".into()));
        }
        (Some(b), Some(s)) if s >= b.image.sets.len().max(1) => {
            return Err(Error::Request(format!(
                "this image has no firmware set {}; it has {}",
                s + 1,
                b.image.sets.len().max(1)
            )));
        }
        (Some(b), None) if b.image.sets.len() <= 1 => Some(0),
        (_, s) => s,
    };

    let probed = probe::probe(profile, &mut budget)?;
    let mut rules: BTreeSet<String> = BTreeSet::new();
    let mut evidence: Vec<Evidence> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();

    // Observations: signatures only, bounded, no file contents beyond them.
    let mut observations = Vec::new();
    if probed.magic {
        observations.push(Observation {
            kind: "signature".into(),
            span: Range::new(0, 5),
            text: "$MOS$".into(),
        });
    }
    for &at in &probed.oci_signatures {
        budget.observation()?;
        observations.push(Observation {
            kind: "signature".into(),
            span: Range::new(at, at + 5),
            text: "$OCI$".into(),
        });
    }

    // Stage B/C per candidate.
    let mut infos = Vec::new();
    let mut eligible: Vec<(usize, ProfileId, bool, IntegrityFinding)> = Vec::new();
    for (i, c) in probed.candidates.iter().enumerate() {
        rules.insert(format!("{}/{}", c.rule, c.rule_version));
        let id = candidate_identity(c);
        let est = established(c, &id);
        let integ = integrity(
            profile,
            c,
            est,
            bios.as_ref()
                .is_some_and(|b| profile_code::traced(&b.image.sha256)),
        );
        let mut contradictions = Vec::new();
        if !c.complete() {
            contradictions.push(
                c.chain_error
                    .clone()
                    .unwrap_or_else(|| "record chain incomplete".into()),
            );
        } else if c.trailers.len() > 1 {
            contradictions.push(format!(
                "{} plausible $OCI$ trailers after the records",
                c.trailers.len()
            ));
        } else if c.trailers.is_empty() && !c.ends_after_cmos_block(profile.len()) {
            contradictions.push(
                "no coherent $OCI$ block after the records, and the file does not end right after the 256-byte CMOS block"
                    .into(),
            );
        }
        if id.status != "resolved" {
            contradictions.push(id.reason.clone());
        }
        let ok = contradictions.is_empty();
        infos.push(candidate_info(c, ok, contradictions.clone(), integ.clone()));
        evidence.push(Evidence {
            id: format!("ev:container:{}", c.id),
            kind: "container".into(),
            summary: format!(
                "{} {}",
                c.id,
                if ok {
                    "fits the file"
                } else {
                    "does not fully fit"
                }
            ),
            observations: c
                .checks
                .iter()
                .map(|k| format!("{}: {:?} ({})", k.name, k.status, k.detail))
                .collect(),
            contradictions,
            limitations: {
                let mut l = c.assumptions.clone();
                if c.ends_after_cmos_block(profile.len()) {
                    l.push("No $OCI$ trailer: the file ends exactly 256 bytes (the CMOS block) after the records; firmware with no $OCI$ code is expected to write files like this (not traced)".into());
                }
                l
            },
            basis: "file_structure".into(),
        });
        if ok {
            eligible.push((i, id, est, integ));
        }
    }
    rules.insert(RULE_CHECKSUM.into());

    let selected = match eligible.len() {
        1 => eligible.pop(),
        0 => {
            reasons.push(if probed.magic {
                "No container hypothesis fits the whole file; showing file facts only".into()
            } else {
                let base = "The file does not start with $MOS$";
                let hint =
                    crate::format::with_swapped_hint(base, ": ", profile, profile.len() as u64);
                format!("{hint}; showing file facts only")
            });
            None
        }
        n => {
            // The firmware's loader compares the header with the running BIOS id over that id's
            // length, so with a BIOS image the candidate whose header is that id is the layout.
            let exact: Vec<usize> = match &bios {
                Some(b) => eligible
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| probed.candidates[e.0].header_id == b.image.identity.bios_id)
                    .map(|(k, _)| k)
                    .collect(),
                None => Vec::new(),
            };
            if let [k] = exact.as_slice() {
                reasons.push(format!(
                    "{n} container hypotheses fit the file; chose the one whose header is the BIOS id, as the firmware's loader reads it"
                ));
                Some(eligible.swap_remove(*k))
            } else {
                reasons.push(format!(
                    "{n} container hypotheses fit the file; none is chosen, so no settings are interpreted"
                ));
                None
            }
        }
    };

    let container = match &selected {
        Some((_, _, true, _)) => "established",
        Some(_) => "candidate",
        None if probed.candidates.iter().any(ContainerCandidate::complete) => "candidate",
        None => "unrecognized",
    };

    // Identity against the BIOS.
    let (identity, integrity_top) = match &selected {
        Some((_, pid, _, integ)) => {
            let (status, reason) = match (&bios, &pid.id) {
                (_, None) => (pid.status, pid.reason.clone()),
                (None, Some(p)) => (
                    "unknown",
                    format!(
                        "profile was saved on {}; no BIOS image supplied. Open that BIOS image (MSI version {}) to read the settings.",
                        full_id(p),
                        crate::firmware::platform::version_label(&p.2)
                    ),
                ),
                (Some(b), Some(p)) => {
                    let bi = &b.image.identity;
                    if p.0 == bi.board && p.1 == bi.flavor && p.2 == bi.revision {
                        ("match", format!("profile and BIOS are both {}", bi.bios_id))
                    } else {
                        (
                            "mismatch",
                            crate::firmware::platform::identity_mismatch(&full_id(p), &bi.bios_id),
                        )
                    }
                }
            };
            (
                IdentityFinding {
                    status: if status == "resolved" {
                        "unknown".into()
                    } else {
                        status.into()
                    },
                    reason,
                    profile_bios_id: pid.id.as_ref().map(full_id),
                    observed: pid.observed.clone(),
                    limitations: pid.limitations.clone(),
                },
                integ.clone(),
            )
        }
        None => (
            IdentityFinding {
                status: "unknown".into(),
                reason: "no eligible container, so no identifier is taken from the file".into(),
                profile_bios_id: None,
                observed: Vec::new(),
                limitations: Vec::new(),
            },
            IntegrityFinding {
                status: "unknown".into(),
                rule: None,
                scope: String::new(),
                checksum_at: None,
                stored: None,
                computed: None,
                limitations: vec!["No container was chosen".into()],
            },
        ),
    };
    evidence.push(Evidence {
        id: "ev:identity".into(),
        kind: "identity".into(),
        summary: identity.reason.clone(),
        observations: identity
            .observed
            .iter()
            .map(|o| format!("{}: {:?}", o.source, o.text))
            .collect(),
        contradictions: if identity.status == "match" {
            Vec::new()
        } else {
            vec![identity.reason.clone()]
        },
        limitations: identity.limitations.clone(),
        basis: "file_structure".into(),
    });
    evidence.push(Evidence {
        id: "ev:integrity".into(),
        kind: "integrity".into(),
        summary: format!("checksum {}", integrity_top.status),
        observations: vec![format!(
            "stored {}, computed {}",
            integrity_top.stored.as_deref().unwrap_or("-"),
            integrity_top.computed.as_deref().unwrap_or("-")
        )],
        contradictions: Vec::new(),
        limitations: integrity_top.limitations.clone(),
        basis: "file_structure".into(),
    });

    // Gate for BIOS-based interpretation.
    let mut interpret = selected.is_some();
    if selected.is_some() {
        if bios.is_none() {
            reasons.push("No BIOS image: file structure only".into());
            interpret = false;
        } else if set.is_none() {
            reasons.push(
                "This BIOS image holds several firmware sets; choose one to interpret settings"
                    .into(),
            );
            interpret = false;
        } else if identity.status != "match" {
            reasons.push(format!("Settings are not interpreted: {}", identity.reason));
            interpret = false;
        } else if integrity_top.status == "failed" {
            reasons.push(
                "The established checksum does not match; only file structure is shown".into(),
            );
            interpret = false;
        } else if let (Some(b), Some(s), Some((i, ..))) = (&bios, set, &selected)
            && probed.candidates[*i].trailers.is_empty()
            && writes_trailer(b.image, s)
        {
            reasons.push(
                "The file has no $OCI$ trailer, but this BIOS's profile code holds that signature: the file looks truncated, so only file structure is shown"
                    .into(),
            );
            interpret = false;
        } else if let Some(b) = &bios
            && profile_code::traced(&b.image.sha256)
            && !matches!(
                integrity_top.status.as_str(),
                "verified" | "candidate_match"
            )
        {
            reasons.push(
                "The checksum does not match, and this release's traced profile loader rejects such a file; only file structure is shown"
                    .into(),
            );
            interpret = false;
        } else if integrity_top.status == "unknown" {
            reasons.push(
                "Integrity not established: this board's checksum is unknown, so values may come from a damaged file".into(),
            );
        }
    }

    let records: Vec<probe::ChainRecord> = selected
        .as_ref()
        .map(|(i, ..)| probed.candidates[*i].records.clone())
        .unwrap_or_default();
    let established_scope =
        matches!(&selected, Some((_, _, true, _))) && integrity_top.status == "verified";

    // Stage D: inventory.
    let img = bios.as_ref().map(|b| b.image);
    let traced_release = img.is_some_and(|i| profile_code::traced(&i.sha256));
    // Nothing is compared with the BIOS's stores unless interpretation is allowed: a wrong or
    // unidentified BIOS gives no evidence about this file's records.
    let vars: Vec<Var> = match (img, set) {
        (Some(img), Some(s)) if interpret => {
            rules.insert(RULE_UEFI_IDENTITY.into());
            inventory(img, s)
        }
        _ => Vec::new(),
    };
    let var_questions: Vec<Vec<(&Formset, &Question)>> = vars
        .iter()
        .map(|v| match img {
            Some(img) if v.supported() => questions(img, v),
            _ => Vec::new(),
        })
        .collect();
    let var_defaults: Vec<(Option<Vec<u8>>, Option<String>)> = vars
        .iter()
        .map(|v| match img {
            Some(img) if v.supported() => var_defaults(img, v),
            _ => (None, None),
        })
        .collect();

    // The validated MS-7D91 binding: Setup and CpuSetup each the only record of their size,
    // Setup first. Only inside the established scope.
    let mut bound: Vec<(usize, usize)> = Vec::new();
    if established_scope && interpret {
        rules.insert(RULE_7D91_BINDING.into());
        for name in ["Setup", "CpuSetup"] {
            let named: Vec<usize> = vars
                .iter()
                .enumerate()
                .filter(|(_, v)| v.supported() && v.name == name)
                .map(|(i, _)| i)
                .collect();
            let [vi] = named.as_slice() else { continue };
            let same: Vec<usize> = records
                .iter()
                .enumerate()
                .filter(|(_, r)| r.size == usize::from(vars[*vi].size))
                .map(|(i, _)| i)
                .collect();
            let [ri] = same.as_slice() else { continue };
            if name == "Setup" && *ri != 0 {
                continue;
            }
            bound.push((*ri, *vi));
        }
    }

    // Firmware-derived names: record id -> the setup browser's own variable list.
    let index = match (img, set) {
        (Some(img), Some(s)) if interpret && !records.is_empty() => {
            rules.insert(RULE_VARIABLE_INDEX.into());
            Some(index_rule(img, s, &records, &vars, &bound))
        }
        _ => None,
    };
    let by_index = index.as_ref().is_some_and(IndexRule::applied);
    if let Some(ix) = &index {
        if !ix.applied() {
            reasons.push(format!(
                "The BIOS's variable list was not used ({}); mappings fall back to sizes",
                ix.contradictions.first().cloned().unwrap_or_default()
            ));
        }
        evidence.push(Evidence {
            id: format!("ev:rule:{RULE_VARIABLE_INDEX}"),
            kind: "rule".into(),
            summary: if ix.applied() {
                "Record ids name variables in this BIOS's setup-browser variable list".into()
            } else {
                "The setup-browser variable list does not fit this file; not used".into()
            },
            basis: "firmware_code".into(),
            observations: ix.observations.clone(),
            contradictions: ix.contradictions.clone(),
            limitations: ix.limitations.clone(),
        });
    }

    // Stage E: candidate graph: the listed variable when the index rule applies, else size.
    let mut rec_cands: Vec<Vec<usize>> = vec![Vec::new(); records.len()];
    let mut var_cands: Vec<Vec<usize>> = vec![Vec::new(); vars.len()];
    if let (true, Some(ix)) = (by_index, &index) {
        for (ri, r) in records.iter().enumerate().filter(|(_, r)| r.size > 0) {
            let Some(e) = ix.entries[ri] else { continue };
            let vs: Vec<usize> = vars
                .iter()
                .enumerate()
                .filter(|(_, v)| {
                    v.supported()
                        && v.guid == e.guid
                        && v.name == e.name
                        && usize::from(v.size) == r.size
                })
                .map(|(i, _)| i)
                .collect();
            budget.edges(vs.len())?;
            for &vi in &vs {
                var_cands[vi].push(ri);
            }
            rec_cands[ri] = vs;
        }
    } else {
        let mut by_size: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (vi, v) in vars.iter().enumerate().filter(|(_, v)| v.supported()) {
            by_size.entry(usize::from(v.size)).or_default().push(vi);
        }
        for (ri, r) in records.iter().enumerate().filter(|(_, r)| r.size > 0) {
            if let Some(vs) = by_size.get(&r.size) {
                budget.edges(vs.len())?;
                rec_cands[ri] = vs.clone();
                for &vi in vs {
                    var_cands[vi].push(ri);
                }
            }
        }
    }
    let listed = |ri: usize| {
        index
            .as_ref()
            .filter(|_| by_index)
            .and_then(|ix| ix.entries[ri])
    };

    // Mapping: (state, var index, rule) per record.
    let mut rec_map: Vec<(&'static str, Option<usize>, String)> = records
        .iter()
        .enumerate()
        .map(|(ri, r)| {
            if r.size == 0 {
                (
                    "unmapped",
                    None,
                    match listed(ri) {
                        Some(e) => format!("empty record ({} had no data when the file was saved)", e.name),
                        None => "empty record".to_string(),
                    },
                )
            } else if rec_cands[ri].is_empty() {
                (
                    "unmapped",
                    None,
                    if let Some(e) = listed(ri) {
                        match vars.iter().find(|v| v.guid == e.guid && v.name == e.name) {
                            Some(v) if !v.supported() => format!(
                                "{} (by the BIOS's variable list); its store is not mapped: {}",
                                e.name, v.support
                            ),
                            Some(v) => format!(
                                "{} (by the BIOS's variable list); the record is {} bytes but its BIOS store {} ({}), so it is not decoded",
                                e.name,
                                r.size,
                                v.size,
                                if traced_release {
                                    "sized at run time"
                                } else {
                                    "probably sized at run time; not traced for this release"
                                }
                            ),
                            _ => format!(
                                "{} (by the BIOS's variable list); no BIOS menu reads this variable",
                                e.name
                            ),
                        }
                    } else if !interpret {
                        "not compared with BIOS stores (see the status reasons)".to_string()
                    } else if vars.is_empty() {
                        "the BIOS defines no stores in this set".to_string()
                    } else {
                        format!("no supported store is {} bytes", r.size)
                    },
                )
            } else {
                ("ambiguous", None, String::new())
            }
        })
        .collect();

    // Reviewed rule first: the validated MS-7D91 binding, only inside its established scope.
    for &(ri, vi) in &bound {
        rec_map[ri] = (
            "confirmed",
            Some(vi),
            format!(
                "{RULE_7D91_BINDING}: the validated {} binding",
                vars[vi].name
            ),
        );
    }
    let taken: BTreeSet<usize> = rec_map.iter().filter_map(|m| m.1).collect();
    // Firmware-derived: the listed variable, when its menus declare a store of the record's size.
    if by_index {
        for ri in 0..records.len() {
            if rec_map[ri].0 != "ambiguous" {
                continue;
            }
            let Some(e) = listed(ri) else { continue };
            rec_map[ri] = match rec_cands[ri].as_slice() {
                [vi] if taken.contains(vi) => (
                    "unmapped",
                    None,
                    format!(
                        "{}: its store is bound to another record by a reviewed rule",
                        vars[*vi].name
                    ),
                ),
                // One record per store: two records named the same variable stay ambiguous.
                [vi] if var_cands[*vi].len() == 1 => (
                    "inferred",
                    Some(*vi),
                    format!(
                        "{RULE_VARIABLE_INDEX}: record id {} is {} ({}) in this BIOS's variable list",
                        records[ri].id, e.name, e.guid
                    ),
                ),
                [vi] => (
                    "ambiguous",
                    None,
                    format!(
                        "{} records are named {} by the variable list",
                        var_cands[*vi].len(),
                        vars[*vi].name
                    ),
                ),
                many => (
                    "ambiguous",
                    None,
                    format!("{} stores are {} with this size", many.len(), e.name),
                ),
            };
        }
    }
    // Generic inference: isolated one-to-one pairs only.
    for ri in 0..records.len() {
        if by_index || rec_map[ri].0 != "ambiguous" {
            continue;
        }
        let cands: Vec<usize> = rec_cands[ri]
            .iter()
            .copied()
            .filter(|v| !taken.contains(v))
            .collect();
        let rivals = |vi: usize| {
            var_cands[vi]
                .iter()
                .filter(|&&r| rec_map[r].0 != "confirmed")
                .count()
        };
        rec_map[ri] = match cands.as_slice() {
            [] => (
                "unmapped",
                None,
                "its only candidate store is bound to another record by a reviewed rule".into(),
            ),
            [vi] if rivals(*vi) == 1 => {
                let v = &vars[*vi];
                let reach = var_questions[*vi].iter().any(|(_, q)| {
                    usize::from(q.offset) + usize::from(q.width_bits / 8) > usize::from(v.size)
                });
                if reach {
                    (
                        "unmapped",
                        None,
                        format!("BIOS questions reach past {}'s declared size", v.name),
                    )
                } else {
                    rules.insert(RULE_UNIQUE_SIZE.into());
                    (
                        "inferred",
                        Some(*vi),
                        format!(
                            "{RULE_UNIQUE_SIZE}: the only {}-byte record and the only {}-byte store ({})",
                            records[ri].size, v.size, v.name
                        ),
                    )
                }
            }
            [vi] => (
                "ambiguous",
                None,
                format!(
                    "{} records are {} bytes, like {}",
                    rivals(*vi),
                    records[ri].size,
                    vars[*vi].name
                ),
            ),
            many => (
                "ambiguous",
                None,
                format!(
                    "{} stores are {} bytes: {}",
                    many.len(),
                    records[ri].size,
                    many.iter()
                        .map(|&v| vars[v].name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
        };
    }
    let var_of_record: BTreeMap<usize, usize> = rec_map
        .iter()
        .enumerate()
        .filter_map(|(ri, m)| m.1.map(|v| (v, ri)))
        .collect();

    // Record infos with mapping evidence (candidate statistics explain, they never decide).
    let mut record_infos = Vec::with_capacity(records.len());
    for (ri, r) in records.iter().enumerate() {
        budget.tick()?;
        let key = format!("r{}", r.index);
        let (state, _, reason) = &rec_map[ri];
        let mut obs = Vec::new();
        for &vi in &rec_cands[ri] {
            let s = edge_stats(
                profile,
                r.payload_at,
                &vars[vi],
                &var_questions[vi],
                var_defaults[vi].0.as_deref(),
                &mut budget,
            )?;
            obs.push(format!("{}: {}", vars[vi].name, stats_text(&s)));
        }
        let ev = format!("ev:map:{key}");
        if !rec_cands[ri].is_empty() {
            budget.observation()?;
            evidence.push(Evidence {
                id: ev.clone(),
                kind: "mapping".into(),
                summary: format!("{key} ({} bytes): {state}", r.size),
                observations: obs,
                contradictions: if *state == "ambiguous" {
                    vec![reason.clone()]
                } else {
                    Vec::new()
                },
                limitations: vec![if by_index {
                    "Named by the BIOS's own variable list; value statistics only describe the fit"
                        .into()
                } else {
                    "Equal size is necessary, not sufficient; value statistics do not prove a mapping"
                        .into()
                }],
                basis: match *state {
                    "confirmed" => "reviewed_binding",
                    _ if by_index => "firmware_code",
                    _ => "size_only",
                }
                .into(),
            });
        }
        let n = budget.take_preview(r.size.min(PREVIEW_BYTES));
        record_infos.push(RecordInfo {
            key: key.clone(),
            index: r.index,
            id: r.id,
            header: Range::new(r.header_at, r.payload_at),
            payload: Range::new(r.payload_at, r.payload_end()),
            size: r.size,
            mapping: (*state).into(),
            candidate_stores: rec_cands[ri].iter().map(|&v| vars[v].key.clone()).collect(),
            variable: listed(ri).map(|e| ListedVariableInfo {
                index: e.index,
                guid: e.guid.clone(),
                name: e.name.clone(),
                declared_size: e.size,
            }),
            reason: reason.clone(),
            evidence: if rec_cands[ri].is_empty() {
                Vec::new()
            } else {
                vec![ev]
            },
            preview_hex: hex(&profile[r.payload_at..r.payload_at + n]),
            preview_truncated: n < r.size,
            preview_omitted: false,
        });
    }

    // Stage F: generic field interpretation for mapped records.
    let mut fields = Vec::new();
    for (vi, v) in vars.iter().enumerate() {
        let Some(&ri) = var_of_record.get(&vi) else {
            continue;
        };
        let r = &records[ri];
        let rkey = format!("r{}", r.index);
        let state = rec_map[ri].0;
        let defaults = var_defaults[vi].0.as_deref();
        for (fs, q) in &var_questions[vi] {
            budget.field()?;
            fields.push(decode(profile, r, &rkey, state, v, fs, q, defaults));
        }
    }
    fields.sort_by(|a, b| {
        (
            a.span.map(|s| s.start),
            a.store_offset,
            a.width_bits,
            &a.key,
        )
            .cmp(&(
                b.span.map(|s| s.start),
                b.store_offset,
                b.width_bits,
                &b.key,
            ))
    });

    // Store infos.
    let store_infos: Vec<StoreInfo> = match img {
        None => Vec::new(),
        Some(img) => vars
            .iter()
            .enumerate()
            .map(|(vi, v)| {
                let mapped = var_of_record.get(&vi).copied();
                let (mapping, reason) = match mapped {
                    Some(ri) => (rec_map[ri].0, rec_map[ri].2.clone()),
                    None if !v.supported() => ("unmapped", v.support.clone()),
                    None if var_cands[vi].is_empty() => (
                        "unmapped",
                        if records.is_empty() {
                            "no records to compare with".to_string()
                        } else {
                            format!("no record is {} bytes", v.size)
                        },
                    ),
                    None => ("ambiguous", {
                        let rs: Vec<String> = var_cands[vi]
                            .iter()
                            .map(|&r| format!("r{}", records[r].index))
                            .collect();
                        format!("{} same-size records: {}", rs.len(), rs.join(", "))
                    }),
                };
                StoreInfo {
                    key: v.key.clone(),
                    name: v.name.clone(),
                    guid: v.guid.clone(),
                    kind: kind_name(&v.kind).into(),
                    size: v.size,
                    set: set.unwrap_or_default(),
                    declared_by: v
                        .decls
                        .iter()
                        .map(|&(fi, local)| {
                            let fs = &img.formsets[fi];
                            StoreDeclaration {
                                formset_guid: fs.guid.clone(),
                                formset_title: fs.title.clone(),
                                module: fs
                                    .module
                                    .name
                                    .clone()
                                    .unwrap_or_else(|| fs.module.file_guid.clone()),
                                module_sha256: fs.module.pe32_sha256.clone(),
                                local_id: local,
                            }
                        })
                        .collect(),
                    support: v.support.clone(),
                    candidate_records: var_cands[vi]
                        .iter()
                        .map(|&r| format!("r{}", records[r].index))
                        .collect(),
                    mapping: mapping.into(),
                    mapped_record: mapped.map(|r| format!("r{}", records[r].index)),
                    reason,
                    evidence: mapped
                        .map(|r| vec![format!("ev:map:r{}", records[r].index)])
                        .unwrap_or_default(),
                    field_count: var_questions[vi].len(),
                }
            })
            .collect(),
    };

    if rules.contains(RULE_UEFI_IDENTITY) {
        evidence.push(Evidence {
            id: format!("ev:rule:{RULE_UEFI_IDENTITY}"),
            kind: "rule".into(),
            summary: "Formsets declaring the same GUID, name and size share one variable".into(),
            observations: vec![format!(
                "{} declarations merged into {} variables",
                vars.iter().map(|v| v.decls.len()).sum::<usize>(),
                vars.len()
            )],
            contradictions: vars
                .iter()
                .filter(|v| v.support.starts_with("declared with different sizes"))
                .map(|v| format!("{}: {}", v.name, v.support))
                .collect(),
            limitations: vec![
                "UEFI variable identity (GUID + name) within one firmware set".into(),
            ],
            basis: "firmware_code".into(),
        });
    }

    let coverage = coverage(profile.len(), &record_infos, &fields);
    if interpret && coverage.fields_decoded > 0 {
        reasons.push(format!(
            "{} settings interpreted from {} mapped records; mappings marked inferred are not confirmed",
            coverage.fields_decoded,
            coverage.records_confirmed + coverage.records_inferred
        ));
    } else if interpret {
        reasons.push("No record could be mapped to a BIOS store without ambiguity".into());
    }

    let bios_input = bios.as_ref().map(|b| bios_input(b, set));

    let payload = ExperimentalInspectPayload {
        analysis: AnalysisInfo {
            analyzer: ANALYZER.into(),
            version: ANALYZER_VERSION.into(),
            mode: "experimental_read_only".into(),
            rules: rules.into_iter().collect(),
            completion: "complete".into(),
            shareable: false,
            redactions: Vec::new(),
        },
        inputs: AnalysisInputs {
            profile_sha256: sha256_hex(profile),
            profile_size: profile.len(),
            bios: bios_input,
            baseline: None,
        },
        container: container.into(),
        status_reasons: reasons,
        identity,
        integrity: integrity_top,
        container_candidates: infos,
        selected_candidate: selected.map(|(i, ..)| probed.candidates[i].id.clone()),
        records: record_infos,
        stores: store_infos,
        fields,
        evidence,
        coverage,
        observations,
        capabilities: ExperimentalCapabilities::READ_ONLY,
    };
    let mut r = Report::new(OPERATION, payload, Vec::new());
    r.payload_version = EXPERIMENTAL_PAYLOAD_VERSION.into();
    Ok(r)
}

fn bios_input(b: &BiosRef<'_>, set: Option<usize>) -> BiosInput {
    BiosInput {
        sha256: b.image.sha256.clone(),
        size: b.image.size,
        bios_id: b.image.identity.bios_id.clone(),
        set,
        sets: b.image.sets.iter().map(|s| s.label.clone()).collect(),
        family: b.image.platform.family.clone(),
        profiles_supported: b.profiles_supported,
    }
}

#[allow(clippy::too_many_arguments)]
fn decode(
    bytes: &[u8],
    r: &probe::ChainRecord,
    rkey: &str,
    mapping: &str,
    v: &Var,
    fs: &Formset,
    q: &Question,
    defaults: Option<&[u8]>,
) -> ExperimentalField {
    let kind = match q.kind {
        QuestionKind::OneOf => "one_of",
        QuestionKind::Checkbox => "checkbox",
        QuestionKind::Numeric => "numeric",
    };
    let mut f = ExperimentalField {
        key: format!(
            "{}@{}/{}#{}:{}",
            v.key,
            q.offset,
            q.width_bits,
            fs.module.pe32_sha256.get(..12).unwrap_or_default(),
            q.ifr_offset
        ),
        store: v.key.clone(),
        record: rkey.into(),
        label: q.prompt.trim().into(),
        help: q.help.trim().into(),
        menu_path: fs.menu_path(q),
        menu_order: fs.menu_order(q),
        section: q.section.clone(),
        kind: kind.into(),
        store_offset: q.offset,
        width_bits: q.width_bits,
        span: None,
        raw: None,
        state: String::new(),
        value_label: None,
        note: None,
        mapping: mapping.into(),
        default_raw: None,
        default_label: None,
        differs_from_default: None,
        baseline_raw: None,
        baseline_label: None,
        differs_from_baseline: None,
        baseline_match: None,
        baseline_off_default: None,
        hidden: q.never_shown(),
        conditional: !q.conditions.is_empty(),
        callback: q.is_callback(),
        evidence: vec![format!("ev:map:{rkey}"), "ev:identity".into()],
    };
    let w = usize::from(q.width_bits / 8);
    if !q.width_bits.is_multiple_of(8) || !matches!(w, 1 | 2 | 4 | 8) {
        f.state = "unsupported_encoding".into();
        f.note = Some(format!(
            "{}-bit field; only whole bytes are read",
            q.width_bits
        ));
        return f;
    }
    let off = usize::from(q.offset);
    let Some(end) = off
        .checked_add(w)
        .filter(|&e| e <= usize::from(v.size) && e <= r.size)
    else {
        f.state = "out_of_bounds".into();
        f.note = Some(format!("bytes {off}..{} are outside the store", off + w));
        return f;
    };
    let at = r.payload_at + off;
    f.span = Some(Range::new(at, r.payload_at + end));
    let Ok(raw) = uint_at(bytes, at, w, "field") else {
        f.state = "out_of_bounds".into();
        return f;
    };
    f.raw = Some(RawInt::new(raw));
    let label_of = |val: u64| -> (Option<String>, Option<String>) {
        match q.kind {
            QuestionKind::OneOf => {
                let labels: BTreeSet<&str> = q
                    .options
                    .iter()
                    .filter(|o| o.value == val)
                    .map(|o| o.label.as_str())
                    .collect();
                match labels.len() {
                    0 => (
                        None,
                        Some("no option of this setting has this value".into()),
                    ),
                    1 => (labels.into_iter().next().map(String::from), None),
                    _ => (
                        None,
                        Some(format!(
                            "options with this value have different labels: {}",
                            labels.into_iter().collect::<Vec<_>>().join(" / ")
                        )),
                    ),
                }
            }
            QuestionKind::Checkbox => match val {
                0 => (Some("Disabled".into()), None),
                1 => (Some("Enabled".into()), None),
                _ => (None, Some("checkbox values are 0 or 1".into())),
            },
            QuestionKind::Numeric => (None, None),
        }
    };
    let (label, why) = label_of(raw);
    f.state = if q.kind == QuestionKind::Numeric || label.is_some() {
        "decoded"
    } else {
        "unknown_value"
    }
    .into();
    f.value_label = label;
    f.note = why.or_else(|| {
        (q.kind == QuestionKind::Numeric).then(|| {
            let range = match (q.min, q.max) {
                (Some(lo), Some(hi)) if raw < lo || raw > hi => {
                    format!("; outside the BIOS range {lo}..{hi}")
                }
                _ => String::new(),
            };
            format!("raw integer, no unit or sign applied{range}")
        })
    });
    if let Some(d) = defaults.and_then(|d| uint_at(d, off, w, "default").ok()) {
        f.default_raw = Some(RawInt::new(d));
        f.default_label = label_of(d).0;
        f.differs_from_default = Some(d != raw);
    }
    f
}

const OMITTED: &str = "[omitted]";

/// Turns a finished report into its shareable variant, in place: removes the CPU brand and save
/// time from the `$OCI$` trailer wherever the payload repeats them, and every raw byte preview.
/// Interpreted settings stay. The payload carries no local file path (paths are invocation
/// metadata only) and no CMOS or tail bytes (only the tail's length), so nothing else is cut.
pub fn make_shareable(p: &mut ExperimentalInspectPayload) {
    // Trailer strings after board and revision: CPU brand, save time, anything else.
    let mut secrets: BTreeSet<String> = BTreeSet::new();
    for t in p
        .container_candidates
        .iter_mut()
        .flat_map(|c| c.trailers.iter_mut())
    {
        for s in t.strings.iter_mut().skip(2) {
            if !s.is_empty() {
                secrets.insert(std::mem::replace(s, OMITTED.into()));
            }
        }
    }
    for r in &mut p.records {
        r.preview_omitted = true;
        r.preview_truncated = r.size > 0;
        r.preview_hex.clear();
    }
    // Any other place the strings were copied into (check details, evidence, reasons).
    let scrub = |text: &mut String| {
        for s in &secrets {
            if text.contains(s.as_str()) {
                *text = text.replace(s.as_str(), OMITTED);
            }
        }
    };
    for c in &mut p.container_candidates {
        c.checks.iter_mut().for_each(|k| scrub(&mut k.detail));
        c.assumptions.iter_mut().for_each(scrub);
        c.contradictions.iter_mut().for_each(scrub);
        c.integrity.limitations.iter_mut().for_each(scrub);
    }
    p.integrity.limitations.iter_mut().for_each(scrub);
    for e in &mut p.evidence {
        scrub(&mut e.summary);
        for l in [
            &mut e.observations,
            &mut e.contradictions,
            &mut e.limitations,
        ] {
            l.iter_mut().for_each(scrub);
        }
    }
    p.status_reasons.iter_mut().for_each(scrub);
    if let Some(b) = &mut p.inputs.baseline {
        scrub(&mut b.reason);
    }
    scrub(&mut p.identity.reason);
    p.identity.limitations.iter_mut().for_each(scrub);
    for o in &mut p.identity.observed {
        scrub(&mut o.text);
    }
    for o in &mut p.observations {
        scrub(&mut o.text);
    }
    p.analysis.shareable = true;
    p.analysis.redactions = vec![
        "CPU brand and save time from the $OCI$ trailer".into(),
        "raw byte preview of every record".into(),
    ];
}

fn union_len(mut spans: Vec<Range>) -> usize {
    spans.sort();
    let mut total = 0;
    let mut cur: Option<Range> = None;
    for s in spans {
        match &mut cur {
            Some(c) if s.start <= c.end => c.end = c.end.max(s.end),
            _ => {
                if let Some(c) = cur {
                    total += c.end - c.start;
                }
                cur = Some(s);
            }
        }
    }
    total + cur.map_or(0, |c| c.end - c.start)
}

fn coverage(size: usize, records: &[RecordInfo], fields: &[ExperimentalField]) -> Coverage {
    let count = |m: &str| records.iter().filter(|r| r.mapping == m).count();
    let fcount = |s: &str| fields.iter().filter(|f| f.state == s).count();
    let interpreted = union_len(
        fields
            .iter()
            .filter(|f| f.state == "decoded" || f.state == "unknown_value")
            .filter_map(|f| f.span)
            .collect(),
    );
    Coverage {
        records_total: records.len(),
        records_confirmed: count("confirmed"),
        records_inferred: count("inferred"),
        records_ambiguous: count("ambiguous"),
        records_unmapped: count("unmapped"),
        fields_total: fields.len(),
        fields_decoded: fcount("decoded"),
        fields_unknown_value: fcount("unknown_value"),
        fields_unsupported: fcount("unsupported_encoding"),
        fields_out_of_bounds: fcount("out_of_bounds"),
        profile_bytes: size,
        mapped_bytes: records
            .iter()
            .filter(|r| r.mapping == "confirmed" || r.mapping == "inferred")
            .map(|r| r.size)
            .sum(),
        interpreted_bytes: interpreted,
        opaque_bytes: size - interpreted.min(size),
        fields_compared_with_baseline: None,
        fields_differing_from_baseline: None,
        fields_not_compared_with_baseline: None,
        baseline_differing_at_default: None,
        baseline_differing_off_default: None,
        baseline_differing_no_default: None,
    }
}

mod format;
pub use format::{OPERATION as FORMAT_OPERATION, profile_format};

#[cfg(test)]
mod tests;
