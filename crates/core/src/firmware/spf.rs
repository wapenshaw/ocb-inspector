//! AMI setup-browser variable list (`$SPF` setup data). Read-only, bounded.
//!
//! Format facts, recovered from firmware code and checked on every image of the local corpus
//! (`docs/FIRMWARE_PROFILE_FORMAT_RESEARCH.md`): the browser's setup data lives in the FFS file
//! `FE612B72-203C-47B1-8560-A66D946EB371` and starts with the signature `$SPF`. The u32 at
//! +0x34 is the offset of the variable list: a u32 count, then `count` u32 offsets (relative to
//! the list) of 124-byte entries `{GUID, CHAR16 name[40], u32 attributes, u32 (not read;
//! not the list position), 16 bytes, u32 size}`. The browser's OC-profile save and load use the position in this list as the
//! record id, so the list names every record of a profile saved by this exact release.
//!
//! Nothing here decides writes; the list is display/analysis input only.

use crate::bytes::{guid_string, sha256_hex, slice, u32_at};
use crate::error::{Error, Result};

use super::volume::Walk;

/// FFS file that holds the setup data.
pub const SETUP_DATA_FILE: &str = "FE612B72-203C-47B1-8560-A66D946EB371";
const SIGNATURE: &[u8; 4] = b"$SPF";
const LIST_OFFSET_AT: usize = 0x34;
const ENTRY_LEN: usize = 124;
const NAME_AT: usize = 16;
const NAME_UNITS: usize = 40;
const ATTRIBUTES_AT: usize = 96;
const SIZE_AT: usize = 120;
const MAX_ENTRIES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedVariable {
    /// Position in the list: the record id a profile uses for this variable.
    pub index: u32,
    pub guid: String,
    pub name: String,
    pub attributes: u32,
    /// Size declared in the setup data (0 when none is declared).
    pub size: u32,
}

#[derive(Debug, Clone)]
pub struct VariableList {
    /// Top-level volume the list belongs to (`fv@0x…`), for matching a firmware set.
    pub root: String,
    pub container_path: String,
    /// Digest of the list bytes (count, offsets and entries).
    pub sha256: String,
    pub entries: Vec<ListedVariable>,
}

/// Every `$SPF` variable list in the setup-data files of the walk. Lists that fail a bound
/// check are reported in `diagnostics`, never guessed.
pub fn find_lists(walk: &Walk, diagnostics: &mut Vec<String>) -> Vec<VariableList> {
    let mut out = Vec::new();
    for (i, buf) in walk.arena.iter().enumerate() {
        let Some(path) = walk.arena_paths.get(i) else {
            continue;
        };
        if !path.contains(&format!("/{SETUP_DATA_FILE}/")) {
            continue;
        }
        let mut from = 0;
        while let Some(p) = buf[from..].windows(4).position(|w| w == SIGNATURE) {
            let at = from + p;
            from = at + 4;
            match parse_list(&buf[at..]) {
                Ok((entries, sha256)) => out.push(VariableList {
                    root: super::root_of(path).to_string(),
                    container_path: path.clone(),
                    sha256,
                    entries,
                }),
                Err(e) => diagnostics.push(format!("{path}: setup data at 0x{at:x} not read: {e}")),
            }
        }
    }
    out
}

/// Parses the list of one blob starting at its `$SPF` signature.
pub fn parse_list(blob: &[u8]) -> Result<(Vec<ListedVariable>, String)> {
    if !blob.starts_with(SIGNATURE) {
        return Err(Error::malformed("setup data", "no $SPF signature"));
    }
    let list = u32_at(blob, LIST_OFFSET_AT, "setup data header")? as usize;
    let count = u32_at(blob, list, "variable list")? as usize;
    if count == 0 || count > MAX_ENTRIES {
        return Err(Error::malformed("variable list", format!("count {count}")));
    }
    let mut entries = Vec::with_capacity(count);
    let mut end = list + 4 + count * 4;
    for i in 0..count {
        let rel = u32_at(blob, list + 4 + i * 4, "variable list offsets")? as usize;
        let at = list
            .checked_add(rel)
            .ok_or_else(|| Error::malformed("variable list", "offset overflow"))?;
        let e = slice(blob, at, ENTRY_LEN, "variable entry")?;
        end = end.max(at + ENTRY_LEN);
        let units: Vec<u16> = e[NAME_AT..NAME_AT + NAME_UNITS * 2]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .take_while(|&c| c != 0)
            .collect();
        if units.is_empty()
            || units.len() == NAME_UNITS
            || units.iter().any(|&c| !(0x20..0x7F).contains(&c))
        {
            return Err(Error::malformed(
                "variable entry",
                format!("entry {i} has no printable name"),
            ));
        }
        entries.push(ListedVariable {
            index: i as u32,
            guid: guid_string(&e[..16]),
            name: String::from_utf16_lossy(&units),
            attributes: u32_at(e, ATTRIBUTES_AT, "variable entry")?,
            size: u32_at(e, SIZE_AT, "variable entry")?,
        });
    }
    let sha256 = sha256_hex(slice(blob, list, end - list, "variable list")?);
    Ok((entries, sha256))
}

#[cfg(test)]
pub(crate) mod build {
    //! Synthetic `$SPF` blobs for tests (independent of any firmware bytes).

    /// (guid bytes, name, size) per entry.
    pub fn blob(entries: &[([u8; 16], &str, u32)]) -> Vec<u8> {
        let list = 0x40usize;
        let mut b = vec![0u8; list];
        b[..4].copy_from_slice(b"$SPF");
        b[0x34..0x38].copy_from_slice(&(list as u32).to_le_bytes());
        b.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        let table = 4 + entries.len() * 4;
        for i in 0..entries.len() {
            b.extend_from_slice(&((table + i * 124) as u32).to_le_bytes());
        }
        for (guid, name, size) in entries {
            let mut e = vec![0u8; 124];
            e[..16].copy_from_slice(guid);
            for (k, u) in name.encode_utf16().enumerate() {
                e[16 + k * 2..18 + k * 2].copy_from_slice(&u.to_le_bytes());
            }
            e[96..100].copy_from_slice(&7u32.to_le_bytes());
            e[120..124].copy_from_slice(&size.to_le_bytes());
            b.extend_from_slice(&e);
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_names_sizes_and_positions() {
        let b = build::blob(&[([0x11; 16], "Setup", 5081), ([0x22; 16], "CpuSetup", 961)]);
        let (v, _) = parse_list(&b).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(
            (v[1].index, v[1].name.as_str(), v[1].size),
            (1, "CpuSetup", 961)
        );
        assert_eq!(v[0].guid, guid_string(&[0x11; 16]));
    }

    #[test]
    fn rejects_truncated_and_implausible_lists() {
        let b = build::blob(&[([0x11; 16], "Setup", 5081)]);
        for cut in [3, 0x30, 0x40, 0x46, b.len() - 1] {
            assert!(parse_list(&b[..cut]).is_err(), "cut at {cut}");
        }
        let mut zero = b.clone();
        zero[0x40..0x44].copy_from_slice(&0u32.to_le_bytes());
        assert!(parse_list(&zero).is_err());
        let mut huge = b.clone();
        huge[0x44..0x48].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_list(&huge).is_err(), "offset past the blob");
        let mut unnamed = b;
        let at = 0x40 + 8 + 16;
        unnamed[at..at + 2].copy_from_slice(&[0, 0]);
        assert!(parse_list(&unnamed).is_err());
    }
}
