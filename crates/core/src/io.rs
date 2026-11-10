//! Filesystem boundary: bounded reads and exclusive, atomic publication (SPEC I-07).

use crate::error::{Error, Result};
use crate::firmware::{BiosSchema, Limits, parse_bios};
use crate::format::MAX_PROFILE_BYTES;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

fn io_err(path: &Path, e: std::io::Error) -> Error {
    Error::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    }
}

/// Read a whole file of at most `limit` bytes. The size on disk rejects a large file early, and
/// the read itself stops after `limit + 1` bytes, so a file that grows (or is replaced) after
/// that check can never be read past the limit.
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = fs::File::open(path).map_err(|e| io_err(path, e))?;
    let len = file.metadata().map_err(|e| io_err(path, e))?.len();
    if len > limit as u64 {
        return Err(Error::Limit(format!(
            "{} is {len} bytes; limit is {limit}",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io_err(path, e))?;
    if bytes.len() > limit {
        return Err(Error::Limit(format!(
            "{} grew past the limit of {limit} bytes while being read",
            path.display()
        )));
    }
    Ok(bytes)
}

/// The first bytes of a file and its size, for naming a wrong input after a size-limit failure.
fn peek(path: &Path) -> Option<(Vec<u8>, u64)> {
    let file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut head = Vec::new();
    file.take(8).read_to_end(&mut head).ok()?;
    Some((head, len))
}

/// Read a file given as a BIOS image. A file over the limit that is an archive or an OC profile
/// says so instead of only reporting its size.
pub fn read_bios(path: &Path, limits: &Limits) -> Result<Vec<u8>> {
    read_bounded(path, limits.max_bios_bytes).map_err(|e| match e {
        Error::Limit(m) => {
            match peek(path).and_then(|(h, _)| crate::firmware::not_a_bios_image(&h)) {
                Some(why) => Error::Limit(format!("{m}; {why}")),
                None => Error::Limit(m),
            }
        }
        other => other,
    })
}

pub fn load_bios(path: &Path) -> Result<BiosSchema> {
    let limits = Limits::default();
    let bytes = read_bios(path, &limits)?;
    parse_bios(&bytes, limits)
}

/// Read an image for browsing: menus of any supported family, plus the decode schema when the
/// family has one.
pub fn load_analysis(path: &Path) -> Result<crate::firmware::Analysis> {
    let limits = Limits::default();
    let bytes = read_bios(path, &limits)?;
    crate::firmware::analyze(&bytes, limits)
}

/// Read a file given as an OC profile. A file over the limit that is evidently a BIOS image says
/// so instead of only reporting its size.
pub fn load_profile(path: &Path) -> Result<Vec<u8>> {
    read_bounded(path, MAX_PROFILE_BYTES).map_err(|e| match e {
        Error::Limit(m) => match peek(path) {
            Some((h, len)) => Error::Limit(crate::format::with_swapped_hint(&m, "; ", &h, len)),
            None => Error::Limit(m),
        },
        other => other,
    })
}

fn canonical_target(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).map_err(|e| io_err(parent, e))?;
    let name = path
        .file_name()
        .ok_or_else(|| Error::Request(format!("{} has no file name", path.display())))?;
    Ok(parent.join(name))
}

/// Refuse outputs that alias any input path.
pub fn ensure_not_input(output: &Path, inputs: &[&Path]) -> Result<()> {
    let out = canonical_target(output)?;
    for i in inputs {
        if let Ok(c) = fs::canonicalize(i)
            && c == out
        {
            return Err(Error::OutputExists(format!(
                "{} is an input file",
                output.display()
            )));
        }
    }
    Ok(())
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Write `bytes` to a new, uniquely named, flushed temp file beside `target`.
fn write_temp_sibling(target: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let dir = target
        .parent()
        .ok_or_else(|| Error::Internal("no parent".into()))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}-{}",
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| io_err(&tmp, e))?;
        f.write_all(bytes).map_err(|e| io_err(&tmp, e))?;
        f.sync_all().map_err(|e| io_err(&tmp, e))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written.map(|()| tmp)
}

/// Replace the file at `path` (or create it) atomically: a flushed temp sibling renamed over it.
/// For the program's own files (settings), never for user outputs, which use `publish_exclusive`.
pub fn replace_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = write_temp_sibling(path, bytes)?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io_err(path, e)
    })
}

/// Publish bytes at `path` atomically without ever replacing an existing file:
/// write a temporary sibling, flush it, then hard-link it into place (fails if the name exists).
pub fn publish_exclusive(path: &Path, bytes: &[u8]) -> Result<()> {
    let target = canonical_target(path)?;
    if target.exists() {
        return Err(Error::OutputExists(path.display().to_string()));
    }
    let tmp = write_temp_sibling(&target, bytes)?;
    let result = fs::hard_link(&tmp, &target).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => Error::OutputExists(path.display().to_string()),
        _ => Error::Io {
            path: path.display().to_string(),
            message: format!("atomic publication via hard link failed ({e}); this filesystem cannot satisfy the publication guarantee"),
        },
    });
    let _ = fs::remove_file(&tmp);
    result
}

/// Publish the manifest first, then the OCB as the completion artifact. On failure, neither remains.
pub fn publish_build(
    ocb_path: &Path,
    ocb: &[u8],
    report_path: &Path,
    report: &[u8],
    inputs: &[&Path],
) -> Result<()> {
    ensure_not_input(ocb_path, inputs)?;
    ensure_not_input(report_path, inputs)?;
    if canonical_target(ocb_path)? == canonical_target(report_path)? {
        return Err(Error::Request(
            "output and report paths are the same".into(),
        ));
    }
    for p in [ocb_path, report_path] {
        if canonical_target(p)?.exists() {
            return Err(Error::OutputExists(p.display().to_string()));
        }
    }
    publish_exclusive(report_path, report)?;
    if let Err(e) = publish_exclusive(ocb_path, ocb) {
        let _ = fs::remove_file(report_path);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str, bytes: &[u8]) -> PathBuf {
        let p = std::env::temp_dir().join(format!("ocb-io-{}-{name}", std::process::id()));
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn a_file_at_the_limit_is_read_and_one_byte_more_is_not() {
        let at = tmp("at", &[7; 16]);
        assert_eq!(read_bounded(&at, 16).unwrap(), vec![7; 16]);
        let over = tmp("over", &[7; 17]);
        assert!(matches!(read_bounded(&over, 16), Err(Error::Limit(_))));
        let _ = fs::remove_file(at);
        let _ = fs::remove_file(over);
    }
    #[test]
    fn an_oversized_profile_that_is_firmware_says_so_and_stays_a_limit_error() {
        let big = tmp("bigfw", &vec![0xFF; MAX_PROFILE_BYTES + 1]);
        let e = load_profile(&big).expect_err("limit");
        assert!(matches!(e, Error::Limit(_)), "{e}");
        assert!(e.to_string().contains("looks like a BIOS image"), "{e}");
        let mut own = vec![0u8; MAX_PROFILE_BYTES + 1];
        own[..5].copy_from_slice(b"$MOS$");
        let plain = tmp("bigmos", &own);
        let e = load_profile(&plain).expect_err("limit");
        assert!(!e.to_string().contains("BIOS image"), "{e}");
        let _ = fs::remove_file(big);
        let _ = fs::remove_file(plain);
    }

    #[test]
    fn archives_and_profiles_given_as_the_bios_are_named() {
        let zip = tmp("zip", b"PK not really");
        let e = load_bios(&zip).expect_err("zip");
        assert!(matches!(e, Error::Unsupported(_)), "{e}");
        assert!(e.to_string().contains("zip archive"), "{e}");
        let mut big = vec![0u8; Limits::default().max_bios_bytes + 1];
        big[..4].copy_from_slice(b"PK");
        let over = tmp("bigzip", &big);
        let e = load_analysis(&over).expect_err("limit");
        assert!(matches!(e, Error::Limit(_)), "{e}");
        assert!(e.to_string().contains("zip archive"), "{e}");
        let mos = tmp("mos", b"$MOS$E7D91IMS.HJ0");
        let e = load_bios(&mos).expect_err("profile");
        assert!(
            e.to_string().contains("OC profile, not a BIOS image"),
            "{e}"
        );
        for p in [zip, over, mos] {
            let _ = fs::remove_file(p);
        }
    }
}
