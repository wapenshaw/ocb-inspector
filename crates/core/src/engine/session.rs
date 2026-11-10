//! Pairing a BIOS schema with a profile (SPEC I-01, I-02).

use crate::approvals;
use crate::bytes::sha256_hex;
use crate::catalog::{ADAPTER_ID, ADAPTER_VERSION};
use crate::contracts::{
    Binding, BiosSummary, Capabilities, DefaultsInfo, Diagnostic, PlatformInfo, ProfileEvidence,
    ProfileSummary, Severity, StoreSpan,
};
use crate::error::{Error, Result};
use crate::firmware::platform::{Platform, Vendor};
use crate::firmware::{BiosImage, BiosSchema};
use crate::format::{OcbFile, Record};
use crate::settings;
use std::sync::Arc;

/// A BIOS/profile pair that passed identity and structural checks. Only `pair` constructs it.
#[derive(Debug, Clone)]
pub struct Session {
    pub schema: Arc<BiosSchema>,
    pub profile: OcbFile,
    pub profile_sha256: String,
    pub setup: Record,
    pub cpu_setup: Record,
    /// Why writing is not allowed (experimental switch off, or image not approved); None when
    /// writing is allowed. The one source for `approved()`.
    pub write_blocked: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub bios_path: Option<String>,
    pub profile_path: Option<String>,
}

impl Session {
    pub fn pair(schema: Arc<BiosSchema>, profile_bytes: Vec<u8>) -> Result<Session> {
        if !schema.platform.profiles_supported {
            return Err(Error::Unsupported(schema.platform.profiles_note.clone()));
        }
        let profile_sha256 = sha256_hex(&profile_bytes);
        let profile = OcbFile::parse(profile_bytes)?;
        let id = &schema.identity;
        let mut diagnostics = Vec::new();

        if profile.bios_id != id.bios_id {
            return Err(Error::IdentityMismatch(
                crate::firmware::platform::identity_mismatch(&profile.bios_id, &id.bios_id),
            ));
        }
        let expected_board = format!("E{}{}", id.board, id.flavor);
        if profile.oci.board != expected_board || profile.oci.revision != id.revision {
            return Err(Error::IdentityMismatch(format!(
                "profile $OCI$ metadata says {}.{}, header and image say {}",
                profile.oci.board, profile.oci.revision, id.bios_id
            )));
        }
        if !profile.checksum_valid() {
            return Err(Error::malformed(
                "OCB",
                format!(
                    "checksum mismatch: stored {}, computed {}",
                    profile.stored_checksum,
                    profile.computed_checksum().unwrap_or_default()
                ),
            ));
        }
        let bind = |name: &str| -> Result<Record> {
            let store = schema.store(name).ok_or_else(|| {
                Error::Unsupported(format!("BIOS defines no {name} buffer store"))
            })?;
            match profile.record_by_size(usize::from(store.size)).as_slice() {
                [r] => Ok((*r).clone()),
                [] => Err(Error::IdentityMismatch(format!(
                    "no profile record matches {name} size {}",
                    store.size
                ))),
                many => Err(Error::Unsupported(format!(
                    "{} profile records match {name} size {}; binding is ambiguous",
                    many.len(),
                    store.size
                ))),
            }
        };
        let setup = bind("Setup")?;
        let cpu_setup = bind("CpuSetup")?;
        if setup.offset == cpu_setup.offset {
            return Err(Error::Unsupported(
                "Setup and CpuSetup bind to the same record".into(),
            ));
        }
        // Second anchor: Setup must be the first record (adapter layout rule for this family).
        if profile.records.first().map(|r| r.offset) != Some(setup.offset) {
            return Err(Error::Unsupported(
                "Setup record is not the first record; layout differs from the adapter".into(),
            ));
        }

        // Two locks: the experimental writing switch, then the approvals file.
        let writing = settings::writing_enabled();
        let image_ok = approvals::active().is_approved(&id.bios_id, &schema.sha256, &id.board);
        let write_blocked = if !writing {
            let reason = settings::writing_off_reason();
            diagnostics.push(Diagnostic::new(
                "writing_disabled",
                Severity::Info,
                "pairing",
                format!("Read-only: {reason}."),
            ));
            Some(reason)
        } else if !image_ok {
            let source = approvals::active().describe();
            diagnostics.push(
                Diagnostic::new(
                    "image_not_approved",
                    Severity::Warning,
                    "pairing",
                    format!(
                        "This BIOS image is not approved for writing, so settings are inspect-only ({source})."
                    ),
                )
                .role("bios"),
            );
            Some(format!(
                "this BIOS image is not approved for writing ({source})"
            ))
        } else {
            None
        };
        diagnostics.push(Diagnostic::new(
            "identity_is_evidence",
            Severity::Info,
            "pairing",
            "OCB headers carry no firmware digest; a matching identifier is compatibility evidence, not proof of provenance.",
        ));

        Ok(Session {
            schema,
            profile,
            profile_sha256,
            setup,
            cpu_setup,
            write_blocked,
            diagnostics,
            bios_path: None,
            profile_path: None,
        })
    }

    pub fn with_paths(mut self, bios: Option<String>, profile: Option<String>) -> Self {
        self.bios_path = bios;
        self.profile_path = profile;
        self
    }

    /// Writing allowed: the experimental switch is on and the image is approved.
    pub fn approved(&self) -> bool {
        self.write_blocked.is_none()
    }

    /// Why writing is not allowed for this pair (generic text when it is).
    pub fn blocked_reason(&self) -> String {
        self.write_blocked
            .clone()
            .unwrap_or_else(|| "writing is allowed".into())
    }

    pub fn record_for(&self, store: &str) -> Option<&Record> {
        match store {
            "Setup" => Some(&self.setup),
            "CpuSetup" => Some(&self.cpu_setup),
            _ => None,
        }
    }

    pub fn binding(&self) -> Binding {
        Binding {
            bios_id: self.schema.identity.bios_id.clone(),
            bios_sha256: self.schema.sha256.clone(),
            profile_sha256: self.profile_sha256.clone(),
            schema_fingerprint: self.schema.fingerprint.clone(),
            adapter_id: ADAPTER_ID.into(),
            adapter_version: ADAPTER_VERSION.into(),
        }
    }

    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            metadata_readable: true,
            settings_inspectable: true,
            changes_plannable: true,
            container_writer: if self.approved() {
                "fixture_consistent: additive checksum recomputed, metadata preserved".into()
            } else {
                "unavailable: image not approved".into()
            },
            import_acceptance: "not_demonstrated".into(),
            image_approved_for_writing: self.approved(),
            approval_source: if self.approved() {
                approvals::active().source.clone()
            } else {
                None
            },
        }
    }

    pub fn bios_summary(&self) -> BiosSummary {
        bios_summary(&self.schema, self.bios_path.clone())
    }

    pub fn profile_summary(&self) -> ProfileSummary {
        let p = &self.profile;
        ProfileSummary {
            path: self.profile_path.clone(),
            sha256: self.profile_sha256.clone(),
            bios_id: p.bios_id.clone(),
            cpu_brand: p.oci.cpu_brand.clone(),
            saved_at: p.oci.saved_at.clone(),
            record_count: p.records.len(),
            checksum_stored: p.stored_checksum,
            checksum_computed: p.computed_checksum().unwrap_or_default(),
            checksum_valid: p.checksum_valid(),
            setup_record: StoreSpan {
                store: "Setup".into(),
                record_id: self.setup.id,
                file_offset: self.setup.offset,
                size: self.setup.size,
            },
            cpu_setup_record: StoreSpan {
                store: "CpuSetup".into(),
                record_id: self.cpu_setup.id,
                file_offset: self.cpu_setup.offset,
                size: self.cpu_setup.size,
            },
        }
    }
}

pub fn bios_summary(schema: &BiosSchema, path: Option<String>) -> BiosSummary {
    let id = &schema.identity;
    BiosSummary {
        path,
        bios_id: id.bios_id.clone(),
        board: id.board.clone(),
        flavor: id.flavor.clone(),
        revision: id.revision.clone(),
        build_date: id.build_date.clone(),
        sha256: schema.sha256.clone(),
        schema_fingerprint: schema.fingerprint.clone(),
        approved_for_writing: approvals::active().is_approved(
            &id.bios_id,
            &schema.sha256,
            &id.board,
        ),
        setup_module: format!(
            "{} ({})",
            schema
                .setup_module
                .name
                .clone()
                .unwrap_or_else(|| "unnamed".into()),
            schema.setup_module.file_guid
        ),
        question_count: schema.questions.len(),
        decoded_question_count: schema.decoded_questions().count(),
        defaults: DefaultsInfo {
            available: schema.std_defaults.is_some(),
            source: schema.std_defaults_note.clone(),
        },
        platform: Some(platform_info(&schema.platform, &id.board)),
        profiles_supported: schema.platform.profiles_supported,
    }
}

/// Whether profiles from `board` were ever examined. Display only; `None` when profiles are not
/// supported at all.
pub fn profile_evidence(p: &Platform, board: &str) -> Option<ProfileEvidence> {
    p.profiles_supported.then(|| {
        if crate::catalog::ADAPTER_BOARDS.contains(&board) {
            ProfileEvidence::Examined
        } else {
            ProfileEvidence::Untested
        }
    })
}

/// The one-line caution printed after a verified pair when the board's profiles were never
/// examined; `None` otherwise.
pub fn untested_board_note(evidence: Option<ProfileEvidence>) -> Option<&'static str> {
    (evidence == Some(ProfileEvidence::Untested)).then_some(
        "note: no profile from this board has been examined; the layout is assumed from MS-7D91. Check a few values against your BIOS screens.",
    )
}

pub fn platform_info(p: &Platform, board: &str) -> PlatformInfo {
    PlatformInfo {
        profile_evidence: profile_evidence(p, board),
        vendor: match p.vendor {
            Vendor::Intel => "intel".into(),
            Vendor::Amd => "amd".into(),
        },
        family: p.family.clone(),
        chipset: p.chipset.clone(),
        models: p.models.clone(),
        version: p.version.clone(),
        parser: p.parser.into(),
        profiles_note: p.profiles_note.clone(),
    }
}

/// Summary of an image opened on its own. Uses the decode schema when the family has one, so
/// an Intel image reads the same here as when paired; otherwise describes the browsable view.
pub fn image_summary(
    img: &BiosImage,
    schema: Option<&BiosSchema>,
    path: Option<String>,
) -> BiosSummary {
    if let Some(s) = schema {
        return bios_summary(s, path);
    }
    let id = &img.identity;
    let main = img.formsets.first();
    BiosSummary {
        path,
        bios_id: id.bios_id.clone(),
        board: id.board.clone(),
        flavor: id.flavor.clone(),
        revision: id.revision.clone(),
        build_date: id.build_date.clone(),
        sha256: img.sha256.clone(),
        schema_fingerprint: String::new(),
        approved_for_writing: false,
        setup_module: main
            .map(|f| {
                format!(
                    "{} ({})",
                    f.module.name.clone().unwrap_or_else(|| "unnamed".into()),
                    f.module.file_guid
                )
            })
            .unwrap_or_default(),
        question_count: img
            .formsets
            .iter()
            .filter(|f| f.set == 0)
            .map(|f| f.questions.len())
            .sum(),
        decoded_question_count: 0,
        defaults: DefaultsInfo {
            available: img
                .formsets
                .iter()
                .any(|f| f.set == 0 && !f.defaults.is_empty()),
            source: img.defaults_note.clone(),
        },
        platform: Some(platform_info(&img.platform, &id.board)),
        profiles_supported: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::firmware::platform::PARSER_INTEL;

    fn platform(profiles_supported: bool) -> Platform {
        Platform {
            vendor: Vendor::Intel,
            family: "Intel".into(),
            chipset: None,
            models: None,
            version: String::new(),
            parser: PARSER_INTEL,
            profiles_supported,
            profiles_note: String::new(),
        }
    }

    #[test]
    fn profile_evidence_follows_the_reviewed_boards_only() {
        let p = platform(true);
        assert_eq!(
            profile_evidence(&p, "7D91"),
            Some(ProfileEvidence::Examined)
        );
        assert_eq!(
            profile_evidence(&p, "7D25"),
            Some(ProfileEvidence::Untested)
        );
        assert_eq!(
            profile_evidence(&p, "7E07"),
            Some(ProfileEvidence::Untested)
        );
        // No profile support, no claim either way.
        assert_eq!(profile_evidence(&platform(false), "7D91"), None);
        assert_eq!(profile_evidence(&platform(false), "7D25"), None);
    }

    #[test]
    fn the_caution_line_appears_only_for_untested_boards() {
        assert!(untested_board_note(Some(ProfileEvidence::Untested)).is_some());
        assert!(untested_board_note(Some(ProfileEvidence::Examined)).is_none());
        assert!(untested_board_note(None).is_none());
    }

    #[test]
    fn the_field_is_omitted_when_absent_and_snake_case_when_present() {
        let json =
            |p: &Platform, b: &str| serde_json::to_value(platform_info(p, b)).expect("serialize");
        assert!(
            json(&platform(false), "7D91")
                .get("profile_evidence")
                .is_none()
        );
        assert_eq!(
            json(&platform(true), "7D91")["profile_evidence"],
            "examined"
        );
        assert_eq!(
            json(&platform(true), "7D25")["profile_evidence"],
            "untested"
        );
    }
}
