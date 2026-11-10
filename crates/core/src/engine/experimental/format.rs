//! What a BIOS image says about the profiles it writes, without any profile (operation
//! `profile_format`). Read-only and display only: it lists the setup browser's variable list
//! (record id = position), where the profile code lives and the file layout, each part labelled
//! with what it rests on. Nothing here reads, maps or writes a profile.

use super::{inventory, questions, set_roots, variable_list};
use crate::contracts::*;
use crate::error::{Error, Result};
use crate::firmware::BiosImage;
use crate::firmware::ifr::StoreKind;
use crate::firmware::profile_code::{self, traced_facts};
use crate::format::probe::CMOS_BLOCK_LEN;

pub const OPERATION: &str = "profile_format";

fn fact(part: &str, detail: String, basis: &str) -> FormatFact {
    FormatFact {
        part: part.into(),
        detail,
        basis: basis.into(),
    }
}

/// The profile format of one firmware set of `img` (set 0 when `set` is `None`).
pub fn profile_format(
    img: &BiosImage,
    profiles_supported: bool,
    set: Option<usize>,
) -> Result<Report<ProfileFormatPayload>> {
    let sets = img.sets.len().max(1);
    let set = set.unwrap_or(0);
    if set >= sets {
        return Err(Error::Request(format!(
            "this image has no firmware set {}; it has {sets}",
            set + 1
        )));
    }
    let traced = profile_code::traced(&img.sha256);
    let roots = set_roots(img, set);
    let code: Vec<ProfileCodeModule> = img
        .profile_modules
        .modules
        .iter()
        .filter(|m| roots.as_ref().is_none_or(|r| r.contains(m.root.as_str())))
        .map(|m| ProfileCodeModule {
            name: m.name.clone(),
            guid: m.guid.clone(),
            sha256: m.sha256.clone(),
            holds_mos: m.mos,
            holds_oci: m.oci,
        })
        .collect();
    let has_mos = code.iter().any(|m| m.holds_mos);
    let oci_in: Vec<&str> = code
        .iter()
        .filter(|m| m.holds_oci)
        .map(|m| m.name.as_str())
        .collect();
    let derived = if traced { "firmware_code" } else { "expected" };
    let id = &img.identity.bios_id;

    let mut layout = Vec::new();
    if !has_mos {
        layout.push(fact(
            "profile_code",
            "No module of this set holds the $MOS$ signature; where this BIOS saves profiles is unknown".into(),
            "image_structure",
        ));
    }
    layout.push(fact(
        "header",
        format!(
            "$MOS$, then the BIOS id over its full length ({} bytes: {id}), a 32-bit checksum and the record count",
            id.len()
        ),
        derived,
    ));
    layout.push(fact(
        "checksum",
        "Minus the 32-bit wrapping sum of every other byte of the file".into(),
        derived,
    ));
    layout.push(fact(
        "records",
        "Each record is a u32 id, a u16 size and the variable's bytes; the id is the variable's position in the list below, and records follow list order".into(),
        derived,
    ));
    if traced {
        layout.push(fact(
            "skipped",
            format!(
                "Positions {} and the variable {} are never saved",
                traced_facts::SKIPPED_INDICES
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                traced_facts::SKIPPED_NAME
            ),
            "firmware_code",
        ));
    }
    layout.push(fact(
        "cmos",
        format!(
            "{CMOS_BLOCK_LEN} bytes: the CMOS contents at save time; loading writes bytes 0x{:02X}-0x{:02X} back",
            traced_facts::CMOS_RESTORED.0,
            traced_facts::CMOS_RESTORED.1
        ),
        derived,
    ));
    layout.push(if oci_in.is_empty() {
        fact(
            "trailer",
            "No module holds $OCI$, so files are expected to end right after the CMOS block".into(),
            "expected",
        )
    } else if profile_code::trailer_traced(&img.sha256) {
        fact(
            "trailer",
            format!(
                "$OCI$, then board, revision, CPU brand and save time, then six numbers of unknown meaning (code in {})",
                oci_in.join(", ")
            ),
            "firmware_code",
        )
    } else {
        fact(
            "trailer",
            format!(
                "A $OCI$ trailer is expected: {} holds that signature. Its contents were traced only on MS-7D91 (board, revision, CPU brand, save time, six numbers), not here",
                oci_in.join(", ")
            ),
            "expected",
        )
    });

    let vars = inventory(img, set);
    let (variable_list, list_problem) = match variable_list(img, set) {
        Ok(list) => {
            let mut skipped_name = false;
            let entries = list
                .entries
                .iter()
                .map(|e| {
                    let stores: Vec<_> = vars
                        .iter()
                        .filter(|v| {
                            v.kind == StoreKind::Buffer
                                && v.supported()
                                && v.guid == e.guid
                                && v.name == e.name
                        })
                        .collect();
                    let store = match stores.as_slice() {
                        [one] => Some(*one),
                        _ => None,
                    };
                    let saved = if !traced {
                        "unknown"
                    } else if traced_facts::SKIPPED_INDICES.contains(&e.index)
                        || (e.name == traced_facts::SKIPPED_NAME && !skipped_name)
                    {
                        "skipped"
                    } else {
                        "saved"
                    };
                    skipped_name |= e.name == traced_facts::SKIPPED_NAME;
                    let mut notes = Vec::new();
                    if e.size == 0 {
                        notes.push("the setup data declares no size");
                    }
                    if traced
                        && (traced_facts::SIZE_TOLERANT_INDICES.contains(&e.index)
                            || traced_facts::SIZE_TOLERANT_NAMES.contains(&e.name.as_str()))
                    {
                        notes.push("loading accepts a different size for this record");
                    }
                    ListedEntryInfo {
                        index: e.index,
                        guid: e.guid.clone(),
                        name: e.name.clone(),
                        attributes: e.attributes,
                        declared_size: e.size,
                        store_size: store.map(|v| v.size),
                        field_count: store.map_or(0, |v| questions(img, v).len()),
                        saved: saved.into(),
                        note: (!notes.is_empty()).then(|| notes.join("; ")),
                    }
                })
                .collect();
            (
                Some(VariableListInfo {
                    container_path: list.container_path.clone(),
                    sha256: list.sha256.clone(),
                    entries,
                }),
                None,
            )
        }
        Err(e) => (None, Some(e)),
    };

    let mut limitations =
        vec!["Exported profiles have been checked against this layout only on MS-7D91".to_string()];
    if !traced {
        limitations.push(
            "This release's profile code was not traced: parts marked expected follow the traced releases, and which positions are saved is unknown".into(),
        );
    }
    limitations.push(
        "Record ids belong to this exact release: another release's list can shift them".into(),
    );

    let payload = ProfileFormatPayload {
        bios: BiosInput {
            sha256: img.sha256.clone(),
            size: img.size,
            bios_id: id.clone(),
            set: Some(set),
            sets: img.sets.iter().map(|s| s.label.clone()).collect(),
            family: img.platform.family.clone(),
            profiles_supported,
        },
        traced,
        code,
        layout,
        variable_list,
        list_problem,
        limitations,
    };
    let mut r = Report::new(OPERATION, payload, Vec::new());
    r.payload_version = PROFILE_FORMAT_PAYLOAD_VERSION.into();
    Ok(r)
}
