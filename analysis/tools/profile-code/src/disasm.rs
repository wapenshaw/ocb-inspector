//! Linear-sweep x64 disassembly helpers over one extracted module (iced-x86 decoder).
//! RVAs are tool-independent: import the same module elsewhere and the RVA names the same byte.

use crate::pe::{self, Image};
use crate::{hex, opt};
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter, OpKind};
use std::fs;

fn load(args: &[String]) -> Result<(Vec<u8>, Image), String> {
    let path = opt(args, "--module").ok_or("--module required")?;
    let b = fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    let img = pe::parse(&b).ok_or("not a PE32+/TE image")?;
    if img.machine != 0x8664 {
        return Err(format!("machine 0x{:04X} is not x64", img.machine));
    }
    Ok((b, img))
}

/// A printable string at an RVA (ASCII or UTF-16LE), for annotating references.
pub fn string_at(b: &[u8], img: &Image, rva: u64) -> Option<String> {
    let off = img.rva_to_offset(u32::try_from(rva).ok()?)?;
    let s = b.get(off..)?;
    let ascii: Vec<u8> = s
        .iter()
        .take(96)
        .take_while(|&&c| c != 0)
        .copied()
        .collect();
    if ascii.len() >= 3
        && ascii.iter().all(|&c| (0x20..0x7F).contains(&c))
        && s.get(ascii.len()) == Some(&0)
    {
        return Some(format!("\"{}\"", String::from_utf8_lossy(&ascii)));
    }
    let mut u = Vec::new();
    for c in s.as_chunks::<2>().0.iter().take(96) {
        let v = u16::from_le_bytes(*c);
        if v == 0 {
            break;
        }
        if !(0x20..0x7F).contains(&v) {
            return None;
        }
        u.push(v);
    }
    (u.len() >= 3).then(|| format!("L\"{}\"", String::from_utf16_lossy(&u)))
}

/// Decodes every executable section linearly, calling `f` per instruction.
fn sweep(b: &[u8], img: &Image, mut f: impl FnMut(&Instruction)) {
    for s in img.executable() {
        let Some(start) = img.rva_to_offset(s.vaddr) else {
            continue;
        };
        let len = (s.raw_size.min(s.vsize.max(s.raw_size))) as usize;
        let Some(code) = b.get(start..(start + len).min(b.len())) else {
            continue;
        };
        let mut d = Decoder::with_ip(64, code, u64::from(s.vaddr), DecoderOptions::NONE);
        let mut ins = Instruction::default();
        while d.can_decode() {
            d.decode_out(&mut ins);
            f(&ins);
        }
    }
}

fn annotate(b: &[u8], img: &Image, ins: &Instruction) -> String {
    let mut notes = Vec::new();
    if ins.is_ip_rel_memory_operand() {
        let t = ins.ip_rel_memory_address();
        notes.push(format!("-> 0x{t:X}"));
        if let Some(s) = string_at(b, img, t) {
            notes.push(s);
        }
    }
    for i in 0..ins.op_count() {
        if matches!(ins.op_kind(i), OpKind::NearBranch64) {
            notes.push(format!("-> 0x{:X}", ins.near_branch_target()));
        }
    }
    if notes.is_empty() {
        String::new()
    } else {
        format!("    ; {}", notes.join(" "))
    }
}

fn print(b: &[u8], img: &Image, ins: &Instruction) {
    let mut fmt = IntelFormatter::new();
    fmt.options_mut().set_rip_relative_addresses(true);
    let mut out = String::new();
    fmt.format(ins, &mut out);
    println!("{:08X}  {:<48}{}", ins.ip(), out, annotate(b, img, ins));
}

pub fn cmd_disasm(args: &[String]) -> Result<(), String> {
    let (b, img) = load(args)?;
    let rva = opt(args, "--rva")
        .map(hex)
        .transpose()?
        .unwrap_or(u64::from(img.entry));
    let count: usize = opt(args, "--count")
        .and_then(|c| c.parse().ok())
        .unwrap_or(80);
    let stop_at_ret = args.iter().any(|a| a == "--func");
    let off = img
        .rva_to_offset(u32::try_from(rva).map_err(|e| e.to_string())?)
        .ok_or("RVA not in a section")?;
    let mut d = Decoder::with_ip(64, &b[off..], rva, DecoderOptions::NONE);
    let mut ins = Instruction::default();
    let mut n = 0;
    while d.can_decode() && n < count {
        d.decode_out(&mut ins);
        print(&b, &img, &ins);
        n += 1;
        if stop_at_ret
            && matches!(
                ins.mnemonic(),
                iced_x86::Mnemonic::Ret | iced_x86::Mnemonic::Int3
            )
        {
            break;
        }
    }
    Ok(())
}

/// Function ranges: the exception table when present, else direct call targets (plus the entry
/// point) as starts, each running to the next start. The fallback is a heuristic boundary.
pub fn functions(b: &[u8], img: &Image) -> Vec<(u32, u32)> {
    let f = img.functions(b);
    if !f.is_empty() {
        return f;
    }
    let mut starts = std::collections::BTreeSet::new();
    starts.insert(img.entry);
    let mut ends = Vec::new();
    let mut code_refs = Vec::new();
    sweep(b, img, |ins| {
        if ins.mnemonic() == iced_x86::Mnemonic::Call && ins.op0_kind() == OpKind::NearBranch64 {
            starts.insert(ins.near_branch_target() as u32);
        }
        // Address-taken code (protocol tables, callbacks): `lea reg, [rip+x]` into code.
        if ins.mnemonic() == iced_x86::Mnemonic::Lea && ins.is_ip_rel_memory_operand() {
            code_refs.push(ins.ip_rel_memory_address() as u32);
        }
    });
    for s in img.executable() {
        ends.push(s.vaddr + s.vsize.max(s.raw_size));
        let end = s.vaddr + s.vsize.max(s.raw_size);
        starts.extend(
            code_refs
                .iter()
                .copied()
                .filter(|&r| r >= s.vaddr && r < end),
        );
    }
    let v: Vec<u32> = starts.into_iter().collect();
    v.iter()
        .enumerate()
        .map(|(i, &s)| {
            let next = v.get(i + 1).copied().unwrap_or(u32::MAX);
            let sec_end = ends
                .iter()
                .copied()
                .filter(|&e| e > s)
                .min()
                .unwrap_or(s + 1);
            (s, next.min(sec_end))
        })
        .collect()
}

fn function_of(b: &[u8], img: &Image, rva: u32) -> Option<(u32, u32)> {
    functions(b, img)
        .into_iter()
        .filter(|&(s, e)| rva >= s && rva < e)
        .min_by_key(|&(s, e)| e - s)
}

/// Disassembles the whole function containing --rva (bounds from the exception table).
pub fn cmd_func(args: &[String]) -> Result<(), String> {
    let (b, img) = load(args)?;
    let rva = opt(args, "--rva")
        .map(hex)
        .transpose()?
        .ok_or("--rva required")?;
    let (s, e) =
        function_of(&b, &img, rva as u32).ok_or("no exception-table entry covers that RVA")?;
    println!("; function 0x{s:X}..0x{e:X}");
    let off = img.rva_to_offset(s).ok_or("unmapped")?;
    let code = b.get(off..off + (e - s) as usize).ok_or("truncated")?;
    let mut d = Decoder::with_ip(64, code, u64::from(s), DecoderOptions::NONE);
    let mut ins = Instruction::default();
    while d.can_decode() {
        d.decode_out(&mut ins);
        print(&b, &img, &ins);
    }
    Ok(())
}

/// Callers: direct calls/jumps to the function containing --rva, plus address-taken uses.
pub fn cmd_callers(args: &[String]) -> Result<(), String> {
    let (b, img) = load(args)?;
    let rva = opt(args, "--rva")
        .map(hex)
        .transpose()?
        .ok_or("--rva required")?;
    let (s, _) = function_of(&b, &img, rva as u32).ok_or("no function")?;
    let funcs = functions(&b, &img);
    sweep(&b, &img, |ins| {
        let br = (0..ins.op_count()).any(|i| matches!(ins.op_kind(i), OpKind::NearBranch64))
            && ins.near_branch_target() == u64::from(s);
        let mem = ins.is_ip_rel_memory_operand() && ins.ip_rel_memory_address() == u64::from(s);
        if br || mem {
            let f = funcs
                .iter()
                .filter(|&&(a, e)| ins.ip() >= u64::from(a) && ins.ip() < u64::from(e))
                .map(|&(a, _)| format!("in 0x{a:X}"))
                .next()
                .unwrap_or_default();
            print!("{f:<12}");
            print(&b, &img, ins);
        }
    });
    Ok(())
}

pub fn cmd_xrefs(args: &[String]) -> Result<(), String> {
    let (b, img) = load(args)?;
    let target = opt(args, "--rva")
        .map(hex)
        .transpose()?
        .ok_or("--rva required")?;
    sweep(&b, &img, |ins| {
        let mem = ins.is_ip_rel_memory_operand() && ins.ip_rel_memory_address() == target;
        let br = (0..ins.op_count()).any(|i| matches!(ins.op_kind(i), OpKind::NearBranch64))
            && ins.near_branch_target() == target;
        if mem || br {
            print(&b, &img, ins);
        }
    });
    Ok(())
}

pub fn cmd_find(args: &[String]) -> Result<(), String> {
    let (b, img) = load(args)?;
    let want = opt(args, "--imm")
        .map(hex)
        .transpose()?
        .ok_or("--imm required")?;
    sweep(&b, &img, |ins| {
        for i in 0..ins.op_count() {
            let v = match ins.op_kind(i) {
                OpKind::Immediate8 => u64::from(ins.immediate8()),
                OpKind::Immediate16 => u64::from(ins.immediate16()),
                OpKind::Immediate32 => u64::from(ins.immediate32()),
                OpKind::Immediate32to64 => ins.immediate32to64() as u64,
                OpKind::Immediate8to32 => ins.immediate8to32() as u32 as u64,
                OpKind::Immediate8to64 => ins.immediate8to64() as u64,
                OpKind::Immediate64 => ins.immediate64(),
                _ => continue,
            };
            if v == want || (want <= 0xFFFF_FFFF && v & 0xFFFF_FFFF == want) {
                print(&b, &img, ins);
            }
        }
    });
    Ok(())
}
