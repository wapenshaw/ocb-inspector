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
