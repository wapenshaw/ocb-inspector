//! Checked little-endian reads over immutable buffers.

use crate::error::{Error, Result};
use sha2::{Digest, Sha256};

/// Lower-case hexadecimal text of `data`, two digits per byte, no separator.
pub fn hex(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

pub fn slice<'a>(b: &'a [u8], at: usize, len: usize, what: &'static str) -> Result<&'a [u8]> {
    let end = at
        .checked_add(len)
        .ok_or_else(|| Error::malformed(what, "offset overflow"))?;
    b.get(at..end)
        .ok_or_else(|| Error::malformed(what, format!("truncated at 0x{at:x}+{len}")))
}

pub fn u8_at(b: &[u8], at: usize, what: &'static str) -> Result<u8> {
    Ok(slice(b, at, 1, what)?[0])
}

pub fn u16_at(b: &[u8], at: usize, what: &'static str) -> Result<u16> {
    let s = slice(b, at, 2, what)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

pub fn u24_at(b: &[u8], at: usize, what: &'static str) -> Result<u32> {
    let s = slice(b, at, 3, what)?;
    Ok(u32::from(s[0]) | u32::from(s[1]) << 8 | u32::from(s[2]) << 16)
}

pub fn u32_at(b: &[u8], at: usize, what: &'static str) -> Result<u32> {
    let s = slice(b, at, 4, what)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn u64_at(b: &[u8], at: usize, what: &'static str) -> Result<u64> {
    let s = slice(b, at, 8, what)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Ok(u64::from_le_bytes(a))
}

/// Little-endian unsigned integer of 1, 2, 4 or 8 bytes.
pub fn uint_at(b: &[u8], at: usize, width: usize, what: &'static str) -> Result<u64> {
    match width {
        1 => u8_at(b, at, what).map(u64::from),
        2 => u16_at(b, at, what).map(u64::from),
        4 => u32_at(b, at, what).map(u64::from),
        8 => u64_at(b, at, what),
        _ => Err(Error::malformed(what, format!("unsupported width {width}"))),
    }
}

/// Format a GUID stored in EFI mixed-endian layout.
pub fn guid_string(g: &[u8]) -> String {
    if g.len() != 16 {
        return String::from("invalid-guid");
    }
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        u32::from_le_bytes([g[0], g[1], g[2], g[3]]),
        u16::from_le_bytes([g[4], g[5]]),
        u16::from_le_bytes([g[6], g[7]]),
        g[8],
        g[9],
        g[10],
        g[11],
        g[12],
        g[13],
        g[14],
        g[15]
    )
}

pub fn align_up(v: usize, a: usize) -> Option<usize> {
    v.checked_add(a - 1).map(|x| x & !(a - 1))
}
