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
            "bios_catalog": ["1.0"], "inspect": ["1.0"], "diff": ["1.0"], "verify": ["1.0"]}},
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
