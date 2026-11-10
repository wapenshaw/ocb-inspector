// Mirrors crates/core/src/contracts.rs. Rust is authoritative; the UI never decides validity.

export type Value =
  | { kind: "auto" }
  | { kind: "integer"; value: number; unit: string }
  | { kind: "enum"; symbol: string; raw?: number; label?: string }
  | { kind: "boolean"; value: boolean }
  | { kind: "unknown"; raw: number };

export type Severity = "info" | "warning" | "error";
export interface Diagnostic {
  code: string;
  severity: Severity;
  stage: string;
  message: string;
  setting_id?: string;
  input_role?: string;
}

export interface Binding {
  bios_id: string;
  bios_sha256: string;
  profile_sha256: string;
  schema_fingerprint: string;
  adapter_id: string;
  adapter_version: string;
}

export type CheckStatus = "pass" | "fail" | "not_evaluated";
export interface Check {
  name: string;
  status: CheckStatus;
  detail: string;
}

export interface Capabilities {
  metadata_readable: boolean;
  settings_inspectable: boolean;
  changes_plannable: boolean;
  container_writer: string;
  import_acceptance: string;
  image_approved_for_writing: boolean;
  /** Approvals file that approved the image (absent when not approved). */
  approval_source?: { path: string; sha256: string } | null;
}

export interface Report<P> {
  kind: "ocb.report";
  schema_version: string;
  operation: string;
  payload_version: string;
  payload: P;
  diagnostics: Diagnostic[];
}

export interface BiosSummary {
  path: string | null;
  bios_id: string;
  board: string;
  flavor: string;
  revision: string;
  build_date: string;
  sha256: string;
  schema_fingerprint: string;
  approved_for_writing: boolean;
  setup_module: string;
  question_count: number;
  decoded_question_count: number;
  /** Optimized defaults read from the image (AMI StdDefaults). Display only. */
  defaults: { available: boolean; source: string };
  /** Firmware family and labels. Absent in documents from older builds. */
  platform?: PlatformInfo;
  /** False when no OC profile from this BIOS can be opened (the BIOS can still be browsed). */
  profiles_supported?: boolean;
}

export interface PlatformInfo {
  vendor: "intel" | "amd";
  family: string;
  chipset: string | null;
  models: string | null;
  version: string;
  parser: string;
  profiles_note: string;
  /** "untested": no profile from this board has been examined (layout assumed). Absent when profiles are not supported. */
  profile_evidence?: "examined" | "untested";
}

/** A BIOS opened on its own: every setting its menus define. */
export interface BrowsePayload {
  bios: BiosSummary;
  formsets: FormsetInfo[];
  fields: BiosField[];
  /** Complete firmware sets when the image holds several (32 MiB AM4); empty otherwise. */
  sets: FirmwareSetInfo[];
  /** Index of the set shown. */
  set: number;
}

export interface FirmwareSetInfo {
  label: string;
  note: string;
  defaults_note: string;
}

export interface FormsetInfo {
  title: string;
  guid: string;
  module: string;
  placed_under: string[];
  field_count: number;
  defaults_missing: string[];
  /** main | linked | title_match (placed by matching page title; inferred) | top_level */
  placement: string;
  note: string | null;
}

export interface BiosField {
  formset: number;
  store: string;
  offset: number;
  width_bits: number;
  prompt: string;
  help: string;
  kind: "one_of" | "checkbox" | "numeric";
  options: { raw: number; label: string }[];
  min: number | null;
  max: number | null;
  step: number | null;
  menu_path: string[];
  menu_order: number[];
  section: string | null;
  default_raw: number | null;
  default_label: string | null;
  callback: boolean;
  reset_required: boolean;
  read_only: boolean;
  hidden: boolean;
  conditional: boolean;
}

/** Where a setting sits in the BIOS menus. Section headings are heuristic; display only. */
export interface MenuLocation {
  path: string[];
  section: string | null;
  prompt: string;
  /** Sort key: page positions, then the field's position on its page. */
  order: number[];
  also_on?: string[][];
}

export interface StoreSpan {
  store: string;
  record_id: number;
  file_offset: number;
  size: number;
}

export interface ProfileSummary {
  path: string | null;
  sha256: string;
  bios_id: string;
  cpu_brand: string;
  saved_at: string;
  record_count: number;
  checksum_stored: number;
  checksum_computed: number;
  checksum_valid: boolean;
  setup_record: StoreSpan;
  cpu_setup_record: StoreSpan;
}

export interface FieldEvidence {
  store: string;
  store_offset: number;
  file_offset: number;
  width_bits: number;
  raw: number;
  bytes_hex: string;
  prompts: string[];
  question_ids: number[];
  callback: boolean;
  reset_required: boolean;
  condition_count: number;
  conditions_reference_undecoded_stores: boolean;
}

export interface EnumOption {
  symbol: string;
  raw: number;
  label: string;
}

export interface Constraints {
  unit?: string;
  min?: number;
  max?: number;
  step?: number;
  auto_allowed: boolean;
  options: EnumOption[];
}

export type Activity = "active" | "inactive" | "unknown";

export interface DecodedSetting {
  setting_id: string;
  group: string;
  label: string;
  summary: string;
  available: boolean;
  value: Value | null;
  activity: Activity;
  activity_reason: string;
  editability: { writable: boolean; auto_only: boolean; reason: string };
  constraints: Constraints | null;
  fields: FieldEvidence[];
  evidence: string[];
  menu: MenuLocation | null;
  bios_help: string | null;
  default_value: Value | null;
  /** null when the BIOS image has no readable optimized defaults. */
  differs_from_default: boolean | null;
}

export interface RawField {
  store: string;
  offset: number;
  width_bits: number;
  prompt: string;
  raw: number;
  label: string | null;
  kind: string;
  callback: boolean;
  menu_path: string[];
  menu_order: number[];
  section: string | null;
  default_raw: number | null;
  default_label: string | null;
}

export interface InspectPayload {
  bios: BiosSummary;
  profile: ProfileSummary;
  binding: Binding;
  capabilities: Capabilities;
  groups: { id: string; label: string }[];
  settings: DecodedSetting[];
  raw_fields?: RawField[];
  /** Every other setting the BIOS menus show, decoded generically (read only, group "bios"). */
  bios_settings?: DecodedSetting[];
  opaque_records: number;
}

export interface Operation {
  op: "set";
  setting_id: string;
  expected: Value;
  value: Value;
}

export type Reason =
  | "missing_setting"
  | "unsupported_value"
  | "ambiguous_semantics"
  | "unknown_applicability"
  | "recipe_unavailable";
export type ResolutionKind = "none" | "keep_target" | "omit" | "set_value";
export type FinalAction = "transfer" | "set" | "retain_target" | "blocked";

export interface Assessment {
  status: "equivalent" | "unavailable";
  reason?: Reason;
  detail: string;
  permitted_resolutions: ResolutionKind[];
}

export interface MigrationEntry {
  setting_id: string;
  label: string;
  group: string;
  selection: "selected" | "unselected";
  source_value: Value | null;
  target_baseline_value: Value | null;
  assessment: Assessment | null;
  history: { provenance: string; assessment: Assessment }[];
  resolution: { kind: ResolutionKind; value?: Value };
  final_action: FinalAction | null;
  proposed_value: Value | null;
  notes: string[];
}

export interface Selection {
  groups: string[];
  settings: string[];
}

export interface MigrationPlan {
  selection: Selection;
  entries: MigrationEntry[];
  target_only: { setting_id: string; label: string; retained_value: Value | null; note: string }[];
  accounting: {
    selected: number;
    unselected: number;
    blocked: number;
    other_differing_fields: number;
    opaque_records_retained_from_target: number;
  };
  can_build: boolean;
}

export interface Patch {
  kind: "ocb.patch";
  schema_version: string;
  producer: { name: string; version: string };
  mode: "edit" | "migrate";
  payload_version: string;
  base?: Binding;
  source?: Binding;
  target?: Binding;
  operations?: Operation[];
  migration?: MigrationPlan;
  policy: { unresolved: string };
  diagnostics?: Diagnostic[];
}

export interface ByteSpan {
  start: number;
  end: number;
  region: string;
}

export interface ChangeRecord {
  setting_id: string;
  label: string;
  action: FinalAction;
  from: Value | null;
  to: Value | null;
  file_spans: ByteSpan[];
}

export interface ExportManifest {
  kind: "ocb.export-manifest";
  operation: "edit" | "migrate";
  source?: Binding;
  base: Binding;
  output: { file_name: string; sha256: string; size: number };
  capabilities: Capabilities;
  changes: ChangeRecord[];
  retained: string[];
  binary_spans: ByteSpan[];
  checks: Check[];
  validation: string;
  limitations: string[];
}

export interface BuildResult {
  output_path: string;
  report_path: string;
  manifest: ExportManifest;
}

export interface EditPreview {
  changes: ChangeRecord[];
  checks: Check[];
}

export interface CommandError {
  code: string;
  exit_code: number;
  message: string;
}

/** One setting that differs between two profiles, or exists in only one. Display only. */
export interface CompareEntry {
  key: string;
  label: string;
  reviewed: boolean;
  menu: MenuLocation | null;
  left: Value | null;
  right: Value | null;
  left_default: Value | null;
  right_default: Value | null;
  only?: "left" | "right";
}

export interface DiffPayload {
  left: Binding;
  right: Binding;
  same_bios: boolean;
  all_differences?: CompareEntry[];
  compared?: number;
}

/** desktop `app_info`: what the About screen shows without the network. */
export interface AppInfo {
  version: string;
  /** CHANGELOG.md built into this version. */
  changelog: string;
  repo_url: string;
  /** False when config/settings.json has "update_check": false. */
  update_check: boolean;
}

export interface ReleaseInfo {
  version: string;
  /** release-please's Markdown entry for this release. */
  notes: string;
  url: string;
  published_at: string;
}

/** desktop `check_updates`: releases newer than this build, newest first. */
export interface UpdateInfo {
  current: string;
  newer: ReleaseInfo[];
}

/** The Settings switches as saved in config/settings.json (they apply at the next start). */
export interface SavedSwitches {
  update_check: boolean;
  writing: boolean;
}

/** What `settings_load` returns: the settings in effect plus the switches saved now. */
export interface UiSettings {
  theme?: string | null;
  /** Experimental writing switch in effect (read at start-up). */
  writing: boolean;
  /** Why writing is off, or empty. */
  writing_note: string;
  /** The settings file, for display. */
  path: string;
  saved: SavedSwitches;
  /** Why the file cannot be read or changed, or empty. */
  error: string;
}

/** Keys `settings_save` may change; the rest of the file is kept. */
export interface SettingsChange {
  theme?: string;
  update_check?: boolean;
  writing?: boolean;
}

// ---------------------------------------------- experimental_inspect (read-only analysis)
// Mirrors `contracts.rs`. Every range is [start, end). Integers that can exceed 2^53 - 1 are
// RawInt strings, never JSON numbers.

export interface Range {
  start: number;
  end: number;
}

export interface RawInt {
  dec: string;
  hex: string;
}

export type ContainerState = "established" | "candidate" | "unrecognized";
export type IdentityStatus = "match" | "mismatch" | "unknown" | "conflicting";
export type IntegrityStatus = "verified" | "candidate_match" | "failed" | "unknown";
export type MappingState = "confirmed" | "inferred" | "ambiguous" | "unmapped";
export type FieldState = "decoded" | "unknown_value" | "unsupported_encoding" | "out_of_bounds";

export interface AnalysisInfo {
  analyzer: string;
  version: string;
  mode: "experimental_read_only";
  rules: string[];
  completion: string;
  /** Set on a shareable report: personal strings and raw byte previews were removed. */
  shareable?: boolean;
  /** What a shareable report omitted. */
  redactions?: string[];
}

export interface BiosInput {
  sha256: string;
  size: number;
  bios_id: string;
  /** Firmware set analysed; null when the image has several and none was chosen. */
  set: number | null;
  /** Labels of the image's firmware sets (empty when it has one). */
  sets: string[];
  family: string;
  /** Whether the validated profile reader supports this image. */
  profiles_supported: boolean;
}

export interface AnalysisInputs {
  profile_sha256: string;
  profile_size: number;
  bios: BiosInput | null;
  /** The baseline profile supplied for comparison; absent without one. */
  baseline?: BaselineInput;
}

export interface BaselineInput {
  sha256: string;
  size: number;
  /** "blocked": the baseline was not usable and no field is annotated. */
  status: "applied" | "blocked";
  reason: string;
  /**
   * "same_image": saved on the same BIOS image and set, matched by key, compared by raw bytes.
   * "cross_version": saved on another release of the same board, matched by menu place and name.
   * Absent in reports recorded before the mode existed (same image).
   */
  mode?: "same_image" | "cross_version";
  /** The BIOS image the baseline was read against (cross_version only). */
  bios?: BiosInput;
}

export interface IdentifierObservation {
  source: "header" | "oci";
  text: string;
  spans: Range[];
  board: string | null;
  flavor: string | null;
  revision: string | null;
}

export interface IdentityFinding {
  status: IdentityStatus;
  reason: string;
  profile_bios_id: string | null;
  observed: IdentifierObservation[];
  limitations: string[];
}

export interface IntegrityFinding {
  status: IntegrityStatus;
  rule: string | null;
  scope: string;
  checksum_at: number | null;
  stored: string | null;
  computed: string | null;
  limitations: string[];
}

export interface TrailerInfo {
  at: number;
  gap: number;
  strings: string[];
  spans: Range[];
  tail_len: number;
  timestamp_valid: boolean;
}

export interface ContainerCandidateInfo {
  id: string;
  rule: string;
  assumptions: string[];
  header_id: string;
  header_id_span: Range;
  checksum_at: number;
  count_at: number;
  records_at: number;
  record_count: number | null;
  records_span: Range | null;
  chain_error: string | null;
  trailers: TrailerInfo[];
  checks: Check[];
  eligible: boolean;
  contradictions: string[];
  integrity: IntegrityFinding;
}

export interface RecordInfo {
  key: string;
  index: number;
  id: number;
  header: Range;
  payload: Range;
  size: number;
  mapping: MappingState;
  candidate_stores: string[];
  /** The variable the BIOS's own variable list names for this record id, when that rule applies. */
  variable: ListedVariableInfo | null;
  reason: string;
  evidence: string[];
  preview_hex: string;
  preview_truncated: boolean;
  /** Shareable report: the preview was removed. */
  preview_omitted?: boolean;
}

export interface ListedVariableInfo {
  index: number;
  guid: string;
  name: string;
  /** Size the setup data declares; 0 = none declared. */
  declared_size: number;
}

export interface StoreDeclaration {
  formset_guid: string;
  formset_title: string;
  module: string;
  module_sha256: string;
  local_id: number;
}

export interface StoreInfo {
  key: string;
  name: string;
  guid: string;
  kind: "buffer" | "efi" | "name_value";
  size: number;
  set: number;
  declared_by: StoreDeclaration[];
  /** "supported", or why this analysis cannot map it. */
  support: string;
  candidate_records: string[];
  mapping: MappingState;
  mapped_record: string | null;
  reason: string;
  evidence: string[];
  field_count: number;
}

/** Optional recipe interpretation, separate from record mapping and integrity assurance. */
export interface ReviewedInterpretation {
  setting_id: string;
  label: string;
  value: Value;
  default_value: Value | null;
  baseline_value: Value | null;
  differs_from_default: boolean | null;
  evidence: string[];
  board_evidence?: "examined" | "untested";
  components: { store: string; store_offset: number; width_bits: number; span: Range }[];
}

export interface ExperimentalField {
  reviewed?: ReviewedInterpretation | null;
  key: string;
  store: string;
  record: string;
  label: string;
  help: string;
  menu_path: string[];
  menu_order: number[];
  section: string | null;
  kind: "one_of" | "checkbox" | "numeric";
  store_offset: number;
  width_bits: number;
  span: Range | null;
  raw: RawInt | null;
  state: FieldState;
  value_label: string | null;
  note: string | null;
  mapping: MappingState;
  default_raw: RawInt | null;
  default_label: string | null;
  differs_from_default: boolean | null;
  /** What the baseline stores for this field (applied baseline only). */
  baseline_raw?: RawInt | null;
  baseline_label?: string | null;
  /** The two files store different bytes here; absent when only one side has the field. Not "changed by the user". */
  differs_from_baseline?: boolean | null;
  /** How the baseline field was found: "key" (same image) or "menu_place" (cross_version). */
  baseline_match?: "key" | "menu_place" | null;
  /**
   * The baseline's own value here differs from its BIOS's optimized default (rule baseline_firmware_filled/1, display
   * only). Absent: the baseline field has no default, so nothing is known. Meaningful only for a baseline saved at BIOS defaults.
   */
  baseline_off_default?: boolean | null;
  hidden: boolean;
  conditional: boolean;
  callback: boolean;
  evidence: string[];
}

export interface Evidence {
  id: string;
  kind: "container" | "identity" | "integrity" | "mapping" | "rule";
  summary: string;
  /** What the claim rests on. Not a confidence. */
  basis: "file_structure" | "reviewed_binding" | "firmware_code" | "file_comparison" | "size_only";
  observations: string[];
  contradictions: string[];
  limitations: string[];
}

export interface Coverage {
  records_total: number;
  records_confirmed: number;
  records_inferred: number;
  records_ambiguous: number;
  records_unmapped: number;
  fields_total: number;
  fields_decoded: number;
  fields_unknown_value: number;
  fields_unsupported: number;
  fields_out_of_bounds: number;
  profile_bytes: number;
  mapped_bytes: number;
  interpreted_bytes: number;
  opaque_bytes: number;
  /** Fields with a value in both the profile and an applied baseline. */
  fields_compared_with_baseline?: number | null;
  fields_differing_from_baseline?: number | null;
  /** Cross-version baseline only: interpreted fields with no usable counterpart in the baseline. */
  fields_not_compared_with_baseline?: number | null;
  /** Of the fields differing from the baseline: the baseline at its own BIOS default / off it / without a default. */
  baseline_differing_at_default?: number | null;
  baseline_differing_off_default?: number | null;
  baseline_differing_no_default?: number | null;
}

export interface Observation {
  kind: "signature" | "string";
  span: Range;
  text: string;
}

export interface ExperimentalCapabilities {
  profile_write: false;
  change_planning: false;
  migration: false;
  profile_synthesis: false;
  analysis_export: boolean;
}

export interface ExperimentalInspectPayload {
  analysis: AnalysisInfo;
  inputs: AnalysisInputs;
  container: ContainerState;
  status_reasons: string[];
  identity: IdentityFinding;
  integrity: IntegrityFinding;
  container_candidates: ContainerCandidateInfo[];
  selected_candidate: string | null;
  records: RecordInfo[];
  stores: StoreInfo[];
  fields: ExperimentalField[];
  evidence: Evidence[];
  coverage: Coverage;
  observations: Observation[];
  capabilities: ExperimentalCapabilities;
}

/** What `save_analysis` needs: the inputs and the hashes of the analysis the user saw. */
export interface AnalysisRequest {
  profile: string;
  bios: string | null;
  set: number | null;
  /** Profile saved right after "Load Optimized Defaults" on the same BIOS (needs `bios`). */
  baseline?: string | null;
  /** The BIOS the baseline was saved on, when it is another release of the same board (needs `baseline`). */
  baselineBios?: string | null;
  /** Firmware set of `baselineBios` (0-based). */
  baselineSet?: number | null;
}

// ------------------------------------------------- profile_format (BIOS only, read-only)

export type FormatBasis = "firmware_code" | "profiles" | "image_structure" | "expected";

export interface FormatFact {
  /** Short name of the part: header, checksum, records, skipped, cmos, trailer, profile_code. */
  part: string;
  detail: string;
  basis: FormatBasis;
}

export interface ProfileCodeModule {
  name: string;
  guid: string;
  sha256: string;
  holds_mos: boolean;
  holds_oci: boolean;
}

export interface ListedEntryInfo {
  /** Position in the list: the record id a profile from this release uses. */
  index: number;
  guid: string;
  name: string;
  attributes: number;
  /** Size the setup data declares; 0 = none declared. */
  declared_size: number;
  /** Size of the buffer store the BIOS menus declare for it, when exactly one. */
  store_size: number | null;
  field_count: number;
  saved: "saved" | "skipped" | "unknown";
  note: string | null;
}

export interface VariableListInfo {
  container_path: string;
  sha256: string;
  entries: ListedEntryInfo[];
}

export interface ProfileFormatPayload {
  bios: BiosInput;
  /** The release's profile save/load code was traced, or is the same code as a traced release. */
  traced: boolean;
  code: ProfileCodeModule[];
  layout: FormatFact[];
  variable_list: VariableListInfo | null;
  list_problem: string | null;
  limitations: string[];
}
