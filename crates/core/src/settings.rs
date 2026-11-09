//! App settings, read at runtime from `config/settings.json` beside the running executable
//! (the programs are portable: no installer, no per-user config directory).
//!
//! The tools are read-only by default. Writing (edit, migrate build) is experimental and only
//! turns on with
//!
//! ```json
//! { "experimental": { "writing": true } }
//! ```
//!
//! It is a second lock, not a replacement: an image must still be listed in the approvals file
//! (`approvals`), and only reviewed recipes are ever written. A missing file means defaults; an
//! unreadable or invalid one also means writing stays off, and the reason is kept for display.
//! The desktop app also keeps its colour mode (`theme`) here; saving it leaves every other key as
//! the user wrote it.

use crate::approvals::CONFIG_DIR;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const FILE_NAME: &str = "settings.json";
/// Settings files are tiny; anything larger is not one.
const MAX_FILE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub experimental: Experimental,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Experimental {
    #[serde(default)]
    pub writing: bool,
}

/// The settings in effect for this process.
#[derive(Debug, Clone, Default)]
pub struct Active {
    pub settings: Settings,
    /// The file that was read, or would have been.
    pub path: Option<PathBuf>,
    /// Why the file was rejected (settings are then defaults: writing off).
    pub error: Option<String>,
}

/// `config/settings.json` beside the running executable.
pub fn default_path() -> Option<PathBuf> {
    Some(crate::approvals::config_dir()?.join(FILE_NAME))
}

fn read_object(path: &Path) -> Result<Option<serde_json::Map<String, serde_json::Value>>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = crate::io::read_bounded(path, MAX_FILE_BYTES).map_err(|e| e.to_string())?;
    match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(serde_json::Value::Object(m)) => Ok(Some(m)),
        Ok(_) => Err(format!("{} is not a JSON object", path.display())),
        Err(e) => Err(format!("{} is not valid JSON: {e}", path.display())),
    }
}

/// Read settings from `path`. A missing file gives defaults.
pub fn load(path: &Path) -> Active {
    let mut active = Active {
        path: Some(path.to_path_buf()),
        ..Active::default()
    };
    match read_object(path) {
        Ok(None) => {}
        Ok(Some(m)) => match serde_json::from_value::<Settings>(serde_json::Value::Object(m)) {
            Ok(s) => active.settings = s,
            Err(e) => active.error = Some(format!("{}: {e}", path.display())),
        },
        Err(e) => active.error = Some(e),
    }
    active
}

static ACTIVE: OnceLock<Active> = OnceLock::new();

/// Use the settings file at `path` for this process (CLI `--settings`, tests). Must run before
/// the first use; returns false if settings were already in effect.
pub fn install_from(path: &Path) -> bool {
    ACTIVE.set(load(path)).is_ok()
}

/// Use these settings for this process without a file (tests). Same rules as `install_from`.
pub fn install(settings: Settings) -> bool {
    ACTIVE
        .set(Active {
            settings,
            ..Active::default()
        })
        .is_ok()
}

/// The settings in effect: the installed ones, else the default file, else defaults.
pub fn active() -> &'static Active {
    ACTIVE.get_or_init(|| match default_path() {
        Some(p) => load(&p),
        None => Active::default(),
    })
}

/// Whether the experimental writing switch is on for this process.
pub fn writing_enabled() -> bool {
    active().settings.experimental.writing
}

/// Why writing is off, for messages: the switch and where it lives.
pub fn writing_off_reason() -> String {
    let where_ = active()
        .path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("{CONFIG_DIR}/{FILE_NAME}"));
    match &active().error {
        Some(e) => format!("writing is off because the settings file was rejected ({e})"),
        None => format!(
            "writing is turned off; it is experimental and enabled by \
             \"experimental\": {{ \"writing\": true }} in {where_}"
        ),
    }
}

/// Set `theme` in the settings file at `path`, keeping every other key exactly as it was.
/// Written to a flushed temp sibling, then renamed over the file.
pub fn save_theme(path: &Path, theme: &str) -> Result<(), String> {
    let mut obj = read_object(path)?.unwrap_or_default();
    obj.insert("theme".into(), serde_json::Value::String(theme.into()));
    let json =
        serde_json::to_string_pretty(&serde_json::Value::Object(obj)).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    crate::io::replace_atomic(
        path,
        format!(
            "{json}
"
        )
        .as_bytes(),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ocb-settings-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d.join(FILE_NAME)
    }

    #[test]
    fn missing_file_means_read_only() {
        let a = load(&tmp("missing").with_file_name("absent.json"));
        assert!(!a.settings.experimental.writing);
        assert!(a.error.is_none());
    }

    #[test]
    fn invalid_file_means_read_only_with_reason() {
        let p = tmp("invalid");
        std::fs::write(&p, "{ \"experimental\": { \"writing\": \"yes\" } }").unwrap();
        let a = load(&p);
        assert!(!a.settings.experimental.writing);
        assert!(a.error.is_some());
    }

    #[test]
    fn saving_the_theme_keeps_the_writing_switch() {
        let p = tmp("merge");
        std::fs::write(
            &p,
            "{ \"experimental\": { \"writing\": true }, \"note\": 1 }",
        )
        .unwrap();
        save_theme(&p, "dark").unwrap();
        let a = load(&p);
        assert!(a.settings.experimental.writing);
        assert_eq!(a.settings.theme.as_deref(), Some("dark"));
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
        assert_eq!(v["note"], 1);
    }
}
