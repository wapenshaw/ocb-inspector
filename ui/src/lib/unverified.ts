// Adapter from the BIOS-derived analysis report to the rows Inspect already draws. It claims nothing
// the report does not: no binding, no capabilities, no checksum verdict of its own. Every row is a
// read-only `DecodedSetting` with no editing metadata; what only the experimental reader knows (the
// mapping state, where the bytes sit, what evidence the engine gave) travels beside the rows in
// `UnverifiedView`, so Inspect has to ask for it explicitly. Type-only imports keep this file pure.
import type {
  BiosSummary,
  DecodedSetting,
  ExperimentalField,
  ExperimentalInspectPayload,
  FieldState,
  IntegrityStatus,
  MappingState,
  RawInt,
  Value,
} from "../api/types";

/** What the experimental reader knows about one row, shown in the row inspector. */
export interface RowInfo {
  reviewed: ExperimentalField["reviewed"];
  defaultRaw: RawInt | null;
  mapping: MappingState;
  state: FieldState;
  /** Why the value is not interpreted, when the engine says. */
  note: string | null;
  /** Store name (as the BIOS calls it) and offset inside it. */
  store: string;
  storeOffset: number;
  widthBits: number;
  /** Bytes of the profile file this value was read from, [start, end). */
  span: { start: number; end: number } | null;
  raw: RawInt | null;
  hidden: boolean;
  conditional: boolean;
  callback: boolean;
  /** The engine's own one-line summaries of the evidence behind the mapping. */
  evidence: string[];
}

export interface UnverifiedView {
  bios: BiosSummary;
  set: number | null;
  /** From the profile's trailer; null when the report does not carry them. */
  savedAt: string | null;
  cpuBrand: string | null;
  /** Every field, one row each (the same store+offset can sit on several menu pages). */
  settings: DecodedSetting[];
  rows: ReadonlyMap<string, RowInfo>;
  recordsTotal: number;
  /** Profile records that gave at least one setting. */
  recordsWithSettings: number;
  /** Settings the BIOS gives no default for: they can be judged neither way. */
  noDefault: number;
  /** Settings whose stored value differs from the BIOS default. */
  differs: number;
  identityReason: string;
  integrity: IntegrityStatus;
}

const NOT_ESTABLISHED = "Applicability not established: the BIOS's own show/hide conditions are not evaluated.";

/** The value as the BIOS labels it, else the number as stored. No unit, no sign. */
function valueOf(kind: ExperimentalField["kind"], raw: RawInt | null, label: string | null, decoded: boolean): Value | null {
  if (!raw) return null;
  const n = Number(raw.dec);
  if (!Number.isSafeInteger(n)) return { kind: "enum", symbol: raw.dec, label: raw.dec };
  if (label !== null) return { kind: "enum", symbol: raw.dec, raw: n, label };
  if (kind === "numeric" && decoded) return { kind: "integer", value: n, unit: "" };
  return { kind: "unknown", raw: n };
}

function rowOf(f: ExperimentalField, storeNames: Map<string, string>, evidence: Map<string, string>): [DecodedSetting, RowInfo] {
  const decoded = f.state === "decoded";
  const flags = [f.hidden && "hidden in the menus", f.conditional && "shown only under a condition"].filter(Boolean).join(", ");
  const setting: DecodedSetting = {
    setting_id: `bios.${f.key}`,
    group: "bios",
    label: f.label,
    summary: "",
    available: true,
    // An unknown or uninterpreted value never carries the BIOS's label: the report gives none.
    value: valueOf(f.kind, f.raw, decoded ? f.value_label : null, decoded),
    activity: "unknown",
    activity_reason: flags ? `${NOT_ESTABLISHED} This setting is ${flags}.` : NOT_ESTABLISHED,
    editability: { writable: false, auto_only: false, reason: "Shown from the BIOS's own definition, read only." },
    constraints: null,
    fields: [],
    evidence: [],
    menu: { path: f.menu_path, section: f.section, prompt: f.label, order: f.menu_order },
    bios_help: f.help || null,
    default_value: valueOf(f.kind, f.default_raw, f.default_label, true),
    differs_from_default: f.differs_from_default,
  };
  const info: RowInfo = {
    reviewed: f.reviewed,
    defaultRaw: f.default_raw,
    mapping: f.mapping,
    state: f.state,
    note: f.note,
    store: storeNames.get(f.store) ?? f.store,
    storeOffset: f.store_offset,
    widthBits: f.width_bits,
    span: f.span,
    raw: f.raw,
    hidden: f.hidden,
    conditional: f.conditional,
    callback: f.callback,
    evidence: f.evidence.map((id) => evidence.get(id) ?? id),
  };
  return [setting, info];
}

/** The saved time and CPU the profile's own trailer holds, if the engine found one. */
function trailerOf(p: ExperimentalInspectPayload): { savedAt: string | null; cpuBrand: string | null } {
  const t = p.container_candidates.find((c) => c.id === p.selected_candidate)?.trailers[0];
  const stamp = t?.strings.find((s) => /^\d{14}$/.test(s)) ?? null;
  // Header id, revision, CPU brand, time: the brand is the string before the time.
  const i = stamp && t ? t.strings.indexOf(stamp) : -1;
  return { savedAt: stamp, cpuBrand: i >= 2 ? (t?.strings[i - 1] ?? null) : null };
}

/**
 * The primary read-only Inspect view for every BIOS/profile pair, or null when
 * there is nothing to show yet: identity not matched, no firmware set chosen, or no settings.
 */
export function unverifiedView(p: ExperimentalInspectPayload, bios: BiosSummary): UnverifiedView | null {
  if (p.identity.status !== "match" || p.fields.length === 0) return null;
  if (p.inputs.bios && p.inputs.bios.sets.length > 1 && p.inputs.bios.set === null) return null;
  const storeNames = new Map(p.stores.map((s): [string, string] => [s.key, s.name]));
  const evidence = new Map(p.evidence.map((e): [string, string] => [e.id, e.summary]));
  const settings: DecodedSetting[] = [];
  const rows = new Map<string, RowInfo>();
  const records = new Set<string>();
  let noDefault = 0;
  let differs = 0;
  for (const f of p.fields) {
    const [setting, info] = rowOf(f, storeNames, evidence);
    settings.push(setting);
    rows.set(setting.setting_id, info);
    records.add(f.record);
    if (f.differs_from_default === null) noDefault++;
    else if (f.differs_from_default) differs++;
  }
  return {
    bios,
    set: p.inputs.bios?.set ?? null,
    ...trailerOf(p),
    settings,
    rows,
    recordsTotal: p.coverage.records_total,
    recordsWithSettings: records.size,
    noDefault,
    differs,
    identityReason: p.identity.reason,
    integrity: p.integrity.status,
  };
}
