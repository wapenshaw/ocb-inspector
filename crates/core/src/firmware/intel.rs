//! MSI Intel family (`msi-intel-setup`): one AMI Setup formset holds every page.
//!
//! Three layouts are recognised from structure:
//! - client split (300-800 series): CPU tuning in a `CpuSetup` store next to `Setup`. Profiles
//!   bind to those two stores, so this layout also builds the decode schema; its fingerprint
//!   and selection rules are unchanged from the original single-family parser. Profiles are
//!   offered only for the CpuSetup layout whose profiles have been examined.
//! - merged Setup (older client boards such as H310): CPU tuning lives in `Setup`, no `CpuSetup`.
//! - socket (X299 HEDT): CPU tuning in `Setup` plus `Socket*` EFI varstores, shown by separate
//!   "Socket Configuration"/"Platform Configuration" formsets the AMI browser links itself.
//!
//! The last two are browse-only: no profile from them has been examined.

use super::platform::{self, Vendor};
use super::scan::ScannedFormset;
use super::{
    Analysis, BiosSchema, Context, DECODED_STORES, assemble, decoded_defaults, find_main,
    menu_paths,
};
use crate::bytes::sha256_hex;
use crate::error::{Error, Result};
use crate::firmware::ifr::StoreKind;
use std::collections::BTreeSet;

/// Stores every MSI Intel image examined defines and no AMD image does (by name).
const INTEL_MARKERS: [&str; 4] = [
    "MeSetupStorage",
    "CpuSetupVolatileData",
    "IccAdvancedSetupDataVar",
    "OcInfoPassToSetup",
];

/// CpuSetup size of the layout whose OC profiles have been examined (MS-7D91, 600/700 series).
/// Other sizes mean another generation, where recipe prompts can carry different option meanings.
const PROFILE_CPUSETUP_SIZE: u16 = 961;

/// Intel structure without the decoded store pair: at least two marker stores, anywhere.
pub fn has_intel_markers(scanned: &[ScannedFormset]) -> bool {
    let found: BTreeSet<&str> = scanned
        .iter()
        .flat_map(|s| s.pkg.stores.values())
        .filter_map(|v| INTEL_MARKERS.iter().find(|m| **m == v.name).copied())
        .collect();
    found.len() >= 2
}

/// Separate HEDT formsets holding socket/platform settings (not linked by IFR).
fn is_socket_formset(s: &ScannedFormset) -> bool {
    s.pkg.stores.values().any(|v| {
        matches!(v.kind, StoreKind::Efi | StoreKind::Buffer)
            && (v.name.starts_with("Socket") || v.name == "ServerSetup")
    })
}

/// A formset that defines both decoded stores as buffers.
pub fn is_candidate(s: &ScannedFormset) -> bool {
    DECODED_STORES.iter().all(|n| {
        s.pkg
            .stores
            .values()
            .any(|v| v.name == *n && v.kind == StoreKind::Buffer)
    })
}

pub(crate) fn analyze(ctx: Context, scanned: &[ScannedFormset]) -> Analysis {
    let (platform, notes) = platform::describe(&ctx.identity, Vendor::Intel);
    let mut diagnostics = ctx.diagnostics.clone();
    diagnostics.extend(notes);
    let candidates: Vec<usize> = scanned
        .iter()
        .enumerate()
        .filter(|(_, s)| is_candidate(s))
        .map(|(i, _)| i)
        .collect();
    let main = match candidates.as_slice() {
        [one] => Some(*one),
        _ => find_main(scanned),
    };
    let socket: Vec<usize> = scanned
        .iter()
        .enumerate()
        .filter(|(i, s)| Some(*i) != main && is_socket_formset(s))
        .map(|(i, _)| i)
        .collect();
    let mut platform = platform;
    if let [one] = candidates.as_slice() {
        let size = scanned[*one]
            .pkg
            .stores
            .values()
            .find(|v| v.name == "CpuSetup" && v.kind == StoreKind::Buffer)
            .map(|v| v.size);
        if platform.profiles_supported && size != Some(PROFILE_CPUSETUP_SIZE) {
            platform.profiles_supported = false;
            platform.profiles_note = format!(
                "This BIOS uses a {}-byte CpuSetup layout (another Intel generation than the profiles examined so far, which use {PROFILE_CPUSETUP_SIZE}), and some settings with the same name mean different things there. Profiles cannot be opened yet; browse the BIOS instead.",
                size.map_or_else(|| "different".to_string(), |s| s.to_string())
            );
        }
    } else {
        platform.profiles_supported = false;
        platform.profiles_note = "This older Intel layout keeps CPU settings in Setup (no CpuSetup store), and no profile from it has been examined. Browse the BIOS instead."
            .into();
    }
    let formsets = assemble(scanned, main, &socket, None, ctx.copies());
    let schema = match candidates.as_slice() {
        [one] => schema(&ctx, &scanned[*one], platform.clone(), diagnostics.clone()),
        [] if !socket.is_empty() => Err(Error::Unsupported(
            "this is an Intel HEDT (socket) BIOS: CPU settings live in Setup and Socket* stores rather than CpuSetup, and no profile from this layout has been examined, so profiles cannot be opened. The BIOS itself can be browsed."
                .into(),
        )),
        [] => Err(Error::Unsupported(
            "this older Intel BIOS keeps CPU settings in Setup and has no CpuSetup store, and no profile from this layout has been examined, so profiles cannot be opened. The BIOS itself can be browsed."
                .into(),
        )),
        many => Err(Error::Unsupported(format!(
            "{} ambiguous setup forms packages define Setup and CpuSetup",
            many.len()
        ))),
    };
    Analysis {
        image: ctx.image(platform, formsets, diagnostics),
        schema,
    }
}

fn schema(
    ctx: &Context,
    cand: &ScannedFormset,
    platform: platform::Platform,
    mut diagnostics: Vec<String>,
) -> Result<BiosSchema> {
    let stores: Vec<_> = cand.pkg.stores.values().cloned().collect();
    // Duplicate store names with different sizes are ambiguous.
    for name in DECODED_STORES {
        let sizes: BTreeSet<u16> = stores
            .iter()
            .filter(|s| s.name == name)
            .map(|s| s.size)
            .collect();
        if sizes.len() != 1 {
            return Err(Error::Unsupported(format!(
                "store {name} has ambiguous definitions: {sizes:?}"
            )));
        }
    }

    let fingerprint = {
        let mut lines: Vec<String> = cand
            .pkg
            .questions
            .iter()
            .filter(|q| DECODED_STORES.contains(&q.store.as_str()))
            .map(|q| {
                format!(
                    "{}|{}|{}|{}|{:?}",
                    q.store, q.offset, q.width_bits, q.prompt, q.kind
                )
            })
            .collect();
        lines.sort();
        for s in &stores {
            lines.push(format!("store|{}|{}|{}", s.name, s.guid, s.size));
        }
        sha256_hex(lines.join("\n").as_bytes())
    };

    let menus = menu_paths(&cand.pkg.forms, &cand.pkg.refs);
    let (std_defaults, std_defaults_note) = decoded_defaults(ctx, &stores, &mut diagnostics);

    Ok(BiosSchema {
        sha256: ctx.sha256.clone(),
        size: ctx.size,
        identity: ctx.identity.clone(),
        setup_module: cand.module.clone(),
        stores,
        questions: cand.pkg.questions.clone(),
        fingerprint,
        diagnostics,
        menus,
        std_defaults,
        std_defaults_note,
        platform,
    })
}
