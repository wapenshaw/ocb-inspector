//! OCB Inspector desktop shell. Commands are thin wrappers over `ocb_core::engine`, the same
//! services the CLI uses, so the UI and CLI make identical decisions (SPEC §1, §12).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ocb_core::bytes::sha256_hex;
use ocb_core::contracts::*;
use ocb_core::engine::{Session, browse, edit, inspect, migrate, verify};
use ocb_core::error::Error;
use ocb_core::firmware::{BiosImage, BiosSchema, Limits, analyze};
use ocb_core::format::MAX_PROFILE_BYTES;
use ocb_core::io;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Same shape as the CLI's JSON error envelope.
#[derive(Debug, Serialize)]
struct CmdError {
    code: String,
    exit_code: i32,
    message: String,
}

impl From<Error> for CmdError {
    fn from(e: Error) -> Self {
        CmdError {
            code: e.code().into(),
            exit_code: e.exit_code(),
            message: e.to_string(),
        }
    }
}

type R<T> = Result<T, CmdError>;

/// One parsed image: the browsable view, and the decode schema or why there is none.
struct Parsed {
    image: Arc<BiosImage>,
    schema: Result<Arc<BiosSchema>, Error>,
}

/// Parsed images keyed by digest, so each image parses once per run.
#[derive(Default)]
struct AppState {
    parsed: Mutex<HashMap<String, Arc<Parsed>>>,
}

impl AppState {
    fn parsed(&self, path: &str) -> Result<Arc<Parsed>, Error> {
        let limits = Limits::default();
        let bytes = io::read_bounded(Path::new(path), limits.max_bios_bytes)?;
        let sha = sha256_hex(&bytes);
        let lock = || {
            self.parsed
                .lock()
                .map_err(|_| Error::Internal("cache poisoned".into()))
        };
        if let Some(p) = lock()?.get(&sha) {
            return Ok(p.clone());
        }
        let a = analyze(&bytes, limits)?;
        let p = Arc::new(Parsed {
            image: Arc::new(a.image),
            schema: a.schema.map(Arc::new),
        });
        lock()?.insert(sha, p.clone());
        Ok(p)
    }

    fn schema(&self, path: &str) -> Result<Arc<BiosSchema>, Error> {
        self.parsed(path)?.schema.clone()
    }

    fn session(&self, bios: &str, profile: &str) -> Result<Session, Error> {
        let schema = self.schema(bios)?;
        let bytes = io::read_bounded(Path::new(profile), MAX_PROFILE_BYTES)?;
        Ok(Session::pair(schema, bytes)?.with_paths(Some(bios.into()), Some(profile.into())))
    }
}

#[derive(Serialize)]
struct BuildResult {
    output_path: String,
    report_path: String,
    manifest: ExportManifest,
}

fn report_path_for(out: &Path) -> PathBuf {
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".into());
    out.with_file_name(format!("{stem}.manifest.json"))
}

fn publish(b: edit::BuildOutput, out: &str, inputs: &[&str]) -> R<BuildResult> {
    let out_path = PathBuf::from(out);
    let report_path = report_path_for(&out_path);
    let manifest = serde_json::to_string_pretty(&b.manifest)
        .map_err(|e| CmdError::from(Error::Internal(e.to_string())))?;
    let inputs: Vec<&Path> = inputs.iter().map(Path::new).collect();
    io::publish_build(
        &out_path,
        &b.bytes,
        &report_path,
        format!("{manifest}\n").as_bytes(),
        &inputs,
    )?;
    Ok(BuildResult {
        output_path: out_path.display().to_string(),
        report_path: report_path.display().to_string(),
        manifest: b.manifest,
    })
}

/// Identity of a BIOS image alone, shown before a profile is chosen. Works for every
/// supported family; `profiles_supported` says whether a profile can be paired with it.
#[tauri::command(async)]
fn bios_summary(state: tauri::State<'_, AppState>, bios: String) -> R<BiosSummary> {
    let p = state.parsed(&bios)?;
    Ok(ocb_core::engine::session::image_summary(
        &p.image,
        p.schema.as_ref().ok().map(|s| s.as_ref()),
        Some(bios),
    ))
}

/// Every setting the BIOS menus define, with optimized defaults. No profile needed.
#[tauri::command(async)]
fn bios_browse(
    state: tauri::State<'_, AppState>,
    bios: String,
    set: Option<usize>,
) -> R<Report<BrowsePayload>> {
    let p = state.parsed(&bios)?;
    Ok(browse::browse(
        &p.image,
        p.schema.as_ref().ok().map(|s| s.as_ref()),
        Some(bios),
        set.unwrap_or(0),
    ))
}

#[tauri::command(async)]
fn inspect_pair(
    state: tauri::State<'_, AppState>,
    bios: String,
    profile: String,
) -> R<Report<InspectPayload>> {
    Ok(inspect::inspect(&state.session(&bios, &profile)?, true))
}

/// Two profiles side by side: every setting, reviewed or not, that differs. Read-only.
#[tauri::command(async)]
fn diff_pairs(
    state: tauri::State<'_, AppState>,
    left_bios: String,
    left_profile: String,
    right_bios: String,
    right_profile: String,
) -> R<Report<DiffPayload>> {
    let l = state.session(&left_bios, &left_profile)?;
    let r = state.session(&right_bios, &right_profile)?;
    Ok(inspect::diff_with(&l, &r, true))
}

#[tauri::command(async)]
fn verify_pair(
    state: tauri::State<'_, AppState>,
    bios: String,
    profile: String,
) -> R<Report<VerifyPayload>> {
    Ok(verify::verify_profile(&state.session(&bios, &profile)?))
}

fn edit_patch(base: Binding, operations: Vec<Operation>) -> Patch {
    Patch {
        kind: KIND_PATCH.into(),
        schema_version: SCHEMA_VERSION.into(),
        producer: Producer::this(),
        mode: PatchMode::Edit,
        payload_version: PAYLOAD_VERSION.into(),
        base: Some(base),
        source: None,
        target: None,
        operations,
        migration: None,
        policy: Policy {
            unresolved: "error".into(),
        },
        extensions: None,
        diagnostics: vec![],
    }
}

#[derive(Serialize)]
struct EditPreview {
    changes: Vec<ChangeRecord>,
    checks: Vec<Check>,
}

/// Dry run: full validation and in-memory build, nothing written.
#[tauri::command(async)]
fn edit_validate(
    state: tauri::State<'_, AppState>,
    bios: String,
    profile: String,
    base: Binding,
    operations: Vec<Operation>,
) -> R<EditPreview> {
    let s = state.session(&bios, &profile)?;
    let out = edit::build_edit(&s, &edit_patch(base, operations), "preview.ocb")?;
    Ok(EditPreview {
        changes: out.manifest.changes,
        checks: out.manifest.checks,
    })
}

#[tauri::command(async)]
fn edit_build(
    state: tauri::State<'_, AppState>,
    bios: String,
    profile: String,
    base: Binding,
    operations: Vec<Operation>,
    out: String,
) -> R<BuildResult> {
    let s = state.session(&bios, &profile)?;
    let name = Path::new(&out)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let b = edit::build_edit(&s, &edit_patch(base, operations), &name)?;
    publish(b, &out, &[&bios, &profile])
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(async)]
fn migrate_plan(
    state: tauri::State<'_, AppState>,
    source_bios: String,
    source_profile: String,
    target_bios: String,
    target_profile: String,
    selection: Selection,
    prior: Option<Patch>,
) -> R<Patch> {
    let src = state.session(&source_bios, &source_profile)?;
    let tgt = state.session(&target_bios, &target_profile)?;
    Ok(migrate::plan(&src, &tgt, selection, prior.as_ref())?.0)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command(async)]
fn migrate_build(
    state: tauri::State<'_, AppState>,
    source_bios: String,
    source_profile: String,
    target_bios: String,
    target_profile: String,
    plan: Patch,
    out: String,
) -> R<BuildResult> {
    let src = state.session(&source_bios, &source_profile)?;
    let tgt = state.session(&target_bios, &target_profile)?;
    let name = Path::new(&out)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let b = migrate::build(&src, &tgt, &plan, &name)?;
    publish(
        b,
        &out,
        &[&source_bios, &source_profile, &target_bios, &target_profile],
    )
}

/// Save a plan document (exclusive create) when building is unavailable or for agents.
#[tauri::command(async)]
fn save_plan(plan: Patch, out: String) -> R<String> {
    let json = serde_json::to_string_pretty(&plan)
        .map_err(|e| CmdError::from(Error::Internal(e.to_string())))?;
    io::publish_exclusive(Path::new(&out), format!("{json}\n").as_bytes())?;
    Ok(out)
}

/// `config/` beside the running executable. The app is portable: everything it reads or writes
/// for itself (approvals, UI settings, WebView2 data) lives there, nothing in %APPDATA% or
/// %LOCALAPPDATA%.
fn portable_dir() -> Result<PathBuf, String> {
    ocb_core::approvals::config_dir()
        .ok_or_else(|| "cannot locate the program's folder".to_string())
}

fn settings_path() -> R<PathBuf> {
    ocb_core::settings::default_path()
        .ok_or_else(|| CmdError::from(Error::Internal("cannot locate the program's folder".into())))
}

/// What the UI needs from `config/settings.json`.
#[derive(Serialize)]
struct UiSettings {
    theme: Option<String>,
    /// Experimental writing switch, as read at start-up (changing it needs a restart).
    writing: bool,
    /// Why writing is off, or empty.
    writing_note: String,
}

#[derive(serde::Deserialize)]
struct ThemeChoice {
    theme: String,
}

const THEMES: [&str; 3] = ["light", "dark", "system"];

/// A missing or unreadable settings file means defaults; it never stops the app.
#[tauri::command]
fn settings_load() -> R<UiSettings> {
    // Read once at start-up; the theme is only ever changed by this app, which the UI tracks.
    let theme = ocb_core::settings::active()
        .settings
        .theme
        .clone()
        .filter(|t| THEMES.contains(&t.as_str()));
    let writing = ocb_core::settings::writing_enabled();
    Ok(UiSettings {
        theme,
        writing,
        writing_note: if writing {
            String::new()
        } else {
            ocb_core::settings::writing_off_reason()
        },
    })
}

/// Save the colour mode. Every other key in the file is kept as the user wrote it.
#[tauri::command]
fn settings_save(settings: ThemeChoice) -> R<()> {
    if !THEMES.contains(&settings.theme.as_str()) {
        return Err(Error::Request(format!("unknown theme {:?}", settings.theme)).into());
    }
    let path = settings_path()?;
    ocb_core::settings::save_theme(&path, &settings.theme).map_err(|m| {
        CmdError::from(Error::Io {
            path: path.display().to_string(),
            message: m,
        })
    })
}

/// Create the main window with its web view profile (WebView2 on Windows, WebKitGTK on Linux)
/// in `config/webview-data/`.
fn open_main_window(app: &tauri::App) -> Result<(), String> {
    let data = portable_dir()?.join("webview-data");
    std::fs::create_dir_all(&data).map_err(|e| format!("cannot create {}: {e}", data.display()))?;
    let cfg = app
        .config()
        .app
        .windows
        .first()
        .cloned()
        .ok_or("no window configured")?;
    tauri::WebviewWindowBuilder::from_config(app.handle(), &cfg)
        .map_err(|e| e.to_string())?
        .data_directory(data)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn main() {
    // WebKitGTK's DMA-BUF renderer draws a blank window on some Linux GPU drivers (notably
    // NVIDIA); the plain renderer works everywhere. Respect a value the user set themselves.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: first statement of main, before any other thread exists.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            if let Err(m) = open_main_window(app) {
                // Release builds have no console, so say it in a dialog. The folder must be
                // writable: the app keeps its data beside itself and nowhere else.
                rfd::MessageDialog::new()
                    .set_level(rfd::MessageLevel::Error)
                    .set_title("OCB Inspector")
                    .set_description(format!(
                        "OCB Inspector keeps its data in the folder it runs from, and could not \
                         start there.\n\n{m}\n\nIt needs a writable config folder beside it: move the program to a folder you can write to."
                    ))
                    .show();
                return Err(m.into());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bios_summary,
            bios_browse,
            inspect_pair,
            diff_pairs,
            verify_pair,
            edit_validate,
            edit_build,
            migrate_plan,
            migrate_build,
            save_plan,
            settings_load,
            settings_save
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("failed to start: {e}");
            std::process::exit(1);
        });
}
