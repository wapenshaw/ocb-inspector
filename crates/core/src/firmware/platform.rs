//! Firmware families: which parser reads an image, and how the image is described.
//!
//! The parser is chosen from the firmware's structure, never from file names: an Intel image
//! keeps its CPU settings in a `CpuSetup` store inside the AMI Setup formset, while an AMD AM5
//! image spreads them over AMD CBS and AMD Overclocking formsets in separate modules. Board
//! names and chipsets come from a short table of boards examined so far and are labels only.

use super::BiosIdentity;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Vendor {
    Intel,
    Amd,
}

impl Vendor {
    pub fn name(self) -> &'static str {
        match self {
            Vendor::Intel => "Intel",
            Vendor::Amd => "AMD",
        }
    }
}

/// Parser ids. Each family has its own selection of formsets and its own profile rules.
pub const PARSER_INTEL: &str = "msi-intel-setup";
pub const PARSER_AMD: &str = "msi-amd-cbs";

#[derive(Debug, Clone, Serialize)]
pub struct Platform {
    pub vendor: Vendor,
    /// Socket family, e.g. "Intel LGA1700", or the vendor alone for boards not in the table.
    pub family: String,
    pub chipset: Option<String>,
    /// Retail board names sharing this BIOS, from the vendor release notes.
    pub models: Option<String>,
    /// MSI's version label, e.g. "V17.J0" for revision HJ0.
    pub version: String,
    pub parser: &'static str,
    /// Whether OC profiles saved by this BIOS can be opened at all.
    pub profiles_supported: bool,
    pub profiles_note: String,
}

/// (board project, major version when one project id covers several boards, chipset, retail
/// names) from the vendor release notes of the images examined. Labels only.
const KNOWN_BOARDS: &[(&str, Option<u32>, &str, &str)] = &[
    ("7A94", None, "X299", "X299-A PRO / X299-S01A"),
    ("7B96", None, "X299", "Creator X299"),
    ("7C02", None, "B450", "B450 TOMAHAWK MAX II"),
    ("7C52", Some(1), "A320", "A320M-A PRO M2"),
    ("7C52", Some(4), "A320", "PRO A320M-B"),
    ("7C52", Some(10), "B450", "B450M-A PRO MAX II"),
    ("7C59", None, "TRX40", "Creator TRX40"),
    ("7C60", None, "TRX40", "TRX40 PRO 10G / TRX40-A PRO"),
    (
        "7C73",
        None,
        "Z490",
        "MPG Z490 GAMING CARBON WIFI / MPG Z490 Carbon EK X",
    ),
    ("7C81", None, "B460", "MAG B460 TORPEDO"),
    ("7C83", None, "B460", "B460M PRO-VDH WIFI / B460M PRO-VDH"),
    ("7C85", None, "H410", "H410M PRO-E"),
    ("7C91", None, "B550", "MAG B550 TOMAHAWK WIFI"),
    ("7C96", None, "A520", "PRO A520M-S"),
    (
        "7C98",
        None,
        "Z490",
        "Z490-S01 / Z490 PLUS / Z490 PLUS WIFI",
    ),
    ("7D02", None, "H310", "H310M PRO-C"),
    ("7D09", None, "Z590", "Z590 PRO WIFI (CEC)"),
    ("7D11", None, "Z590", "Z590 PLUS / Z590-S01"),
    ("7D14", Some(1), "A520", "A520M PRO / PRO A520M WIFI6E"),
    ("7D14", Some(2), "B550", "B550M PRO / PRO B550M WIFI6E"),
    ("7D15", None, "B560", "B560-A PRO"),
    ("7D18", None, "B560", "B560M PRO-VDH WIFI (CEC)"),
    ("7D25", None, "H670", "MAG H670 TOMAHAWK WIFI DDR4"),
    ("7D26", None, "Z690", "MEG Z690 GODLIKE"),
    ("7D33", None, "Z690", "PRO Z690-P WIFI"),
    ("7D37", Some(1), "B660", "PRO B660M-A CEC WIFI DDR4"),
    ("7D37", Some(2), "B660", "PRO B660M-A CEC WIFI DDR4 V2"),
    ("7D50", None, "X570", "MEG X570S ACE MAX"),
    ("7D51", None, "X570", "MEG X570S UNIFY-X MAX"),
    ("7D67", None, "X670", "PRO X670-P WIFI"),
    ("7D82", None, "H410", "PRO H410M-B / H510M PLUS"),
    (
        "7D91",
        None,
        "Z790",
        "MPG Z790 EDGE WIFI / MAG Z790 TOMAHAWK WIFI",
    ),
    ("7D99", None, "B760", "B760M GAMING PLUS WIFI GEN5 II"),
    ("7E05", Some(2), "H510", "PRO H510M-B II"),
    ("7E05", Some(17), "H510", "H510M PLUS II"),
    ("7E07", None, "Z790", "PRO Z790-A MAX WIFI"),
    ("7E11", None, "Z790", "Z790M GAMING PLUS WIFI"),
    ("7E12", None, "X670E", "MAG X670E TOMAHAWK WIFI"),
    ("7E16", None, "X670E", "X670E GAMING PLUS WIFI"),
    (
        "7E26",
        None,
        "B650",
        "B650 GAMING PLUS WIFI / B650 GAMING PLUS / PRO B650-S WIFI",
    ),
    ("7E30", None, "A620", "A620M GAMING"),
    ("7E34", None, "Z890", "PRO Z890-P WIFI / Z890-W WIFI"),
    ("7E38", None, "H810", "PRO H810M-C EX"),
    ("7E42", Some(1), "B860", "PRO B860M-A WIFI6E"),
    ("7E42", Some(2), "B860", "B860M GAMING PLUS WIFI6E"),
    ("7E42", Some(3), "Z890", "MAG Z890M GAMING PLUS WIFI"),
    (
        "7E44",
        None,
        "H610",
        "PRO H610M-S DDR4 II / PRO H610M-S WIFI DDR4 II",
    ),
    ("7E46", None, "B760", "PRO B760M-X III"),
    (
        "7E47",
        None,
        "X870",
        "X870 GAMING PLUS WIFI BULK / X870 GAMING PLUS WIFI",
    ),
    (
        "7E48",
        None,
        "X870E",
        "MEG X870E GODLIKE X EDITION / MEG X870E GODLIKE MAX",
    ),
    ("7E49", None, "X870E", "MPG X870E CARBON WIFI"),
    ("7E50", None, "X870", "MPG X870I EDGE TI EVO WIFI"),
    (
        "7E54",
        None,
        "Z890",
        "PRO Z890-S WIFI / PRO Z890-VC WIFI6E / PRO Z890-VC WIFI6E W",
    ),
    ("7E56", None, "B850", "MAG B850 GAMING PLUS MAX WIFI S"),
    ("7E59", None, "X870E", "MAG X870E TOMAHAWK MAX WIFI"),
    ("7E70", None, "X870E", "MAG X870E GAMING PLUS MAX WIFI S"),
    ("7E72", None, "H810", "PRO H810M-E / PRO H810M-E WIFI6E"),
    ("7E85", None, "X870E", "MEG X870E ACE MAX"),
    ("7E90", None, "B840", "PRO B840M-P EVO WIFI6E PZ"),
    ("7E92", None, "B850", "MPG B850M EDGE TI MAX WIFI (Rev 2)"),
    ("7E93", None, "B840", "MAG B840 GAMING EVO WIFI6E"),
    (
        "7E96",
        None,
        "A620",
        "MAG A620AM GAMING EVO WIFI5 / MAG A620AM GAMING EVO WIFI",
    ),
    ("7E97", None, "B650E", "B650E GAMING WIFI"),
];

/// Chipset -> (vendor, socket, generation). Labels only; the parser is still chosen by structure.
fn chipset_family(chipset: &str) -> Option<(Vendor, &'static str, &'static str)> {
    use Vendor::{Amd, Intel};
    Some(match chipset {
        "X299" => (Intel, "LGA2066", "X299 HEDT"),
        "H310" | "B360" | "B365" | "H370" | "Z370" | "Z390" => (Intel, "LGA1151", "300 series"),
        "H410" | "B460" | "H470" | "Z490" => (Intel, "LGA1200", "400 series"),
        "H510" | "B560" | "H570" | "Z590" => (Intel, "LGA1200", "500 series"),
        "H610" | "B660" | "H670" | "Z690" => (Intel, "LGA1700", "600 series"),
        "B760" | "H770" | "Z790" => (Intel, "LGA1700", "700 series"),
        "H810" | "B860" | "Z890" => (Intel, "LGA1851", "800 series"),
        "A320" | "B350" | "X370" | "B450" | "X470" | "A520" | "B550" | "X570" => {
            (Amd, "AM4", "AM4")
        }
        "TRX40" => (Amd, "sTRX4", "TRX40 HEDT"),
        "A620" | "B650" | "B650E" | "X670" | "X670E" | "B840" | "B850" | "X870" | "X870E" => {
            (Amd, "AM5", "AM5")
        }
        _ => return None,
    })
}

fn known_board(identity: &BiosIdentity) -> Option<(&'static str, &'static str)> {
    let major = identity
        .revision
        .chars()
        .next()
        .and_then(|c| c.to_digit(36));
    KNOWN_BOARDS
        .iter()
        .find(|(b, m, _, _)| *b == identity.board && (m.is_none() || *m == major))
        .map(|(_, _, c, models)| (*c, *models))
}

/// MSI version label from the revision, as the release notes print it: the first character is
/// the major version in base 36 (`H` = 17); a 3-character revision drops a trailing `0`.
/// `H85` -> `V17.85`, `Q10` -> `V26.1`, `HO0` -> `V17.O`, `1AF0` -> `V1.AF0`.
pub fn version_label(revision: &str) -> String {
    let mut chars = revision.chars();
    match chars.next().and_then(|c| c.to_digit(36)) {
        Some(major) => {
            let rest = chars.as_str();
            let minor = match rest.strip_suffix('0') {
                Some(m) if revision.len() == 3 && !m.is_empty() => m,
                _ => rest,
            };
            format!("V{major}.{minor}")
        }
        None => revision.to_string(),
    }
}

/// What to open when a profile was saved by another BIOS than the one supplied: the saved-by id,
/// with MSI's version label when the id has the usual shape. A sentence, ready to append.
pub fn open_instead(bios_id: &str) -> String {
    match super::parse_bios_id(bios_id) {
        Some((_, _, rev)) => format!(
            "Open the BIOS image {bios_id} (MSI version {}) instead.",
            version_label(&rev)
        ),
        None => format!("Open the BIOS image {bios_id} instead."),
    }
}

/// The sentence for a profile saved by `saved_by` but paired with the image `image_id`,
/// ending with what to open instead.
pub fn identity_mismatch(saved_by: &str, image_id: &str) -> String {
    format!(
        "profile was saved by {saved_by} but the BIOS image is {image_id}. {}",
        open_instead(saved_by)
    )
}

/// Describe an image whose vendor was established from its structure. Returns notes for
/// anything that disagrees (table or identifier flavor naming another vendor).
pub fn describe(identity: &BiosIdentity, vendor: Vendor) -> (Platform, Vec<String>) {
    let mut notes = Vec::new();
    let known = known_board(identity);
    let family = known.and_then(|(c, _)| chipset_family(c));
    if let Some((v, _, _)) = family
        && v != vendor
    {
        notes.push(format!(
            "board {} is listed as {} but the firmware structure is {}",
            identity.board,
            v.name(),
            vendor.name()
        ));
    }
    let flavor_vendor = match identity.flavor.chars().next() {
        Some('I') => Some(Vendor::Intel),
        Some('A') => Some(Vendor::Amd),
        _ => None,
    };
    if flavor_vendor.is_some_and(|v| v != vendor) {
        notes.push(format!(
            "BIOS flavor {} suggests another vendor than the {} firmware structure",
            identity.flavor,
            vendor.name()
        ));
    }
    let (known, family) = match family {
        Some(f) if f.0 == vendor => (known, Some(f)),
        _ => (None, None),
    };
    let short_id = identity.bios_id.len() <= 12;
    let (parser, profiles_supported, profiles_note) = match vendor {
        Vendor::Intel if short_id => (
            PARSER_INTEL,
            true,
            if identity.board == "7D91" {
                "OC profiles saved by this BIOS can be opened.".to_string()
            } else {
                "OC profiles can be opened if their layout matches the MS-7D91 profiles examined so far; \
                 pairing checks reject any mismatch."
                    .to_string()
            },
        ),
        Vendor::Intel => (
            PARSER_INTEL,
            false,
            format!(
                "OC profiles store a 12-character BIOS ID, and {} has 14, so profiles from this BIOS use a \
                 layout that has not been examined yet. Browse the BIOS instead.",
                identity.bios_id
            ),
        ),
        Vendor::Amd => (
            PARSER_AMD,
            false,
            "No AMD OC profile has been examined yet, so profiles from this BIOS cannot be opened. \
             Browse the BIOS instead."
                .to_string(),
        ),
    };
    let platform = Platform {
        vendor,
        family: match family {
            Some((_, socket, generation)) if generation == socket => {
                format!("{} {socket}", vendor.name())
            }
            Some((_, socket, generation)) => format!("{} {socket}, {generation}", vendor.name()),
            None => format!("{} (board not in the examined list)", vendor.name()),
        },
        chipset: known.map(|(c, _)| c.to_string()),
        models: known.map(|(_, m)| m.to_string()),
        version: version_label(&identity.revision),
        parser,
        profiles_supported,
        profiles_note,
    };
    (platform, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_instead_names_the_bios_and_msi_version() {
        assert_eq!(
            open_instead("E7D91IMS.HI0"),
            "Open the BIOS image E7D91IMS.HI0 (MSI version V17.I) instead."
        );
        assert_eq!(
            open_instead("E7E34IMSI.1AF0"),
            "Open the BIOS image E7E34IMSI.1AF0 (MSI version V1.AF0) instead."
        );
        assert_eq!(
            open_instead("garbage"),
            "Open the BIOS image garbage instead."
        );
    }

    #[test]
    fn version_labels_match_release_notes() {
        assert_eq!(version_label("H85"), "V17.85");
        assert_eq!(version_label("1AF0"), "V1.AF0");
        assert_eq!(version_label("2A35"), "V2.A35");
        assert_eq!(version_label("5A33"), "V5.A33");
        assert_eq!(version_label("Q10"), "V26.1");
        assert_eq!(version_label("HO0"), "V17.O");
        assert_eq!(version_label("H00"), "V17.0");
        assert_eq!(version_label("1A50"), "V1.A50");
    }
}
