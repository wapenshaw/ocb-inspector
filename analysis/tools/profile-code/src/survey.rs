//! `profile-code survey --list <file of BIOS paths> --out <jsonl>`
//! Per image: AMITSE profile save/load routine fingerprints, their skip masks and referenced
//! strings, the `$SPF` setup-data variable list, and the OcProfileInfo trailer builder.
//! Fingerprints hash the instruction stream with RIP-relative and branch targets masked, so a
//! rebuilt module with the same code shape at different addresses fingerprints the same.

use crate::disasm::{functions, string_at};
use crate::opt;
use crate::pe::{self, Image};
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter, Mnemonic, OpKind};
use ocb_core::bytes::sha256_hex;
use ocb_core::firmware::volume::{Limits, walk_image};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;

fn decode(b: &[u8], img: &Image, s: u32, e: u32) -> Vec<Instruction> {
    let Some(off) = img.rva_to_offset(s) else {
        return Vec::new();
    };
    let Some(code) = b.get(off..(off + (e - s) as usize).min(b.len())) else {
        return Vec::new();
    };
    let mut d = Decoder::with_ip(64, code, u64::from(s), DecoderOptions::NONE);
    let mut v = Vec::new();
    while d.can_decode() {
        v.push(d.decode());
    }
    v
}

fn masked(ins: &Instruction) -> String {
    let branch = (0..ins.op_count()).any(|i| matches!(ins.op_kind(i), OpKind::NearBranch64));
    if ins.is_ip_rel_memory_operand() || branch {
        let mut s = format!("{:?}", ins.mnemonic());
        for i in 0..ins.op_count() {
            s.push_str(&format!(" {:?}", ins.op_kind(i)));
            if ins.op_kind(i) == OpKind::Register {
                s.push_str(&format!(":{:?}", ins.op_register(i)));
            }
        }
        s
    } else {
        let mut f = IntelFormatter::new();
        let mut s = String::new();
        f.format(ins, &mut s);
        s
    }
}

fn fingerprint(code: &[Instruction]) -> String {
    let text: Vec<String> = code.iter().map(masked).collect();
    sha256_hex(text.join("\n").as_bytes())[..16].to_string()
}

fn describe(b: &[u8], img: &Image, s: u32, e: u32) -> Value {
    let code = decode(b, img, s, e);
    let mut strings = BTreeSet::new();
    let mut imm64 = BTreeSet::new();
    let mut ports = BTreeSet::new();
    for ins in &code {
        if ins.is_ip_rel_memory_operand()
            && let Some(t) = string_at(b, img, ins.ip_rel_memory_address())
        {
            strings.insert(t);
        }
        for i in 0..ins.op_count() {
            if ins.op_kind(i) == OpKind::Immediate64 {
                imm64.insert(format!("0x{:X}", ins.immediate64()));
            }
        }
        if matches!(ins.mnemonic(), Mnemonic::In | Mnemonic::Out) {
            ports.insert(format!("{:?}", ins.mnemonic()));
        }
    }
    json!({"rva": format!("0x{s:X}"), "len": e - s, "insns": code.len(), "fingerprint": fingerprint(&code),
           "strings": strings, "imm64": imm64, "port_io": ports})
}

/// Functions in `b` that reference (RIP-relative) the ASCII string `needle`.
fn referencing(b: &[u8], img: &Image, funcs: &[(u32, u32)], needle: &[u8]) -> Vec<(u32, u32)> {
    let mut targets = Vec::new();
    let mut i = 0;
    while let Some(p) = b[i..].windows(needle.len()).position(|w| w == needle) {
        if let Some(r) = img.offset_to_rva(i + p) {
            targets.push(u64::from(r));
        }
        i += p + 1;
    }
    let mut out = BTreeSet::new();
    for &(s, e) in funcs {
        for ins in decode(b, img, s, e) {
            if ins.is_ip_rel_memory_operand() && targets.contains(&ins.ip_rel_memory_address()) {
                out.insert((s, e));
            }
        }
    }
    out.into_iter().collect()
}

fn spf_variables(buf: &[u8]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = buf[from..].windows(4).position(|w| w == b"$SPF") {
        let base = from + p;
        from = base + 4;
        let b = &buf[base..];
        let rd = |o: usize| {
            b.get(o..o + 4)
                .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
        };
        let Some(vl) = rd(0x34) else { continue };
        let Some(n) = rd(vl) else { continue };
        if n == 0 || n > 4096 {
            out.push(json!({"at": base, "error": "implausible count"}));
            continue;
        }
        let mut vars = Vec::new();
        let mut strides = BTreeSet::new();
        let mut prev = None;
        for i in 0..n {
            let Some(o) = rd(vl + 4 + i * 4) else { break };
            if let Some(p) = prev {
                strides.insert(o as i64 - p as i64);
            }
            prev = Some(o);
            let e = vl + o;
            let Some(ent) = b.get(e..e + 124) else { break };
            let guid = ocb_core::bytes::guid_string(&ent[..16]);
            let name: String = String::from_utf16_lossy(
                &ent[16..96]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .take_while(|&c| c != 0)
                    .collect::<Vec<_>>(),
            );
            let size = u32::from_le_bytes([ent[120], ent[121], ent[122], ent[123]]);
            vars.push(json!([guid, name, size]));
        }
        out.push(json!({"at": base, "count": n, "strides": strides, "vars": vars}));
    }
    out
}

pub fn cmd_survey(args: &[String]) -> Result<(), String> {
    let list = fs::read_to_string(opt(args, "--list").ok_or("--list required")?)
        .map_err(|e| e.to_string())?;
    let mut out =
        fs::File::create(opt(args, "--out").ok_or("--out required")?).map_err(|e| e.to_string())?;
    for path in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let row = survey_one(path).unwrap_or_else(|e| json!({"path": path, "error": e}));
        writeln!(out, "{row}").map_err(|e| e.to_string())?;
        eprintln!("{path}");
    }
    Ok(())
}

fn survey_one(path: &str) -> Result<Value, String> {
    let image = fs::read(path).map_err(|e| e.to_string())?;
    let walk = walk_image(&image, Limits::default()).map_err(|e| e.to_string())?;
    let id = ocb_core::firmware::find_identity(&image)
        .ok()
        .map(|i| i.bios_id);
    let mut tse = Vec::new();
    let mut opi = Vec::new();
    for f in &walk.files {
        let name = f.name.clone().unwrap_or_default();
        for r in &f.pe32 {
            let b = walk.bytes_of(r);
            let Some(img) = pe::parse(b) else { continue };
            if img.machine != 0x8664 {
                continue;
            }
            let has_mos = b.windows(6).any(|w| w == b"$MOS$\0");
            let has_oci = b.windows(6).any(|w| w == b"$OCI$\0");
            if !has_mos && !has_oci {
                continue;
            }
            let funcs = functions(b, &img);
            let mos: Vec<Value> = referencing(b, &img, &funcs, b"$MOS$\0")
                .into_iter()
                .map(|(s, e)| describe(b, &img, s, e))
                .collect();
            let oci: Vec<Value> = referencing(b, &img, &funcs, b"$OCI$\0")
                .into_iter()
                .map(|(s, e)| describe(b, &img, s, e))
                .collect();
            let entry = json!({"module": name, "guid": f.guid, "path": f.path, "sha256": sha256_hex(b), "mos_functions": mos, "oci_functions": oci});
            if has_oci && !name.starts_with("AMITSE") {
                opi.push(entry)
            } else {
                tse.push(entry)
            }
        }
    }
    let mut spf = Vec::new();
    for (i, a) in walk.arena.iter().enumerate() {
        for mut s in spf_variables(a) {
            s["buffer"] = json!(walk.arena_paths.get(i));
            spf.push(s);
        }
    }
    Ok({
        // `$OCI$` code can sit in a separate module (OcProfileInfo) or in the setup browser
        // itself (AM5, Intel 800): an image without a trailer module may still write one.
        let oci_in_browser = tse
            .iter()
            .any(|m| m["oci_functions"].as_array().is_some_and(|f| !f.is_empty()));
        json!({"path": path, "bios_sha256": sha256_hex(&image), "bios_id": id, "profile_modules": tse, "trailer_modules": opi, "oci_in_setup_browser": oci_in_browser, "spf": spf})
    })
}
