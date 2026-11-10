//! `ocbi` — headless CLI for OCB Inspector (SPEC §10).
//!
//! JSON goes to stdout or `--out`; progress and human messages go to stderr.

use clap::{Args, Parser, Subcommand, ValueEnum};
use ocb_core::contracts::{self, Patch, Selection, Value};
use ocb_core::engine::{Session, browse, edit, experimental, inspect, migrate, verify};
use ocb_core::error::{Error, Result};
use ocb_core::io;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `println!` that ends the process quietly when stdout is closed (`ocbi ... | head`).
macro_rules! out {
    ($($t:tt)*) => {
        out_line(&format!($($t)*))
    };
}

fn out_line(s: &str) {
    write_stdout(&[s.as_bytes(), b"\n"]);
}

/// Write the parts to stdout under one lock; a failure goes through `stdout_failed`.
fn write_stdout(parts: &[&[u8]]) {
    use std::io::Write as _;
    let mut o = std::io::stdout().lock();
    if let Err(e) = parts.iter().try_for_each(|p| o.write_all(p)) {
        stdout_failed(&e);
    }
}

/// A closed stdout is a normal end for a reader that wanted less; any other failure keeps the
/// behaviour of `println!`.
fn stdout_failed(e: &std::io::Error) -> ! {
    if e.kind() == std::io::ErrorKind::BrokenPipe {
        std::process::exit(0);
    }
    panic!("failed printing to stdout: {e}");
}

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
    /// What this BIOS says about the OC profiles it writes (no profile needed): the setup
    /// browser's variable list (record id = position), the profile code and the file layout.
    /// Read-only; parts not traced for this release are labelled as expected.
    ProfileFormat {
        #[arg(long)]
        bios: PathBuf,
        /// Firmware set, for images holding several (32 MiB AM4); 1 is the default.
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
    /// Experimental, read-only analysis of any profile: file structure, and with the BIOS that
    /// saved it, settings read through that BIOS's own definitions (inferred mappings marked).
    /// Exit 0 means the analysis ran, not that the profile is valid. Never writes a profile.
    Analyze {
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        bios: Option<PathBuf>,
        /// Firmware set of the BIOS image (1-based), for images holding several.
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
        set: Option<u16>,
        /// A profile saved right after "Load Optimized Defaults" on the same BIOS: fields are
        /// compared with it (needs --bios). A difference means the two files store different bytes.
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// The BIOS image the baseline was saved on, when it is another release of the same board
        /// (a profile saved after an update, at defaults). Settings then match by menu place and
        /// name, and a difference can also be a default that changed between releases.
        #[arg(long)]
        baseline_bios: Option<PathBuf>,
        /// Firmware set of the baseline's BIOS image (1-based).
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
        baseline_set: Option<u16>,
        /// Omit the CPU brand, save time and raw byte previews, for posting in public.
        #[arg(long)]
        shareable: bool,
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
                out!("{}", serde_json::to_string_pretty(&env).unwrap_or_default());
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
            cmd:
                BiosCmd::Inspect { common, .. }
                | BiosCmd::Browse { common, .. }
                | BiosCmd::ProfileFormat { common, .. },
        } => common.format,
        Cmd::Profile { cmd } => match cmd {
            ProfileCmd::Inspect { common, .. }
            | ProfileCmd::Diff { common, .. }
            | ProfileCmd::Verify { common, .. }
            | ProfileCmd::Analyze { common, .. } => common.format,
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
    emit_json(format!("{json}\n").into_bytes(), common, inputs, text)
}

/// Emit an already serialized document (ends with a newline).
fn emit_json(
    json: Vec<u8>,
    common: &Common,
    inputs: &[&Path],
    text: impl FnOnce() -> String,
) -> Result<()> {
    if let Some(out) = &common.out {
        io::ensure_not_input(out, inputs)?;
        io::publish_exclusive(out, &json)?;
        if !common.quiet {
            eprintln!("wrote {}", out.display());
        }
        if common.format == Format::Text && !common.quiet {
            eprintln!("{}", text());
        }
    } else if common.format == Format::Json {
        write_stdout(&[&json]);
    } else if !common.quiet {
        out!("{}", text());
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
        Cmd::Bios {
            cmd: BiosCmd::ProfileFormat { bios, set, common },
        } => {
            progress(&format!("parsing {}", bios.display()));
            let a = io::load_analysis(&bios)?;
            let b = bios_of(&a);
            let report = experimental::profile_format(
                b.image,
                b.profiles_supported,
                Some(usize::from(set - 1)),
            )?;
            emit(&report, &common, &[&bios], || {
                profile_format_text(&report.payload)
            })?;
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
            out!(
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
                ("ocb.report", Some("experimental_inspect")) => {
                    schemars::schema_for!(contracts::Report<contracts::ExperimentalInspectPayload>)
                }
                ("ocb.report", Some("profile_format")) => {
                    schemars::schema_for!(contracts::Report<contracts::ProfileFormatPayload>)
                }
                _ => {
                    return Err(Error::Request(format!(
                        "unknown document/payload {document} {payload:?}"
                    )));
                }
            };
            out!(
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
    let note = untested_line(p.bios.platform.as_ref().and_then(|pl| pl.profile_evidence));
    let mut t = format!(
        "{} ↔ {}  (match verified)\n{note}profile saved {}  CPU {}\nchecksum {} ({})  writer: {}\n\n",
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
    t += &other_settings_text(&p.bios_settings);
    t
}

/// Lines printed per grouped settings list before the rest is left to the JSON output.
const LIST_LINES: usize = 200;

/// One row of a grouped list: its menu page, its menu order, a tie-breaker for equal orders, and
/// what the line is made from.
type ListRow<'a, T> = (&'a [String], &'a [u32], usize, T);

/// Print `rows` grouped by menu page: pages in the menu order of their first row, rows in menu
/// order within a page, a `heading` line when the page changes, at most `LIST_LINES` rows, then
/// how many more were left out.
fn grouped_lines<T>(
    t: &mut String,
    mut rows: Vec<ListRow<'_, T>>,
    heading: impl Fn(&[String]) -> String,
    line: impl Fn(&T) -> String,
) {
    use std::fmt::Write as _;
    let mut first: BTreeMap<&[String], &[u32]> = BTreeMap::new();
    for (path, order, _, _) in &rows {
        let e = first.entry(path).or_insert(order);
        *e = (*e).min(order);
    }
    rows.sort_by(|a, b| (first[a.0], a.0, a.1, a.2).cmp(&(first[b.0], b.0, b.1, b.2)));
    let mut page: Option<&[String]> = None;
    for (path, _, _, item) in rows.iter().take(LIST_LINES) {
        if page != Some(*path) {
            let _ = writeln!(t, "{}", heading(path));
            page = Some(*path);
        }
        let _ = writeln!(t, "{}", line(item));
    }
    if rows.len() > LIST_LINES {
        let _ = writeln!(t, "  {} more: use --format json", rows.len() - LIST_LINES);
    }
}

/// The `bios_settings` that differ from optimized defaults, by menu page. Empty when the list is
/// empty (the flag was not given).
fn other_settings_text(all: &[contracts::DecodedSetting]) -> String {
    if all.is_empty() {
        return String::new();
    }
    let rows: Vec<ListRow<'_, (&contracts::DecodedSetting, &contracts::MenuLocation)>> = all
        .iter()
        .filter(|s| s.differs_from_default == Some(true))
        .filter_map(|s| {
            s.menu
                .as_ref()
                .map(|m| (m.path.as_slice(), m.order.as_slice(), 0, (s, m)))
        })
        .collect();
    let mut t = format!(
        "\nOther BIOS settings that differ from optimized defaults ({} of {})\nSome of these are values the firmware fills in itself, not choices made in the BIOS menus.\n",
        rows.len(),
        all.len()
    );
    grouped_lines(
        &mut t,
        rows,
        |path| {
            if path.is_empty() {
                "Setup".to_string()
            } else {
                path.join(" › ")
            }
        },
        |(s, m)| {
            let v = s.value.as_ref().map(Value::display).unwrap_or_default();
            let v = v.trim_end();
            let def = s
                .default_value
                .as_ref()
                .map(|d| format!("  [default {}]", d.display().trim_end()))
                .unwrap_or_default();
            format!("  * {:<36} {v}{def}", m.prompt)
        },
    );
    t
}

/// Text form of an experimental analysis: status first, then the findings.
fn profile_format_text(p: &contracts::ProfileFormatPayload) -> String {
    use std::fmt::Write as _;
    let mut t = format!(
        "PROFILE FORMAT OF {} (read-only; {})
",
        p.bios.bios_id,
        if p.traced {
            "this release's profile code was traced"
        } else {
            "this release's profile code was not traced"
        }
    );
    for f in &p.layout {
        let _ = writeln!(t, "{:<12} [{}] {}", f.part, f.basis, f.detail);
    }
    for m in &p.code {
        let _ = writeln!(
            t,
            "code         {} {}{}",
            m.name,
            if m.holds_mos { "$MOS$ " } else { "" },
            if m.holds_oci { "$OCI$" } else { "" }
        );
    }
    match (&p.variable_list, &p.list_problem) {
        (Some(l), _) => {
            let _ = writeln!(
                t,
                "variables    {} (list sha256 {})",
                l.entries.len(),
                &l.sha256[..16]
            );
            for e in &l.entries {
                let store = e.store_size.map_or("-".to_string(), |s| s.to_string());
                let _ = writeln!(
                    t,
                    "  {:>4}  {:<32} declared {:>5}  store {:>5}  {:>4} fields  {}{}",
                    e.index,
                    e.name,
                    e.declared_size,
                    store,
                    e.field_count,
                    e.saved,
                    e.note
                        .as_deref()
                        .map(|n| format!(" ({n})"))
                        .unwrap_or_default()
                );
            }
        }
        (None, problem) => {
            let _ = writeln!(
                t,
                "variables    none: {}",
                problem.as_deref().unwrap_or("-")
            );
        }
    }
    for l in &p.limitations {
        let _ = writeln!(t, "- {l}");
    }
    t
}

fn analysis_text(p: &contracts::ExperimentalInspectPayload) -> String {
    use std::fmt::Write as _;
    let mut t = String::from(
        "EXPERIMENTAL, READ-ONLY INTERPRETATION. Some record mappings are inferred.\n",
    );
    let c = &p.coverage;
    let _ = writeln!(
        t,
        "container {}{}",
        p.container,
        p.selected_candidate
            .as_ref()
            .map(|s| format!(" ({s})"))
            .unwrap_or_default()
    );
    let _ = writeln!(t, "identity  {}: {}", p.identity.status, p.identity.reason);
    let _ = writeln!(
        t,
        "integrity {} (stored {}, computed {})",
        p.integrity.status,
        p.integrity.stored.as_deref().unwrap_or("-"),
        p.integrity.computed.as_deref().unwrap_or("-")
    );
    if let Some(b) = &p.inputs.bios {
        let set = match (b.set, b.sets.len()) {
            (Some(s), n) if n > 1 => format!(", set {} of {n}", s + 1),
            (None, n) => format!(", {n} firmware sets: choose one with --set"),
            _ => String::new(),
        };
        let _ = writeln!(t, "bios      {} ({}){set}", b.bios_id, b.family);
    }
    let _ = writeln!(
        t,
        "records   {} total: {} confirmed, {} inferred, {} ambiguous, {} unmapped",
        c.records_total,
        c.records_confirmed,
        c.records_inferred,
        c.records_ambiguous,
        c.records_unmapped
    );
    let named = p.records.iter().filter(|r| r.variable.is_some()).count();
    if named > 0 {
        let _ = writeln!(
            t,
            "names     {named} records named by the BIOS's own variable list (firmware-derived)"
        );
    }
    let _ = writeln!(
        t,
        "fields    {} read: {} decoded, {} unknown values, {} unsupported, {} out of bounds",
        c.fields_total,
        c.fields_decoded,
        c.fields_unknown_value,
        c.fields_unsupported,
        c.fields_out_of_bounds
    );
    let _ = writeln!(
        t,
        "bytes     {} of {} interpreted; {} opaque",
        c.interpreted_bytes, c.profile_bytes, c.opaque_bytes
    );
    for r in &p.status_reasons {
        let _ = writeln!(t, "- {r}");
    }
    for s in p.stores.iter().filter(|s| s.mapped_record.is_some()) {
        let _ = writeln!(
            t,
            "  {:<24} {:>6} bytes  {:<9} {}",
            s.name,
            s.size,
            s.mapping,
            s.mapped_record.as_deref().unwrap_or("")
        );
    }
    if let Some(b) = &p.inputs.baseline {
        let ids = match (&b.bios, &p.inputs.bios) {
            (Some(o), Some(m)) if b.status == "applied" => {
                format!("profile on {}, baseline on {}; ", m.bios_id, o.bios_id)
            }
            _ => String::new(),
        };
        let _ = writeln!(t, "baseline  {}: {ids}{}", b.status, b.reason);
        if let (true, Some(n)) = (
            b.mode == "cross_version",
            c.fields_not_compared_with_baseline,
        ) {
            let _ = writeln!(
                t,
                "          {} fields compared, {} differ, {n} not compared",
                c.fields_compared_with_baseline.unwrap_or(0),
                c.fields_differing_from_baseline.unwrap_or(0)
            );
        }
    }
    let visible = |f: &&contracts::ExperimentalField| !f.hidden;
    if p.inputs
        .baseline
        .as_ref()
        .is_some_and(|b| b.status == "applied")
    {
        let base_value = |f: &contracts::ExperimentalField| {
            f.baseline_label
                .clone()
                .or_else(|| f.baseline_raw.as_ref().map(|r| r.dec.clone()))
        };
        let differing = |want: Option<bool>| -> Vec<_> {
            p.fields
                .iter()
                .filter(visible)
                .filter(|f| f.differs_from_baseline == Some(true) && f.baseline_off_default == want)
                .collect()
        };
        if let (Some(at), Some(off), Some(unknown)) = (
            c.baseline_differing_at_default,
            c.baseline_differing_off_default,
            c.baseline_differing_no_default,
        ) {
            let _ = writeln!(
                t,
                "Of the {} fields that differ from the baseline: {at} with the baseline at its BIOS default, {off} with the baseline off its default, {unknown} with no default to judge by.",
                at + off + unknown
            );
            let _ = writeln!(
                t,
                "A baseline value that is off its own BIOS default is often filled in by the firmware (fan type detection, link equalisation), so those are less likely your own choices. This only holds if the baseline was saved with the BIOS at its defaults."
            );
        }
        // The last list (no default to judge by) is left out when it is empty.
        let lists: [(&str, Option<bool>); 3] = [
            (
                "differ from baseline, baseline at its BIOS default (most likely settings made in the BIOS menus)",
                Some(false),
            ),
            (
                "differ from baseline, baseline itself off its BIOS default (often filled in by the firmware)",
                Some(true),
            ),
            (
                "differ from baseline, baseline has no BIOS default to judge by",
                None,
            ),
        ];
        for (title, want) in lists {
            let rows = differing(want);
            if want.is_none() && rows.is_empty() {
                continue;
            }
            field_list(&mut t, title, rows, "baseline", base_value);
        }
    }
    let rows: Vec<_> = p
        .fields
        .iter()
        .filter(visible)
        .filter(|f| f.differs_from_default == Some(true))
        .collect();
    field_list(
        &mut t,
        "differ from the BIOS optimized default",
        rows,
        "default",
        |f| {
            f.default_label
                .clone()
                .or_else(|| f.default_raw.as_ref().map(|r| r.dec.clone()))
        },
    );
    let judged = p
        .fields
        .iter()
        .filter(|f| matches!(f.state.as_str(), "decoded" | "unknown_value"))
        .filter(|f| f.default_raw.is_none())
        .count();
    let _ = writeln!(
        t,
        "{judged} interpreted fields have no optimized default and cannot be judged."
    );
    t.push_str("Use --format json for every field, record and piece of evidence.");
    t
}

/// Visible settings, grouped by menu path: label, stored value, and the reference value.
fn field_list(
    t: &mut String,
    title: &str,
    rows: Vec<&contracts::ExperimentalField>,
    reference: &str,
    other: impl Fn(&contracts::ExperimentalField) -> Option<String>,
) {
    use std::fmt::Write as _;
    let _ = writeln!(t, "{} visible settings {title}:", rows.len());
    let rows = rows
        .into_iter()
        .map(|f| {
            (
                f.menu_path.as_slice(),
                f.menu_order.as_slice(),
                usize::from(f.store_offset),
                f,
            )
        })
        .collect();
    grouped_lines(
        t,
        rows,
        |path| format!("  {}", path.join(" > ")),
        |f| {
            let value = f
                .value_label
                .clone()
                .or_else(|| f.raw.as_ref().map(|r| r.dec.clone()))
                .unwrap_or_else(|| "?".into());
            format!(
                "    {}: {value} ({reference}: {})",
                f.label,
                other(f).unwrap_or_else(|| "?".into())
            )
        },
    );
}

/// The caution line for a pair whose board's profiles were never examined, with its newline.
fn untested_line(evidence: Option<contracts::ProfileEvidence>) -> String {
    ocb_core::engine::session::untested_board_note(evidence)
        .map(|n| format!("{n}\n"))
        .unwrap_or_default()
}

/// `untested_line` for a session that has no report carrying the platform (a comparison).
fn untested_note(s: &Session) -> String {
    untested_line(ocb_core::engine::session::profile_evidence(
        &s.schema.platform,
        &s.schema.identity.board,
    ))
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
                    let mut t = untested_note(&l);
                    if r.schema.identity.board != l.schema.identity.board {
                        t += &untested_note(&r);
                    }
                    let head = t.len();
                    for s in d.payload.settings.iter().filter(|s| !s.equal) {
                        let f = |v: &Option<Value>| {
                            v.as_ref().map(Value::display).unwrap_or("—".into())
                        };
                        t += &format!("  {:<34} {:>24}  →  {}\n", s.label, f(&s.left), f(&s.right));
                    }
                    if t.len() == head {
                        t += "  reviewed settings are identical\n";
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
        ProfileCmd::Analyze {
            profile,
            bios,
            set,
            baseline,
            baseline_bios,
            baseline_set,
            shareable,
            common,
        } => {
            if set.is_some() && bios.is_none() {
                return Err(Error::Request("--set needs --bios".into()));
            }
            if baseline.is_some() && bios.is_none() {
                return Err(Error::Request("--baseline needs --bios".into()));
            }
            if (baseline_bios.is_some() || baseline_set.is_some()) && baseline.is_none() {
                return Err(Error::Request(
                    "--baseline-bios and --baseline-set need --baseline".into(),
                ));
            }
            let bytes = io::load_profile(&profile)?;
            let base_bytes = baseline.as_deref().map(io::load_profile).transpose()?;
            let analysis = match &bios {
                Some(b) => {
                    progress(&format!("parsing {}", b.display()));
                    Some(io::load_analysis(b)?)
                }
                None => None,
            };
            let base_analysis = match (&baseline_bios, &bios) {
                (Some(b), Some(m)) if b != m => {
                    progress(&format!("parsing {}", b.display()));
                    Some(io::load_analysis(b)?)
                }
                _ => None,
            };
            let bios_ref = analysis.as_ref().map(bios_of);
            // The same file given twice is one image (the engine also treats equal digests so).
            let base_ref = match (&base_analysis, &baseline_bios) {
                (Some(a), _) => Some(bios_of(a)),
                (None, Some(_)) => bios_ref,
                _ => None,
            };
            let opts = experimental::AnalyzeOptions {
                set: set.map(|s| usize::from(s) - 1),
                ..Default::default()
            };
            let source = base_bytes
                .as_deref()
                .map(|profile| experimental::BaselineSource {
                    profile,
                    bios: base_ref,
                    set: baseline_set.map(|s| usize::from(s) - 1),
                });
            let mut r = experimental::analyze_with_baseline_on(&bytes, bios_ref, source, &opts)?;
            if shareable {
                experimental::make_shareable(&mut r.payload);
            }
            let json = experimental::to_json_bounded(&r, opts.limits.max_json_bytes)?;
            let mut inputs: Vec<&Path> = vec![&profile];
            if let Some(b) = &bios {
                inputs.push(b);
            }
            if let Some(b) = &baseline {
                inputs.push(b);
            }
            if let Some(b) = &baseline_bios {
                inputs.push(b);
            }
            emit_json(json, &common, &inputs, || analysis_text(&r.payload))?;
            Ok(0)
        }
        ProfileCmd::Verify { pair, common } => {
            let s = load_session(&pair.bios, &pair.profile)?;
            let r = verify::verify_profile(&s);
            let ok = r.payload.status == "valid";
            emit(&r, &common, &[&pair.bios, &pair.profile], || {
                format!(
                    "{}{}",
                    untested_line(
                        r.payload
                            .bios
                            .platform
                            .as_ref()
                            .and_then(|pl| pl.profile_evidence)
                    ),
                    checks_text(&r.payload.checks, &r.payload.status)
                )
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

fn bios_of(a: &ocb_core::firmware::Analysis) -> experimental::BiosRef<'_> {
    experimental::BiosRef::new(&a.image, a.schema.as_ref().ok())
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
        out!("{manifest}");
    } else {
        out!(
            "wrote {} ({} bytes, sha256 {})",
            output.display(),
            b.bytes.len(),
            b.manifest.output.sha256
        );
        out!("wrote {}", report.display());
        for c in &b.manifest.changes {
            out!(
                "  {:?} {}: {} -> {}",
                c.action,
                c.label,
                c.from.as_ref().map(Value::display).unwrap_or_default(),
                c.to.as_ref().map(Value::display).unwrap_or_default()
            );
        }
        out!(
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
