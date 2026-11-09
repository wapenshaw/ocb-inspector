//! Version, changelog and the update check: one GET of this repository's public release list
//! on GitHub. It sends nothing but the request itself (the User-Agent names the app and its
//! version), downloads nothing and installs nothing; the user opens the release page in their
//! browser. Off with `"update_check": false` in `config/settings.json`.

use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const REPO_URL: &str = "https://github.com/wapenshaw/ocb-inspector";
const RELEASES_API: &str =
    "https://api.github.com/repos/wapenshaw/ocb-inspector/releases?per_page=30";
/// The release list is a few kilobytes per release; anything far larger is not it.
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

/// What the About screen shows without the network.
#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    /// CHANGELOG.md as built into this version (written by release-please).
    changelog: &'static str,
    repo_url: &'static str,
    /// False when `"update_check": false` is in the settings file.
    update_check: bool,
}

pub fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        changelog: include_str!("../../../CHANGELOG.md"),
        repo_url: REPO_URL,
        update_check: ocb_core::settings::active().settings.update_check != Some(false),
    }
}

/// One published release, newest first in `UpdateInfo::newer`.
#[derive(Serialize)]
pub struct Release {
    version: String,
    /// Release notes (release-please's Markdown changelog entry).
    notes: String,
    url: String,
    published_at: String,
}

#[derive(Serialize)]
pub struct UpdateInfo {
    current: &'static str,
    /// Releases newer than this one, newest first; empty when up to date.
    newer: Vec<Release>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
}

/// `v1.2.3` or `1.2.3` as comparable numbers; anything else is ignored.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v.strip_prefix('v').unwrap_or(v).split('.');
    let n = |p: Option<&str>| p.and_then(|s| s.parse().ok());
    let v = (n(it.next())?, n(it.next())?, n(it.next())?);
    it.next().is_none().then_some(v)
}

/// Fetch the release list and keep the published releases newer than this build.
pub fn check() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION");
    let mine = parse_version(current).ok_or("this build has no release version")?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    let text = agent
        .get(RELEASES_API)
        .header("User-Agent", &format!("ocb-inspector/{current}"))
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("could not reach GitHub: {e}"))?
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(|e| format!("could not read GitHub's answer: {e}"))?;
    let list: Vec<GhRelease> =
        serde_json::from_str(&text).map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    let mut newer: Vec<((u64, u64, u64), Release)> = list
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| {
            let v = parse_version(&r.tag_name)?;
            (v > mine).then(|| {
                (
                    v,
                    Release {
                        version: format!("{}.{}.{}", v.0, v.1, v.2),
                        notes: r.body.unwrap_or_default(),
                        url: r.html_url,
                        published_at: r.published_at.unwrap_or_default(),
                    },
                )
            })
        })
        .collect();
    newer.sort_by_key(|r| std::cmp::Reverse(r.0));
    Ok(UpdateInfo {
        current,
        newer: newer.into_iter().map(|(_, r)| r).collect(),
    })
}

/// Open a page of this repository in the default browser. Only this repository's pages are
/// allowed, so the window can never open an arbitrary address or program.
pub fn open_page(url: &str) -> Result<(), String> {
    let ours = url == REPO_URL
        || url
            .strip_prefix(REPO_URL)
            .is_some_and(|rest| rest.starts_with('/'));
    let ok = ours && url.chars().all(|c| c.is_ascii_graphic());
    if !ok {
        return Err(format!("not a page of {REPO_URL}"));
    }
    #[cfg(windows)]
    let spawned = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(not(windows))]
    let spawned = std::process::Command::new("xdg-open").arg(url).spawn();
    spawned
        .map(|_| ())
        .map_err(|e| format!("could not open the browser: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert_eq!(parse_version("v0.10.2"), Some((0, 10, 2)));
        assert!(parse_version("v0.10.0") > parse_version("0.9.9"));
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3-beta"), None);
    }

    #[test]
    fn only_this_repository_opens() {
        assert!(open_page("https://github.com/wapenshaw/ocb-inspector.evil/x").is_err());
        assert!(open_page("https://example.com").is_err());
        assert!(open_page("file:///C:/Windows").is_err());
        assert!(open_page("https://github.com/wapenshaw/ocb-inspector/a b").is_err());
    }
}
