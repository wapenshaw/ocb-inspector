//! Lossless OCB parsing and serialization (SPEC §5, OCB pipeline).
//!
//! Layout established for the E7D91IMS HI0/HJ0 adapter (observations, not universal constants):
//!
//! ```text
//! 0x00  "$MOS$"
//! 0x05  BIOS id, 12 ASCII bytes (e.g. "E7D91IMS.HJ0")
//! 0x11  i32 LE checksum = -(sum of every other byte in the file)
//! 0x15  u32 LE record count
//! 0x19  records: u32 id, u16 size, payload
//!       256-byte RTC/CMOS-like block
//!       "$OCI$" metadata block (board, revision, CPU brand, timestamp, opaque u32s)
//! ```

use crate::bytes::{u16_at, u32_at};
use crate::error::{Error, Result};
use serde::Serialize;

pub const MAGIC: &[u8] = b"$MOS$";
pub const OCI_MAGIC: &[u8] = b"$OCI$";
pub const CHECKSUM_AT: usize = 0x11;
pub const COUNT_AT: usize = 0x15;
pub const RECORDS_AT: usize = 0x19;
pub const CMOS_LEN: usize = 256;
pub const MAX_PROFILE_BYTES: usize = 16 << 20;
/// Opaque bytes after the four $OCI$ strings (six u32 values in every fixture).
pub const OCI_TAIL_LEN: usize = 24;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Record {
    pub id: u32,
    /// File offset of the payload.
    pub offset: usize,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OciMetadata {
    pub offset: usize,
    pub board: String,
    pub revision: String,
    pub cpu_brand: String,
    /// YYYYMMDDHHMMSS, no timezone.
    pub saved_at: String,
}

/// A parsed profile. Owns its bytes; spans index into them. Serialization is the identity.
#[derive(Debug, Clone)]
pub struct OcbFile {
    bytes: Vec<u8>,
    pub bios_id: String,
    pub stored_checksum: i32,
    pub records: Vec<Record>,
    pub records_end: usize,
    pub cmos_offset: usize,
    pub oci: OciMetadata,
}

impl OcbFile {
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(Error::Limit(format!(
                "profile exceeds {MAX_PROFILE_BYTES} bytes"
            )));
        }
        if bytes.len() < RECORDS_AT || &bytes[..5] != MAGIC {
            return Err(Error::malformed("OCB", "missing $MOS$ header"));
        }
        let id_bytes = &bytes[5..CHECKSUM_AT];
        if !id_bytes.iter().all(|b| b.is_ascii_graphic()) {
            return Err(Error::malformed("OCB", "BIOS id is not printable ASCII"));
        }
        let bios_id = String::from_utf8_lossy(id_bytes).into_owned();
        let stored_checksum = u32_at(&bytes, CHECKSUM_AT, "OCB checksum")? as i32;
        let count = u32_at(&bytes, COUNT_AT, "OCB record count")? as usize;
        if count == 0 || count > 100_000 {
            return Err(Error::malformed(
                "OCB",
                format!("implausible record count {count}"),
            ));
        }
        let mut records = Vec::with_capacity(count);
        let mut pos = RECORDS_AT;
        for i in 0..count {
            let id = u32_at(&bytes, pos, "OCB record header")?;
            let size = usize::from(u16_at(&bytes, pos + 4, "OCB record header")?);
            let offset = pos + 6;
            let end = offset
                .checked_add(size)
                .filter(|&e| e <= bytes.len())
                .ok_or_else(|| {
                    Error::malformed("OCB", format!("record {i} (id {id}) truncated"))
                })?;
            records.push(Record { id, offset, size });
            pos = end;
        }
        // Adapter invariant for this layout: strictly increasing record ids.
        if records.windows(2).any(|w| w[0].id >= w[1].id) {
            return Err(Error::malformed(
                "OCB",
                "record ids are not strictly increasing",
            ));
        }
        let records_end = pos;
        let cmos_offset = records_end;
        let oci_at = records_end + CMOS_LEN;
        if bytes.get(oci_at..oci_at + OCI_MAGIC.len()) != Some(OCI_MAGIC) {
            return Err(Error::malformed(
                "OCB",
                format!("$OCI$ not at record end + 256 (0x{oci_at:x})"),
            ));
        }
        let (oci, strings_end) = parse_oci(&bytes, oci_at)?;
        if bytes.len() != strings_end + OCI_TAIL_LEN {
            return Err(Error::Unsupported(format!(
                "$OCI$ tail is {} bytes; this adapter's layout has {OCI_TAIL_LEN}",
                bytes.len().saturating_sub(strings_end)
            )));
        }
        let f = OcbFile {
            bytes,
            bios_id,
            stored_checksum,
            records,
            records_end,
            cmos_offset,
            oci,
        };
        Ok(f)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Checksum the adapter's additive rule predicts for these bytes.
    pub fn computed_checksum(&self) -> Result<i32> {
        compute_checksum(&self.bytes)
    }

    pub fn checksum_valid(&self) -> bool {
        self.computed_checksum()
            .map(|c| c == self.stored_checksum)
            .unwrap_or(false)
    }

    pub fn record_by_size(&self, size: usize) -> Vec<&Record> {
        self.records.iter().filter(|r| r.size == size).collect()
    }
}

/// `-(sum of bytes[0..0x11] + bytes[0x15..])`, computed in i64 and required to fit i32.
pub fn compute_checksum(bytes: &[u8]) -> Result<i32> {
    if bytes.len() < COUNT_AT {
        return Err(Error::malformed("OCB", "too short for checksum"));
    }
    let sum: i64 = bytes[..CHECKSUM_AT]
        .iter()
        .chain(&bytes[COUNT_AT..])
        .map(|&b| i64::from(b))
        .sum();
    i32::try_from(-sum).map_err(|_| Error::Unsupported("checksum not representable as i32".into()))
}

fn parse_oci(b: &[u8], at: usize) -> Result<(OciMetadata, usize)> {
    // "$OCI$\0" board "\0" revision "\0" cpu "\0" timestamp "\0" ...
    let mut fields = Vec::new();
    let mut p = at + OCI_MAGIC.len() + 1;
    for _ in 0..4 {
        let rest = b
            .get(p..)
            .ok_or_else(|| Error::malformed("$OCI$", "truncated"))?;
        let end = rest
            .iter()
            .take(128)
            .position(|&c| c == 0)
            .ok_or_else(|| Error::malformed("$OCI$", "unterminated field"))?;
        fields.push(String::from_utf8_lossy(&rest[..end]).into_owned());
        p += end + 1;
    }
    let saved_at = fields[3].clone();
    if saved_at.len() != 14 || !saved_at.bytes().all(|c| c.is_ascii_digit()) {
        return Err(Error::malformed("$OCI$", "timestamp is not YYYYMMDDHHMMSS"));
    }
    Ok((
        OciMetadata {
            offset: at,
            board: fields[0].clone(),
            revision: fields[1].clone(),
            cpu_brand: fields[2].clone(),
            saved_at,
        },
        p,
    ))
}

/// Byte-level mutation of a parsed baseline. Only the engine creates these.
pub(crate) struct Mutation {
    bytes: Vec<u8>,
}

impl Mutation {
    pub(crate) fn from_baseline(f: &OcbFile) -> Self {
        Mutation {
            bytes: f.bytes.clone(),
        }
    }

    pub(crate) fn write_uint(&mut self, offset: usize, width: usize, value: u64) -> Result<()> {
        let le = value.to_le_bytes();
        if width == 0 || width > 8 || (width < 8 && value >> (width * 8) != 0) {
            return Err(Error::Validation(format!(
                "value {value} does not fit {width} bytes"
            )));
        }
        let slot = self
            .bytes
            .get_mut(offset..offset + width)
            .ok_or_else(|| Error::Internal(format!("write outside profile at 0x{offset:x}")))?;
        slot.copy_from_slice(&le[..width]);
        Ok(())
    }

    /// Recompute the checksum and reparse; the result must be a structurally valid profile.
    pub(crate) fn finish(mut self) -> Result<OcbFile> {
        let c = compute_checksum(&self.bytes)?;
        self.bytes[CHECKSUM_AT..COUNT_AT].copy_from_slice(&c.to_le_bytes());
        OcbFile::parse(self.bytes)
    }
}

/// Offsets where two byte strings differ.
pub fn diff_offsets(a: &[u8], b: &[u8]) -> Vec<usize> {
    let mut out: Vec<usize> = a
        .iter()
        .zip(b)
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .map(|(i, _)| i)
        .collect();
    let (short, long) = (a.len().min(b.len()), a.len().max(b.len()));
    out.extend(short..long);
    out
}

/// Collapse sorted offsets into half-open spans.
pub fn spans(offsets: &[usize]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for &o in offsets {
        match out.last_mut() {
            Some(last) if last.1 == o => last.1 = o + 1,
            _ => out.push((o, o + 1)),
        }
    }
    out
}
