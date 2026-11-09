//! AMI NVAR entries: the `StdDefaults` store embedded in a BIOS image.
//!
//! `StdDefaults` is the AMI optimized-defaults variable set (what "Load Optimized Defaults"
//! restores). It is an NVAR entry whose data is itself a sequence of NVAR entries, one per
//! variable (`Setup`, `CpuSetup`, ...). It is used for display only: comparing a profile with
//! defaults never influences what is written.
//!
//! Nested entries name their variable GUID by an index into a table of 16-byte GUIDs stored
//! at the end of the parent entry (index 0 is the last 16 bytes). A variable is only accepted
//! when its name, GUID and size all match the IFR varstore.
//!
//! Extraction is deliberately strict. Any copy that cannot be read exactly (linked or data-only
//! entries, duplicate names, wrong sizes) or copies that disagree make the whole result
//! unavailable rather than guessed.

use crate::bytes::guid_string;
use std::collections::BTreeMap;

const SIG: &[u8; 4] = b"NVAR";
const HEADER: usize = 10;
const ATTR_ASCII_NAME: u8 = 0x02;
const ATTR_GUID: u8 = 0x04;
const ATTR_DATA_ONLY: u8 = 0x08;
const ATTR_VALID: u8 = 0x80;
const NO_NEXT: u32 = 0x00FF_FFFF;
const STD_DEFAULTS: &str = "StdDefaults";

#[derive(Debug, Clone)]
pub struct StdDefaults {
    /// Variable name -> default bytes, for the requested stores only.
    pub stores: BTreeMap<String, Vec<u8>>,
    /// Image offsets of the byte-identical copies that were found.
    pub copies: Vec<usize>,
}

struct Entry<'a> {
    name: String,
    /// Inline GUID, or the index into the parent's GUID table.
    guid: GuidRef,
    data: &'a [u8],
    size: usize,
}

enum GuidRef {
    Inline(String),
    Index(u8),
}

/// Parse one self-contained, named NVAR entry at `o`. Rejects linked or data-only entries.
fn entry(buf: &[u8], o: usize) -> Result<Entry<'_>, String> {
    let h = buf
        .get(o..o + HEADER)
        .ok_or_else(|| format!("truncated NVAR header at 0x{o:x}"))?;
    if &h[..4] != SIG {
        return Err(format!("no NVAR signature at 0x{o:x}"));
    }
    let size = usize::from(u16::from_le_bytes([h[4], h[5]]));
    let next = u32::from_le_bytes([h[6], h[7], h[8], 0]);
    let attr = h[9];
    if size <= HEADER || o + size > buf.len() {
        return Err(format!("bad NVAR size {size} at 0x{o:x}"));
    }
    if attr & ATTR_VALID == 0 || attr & ATTR_DATA_ONLY != 0 || next != NO_NEXT {
        return Err(format!(
            "NVAR entry at 0x{o:x} is invalid, data-only or linked (attr 0x{attr:02x}); not supported"
        ));
    }
    if attr & ATTR_ASCII_NAME == 0 {
        return Err(format!(
            "NVAR entry at 0x{o:x} has a UCS-2 name; not supported"
        ));
    }
    let body = &buf[o + HEADER..o + size];
    let (guid, name_at) = if attr & ATTR_GUID != 0 {
        let g = body
            .get(..16)
            .ok_or_else(|| format!("truncated NVAR GUID at 0x{o:x}"))?;
        (GuidRef::Inline(guid_string(g)), 16)
    } else {
        (GuidRef::Index(body[0]), 1)
    };
    let rest = body
        .get(name_at..)
        .ok_or_else(|| format!("truncated NVAR name at 0x{o:x}"))?;
    let end = rest
        .iter()
        .position(|&c| c == 0)
        .ok_or_else(|| format!("unterminated NVAR name at 0x{o:x}"))?;
    let name = String::from_utf8_lossy(&rest[..end]).into_owned();
    Ok(Entry {
        name,
        guid,
        data: &rest[end + 1..],
        size,
    })
}

/// Read the nested variables of one StdDefaults entry, keyed by (name, GUID).
///
/// Layout: NVAR entries, then erased (0xFF) space, then a GUID table that ends the entry.
/// Nested variables keyed by (name, GUID); a key with several entries is ambiguous.
type Vars<'a> = BTreeMap<(String, String), Vec<&'a [u8]>>;

fn nested(data: &[u8]) -> Result<Vars<'_>, String> {
    let mut entries = Vec::new();
    let mut o = 0;
    while data.get(o..o + 4) == Some(SIG.as_slice()) {
        let e = entry(data, o)?;
        o += e.size;
        entries.push(e);
    }
    let tail = &data[o..];
    let table = &tail[tail.iter().take_while(|&&b| b == 0xFF).count()..];
    if !table.len().is_multiple_of(16) {
        return Err(format!(
            "{} bytes after the entries are not a GUID table",
            table.len()
        ));
    }
    let slots = table.len() / 16;
    let mut out: Vars = BTreeMap::new();
    for e in entries {
        let guid = match e.guid {
            GuidRef::Inline(g) => g,
            GuidRef::Index(i) if usize::from(i) < slots => {
                let end = table.len() - 16 * usize::from(i);
                guid_string(&table[end - 16..end])
            }
            GuidRef::Index(i) => {
                return Err(format!("{} uses GUID index {i}; table has {slots}", e.name));
            }
        };
        out.entry((e.name, guid)).or_default().push(e.data);
    }
    Ok(out)
}

/// Every StdDefaults copy in an image: its offset and its nested variables, keyed by
/// (name, GUID). A key with several entries is kept as several values (ambiguous).
/// Nested variables of one copy: (name, GUID) -> every entry with that key.
pub type CopyVars = BTreeMap<(String, String), Vec<Vec<u8>>>;

#[derive(Debug, Clone, Default)]
pub struct DefaultsCopies {
    /// (image offset, variables) per copy.
    pub copies: Vec<(usize, CopyVars)>,
}

impl DefaultsCopies {
    /// Default bytes of one variable, accepted only when every copy holds it exactly once, with
    /// the expected size, and all copies agree.
    pub fn pick(&self, name: &str, guid: &str, size: usize) -> Result<Vec<u8>, String> {
        let key = (name.to_string(), guid.to_string());
        let mut found: Option<&Vec<u8>> = None;
        for (o, vars) in &self.copies {
            let d = match vars.get(&key).map(Vec::as_slice) {
                Some([d]) if d.len() == size => d,
                Some([d]) => {
                    return Err(format!(
                        "StdDefaults at 0x{o:x}: {name} is {} bytes, expected {size}",
                        d.len()
                    ));
                }
                Some(list) if list.len() > 1 => {
                    return Err(format!(
                        "StdDefaults at 0x{o:x}: {name} appears {} times",
                        list.len()
                    ));
                }
                _ => {
                    return Err(format!(
                        "StdDefaults at 0x{o:x}: {name} {{{guid}}} is missing"
                    ));
                }
            };
            match found {
                Some(f) if f != d => {
                    return Err(format!(
                        "{} StdDefaults copies disagree on {name}; defaults not used",
                        self.copies.len()
                    ));
                }
                _ => found = Some(d),
            }
        }
        found
            .cloned()
            .ok_or_else(|| "this BIOS image has no StdDefaults store".to_string())
    }

    pub fn offsets(&self) -> Vec<usize> {
        self.copies.iter().map(|(o, _)| *o).collect()
    }
}

/// Read every StdDefaults copy in the image. `Ok(None)` when there is none; `Err` when one exists
/// but its nested entries cannot be read exactly.
pub fn read_std_defaults(image: &[u8]) -> Result<Option<DefaultsCopies>, String> {
    let mut out = DefaultsCopies::default();
    let mut o = 0;
    while o + HEADER <= image.len() {
        if &image[o..o + 4] != SIG {
            o += 1;
            continue;
        }
        let Ok(e) = entry(image, o) else {
            o += 1;
            continue;
        };
        if e.name != STD_DEFAULTS {
            o += 1;
            continue;
        }
        let vars = nested(e.data).map_err(|m| format!("StdDefaults at 0x{o:x}: {m}"))?;
        let owned = vars
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().map(<[u8]>::to_vec).collect()))
            .collect();
        out.copies.push((o, owned));
        o += e.size;
    }
    Ok((!out.copies.is_empty()).then_some(out))
}

/// Find StdDefaults copies and extract the `wanted` (name, GUID, exact size) variables.
///
/// Returns `Ok(None)` when the image has no StdDefaults entry, and `Err` when one exists but
/// any wanted variable cannot be read reliably from every copy.
pub fn find_std_defaults(
    image: &[u8],
    wanted: &[(&str, &str, usize)],
) -> Result<Option<StdDefaults>, String> {
    let Some(copies) = read_std_defaults(image)? else {
        return Ok(None);
    };
    std_defaults_from(&copies, wanted).map(Some)
}

/// Strict selection over already-read copies: every wanted variable must be readable.
pub fn std_defaults_from(
    copies: &DefaultsCopies,
    wanted: &[(&str, &str, usize)],
) -> Result<StdDefaults, String> {
    let mut stores = BTreeMap::new();
    for (name, guid, size) in wanted {
        stores.insert((*name).to_string(), copies.pick(name, guid, *size)?);
    }
    Ok(StdDefaults {
        stores,
        copies: copies.offsets(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: [u8; 16] = [7; 16];

    fn nvar(name: &str, data: &[u8]) -> Vec<u8> {
        let size = HEADER + 1 + name.len() + 1 + data.len();
        let mut v = b"NVAR".to_vec();
        v.extend_from_slice(&(size as u16).to_le_bytes());
        v.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0x83, 0x00]);
        v.extend_from_slice(name.as_bytes());
        v.push(0);
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn reads_identical_copies() {
        let g = guid_string(&G);
        let inner = [
            nvar("Setup", &[1, 2, 3]),
            nvar("Other", &[9]),
            vec![0xFF; 4],
            G.to_vec(),
        ]
        .concat();
        let block = nvar("StdDefaults", &inner);
        let image = [vec![0u8; 7], block.clone(), vec![0xFF; 5], block].concat();
        let d = find_std_defaults(&image, &[("Setup", &g, 3)])
            .unwrap()
            .unwrap();
        assert_eq!(d.copies.len(), 2);
        assert_eq!(d.stores["Setup"], vec![1, 2, 3]);
    }

    #[test]
    fn rejects_disagreement_and_wrong_size() {
        let g = guid_string(&G);
        let w = |size| [("Setup", g.as_str(), size)];
        let a = nvar(
            "StdDefaults",
            &[nvar("Setup", &[1, 2, 3]), G.to_vec()].concat(),
        );
        let b = nvar(
            "StdDefaults",
            &[nvar("Setup", &[1, 2, 4]), G.to_vec()].concat(),
        );
        assert!(find_std_defaults(&[a.clone(), b].concat(), &w(3)).is_err());
        assert!(find_std_defaults(&a, &w(4)).is_err());
        // Same name under another GUID is not the Setup varstore.
        assert!(
            find_std_defaults(&a, &[("Setup", "00000000-0000-0000-0000-000000000000", 3)]).is_err()
        );
        assert!(find_std_defaults(&[0u8; 64], &w(3)).unwrap().is_none());
    }
}
