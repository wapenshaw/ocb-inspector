//! Tests that need no proprietary fixtures (safe for public CI).

use ocb_core::engine::codec::slug;
use ocb_core::format::{CHECKSUM_AT, COUNT_AT, OcbFile, compute_checksum};
use ocb_core::io;

fn synthetic(cpu: &str) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"$MOS$E7D91IMS.HJ0");
    b.extend_from_slice(&0i32.to_le_bytes());
    b.extend_from_slice(&2u32.to_le_bytes());
    for (id, payload) in [(0u32, &[1u8, 2, 3, 4][..]), (5u32, &[9u8, 8, 7][..])] {
        b.extend_from_slice(&id.to_le_bytes());
        b.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        b.extend_from_slice(payload);
    }
    b.extend(std::iter::repeat_n(0x11u8, 256));
    b.extend_from_slice(b"$OCI$\0E7D91IMS\0HJ0\0");
    b.extend_from_slice(cpu.as_bytes());
    b.extend_from_slice(b"\x0020260101120000\0");
    b.extend(std::iter::repeat_n(0u8, 24));
    let c = compute_checksum(&b).unwrap();
    b[CHECKSUM_AT..COUNT_AT].copy_from_slice(&c.to_le_bytes());
    b
}

#[test]
fn synthetic_profile_parses_losslessly() {
    let b = synthetic("Test CPU");
    let f = OcbFile::parse(b.clone()).unwrap();
    assert_eq!(f.bytes(), b.as_slice());
    assert_eq!(f.bios_id, "E7D91IMS.HJ0");
    assert_eq!(f.records.len(), 2);
    assert_eq!(f.oci.cpu_brand, "Test CPU");
    assert_eq!(f.oci.saved_at, "20260101120000");
    assert!(f.checksum_valid());
}

#[test]
fn checksum_excludes_its_own_bytes() {
    let mut b = synthetic("X");
    let c = compute_checksum(&b).unwrap();
    b[CHECKSUM_AT] ^= 0xFF;
    assert_eq!(
        compute_checksum(&b).unwrap(),
        c,
        "checksum field must not affect itself"
    );
    b[0x30] ^= 1;
    assert_ne!(compute_checksum(&b).unwrap(), c);
}

#[test]
fn malformed_profiles_fail_cleanly() {
    let b = synthetic("CPU");
    for cut in 0..b.len() {
        assert!(
            OcbFile::parse(b[..cut].to_vec()).is_err(),
            "truncated at {cut}"
        );
    }
    let mut bad = b.clone();
    bad[COUNT_AT] = 0xFF; // record count overflow
    assert!(OcbFile::parse(bad).is_err());
    let mut bad = b.clone();
    bad[0x19] = 9; // first id now greater than the second
    assert!(OcbFile::parse(bad).is_err());
    let mut bad = b;
    bad.push(0); // unexpected $OCI$ tail length
    assert!(OcbFile::parse(bad).is_err());
}

#[test]
fn slugs_are_stable() {
    assert_eq!(slug("Adaptive + Offset Mode"), "adaptive_plus_offset_mode");
    assert_eq!(slug(" - "), "minus");
    assert_eq!(slug("Turbo Ratio Offset"), "turbo_ratio_offset");
}

#[test]
fn publication_never_overwrites() {
    let dir = std::env::temp_dir().join(format!("ocb-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("out.ocb");
    io::publish_exclusive(&p, b"first").unwrap();
    let e = io::publish_exclusive(&p, b"second").unwrap_err();
    assert_eq!(e.exit_code(), 3);
    assert_eq!(std::fs::read(&p).unwrap(), b"first");
    // Output paths that alias an input are refused.
    assert!(io::ensure_not_input(&p, &[&p]).is_err());
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files left behind");
    std::fs::remove_file(&p).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn bios_identity_accepts_both_msi_formats() {
    use ocb_core::firmware::find_identity;
    let older = b"..E7D91IMS.HJ0 09/14/2026..";
    let id = find_identity(older).unwrap();
    assert_eq!(
        (
            id.bios_id.as_str(),
            id.board.as_str(),
            id.flavor.as_str(),
            id.revision.as_str()
        ),
        ("E7D91IMS.HJ0", "7D91", "IMS", "HJ0")
    );
    let newer = b"xxE7E48AMSI.2A35 09/03/2026\0";
    let id = find_identity(newer).unwrap();
    assert_eq!(
        (
            id.bios_id.as_str(),
            id.board.as_str(),
            id.flavor.as_str(),
            id.revision.as_str()
        ),
        ("E7E48AMSI.2A35", "7E48", "AMSI", "2A35")
    );
    assert_eq!(id.build_date, "09/03/2026");
    assert!(find_identity(b"E7D91IMS.HJ0 09/14/2026 E7D91IMS.HI0 03/17/2026").is_err());
}
