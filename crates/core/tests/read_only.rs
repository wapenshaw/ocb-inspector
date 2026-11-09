//! The default is read-only: with the image approved but the experimental writing switch off,
//! nothing is writable and the reason names the switch. Runs in its own process because settings
//! and approvals are fixed per process. Skipped when fixtures/private is absent.

use ocb_core::engine::{Session, edit, inspect};
use ocb_core::{catalog, contracts::Value, io};
use std::path::PathBuf;
use std::sync::Arc;

#[test]
fn approved_image_stays_read_only_without_the_switch() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = base.join("fixtures/private");
    if !fixtures.join("bios/E7D91IMS.HJ0").exists() {
        eprintln!("skipping: private fixture corpus not provisioned");
        return;
    }
    ocb_core::approvals::install_from(&base.join("config/approved-images.toml"));
    ocb_core::settings::install(ocb_core::settings::Settings::default());

    let schema = Arc::new(io::load_bios(&fixtures.join("bios/E7D91IMS.HJ0")).expect("parse bios"));
    let bytes = std::fs::read(fixtures.join("profiles/Base-100mv-NoOC.ocb")).expect("read profile");
    let s = Session::pair(schema, bytes).expect("pair");

    assert!(!s.approved(), "writing must be off by default");
    assert!(s.diagnostics.iter().any(|d| d.code == "writing_disabled"));
    assert!(!s.diagnostics.iter().any(|d| d.code == "image_not_approved"));
    assert!(s.blocked_reason().contains("experimental"));

    let report = inspect::inspect(&s, false);
    assert!(
        report
            .payload
            .settings
            .iter()
            .all(|x| !x.editability.writable)
    );
    assert!(!report.payload.capabilities.image_approved_for_writing);

    let recipe = catalog::RECIPES
        .iter()
        .find(|r| r.id == "cpu.cep")
        .expect("recipe");
    let err = edit::plan_write(&s, recipe, &Value::Auto).expect_err("write must be refused");
    assert!(err.contains("experimental"), "{err}");
}
