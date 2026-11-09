//! EFI 1.1 ("UEFI Decompress", EDK2 version 1) and Tiano (EDK2 version 2)
//! decompression for the stream inside an FFS compression section
//! (section type 0x01, CompressionType 0x01).
//!
//! Input layout: `u32 CompSize`, `u32 OrigSize`, then `CompSize` bytes of
//! MSB-first bit stream. The section body's leading `u32 UncompressedLength`
//! and `u8 CompressionType` are not part of this input.
//!
//! Provenance and licence: the bit order, table construction, constants and
//! loop semantics follow the UEFI/PI compression description. EDK2's
//! `MdePkg/Library/BaseUefiDecompressLib/BaseUefiDecompressLib.c` and
//! `BaseUefiDecompressLibInternals.h` were consulted as a reference, as the
//! project brief permits. Those files are licensed under:
//!
//! ```text
//! SPDX-License-Identifier: BSD-2-Clause-Patent
//! Copyright (c) 2006 - 2019, Intel Corporation. All rights reserved.
//! Portions copyright (c) 2008 - 2009, Apple Inc. All rights reserved.
//! ```
//!
//! No GPL code was read. The full BSD-2-Clause-Patent text is in `LICENSE-EDK2.txt`
//! next to this file and must accompany any distribution of it.
//!
//! Validity (an `Ok` result satisfies all of these):
//! - the header is present, `OrigSize <= max_out`, and `CompSize` bytes follow it;
//! - every Huffman table is complete (Kraft sum exactly 1) with lengths <= 16,
//!   or is a single symbol coded with zero bits;
//! - every code decodes to an assigned symbol, and no read goes past
//!   `CompSize * 8` bits;
//! - every match refers to bytes already produced and does not extend past
//!   `OrigSize`;
//! - the output is exactly `OrigSize` bytes;
//! - the decoder consumed all but at most one of the `CompSize` bytes
//!   (`ceil(bits / 8)`); the encoder's trailing byte is observed, not spec-given.
//!
//! `decompress` tries EFI 1.1 first and falls back to Tiano.
//!
//! The decoder never panics on malformed input and contains no `unsafe`.

/// Stream variant. The two differ only in the width of the position-table
/// count field (`PBit`): 4 bits for EFI 1.1, 5 bits for Tiano.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// UEFI 1.1 "EFI" algorithm: 4-bit position-table count field.
    Efi11,
    /// Tiano algorithm: 5-bit position-table count field.
    Tiano,
}

impl Variant {
    fn position_count_bits(self) -> u32 {
        match self {
            Variant::Efi11 => 4,
            Variant::Tiano => 5,
        }
    }
}

const HEADER_LEN: usize = 8;
const THRESHOLD: usize = 3;
const MAX_MATCH: usize = 256;
/// Literal/length symbols: 256 literals plus match lengths 3..=256.
const NC: usize = 0xff + MAX_MATCH + 2 - THRESHOLD; // 510
/// Width of the literal/length code-count field.
const CBIT: u32 = 9;
/// Code-length-code symbols (the "T" table).
const NT: usize = 19;
/// Width of the T-table count field.
const TBIT: u32 = 5;
/// Position symbols (EDK2 `MAXNP`). Only the count field width depends on the variant.
const NP: usize = 31;

/// Decodes `src` (starting at the `CompSize` field) with the EFI 1.1 variant,
/// falling back to Tiano. Refuses `OrigSize > max_out`.
pub fn decompress(src: &[u8], max_out: usize) -> Result<Vec<u8>, String> {
    decompress_detect(src, max_out).map(|(_, out)| out)
}

/// Like [`decompress`], but also reports which variant produced the output.
pub fn decompress_detect(src: &[u8], max_out: usize) -> Result<(Variant, Vec<u8>), String> {
    match decompress_with(src, Variant::Efi11, max_out) {
        Ok(out) => Ok((Variant::Efi11, out)),
        Err(efi_err) => match decompress_with(src, Variant::Tiano, max_out) {
            Ok(out) => Ok((Variant::Tiano, out)),
            Err(tiano_err) => Err(format!("{efi_err}; {tiano_err}")),
        },
    }
}

/// Decodes `src` with one specific variant.
pub fn decompress_with(src: &[u8], variant: Variant, max_out: usize) -> Result<Vec<u8>, String> {
    decompress_slack(src, variant, max_out).map(|(out, _)| out)
}

/// Like [`decompress_with`], also returning the number of unread trailing
/// bytes of the `CompSize` payload (diagnostic).
pub fn decompress_slack(
    src: &[u8],
    variant: Variant,
    max_out: usize,
) -> Result<(Vec<u8>, usize), String> {
    decode(src, variant, max_out).map_err(|e| format!("{variant:?}: {e}"))
}

fn decode(src: &[u8], variant: Variant, max_out: usize) -> Result<(Vec<u8>, usize), &'static str> {
    let comp = read_u32(src, 0).ok_or("truncated header")? as usize;
    let orig = read_u32(src, 4).ok_or("truncated header")? as usize;
    if orig > max_out {
        return Err("OrigSize exceeds max_out");
    }
    let payload = src
        .get(HEADER_LEN..)
        .and_then(|rest| rest.get(..comp))
        .ok_or("CompSize exceeds input")?;
    if orig == 0 {
        return Ok((Vec::new(), payload.len()));
    }

    let mut bits = BitReader::new(payload);
    let mut out: Vec<u8> = Vec::with_capacity(orig);
    let mut block_left: u32 = 0;
    let mut c_code = Code::single(0);
    let mut p_code = Code::single(0);

    while out.len() < orig {
        if block_left == 0 {
            block_left = bits.read(16)?;
            if block_left == 0 {
                return Err("zero-length block");
            }
            let t_code = read_position_table(&mut bits, NT, TBIT, Some(3))?;
            c_code = read_c_len(&mut bits, &t_code)?;
            p_code = read_position_table(&mut bits, NP, variant.position_count_bits(), None)?;
        }
        block_left -= 1;

        let sym = usize::from(c_code.decode(&mut bits)?);
        if sym < 256 {
            out.push(sym as u8);
            continue;
        }
        let len = sym - (256 - THRESHOLD);
        let dist = decode_distance(&mut bits, &p_code)?;
        let start = out
            .len()
            .checked_sub(dist + 1)
            .ok_or("match distance exceeds output")?;
        if len > orig - out.len() {
            return Err("match overruns OrigSize");
        }
        for k in 0..len {
            let byte = *out.get(start + k).ok_or("match source out of range")?;
            out.push(byte);
        }
    }

    // Observed: every valid EFI 1.1 stream in the fixtures leaves exactly one
    // trailing byte unread. Allow at most one; more means the stream is not
    // the one the header describes.
    let slack = payload.len().saturating_sub(bits.pos.div_ceil(8));
    if slack > 1 {
        return Err("more than one trailing byte unread");
    }
    Ok((out, slack))
}

fn read_u32(src: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; 4] = src.get(at..at + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

/// MSB-first bit reader. Bits past the end read as zero for lookahead only;
/// consuming them is an error.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// The next `n` bits (`n <= 32`) without consuming them.
    fn peek(&self, n: u32) -> u32 {
        let byte = self.pos / 8;
        let off = (self.pos % 8) as u32;
        let mut window: u64 = 0;
        for k in 0..5 {
            let b = self.data.get(byte + k).copied().unwrap_or(0);
            window = (window << 8) | u64::from(b);
        }
        // `window` holds 40 bits starting at `byte`; the wanted bits start at `off`.
        ((window >> (40 - off - n)) & ((1u64 << n) - 1)) as u32
    }

    fn consume(&mut self, n: u32) -> Result<(), &'static str> {
        let end = self.pos + n as usize;
        if end > self.data.len() * 8 {
            return Err("compressed data exhausted");
        }
        self.pos = end;
        Ok(())
    }

    fn read(&mut self, n: u32) -> Result<u32, &'static str> {
        let v = self.peek(n);
        self.consume(n)?;
        Ok(v)
    }
}

/// Canonical Huffman code. Codes are assigned in order of length, then symbol
/// value, starting from all zeros. `single` marks the zero-width special case
/// used when a table's count field is zero.
struct Code {
    /// Number of symbols of each code length 1..=16 (index 0 unused).
    count: [u16; 17],
    /// Symbols ordered by (code length, symbol value).
    symbols: Vec<u16>,
    single: Option<u16>,
}

impl Code {
    fn single(sym: u16) -> Self {
        Self {
            count: [0; 17],
            symbols: Vec::new(),
            single: Some(sym),
        }
    }

    fn from_lengths(lengths: &[u8]) -> Result<Self, &'static str> {
        let mut count = [0u16; 17];
        for &len in lengths {
            let len = usize::from(len);
            if len > 16 {
                return Err("code length above 16");
            }
            if len > 0 {
                count[len] += 1;
            }
        }
        // The Kraft sum must be exactly 1 (complete code).
        let mut kraft: u32 = 0;
        for (len, &n) in count.iter().enumerate().skip(1) {
            kraft += u32::from(n) << (16 - len);
        }
        if kraft != 1u32 << 16 {
            return Err("Huffman table incomplete or oversubscribed");
        }

        let mut next = [0usize; 17];
        let mut acc = 0usize;
        for len in 1..=16usize {
            next[len] = acc;
            acc += usize::from(count[len]);
        }
        let mut symbols = vec![0u16; acc];
        for (sym, &len) in lengths.iter().enumerate() {
            let len = usize::from(len);
            if len == 0 {
                continue;
            }
            let slot = next[len];
            next[len] += 1;
            if let Some(s) = symbols.get_mut(slot) {
                *s = sym as u16;
            }
        }
        Ok(Self {
            count,
            symbols,
            single: None,
        })
    }

    fn decode(&self, bits: &mut BitReader) -> Result<u16, &'static str> {
        if let Some(sym) = self.single {
            return Ok(sym);
        }
        let window = bits.peek(16);
        let mut code: u32 = 0;
        let mut first: u32 = 0;
        let mut index: usize = 0;
        for len in 1..=16u32 {
            code |= (window >> (16 - len)) & 1;
            let count = u32::from(self.count[len as usize]);
            if code < first + count {
                bits.consume(len)?;
                let slot = index + (code - first) as usize;
                return self
                    .symbols
                    .get(slot)
                    .copied()
                    .ok_or("code maps to no symbol");
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("bit pattern matches no code")
    }
}

/// Reads a position-style code-length table (`ReadPTLen` in EDK2). `special`
/// is the index after which a 2-bit zero-run count follows (used by the T table).
fn read_position_table(
    bits: &mut BitReader,
    nn: usize,
    nbit: u32,
    special: Option<usize>,
) -> Result<Code, &'static str> {
    let n = bits.read(nbit)? as usize;
    if n == 0 {
        let sym = bits.read(nbit)? as usize;
        if sym >= nn {
            return Err("single symbol out of range");
        }
        return Ok(Code::single(sym as u16));
    }
    if n > nn {
        return Err("code-length count exceeds table size");
    }

    let mut lengths = vec![0u8; nn];
    let mut i = 0usize;
    while i < n {
        // Lengths 0..=6 are 3 bits. The value 7 is extended by a unary run of
        // ones starting at the fourth bit, terminated by a zero.
        let mut c = bits.peek(3);
        if c == 7 {
            let window = bits.peek(16);
            let mut mask: u32 = 1 << 12;
            while mask != 0 && window & mask != 0 {
                mask >>= 1;
                c += 1;
            }
        }
        if c > 16 {
            return Err("code length above 16");
        }
        bits.consume(if c < 7 { 3 } else { c - 3 })?;
        if let Some(slot) = lengths.get_mut(i) {
            *slot = c as u8;
        }
        i += 1;

        if Some(i) == special {
            let zeros = bits.read(2)? as usize;
            for _ in 0..zeros {
                if i >= nn {
                    break;
                }
                i += 1; // already zero
            }
        }
    }
    Code::from_lengths(&lengths)
}

/// Reads the literal/length code-length table (`ReadCLen` in EDK2).
fn read_c_len(bits: &mut BitReader, t_code: &Code) -> Result<Code, &'static str> {
    let n = bits.read(CBIT)? as usize;
    if n == 0 {
        let sym = bits.read(CBIT)? as usize;
        if sym >= NC {
            return Err("single literal/length symbol out of range");
        }
        return Ok(Code::single(sym as u16));
    }
    if n > NC {
        return Err("literal/length count exceeds table size");
    }

    let mut lengths = vec![0u8; NC];
    let mut i = 0usize;
    while i < n {
        let sym = usize::from(t_code.decode(bits)?);
        if sym <= 2 {
            // Zero-length runs: 0 = one entry, 1 = 4-bit count + 3, 2 = 9-bit count + 20.
            let run = match sym {
                0 => 1,
                1 => bits.read(4)? as usize + 3,
                _ => bits.read(CBIT)? as usize + 20,
            };
            if run > NC - i {
                return Err("zero-length run overflows table");
            }
            i += run;
        } else {
            // Symbols 3..=18 encode code lengths 1..=16.
            if let Some(slot) = lengths.get_mut(i) {
                *slot = (sym - 2) as u8;
            }
            i += 1;
        }
    }
    Code::from_lengths(&lengths)
}

/// Decodes a match distance (`DecodeP` in EDK2).
fn decode_distance(bits: &mut BitReader, p_code: &Code) -> Result<usize, &'static str> {
    let v = usize::from(p_code.decode(bits)?);
    if v > 1 {
        let extra = (v - 1) as u32;
        Ok((1usize << extra) + bits.read(extra)? as usize)
    } else {
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built stream: BlockSize 5; zero-width T, C and P tables with single
    /// symbols T=0, C=0x41 ('A') and P=0. Five literals decode with no bits.
    /// The payload bits are identical under both variants, so both decode it.
    fn hand_vector() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&7u32.to_le_bytes()); // CompSize
        v.extend_from_slice(&5u32.to_le_bytes()); // OrigSize
        v.extend_from_slice(&[0x00, 0x05, 0x00, 0x00, 0x04, 0x10, 0x00]);
        v
    }

    #[test]
    fn hand_vector_decodes_under_both_variants() {
        let src = hand_vector();
        assert_eq!(decompress_with(&src, Variant::Efi11, 16).unwrap(), b"AAAAA");
        assert_eq!(decompress_with(&src, Variant::Tiano, 16).unwrap(), b"AAAAA");
        let (variant, out) = decompress_detect(&src, 16).unwrap();
        assert_eq!(variant, Variant::Efi11);
        assert_eq!(out, b"AAAAA");
    }

    #[test]
    fn hand_vector_rejects_bad_framing() {
        let good = hand_vector();

        // Output limit.
        assert!(decompress(&good, 4).is_err());

        // Claimed CompSize larger than the input.
        let mut long = good.clone();
        long[0] = 8;
        assert!(decompress(&long, 16).is_err());

        // Input shorter than the claimed CompSize.
        assert!(decompress(&good[..good.len() - 1], 16).is_err());

        // Two bytes more than the stream uses: rejected (one trailing byte is allowed).
        let mut extra = good.clone();
        extra[0] = 9;
        extra.push(0);
        extra.push(0);
        assert!(decompress(&extra, 16).is_err());
        // One trailing byte is the observed encoder behaviour: accepted.
        let mut one = good.clone();
        one[0] = 8;
        one.push(0);
        assert_eq!(decompress_slack(&one, Variant::Efi11, 16).unwrap().1, 1);

        // Too short for a header.
        assert!(decompress(&good[..7], 16).is_err());
        assert!(decompress(&[], 16).is_err());
    }

    #[test]
    fn zero_orig_size_is_empty() {
        let src = [0u8, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(decompress(&src, 0).unwrap(), Vec::<u8>::new());
    }

    struct XorShift(u64);
    impl XorShift {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    #[test]
    fn random_inputs_never_panic() {
        let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
        let mut accepted = 0usize;
        for _ in 0..5000 {
            let len = (rng.next() % 96) as usize;
            let mut buf: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            if len >= 8 && rng.next().is_multiple_of(2) {
                let avail = (len - 8) as u64;
                let comp = (rng.next() % (avail + 1)) as u32;
                let orig = (rng.next() % 300) as u32;
                buf[0..4].copy_from_slice(&comp.to_le_bytes());
                buf[4..8].copy_from_slice(&orig.to_le_bytes());
            }
            if decompress_detect(&buf, 1 << 16).is_ok() {
                accepted += 1;
            }
        }
        // Random data should almost never validate; this is a sanity bound only.
        assert!(accepted < 5000);
    }

    #[test]
    fn mutated_vector_never_panics() {
        let base = hand_vector();
        for byte in 8..base.len() {
            for bit in 0..8 {
                let mut m = base.clone();
                m[byte] ^= 1 << bit;
                let _ = decompress(&m, 64);
            }
        }
        for orig in 0..64u32 {
            let mut m = base.clone();
            m[4..8].copy_from_slice(&orig.to_le_bytes());
            let _ = decompress(&m, 64);
        }
    }
}
