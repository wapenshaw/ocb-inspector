//! Where an image keeps its OC-profile code. Read-only, structural: modules are found by the
//! signatures they hold, never executed or disassembled here.
//!
//! Facts from the research (docs/FIRMWARE_PROFILE_FORMAT_RESEARCH.md): the setup browser
//! (AMITSE) holds the profile save and load routines and the `$MOS$` signature they write. The
//! `$OCI$` trailer is built by a separate module on MS-7D91 (OcProfileInfo, which also holds
//! `$MOS$`); on the AM5 and Intel 800-series images of the corpus the setup browser itself holds
//! and references `$OCI$`. An image with `$OCI$` in none of its modules is expected to write
//! files that end after the 256-byte CMOS block (not traced).

use super::volume::Walk;

/// A module that holds one or both profile signatures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRef {
    pub name: String,
    pub guid: String,
    /// Top-level volume (`fv@0x…`), for matching a firmware set.
    pub root: String,
    pub sha256: String,
    /// Holds `$MOS$`, the signature profiles start with.
    pub mos: bool,
    /// Holds `$OCI$`, the trailer signature.
    pub oci: bool,
}

/// Profile code found in an image.
#[derive(Debug, Clone, Default)]
pub struct ProfileModules {
    pub modules: Vec<ModuleRef>,
}

impl ProfileModules {
    fn in_roots<'a>(
        &'a self,
        roots: Option<&'a std::collections::BTreeSet<&str>>,
    ) -> impl Iterator<Item = &'a ModuleRef> {
        self.modules
            .iter()
            .filter(move |m| roots.is_none_or(|r| r.contains(m.root.as_str())))
    }

    /// Whether code in one of `roots` (every root when `None`) holds the `$OCI$` signature.
    pub fn has_trailer_in(&self, roots: Option<&std::collections::BTreeSet<&str>>) -> bool {
        self.in_roots(roots).any(|m| m.oci)
    }

    /// Whether code in one of `roots` holds the `$MOS$` signature.
    pub fn has_profile_code_in(&self, roots: Option<&std::collections::BTreeSet<&str>>) -> bool {
        self.in_roots(roots).any(|m| m.mos)
    }
}

const MOS: &[u8] = b"$MOS$\0";
const OCI: &[u8] = b"$OCI$\0";

fn holds(b: &[u8], sig: &[u8]) -> bool {
    b.windows(sig.len()).any(|w| w == sig)
}

/// Every PE32 module of the walk that holds `$MOS$` or `$OCI$`.
pub fn find(walk: &Walk) -> ProfileModules {
    let mut out = ProfileModules::default();
    for f in &walk.files {
        for r in &f.pe32 {
            let b = walk.bytes_of(r);
            let (mos, oci) = (holds(b, MOS), holds(b, OCI));
            if mos || oci {
                out.modules.push(ModuleRef {
                    name: f.name.clone().unwrap_or_default(),
                    guid: f.guid.clone(),
                    root: super::root_of(&f.path).to_string(),
                    sha256: crate::bytes::sha256_hex(b),
                    mos,
                    oci,
                });
            }
        }
    }
    out
}

/// Whole-image SHA-256 and BIOS id of releases whose profile save/load code was traced
/// (E7D91IMS.HJ0) or is instruction-for-instruction the same with addresses masked (research
/// survey of the local corpus). Labels evidence; it never decides a mapping on its own.
pub const TRACED_IMAGES: &[(&str, &str)] = &[
    (
        "26343bb33f13780e721526296655ac2597108ed494cc36602fd4cb43261b10a1",
        "E7D26IMS.1Q0",
    ),
    (
        "ca16831235fd0fa81e70f51ae11d224d6e3227e3c3cdf7c434cfbb385a1e3b45",
        "E7D26IMS.1R0",
    ),
    (
        "b1a9870522bba063a71f90b4b6240b30b56e6be55ffd9e80ff2b5276de6bd4b6",
        "E7D33IMS.1M0",
    ),
    (
        "eb01848bf2cf696a89e66477815555b05bbac6f5315b816fbf8a1914a028b57a",
        "E7D33IMS.1N0",
    ),
    (
        "d6be30735eeefa81a93159ed319a24b27e5ebee08923c23090d25dbe4cd1ea5a",
        "E7D37IMS.1N0",
    ),
    (
        "1447b791cda0a7510b26ef0fe64a350904d04e1ee4b0b95966b2eb39d15c19f2",
        "E7D37IMS.1O0",
    ),
    (
        "6cba9e17f8caeb15204baca01f0b296d56564f8d21c306ab4650c64fba868471",
        "E7D37IMS.2F0",
    ),
    (
        "f1163cafa9649d4062359298ba33479636c0ecbdbfa1a183a32bfa0794dca8ec",
        "E7D37IMS.2G0",
    ),
    (
        "69aa483fcdf5c9528941283ca5158770de9191faa086e17e22af50600aab4104",
        "E7D91IMS.HI0",
    ),
    (
        "fdd8bbf24b06370478261a58b8034dca7d713aea4e41be0e2080efe9fd5b38fe",
        "E7D91IMS.HJ0",
    ),
    (
        "77ca247f27a9f7b42d6beac9ea6bd544878cd579cb9f2aaf0d21052fc7dd9da6",
        "E7D99IMS.Q10",
    ),
    (
        "15a577e070635feae9550deb4d24d314b7bf0008c9fe9bc29b0ffd366ebb614c",
        "E7D99IMS.Q20",
    ),
    (
        "9b8e67c3123fc35ed3082619f2208b6295b81260119e488d0879834a2a0798ec",
        "E7E07IMS.MA0",
    ),
    (
        "bf40f12d7d6cfef81622b87183f5d8e2dee3c22e1d3a59ffb398293fa1975954",
        "E7E07IMS.MB0",
    ),
    (
        "e4cc66f83980a1f4411ee067c9a6d8ef0a83a9591fbfe401228478672c5f0b00",
        "E7E11IMS.A20",
    ),
    (
        "4817dd3605d59392f216fe2cd46d2ccbc846587bc9591eff4a2379278666efe9",
        "E7E11IMS.A30",
    ),
    (
        "0e6999de5772a4c79083d233b1f299a047382eda560d27ca233f18715190f6a5",
        "E7E44IMS.320",
    ),
    (
        "876612a497218deb06e5d76014cdc34139b6beb936eefd1d43c951a2f0360e16",
        "E7E44IMS.330",
    ),
    (
        "275706a41b5b32dba20c5874d5268fa31a7e4025a84c9881ebde1883968e4a51",
        "E7E46IMS.A10",
    ),
    (
        "de39cb10d78fdd77502445fc7f10740417e5302c95dd4bece318eb688983ec53",
        "E7E46IMS.A20",
    ),
];

/// The release's profile save/load code was traced, or is the same code as a traced release.
pub fn traced(image_sha256: &str) -> bool {
    TRACED_IMAGES.iter().any(|(h, _)| *h == image_sha256)
}

/// Traced releases whose `$OCI$` builder is the same code as HJ0's OcProfileInfo (survey
/// fingerprints); the other traced releases build it with different code.
const TRAILER_TRACED: &[&str] = &[
    "E7D91IMS.HI0",
    "E7D91IMS.HJ0",
    "E7E11IMS.A20",
    "E7E11IMS.A30",
];

/// The release's `$OCI$` trailer builder was traced, or is the same code as a traced builder.
pub fn trailer_traced(image_sha256: &str) -> bool {
    TRACED_IMAGES
        .iter()
        .any(|(h, id)| *h == image_sha256 && TRAILER_TRACED.contains(id))
}

/// Facts the traced save and load routines establish (HJ0 AMITSE save `0x82420`, load
/// `0x82D98`). Valid only for `traced` images.
pub mod traced_facts {
    /// List positions the save routine never writes (skip mask 0x400000000000998 plus 12).
    pub const SKIPPED_INDICES: &[u32] = &[3, 4, 7, 8, 11, 12, 58];
    /// Variable the save routine skips by name.
    pub const SKIPPED_NAME: &str = "OcInfoPassToSetup";
    /// List positions whose record may differ from the live size on load (allow-list bits
    /// 0x1800000040A002), besides the variables named FixedBoot and FixedBootGroup.
    pub const SIZE_TOLERANT_INDICES: &[u32] = &[1, 13, 15, 22, 51, 52];
    pub const SIZE_TOLERANT_NAMES: &[&str] = &["FixedBoot", "FixedBootGroup"];
    /// CMOS bytes the load routine writes back from the file's 256-byte block.
    pub const CMOS_RESTORED: (u8, u8) = (0x40, 0xFF);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traced_digests_are_full_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for (h, id) in TRACED_IMAGES {
            assert_eq!(h.len(), 64, "{id}");
            assert!(h.bytes().all(|c| c.is_ascii_hexdigit()), "{id}");
            assert!(seen.insert(*h), "{id} twice");
        }
        assert!(traced(TRACED_IMAGES[0].0));
        assert!(!traced(&"0".repeat(64)));
    }

    #[test]
    fn signatures_are_found_whole_not_as_prefixes() {
        assert!(holds(b"xx$MOS$\0yy", MOS));
        assert!(!holds(b"xx$MOS$yy", MOS));
        assert!(!holds(b"$OC", OCI));
    }
}
