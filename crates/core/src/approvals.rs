//! Which complete vendor BIOS images may be written to: read from an approvals file at runtime.
//!
//! Nothing is built in. The file is found, in order, at:
//! 1. the path installed by the front end (`ocbi --approvals <path>`),
//! 2. `OCB_APPROVALS`,
//! 3. `config/approved-images.toml` in the running executable's folder (portable: no installer,
//!    no per-user config directory; everything the programs read or keep lives in `config/`).
//!
//! With no file, or an invalid one, nothing is approved and every image is inspect-only. An
//! entry approves one image (BIOS ID plus complete SHA-256) for one recipe adapter, and only
//! takes effect for boards that adapter's recipes were reviewed on: the file can narrow writing,
//! never widen it beyond reviewed recipes. Reports and export manifests record the file used.
//!
//! ```toml
//! [[image]]
//! bios_id = "E7D91IMS.HJ0"
//! sha256 = "fdd8bbf24b06370478261a58b8034dca7d713aea4e41be0e2080efe9fd5b38fe"
//! adapter = "msi-7d91"
//! note = "optional free text"
//! ```

use crate::bytes::sha256_hex;
use crate::catalog;
use crate::contracts::ApprovalSource;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const FILE_NAME: &str = "approved-images.toml";
/// Folder beside the executable that holds the approvals file and the desktop app's own data.
pub const CONFIG_DIR: &str = "config";
pub const ENV_VAR: &str = "OCB_APPROVALS";
/// Approvals files are small; anything larger is not one.
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileFormat {
    #[serde(default)]
    image: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub bios_id: String,
    pub sha256: String,
    pub adapter: String,
    #[serde(default)]
    pub note: Option<String>,
}

/// The approvals in effect for this process.
#[derive(Debug, Clone, Default)]
pub struct Approvals {
    pub entries: Vec<Entry>,
    /// The file the entries came from; `None` when no file was found.
    pub source: Option<ApprovalSource>,
    /// Why the file was rejected (then `entries` is empty).
    pub error: Option<String>,
    /// Where the file was looked for, when none was found.
    pub searched: Option<String>,
}

impl Approvals {
    /// Approved when an entry names this exact image for an adapter whose reviewed recipes
    /// cover this board.
    pub fn is_approved(&self, bios_id: &str, sha256: &str, board: &str) -> bool {
        self.entries.iter().any(|e| {
            e.bios_id == bios_id
                && e.sha256 == sha256
                && e.adapter == catalog::ADAPTER_ID
                && catalog::ADAPTER_BOARDS.contains(&board)
        })
    }

    /// One line for logs and the CLI: where approvals came from, or why there are none.
    pub fn describe(&self) -> String {
        match (&self.source, &self.error) {
            (_, Some(e)) => format!("approvals file rejected, nothing is approved: {e}"),
            (Some(s), None) => format!(
                "{} approved image(s) from {} (sha256 {})",
                self.entries.len(),
                s.path,
                &s.sha256[..12]
            ),
            (None, None) => format!(
                "no approvals file ({}); every image is inspect-only",
                self.searched.as_deref().unwrap_or("no location configured")
            ),
        }
    }
}

/// Parse and validate an approvals file. Any problem rejects the whole file.
pub fn load(path: &Path) -> Approvals {
    let shown = path.display().to_string();
    let fail = |e: String| Approvals {
        error: Some(format!("{shown}: {e}")),
        ..Approvals::default()
    };
    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_FILE_BYTES => return fail("file is too large".into()),
        Ok(_) => {}
        Err(e) => return fail(e.to_string()),
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return fail(e.to_string()),
    };
    let text = match std::str::from_utf8(&bytes) {
        Ok(t) => t,
        Err(_) => return fail("not UTF-8 text".into()),
    };
    let parsed: FileFormat = match toml::from_str(text) {
        Ok(p) => p,
        Err(e) => return fail(e.to_string()),
    };
    for (i, e) in parsed.image.iter().enumerate() {
        if let Err(why) = validate(e) {
            return fail(format!("image entry {}: {why}", i + 1));
        }
    }
    Approvals {
        entries: parsed.image,
        source: Some(ApprovalSource {
            path: shown,
            sha256: sha256_hex(&bytes),
        }),
        error: None,
        searched: None,
    }
}

fn validate(e: &Entry) -> std::result::Result<(), String> {
    let id_ok = (8..=16).contains(&e.bios_id.len())
        && e.bios_id
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'.');
    if !id_ok {
        return Err(format!("bios_id {:?} is not a BIOS ID", e.bios_id));
    }
    let hash_ok = e.sha256.len() == 64
        && e.sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !hash_ok {
        return Err(format!(
            "sha256 for {} must be 64 lowercase hex digits",
            e.bios_id
        ));
    }
    if e.adapter != catalog::ADAPTER_ID {
        return Err(format!(
            "adapter {:?} for {} is unknown (this build has {})",
            e.adapter,
            e.bios_id,
            catalog::ADAPTER_ID
        ));
    }
    Ok(())
}

/// Where the approvals file is looked for when no path was installed: `OCB_APPROVALS`, else
/// `config/` beside the running executable (the app is a portable exe; there is no per-user
/// config location).
pub fn default_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(ENV_VAR).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(p));
    }
    Some(config_dir()?.join(FILE_NAME))
}

/// `config/` beside the running executable: the one place the portable programs keep what they
/// read or write for themselves (approvals, settings, the desktop's WebView2 data).
pub fn config_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join(CONFIG_DIR))
}

static ACTIVE: OnceLock<Approvals> = OnceLock::new();

/// Use the approvals file at `path` for this process. Must run before the first approval check;
/// returns false if approvals were already in effect.
pub fn install_from(path: &Path) -> bool {
    ACTIVE.set(load(path)).is_ok()
}

/// The approvals in effect: the installed file, else the default location, else none.
pub fn active() -> &'static Approvals {
    ACTIVE.get_or_init(|| match default_path() {
        Some(p) if p.exists() => load(&p),
        Some(p) => Approvals {
            searched: Some(p.display().to_string()),
            ..Approvals::default()
        },
        None => Approvals::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HJ0: &str = "fdd8bbf24b06370478261a58b8034dca7d713aea4e41be0e2080efe9fd5b38fe";

    fn write(name: &str, text: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ocb-approvals-{}-{name}", std::process::id()));
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn valid_file_approves_only_reviewed_boards() {
        let p = write(
            "ok.toml",
            &format!(
                "[[image]]\nbios_id = \"E7D91IMS.HJ0\"\nsha256 = \"{HJ0}\"\nadapter = \"msi-7d91\"\n"
            ),
        );
        let a = load(&p);
        assert!(a.error.is_none(), "{:?}", a.error);
        assert!(a.is_approved("E7D91IMS.HJ0", HJ0, "7D91"));
        assert!(!a.is_approved("E7D91IMS.HJ0", HJ0, "7E34"));
        assert!(!a.is_approved("E7D91IMS.HI0", HJ0, "7D91"));
        assert!(a.source.is_some());
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn any_bad_entry_rejects_the_whole_file() {
        for (name, body) in [
            (
                "hash",
                format!(
                    "bios_id = \"E7D91IMS.HJ0\"\nsha256 = \"{}\"\nadapter = \"msi-7d91\"",
                    HJ0.to_uppercase()
                ),
            ),
            (
                "adapter",
                format!("bios_id = \"E7D91IMS.HJ0\"\nsha256 = \"{HJ0}\"\nadapter = \"msi-7e34\""),
            ),
            (
                "field",
                format!(
                    "bios_id = \"E7D91IMS.HJ0\"\nsha256 = \"{HJ0}\"\nadapter = \"msi-7d91\"\nforce = true"
                ),
            ),
            (
                "id",
                format!("bios_id = \"../x\"\nsha256 = \"{HJ0}\"\nadapter = \"msi-7d91\""),
            ),
        ] {
            let ok = format!(
                "[[image]]\nbios_id = \"E7D91IMS.HI0\"\nsha256 = \"{HJ0}\"\nadapter = \"msi-7d91\"\n"
            );
            let p = write(name, &format!("{ok}[[image]]\n{body}\n"));
            let a = load(&p);
            assert!(a.error.is_some(), "{name}");
            assert!(a.entries.is_empty(), "{name}");
            assert!(!a.is_approved("E7D91IMS.HI0", HJ0, "7D91"), "{name}");
            let _ = std::fs::remove_file(p);
        }
        let missing = load(Path::new("definitely/not/here.toml"));
        assert!(missing.error.is_some() && missing.entries.is_empty());
    }
}
