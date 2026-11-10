//! Offline research tool (never shipped, never built by CI). Static analysis only: nothing
//! extracted is executed. Workflow: docs/FIRMWARE_PROFILE_FORMAT_RESEARCH.md. Build here with
//! `cargo build --release`; outputs belong under `analysis/out/` (git-ignored), never in git.
//!
//!   profile-code inventory --bios <image> --out <dir>
//!       Walks the image with ocb-core's bounded walker, writes every PE32/TE module and a
//!       manifest (provenance, digests, PE layout) plus an anchor index (ASCII and UTF-16).
//!       Each run goes to <dir>/<bios sha256>/run-<n>; earlier runs are never overwritten.
//!   profile-code disasm --module <file> [--rva <hex>] [--count N] [--func]
//!   profile-code func --module <file> --rva <hex>
//!       Linear-sweep disassembly of the function that starts at an RVA.
//!   profile-code callers --module <file> --rva <hex>
//!   profile-code xrefs --module <file> --rva <hex>
//!       RIP-relative and call/jmp references to an RVA inside one module.
//!   profile-code find --module <file> --imm <hex>
//!       Instructions that use an immediate.
//!   profile-code survey --list <file of BIOS paths> --out <jsonl>
//!       Per image: profile save/load fingerprints, skip masks, `$SPF` lists, `$OCI$` builders
//!       (summarize with summarize_survey.py).

mod disasm;
mod pe;
mod survey;

use ocb_core::bytes::sha256_hex;
use ocb_core::firmware::volume::{Limits, Pe32Ref, walk_image};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const TOOL_VERSION: &str = "profile-code/1";
const MAX_MODULES: usize = 4096;
const MAX_OUTPUT: usize = 1 << 30;
const MAX_HITS_PER_ANCHOR: usize = 64;

/// Discovery anchors (doc §7). Leads to rank, never format proof.
const ANCHORS: &[&str] = &[
    "$MOS$",
    "$OCI$",
    ".ocb",
    ".OCB",
    "MsOcFile",
    "OC Profile",
    "Overclocking Profile",
    "Save to File",
    "Load from File",
    "Save Profile",
    "Load Profile",
    "Profile",
    "%04d%02d%02d",
    "%02d%02d%02d",
    "EFI_SIMPLE_FILE_SYSTEM",
    "SimpleFileSystem",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("inventory") => inventory(&args[1..]),
        Some("disasm") => disasm::cmd_disasm(&args[1..]),
        Some("xrefs") => disasm::cmd_xrefs(&args[1..]),
        Some("survey") => survey::cmd_survey(&args[1..]),
        Some("func") => disasm::cmd_func(&args[1..]),
        Some("callers") => disasm::cmd_callers(&args[1..]),
        Some("find") => disasm::cmd_find(&args[1..]),
        _ => Err("usage: profile-code inventory|disasm|func|callers|xrefs|find|survey ... (see src/main.rs)".into()),
    };
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
}

pub fn opt<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

pub fn hex(s: &str) -> Result<u64, String> {
    u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| format!("bad hex {s}: {e}"))
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return Vec::new();
    }
    hay.windows(needle.len())
        .enumerate()
        .filter(|(_, w)| *w == needle)
        .map(|(i, _)| i)
        .take(MAX_HITS_PER_ANCHOR)
        .collect()
}

fn safe_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect()
}

fn next_run_dir(base: &Path) -> Result<PathBuf, String> {
    for n in 1..10_000 {
        let d = base.join(format!("run-{n}"));
        match fs::create_dir(&d) {
            Ok(()) => return Ok(d),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("create {}: {e}", d.display())),
        }
    }
    Err("too many runs".into())
}

fn inventory(args: &[String]) -> Result<(), String> {
    let bios = PathBuf::from(opt(args, "--bios").ok_or("--bios required")?);
    let out = PathBuf::from(opt(args, "--out").ok_or("--out required")?);
    let bios_abs = fs::canonicalize(&bios).map_err(|e| e.to_string())?;
    if let Ok(o) = fs::canonicalize(&out)
        && bios_abs.starts_with(&o)
    {
        return Err("output directory must not contain the input".into());
    }
    let image = fs::read(&bios).map_err(|e| format!("read {}: {e}", bios.display()))?;
    let digest = sha256_hex(&image);
    let id = ocb_core::firmware::find_identity(&image)
        .ok()
        .map(|i| i.bios_id);
    let walk = walk_image(&image, Limits::default()).map_err(|e| e.to_string())?;

    let base = out.join(&digest);
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let run = next_run_dir(&base)?;
    let mods_dir = run.join("modules");
    fs::create_dir(&mods_dir).map_err(|e| e.to_string())?;

    let mut written = 0usize;
    let mut modules = Vec::new();
    let mut keys: BTreeMap<String, usize> = BTreeMap::new();
    let mut anchors: Vec<Value> = Vec::new();

    let mut status = "complete";
    'files: for f in &walk.files {
        let sections: Vec<(&str, &Pe32Ref)> = f
            .pe32
            .iter()
            .map(|r| ("pe32", r))
            .chain(f.te.iter().map(|r| ("te", r)))
            .collect();
        for (kind, r) in sections {
            if modules.len() >= MAX_MODULES {
                status = "incomplete: module cap reached";
                break 'files;
            }
            let bytes = walk.bytes_of(r);
            let name = f.name.clone().unwrap_or_default();
            let stem = format!(
                "{}-{}",
                safe_name(if name.is_empty() { "unnamed" } else { &name }),
                &f.guid[..8]
            );
            let n = keys.entry(stem.clone()).or_insert(0);
            *n += 1;
            let key = if *n == 1 { stem } else { format!("{stem}-{n}") };
            let sha = sha256_hex(bytes);
            written += bytes.len();
            if written > MAX_OUTPUT {
                status = "incomplete: output cap reached";
                break 'files;
            }
            let file = mods_dir.join(format!("{key}.efi"));
            fs::write(&file, bytes).map_err(|e| e.to_string())?;
            let pe = pe::parse(bytes);
            let layout = pe.as_ref().map(|p| {
                json!({
                    "format": p.kind, "machine": format!("0x{:04X}", p.machine),
                    "entry_rva": format!("0x{:X}", p.entry), "image_base": format!("0x{:X}", p.image_base),
                    "size_of_image": p.size_of_image, "te_adjust": p.te_adjust, "relocations": p.has_relocs,
                    "sections": p.sections.iter().map(|s| json!({
                        "name": s.name, "rva": format!("0x{:X}", s.vaddr), "vsize": s.vsize,
                        "raw_offset": s.raw_ptr, "raw_size": s.raw_size,
                        "characteristics": format!("0x{:08X}", s.characteristics)})).collect::<Vec<_>>(),
                })
            });
            for a in ANCHORS {
                for (enc, needle) in [("ascii", a.as_bytes().to_vec()), ("utf16", utf16(a))] {
                    for off in find_all(bytes, &needle) {
                        anchors.push(json!({
                            "anchor": a, "encoding": enc, "module": key, "offset": off,
                            "rva": pe.as_ref().and_then(|p| p.offset_to_rva(off)).map(|r| format!("0x{r:X}")),
                        }));
                    }
                }
            }
            modules.push(json!({
                "key": key, "ffs_guid": f.guid, "ui_name": f.name, "path": f.path, "section": kind,
                "container": match r.buf { ocb_core::firmware::volume::BufId::Image => "image".to_string(),
                    ocb_core::firmware::volume::BufId::Arena(i) => format!("decompressed buffer {i} (offsets are not BIOS file offsets)") },
                "section_body": [r.start, r.end], "sha256": sha, "size": bytes.len(),
                "layout": layout, "parse": if pe.is_some() { "ok" } else { "unrecognized header" },
            }));
        }
    }
    // Anchors outside any module body (raw image and decompressed buffers): HII packages,
    // NVRAM, freeform sections.
    let mut buffers = vec![("image".to_string(), &image[..])];
    for (i, a) in walk.arena.iter().enumerate() {
        buffers.push((format!("arena{i}"), &a[..]));
    }
    let mut raw_hits = Vec::new();
    for (name, b) in &buffers {
        for a in ["$MOS$", "$OCI$", ".ocb", "MsOcFile", "$SPF"] {
            for (enc, needle) in [("ascii", a.as_bytes().to_vec()), ("utf16", utf16(a))] {
                for off in find_all(b, &needle) {
                    raw_hits
                        .push(json!({"buffer": name, "anchor": a, "encoding": enc, "offset": off}));
                }
            }
        }
    }
    // Keep the decompressed buffers that hold anchors (e.g. a setup-data blob outside any module).
    let bufs_dir = run.join("buffers");
    let mut kept = Vec::new();
    for (i, a) in walk.arena.iter().enumerate() {
        let name = format!("arena{i}");
        if raw_hits.iter().any(|h| h["buffer"] == name.as_str()) {
            fs::create_dir_all(&bufs_dir).map_err(|e| e.to_string())?;
            fs::write(bufs_dir.join(format!("{name}.bin")), a).map_err(|e| e.to_string())?;
            kept.push(json!({"buffer": name, "path": walk.arena_paths.get(i), "size": a.len(), "sha256": sha256_hex(a)}));
        }
    }
    let manifest = json!({
        "schema_version": 1,
        "tool": TOOL_VERSION,
        "status": status,
        "input": {"path": bios.display().to_string(), "bios_sha256": digest, "size": image.len(), "bios_id": id},
        "walker_diagnostics": walk.diagnostics,
        "module_count": modules.len(),
        "modules": modules,
        "anchors": anchors,
        "raw_anchor_hits": raw_hits,
        "kept_buffers": kept,
    });
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(run.join("inventory.json"), text).map_err(|e| e.to_string())?;
    println!("{}", run.display());
    Ok(())
}
