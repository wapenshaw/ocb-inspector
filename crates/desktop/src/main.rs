//! OCB Inspector desktop shell. Commands are thin wrappers over `ocb_core::engine`, the same
//! services the CLI uses, so the UI and CLI make identical decisions (SPEC §1, §12).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ocb_core::bytes::sha256_hex;
use ocb_core::contracts::*;
use ocb_core::engine::{Session, browse, edit, inspect, migrate};
use ocb_core::error::Error;
use ocb_core::firmware::{BiosImage, BiosSchema, Limits, analyze};
use ocb_core::format::MAX_PROFILE_BYTES;
use ocb_core::io;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

mod updates;

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

/// A file as last seen: where it is, its size and when it was last changed.
#[derive(PartialEq, Eq, Hash)]
struct FileStamp {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}

impl FileStamp {
    fn of(path: &str) -> Option<FileStamp> {
        let path = std::fs::canonicalize(path).ok()?;
        let meta = std::fs::metadata(&path).ok()?;
        Some(FileStamp {
            len: meta.len(),
            modified: meta.modified().ok()?,
            path,
        })
    }
}

/// How much to trust a BIOS image read earlier.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImageRead {
    /// Display only: an image whose path, size and modification time are unchanged is not read
    /// and hashed again.
    Cached,
    /// Anything that writes: always read and hash the file now, so approval is decided on what
    /// the file holds, never on what it held before.
    Fresh,
}

/// Parsed images keyed by digest, so each image parses once per run, plus the digest each file
/// had when last read.
#[derive(Default)]
struct AppState {
    parsed: Mutex<HashMap<String, Arc<Parsed>>>,
    digests: Mutex<HashMap<FileStamp, String>>,
}

fn locked<T>(m: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, Error> {
    m.lock()
        .map_err(|_| Error::Internal("cache poisoned".into()))
}

impl AppState {
    fn parsed(&self, path: &str, read: ImageRead) -> Result<Arc<Parsed>, Error> {
        let stamp = FileStamp::of(path);
        if read == ImageRead::Cached
            && let Some(stamp) = &stamp
            && let Some(sha) = locked(&self.digests)?.get(stamp)
            && let Some(p) = locked(&self.parsed)?.get(sha)
        {
            return Ok(p.clone());
        }
        let limits = Limits::default();
        let bytes = io::read_bounded(Path::new(path), limits.max_bios_bytes)?;
        let sha = sha256_hex(&bytes);
        if let Some(stamp) = stamp {
            locked(&self.digests)?.insert(stamp, sha.clone());
        }
        if let Some(p) = locked(&self.parsed)?.get(&sha) {
            return Ok(p.clone());
        }
        let a = analyze(&bytes, limits)?;
        let p = Arc::new(Parsed {
            image: Arc::new(a.image),
            schema: a.schema.map(Arc::new),
        });
        locked(&self.parsed)?.insert(sha, p.clone());
        Ok(p)
    }

    fn session(&self, bios: &str, profile: &str, read: ImageRead) -> Result<Session, Error> {
        let schema = self.parsed(bios, read)?.schema.clone()?;
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
    let p = state.parsed(&bios, ImageRead::Cached)?;
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
    let p = state.parsed(&bios, ImageRead::Cached)?;
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
    Ok(inspect::inspect(
        &state.session(&bios, &profile, ImageRead::Cached)?,
        true,
    ))
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
    let l = state.session(&left_bios, &left_profile, ImageRead::Cached)?;
    let r = state.session(&right_bios, &right_profile, ImageRead::Cached)?;
    Ok(inspect::diff_with(&l, &r, true))
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
    let s = state.session(&bios, &profile, ImageRead::Cached)?;
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
    let s = state.session(&bios, &profile, ImageRead::Fresh)?;
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
    let src = state.session(&source_bios, &source_profile, ImageRead::Cached)?;
    let tgt = state.session(&target_bios, &target_profile, ImageRead::Cached)?;
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
    let src = state.session(&source_bios, &source_profile, ImageRead::Fresh)?;
    let tgt = state.session(&target_bios, &target_profile, ImageRead::Fresh)?;
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
    /// The settings file, for display.
    path: String,
    /// What the file holds now, for the Settings switches. Saved changes apply at the next start.
    saved: SavedSwitches,
    /// Why the file cannot be read or changed, or empty (the switches are then disabled).
    error: String,
}

#[derive(Serialize, Default)]
struct SavedSwitches {
    update_check: bool,
    writing: bool,
}

impl From<&ocb_core::settings::Settings> for SavedSwitches {
    fn from(s: &ocb_core::settings::Settings) -> Self {
        SavedSwitches {
            update_check: s.update_check_on(),
            writing: s.experimental.writing,
        }
    }
}

/// A missing or unreadable settings file means defaults; it never stops the app.
#[tauri::command]
fn settings_load() -> R<UiSettings> {
    use ocb_core::settings::{THEMES, active, saved, writing_enabled, writing_off_reason};
    // Read once at start-up; the theme is only ever changed by this app, which the UI tracks.
    let theme = active()
        .settings
        .theme
        .clone()
        .filter(|t| THEMES.contains(&t.as_str()));
    let writing = writing_enabled();
    // What the file holds now, which may differ from what was read at start.
    let now = portable_dir().and_then(|dir| {
        let p = dir.join(ocb_core::settings::FILE_NAME);
        Ok((p.display().to_string(), saved(&p)?))
    });
    let (path, saved, error) = match now {
        Ok((path, s)) => (path, SavedSwitches::from(&s), String::new()),
        Err(e) => (String::new(), SavedSwitches::default(), e),
    };
    Ok(UiSettings {
        theme,
        writing,
        writing_note: if writing {
            String::new()
        } else {
            writing_off_reason()
        },
        path,
        saved,
        error,
    })
}

/// Save the colour mode or a Settings switch. Every other key in the file is kept as the user
/// wrote it. Returns the switches as now saved.
#[tauri::command]
fn settings_save(settings: ocb_core::settings::Change) -> R<SavedSwitches> {
    if let Some(theme) = &settings.theme
        && !ocb_core::settings::THEMES.contains(&theme.as_str())
    {
        return Err(Error::Request(format!("unknown theme {theme:?}")).into());
    }
    let path = settings_path()?;
    match ocb_core::settings::save(&path, &settings) {
        Ok(s) => Ok(SavedSwitches::from(&s)),
        Err(message) => Err(Error::Io {
            path: path.display().to_string(),
            message,
        }
        .into()),
    }
}

/// Version, built-in changelog and whether the update check is on.
#[tauri::command]
fn app_info() -> updates::AppInfo {
    updates::app_info()
}

/// Releases on GitHub newer than this build (one GET of the public release list).
#[tauri::command(async)]
fn check_updates() -> R<updates::UpdateInfo> {
    updates::check().map_err(|m| {
        CmdError::from(Error::Io {
            path: "GitHub releases".into(),
            message: m,
        })
    })
}

/// Open a page of this repository (a release, the changelog) in the default browser.
#[tauri::command]
fn open_page(url: String) -> R<()> {
    updates::open_page(&url).map_err(|m| Error::Request(m).into())
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
            edit_validate,
            edit_build,
            migrate_plan,
            migrate_build,
            save_plan,
            settings_load,
            settings_save,
            app_info,
            check_updates,
            open_page
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("failed to start: {e}");
            std::process::exit(1);
        });
}
