//! Minimal, bounds-checked PE32/PE32+ and TE header reader for research inventories.
//! Never executes anything; only reads headers so RVAs can be mapped to section bytes.

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub vaddr: u32,
    pub vsize: u32,
    pub raw_ptr: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}

#[derive(Debug, Clone)]
pub struct Image {
    pub kind: &'static str, // "pe32", "pe32+", "te"
    pub machine: u16,
    pub entry: u32,
    pub image_base: u64,
    pub size_of_image: u32,
    pub sections: Vec<Section>,
    /// TE only: bytes stripped from the original PE headers, minus the 40-byte TE header.
    /// file_offset = rva - te_adjust for TE images (see `rva_to_offset`).
    pub te_adjust: u32,
    pub has_relocs: bool,
    /// Exception directory (x64 RUNTIME_FUNCTION table): (rva, size).
    pub pdata: Option<(u32, u32)>,
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8)
        .map(|s| u64::from_le_bytes(s.try_into().unwrap_or([0; 8])))
}

fn sections(b: &[u8], at: usize, n: usize) -> Option<Vec<Section>> {
    let mut v = Vec::with_capacity(n.min(96));
    for i in 0..n.min(96) {
        let o = at + i * 40;
        let raw = b.get(o..o + 8)?;
        let name: String = raw
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as char)
            .collect();
        v.push(Section {
            name,
            vsize: u32_at(b, o + 8)?,
            vaddr: u32_at(b, o + 12)?,
            raw_size: u32_at(b, o + 16)?,
            raw_ptr: u32_at(b, o + 20)?,
            characteristics: u32_at(b, o + 36)?,
        });
    }
    Some(v)
}

pub fn parse(b: &[u8]) -> Option<Image> {
    if b.starts_with(b"MZ") {
        let pe = u32_at(b, 0x3C)? as usize;
        if b.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let coff = pe + 4;
        let machine = u16_at(b, coff)?;
        let nsec = u16_at(b, coff + 2)? as usize;
        let opt_size = u16_at(b, coff + 16)? as usize;
        let opt = coff + 20;
        let magic = u16_at(b, opt)?;
        let (kind, image_base, dd_count_at) = match magic {
            0x20B => ("pe32+", u64_at(b, opt + 24)?, opt + 108),
            0x10B => ("pe32", u64::from(u32_at(b, opt + 28)?), opt + 92),
            _ => return None,
        };
        let dd_count = u32_at(b, dd_count_at)? as usize;
        // Base relocation table is data directory 5.
        let has_relocs = dd_count > 5 && u32_at(b, dd_count_at + 4 + 5 * 8 + 4)? != 0;
        let pdata = (dd_count > 3)
            .then(|| {
                Some((
                    u32_at(b, dd_count_at + 4 + 3 * 8)?,
                    u32_at(b, dd_count_at + 4 + 3 * 8 + 4)?,
                ))
            })
            .flatten()
            .filter(|&(r, s)| r != 0 && s != 0);
        Some(Image {
            kind,
            machine,
            entry: u32_at(b, opt + 16)?,
            image_base,
            size_of_image: u32_at(b, opt + 56)?,
            sections: sections(b, opt + opt_size, nsec)?,
            te_adjust: 0,
            has_relocs,
            pdata,
        })
    } else if b.starts_with(b"VZ") {
        // EFI_TE_IMAGE_HEADER (PI spec vol. 1): 40 bytes.
        let machine = u16_at(b, 2)?;
        let nsec = usize::from(*b.get(4)?);
        let stripped = u16_at(b, 6)? as u32;
        let entry = u32_at(b, 8)?;
        let image_base = u64_at(b, 16)?;
        let reloc_rva = u32_at(b, 24)?;
        let te_adjust = stripped.checked_sub(40)?;
        Some(Image {
            kind: "te",
            machine,
            entry,
            image_base,
            size_of_image: 0,
            sections: sections(b, 40, nsec)?,
            te_adjust,
            has_relocs: reloc_rva != 0,
            pdata: Some((u32_at(b, 32)?, u32_at(b, 36)?)).filter(|&(r, s)| r != 0 && s != 0),
        })
    } else {
        None
    }
}

impl Image {
    /// File offset inside the section body for an RVA, or None when the RVA is in
    /// zero-filled (virtual-only) space or outside every section.
    pub fn rva_to_offset(&self, rva: u32) -> Option<usize> {
        for s in &self.sections {
            let span = s.vsize.max(s.raw_size);
            if rva >= s.vaddr && rva < s.vaddr.checked_add(span)? {
                let delta = rva - s.vaddr;
                if delta >= s.raw_size {
                    return None; // zero-fill
                }
                return Some((s.raw_ptr.checked_add(delta)?.checked_sub(self.te_adjust)?) as usize);
            }
        }
        None
    }

    pub fn offset_to_rva(&self, off: usize) -> Option<u32> {
        let off = u32::try_from(off).ok()?.checked_add(self.te_adjust)?;
        self.sections
            .iter()
            .find(|s| off >= s.raw_ptr && off < s.raw_ptr + s.raw_size)
            .map(|s| s.vaddr + (off - s.raw_ptr))
    }

    /// Function ranges from the exception directory, sorted by start.
    pub fn functions(&self, b: &[u8]) -> Vec<(u32, u32)> {
        let Some((rva, size)) = self.pdata else {
            return Vec::new();
        };
        let Some(off) = self.rva_to_offset(rva) else {
            return Vec::new();
        };
        let mut v: Vec<(u32, u32)> = (0..(size as usize / 12))
            .filter_map(|i| Some((u32_at(b, off + i * 12)?, u32_at(b, off + i * 12 + 4)?)))
            .filter(|&(s, e)| s < e)
            .collect();
        v.sort_unstable();
        v
    }

    pub fn executable(&self) -> impl Iterator<Item = &Section> {
        self.sections
            .iter()
            .filter(|s| s.characteristics & 0x2000_0000 != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pe_with(section_vaddr: u32, raw_ptr: u32, raw_size: u32, vsize: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        let coff = 0x84;
        b[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
        b[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        b[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
        let opt = coff + 20;
        b[opt..opt + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
        b[opt + 16..opt + 20].copy_from_slice(&0x1234u32.to_le_bytes());
        b[opt + 108..opt + 112].copy_from_slice(&16u32.to_le_bytes());
        let s = opt + 240;
        b[s..s + 5].copy_from_slice(b".text");
        b[s + 8..s + 12].copy_from_slice(&vsize.to_le_bytes());
        b[s + 12..s + 16].copy_from_slice(&section_vaddr.to_le_bytes());
        b[s + 16..s + 20].copy_from_slice(&raw_size.to_le_bytes());
        b[s + 20..s + 24].copy_from_slice(&raw_ptr.to_le_bytes());
        b[s + 36..s + 40].copy_from_slice(&0x6000_0020u32.to_le_bytes());
        b
    }

    #[test]
    fn pe_rva_maps_and_zero_fill_does_not() {
        let img = parse(&pe_with(0x1000, 0x200, 0x100, 0x300)).expect("pe");
        assert_eq!(img.kind, "pe32+");
        assert_eq!(img.entry, 0x1234);
        assert_eq!(img.rva_to_offset(0x1010), Some(0x210));
        assert_eq!(img.offset_to_rva(0x210), Some(0x1010));
        assert_eq!(img.rva_to_offset(0x1100), None, "zero-filled tail");
        assert_eq!(img.rva_to_offset(0x5000), None, "outside sections");
    }

    #[test]
    fn te_adjusts_for_stripped_headers() {
        let mut b = vec![0u8; 0x200];
        b[0..2].copy_from_slice(b"VZ");
        b[2..4].copy_from_slice(&0x8664u16.to_le_bytes());
        b[4] = 1;
        b[6..8].copy_from_slice(&0x1E0u16.to_le_bytes()); // stripped
        let s = 40;
        b[s..s + 5].copy_from_slice(b".text");
        b[s + 8..s + 12].copy_from_slice(&0x100u32.to_le_bytes());
        b[s + 12..s + 16].copy_from_slice(&0x240u32.to_le_bytes());
        b[s + 16..s + 20].copy_from_slice(&0x100u32.to_le_bytes());
        b[s + 20..s + 24].copy_from_slice(&0x240u32.to_le_bytes());
        let img = parse(&b).expect("te");
        assert_eq!(img.te_adjust, 0x1E0 - 40);
        // rva 0x250 -> raw 0x250 in the original PE -> 0x250 - 0x1B8 in the TE file.
        assert_eq!(img.rva_to_offset(0x250), Some(0x250 - 0x1B8));
        assert_eq!(img.offset_to_rva(0x250 - 0x1B8), Some(0x250));
    }

    #[test]
    fn truncated_headers_fail_cleanly() {
        let b = pe_with(0x1000, 0x200, 0x100, 0x100);
        for cut in [2, 0x40, 0x86, 0x100, 0x180] {
            let _ = parse(&b[..cut]); // must not panic
        }
        assert!(parse(&b[..0x86]).is_none());
    }
}
