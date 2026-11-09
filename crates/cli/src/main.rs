//! `ocbi` — headless CLI for OCB Inspector (SPEC §10).
//!
//! JSON goes to stdout or `--out`; progress and human messages go to stderr.

use clap::{Args, Parser, Subcommand, ValueEnum};
use ocb_core::contracts::{self, Patch, Selection, Value};
use ocb_core::engine::{Session, browse, edit, inspect, migrate, verify};
use ocb_core::error::{Error, Result};
use ocb_core::io;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "ocbi",
    version,
    about = "Inspect, edit, compare and migrate MSI BIOS OC profiles (.ocb)"
)]
struct Cli {
    /// Approvals file listing BIOS images that may be written to (default: OCB_APPROVALS, then
    /// config/approved-images.toml beside this executable). Without one, every image is inspect-only.
    #[arg(long, global = true, value_name = "PATH")]
    approvals: Option<PathBuf>,
    /// Settings file (default: config/settings.json beside this executable). Writing (edit and
    /// migrate build) is experimental and off unless it sets "experimental": {"writing": true}.
    #[arg(long, global = true, value_name = "PATH")]
    settings: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Copy, Clone, ValueEnum, PartialEq, Eq)]
enum Format {
    Text,
    Json,
}

#[derive(Args, Clone)]
struct Common {
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    /// Write the JSON document here (exclusive create) instead of stdout.
    #[arg(long)]
    out: Option<PathBuf>,
    #[arg(long)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// BIOS image operations.
    Bios {
        #[command(subcommand)]
        cmd: BiosCmd,
    },
    /// Profile operations.
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },
    /// Edit patch operations.
    Patch {
        #[command(subcommand)]
        cmd: PatchCmd,
    },
    /// Cross-release migration.
    Migrate {
        #[command(subcommand)]
        cmd: MigrateCmd,
    },
    /// Print capability and document support tables.
    Capabilities {
        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
    },
    /// JSON Schema output.
    Schema {
        #[command(subcommand)]
        cmd: SchemaCmd,
    },
}

#[derive(Subcommand)]
enum BiosCmd {
    Inspect {
        #[arg(long)]
        bios: PathBuf,
        #[command(flatten)]
        common: Common,
    },
    /// List every setting the BIOS menus define, with optimized defaults (any supported family).
    Browse {
        #[arg(long)]
        bios: PathBuf,
        /// Only settings whose prompt or menu path contains this text (case-insensitive).
        #[arg(long)]
        search: Option<String>,
        /// Include settings the BIOS never shows (defined only under suppressif TRUE).
        #[arg(long)]
        hidden: bool,
        /// Firmware set to show, for images holding several (32 MiB AM4); 1 is the default.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u16).range(1..))]
        set: u16,
        #[command(flatten)]
        common: Common,
    },
}

#[derive(Args, Clone)]
struct Pair {
    #[arg(long)]
    bios: PathBuf,
    #[arg(long)]
    profile: PathBuf,
}

#[derive(Subcommand)]
enum ProfileCmd {
    Inspect {
        #[command(flatten)]
        pair: Pair,
        /// Include every named Setup/CpuSetup field (read-only physical view).
        #[arg(long)]
        all_fields: bool,
        #[command(flatten)]
        common: Common,
    },
    Diff {
        /// Also compare every setting the BIOS menus show, matched by name (read-only).
        #[arg(long)]
        all_fields: bool,
        #[arg(long)]
        left_bios: PathBuf,
        #[arg(long)]
        left_profile: PathBuf,
        #[arg(long)]
        right_bios: PathBuf,
        #[arg(long)]
        right_profile: PathBuf,
        #[command(flatten)]
        common: Common,
    },
    Verify {
        #[command(flatten)]
        pair: Pair,
        #[command(flatten)]
        common: Common,
    },
    Build {
        #[arg(long)]
        bios: PathBuf,
        #[arg(long)]
        base_profile: PathBuf,
        /// Patch file, or `-` for stdin.
        #[arg(long)]
        patch: String,
        #[arg(long = "out")]
        output: PathBuf,
        #[arg(long)]
        report: PathBuf,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
}

#[derive(Subcommand)]
enum PatchCmd {
    Validate {
        #[command(flatten)]
        pair: Pair,
        #[arg(long)]
        patch: String,
        #[command(flatten)]
        common: Common,
    },
}

#[derive(Args, Clone)]
struct MigrationInputs {
    #[arg(long)]
    source_bios: PathBuf,
    #[arg(long)]
    source_profile: PathBuf,
    #[arg(long)]
    target_bios: PathBuf,
    #[arg(long)]
    target_profile: PathBuf,
}

#[derive(Subcommand)]
enum MigrateCmd {
    Plan {
        #[command(flatten)]
        inputs: MigrationInputs,
        /// Setting groups to carry over (e.g. `cpu`, `cpu.ratios`, `platform`, `all`).
        #[arg(long = "select-group")]
        groups: Vec<String>,
        /// Individual setting IDs to carry over.
        #[arg(long = "select-setting")]
        settings: Vec<String>,
        /// Previous plan whose selection overrides and resolutions should be kept.
        #[arg(long)]
        from_plan: Option<String>,
        #[command(flatten)]
        common: Common,
    },
    Validate {
        #[command(flatten)]
        inputs: MigrationInputs,
        #[arg(long)]
        plan: String,
        #[command(flatten)]
        common: Common,
    },
    Build {
        #[command(flatten)]
        inputs: MigrationInputs,
        #[arg(long)]
        plan: String,
        #[arg(long = "out")]
        output: PathBuf,
        #[arg(long)]
        report: PathBuf,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
}

#[derive(Subcommand)]
enum SchemaCmd {
    Print {
        #[arg(long)]
        document: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        payload: Option<String>,
        #[arg(long)]
        payload_version: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Some(p) = &cli.approvals {
        ocb_core::approvals::install_from(p);
    }
    if let Some(p) = &cli.settings {
        ocb_core::settings::install_from(p);
    }
    if let Some(e) = &ocb_core::settings::active().error {
        eprintln!("warning: settings file rejected, writing stays off: {e}");
    }
    if let Some(e) = &ocb_core::approvals::active().error {
        eprintln!("warning: approvals file rejected, nothing is approved: {e}");
    }
    let json_errors = wants_json(&cli.cmd);
    match run(cli.cmd) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            if json_errors {
                let env = serde_json::json!({
                    "kind": "ocb.error", "code": e.code(), "exit_code": e.exit_code(), "message": e.to_string()
                });
                println!("{}", serde_json::to_string_pretty(&env).unwrap_or_default());
            } else {
                eprintln!("error: {e}");
            }
            std::process::exit(e.exit_code());
        }
    }
}

fn wants_json(cmd: &Cmd) -> bool {
    let f = match cmd {
        Cmd::Bios {
            cmd: BiosCmd::Inspect { common, .. } | BiosCmd::Browse { common, .. },
        } => common.format,
        Cmd::Profile { cmd } => match cmd {
            ProfileCmd::Inspect { common, .. }
            | ProfileCmd::Diff { common, .. }
            | ProfileCmd::Verify { common, .. } => common.format,
            ProfileCmd::Build { format, .. } => *format,
        },
        Cmd::Patch {
            cmd: PatchCmd::Validate { common, .. },
        } => common.format,
        Cmd::Migrate { cmd } => match cmd {
            MigrateCmd::Plan { common, .. } | MigrateCmd::Validate { common, .. } => common.format,
            MigrateCmd::Build { format, .. } => *format,
        },
        Cmd::Capabilities { format } => *format,
        Cmd::Schema { .. } => Format::Json,
    };
    f == Format::Json
}

fn progress(msg: &str) {
    eprintln!("{msg}");
}

fn load_session(bios: &Path, profile: &Path) -> Result<Session> {
    progress(&format!("parsing {}", bios.display()));
    let schema = Arc::new(io::load_bios(bios)?);
    let bytes = io::load_profile(profile)?;
    Ok(Session::pair(schema, bytes)?.with_paths(
        Some(bios.display().to_string()),
        Some(profile.display().to_string()),
    ))
}

fn read_json_input(arg: &str) -> Result<String> {
    const LIMIT: usize = 4 << 20;
    if arg == "-" {
        let mut s = String::new();
        std::io::stdin()
            .take(LIMIT as u64 + 1)
            .read_to_string(&mut s)
            .map_err(|e| Error::Io {
                path: "stdin".into(),
                message: e.to_string(),
            })?;
        if s.len() > LIMIT {
            return Err(Error::Limit("stdin JSON exceeds 4 MiB".into()));
        }
        Ok(s)
    } else {
        let b = io::read_bounded(Path::new(arg), LIMIT)?;
        String::from_utf8(b).map_err(|_| Error::Request(format!("{arg} is not valid UTF-8")))
    }
}

fn parse_patch(text: &str) -> Result<Patch> {
    // Reject NaN/Infinity and duplicate keys: serde_json rejects the former; derived structs reject duplicate fields.
    serde_json::from_str(text).map_err(|e| Error::Request(format!("patch: {e}")))
}

fn emit<T: serde::Serialize>(
    doc: &T,
    common: &Common,
    inputs: &[&Path],
    text: impl FnOnce() -> String,
) -> Result<()> {
    let json = serde_json::to_string_pretty(doc).map_err(|e| Error::Internal(e.to_string()))?;
    if let Some(out) = &common.out {
        io::ensure_not_input(out, inputs)?;
        io::publish_exclusive(out, format!("{json}\n").as_bytes())?;
        if !common.quiet {
            eprintln!("wrote {}", out.display());
        }
        if common.format == Format::Text && !common.quiet {
            eprintln!("{}", text());
        }
    } else if common.format == Format::Json {
        println!("{json}");
    } else if !common.quiet {
        println!("{}", text());
    }
    Ok(())
}

fn run(cmd: Cmd) -> Result<i32> {
    match cmd {
        Cmd::Bios {
            cmd: BiosCmd::Inspect { bios, common },
        } => {
            progress(&format!("parsing {}", bios.display()));
            let schema = io::load_bios(&bios)?;
            let report = inspect::bios_catalog(&schema, Some(bios.display().to_string()));
            let b = &report.payload.bios;
            emit(&report, &common, &[&bios], || {
                format!(
                    "{}  board {}  flavor {}  revision {}  built {}\nsha256 {}\nsetup module {}\n{} questions, {} named Setup/CpuSetup questions\napproved for writing: {}",
                    b.bios_id,
                    b.board,
                    b.flavor,
                    b.revision,
                    b.build_date,
                    b.sha256,
                    b.setup_module,
                    b.question_count,
                    b.decoded_question_count,
                    b.approved_for_writing
                )
            })?;
            Ok(0)
        }
        Cmd::Bios {
            cmd:
                BiosCmd::Browse {
                    bios,
                    search,
                    hidden,
                    set,
                    common,
                },
        } => {
            progress(&format!("parsing {}", bios.display()));
            let a = io::load_analysis(&bios)?;
            let mut report = browse::browse(
                &a.image,
                a.schema.as_ref().ok(),
                Some(bios.display().to_string()),
                usize::from(set - 1),
            );
            let q = search.map(|s| s.to_lowercase());
            report.payload.fields.retain(|f| {
                (hidden || !f.hidden)
                    && q.as_ref().is_none_or(|q| {
                        format!("{} {}", f.prompt, f.menu_path.join(" "))
                            .to_lowercase()
                            .contains(q)
                    })
            });
            emit(&report, &common, &[&bios], || browse_text(&report))?;
            Ok(0)
        }
        Cmd::Profile { cmd } => profile_cmd(cmd),
        Cmd::Patch {
            cmd:
                PatchCmd::Validate {
                    pair,
                    patch,
                    common,
                },
        } => {
            let s = load_session(&pair.bios, &pair.profile)?;
            let mut p = parse_patch(&read_json_input(&patch)?)?;
            let writes = edit::plan_edit(&s, &p)?;
            edit::apply_and_verify(&s, &writes)?;
            p.diagnostics = vec![contracts::Diagnostic::new(
                "patch_valid",
                contracts::Severity::Info,
                "validation",
                format!(
                    "{} operation(s) validate and build in a dry run",
                    writes.len()
                ),
            )];
            emit(&p, &common, &[&pair.bios, &pair.profile], || {
                let mut t = String::from("patch is valid:\n");
                for w in &writes {
                    t += &format!(
                        "  {}: {} -> {}\n",
                        w.label,
                        w.from.as_ref().map(Value::display).unwrap_or_default(),
                        w.to.display()
                    );
                }
                t
            })?;
            Ok(0)
        }
        Cmd::Migrate { cmd } => migrate_cmd(cmd),
        Cmd::Capabilities { .. } => {
            let caps = serde_json::json!({
                "adapter": {"id": ocb_core::catalog::ADAPTER_ID, "version": ocb_core::catalog::ADAPTER_VERSION,
                            "recipe_set_version": ocb_core::catalog::RECIPE_SET_VERSION},
                "writing": {
                    "enabled": ocb_core::settings::writing_enabled(),
                    "settings_file": ocb_core::settings::active().path,
                    "error": ocb_core::settings::active().error,
                    "note": "Experimental. Needs \"experimental\": {\"writing\": true} in the settings file and an approved image.",
                },
                "approvals": {
                    "summary": ocb_core::approvals::active().describe(),
                    "source": ocb_core::approvals::active().source,
                    "error": ocb_core::approvals::active().error,
                    "adapter_boards": ocb_core::catalog::ADAPTER_BOARDS,
                    "images": ocb_core::approvals::active().entries.iter().map(|a| serde_json::json!({
                        "bios_id": a.bios_id, "sha256": a.sha256, "adapter": a.adapter, "note": a.note})).collect::<Vec<_>>(),
                },
                "documents": contracts::support_table(),
                "capabilities": {"metadata_readable": true, "settings_inspectable": true, "changes_plannable": true,
                    "container_writer": "fixture_consistent for approved images (additive checksum, metadata preserved)",
                    "import_acceptance": "not_demonstrated"},
                "recipes": ocb_core::catalog::RECIPES.iter().map(|r| serde_json::json!({"id": r.id, "group": r.group, "label": r.label,
                    "writability": format!("{:?}", r.writability)})).collect::<Vec<_>>(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&caps).unwrap_or_default()
            );
            Ok(0)
        }
        Cmd::Schema {
            cmd:
                SchemaCmd::Print {
                    document,
                    version,
                    payload,
                    payload_version,
                },
        } => {
            if version != contracts::SCHEMA_VERSION
                || payload_version
                    .as_deref()
                    .is_some_and(|v| v != contracts::PAYLOAD_VERSION)
            {
                return Err(Error::Request(
                    "unsupported version; see `ocbi capabilities`".into(),
                ));
            }
            let schema = match (document.as_str(), payload.as_deref()) {
                ("ocb.patch", _) => schemars::schema_for!(Patch),
                ("ocb.export-manifest", _) => schemars::schema_for!(contracts::ExportManifest),
                ("ocb.report", Some("inspect") | None) => {
                    schemars::schema_for!(contracts::Report<contracts::InspectPayload>)
                }
                ("ocb.report", Some("diff")) => {
                    schemars::schema_for!(contracts::Report<contracts::DiffPayload>)
                }
                ("ocb.report", Some("verify")) => {
                    schemars::schema_for!(contracts::Report<contracts::VerifyPayload>)
                }
                ("ocb.report", Some("bios_catalog")) => {
                    schemars::schema_for!(contracts::Report<contracts::CatalogPayload>)
                }
                _ => {
                    return Err(Error::Request(format!(
                        "unknown document/payload {document} {payload:?}"
                    )));
                }
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&schema).unwrap_or_default()
            );
            Ok(0)
        }
    }
}

fn browse_text(r: &contracts::Report<contracts::BrowsePayload>) -> String {
    use std::fmt::Write as _;
    let p = &r.payload;
    let b = &p.bios;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{}  built {}  sha256 {}",
        b.bios_id,
        b.build_date,
        &b.sha256[..12]
    );
    if let Some(pl) = &b.platform {
        let _ = writeln!(
            s,
            "{} {}  {}  {}  parser {}",
            pl.family,
            pl.chipset.as_deref().unwrap_or(""),
            pl.models.as_deref().unwrap_or(""),
            pl.version,
            pl.parser
        );
        let _ = writeln!(s, "profiles: {}", pl.profiles_note);
    }
    let _ = writeln!(s, "defaults: {}", b.defaults.source);
    for (i, set) in p.sets.iter().enumerate() {
        let mark = if i == p.set { " (shown)" } else { "" };
        let _ = writeln!(
            s,
            "firmware set {}{mark}: {}. {}",
            i + 1,
            set.label,
            set.note
        );
    }
    for f in p
        .formsets
        .iter()
        .filter(|f| f.note.is_some() || f.placement == "title_match")
    {
        let mut what = Vec::new();
        if f.placement == "title_match" {
            what.push(format!(
                "placed under {} by matching page title (not linked by the BIOS)",
                f.placed_under.join(" > ")
            ));
        }
        what.extend(f.note.clone());
        let _ = writeln!(s, "note: {}: {}", f.title, what.join(". "));
    }
    let mut fields: Vec<&contracts::BiosField> = p.fields.iter().collect();
    // Pages in menu order: a page sorts by its first field's order key.
    let mut first: BTreeMap<Vec<String>, Vec<u32>> = BTreeMap::new();
    for f in &fields {
        let e = first
            .entry(f.menu_path.clone())
            .or_insert_with(|| f.menu_order.clone());
        if f.menu_order < *e {
            *e = f.menu_order.clone();
        }
    }
    fields.sort_by_cached_key(|f| (first.get(&f.menu_path).cloned(), f.menu_order.clone()));
    let mut page: Option<&Vec<String>> = None;
    let mut section: Option<&Option<String>> = None;
    for f in &fields {
        if page != Some(&f.menu_path) {
            let title = if f.menu_path.is_empty() {
                "Setup".to_string()
            } else {
                f.menu_path.join(" > ")
            };
            let _ = writeln!(
                s,
                "
{title}"
            );
            page = Some(&f.menu_path);
            section = None;
        }
        if section != Some(&f.section) {
            if let Some(h) = &f.section {
                let _ = writeln!(s, "  -- {h}");
            }
            section = Some(&f.section);
        }
        let def = match (&f.default_label, f.default_raw) {
            (Some(l), _) => l.clone(),
            (None, Some(v)) => v.to_string(),
            (None, None) => "-".into(),
        };
        let _ = writeln!(
            s,
            "  {:<48} {:<22} {}+0x{:x}{}",
            f.prompt,
            def,
            f.store,
            f.offset,
            if f.hidden { "  (hidden)" } else { "" }
        );
    }
    let _ = writeln!(
        s,
        "
{} settings",
        p.fields.len()
    );
    s
}

fn settings_text(r: &contracts::Report<contracts::InspectPayload>) -> String {
    let p = &r.payload;
    let mut t = format!(
        "{} ↔ {}  (match verified)\nprofile saved {}  CPU {}\nchecksum {} ({})  writer: {}\n\n",
        p.bios.bios_id,
        p.profile.path.clone().unwrap_or_default(),
        p.profile.saved_at,
        p.profile.cpu_brand,
        p.profile.checksum_stored,
        if p.profile.checksum_valid {
            "valid"
        } else {
            "INVALID"
        },
        if p.capabilities.image_approved_for_writing {
            "available"
        } else {
            "inspect-only"
        }
    );
    t += &if p.bios.defaults.available {
        format!(
            "* = differs from optimized defaults ({})\n\n",
            p.bios.defaults.source
        )
    } else {
        format!("defaults: {}\n\n", p.bios.defaults.source)
    };
    // Group by BIOS menu page in menu order, with the page's section headings.
    let mut rows: Vec<&contracts::DecodedSetting> =
        p.settings.iter().filter(|s| s.menu.is_some()).collect();
    // Pages in order of their first field; fields in menu order within a page.
    let page_key = |path: &Vec<String>| {
        p.settings
            .iter()
            .filter_map(|s| s.menu.as_ref())
            .filter(|m| &m.path == path)
            .map(|m| m.order.clone())
            .min()
    };
    rows.sort_by_cached_key(|s| {
        s.menu
            .as_ref()
            .map(|m| (page_key(&m.path), m.order.clone()))
    });
    let (mut page, mut section): (Option<&Vec<String>>, Option<&String>) = (None, None);
    for s in rows {
        let Some(m) = &s.menu else { continue };
        if page != Some(&m.path) {
            t += &format!("{}\n", m.path.join(" › "));
            page = Some(&m.path);
            section = None;
        }
        if let Some(sec) = m.section.as_ref().filter(|x| Some(*x) != section) {
            t += &format!("  {sec}\n");
            section = Some(sec);
        }
        let v = s.value.as_ref().map(Value::display).unwrap_or_default();
        let act = match s.activity {
            contracts::Activity::Inactive => "  (inactive)",
            _ => "",
        };
        let (mark, def) = match (s.differs_from_default, &s.default_value) {
            (Some(true), Some(d)) => ("*", format!("  [default {}]", d.display())),
            _ => (" ", String::new()),
        };
        t += &format!("  {mark} {:<36} {v}{act}{def}\n", m.prompt);
    }
    let missing: Vec<&str> = p
        .settings
        .iter()
        .filter(|s| !s.available)
        .map(|s| s.label.as_str())
        .collect();
    if !missing.is_empty() {
        t += &format!("Not in this BIOS: {}\n", missing.join(", "));
    }
    if !p.raw_fields.is_empty() {
        let n = p
            .raw_fields
            .iter()
            .filter(|f| f.default_raw.is_some_and(|d| d != f.raw))
            .count();
        t += &format!(
            "\n{n} of {} named fields differ from optimized defaults (the JSON output lists them)\n",
            p.raw_fields.len()
        );
    }
    t
}

fn profile_cmd(cmd: ProfileCmd) -> Result<i32> {
    match cmd {
        ProfileCmd::Inspect {
            pair,
            all_fields,
            common,
        } => {
            let s = load_session(&pair.bios, &pair.profile)?;
            let r = inspect::inspect(&s, all_fields);
            emit(&r, &common, &[&pair.bios, &pair.profile], || {
                settings_text(&r)
            })?;
            Ok(0)
        }
        ProfileCmd::Diff {
            all_fields,
            left_bios,
            left_profile,
            right_bios,
            right_profile,
            common,
        } => {
            let l = load_session(&left_bios, &left_profile)?;
            let r = load_session(&right_bios, &right_profile)?;
            let d = inspect::diff_with(&l, &r, all_fields);
            emit(
                &d,
                &common,
                &[&left_bios, &left_profile, &right_bios, &right_profile],
                || {
                    let mut t = String::new();
                    for s in d.payload.settings.iter().filter(|s| !s.equal) {
                        let f = |v: &Option<Value>| {
                            v.as_ref().map(Value::display).unwrap_or("—".into())
                        };
                        t += &format!("  {:<34} {:>24}  →  {}\n", s.label, f(&s.left), f(&s.right));
                    }
                    if t.is_empty() {
                        t = "  reviewed settings are identical\n".into();
                    }
                    format!(
                        "{t}{} other named field(s) differ; {} byte span(s) differ",
                        d.payload.other_field_differences.len(),
                        d.payload.byte_spans.len()
                    )
                },
            )?;
            Ok(0)
        }
        ProfileCmd::Verify { pair, common } => {
            let s = load_session(&pair.bios, &pair.profile)?;
            let r = verify::verify_profile(&s);
            let ok = r.payload.status == "valid";
            emit(&r, &common, &[&pair.bios, &pair.profile], || {
                checks_text(&r.payload.checks, &r.payload.status)
            })?;
            Ok(if ok { 0 } else { 8 })
        }
        ProfileCmd::Build {
            bios,
            base_profile,
            patch,
            output,
            report,
            format,
        } => {
            let s = load_session(&bios, &base_profile)?;
            let p = parse_patch(&read_json_input(&patch)?)?;
            let name = output
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let b = edit::build_edit(&s, &p, &name)?;
            publish(&b, &output, &report, &[&bios, &base_profile], format)
        }
    }
}

fn checks_text(checks: &[contracts::Check], status: &str) -> String {
    let mut t = String::new();
    for c in checks {
        let mark = match c.status {
            contracts::CheckStatus::Pass => "pass",
            contracts::CheckStatus::Fail => "FAIL",
            contracts::CheckStatus::NotEvaluated => "n/a ",
        };
        t += &format!("  [{mark}] {:<26} {}\n", c.name, c.detail);
    }
    format!("{t}result: {status}")
}

fn publish(
    b: &edit::BuildOutput,
    output: &Path,
    report: &Path,
    inputs: &[&Path],
    format: Format,
) -> Result<i32> {
    let manifest =
        serde_json::to_string_pretty(&b.manifest).map_err(|e| Error::Internal(e.to_string()))?;
    io::publish_build(
        output,
        &b.bytes,
        report,
        format!("{manifest}\n").as_bytes(),
        inputs,
    )?;
    if format == Format::Json {
        println!("{manifest}");
    } else {
        println!(
            "wrote {} ({} bytes, sha256 {})",
            output.display(),
            b.bytes.len(),
            b.manifest.output.sha256
        );
        println!("wrote {}", report.display());
        for c in &b.manifest.changes {
            println!(
                "  {:?} {}: {} -> {}",
                c.action,
                c.label,
                c.from.as_ref().map(Value::display).unwrap_or_default(),
                c.to.as_ref().map(Value::display).unwrap_or_default()
            );
        }
        println!(
            "{}",
            checks_text(&b.manifest.checks, &b.manifest.validation)
        );
    }
    Ok(0)
}

fn migration_sessions(i: &MigrationInputs) -> Result<(Session, Session)> {
    Ok((
        load_session(&i.source_bios, &i.source_profile)?,
        load_session(&i.target_bios, &i.target_profile)?,
    ))
}

fn plan_text(p: &Patch) -> String {
    let Some(m) = &p.migration else {
        return String::new();
    };
    let mut t = String::new();
    let f = |v: &Option<Value>| v.as_ref().map(Value::display).unwrap_or("—".into());
    for e in &m.entries {
        let action = e.final_action.map(|a| format!("{a:?}")).unwrap_or_default();
        let reason = e
            .assessment
            .as_ref()
            .and_then(|a| a.reason)
            .map(|r| format!("  [{r:?}]"))
            .unwrap_or_default();
        t += &format!(
            "  {:<14} {:<34} {:>22} → {:<22}{}\n",
            action,
            e.label,
            f(&e.source_value),
            f(&e.proposed_value),
            reason
        );
    }
    for o in &m.target_only {
        t += &format!(
            "  {:<14} {:<34} {}\n",
            "TargetOnly",
            o.label,
            f(&o.retained_value)
        );
    }
    format!(
        "{t}selected {}, unselected {}, blocked {}; {} other differing field(s) keep target values\ncan build: {}",
        m.accounting.selected,
        m.accounting.unselected,
        m.accounting.blocked,
        m.accounting.other_differing_fields,
        m.can_build
    )
}

fn migrate_cmd(cmd: MigrateCmd) -> Result<i32> {
    match cmd {
        MigrateCmd::Plan {
            inputs,
            groups,
            settings,
            from_plan,
            common,
        } => {
            let (src, tgt) = migration_sessions(&inputs)?;
            let prior = from_plan
                .map(|p| read_json_input(&p).and_then(|t| parse_patch(&t)))
                .transpose()?;
            let selection = if groups.is_empty() && settings.is_empty() {
                prior
                    .as_ref()
                    .and_then(|p| p.migration.as_ref())
                    .map(|m| m.selection.clone())
                    .unwrap_or(Selection {
                        groups: vec!["cpu".into()],
                        settings: vec![],
                    })
            } else {
                Selection { groups, settings }
            };
            let (plan, _) = migrate::plan(&src, &tgt, selection, prior.as_ref())?;
            let blocked = plan
                .migration
                .as_ref()
                .is_some_and(|m| m.accounting.blocked > 0);
            let all = [
                &inputs.source_bios,
                &inputs.source_profile,
                &inputs.target_bios,
                &inputs.target_profile,
            ];
            emit(&plan, &common, &all.map(|p| p.as_path()), || {
                plan_text(&plan)
            })?;
            Ok(if blocked { 7 } else { 0 })
        }
        MigrateCmd::Validate {
            inputs,
            plan,
            common,
        } => {
            let (src, tgt) = migration_sessions(&inputs)?;
            let supplied = parse_patch(&read_json_input(&plan)?)?;
            edit::check_header(&supplied, contracts::PatchMode::Migrate)?;
            let sel = supplied
                .migration
                .as_ref()
                .map(|m| m.selection.clone())
                .unwrap_or_default();
            for (b, s, role) in [
                (&supplied.source, &src, "source"),
                (&supplied.target, &tgt, "target"),
            ] {
                if b.as_ref() != Some(&s.binding()) {
                    return Err(Error::BindingChanged(format!(
                        "{role} inputs differ from the plan's binding"
                    )));
                }
            }
            let (annotated, _) = migrate::plan(&src, &tgt, sel, Some(&supplied))?;
            let m = annotated.migration.as_ref();
            let code = if m.is_some_and(|m| m.accounting.blocked > 0) {
                7
            } else if m.is_some_and(|m| !m.can_build) {
                5
            } else {
                0
            };
            let all = [
                &inputs.source_bios,
                &inputs.source_profile,
                &inputs.target_bios,
                &inputs.target_profile,
            ];
            emit(&annotated, &common, &all.map(|p| p.as_path()), || {
                plan_text(&annotated)
            })?;
            Ok(code)
        }
        MigrateCmd::Build {
            inputs,
            plan,
            output,
            report,
            format,
        } => {
            let (src, tgt) = migration_sessions(&inputs)?;
            let supplied = parse_patch(&read_json_input(&plan)?)?;
            let name = output
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let b = migrate::build(&src, &tgt, &supplied, &name)?;
            let all = [
                &inputs.source_bios,
                &inputs.source_profile,
                &inputs.target_bios,
                &inputs.target_profile,
            ];
            publish(&b, &output, &report, &all.map(|p| p.as_path()), format)
        }
    }
}
