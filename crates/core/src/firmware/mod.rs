//! BIOS image → browsable menus for every supported family, plus the immutable schema of
//! Setup/CpuSetup questions used with Intel profiles (SPEC §5).
//!
//! Each family has its own parser module (`intel`, `amd`) on top of shared primitives: volume
//! walk, formset scan with string pairing, IFR, and StdDefaults.

pub mod amd;
pub mod hii;
pub mod ifr;
pub mod intel;
pub mod nvar;
pub mod platform;
pub mod profile_code;
pub mod scan;
pub mod spf;
pub mod tiano;
pub mod volume;

use crate::bytes::sha256_hex;
use crate::error::{Error, Result};
use ifr::{Question, StoreKind, VarStore};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
pub use volume::Limits;

/// Stores decoded in v1 (buffer-backed).
pub const DECODED_STORES: [&str; 2] = ["Setup", "CpuSetup"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BiosIdentity {
    /// Full embedded identifier, e.g. `E7D91IMS.HJ0`.
    pub bios_id: String,
    /// Motherboard project, e.g. `7D91`.
    pub board: String,
    /// Firmware flavor/branch, e.g. `IMS`.
    pub flavor: String,
    /// Release revision, e.g. `HJ0`.
    pub revision: String,
    pub build_date: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupModule {
    pub file_guid: String,
    pub name: Option<String>,
    pub container_path: String,
    pub pe32_sha256: String,
    pub forms_package_offset: usize,
    pub strings_package_offset: usize,
    pub language: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BiosSchema {
    pub sha256: String,
    pub size: usize,
    pub identity: BiosIdentity,
    pub setup_module: SetupModule,
    pub stores: Vec<VarStore>,
    pub questions: Vec<Question>,
    pub fingerprint: String,
    pub diagnostics: Vec<String>,
    /// Menu path per form id: page titles below the root page, each with the byte offset of
    /// the REF that opens it (for menu ordering). Display only.
    pub menus: BTreeMap<u16, Vec<(String, u32)>>,
    /// AMI optimized defaults for the decoded stores, when they can be read reliably.
    #[serde(skip)]
    pub std_defaults: Option<nvar::StdDefaults>,
    /// Why `std_defaults` is absent, or where it came from.
    pub std_defaults_note: String,
    /// Family and labels; not part of the fingerprint.
    pub platform: platform::Platform,
}

impl BiosSchema {
    pub fn store(&self, name: &str) -> Option<&VarStore> {
        self.stores
            .iter()
            .find(|s| s.name == name && s.kind == StoreKind::Buffer)
    }

    /// Menu path of a question's page, e.g. `["Overclocking", "Advanced CPU Configuration"]`.
    pub fn menu_path(&self, q: &Question) -> Vec<String> {
        self.menu_steps(q).into_iter().map(|(t, _)| t).collect()
    }

    /// Menu path with ordering keys: each page's REF offset, then the question's own offset.
    pub fn menu_order(&self, q: &Question) -> Vec<u32> {
        let mut v: Vec<u32> = self.menu_steps(q).into_iter().map(|(_, o)| o).collect();
        v.push(q.ifr_offset);
        v
    }

    fn menu_steps(&self, q: &Question) -> Vec<(String, u32)> {
        q.form_id
            .and_then(|f| self.menus.get(&f))
            .cloned()
            .unwrap_or_default()
    }

    /// Optimized-default raw value of a store field, if StdDefaults is available.
    pub fn default_raw(&self, store: &str, offset: u16, width: usize) -> Option<u64> {
        let d = self.std_defaults.as_ref()?.stores.get(store)?;
        crate::bytes::uint_at(d, usize::from(offset), width, "default").ok()
    }

    /// Named questions in the decoded stores.
    pub fn decoded_questions(&self) -> impl Iterator<Item = &Question> {
        self.questions
            .iter()
            .filter(|q| DECODED_STORES.contains(&q.store.as_str()) && !q.prompt.is_empty())
    }
}

/// Parse a BIOS image into the decode schema used with profiles (Intel family only).
/// Pure: no filesystem access.
pub fn parse_bios(image: &[u8], limits: Limits) -> Result<BiosSchema> {
    analyze(image, limits)?.schema
}

/// Everything read from one image: the browsable view of its menus, and the profile decode
/// schema when the family supports one.
#[derive(Debug)]
pub struct Analysis {
    pub image: BiosImage,
    pub schema: Result<BiosSchema>,
}

/// Whole-image view for browsing: every formset the BIOS menus show, with optimized defaults
/// per store. Display only; never used to decide writes.
#[derive(Debug, Clone, Serialize)]
pub struct BiosImage {
    pub sha256: String,
    pub size: usize,
    pub identity: BiosIdentity,
    pub platform: platform::Platform,
    pub formsets: Vec<Formset>,
    /// Defaults note for firmware set 0 (the one shown by default).
    pub defaults_note: String,
    pub diagnostics: Vec<String>,
    /// Complete firmware sets when the image holds more than one (32 MiB AM4 images); empty
    /// otherwise. `Formset::set` indexes this list; set 0 is shown by default.
    pub sets: Vec<FirmwareSet>,
    /// The setup browser's variable lists (`$SPF`), one per firmware set that has one.
    /// Analysis input only (record names for profile analysis); not part of any report.
    #[serde(skip)]
    pub variable_lists: Vec<spf::VariableList>,
    /// Modules holding the OC-profile code (`$MOS$` save/load, `$OCI$` trailer). Analysis
    /// input only; not part of any report.
    #[serde(skip)]
    pub profile_modules: profile_code::ProfileModules,
}

/// One of several complete firmware sets in an image. Labels and CPU hints are inferred from
/// module names; which set a board runs cannot be told from the image.
#[derive(Debug, Clone, Serialize)]
pub struct FirmwareSet {
    pub label: String,
    pub note: String,
    pub defaults_note: String,
}

/// How a formset got its place in the menu tree. Display only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// The main setup formset; its pages are the top level.
    Main,
    /// Linked from a page by a cross-formset reference (REF3/REF4).
    Linked,
    /// Not linked; placed under a main-formset page with the same title (inferred).
    TitleMatch,
    /// Not linked and no matching page: a top-level entry.
    TopLevel,
}

#[derive(Debug, Clone, Serialize)]
pub struct Formset {
    /// Display title; empty for the main setup formset (its pages are the top level).
    pub title: String,
    pub guid: String,
    pub module: SetupModule,
    /// Menu path of the page that links to this formset (empty for top-level entries).
    pub placed_under: Vec<String>,
    pub stores: Vec<VarStore>,
    pub questions: Vec<Question>,
    /// Menu path per form id, including this formset's own place in the tree.
    pub menus: BTreeMap<u16, Vec<(String, u32)>>,
    /// Optimized defaults per store name (StdDefaults, matched by name, GUID and size).
    #[serde(skip)]
    pub defaults: BTreeMap<String, Vec<u8>>,
    /// Stores with settings but no usable default, with the reason.
    pub defaults_missing: Vec<String>,
    pub placement: Placement,
    /// Index into `BiosImage::sets` (0 when the image has one set).
    pub set: usize,
    /// Display note about this formset (e.g. overlap with MSI's own pages).
    pub note: Option<String>,
}

impl Formset {
    pub fn menu_path(&self, q: &Question) -> Vec<String> {
        self.menu_steps(q).into_iter().map(|(t, _)| t).collect()
    }

    pub fn menu_order(&self, q: &Question) -> Vec<u32> {
        let mut v: Vec<u32> = self.menu_steps(q).into_iter().map(|(_, o)| o).collect();
        v.push(q.ifr_offset);
        v
    }

    fn menu_steps(&self, q: &Question) -> Vec<(String, u32)> {
        q.form_id
            .and_then(|f| self.menus.get(&f))
            .cloned()
            .unwrap_or_default()
    }

    pub fn default_raw(&self, store: &str, offset: u16, width: usize) -> Option<u64> {
        let d = self.defaults.get(store)?;
        crate::bytes::uint_at(d, usize::from(offset), width, "default").ok()
    }
}

/// Read an image once and hand it to the parser for its family. Fails only when the image is
/// not a recognised MSI BIOS; a family without profile support still yields a browsable view.
pub fn analyze(image: &[u8], limits: Limits) -> Result<Analysis> {
    if let Some(why) = not_a_bios_image(image) {
        return Err(Error::Unsupported(why));
    }
    let identity = find_identity(image)?;
    let walk = volume::walk_image(image, limits)?;
    let mut diagnostics = walk.diagnostics.clone();
    let scanned = scan::scan_formsets(&walk, limits, &mut diagnostics)?;
    let variable_lists = spf::find_lists(&walk, &mut diagnostics);
    let profile_modules = profile_code::find(&walk);
    let intel = scanned.iter().any(intel::is_candidate) || intel::has_intel_markers(&scanned);
    let amd = scanned
        .iter()
        .any(|s| s.pkg.stores.values().any(amd::is_amd_store));
    let ctx = Context {
        sha256: sha256_hex(image),
        size: image.len(),
        identity,
        defaults: nvar::read_std_defaults(image),
        diagnostics,
        variable_lists,
        profile_modules,
    };
    match (intel, amd) {
        (true, false) => Ok(intel::analyze(ctx, &scanned)),
        (false, true) => Ok(amd::analyze(ctx, &scanned)),
        (true, true) => Err(Error::Unsupported(
            "the image has both Intel (CpuSetup or ME/ICC setup) and AMD CBS/AOD settings stores; layout not recognised"
                .into(),
        )),
        (false, false) => Err(Error::Unsupported(
            "no setup formset defines Intel (CpuSetup or ME/ICC setup) or AMD CBS/AOD stores; layout not recognised"
                .into(),
        )),
    }
}

/// What every family parser starts from.
pub(crate) struct Context {
    pub sha256: String,
    pub size: usize,
    pub identity: BiosIdentity,
    pub defaults: std::result::Result<Option<nvar::DefaultsCopies>, String>,
    pub diagnostics: Vec<String>,
    pub variable_lists: Vec<spf::VariableList>,
    pub profile_modules: profile_code::ProfileModules,
}

impl Context {
    pub(crate) fn copies(&self) -> Option<&nvar::DefaultsCopies> {
        self.defaults.as_ref().ok().and_then(Option::as_ref)
    }

    pub(crate) fn defaults_note(&self, formsets: &[Formset]) -> String {
        defaults_note(&self.defaults, formsets)
    }

    pub(crate) fn image(
        &self,
        platform: platform::Platform,
        formsets: Vec<Formset>,
        diagnostics: Vec<String>,
    ) -> BiosImage {
        BiosImage {
            sha256: self.sha256.clone(),
            size: self.size,
            identity: self.identity.clone(),
            platform,
            defaults_note: self.defaults_note(
                &formsets
                    .iter()
                    .filter(|f| f.set == 0)
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            formsets,
            diagnostics,
            sets: Vec::new(),
            variable_lists: self.variable_lists.clone(),
            profile_modules: self.profile_modules.clone(),
        }
    }
}

/// "AMI StdDefaults ..., covers X of Y settings" for one set of formsets.
pub(crate) fn defaults_note(
    defaults: &std::result::Result<Option<nvar::DefaultsCopies>, String>,
    formsets: &[Formset],
) -> String {
    match defaults {
        Err(e) => format!("Optimized defaults could not be read reliably: {e}"),
        Ok(None) => "This BIOS image has no StdDefaults store".into(),
        Ok(Some(c)) => {
            let named = |f: &&Formset| {
                f.questions
                    .iter()
                    .filter(|q| !q.prompt.is_empty())
                    .map(|q| q.store.clone())
                    .collect::<Vec<_>>()
            };
            let (mut with, mut total) = (0usize, 0usize);
            for f in formsets {
                for store in named(&f) {
                    total += 1;
                    with += usize::from(f.defaults.contains_key(&store));
                }
            }
            format!(
                "AMI StdDefaults (optimized defaults), {} cop{}; covers {with} of {total} settings",
                c.copies.len(),
                if c.copies.len() == 1 { "y" } else { "ies" }
            )
        }
    }
}

/// The formset AMI's setup browser opens first (same GUID on every MSI image examined).
pub const AMI_SETUP_FORMSET: &str = "7B59104A-C00D-4158-87FF-F04D6396A915";

/// The main setup formset: AMI's, else the largest one defining a `Setup` buffer store.
pub(crate) fn find_main(scanned: &[scan::ScannedFormset]) -> Option<usize> {
    let by_guid: Vec<usize> = scanned
        .iter()
        .enumerate()
        .filter(|(_, s)| s.pkg.formset_guid == AMI_SETUP_FORMSET)
        .map(|(i, _)| i)
        .collect();
    if let [one] = by_guid.as_slice() {
        return Some(*one);
    }
    scanned
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            s.pkg
                .stores
                .values()
                .any(|v| v.name == "Setup" && v.kind == StoreKind::Buffer)
        })
        .max_by_key(|(_, s)| s.pkg.questions.len())
        .map(|(i, _)| i)
}

/// Build the browsable formsets: the main formset at the root, every formset it links to
/// (REF3, transitively) under the page holding the link, then `extras` as top-level entries
/// after everything else. Menu paths include each formset's place in the tree. Display only.
pub(crate) fn assemble(
    scanned: &[scan::ScannedFormset],
    main: Option<usize>,
    extras: &[usize],
    root: Option<&str>,
    defaults: Option<&nvar::DefaultsCopies>,
) -> Vec<Formset> {
    let inner: Vec<BTreeMap<u16, Vec<(String, u32)>>> = scanned
        .iter()
        .map(|s| menu_paths(&s.pkg.forms, &s.pkg.refs))
        .collect();
    // (scanned index, base path whose last step is the formset's own entry; empty for main)
    let mut placed: Vec<(usize, MenuPath, Placement)> = Vec::new();
    let mut included = BTreeSet::new();
    if let Some(m) = main {
        placed.push((m, Vec::new(), Placement::Main));
        included.insert(m);
    }
    let mut next = 0;
    while next < placed.len() {
        let (fi, base, _) = placed[next].clone();
        next += 1;
        for (parent, guid, _form, at) in &scanned[fi].pkg.xrefs {
            let Some(pp) = inner[fi].get(parent) else {
                continue;
            };
            for (ti, t) in scanned.iter().enumerate() {
                let in_root = root.is_none_or(|r| root_of(&t.module.container_path) == r);
                if in_root && t.pkg.formset_guid == *guid && included.insert(ti) {
                    let mut b = base.clone();
                    b.extend(pp.iter().cloned());
                    b.push((String::new(), *at));
                    placed.push((ti, b, Placement::Linked));
                }
            }
        }
    }
    for &e in extras {
        if !included.insert(e) {
            continue;
        }
        // Unlinked formsets go under a main page with the same title when there is one (AMD
        // Setup carries "AMD CBS"/"AMD Overclocking" pages where the AMD modules belong).
        let stub = main
            .and_then(|m| stub_path(&scanned[m], &inner[m], scanned[e].pkg.formset_title.trim()));
        match stub {
            Some(mut b) => {
                b.push((String::new(), u32::MAX));
                placed.push((e, b, Placement::TitleMatch));
            }
            None => placed.push((e, vec![(String::new(), u32::MAX)], Placement::TopLevel)),
        }
    }
    let order: Vec<usize> = placed.iter().map(|(i, _, _)| *i).collect();
    let labels = formset_labels(scanned, &order);
    placed
        .into_iter()
        .map(|(fi, mut base, placement)| {
            let s = &scanned[fi];
            let is_main = placement == Placement::Main;
            let title = if is_main {
                String::new()
            } else {
                labels.get(&fi).cloned().unwrap_or_default()
            };
            if let Some(last) = base.last_mut() {
                last.0 = title.clone();
            }
            let menus = inner[fi]
                .iter()
                .map(|(form, path)| {
                    let mut p = base.clone();
                    p.extend(path.iter().cloned());
                    (*form, p)
                })
                .collect();
            let (defaults, defaults_missing) = formset_defaults(s, defaults);
            Formset {
                title,
                guid: s.pkg.formset_guid.clone(),
                module: s.module.clone(),
                placed_under: base
                    .iter()
                    .take(base.len().saturating_sub(1))
                    .map(|(t, _)| t.clone())
                    .collect(),
                stores: s.pkg.stores.values().cloned().collect(),
                questions: s.pkg.questions.clone(),
                menus,
                defaults,
                defaults_missing,
                placement,
                set: 0,
                note: None,
            }
        })
        .collect()
}

/// Menu steps: (page title, order key) from the top level down.
type MenuPath = Vec<(String, u32)>;

/// Menu path of the shallowest reachable main-formset page titled `title`.
fn stub_path(
    main: &scan::ScannedFormset,
    paths: &BTreeMap<u16, Vec<(String, u32)>>,
    title: &str,
) -> Option<Vec<(String, u32)>> {
    if title.is_empty() {
        return None;
    }
    main.pkg
        .forms
        .iter()
        .filter(|(id, t)| t.trim() == title && main.pkg.refs.iter().any(|(_, c, _)| c == id))
        .filter_map(|(id, _)| paths.get(id))
        .min_by_key(|p| p.len())
        .cloned()
}

/// Top-level firmware volume of a container path (`fv@0x...`). Images with two complete
/// firmware sets (32 MiB AMD boards) hold one per top-level volume.
pub(crate) fn root_of(path: &str) -> &str {
    path.split('/').next().unwrap_or(path)
}

/// Family token from an AMD module name, e.g. `CbsSetupDxeRV` -> `RV`.
fn module_token(name: &str) -> &str {
    ["CbsSetupDxe", "AodSetupDxe", "AodDxe"]
        .iter()
        .find_map(|p| name.strip_prefix(p))
        .unwrap_or("")
}

/// Display titles. Formsets sharing a title (AMD ships one CBS formset per processor family)
/// get the distinguishing part of their main store's name, e.g. "AMD CBS (RPL)".
fn formset_labels(scanned: &[scan::ScannedFormset], idx: &[usize]) -> BTreeMap<usize, String> {
    let title = |i: usize| {
        let t = scanned[i].pkg.formset_title.trim().to_string();
        if t.is_empty() {
            scanned[i].module.name.clone().unwrap_or_default()
        } else {
            t
        }
    };
    let main_store = |i: usize| {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for q in &scanned[i].pkg.questions {
            *counts.entry(q.store.as_str()).or_default() += 1;
        }
        counts
            .into_iter()
            .max_by_key(|(_, n)| *n)
            .map(|(s, _)| s.to_string())
            .unwrap_or_default()
    };
    let mut out = BTreeMap::new();
    for &i in idx {
        let t = title(i);
        let peers: Vec<usize> = idx.iter().copied().filter(|&j| title(j) == t).collect();
        if peers.len() < 2 {
            out.insert(i, t);
            continue;
        }
        let names: Vec<String> = peers.iter().map(|&j| main_store(j)).collect();
        let common = names.iter().skip(1).fold(names[0].len(), |n, s| {
            names[0]
                .bytes()
                .zip(s.bytes())
                .take(n)
                .take_while(|(a, b)| a == b)
                .count()
        });
        let own = main_store(i);
        let mut suffix = own.get(common..).unwrap_or(&own).to_uppercase();
        if suffix.is_empty() {
            // Same store name everywhere (AM4 `AmdSetup`): use the module's family token.
            suffix = module_token(scanned[i].module.name.as_deref().unwrap_or("")).to_string();
        }
        out.insert(
            i,
            if suffix.is_empty() {
                t
            } else {
                format!("{t} ({suffix})")
            },
        );
    }
    out
}

/// Optimized defaults of each settings store in a formset, matched by name, GUID and size.
fn formset_defaults(
    s: &scan::ScannedFormset,
    defaults: Option<&nvar::DefaultsCopies>,
) -> (BTreeMap<String, Vec<u8>>, Vec<String>) {
    let mut have = BTreeMap::new();
    let mut missing = Vec::new();
    let used: BTreeSet<u16> = s.pkg.questions.iter().map(|q| q.store_id).collect();
    for st in s.pkg.stores.values() {
        if !used.contains(&st.id)
            || st.name.is_empty()
            || st.size == 0
            || st.kind == StoreKind::NameValue
        {
            continue;
        }
        match defaults.map(|d| d.pick(&st.name, &st.guid, usize::from(st.size))) {
            Some(Ok(d)) => {
                have.insert(st.name.clone(), d);
            }
            Some(Err(e)) => missing.push(e),
            None => missing.push(format!("{}: no StdDefaults", st.name)),
        }
    }
    (have, missing)
}

/// Strict optimized defaults for the decoded Intel stores: both or none.
pub(crate) fn decoded_defaults(
    ctx: &Context,
    stores: &[VarStore],
    diagnostics: &mut Vec<String>,
) -> (Option<nvar::StdDefaults>, String) {
    let wanted: Vec<(&str, &str, usize)> = DECODED_STORES
        .iter()
        .filter_map(|n| {
            stores
                .iter()
                .find(|s| s.name == *n && s.kind == StoreKind::Buffer)
                .map(|s| (*n, s.guid.as_str(), usize::from(s.size)))
        })
        .collect();
    let picked = match &ctx.defaults {
        Ok(Some(c)) => nvar::std_defaults_from(c, &wanted).map(Some),
        Ok(None) => Ok(None),
        Err(e) => Err(e.clone()),
    };
    match picked {
        Ok(Some(d)) => {
            let note = format!(
                "AMI StdDefaults (optimized defaults), {} identical cop{}",
                d.copies.len(),
                if d.copies.len() == 1 { "y" } else { "ies" }
            );
            (Some(d), note)
        }
        Ok(None) => (None, "This BIOS image has no StdDefaults store".into()),
        Err(e) => {
            diagnostics.push(format!("optimized defaults unavailable: {e}"));
            (
                None,
                format!("Optimized defaults could not be read reliably: {e}"),
            )
        }
    }
}

/// Breadth-first menu paths from the first form (the formset's root page, whose own title is
/// omitted). Self-links and cycles are ignored; the first path found wins. Forms not reachable
/// from the root get their own title as a one-element path.
fn menu_paths(
    forms: &[(u16, String)],
    refs: &[(u16, u16, u32)],
) -> BTreeMap<u16, Vec<(String, u32)>> {
    let title = |id: u16| {
        forms
            .iter()
            .find(|(f, _)| *f == id)
            .map(|(_, t)| t.clone())
            .unwrap_or_default()
    };
    let mut out: BTreeMap<u16, Vec<(String, u32)>> = BTreeMap::new();
    let Some((root, _)) = forms.first() else {
        return out;
    };
    out.insert(*root, Vec::new());
    let mut queue = VecDeque::from([*root]);
    while let Some(parent) = queue.pop_front() {
        let base = out.get(&parent).cloned().unwrap_or_default();
        for (_, child, at) in refs.iter().filter(|(p, _, _)| *p == parent) {
            if out.contains_key(child) || !forms.iter().any(|(f, _)| f == child) {
                continue;
            }
            let mut path = base.clone();
            path.push((title(*child), *at));
            out.insert(*child, path);
            queue.push_back(*child);
        }
    }
    for (id, t) in forms {
        out.entry(*id)
            .or_insert_with(|| vec![(t.clone(), u32::MAX)]);
    }
    out
}

/// Recognise the common wrong files given as a BIOS image by their first bytes: an archive (MSI
/// ships BIOS images zipped) or an OC profile. `head` is the start of the file. Magic bytes only.
pub fn not_a_bios_image(head: &[u8]) -> Option<String> {
    const ARCHIVES: &[(&[u8], &str)] = &[
        (b"PK\x03\x04", "zip"),
        (b"7z\xBC\xAF\x27\x1C", "7z"),
        (b"Rar!\x1A\x07", "RAR"),
        (b"\x1F\x8B\x08", "gzip"),
    ];
    if let Some((_, kind)) = ARCHIVES.iter().find(|(m, _)| head.starts_with(m)) {
        return Some(format!(
            "this is a {kind} archive, not a BIOS image. Extract it and open the BIOS file inside (its name looks like E7D91IMS.HJ0)"
        ));
    }
    head.starts_with(crate::format::MAGIC).then(|| {
        "this is an OC profile, not a BIOS image; the BIOS and profile files are probably swapped"
            .to_string()
    })
}

/// Split an MSI BIOS identifier (`E` + 4-char board + 3/4-char flavor + `.` + 3/4-char revision,
/// e.g. `E7D91IMS.HJ0`, `E7E34IMSI.1AF0`) into (board, flavor, revision). Pure; no date needed.
pub fn parse_bios_id(s: &str) -> Option<(String, String, String)> {
    let alnum = |t: &str| {
        !t.is_empty()
            && t.bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    };
    let (head, revision) = s.split_once('.')?;
    let board = head.strip_prefix('E')?.get(..4)?;
    let flavor = head.get(5..)?;
    if !(alnum(board)
        && alnum(flavor)
        && alnum(revision)
        && (3..=4).contains(&flavor.len())
        && (3..=4).contains(&revision.len()))
    {
        return None;
    }
    Some((board.into(), flavor.into(), revision.into()))
}

/// Find the embedded MSI BIOS identifier: `E` + 4-char board + 3-char flavor + `.` + 3-char revision,
/// followed by a space and `MM/DD/YYYY`. Exactly one distinct value must exist.
pub fn find_identity(image: &[u8]) -> Result<BiosIdentity> {
    // Layout: E BBBB FFF[F] . RRR[R] ' ' MM/DD/YYYY. Older boards use 3+3 (`E7D91IMS.HJ0`),
    // newer ones 4+4 (`E7E34IMSI.1AF0`, `E7E48AMSI.2A35`). Entries: (id, board, flavor, rev, date).
    let mut found: BTreeSet<(String, String, String, String, String)> = BTreeSet::new();
    let n = image.len();
    let alnum = |c: &[u8]| {
        c.iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    };
    let digits = |c: &[u8]| c.iter().all(|b| b.is_ascii_digit());
    for i in 0..n {
        if image[i] != b'E' {
            continue;
        }
        for fl in [3usize, 4] {
            for rl in [3usize, 4] {
                let dot = i + 5 + fl;
                let sp = dot + 1 + rl;
                let end = sp + 11;
                if end > n || image[dot] != b'.' || image[sp] != b' ' {
                    continue;
                }
                let c = &image[i..end];
                let d = &c[sp - i + 1..];
                if alnum(&c[1..5 + fl])
                    && alnum(&c[6 + fl..6 + fl + rl])
                    && digits(&d[0..2])
                    && d[2] == b'/'
                    && digits(&d[3..5])
                    && d[5] == b'/'
                    && digits(&d[6..10])
                {
                    let t = |r: std::ops::Range<usize>| String::from_utf8_lossy(&c[r]).into_owned();
                    found.insert((
                        t(0..sp - i),
                        t(1..5),
                        t(5..5 + fl),
                        t(6 + fl..6 + fl + rl),
                        String::from_utf8_lossy(&d[0..10]).into_owned(),
                    ));
                }
            }
        }
    }
    let mut ids = found.into_iter();
    match (ids.next(), ids.next()) {
        (Some((bios_id, board, flavor, revision, build_date)), None) => Ok(BiosIdentity {
            bios_id,
            board,
            flavor,
            revision,
            build_date,
        }),
        (None, _) => Err(Error::Unsupported(
            "no MSI BIOS identifier found in image".into(),
        )),
        (Some(a), Some(b)) => Err(Error::IdentityMismatch(format!(
            "conflicting BIOS identifiers in image: {} / {}",
            a.0, b.0
        ))),
    }
}

#[cfg(test)]
mod wrong_input_tests {
    use super::*;

    #[test]
    fn archives_are_named_by_magic_bytes() {
        for (head, kind) in [
            (&b"PK\x03\x04rest"[..], "zip"),
            (b"7z\xBC\xAF\x27\x1C\0\x04", "7z"),
            (b"Rar!\x1A\x07\x01\0", "RAR"),
            (b"\x1F\x8B\x08\0", "gzip"),
        ] {
            let m = not_a_bios_image(head).expect(kind);
            assert!(m.contains(&format!("{kind} archive")), "{m}");
            assert!(
                m.contains("Extract it") && m.contains("E7D91IMS.HJ0"),
                "{m}"
            );
        }
    }

    #[test]
    fn a_profile_given_as_the_bios_is_called_one() {
        let m = not_a_bios_image(b"$MOS$E7D91IMS.HJ0").expect("profile");
        assert!(
            m.contains("OC profile, not a BIOS image") && m.contains("swapped"),
            "{m}"
        );
    }

    #[test]
    fn firmware_and_empty_input_pass_through() {
        assert!(not_a_bios_image(&[0xFF; 16]).is_none());
        assert!(not_a_bios_image(b"").is_none());
        // The error kind stays `unsupported` (exit code 5).
        let e = analyze(b"PK\x03\x04", Limits::default()).expect_err("zip");
        assert!(matches!(e, Error::Unsupported(_)), "{e}");
        assert_eq!(e.exit_code(), 5);
    }
}
