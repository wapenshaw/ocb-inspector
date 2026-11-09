//! Reviewed semantic recipes for the `msi-7d91` adapter (SPEC §5, §6, §12).
//!
//! A recipe binds a semantic setting ID to physical fields, located in each BIOS schema by
//! anchor prompts, and to a codec. Write approval comes from the runtime approvals file
//! (`approvals`), bound to complete image digests and to this adapter's boards.
//! Evidence for every recipe is cited in `evidence`; see `analysis/evidence/` and SPEC §2.

pub const ADAPTER_ID: &str = "msi-7d91";
pub const ADAPTER_VERSION: &str = "1";
pub const RECIPE_SET_VERSION: &str = "1";

/// Boards these recipes were reviewed on. An approvals-file entry for this adapter only takes
/// effect for these boards (see `approvals`).
pub const ADAPTER_BOARDS: &[&str] = &["7D91"];

/// A physical field located by anchor prompts within one store.
#[derive(Debug, Clone, Copy)]
pub struct FieldRef {
    pub store: &'static str,
    /// Accepted prompts, in priority order. The first one present supplies enum labels.
    pub prompts: &'static [&'static str],
    pub width_bits: u16,
    /// Raw values whose meaning the codec or a dependency rule relies on, with the labels the
    /// reviewed releases give them. Resolution refuses an image where any of these raws is
    /// missing or labelled differently (another generation can reuse the prompt, not the meaning).
    pub reviewed: &'static [(u64, &'static [&'static str])],
    /// Enum fields only: accept whatever width the IFR declares (8 or 16 bits). Decoding and
    /// encoding use the image's own option table and width, so only labels carry meaning.
    pub any_width: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum Codec {
    /// Enumerated value. Raw meaning is reviewed as identical across approved releases where the
    /// same raw value carries the same label; `cross_map` lists reviewed raw equivalences where labels differ.
    Enum {
        field: FieldRef,
        cross_map: Option<&'static [(u64, u64)]>,
    },
    /// Integer in `unit`, raw 0 = Auto.
    AutoZero { field: FieldRef, unit: &'static str },
    /// Two's-complement u8 ratio offset; 0 = Auto; allowed values are the IFR options.
    RatioOffset { field: FieldRef },
    /// Compound signed offset: sign enum (0 Auto, 1 +, 2 -) plus magnitude in `unit`.
    SignedOffset {
        sign: FieldRef,
        magnitude: FieldRef,
        unit: &'static str,
    },
}

impl Codec {
    pub fn fields(&self) -> Vec<FieldRef> {
        match *self {
            Codec::Enum { field, .. }
            | Codec::AutoZero { field, .. }
            | Codec::RatioOffset { field } => vec![field],
            Codec::SignedOffset {
                sign, magnitude, ..
            } => vec![sign, magnitude],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writability {
    Writable,
    /// Only the tagged Auto value may be written; other values are read-only.
    AutoOnly(&'static str),
    ReadOnly(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct Recipe {
    pub id: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub summary: &'static str,
    pub codec: Codec,
    pub writability: Writability,
    pub evidence: &'static [&'static str],
}

const fn f(store: &'static str, prompt: &'static [&'static str], width_bits: u16) -> FieldRef {
    FieldRef {
        store,
        prompts: prompt,
        width_bits,
        reviewed: &[],
        any_width: false,
    }
}

/// A field with reviewed raw meanings (see `FieldRef::reviewed`).
const fn fr(
    store: &'static str,
    prompt: &'static [&'static str],
    width_bits: u16,
    reviewed: &'static [(u64, &'static [&'static str])],
) -> FieldRef {
    FieldRef {
        store,
        prompts: prompt,
        width_bits,
        reviewed,
        any_width: false,
    }
}

const APPLY_MODE: &[(u64, &[&str])] = &[(4, &["Turbo Ratio Offset"])];
const RATIO_OFFSET: &[(u64, &[&str])] = &[(0, &["Auto"]), (1, &["+1"]), (255, &["-1"])];

const EV_ANCHOR: &str = "BIOS visit snapshot anchors match OC-Just-Minus-100 (SPEC §2)";
const EV_SAME: &str = "Identical store/offset/width/options in HI0 and HJ0 IFR";
const EV_FILENAME: &str = "Base_-100_+2P decodes to -105 mV / 235 W, independent of filename";

pub const GROUPS: &[(&str, &str)] = &[
    ("cpu.ratios", "CPU ratios"),
    ("cpu.voltage", "CPU voltage"),
    ("cpu.power", "Power & current limits"),
    ("cpu.loadline", "Lite Load & load lines"),
    ("platform", "Platform"),
];

pub const RECIPES: &[Recipe] = &[
    Recipe {
        id: "cpu.pcore.ratio_apply_mode",
        group: "cpu.ratios",
        label: "P-core ratio apply mode",
        summary: "How P-core ratio changes are applied. A turbo ratio offset needs Turbo Ratio Offset mode.",
        codec: Codec::Enum {
            field: fr("Setup", &["P-Core Ratio Apply Mode"], 8, APPLY_MODE),
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME, "Base_-100_+2P stores 4 with offset +2"],
    },
    Recipe {
        id: "cpu.pcore.turbo_ratio_offset",
        group: "cpu.ratios",
        label: "P-core turbo ratio offset",
        summary: "Signed multiplier offset applied to P-core turbo ratios. Active only in Turbo Ratio Offset mode.",
        codec: Codec::RatioOffset {
            field: fr(
                "Setup",
                &["P-Core Turbo Ratio Offset Value"],
                8,
                RATIO_OFFSET,
            ),
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME, "BeforeUpgrade and Base_-100_+2P store +2 (raw 2)"],
    },
    Recipe {
        id: "cpu.ecore.ratio_apply_mode",
        group: "cpu.ratios",
        label: "E-core ratio apply mode",
        summary: "How E-core ratio changes are applied. A turbo ratio offset needs Turbo Ratio Offset mode.",
        codec: Codec::Enum {
            field: fr("Setup", &["E-Core Ratio Apply Mode"], 8, APPLY_MODE),
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME],
    },
    Recipe {
        id: "cpu.ecore.turbo_ratio_offset",
        group: "cpu.ratios",
        label: "E-core turbo ratio offset",
        summary: "Signed multiplier offset applied to E-core turbo ratios. Active only in Turbo Ratio Offset mode.",
        codec: Codec::RatioOffset {
            field: fr(
                "Setup",
                &["E-Core Turbo Ratio Offset Value"],
                8,
                RATIO_OFFSET,
            ),
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME, "BeforeUpgrade stores +2 (raw 2)"],
    },
    Recipe {
        id: "cpu.core.voltage_mode",
        group: "cpu.voltage",
        label: "CPU core voltage mode",
        summary: "Core voltage control mode. A core voltage offset needs Offset or Adaptive + Offset mode.",
        codec: Codec::Enum {
            field: fr(
                "Setup",
                &["CPU Core Voltage Mode"],
                8,
                &[(3, &["Offset Mode"]), (4, &["Adaptive + Offset Mode"])],
            ),
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME, EV_ANCHOR],
    },
    Recipe {
        id: "cpu.core.voltage_offset",
        group: "cpu.voltage",
        label: "CPU core voltage offset",
        summary: "Requested core voltage offset. Sign and magnitude are stored separately and written together.",
        codec: Codec::SignedOffset {
            sign: fr(
                "Setup",
                &["CPU Core Voltage Offset Mode"],
                8,
                &[(0, &["Auto"]), (1, &["+"]), (2, &["-"])],
            ),
            magnitude: f("Setup", &["CPU Core Voltage Offset"], 16),
            unit: "mV",
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME, EV_ANCHOR, EV_FILENAME],
    },
    Recipe {
        id: "cpu.vr_voltage_limit",
        group: "cpu.voltage",
        label: "CPU VR voltage limit",
        summary: "Maximum voltage the VR may be requested to deliver. Not a measured Vcore ceiling.",
        codec: Codec::AutoZero {
            field: f("CpuSetup", &["CPU VR Voltage Limit"], 16),
            unit: "mV",
        },
        writability: Writability::Writable,
        evidence: &[
            EV_SAME,
            "OC-Just-Minus-100 stores 1400 = 1.400 V seen in BIOS",
        ],
    },
    Recipe {
        id: "cpu.under_voltage_protection",
        group: "cpu.voltage",
        label: "CPU under-voltage protection",
        summary: "Firmware protection against undervolting changes from the OS.",
        codec: Codec::Enum {
            field: f("Setup", &["CPU Under Voltage Protection"], 8),
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME],
    },
    Recipe {
        id: "cpu.enhanced_turbo",
        group: "cpu.voltage",
        label: "Enhanced Turbo",
        summary: "MSI Enhanced Turbo.",
        codec: Codec::Enum {
            field: f("Setup", &["Enhanced Turbo"], 8),
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME, EV_ANCHOR],
    },
    Recipe {
        id: "cpu.cooler_tuning",
        group: "cpu.power",
        label: "CPU cooler tuning",
        summary: "MSI power-limit preset selected for the installed cooler.",
        codec: Codec::Enum {
            field: f("Setup", &["CPU Cooler Tuning"], 8),
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME],
    },
    Recipe {
        id: "cpu.power.pl1",
        group: "cpu.power",
        label: "Long duration power limit (PL1)",
        summary: "Sustained package power limit. Stored in watts, as the MSI prompt states.",
        codec: Codec::AutoZero {
            field: f("CpuSetup", &["Long Duration Power Limit(W)"], 32),
            unit: "W",
        },
        writability: Writability::Writable,
        evidence: &[
            EV_SAME,
            EV_FILENAME,
            "MSI prompt says W; generic Intel alias says mW",
        ],
    },
    Recipe {
        id: "cpu.power.pl2",
        group: "cpu.power",
        label: "Short duration power limit (PL2)",
        summary: "Short-term package power limit. Stored in watts, as the MSI prompt states.",
        codec: Codec::AutoZero {
            field: f("CpuSetup", &["Short Duration Power Limit(W)"], 32),
            unit: "W",
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME, EV_FILENAME],
    },
    Recipe {
        id: "cpu.current_limit",
        group: "cpu.power",
        label: "CPU current limit",
        summary: "IccMax current limit in amperes.",
        codec: Codec::AutoZero {
            field: f("Setup", &["CPU Current Limit(A)"], 16),
            unit: "A",
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME, "OC-Just-Minus-100 stores 307 A seen in BIOS"],
    },
    Recipe {
        id: "cpu.cep",
        group: "cpu.power",
        label: "IA CEP support",
        summary: "Current Excursion Protection.",
        codec: Codec::Enum {
            field: f("Setup", &["IA CEP Support"], 8),
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME, EV_ANCHOR],
    },
    Recipe {
        id: "cpu.lite_load_control",
        group: "cpu.loadline",
        label: "CPU Lite Load control",
        summary: "Selects Lite Load mode presets or explicit AC/DC load lines.",
        codec: Codec::Enum {
            field: f("Setup", &["CPU Lite Load Control"], 8),
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME],
    },
    Recipe {
        id: "cpu.lite_load",
        group: "cpu.loadline",
        label: "CPU Lite Load",
        summary: "Lite Load preset. Auto delegates the AC/DC choice to firmware.",
        codec: Codec::Enum {
            field: f("Setup", &["CPU Lite Load"], 8),
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[EV_SAME],
    },
    Recipe {
        id: "cpu.llc",
        group: "cpu.loadline",
        label: "Loadline calibration",
        summary: "VR loadline calibration mode.",
        codec: Codec::Enum {
            field: FieldRef {
                any_width: true,
                ..f("Setup", &["CPU Loadline Calibration Control"], 16)
            },
            cross_map: None,
        },
        writability: Writability::Writable,
        evidence: &[
            EV_SAME,
            "Other 600/700-series builds declare it 8-bit with their own raw table; decoded by label only",
        ],
    },
    Recipe {
        id: "cpu.ac_loadline",
        group: "cpu.loadline",
        label: "CPU AC load line",
        summary: "Explicit AC load line. Stored unit is unverified, so only Auto can be written.",
        codec: Codec::AutoZero {
            field: f("CpuSetup", &["CPU AC LOADLINE"], 16),
            unit: "raw",
        },
        writability: Writability::AutoOnly("stored unit not verified by controlled saves"),
        evidence: &[EV_SAME, "All fixtures store Auto (0)"],
    },
    Recipe {
        id: "cpu.dc_loadline",
        group: "cpu.loadline",
        label: "CPU DC load line",
        summary: "Explicit DC load line. Stored unit is unverified, so only Auto can be written.",
        codec: Codec::AutoZero {
            field: f("CpuSetup", &["CPU DC LOADLINE"], 16),
            unit: "raw",
        },
        writability: Writability::AutoOnly("stored unit not verified by controlled saves"),
        evidence: &[EV_SAME, "All fixtures store Auto (0)"],
    },
    Recipe {
        id: "platform.iommu",
        group: "platform",
        label: "IOMMU",
        summary: "IOMMU control. HJ0 adds raw 2 (Compatible Mode), which HI0 cannot represent.",
        codec: Codec::Enum {
            field: fr(
                "Setup",
                &["IOMMU Control", "Control IOMMU Pre-boot Behavior"],
                8,
                &[
                    (0, &["Disable IOMMU", "Disabled"]),
                    (1, &["Enable IOMMU during boot", "Enabled"]),
                ],
            ),
            cross_map: Some(&[(0, 0), (1, 1)]),
        },
        writability: Writability::Writable,
        evidence: &[
            "Setup offset 2453; HI0 options 0/1, HJ0 adds 2",
            "Reviewed: 0 = disabled, 1 = enabled in both releases",
        ],
    },
    Recipe {
        id: "platform.high_efficiency_mode",
        group: "platform",
        label: "High Efficiency Mode",
        summary: "HJ0-only. Shares its byte with Memory Extension Mode, so it is inspect-only.",
        codec: Codec::Enum {
            field: f("Setup", &["High Efficiency Mode"], 8),
            cross_map: None,
        },
        writability: Writability::ReadOnly(
            "shares Setup offset 3976 with Memory Extension Mode; alias semantics unresolved",
        ),
        evidence: &[
            "Present only in HJ0 IFR",
            "HJ0 aliases offset 3976 as Memory Extension Mode and High Efficiency Mode",
        ],
    },
];

pub fn recipe(id: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.id == id)
}

/// Recipes whose group equals or is nested under `selector` (e.g. `cpu` selects `cpu.*`).
pub fn select_group(selector: &str) -> Vec<&'static Recipe> {
    RECIPES
        .iter()
        .filter(|r| r.group == selector || r.group.starts_with(&format!("{selector}.")))
        .collect()
}
