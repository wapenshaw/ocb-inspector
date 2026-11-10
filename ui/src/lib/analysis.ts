// Pure helpers for the experimental analysis screen. They present the engine's report only:
// labels, lookups, the settings list and its filters. None of them decides a mapping or a value.
import { errorMessage, isCommandError } from "../api";
import type {
  AnalysisRequest,
  ContainerState,
  Evidence,
  ExperimentalField,
  ExperimentalInspectPayload,
  FieldState,
  IdentityStatus,
  IntegrityStatus,
  MappingState,
  RecordInfo,
  StoreInfo,
} from "../api/types";
import { hex } from "./format";
import { menuSort, type Located } from "./menu";
import type { Tone } from "../components/Status";

export interface Lookup {
  records: Map<string, RecordInfo>;
  stores: Map<string, StoreInfo>;
  evidence: Map<string, Evidence>;
}

export function lookupOf(p: ExperimentalInspectPayload): Lookup {
  return {
    records: new Map(p.records.map((r): [string, RecordInfo] => [r.key, r])),
    stores: new Map(p.stores.map((s): [string, StoreInfo] => [s.key, s])),
    evidence: new Map(p.evidence.map((e): [string, Evidence] => [e.id, e])),
  };
}

export const EMPTY_LOOKUP: Lookup = { records: new Map(), stores: new Map(), evidence: new Map() };

/** One settings row: the field, its menu position (for grouping) and precomputed text. */
export interface FieldItem extends Located {
  f: ExperimentalField;
  menu: { path: string[]; section: string | null; order: number[] };
  /** Lower-case text the search box matches against. */
  hay: string;
  value: string;
  defaultText: string;
  /** What the baseline profile stores here; only meaningful when a baseline is applied. */
  baselineText: string;
}

export function valueText(f: ExperimentalField): string {
  switch (f.state) {
    case "decoded":
      return labelOrRaw(f.value_label, f.raw);
    case "unknown_value":
      return "Unknown value";
    case "unsupported_encoding":
    case "out_of_bounds":
      return "Not interpreted";
    default:
      return "—";
  }
}

/** A value's own label, else its raw value; "—" when it has neither. Nothing is judged here. */
const labelOrRaw = (label: string | null | undefined, raw: { dec: string } | null | undefined): string => label ?? raw?.dec ?? "—";

/** The baseline's own label, else its raw value; "—" when it has none. */
export function baselineText(f: ExperimentalField): string {
  return labelOrRaw(f.baseline_label, f.baseline_raw);
}

/** True when the report carries a usable baseline comparison. */
export function baselineApplied(p: ExperimentalInspectPayload): boolean {
  return p.inputs.baseline?.status === "applied";
}

/** Every field, in BIOS menu order, with its search text computed once. */
export function fieldItems(p: ExperimentalInspectPayload): FieldItem[] {
  const items: FieldItem[] = p.fields.map((f) => {
    const value = valueText(f);
    return {
      f,
      menu: { path: f.menu_path, section: f.section, order: f.menu_order },
      hay: `${f.label} ${f.menu_path.join(" ")} ${value} ${f.raw?.hex ?? ""}`.toLowerCase(),
      value,
      // The BIOS's optimized default, never a "factory profile".
      defaultText: labelOrRaw(f.default_label, f.default_raw),
      baselineText: baselineText(f),
    };
  });
  return menuSort(items);
}

/** One choice of a segmented filter: what it shows, and which fields it keeps. */
export interface FilterOption<V extends string> {
  value: V;
  label: string;
  title?: string;
  test: (f: ExperimentalField) => boolean;
}

export type StateFilter = "all" | "decoded" | "unknown_value" | "not_decoded";

export const STATE_FILTERS: FilterOption<StateFilter>[] = [
  { value: "all", label: "All", test: () => true },
  { value: "decoded", label: "Decoded", test: (f) => f.state === "decoded" },
  { value: "unknown_value", label: "Unknown value", test: (f) => f.state === "unknown_value" },
  { value: "not_decoded", label: "Not decoded", test: (f) => f.state === "unsupported_encoding" || f.state === "out_of_bounds" },
];

/** How the settings list is narrowed by comparison with the BIOS optimized default. */
export type DefaultsFilter = "all" | "differs" | "none" | "baseline" | "baseline_at" | "baseline_off";

/** What each baseline view means, for tooltips (the reasoning, and when it does not hold). */
const BASELINE_AT_TITLE =
  "Differs from the baseline, and the baseline holds its BIOS's optimized default there. A profile saved with the BIOS at its defaults should equal those defaults, so such a difference is most likely a setting made in the BIOS menus. This only holds if the baseline was saved with the BIOS at its defaults; a baseline saved after tuning counts its own settings as off default too.";
const BASELINE_OFF_TITLE =
  "Differs from the baseline, and the baseline itself is off its BIOS's optimized default. Values the firmware fills in on its own (fan type detection, link equalisation) look like this, so these are less likely your own choices. This is a guess, not a fact.";

export const DEFAULTS_FILTERS: FilterOption<DefaultsFilter>[] = [
  { value: "all", label: "All", test: () => true },
  { value: "differs", label: "Differs from default", test: (f) => f.differs_from_default === true },
  { value: "none", label: "No default", test: (f) => f.differs_from_default === null },
];

/** The filter choices while a baseline is applied: "Differs from baseline" exists only then. */
export const BASELINE_FILTERS: FilterOption<DefaultsFilter>[] = [
  ...DEFAULTS_FILTERS,
  {
    value: "baseline_at",
    label: "Likely your settings",
    title: BASELINE_AT_TITLE,
    test: (f) => f.differs_from_baseline === true && f.baseline_off_default === false,
  },
  {
    value: "baseline_off",
    label: "Filled in by firmware?",
    title: BASELINE_OFF_TITLE,
    test: (f) => f.differs_from_baseline === true && f.baseline_off_default === true,
  },
  { value: "baseline", label: "Differs from baseline", test: (f) => f.differs_from_baseline === true },
];

/** The test for a chosen option; a value outside the list keeps everything. */
export function filterTest<V extends string>(options: readonly FilterOption<V>[], value: V): (f: ExperimentalField) => boolean {
  return options.find((o) => o.value === value)?.test ?? (() => true);
}

export interface DefaultCounts {
  /** Settings whose stored value differs from the BIOS optimized default. */
  differs: number;
  /** Settings the BIOS image gives no optimized default for: they cannot be judged either way. */
  noDefault: number;
  /** Settings whose stored bytes differ from the baseline profile's. Absent comparisons count for nothing. */
  baselineDiffers: number;
  /** Settings present in both the profile and the baseline. */
  baselineCompared: number;
  /** Of the settings that differ from the baseline: the baseline holds its BIOS default there. */
  baselineAtDefault: number;
  /** ... the baseline itself is off its BIOS default there. */
  baselineOffDefault: number;
  /** ... the baseline's BIOS gives no default, so nothing is known. */
  baselineNoDefault: number;
}

/** Counts every field the list holds (hidden ones included, as the list shows them). */
export function defaultCounts(fields: readonly ExperimentalField[]): DefaultCounts {
  let differs = 0;
  let noDefault = 0;
  let baselineDiffers = 0;
  let baselineCompared = 0;
  let baselineAtDefault = 0;
  let baselineOffDefault = 0;
  let baselineNoDefault = 0;
  for (const f of fields) {
    if (f.differs_from_default === true) differs++;
    else if (f.differs_from_default === null) noDefault++;
    if (f.differs_from_baseline === true) {
      baselineDiffers++;
      if (f.baseline_off_default === true) baselineOffDefault++;
      else if (f.baseline_off_default === false) baselineAtDefault++;
      else baselineNoDefault++;
    }
    if (typeof f.differs_from_baseline === "boolean") baselineCompared++;
  }
  return { differs, noDefault, baselineDiffers, baselineCompared, baselineAtDefault, baselineOffDefault, baselineNoDefault };
}

/**
 * A new result starts, with a baseline applied, on the differences where the baseline is at its BIOS
 * default ("likely your settings"); when none are classified that way, on all differences from the
 * baseline; else on those that differ from the BIOS default, else on everything.
 */
export function initialDefaultsFilter(c: DefaultCounts, baseline = false): DefaultsFilter {
  if (baseline && c.baselineAtDefault > 0) return "baseline_at";
  if (baseline && c.baselineDiffers > 0) return "baseline";
  return c.differs > 0 ? "differs" : "all";
}

/** "N of M compared settings differ from the baseline", from the engine's counts when it gave them. */
export function baselineSummary(p: ExperimentalInspectPayload, c: DefaultCounts): { differing: number; compared: number } {
  return {
    differing: p.coverage.fields_differing_from_baseline ?? c.baselineDiffers,
    compared: p.coverage.fields_compared_with_baseline ?? c.baselineCompared,
  };
}

/** The engine's split of the differences from the baseline, else the counts from the fields. */
export function baselineSplit(p: ExperimentalInspectPayload, c: DefaultCounts): { atDefault: number; offDefault: number; noDefault: number } {
  const k = p.coverage;
  return {
    atDefault: k.baseline_differing_at_default ?? c.baselineAtDefault,
    offDefault: k.baseline_differing_off_default ?? c.baselineOffDefault,
    noDefault: k.baseline_differing_no_default ?? c.baselineNoDefault,
  };
}

/** The settings that could not be compared across two BIOS versions; null unless the baseline is applied across versions. */
export function crossNotCompared(p: ExperimentalInspectPayload): number | null {
  const b = p.inputs.baseline;
  if (!b || b.status !== "applied" || b.mode !== "cross_version") return null;
  return p.coverage.fields_not_compared_with_baseline ?? 0;
}

export const FIELD_STATE_LABEL:Record<FieldState, string> = {
  decoded: "Decoded",
  unknown_value: "Unknown value",
  unsupported_encoding: "Encoding not supported",
  out_of_bounds: "Outside the record",
};

export const MAPPING_LABEL: Record<MappingState, string> = {
  confirmed: "Confirmed mapping",
  inferred: "Inferred mapping",
  ambiguous: "Ambiguous mapping",
  unmapped: "Not mapped",
};

export const MAPPING_TONE: Record<MappingState, Tone> = {
  confirmed: "ok",
  inferred: "attention",
  ambiguous: "attention",
  unmapped: "neutral",
};

export const CONTAINER_LABEL: Record<ContainerState, string> = {
  established: "Known layout",
  candidate: "Candidate layout only",
  unrecognized: "Unrecognized layout",
};

export const IDENTITY_LABEL: Record<IdentityStatus, string> = {
  match: "Identifiers agree",
  mismatch: "Identifiers differ",
  unknown: "Not compared",
  conflicting: "Identifiers conflict",
};

export const IDENTITY_TONE: Record<IdentityStatus, Tone> = {
  match: "neutral",
  mismatch: "blocked",
  unknown: "neutral",
  conflicting: "blocked",
};

/** Deliberately no "verified" wording: a consistent checksum is not proof that the BIOS accepts the file. */
export const INTEGRITY_LABEL: Record<IntegrityStatus, string> = {
  verified: "Consistent with a known rule",
  candidate_match: "Matches a candidate rule only",
  failed: "Checksum does not match",
  unknown: "Not established",
};

export const INTEGRITY_TONE: Record<IntegrityStatus, Tone> = {
  verified: "neutral",
  candidate_match: "attention",
  failed: "blocked",
  unknown: "attention",
};

export const CHECK_LABEL = { pass: "Pass", fail: "Fail", not_evaluated: "Not evaluated" } as const;
export const CHECK_TONE = { pass: "ok", fail: "blocked", not_evaluated: "neutral" } as const;

/** Why an analysis request failed (not a diagnostic result: those are shown on the screen). */
export function failureOf(e: unknown): { title: string; message: string } {
  const message = errorMessage(e);
  const code = isCommandError(e) ? e.code : "";
  switch (code) {
    case "io":
      return { title: "Could not read a file", message };
    case "resource_limit":
      return { title: "The file is larger than the analysis limit", message };
    case "malformed_input":
      return { title: "The file's structure could not be read", message };
    case "identity_mismatch":
      return { title: "Profile and BIOS do not match", message };
    case "unsupported":
      return { title: "Unfamiliar layout", message };
    case "request_invalid":
      return { title: "The analysis request was not valid", message };
    case "binding_changed":
      return { title: "A file changed while it was being read", message };
    case "output_exists":
      return { title: "That file already exists", message };
    default:
      return { title: "The analysis did not finish", message };
  }
}

export function sameRequest(a: AnalysisRequest, b: AnalysisRequest): boolean {
  return (
    a.profile === b.profile &&
    a.bios === b.bios &&
    a.set === b.set &&
    (a.baseline ?? null) === (b.baseline ?? null) &&
    (a.baselineBios ?? null) === (b.baselineBios ?? null)
  );
}

/** A half-open byte range as the engine reports it: [start, end). */
export function rangeText(r: { start: number; end: number }): string {
  return `[${hex(r.start)}, ${hex(r.end)})`;
}

/** First bytes of a record preview, spaced for reading. */
export function previewText(hex: string, truncated: boolean): string {
  const bytes = hex.match(/../g) ?? [];
  return bytes.join(" ") + (truncated ? " …" : "");
}
