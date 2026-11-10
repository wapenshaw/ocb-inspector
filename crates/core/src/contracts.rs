//! Versioned agent-facing documents (SPEC §9): `ocb.report`, `ocb.patch`, `ocb.export-manifest`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "1.0";
pub const PAYLOAD_VERSION: &str = "1.0";
/// Inspect reports are 1.1: additive menu locations and optimized-default comparison.
pub const INSPECT_PAYLOAD_VERSION: &str = "1.1";
pub const KIND_REPORT: &str = "ocb.report";
pub const KIND_PATCH: &str = "ocb.patch";
pub const KIND_MANIFEST: &str = "ocb.export-manifest";

/// Exact accepted/emitted combinations (SPEC §9 support table).
pub fn support_table() -> serde_json::Value {
    serde_json::json!({
        "ocb.report": {"schema_versions": ["1.0"], "payloads": {
            "bios_catalog": ["1.0"], "inspect": ["1.0"], "diff": ["1.0"], "verify": ["1.0"],
            "experimental_inspect": ["1.0"], "profile_format": ["1.0"]}},
        "ocb.patch": {"schema_versions": ["1.0"], "payloads": {"edit": ["1.0"], "migrate": ["1.0"]},
            "accepted_input": {"schema_versions": ["1.0"], "edit": ["1.0"], "migrate": ["1.0"]}},
        "ocb.export-manifest": {"schema_versions": ["1.0"], "payloads": {"edit": ["1.0"], "migrate": ["1.0"]}}
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    pub name: String,
    pub version: String,
}

impl Producer {
    pub fn this() -> Self {
        Producer {
            name: "ocb-inspector".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
}

/// Normalized value union (SPEC §6, §9). Auto is a tag, never zero.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Value {
    Auto,
    Integer {
        value: i64,
        unit: String,
    },
    Enum {
        symbol: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
    Boolean {
        value: bool,
    },
    /// A stored value no reviewed codec option describes. Preserved, never coerced.
    Unknown {
        raw: u64,
    },
}

impl Value {
    /// Semantic equality: enums compare by symbol; display fields are ignored.
    pub fn same_intent(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Enum { symbol: a, .. }, Value::Enum { symbol: b, .. }) => a == b,
            _ => self == other,
        }
    }

    pub fn display(&self) -> String {
        match self {
            Value::Auto => "Auto".into(),
            Value::Integer { value, unit } if unit == "ratio" => format!("{value:+}"),
            Value::Integer { value, unit } => format!("{value} {unit}"),
            Value::Enum { label: Some(l), .. } => l.trim().to_string(),
            Value::Enum { symbol, .. } => symbol.clone(),
            Value::Boolean { value } => value.to_string(),
            Value::Unknown { raw } => format!("unknown raw {raw}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub stage: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setting_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_role: Option<String>,
}

impl Diagnostic {
    pub fn new(code: &str, severity: Severity, stage: &str, message: impl Into<String>) -> Self {
        Diagnostic {
            code: code.into(),
            severity,
            stage: stage.into(),
            message: message.into(),
            setting_id: None,
            input_role: None,
        }
    }
    pub fn setting(mut self, id: &str) -> Self {
        self.setting_id = Some(id.into());
        self
    }
    pub fn role(mut self, role: &str) -> Self {
        self.input_role = Some(role.into());
        self
    }
}

/// Binding of a document to exact immutable inputs.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub bios_id: String,
    pub bios_sha256: String,
    pub profile_sha256: String,
    pub schema_fingerprint: String,
    pub adapter_id: String,
    pub adapter_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    NotEvaluated,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
}

/// Capability claims attached to every output (SPEC §4).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct Capabilities {
    pub metadata_readable: bool,
    pub settings_inspectable: bool,
    pub changes_plannable: bool,
    /// Container-level writer status for this layout.
    pub container_writer: String,
    /// Whether a generated file has been imported on matching firmware.
    pub import_acceptance: String,
    pub image_approved_for_writing: bool,
    /// The approvals file that approved the image (absent when not approved).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_source: Option<ApprovalSource>,
}

/// An approvals file, identified by path and content digest.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ApprovalSource {
    pub path: String,
    pub sha256: String,
}

// ---------------------------------------------------------------- ocb.report

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Report<P> {
    pub kind: String,
    pub schema_version: String,
    pub producer: Producer,
    pub operation: String,
    pub payload_version: String,
    pub payload: P,
    pub diagnostics: Vec<Diagnostic>,
}

impl<P> Report<P> {
    pub fn new(operation: &str, payload: P, diagnostics: Vec<Diagnostic>) -> Self {
        Report {
            kind: KIND_REPORT.into(),
            schema_version: SCHEMA_VERSION.into(),
            producer: Producer::this(),
            operation: operation.into(),
            payload_version: PAYLOAD_VERSION.into(),
            payload,
            diagnostics,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BiosSummary {
    pub path: Option<String>,
    pub bios_id: String,
    pub board: String,
    pub flavor: String,
    pub revision: String,
    pub build_date: String,
    pub sha256: String,
    pub schema_fingerprint: String,
    pub approved_for_writing: bool,
    pub setup_module: String,
    pub question_count: usize,
    pub decoded_question_count: usize,
    /// Optimized defaults found in the image (display only; never used to decide writes).
    #[serde(default)]
    pub defaults: DefaultsInfo,
    /// Firmware family and labels (additive; absent in older documents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<PlatformInfo>,
    /// True when the image can be paired with OC profiles (decode schema available).
    #[serde(default = "yes")]
    pub profiles_supported: bool,
}

fn yes() -> bool {
    true
}

/// Which firmware family an image belongs to and which parser read it. Labels only.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PlatformInfo {
    /// "intel" or "amd", from the firmware structure.
    pub vendor: String,
    /// Socket family, e.g. "Intel LGA1700".
    pub family: String,
    pub chipset: Option<String>,
    pub models: Option<String>,
    /// MSI version label, e.g. "V17.J0".
    pub version: String,
    pub parser: String,
    pub profiles_note: String,
    /// How much real-profile evidence backs opening profiles from this board. Present only when
    /// profiles are supported. Display only: it never influences pairing or writing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_evidence: Option<ProfileEvidence>,
}

/// Whether profiles from a board have ever been examined.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileEvidence {
    /// The board is one the reviewed adapter covers (real profiles were examined).
    Examined,
    /// Profiles are supported by layout only; no profile from this board has been examined.
    Untested,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DefaultsInfo {
    pub available: bool,
    /// Where defaults come from, or why they are unavailable.
    pub source: String,
}

/// Where a setting appears in the BIOS menus (display only; section headings are heuristic).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MenuLocation {
    /// Page titles below the root, e.g. ["Overclocking", "Advanced CPU Configuration"].
    pub path: Vec<String>,
    pub section: Option<String>,
    /// The BIOS's own prompt text for the primary field.
    pub prompt: String,
    /// Sort key: each page's position in its parent menu, then the field's position on the page.
    #[serde(default)]
    pub order: Vec<u32>,
    /// Other pages that show the same field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub also_on: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProfileSummary {
    pub path: Option<String>,
    pub sha256: String,
    pub bios_id: String,
    pub cpu_brand: String,
    pub saved_at: String,
    pub record_count: usize,
    pub checksum_stored: i32,
    pub checksum_computed: i32,
    pub checksum_valid: bool,
    pub setup_record: StoreSpan,
    pub cpu_setup_record: StoreSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StoreSpan {
    pub store: String,
    pub record_id: u32,
    pub file_offset: usize,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FieldEvidence {
    pub store: String,
    pub store_offset: u16,
    pub file_offset: usize,
    pub width_bits: u16,
    pub raw: u64,
    pub bytes_hex: String,
    pub prompts: Vec<String>,
    pub question_ids: Vec<u16>,
    pub callback: bool,
    pub reset_required: bool,
    pub condition_count: usize,
    pub conditions_reference_undecoded_stores: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Constraints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<i64>,
    pub auto_allowed: bool,
    /// Allowed enum values (symbol, raw, label) or allowed integer values for discrete fields.
    pub options: Vec<EnumOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EnumOption {
    pub symbol: String,
    pub raw: u64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Active,
    Inactive,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Editability {
    pub writable: bool,
    pub auto_only: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DecodedSetting {
    pub setting_id: String,
    pub group: String,
    pub label: String,
    pub summary: String,
    pub available: bool,
    pub value: Option<Value>,
    pub activity: Activity,
    pub activity_reason: String,
    pub editability: Editability,
    pub constraints: Option<Constraints>,
    pub fields: Vec<FieldEvidence>,
    pub evidence: Vec<String>,
    #[serde(default)]
    pub menu: Option<MenuLocation>,
    #[serde(default)]
    pub bios_help: Option<String>,
    /// Value decoded from the BIOS's optimized defaults.
    #[serde(default)]
    pub default_value: Option<Value>,
    /// Null when defaults are unavailable.
    #[serde(default)]
    pub differs_from_default: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RawField {
    pub store: String,
    pub offset: u16,
    pub width_bits: u16,
    pub prompt: String,
    pub raw: u64,
    pub label: Option<String>,
    pub kind: String,
    pub callback: bool,
    #[serde(default)]
    pub menu_path: Vec<String>,
    #[serde(default)]
    pub menu_order: Vec<u32>,
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub default_raw: Option<u64>,
    #[serde(default)]
    pub default_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectPayload {
    pub bios: BiosSummary,
    pub profile: ProfileSummary,
    pub binding: Binding,
    pub capabilities: Capabilities,
    pub groups: Vec<GroupInfo>,
    pub settings: Vec<DecodedSetting>,
    /// All named Setup/CpuSetup questions (read-only physical view). Present when requested.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_fields: Vec<RawField>,
    /// Every other setting the BIOS menus show in Setup/CpuSetup, decoded generically from the
    /// BIOS's own definitions: not reviewed, never writable. Present when requested.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bios_settings: Vec<DecodedSetting>,
    pub opaque_records: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GroupInfo {
    pub id: String,
    pub label: String,
}

/// A BIOS opened on its own: every setting its menus define, with optimized defaults.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BrowsePayload {
    pub bios: BiosSummary,
    pub formsets: Vec<FormsetInfo>,
    pub fields: Vec<BiosField>,
    /// Complete firmware sets when the image holds several (32 MiB AM4); empty otherwise.
    #[serde(default)]
    pub sets: Vec<FirmwareSetInfo>,
    /// Index of the set these formsets and fields come from.
    #[serde(default)]
    pub set: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FirmwareSetInfo {
    pub label: String,
    /// Where the set lives and the CPU families its modules suggest (inferred).
    pub note: String,
    pub defaults_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FormsetInfo {
    /// Display title; empty for the main setup formset.
    pub title: String,
    pub guid: String,
    pub module: String,
    /// Menu path of the page linking to it; empty when it is a top-level entry.
    pub placed_under: Vec<String>,
    pub field_count: usize,
    /// Stores with settings but no optimized defaults, with the reason.
    pub defaults_missing: Vec<String>,
    /// `main`, `linked` (REF3 from a page), `title_match` (placed under a page with the same
    /// title; inferred) or `top_level`.
    #[serde(default)]
    pub placement: String,
    #[serde(default)]
    pub note: Option<String>,
}

/// One setting as the BIOS defines it (no profile value). Display only.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BiosField {
    /// Index into `formsets`.
    pub formset: usize,
    pub store: String,
    pub offset: u16,
    pub width_bits: u16,
    pub prompt: String,
    pub help: String,
    /// "one_of", "checkbox" or "numeric".
    pub kind: String,
    pub options: Vec<FieldOption>,
    pub min: Option<u64>,
    pub max: Option<u64>,
    pub step: Option<u64>,
    pub menu_path: Vec<String>,
    pub menu_order: Vec<u32>,
    pub section: Option<String>,
    pub default_raw: Option<u64>,
    pub default_label: Option<String>,
    pub callback: bool,
    pub reset_required: bool,
    pub read_only: bool,
    /// Only defined under `suppressif TRUE`: the BIOS never shows it.
    pub hidden: bool,
    /// Shown or enabled depending on other settings (conditions are not evaluated).
    pub conditional: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FieldOption {
    pub raw: u64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CatalogPayload {
    pub bios: BiosSummary,
    pub stores: Vec<serde_json::Value>,
    pub questions: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SettingDiff {
    pub setting_id: String,
    pub label: String,
    pub left: Option<Value>,
    pub right: Option<Value>,
    pub equal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ByteSpan {
    pub start: usize,
    pub end: usize,
    pub region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiffPayload {
    pub left: Binding,
    pub right: Binding,
    pub same_bios: bool,
    pub settings: Vec<SettingDiff>,
    /// Raw store differences outside reviewed recipes (named questions only).
    pub other_field_differences: Vec<RawFieldDiff>,
    pub byte_spans: Vec<ByteSpan>,
    /// Every setting of both profiles, reviewed or not, matched by name; only the ones that
    /// differ or exist on one side. Present when requested.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub all_differences: Vec<CompareEntry>,
    /// How many settings the full comparison covered (0 when it was not requested).
    #[serde(default)]
    pub compared: usize,
}

/// One setting that differs between two profiles (or exists in only one). Display only.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompareEntry {
    /// Recipe ID for reviewed settings, else store, menu page and prompt.
    pub key: String,
    pub label: String,
    pub reviewed: bool,
    pub menu: Option<MenuLocation>,
    pub left: Option<Value>,
    pub right: Option<Value>,
    pub left_default: Option<Value>,
    pub right_default: Option<Value>,
    /// "left" or "right" when the setting exists in only that profile's BIOS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub only: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RawFieldDiff {
    pub store: String,
    pub offset: u16,
    pub prompt: String,
    pub left: u64,
    pub right: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VerifyPayload {
    pub bios: BiosSummary,
    pub profile: ProfileSummary,
    pub checks: Vec<Check>,
    pub status: String,
}

// ---------------------------------------------------------------- ocb.patch

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatchMode {
    Edit,
    Migrate,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub op: String,
    pub setting_id: String,
    pub expected: Value,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub unresolved: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelectionState {
    Selected,
    Unselected,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentStatus {
    Equivalent,
    Unavailable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    MissingSetting,
    UnsupportedValue,
    AmbiguousSemantics,
    UnknownApplicability,
    RecipeUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub status: AssessmentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    pub detail: String,
    /// Resolutions permitted for this assessment.
    #[serde(default)]
    pub permitted_resolutions: Vec<ResolutionKind>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionKind {
    None,
    KeepTarget,
    Omit,
    SetValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub kind: ResolutionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinalAction {
    Transfer,
    Set,
    RetainTarget,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    /// Always "supplied_unverified": history read from an input plan is never treated as observed.
    pub provenance: String,
    pub assessment: Assessment,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MigrationEntry {
    pub setting_id: String,
    pub label: String,
    pub group: String,
    pub selection: SelectionState,
    #[serde(default)]
    pub source_value: Option<Value>,
    #[serde(default)]
    pub target_baseline_value: Option<Value>,
    /// Engine-computed for this run. Ignored when read back as input.
    #[serde(default)]
    pub assessment: Option<Assessment>,
    #[serde(default)]
    pub history: Vec<HistoryEntry>,
    pub resolution: Resolution,
    /// Engine-derived. Ignored when read back as input.
    #[serde(default)]
    pub final_action: Option<FinalAction>,
    #[serde(default)]
    pub proposed_value: Option<Value>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TargetOnlyEntry {
    pub setting_id: String,
    pub label: String,
    pub retained_value: Option<Value>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub settings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Accounting {
    pub selected: usize,
    pub unselected: usize,
    pub blocked: usize,
    /// Named Setup/CpuSetup fields whose source and target-baseline raw values differ outside the selected recipes.
    pub other_differing_fields: usize,
    pub opaque_records_retained_from_target: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MigrationPlan {
    pub selection: Selection,
    pub entries: Vec<MigrationEntry>,
    #[serde(default)]
    pub target_only: Vec<TargetOnlyEntry>,
    #[serde(default)]
    pub accounting: Accounting,
    #[serde(default)]
    pub can_build: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Patch {
    pub kind: String,
    pub schema_version: String,
    pub producer: Producer,
    pub mode: PatchMode,
    pub payload_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Binding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Binding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<Operation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration: Option<MigrationPlan>,
    pub policy: Policy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<serde_json::Value>,
    /// Engine annotations; never trusted as authorization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
}

// ---------------------------------------------------------------- ocb.export-manifest

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutputFile {
    pub file_name: String,
    pub sha256: String,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChangeRecord {
    pub setting_id: String,
    pub label: String,
    pub action: FinalAction,
    pub from: Option<Value>,
    pub to: Option<Value>,
    pub file_spans: Vec<ByteSpan>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportManifest {
    pub kind: String,
    pub schema_version: String,
    pub producer: Producer,
    pub operation: PatchMode,
    pub payload_version: String,
    pub adapter_id: String,
    pub adapter_version: String,
    pub recipe_set_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Binding>,
    /// Edit base or migration target baseline.
    pub base: Binding,
    pub output: OutputFile,
    pub capabilities: Capabilities,
    pub changes: Vec<ChangeRecord>,
    pub retained: Vec<String>,
    pub binary_spans: Vec<ByteSpan>,
    pub checks: Vec<Check>,
    /// Assessment history copied from the input plan. Unverified: not observed by this build.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supplied_history_unverified: Vec<serde_json::Value>,
    pub validation: String,
    pub limitations: Vec<String>,
}

// ------------------------------------------------- experimental_inspect (read-only)

/// Payload version of `experimental_inspect` reports.
pub const EXPERIMENTAL_PAYLOAD_VERSION: &str = "1.0";

/// A `[start, end)` file range.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

impl Range {
    pub fn new(start: usize, end: usize) -> Self {
        Range { start, end }
    }
}

/// An integer as exact decimal and hex text (JSON numbers lose precision above 2^53 - 1).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct RawInt {
    pub dec: String,
    pub hex: String,
}

impl RawInt {
    pub fn new(v: u64) -> Self {
        RawInt {
            dec: v.to_string(),
            hex: format!("0x{v:X}"),
        }
    }
}

/// Read-only interpretation of a profile against a BIOS's own definitions. Never a `Session`,
/// never an input to editing, migration or export.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExperimentalInspectPayload {
    pub analysis: AnalysisInfo,
    pub inputs: AnalysisInputs,
    /// `established`, `candidate` or `unrecognized`.
    pub container: String,
    /// Why settings are or are not interpreted, in plain words.
    pub status_reasons: Vec<String>,
    pub identity: IdentityFinding,
    pub integrity: IntegrityFinding,
    pub container_candidates: Vec<ContainerCandidateInfo>,
    /// The candidate the records come from; absent when none or several are eligible.
    pub selected_candidate: Option<String>,
    pub records: Vec<RecordInfo>,
    pub stores: Vec<StoreInfo>,
    pub fields: Vec<ExperimentalField>,
    pub evidence: Vec<Evidence>,
    pub coverage: Coverage,
    pub observations: Vec<Observation>,
    pub capabilities: ExperimentalCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisInfo {
    pub analyzer: String,
    pub version: String,
    /// Always `experimental_read_only`.
    pub mode: String,
    /// Registered rules this analysis applied, as `id/version`.
    pub rules: Vec<String>,
    /// `complete`: every stage ran to its end within the limits.
    pub completion: String,
    /// Set on a shareable report: personal strings and raw byte previews were removed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub shareable: bool,
    /// What a shareable report omitted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redactions: Vec<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnalysisInputs {
    pub profile_sha256: String,
    pub profile_size: usize,
    pub bios: Option<BiosInput>,
    /// The baseline profile supplied for comparison, and whether it was used. Absent without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineInput>,
}

/// A second profile, normally saved right after "Load Optimized Defaults" (on the same BIOS, or
/// on a newer release of the same board), analysed through the same pipeline so fields can be
/// compared with it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BaselineInput {
    pub sha256: String,
    pub size: usize,
    /// `applied`, or `blocked` (the baseline was not usable; no field is annotated).
    pub status: String,
    /// Why it was applied or blocked.
    pub reason: String,
    /// `same_image` (the baseline was saved on the same BIOS image and set as the profile:
    /// fields match by key and compare by raw bytes) or `cross_version` (it was saved on another
    /// release of the same board: fields match by menu place and name, option fields compare by
    /// the BIOS's displayed label, and a difference can also be a default or a meaning that
    /// changed between releases).
    #[serde(default = "same_image")]
    pub mode: String,
    /// The BIOS image the baseline was read against; only in `cross_version` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bios: Option<BiosInput>,
}

fn same_image() -> String {
    "same_image".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BiosInput {
    pub sha256: String,
    pub size: usize,
    pub bios_id: String,
    /// Firmware set analysed; absent when the image has several and none was chosen.
    pub set: Option<usize>,
    /// Labels of the image's firmware sets (empty when it has one).
    pub sets: Vec<String>,
    pub family: String,
    /// Whether the validated profile reader supports this image (unchanged by this analysis).
    pub profiles_supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IdentifierObservation {
    /// `header` (after $MOS$) or `oci` (board + revision strings in $OCI$).
    pub source: String,
    pub text: String,
    pub spans: Vec<Range>,
    /// Parsed as `E` + board + flavor `.` revision, when it parses.
    pub board: Option<String>,
    pub flavor: Option<String>,
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IdentityFinding {
    /// `match`, `mismatch`, `unknown` or `conflicting`.
    pub status: String,
    pub reason: String,
    /// Identifier the profile's contents give, e.g. `E7D91IMS.HJ0`.
    pub profile_bios_id: Option<String>,
    pub observed: Vec<IdentifierObservation>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IntegrityFinding {
    /// `verified`, `candidate_match`, `failed` or `unknown`.
    pub status: String,
    /// Rule as `id/version`, when one was evaluated.
    pub rule: Option<String>,
    /// Where the rule is established.
    pub scope: String,
    pub checksum_at: Option<usize>,
    pub stored: Option<String>,
    pub computed: Option<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TrailerInfo {
    pub at: usize,
    /// Bytes between record end and `$OCI$`.
    pub gap: usize,
    pub strings: Vec<String>,
    pub spans: Vec<Range>,
    pub tail_len: usize,
    pub timestamp_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerCandidateInfo {
    pub id: String,
    /// `id/version` of the probe rule.
    pub rule: String,
    pub assumptions: Vec<String>,
    pub header_id: String,
    pub header_id_span: Range,
    pub checksum_at: usize,
    pub count_at: usize,
    pub records_at: usize,
    pub record_count: Option<u32>,
    /// `[records_at, end)` when the chain is complete.
    pub records_span: Option<Range>,
    pub chain_error: Option<String>,
    pub trailers: Vec<TrailerInfo>,
    pub checks: Vec<Check>,
    /// Usable for settings: complete chain, one coherent trailer, a parseable identifier.
    pub eligible: bool,
    pub contradictions: Vec<String>,
    pub integrity: IntegrityFinding,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecordInfo {
    /// Stable key: `r<index>`.
    pub key: String,
    pub index: usize,
    pub id: u32,
    pub header: Range,
    pub payload: Range,
    pub size: usize,
    /// `confirmed`, `inferred`, `ambiguous` or `unmapped`.
    pub mapping: String,
    /// Keys of the stores this record could be (all candidates, before any rule).
    pub candidate_stores: Vec<String>,
    /// The variable the setup browser's own list names for this record id (rule
    /// `amitse_variable_index/1`), when that rule applies.
    pub variable: Option<ListedVariableInfo>,
    pub reason: String,
    pub evidence: Vec<String>,
    /// Hex of the first payload bytes; `preview_truncated` when not all of it.
    pub preview_hex: String,
    pub preview_truncated: bool,
    /// Set on a shareable report: the preview was removed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub preview_omitted: bool,
}

/// One entry of the BIOS setup browser's variable list.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListedVariableInfo {
    /// Position in the list (the record id that names it).
    pub index: u32,
    pub guid: String,
    pub name: String,
    /// Size the setup data declares (0 = none declared).
    pub declared_size: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StoreDeclaration {
    pub formset_guid: String,
    pub formset_title: String,
    pub module: String,
    pub module_sha256: String,
    pub local_id: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StoreInfo {
    /// Stable key: `set<N>:<store GUID>:<name>:<size>`.
    pub key: String,
    pub name: String,
    pub guid: String,
    /// `buffer`, `efi` or `name_value`.
    pub kind: String,
    pub size: u16,
    pub set: usize,
    /// Every formset that declares this variable (UEFI identity: GUID + name).
    pub declared_by: Vec<StoreDeclaration>,
    /// `supported`, or why this analysis cannot map it.
    pub support: String,
    pub candidate_records: Vec<String>,
    /// `confirmed`, `inferred`, `ambiguous` or `unmapped`.
    pub mapping: String,
    pub mapped_record: Option<String>,
    pub reason: String,
    pub evidence: Vec<String>,
    /// Named, byte-aligned questions the BIOS defines in this variable.
    pub field_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExperimentalField {
    /// Stable key including provenance.
    pub key: String,
    pub store: String,
    pub record: String,
    pub label: String,
    pub help: String,
    pub menu_path: Vec<String>,
    pub menu_order: Vec<u32>,
    pub section: Option<String>,
    /// `one_of`, `checkbox` or `numeric`.
    pub kind: String,
    pub store_offset: u16,
    pub width_bits: u16,
    pub span: Option<Range>,
    pub raw: Option<RawInt>,
    /// `decoded`, `unknown_value`, `unsupported_encoding` or `out_of_bounds`.
    pub state: String,
    /// The BIOS's own label for the stored raw value (one-of), or the checkbox state.
    pub value_label: Option<String>,
    pub note: Option<String>,
    /// Mapping state of the record the value comes from.
    pub mapping: String,
    pub default_raw: Option<RawInt>,
    pub default_label: Option<String>,
    pub differs_from_default: Option<bool>,
    /// What the baseline profile stores for this field (only with an applied baseline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_raw: Option<RawInt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_label: Option<String>,
    /// Whether the two files differ here: different stored bytes (`same_image`), or, across
    /// releases, a different displayed label for option fields (else different raw values).
    /// Absent when the field was not compared. It says nothing about who changed what.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub differs_from_baseline: Option<bool>,
    /// How the baseline field was found: `key` (same image) or `menu_place` (store name and GUID,
    /// menu path, section, label, kind and width, unique on both sides).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_match: Option<String>,
    /// Whether the baseline's own value here differs from its BIOS's optimized default (rule
    /// `baseline_firmware_filled/1`, display only). Absent when the baseline field has no
    /// default: nothing is known. Only meaningful when the baseline was saved at BIOS defaults.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_off_default: Option<bool>,
    pub hidden: bool,
    pub conditional: bool,
    pub callback: bool,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Evidence {
    pub id: String,
    /// `container`, `identity`, `integrity`, `mapping` or `rule`.
    pub kind: String,
    pub summary: String,
    /// What the claim rests on: `file_structure`, `reviewed_binding`, `firmware_code`,
    /// `file_comparison` (two files compared through their images' menus and defaults) or
    /// `size_only`. Describes the basis, not a confidence.
    pub basis: String,
    pub observations: Vec<String>,
    pub contradictions: Vec<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Coverage {
    pub records_total: usize,
    pub records_confirmed: usize,
    pub records_inferred: usize,
    pub records_ambiguous: usize,
    pub records_unmapped: usize,
    pub fields_total: usize,
    pub fields_decoded: usize,
    pub fields_unknown_value: usize,
    pub fields_unsupported: usize,
    pub fields_out_of_bounds: usize,
    pub profile_bytes: usize,
    /// Bytes inside mapped record payloads.
    pub mapped_bytes: usize,
    /// Bytes covered by interpreted fields (a union of ranges, no double counting).
    pub interpreted_bytes: usize,
    /// `profile_bytes - interpreted_bytes`.
    pub opaque_bytes: usize,
    /// Fields present with a value in both the profile and an applied baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields_compared_with_baseline: Option<usize>,
    /// Of those, fields where the two files store different bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields_differing_from_baseline: Option<usize>,
    /// Interpreted fields with a value that were not compared with an applied cross-version
    /// baseline: no counterpart in the baseline, or a place name not unique on one side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields_not_compared_with_baseline: Option<usize>,
    /// Of the fields differing from the baseline: the baseline at its own BIOS default (most
    /// likely settings made in the BIOS menus).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_differing_at_default: Option<usize>,
    /// ... the baseline itself off its default (often filled in by the firmware).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_differing_off_default: Option<usize>,
    /// ... no default for the baseline's field: unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_differing_no_default: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Observation {
    /// `signature` or `string`.
    pub kind: String,
    pub span: Range,
    pub text: String,
}

/// What may be done with this analysis. Everything but the JSON export is false, always.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ExperimentalCapabilities {
    pub profile_write: bool,
    pub change_planning: bool,
    pub migration: bool,
    pub profile_synthesis: bool,
    pub analysis_export: bool,
}

impl ExperimentalCapabilities {
    pub const READ_ONLY: ExperimentalCapabilities = ExperimentalCapabilities {
        profile_write: false,
        change_planning: false,
        migration: false,
        profile_synthesis: false,
        analysis_export: true,
    };
}

// ------------------------------------------------- profile_format (read-only, BIOS only)

/// Payload version of `profile_format` reports.
pub const PROFILE_FORMAT_PAYLOAD_VERSION: &str = "1.0";

/// What a BIOS image says about the profiles it writes, without any profile: the setup
/// browser's variable list (record id = position), where the profile code lives, and the file
/// layout. Display and export only; never an input to reading or writing a profile.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProfileFormatPayload {
    pub bios: BiosInput,
    /// The release's profile save/load code was traced, or is the same code as a traced release.
    pub traced: bool,
    /// Modules of this set holding the profile signatures.
    pub code: Vec<ProfileCodeModule>,
    /// The profile file layout, part by part, each with what it rests on.
    pub layout: Vec<FormatFact>,
    pub variable_list: Option<VariableListInfo>,
    /// Why there is no variable list, when there is none.
    pub list_problem: Option<String>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProfileCodeModule {
    pub name: String,
    pub guid: String,
    pub sha256: String,
    /// Holds `$MOS$`, the signature profiles start with.
    pub holds_mos: bool,
    /// Holds `$OCI$`, the trailer signature.
    pub holds_oci: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FormatFact {
    /// Short name of the part, e.g. `header`, `records`, `cmos`, `trailer`.
    pub part: String,
    pub detail: String,
    /// `firmware_code` (traced code), `profiles` (seen in exported profiles), `image_structure`
    /// (found in this image) or `expected` (inferred, not traced for this release).
    pub basis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VariableListInfo {
    pub container_path: String,
    pub sha256: String,
    pub entries: Vec<ListedEntryInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListedEntryInfo {
    /// Position in the list: the record id a profile from this release uses.
    pub index: u32,
    pub guid: String,
    pub name: String,
    pub attributes: u32,
    /// Size the setup data declares (0 = none declared).
    pub declared_size: u32,
    /// Size of the buffer store the BIOS menus declare for this variable, when exactly one.
    pub store_size: Option<u16>,
    /// Named, byte-aligned questions the menus define in that store.
    pub field_count: usize,
    /// `saved` or `skipped` by the traced save routine; `unknown` when the release is not traced.
    pub saved: String,
    pub note: Option<String>,
}
