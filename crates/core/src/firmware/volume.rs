//! Bounded walk over UEFI firmware volumes, FFS files and sections (UEFI PI spec, vol. 3).
//!
//! Only what the setup-module search needs is implemented: FV headers, FFS v2/v3 files,
//! leaf/encapsulation sections, EFI/Tiano compression sections (`tiano.rs`), LZMA GUID-defined
//! sections and nested FV images.

use crate::bytes::{align_up, guid_string, slice, u8_at, u16_at, u24_at, u32_at, u64_at};
use crate::error::{Error, Result};
use std::io::Cursor;

/// Parser resource ceilings (SPEC §5).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_bios_bytes: usize,
    pub max_total_decompressed: usize,
    pub max_depth: usize,
    pub max_ifr_ops: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_bios_bytes: 128 << 20,
            max_total_decompressed: 512 << 20,
            max_depth: 32,
            max_ifr_ops: 250_000,
        }
    }
}

const LZMA_GUID: &str = "EE4E5898-3914-4259-9D6E-DC7BD79403CF";
const SECTION_COMPRESSION: u8 = 0x01;
const SECTION_GUID_DEFINED: u8 = 0x02;
const SECTION_PE32: u8 = 0x10;
const SECTION_USER_INTERFACE: u8 = 0x15;
const SECTION_FV_IMAGE: u8 = 0x17;
const FFS_TYPE_PAD: u8 = 0xF0;

/// Where a byte range lives: the original image or a decompressed buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufId {
    Image,
    Arena(usize),
}

#[derive(Debug, Clone)]
pub struct Pe32Ref {
    pub buf: BufId,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct FfsFile {
    pub guid: String,
    pub name: Option<String>,
    pub pe32: Vec<Pe32Ref>,
    /// Path of containers, for provenance.
    pub path: String,
}

pub struct Walk<'a> {
    pub image: &'a [u8],
    pub arena: Vec<Vec<u8>>,
    pub files: Vec<FfsFile>,
    pub diagnostics: Vec<String>,
    limits: Limits,
    decompressed: usize,
}

impl<'a> Walk<'a> {
    pub fn buf(&self, id: BufId) -> &[u8] {
        match id {
            BufId::Image => self.image,
            BufId::Arena(i) => &self.arena[i],
        }
    }

    pub fn bytes_of(&self, r: &Pe32Ref) -> &[u8] {
        &self.buf(r.buf)[r.start..r.end]
    }
}

/// Walk every top-level firmware volume found in the image.
pub fn walk_image(image: &[u8], limits: Limits) -> Result<Walk<'_>> {
    if image.len() > limits.max_bios_bytes {
        return Err(Error::Limit(format!(
            "BIOS image is {} bytes; limit is {}",
            image.len(),
            limits.max_bios_bytes
        )));
    }
    let mut w = Walk {
        image,
        arena: Vec::new(),
        files: Vec::new(),
        diagnostics: Vec::new(),
        limits,
        decompressed: 0,
    };
    let mut covered_until = 0usize;
    let mut i = 0usize;
    while let Some(pos) = find(image, b"_FVH", i) {
        i = pos + 4;
        let Some(start) = pos.checked_sub(40) else {
            continue;
        };
        if start < covered_until {
            continue; // nested inside a volume already walked
        }
        let Some(len) = fv_length(image, start) else {
            continue;
        };
        walk_fv(
            &mut w,
            BufId::Image,
            start,
            start + len,
            &format!("fv@0x{start:x}"),
            0,
        )?;
        covered_until = start + len;
    }
    if w.files.is_empty() {
        return Err(Error::malformed("BIOS image", "no firmware volumes found"));
    }
    Ok(w)
}

fn find(h: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= h.len() {
        return None;
    }
    h[from..]
        .windows(needle.len())
        .position(|x| x == needle)
        .map(|p| p + from)
}

fn fv_length(b: &[u8], start: usize) -> Option<usize> {
    if b.get(start + 40..start + 44)? != b"_FVH" {
        return None;
    }
    let len = usize::try_from(u64_at(b, start + 32, "fv").ok()?).ok()?;
    let hlen = usize::from(u16_at(b, start + 48, "fv").ok()?);
    if hlen < 56 || len <= hlen || start.checked_add(len)? > b.len() {
        return None;
    }
    Some(len)
}

fn walk_fv(
    w: &mut Walk,
    buf: BufId,
    start: usize,
    end: usize,
    path: &str,
    depth: usize,
) -> Result<()> {
    if depth > w.limits.max_depth {
        return Err(Error::Limit(format!(
            "nesting depth exceeds {}",
            w.limits.max_depth
        )));
    }
    let (hlen, ext) = {
        let b = w.buf(buf);
        (
            usize::from(u16_at(b, start + 48, "fv header")?),
            usize::from(u16_at(b, start + 52, "fv header")?),
        )
    };
    let mut o = start + hlen;
    if ext != 0 {
        let b = w.buf(buf);
        let ext_size = u32_at(b, start + ext + 16, "fv extended header")? as usize;
        o = start + ext + ext_size;
    }
    o = align_up(o, 8).ok_or_else(|| Error::malformed("fv", "overflow"))?;
    while o + 24 <= end {
        // Free space: the whole header is erased. A PAD file also has an all-0xFF name GUID,
        // so the name alone is not enough (MSI AMD images use PAD files mid-volume).
        if w.buf(buf)[o..o + 24].iter().all(|&x| x == 0xFF) {
            break;
        }
        let (guid, ftype, size, hdr) = {
            let b = w.buf(buf);
            let name = slice(b, o, 16, "ffs header")?;
            let ftype = u8_at(b, o + 18, "ffs header")?;
            let attrs = u8_at(b, o + 19, "ffs header")?;
            let mut size = u24_at(b, o + 20, "ffs header")? as usize;
            let mut hdr = 24;
            // FFS3 large file: attribute bit 0 with a zero 24-bit size.
            if attrs & 0x01 != 0 && size == 0 {
                size = usize::try_from(u64_at(b, o + 24, "ffs3 header")?)
                    .map_err(|_| Error::malformed("ffs", "size overflow"))?;
                hdr = 32;
            }
            (guid_string(name), ftype, size, hdr)
        };
        // The size is untrusted (FFS3 sizes are 64-bit): an end past the volume, or one that
        // overflows, stops the walk.
        let file_end = match o.checked_add(size) {
            Some(e) if size >= hdr && e <= end => e,
            _ => {
                w.diagnostics
                    .push(format!("{path}: stopped at invalid file size at 0x{o:x}"));
                break;
            }
        };
        if ftype != FFS_TYPE_PAD {
            let idx = w.files.len();
            w.files.push(FfsFile {
                guid: guid.clone(),
                name: None,
                pe32: Vec::new(),
                path: path.to_string(),
            });
            walk_sections(
                w,
                buf,
                o + hdr,
                file_end,
                idx,
                &format!("{path}/{guid}"),
                depth + 1,
            )?;
        }
        o = align_up(file_end, 8).ok_or_else(|| Error::malformed("fv", "overflow"))?;
    }
    Ok(())
}

fn walk_sections(
    w: &mut Walk,
    buf: BufId,
    start: usize,
    end: usize,
    file: usize,
    path: &str,
    depth: usize,
) -> Result<()> {
    if depth > w.limits.max_depth {
        return Err(Error::Limit(format!(
            "nesting depth exceeds {}",
            w.limits.max_depth
        )));
    }
    let mut o = start;
    while o + 4 <= end {
        let (stype, size, hdr) = {
            let b = w.buf(buf);
            let mut size = u24_at(b, o, "section header")? as usize;
            let stype = u8_at(b, o + 3, "section header")?;
            let mut hdr = 4;
            if size == 0xFF_FFFF {
                size = u32_at(b, o + 4, "section header")? as usize;
                hdr = 8;
            }
            (stype, size, hdr)
        };
        let Some(section_end) = o.checked_add(size).filter(|&e| size >= hdr && e <= end) else {
            break;
        };
        let body = (o + hdr, section_end);
        match stype {
            SECTION_PE32 => w.files[file].pe32.push(Pe32Ref {
                buf,
                start: body.0,
                end: body.1,
            }),
            SECTION_USER_INTERFACE => {
                let b = w.buf(buf);
                let units: Vec<u16> = b[body.0..body.1]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .take_while(|&c| c != 0)
                    .collect();
                w.files[file].name = Some(String::from_utf16_lossy(&units));
            }
            SECTION_FV_IMAGE => {
                if let Some(len) = fv_length(w.buf(buf), body.0)
                    && body.0 + len <= body.1
                {
                    walk_fv(
                        w,
                        buf,
                        body.0,
                        body.0 + len,
                        &format!("{path}/fv"),
                        depth + 1,
                    )?;
                }
            }
            SECTION_GUID_DEFINED => {
                let (guid, data_off) = {
                    let b = w.buf(buf);
                    (
                        guid_string(slice(b, body.0, 16, "guid section")?),
                        usize::from(u16_at(b, body.0 + 16, "guid section")?),
                    )
                };
                let data_start = o + data_off;
                if data_start > body.1 {
                    return Err(Error::malformed(
                        "guid section",
                        format!("data offset beyond section at 0x{o:x}"),
                    ));
                }
                if guid == LZMA_GUID {
                    let out = decompress_lzma(w, buf, data_start, body.1)?;
                    let id = w.arena.len();
                    let len = out.len();
                    w.arena.push(out);
                    walk_sections(
                        w,
                        BufId::Arena(id),
                        0,
                        len,
                        file,
                        &format!("{path}/lzma"),
                        depth + 1,
                    )?;
                } else {
                    // Non-compressing GUID sections (e.g. signed/CRC wrappers) contain plain sections.
                    walk_sections(
                        w,
                        buf,
                        data_start,
                        body.1,
                        file,
                        &format!("{path}/guid:{guid}"),
                        depth + 1,
                    )?;
                }
            }
            SECTION_COMPRESSION => {
                let b = w.buf(buf);
                let ctype = u8_at(b, body.0 + 4, "compression section")?;
                if ctype == 0 {
                    walk_sections(w, buf, body.0 + 5, body.1, file, path, depth + 1)?;
                } else if ctype == 1 {
                    match decompress_efi(w, buf, body.0, body.1) {
                        Ok(out) => {
                            let id = w.arena.len();
                            let len = out.len();
                            w.arena.push(out);
                            walk_sections(
                                w,
                                BufId::Arena(id),
                                0,
                                len,
                                file,
                                &format!("{path}/efi"),
                                depth + 1,
                            )?;
                        }
                        Err(Error::Limit(m)) => return Err(Error::Limit(m)),
                        Err(e) => w.diagnostics.push(format!(
                            "{path}: EFI/Tiano compression section does not decode ({e}); skipped"
                        )),
                    }
                } else {
                    w.diagnostics.push(format!(
                        "{path}: compression type {ctype} not supported; skipped"
                    ));
                }
            }
            _ => {}
        }
        o = align_up(section_end, 4).ok_or_else(|| Error::malformed("section", "overflow"))?;
    }
    Ok(())
}

/// Standard compression (type 1): EFI 1.1 tried first, then Tiano, as both share the type byte.
fn decompress_efi(w: &mut Walk, buf: BufId, start: usize, end: usize) -> Result<Vec<u8>> {
    let remaining = w
        .limits
        .max_total_decompressed
        .saturating_sub(w.decompressed);
    let b = w.buf(buf);
    let declared = u32_at(b, start, "compression section")? as usize;
    if declared > remaining {
        return Err(Error::Limit(format!(
            "cumulative decompression would exceed {} bytes",
            w.limits.max_total_decompressed
        )));
    }
    let stream = b
        .get(start + 5..end)
        .ok_or_else(|| Error::malformed("compression section", "truncated header"))?;
    let out = super::tiano::decompress(stream, remaining)
        .map_err(|e| Error::malformed("compression section", e))?;
    if out.len() != declared {
        return Err(Error::malformed(
            "compression section",
            "decompressed size differs from header",
        ));
    }
    w.decompressed += out.len();
    Ok(out)
}

fn decompress_lzma(w: &mut Walk, buf: BufId, start: usize, end: usize) -> Result<Vec<u8>> {
    let remaining = w
        .limits
        .max_total_decompressed
        .saturating_sub(w.decompressed);
    let data = &w.buf(buf)[start..end];
    // EFI LZMA sections use the .lzma "alone" header: props(5) + unpacked size(8).
    let declared = u64_at(data, 5, "lzma header")?;
    if declared == u64::MAX || declared > remaining as u64 {
        return Err(Error::Limit(format!(
            "cumulative decompression would exceed {} bytes",
            w.limits.max_total_decompressed
        )));
    }
    let mut out = Vec::with_capacity(declared as usize);
    let opts = lzma_rs::decompress::Options {
        unpacked_size: lzma_rs::decompress::UnpackedSize::ReadFromHeader,
        memlimit: Some(remaining),
        allow_incomplete: false,
    };
    lzma_rs::lzma_decompress_with_options(&mut Cursor::new(data), &mut out, &opts)
        .map_err(|e| Error::malformed("lzma section", e.to_string()))?;
    if out.len() as u64 != declared {
        return Err(Error::malformed(
            "lzma section",
            "decompressed size differs from header",
        ));
    }
    w.decompressed += out.len();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: [u8; 16], ftype: u8, body: &[u8]) -> Vec<u8> {
        let size = 24 + body.len();
        let mut h = name.to_vec();
        h.extend_from_slice(&[0, 0, ftype, 0]);
        h.extend_from_slice(&(size as u32).to_le_bytes()[..3]);
        h.push(0xF8);
        h.extend_from_slice(body);
        while !h.len().is_multiple_of(8) {
            h.push(0xFF);
        }
        h
    }

    fn volume(files: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = files.concat();
        let len = 72 + body.len() + 24; // trailing erased space
        let mut v = vec![0u8; 72];
        v[32..40].copy_from_slice(&(len as u64).to_le_bytes());
        v[40..44].copy_from_slice(b"_FVH");
        v[48..50].copy_from_slice(&72u16.to_le_bytes());
        v.extend_from_slice(&body);
        v.extend_from_slice(&[0xFF; 24]);
        v
    }

    #[test]
    fn pad_file_with_erased_name_is_not_free_space() {
        // A PAD file's name GUID is all 0xFF; files after it must still be walked.
        let pad = file([0xFF; 16], FFS_TYPE_PAD, &[0xFF; 8]);
        let raw = file([0x11; 16], 0x01, &[]);
        let image = [vec![0u8; 16], volume(&[pad, raw])].concat();
        let w = walk_image(&image, Limits::default()).unwrap();
        assert_eq!(w.files.len(), 1);
        assert_eq!(w.files[0].guid, guid_string(&[0x11; 16]));
    }

    #[test]
    fn an_ffs3_size_that_overflows_stops_the_walk() {
        // FFS3 large file (attribute bit 0, zero 24-bit size) claiming u64::MAX bytes: adding it
        // to the offset overflows, which must end the walk rather than wrap past the bounds check.
        let raw = file([0x11; 16], 0x01, &[]);
        let mut huge = [0x22u8; 16].to_vec();
        huge.extend_from_slice(&[0, 0, 0x01, 0x01, 0, 0, 0, 0xF8]);
        huge.extend_from_slice(&u64::MAX.to_le_bytes());
        let image = [vec![0u8; 16], volume(&[raw, huge])].concat();
        let w = walk_image(&image, Limits::default()).unwrap();
        assert_eq!(w.files.len(), 1);
        assert!(
            w.diagnostics
                .iter()
                .any(|d| d.contains("invalid file size"))
        );
    }
}
