//! HII string package parsing (UEFI spec §33.3.6).

use crate::bytes::{slice, u8_at, u16_at, u24_at, u32_at};
use crate::error::{Error, Result};
use std::collections::BTreeMap;

pub const PACKAGE_FORMS: u8 = 0x02;
pub const PACKAGE_STRINGS: u8 = 0x04;

#[derive(Debug, Clone)]
pub struct StringPackage {
    pub offset: usize,
    pub length: usize,
    pub language: String,
    pub strings: BTreeMap<u16, String>,
}

/// Locate string packages inside a module image by validating their headers.
pub fn find_string_packages(pe: &[u8]) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut o = 0;
    while o + 50 < pe.len() {
        if pe[o + 3] == PACKAGE_STRINGS
            && let Some(found) = probe_string_header(pe, o)
        {
            o += found.1;
            out.push(found);
            continue;
        }
        o += 1;
    }
    out
}

fn probe_string_header(pe: &[u8], o: usize) -> Option<(usize, usize, String)> {
    let len = u24_at(pe, o, "").ok()? as usize;
    let hdr = u32_at(pe, o + 4, "").ok()? as usize;
    let info = u32_at(pe, o + 8, "").ok()? as usize;
    if len <= 50 || o + len > pe.len() || hdr != info || !(47..200).contains(&hdr) || hdr >= len {
        return None;
    }
    let lang_bytes = pe.get(o + 46..o + hdr)?;
    let end = lang_bytes.iter().position(|&c| c == 0)?;
    let lang = std::str::from_utf8(&lang_bytes[..end]).ok()?;
    if lang.is_empty()
        || !lang
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b';')
    {
        return None;
    }
    Some((o, len, lang.to_string()))
}

fn utf16z(b: &[u8], at: usize) -> Result<(String, usize)> {
    let mut units = Vec::new();
    let mut p = at;
    loop {
        let u = u16_at(b, p, "string block")?;
        p += 2;
        if u == 0 {
            break;
        }
        units.push(u);
    }
    Ok((String::from_utf16_lossy(&units), p))
}

fn bytez(b: &[u8], at: usize) -> Result<(String, usize)> {
    let rest = b
        .get(at..)
        .ok_or_else(|| Error::malformed("string block", "truncated"))?;
    let end = rest
        .iter()
        .position(|&c| c == 0)
        .ok_or_else(|| Error::malformed("string block", "unterminated"))?;
    Ok((
        String::from_utf8_lossy(&rest[..end]).into_owned(),
        at + end + 1,
    ))
}

pub fn parse_string_package(
    pe: &[u8],
    offset: usize,
    length: usize,
    language: &str,
) -> Result<StringPackage> {
    let pkg = slice(pe, offset, length, "string package")?;
    let hdr = u32_at(pkg, 4, "string package")? as usize;
    let mut p = hdr;
    let mut id: u16 = 1;
    let mut strings = BTreeMap::new();
    let next = |id: u16| {
        id.checked_add(1)
            .ok_or_else(|| Error::malformed("string package", "string id overflow"))
    };
    while p < pkg.len() {
        let t = u8_at(pkg, p, "string block")?;
        match t {
            0x00 => break, // SIBT_END
            0x10 | 0x11 => {
                // SIBT_STRING_SCSU(_FONT)
                let start = if t == 0x11 { p + 2 } else { p + 1 };
                let (s, np) = bytez(pkg, start)?;
                strings.insert(id, s);
                id = next(id)?;
                p = np;
            }
            0x12 | 0x13 => {
                let (count, mut q) = if t == 0x13 {
                    (u16_at(pkg, p + 2, "block")?, p + 4)
                } else {
                    (u16_at(pkg, p + 1, "block")?, p + 3)
                };
                for _ in 0..count {
                    let (s, np) = bytez(pkg, q)?;
                    strings.insert(id, s);
                    id = next(id)?;
                    q = np;
                }
                p = q;
            }
            0x14 | 0x15 => {
                // SIBT_STRING_UCS2(_FONT)
                let start = if t == 0x15 { p + 2 } else { p + 1 };
                let (s, np) = utf16z(pkg, start)?;
                strings.insert(id, s);
                id = next(id)?;
                p = np;
            }
            0x16 | 0x17 => {
                let (count, mut q) = if t == 0x17 {
                    (u16_at(pkg, p + 2, "block")?, p + 4)
                } else {
                    (u16_at(pkg, p + 1, "block")?, p + 3)
                };
                for _ in 0..count {
                    let (s, np) = utf16z(pkg, q)?;
                    strings.insert(id, s);
                    id = next(id)?;
                    q = np;
                }
                p = q;
            }
            0x18 => {
                // SIBT_DUPLICATE
                let src = u16_at(pkg, p + 1, "block")?;
                if let Some(s) = strings.get(&src).cloned() {
                    strings.insert(id, s);
                }
                id = next(id)?;
                p += 3;
            }
            0x20 => {
                id = id
                    .checked_add(u16::from(u8_at(pkg, p + 1, "block")?))
                    .ok_or_else(|| Error::malformed("string package", "skip overflow"))?;
                p += 2;
            }
            0x21 => {
                id = id
                    .checked_add(u16_at(pkg, p + 1, "block")?)
                    .ok_or_else(|| Error::malformed("string package", "skip overflow"))?;
                p += 3;
            }
            0x30 => p += usize::from(u8_at(pkg, p + 2, "block")?).max(1),
            0x31 => p += usize::from(u16_at(pkg, p + 2, "block")?).max(1),
            0x32 => p += (u32_at(pkg, p + 2, "block")? as usize).max(1),
            other => {
                return Err(Error::malformed(
                    "string package",
                    format!("unsupported block type 0x{other:02x} at 0x{:x}", offset + p),
                ));
            }
        }
    }
    Ok(StringPackage {
        offset,
        length,
        language: language.to_string(),
        strings,
    })
}

pub const PACKAGE_END: u8 = 0xDF;

/// Locate HII package lists: a 16-byte GUID, a u32 total length, then packages laid end to end
/// that finish exactly with an END package. Returns the byte range covered by the packages.
/// Used to pair a forms package with the string packages registered alongside it.
pub fn package_lists(pe: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut o = 0;
    while o + 24 <= pe.len() {
        let len = u32::from_le_bytes([pe[o + 16], pe[o + 17], pe[o + 18], pe[o + 19]]) as usize;
        let end = o.saturating_add(len);
        if len >= 24 && end <= pe.len() && pe[end - 4..end] == [4, 0, 0, PACKAGE_END] {
            let mut p = o + 20;
            while p + 4 <= end {
                let pl =
                    usize::from(pe[p]) | usize::from(pe[p + 1]) << 8 | usize::from(pe[p + 2]) << 16;
                if pl < 4 || p + pl > end {
                    break;
                }
                p += pl;
            }
            if p == end {
                out.push((o + 20, end));
                o = end;
                continue;
            }
        }
        o += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_list_needs_exact_chain_ending_in_end_package() {
        let pkg = |len: u32, ty: u8| {
            let mut v = vec![(len & 0xFF) as u8, (len >> 8) as u8, (len >> 16) as u8, ty];
            v.resize(len as usize, 0xAA);
            v
        };
        let body = [
            pkg(12, PACKAGE_FORMS),
            pkg(8, PACKAGE_STRINGS),
            pkg(4, PACKAGE_END),
        ]
        .concat();
        let mut list = vec![0x11u8; 16];
        list.extend_from_slice(&(20 + body.len() as u32).to_le_bytes());
        list.extend_from_slice(&body);
        let pe = [vec![0u8; 5], list.clone(), vec![0u8; 7]].concat();
        assert_eq!(package_lists(&pe), vec![(25, 25 + body.len())]);
        // A broken chain (wrong inner length) is not a package list.
        let mut bad = list;
        bad[20] = 13;
        assert!(package_lists(&[vec![0u8; 5], bad].concat()).is_empty());
    }
}
