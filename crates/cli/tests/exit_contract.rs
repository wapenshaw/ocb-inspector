//! End-to-end strict-command exit contract; uses the optional private fixture corpus.
use ocb_core::format::{CHECKSUM_AT, OcbFile, compute_checksum};
use std::path::{Path, PathBuf};
use std::process::Command;

fn inspect(bios: &Path, profile: &Path) -> (i32, serde_json::Value) {
    let output = Command::new(env!("CARGO_BIN_EXE_ocbi"))
        .args(["profile", "inspect", "--bios"])
        .arg(bios)
        .arg("--profile")
        .arg(profile)
        .args(["--format", "json"])
        .output()
        .unwrap();
    (
        output.status.code().unwrap(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

#[test]
fn identity_mismatch_is_4_but_missing_record_size_is_5() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/private");
    let bios = root.join("bios/E7D91IMS.HJ0");
    let source = root.join("profiles/OC-Just-Minus-100.ocb");
    let wrong = root.join("profiles/BeforeUpgrade.ocb");
    if !bios.exists() || !source.exists() || !wrong.exists() {
        eprintln!("skipping: private fixture corpus not provisioned");
        return;
    }
    let (status, error) = inspect(&bios, &wrong);
    assert_eq!(status, 4);
    assert_eq!(error["code"], "identity_mismatch");
    assert_eq!(error["exit_code"], 4);

    // Keep identity, structure and checksum valid, but remove the CpuSetup size match.
    let mut bytes = std::fs::read(source).unwrap();
    let parsed = OcbFile::parse(bytes.clone()).unwrap();
    let record = parsed.records.iter().find(|r| r.size == 961).unwrap();
    bytes.splice(
        record.offset + record.size..record.offset + record.size,
        [0],
    );
    bytes[record.offset - 2..record.offset].copy_from_slice(&962u16.to_le_bytes());
    let checksum = compute_checksum(&bytes).unwrap();
    bytes[CHECKSUM_AT..CHECKSUM_AT + 4].copy_from_slice(&checksum.to_le_bytes());
    assert!(OcbFile::parse(bytes.clone()).unwrap().checksum_valid());
    let temp = std::env::temp_dir().join(format!("ocbi-exit-contract-{}.ocb", std::process::id()));
    std::fs::write(&temp, bytes).unwrap();
    let result = inspect(&bios, &temp);
    std::fs::remove_file(temp).unwrap();
    assert_eq!(result.0, 5);
    assert_eq!(result.1["code"], "unsupported");
    assert_eq!(result.1["exit_code"], 5);
    assert!(
        result.1["message"]
            .as_str()
            .unwrap()
            .contains("CpuSetup size 961")
    );
}
