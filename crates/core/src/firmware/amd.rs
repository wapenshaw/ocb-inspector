//! MSI AMD family (`msi-amd-cbs`: AM4, AM5, TRX40): the AMI Setup formset holds the board pages,
//! and AMD's own modules add more formsets with their own stores:
//!
//! - AMD CBS, one formset per processor family. AM5 names the store per family
//!   (`AmdSetupRPL`, `AmdSetupPHX`, `AmdSetupSTXKRK`); AM4 uses one name, `AmdSetup`, in every
//!   family module (`CbsSetupDxeZP/RV/RN/SSP`), so labels there come from the module name.
//! - AMD Overclocking: `AodSetup*` per family on AM5, one `AOD_SETUP` on AM4.
//! - AMD PBS (`AMD_PBS_SETUP`).
//!
//! Formsets the Setup pages link to are placed under the linking page; the rest appear as
//! top-level entries. 32 MiB AM4 images carry two complete firmware sets, one per top-level
//! volume; only the set holding the chosen Setup formset is shown (its StdDefaults copy too),
//! and the other is reported. There is no decode schema: no AMD OC profile has been examined,
//! so this family is browse-only.

use super::platform::{self, Vendor};
use super::scan::ScannedFormset;
use super::{
    AMI_SETUP_FORMSET, Analysis, Context, FirmwareSet, Formset, assemble, find_main, root_of,
};
use crate::error::Error;
use crate::firmware::ifr::{StoreKind, VarStore};
use crate::firmware::nvar::DefaultsCopies;
use std::collections::BTreeSet;

/// AMD settings stores that identify this family.
pub fn is_amd_store(v: &VarStore) -> bool {
    v.kind == StoreKind::Buffer
        && (v.name.starts_with("AmdSetup")
            || v.name.starts_with("AodSetup")
            || v.name == "AOD_SETUP")
}

fn is_amd_formset(s: &ScannedFormset) -> bool {
    s.pkg
        .stores
        .values()
        .any(|v| is_amd_store(v) || (v.kind == StoreKind::Buffer && v.name == "AMD_PBS_SETUP"))
}

pub(crate) fn analyze(mut ctx: Context, scanned: &[ScannedFormset]) -> Analysis {
    let (platform, notes) = platform::describe(&ctx.identity, Vendor::Amd);
    let mut diagnostics = ctx.diagnostics.clone();
    diagnostics.extend(notes);
    let main = find_main(scanned);
    if main.is_none() {
        diagnostics.push("no AMI Setup formset found; showing AMD formsets only".into());
    }
    let schema = Err(Error::Unsupported(
        "this is an AMD BIOS: its CPU settings live in AMD CBS/AOD stores rather than CpuSetup, and no AMD OC profile layout is known yet, so profiles cannot be opened. The BIOS itself can be browsed."
            .into(),
    ));
    // One AMI Setup formset per top-level volume means several complete firmware sets.
    let mut sets: Vec<(&str, usize)> = Vec::new();
    for (i, s) in scanned.iter().enumerate() {
        if s.pkg.formset_guid != AMI_SETUP_FORMSET {
            continue;
        }
        let r = root_of(&s.module.container_path);
        match sets.iter_mut().find(|(sr, _)| *sr == r) {
            Some(e) if setup_size(s) > setup_size(&scanned[e.1]) => e.1 = i,
            Some(_) => {}
            None => sets.push((r, i)),
        }
    }
    if sets.len() < 2 || main.is_none() {
        let extras = amd_formsets(scanned, None);
        let mut formsets = assemble(scanned, main, &extras, None, ctx.copies());
        annotate(&mut formsets);
        return Analysis {
            image: ctx.image(platform, formsets, diagnostics),
            schema,
        };
    }
    // Largest Setup first: it is shown by default.
    sets.sort_by_key(|(_, m)| std::cmp::Reverse(setup_size(&scanned[*m])));
    let all = ctx.defaults.clone();
    let mut formsets = Vec::new();
    let mut infos = Vec::new();
    for (k, (root, m)) in sets.iter().enumerate() {
        let defaults = own_copies(&all, &scanned[*m]);
        let extras = amd_formsets(scanned, Some(root));
        let mut fs = assemble(
            scanned,
            Some(*m),
            &extras,
            Some(root),
            defaults.as_ref().ok().and_then(Option::as_ref),
        );
        annotate(&mut fs);
        for f in &mut fs {
            f.set = k;
        }
        let tokens: Vec<&str> = fs
            .iter()
            .filter_map(|f| f.module.name.as_deref())
            .filter_map(|n| n.strip_prefix("CbsSetupDxe"))
            .collect();
        let hints: Vec<&str> = tokens.iter().filter_map(|t| cpu_hint(t)).collect();
        infos.push(FirmwareSet {
            label: if tokens.is_empty() {
                format!("Setup {} bytes", setup_size(&scanned[*m]).unwrap_or(0))
            } else {
                format!("CBS {}", tokens.join(" + "))
            },
            note: format!(
                "Firmware set at {root} with a {}-byte Setup.{} Which set a board runs cannot be told from the image.",
                setup_size(&scanned[*m]).unwrap_or(0),
                if hints.is_empty() {
                    String::new()
                } else {
                    format!(" Its AMD modules suggest {} (inferred from module names).", hints.join(", "))
                }
            ),
            defaults_note: super::defaults_note(&defaults, &fs),
        });
        if k == 0 {
            ctx.defaults = defaults;
        }
        formsets.extend(fs);
    }
    diagnostics.push(format!(
        "this image holds {} complete firmware sets ({}); set 1 is shown by default and the others can be selected",
        infos.len(),
        infos
            .iter()
            .enumerate()
            .map(|(n, i)| format!("{}: {}", n + 1, i.label))
            .collect::<Vec<_>>()
            .join("; ")
    ));
    let mut image = ctx.image(platform, formsets, diagnostics);
    image.sets = infos;
    Analysis { image, schema }
}

/// AMD formsets, optionally within one firmware set; identical copies (same formset, stores and
/// question count) once.
fn amd_formsets(scanned: &[ScannedFormset], root: Option<&str>) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    scanned
        .iter()
        .enumerate()
        .filter(|(_, s)| is_amd_formset(s))
        .filter(|(_, s)| root.is_none_or(|r| root_of(&s.module.container_path) == r))
        .filter(|(_, s)| {
            let stores: Vec<(String, u16)> = s
                .pkg
                .stores
                .values()
                .map(|v| (v.name.clone(), v.size))
                .collect();
            seen.insert((s.pkg.formset_guid.clone(), stores, s.pkg.questions.len()))
        })
        .map(|(i, _)| i)
        .collect()
}

/// StdDefaults copies whose `Setup` has the size of this set's Setup (copies otherwise must
/// agree, and the two AM4 sets carry different Setup layouts). Falls back to every copy.
fn own_copies(
    all: &std::result::Result<Option<DefaultsCopies>, String>,
    main: &ScannedFormset,
) -> std::result::Result<Option<DefaultsCopies>, String> {
    let (Ok(Some(copies)), Some(size)) = (all, setup_size(main)) else {
        return all.clone();
    };
    let key = ("Setup".to_string(), setup_guid(main));
    let own: Vec<_> = copies
        .copies
        .iter()
        .filter(|(_, vars)| {
            matches!(vars.get(&key).map(Vec::as_slice), Some([d]) if d.len() == usize::from(size))
        })
        .cloned()
        .collect();
    if own.is_empty() {
        all.clone()
    } else {
        Ok(Some(DefaultsCopies { copies: own }))
    }
}

/// CPU families an AM4 CBS module suggests. Inferred from AMD code names, not from the image.
fn cpu_hint(token: &str) -> Option<&'static str> {
    Some(match token {
        "ZP" => "Zen/Zen+ Ryzen 1000/2000 (Summit/Pinnacle Ridge)",
        "RV" => "Raven Ridge/Picasso APUs",
        "RN" => "Renoir/Cezanne APUs",
        "SSP" => "Zen 2/Zen 3 Ryzen 3000/5000 (Matisse/Vermeer)",
        _ => return None,
    })
}

/// Notes on AMD Overclocking formsets: MSI's own Setup pages can repeat their settings.
fn annotate(formsets: &mut [Formset]) {
    for f in formsets {
        let aod = f
            .stores
            .iter()
            .any(|v| v.name.starts_with("AodSetup") || v.name == "AOD_SETUP");
        if aod {
            f.note = Some(
                "AMD's own overclocking module. MSI's Overclocking pages in Setup can hold overlapping settings (for example Precision Boost Overdrive); which page the BIOS shows cannot be told from the image."
                    .into(),
            );
        }
    }
}

fn setup_store(s: &ScannedFormset) -> Option<&VarStore> {
    s.pkg
        .stores
        .values()
        .find(|v| v.name == "Setup" && v.kind == StoreKind::Buffer)
}

fn setup_size(s: &ScannedFormset) -> Option<u16> {
    setup_store(s).map(|v| v.size)
}

fn setup_guid(s: &ScannedFormset) -> String {
    setup_store(s).map(|v| v.guid.clone()).unwrap_or_default()
}
