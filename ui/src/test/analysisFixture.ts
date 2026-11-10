// A small, hand-written analysis report in the engine's shape, with no proprietary data. It covers
// every state the Analyze screen renders: confirmed, inferred, ambiguous and unmapped records;
// decoded, unknown-value and unsupported fields; unknown integrity; a two-set BIOS with and without
// a chosen set. Tests use it, and the browser preview falls back to it when the recorded analyses
// are absent (ui/public/mock is git-ignored).
import type {
  ContainerCandidateInfo,
  Coverage,
  Evidence,
  ExperimentalField,
  ExperimentalInspectPayload,
  RecordInfo,
  Report,
  StoreInfo,
} from "../api/types";

export const PROFILE_SHA = "a1b2c3d4".repeat(8);
export const BIOS_SHA = "9f8e7d6c".repeat(8);
export const PROFILE_BIOS_ID = "E7D91IMS.HI0";
export const SET_LABELS = ["Set A", "Set B"];

export interface FixtureOptions {
  /** A BIOS image was supplied (false: profile only). */
  bios?: boolean;
  /** Firmware set index that was chosen; null when none was. */
  set?: number | null;
  /** Extra decoded settings appended to the Setup store (for row-limit tests). */
  extra?: number;
  /** No setting differs from its optimized default (the two that do are reported as equal). */
  noDiffs?: boolean;
  /**
   * A baseline profile was supplied. "applied": it is compared field by field (Bootup NumLock State and
   * PCH Thermal Throttling differ from it; the two fields that differ from the BIOS default do not
   * both). Bootup NumLock State has the baseline off its own BIOS default ("filled in by the firmware?"), PCH Thermal
   * Throttling at it ("likely your settings"). "blocked": the engine did not use it and annotates nothing.
   */
  baseline?: "applied" | "blocked";
  /** With an applied baseline: it was saved on another BIOS release (matched by menu place; 3 settings not compared). */
  cross?: boolean;
}

export const BASELINE_SHA = "0f1e2d3c".repeat(8);
export const BASELINE_BIOS_SHA = "7a6b5c4d".repeat(8);
export const BLOCKED_REASON = "The baseline profile was saved by a different BIOS release.";

const SETUP_GUID = "EC87D643-EBA4-4BB5-A1E5-3F3E36B20DA9";
const PCH_GUID = "4570B7F1-ADE8-4943-8DC3-406472842384";
const SECURE_GUID = "7B59104A-C00D-4158-87FF-F04D6396A915";
const SI_GUID = "AAF8E719-48F8-4099-A6F7-645FBD694C3D";
const SECTION = (s: number, e: number) => ({ start: s, end: e });

export function analysisFixture(opts: FixtureOptions = {}): Report<ExperimentalInspectPayload> {
  const hasBios = opts.bios ?? true;
  const set = hasBios ? (opts.set ?? null) : null;
  const interpreted = set !== null;
  const sets = hasBios ? SET_LABELS : [];
  const at = (name: string, guid: string, size: number) => `set${set ?? 0}:${guid}:${name}:${size}`;

  const setup = at("Setup", SETUP_GUID, 5081);
  const pch = at("PchSetup", PCH_GUID, 2063);
  const secureBoot = at("SecureBootSetup", SECURE_GUID, 7);
  const securePresent = at("SecureVarPresent", SECURE_GUID, 7);
  const si = at("SiSetup", SI_GUID, 7);

  const noSetReason = hasBios ? "No firmware set chosen: settings need one set" : "No BIOS image: file structure only";
  const records: RecordInfo[] = [
    {
      key: "r0", index: 0, id: 0, header: SECTION(25, 31), payload: SECTION(31, 5112), size: 5081,
      mapping: interpreted ? "confirmed" : "unmapped",
      candidate_stores: interpreted ? [setup] : [],
      variable: null,
      reason: interpreted ? "setup_size_binding/1: Setup is the only record of its size and the BIOS's variable list names it" : noSetReason,
      evidence: interpreted ? ["ev:map:r0"] : [],
      preview_hex: "0100010101010100000101000001000101", preview_truncated: true,
    },
    {
      key: "r1", index: 1, id: 5, header: SECTION(5112, 5118), payload: SECTION(5118, 5125), size: 7,
      mapping: interpreted ? "ambiguous" : "unmapped",
      candidate_stores: interpreted ? [secureBoot, securePresent, si] : [],
      variable: null,
      reason: interpreted ? "3 stores are 7 bytes: SecureBootSetup, SecureVarPresent, SiSetup" : noSetReason,
      evidence: interpreted ? ["ev:map:r1"] : [],
      preview_hex: "656e2d55530000", preview_truncated: false,
    },
    {
      key: "r2", index: 2, id: 7, header: SECTION(5125, 5131), payload: SECTION(5131, 7194), size: 2063,
      mapping: interpreted ? "inferred" : "unmapped",
      candidate_stores: interpreted ? [pch] : [],
      variable: null,
      reason: interpreted ? "unique_size_pair/1: the only 2063-byte record and the only 2063-byte store (PchSetup)" : noSetReason,
      evidence: interpreted ? ["ev:map:r2"] : [],
      preview_hex: "0000000000000000000000000000000000", preview_truncated: true,
    },
    {
      key: "r3", index: 3, id: 9, header: SECTION(7194, 7200), payload: SECTION(7200, 7264), size: 64,
      mapping: "unmapped", candidate_stores: [],
      variable: interpreted ? { index: 9, guid: "D1405D16-7AFC-4695-BB12-41459D3695A2", name: "NetworkStackVar", declared_size: 64 } : null,
      reason: interpreted ? "NetworkStackVar (by the BIOS's variable list); no BIOS menu reads this variable" : "no store has 64 bytes", evidence: interpreted ? ["ev:map:r3"] : [],
      preview_hex: "ff00aa55", preview_truncated: false,
    },
  ];

  const stores: StoreInfo[] = interpreted
    ? [
        {
          key: setup, name: "Setup", guid: SETUP_GUID, kind: "buffer", size: 5081, set: set ?? 0,
          declared_by: [{ formset_guid: SETUP_GUID, formset_title: "Setup", module: "Setup", module_sha256: "bf408f35".repeat(8), local_id: 61460 }],
          support: "supported", candidate_records: ["r0"], mapping: "confirmed", mapped_record: "r0",
          reason: "setup_size_binding/1: Setup is the only record of its size and the BIOS's variable list names it", evidence: ["ev:map:r0"], field_count: 0,
        },
        {
          key: pch, name: "PchSetup", guid: PCH_GUID, kind: "buffer", size: 2063, set: set ?? 0,
          declared_by: [{ formset_guid: PCH_GUID, formset_title: "", module: "PchInit", module_sha256: "0a1b2c3d".repeat(8), local_id: 7 }],
          support: "supported", candidate_records: ["r2"], mapping: "inferred", mapped_record: "r2",
          reason: "unique_size_pair/1: the only 2063-byte record and the only 2063-byte store (PchSetup)", evidence: ["ev:map:r2"], field_count: 1,
        },
        ...[
          { key: secureBoot, name: "SecureBootSetup", guid: SECURE_GUID },
          { key: securePresent, name: "SecureVarPresent", guid: SECURE_GUID },
          { key: si, name: "SiSetup", guid: SI_GUID },
        ].map(
          (x): StoreInfo => ({
            key: x.key, name: x.name, guid: x.guid, kind: "buffer", size: 7, set: set ?? 0,
            declared_by: [{ formset_guid: x.guid, formset_title: "", module: "SecureBoot", module_sha256: "5e6f7a8b".repeat(8), local_id: 3 }],
            support: "supported", candidate_records: ["r1"], mapping: "ambiguous", mapped_record: null,
            reason: "3 stores are 7 bytes: SecureBootSetup, SecureVarPresent, SiSetup", evidence: ["ev:map:r1"], field_count: 0,
          }),
        ),
        {
          key: `set${set ?? 0}:0C1D2E3F-0000-4000-8000-000000000001:OldVarStore:300`, name: "OldVarStore",
          guid: "0C1D2E3F-0000-4000-8000-000000000001", kind: "efi", size: 300, set: set ?? 0,
          declared_by: [], support: "supported", candidate_records: [], mapping: "unmapped", mapped_record: null,
          reason: "no record has 300 bytes", evidence: [], field_count: 0,
        },
      ]
    : [];

  const base = { help: "", hidden: false, conditional: false, callback: false };
  const fields: ExperimentalField[] = interpreted
    ? [
        {
          ...base, key: `${setup}@0/8`, store: setup, record: "r0", label: "Bootup NumLock State",
          help: "Sets the NumLock status while system is booting.", menu_path: ["Boot"], menu_order: [357913, 561533],
          section: "Boot Configuration", kind: "one_of", store_offset: 0, width_bits: 8, span: SECTION(31, 32),
          raw: { dec: "1", hex: "0x1" }, state: "decoded", value_label: "On", note: null, mapping: "confirmed",
          default_raw: { dec: "1", hex: "0x1" }, default_label: "On", differs_from_default: false,
          evidence: ["ev:map:r0", "ev:identity"],
        },
        {
          ...base, key: `${setup}@4513/16`, store: setup, record: "r0", label: "Fan select Main page",
          menu_path: ["Hardware Monitor"], menu_order: [4294967295, 570786], section: null, kind: "one_of",
          store_offset: 4513, width_bits: 16, span: SECTION(4544, 4546), raw: { dec: "0", hex: "0x0" },
          state: "unknown_value", value_label: null, note: "no option of this setting has this value",
          mapping: "confirmed", default_raw: { dec: "0", hex: "0x0" }, default_label: null,
          differs_from_default: false, conditional: true, callback: true, evidence: ["ev:map:r0"],
        },
        {
          ...base, key: `${setup}@2958/8`, store: setup, record: "r0", label: "DMI Gen3 RTCO Cpre Lane0",
          menu_path: ["Chipset", "System Agent (SA) Configuration", "DMI/OPI Configuration", "DMI Advanced Menu"],
          menu_order: [357883, 426163, 434149, 438349, 439661], section: "DMI Advanced Menu", kind: "numeric",
          store_offset: 2958, width_bits: 8, span: SECTION(2989, 2990), raw: { dec: "5", hex: "0x5" },
          state: "decoded", value_label: null, note: "raw integer, no unit or sign applied", mapping: "confirmed",
          default_raw: { dec: "0", hex: "0x0" }, default_label: null, differs_from_default: true, evidence: ["ev:map:r0"],
        },
        {
          ...base, key: `${setup}@3000/16`, store: setup, record: "r0", label: "Vendor option 9", menu_path: [],
          menu_order: [4294967295, 9], section: null, kind: "numeric", store_offset: 3000, width_bits: 16,
          span: SECTION(3001, 3003), raw: { dec: "9", hex: "0x9" }, state: "unsupported_encoding", value_label: null,
          note: "this encoding is not supported by this analysis", mapping: "confirmed", default_raw: null,
          default_label: null, differs_from_default: null, evidence: ["ev:map:r0"],
        },
        {
          ...base, key: `${setup}@9000/8`, store: setup, record: "r0", label: "Legacy option", menu_path: ["Advanced"],
          menu_order: [1, 2], section: null, kind: "one_of", store_offset: 9000, width_bits: 8, span: null, raw: null,
          state: "out_of_bounds", value_label: null, note: "the offset lies outside the record", mapping: "confirmed",
          default_raw: null, default_label: null, differs_from_default: null, evidence: ["ev:map:r0"],
        },
        {
          ...base, key: `${pch}@1/8`, store: pch, record: "r2", label: "PCH Thermal Throttling",
          help: "Lets the chipset throttle when hot.", menu_path: ["Chipset", "PCH-IO Configuration"],
          menu_order: [357883, 900001], section: null, kind: "one_of", store_offset: 1, width_bits: 8,
          span: SECTION(5132, 5133), raw: { dec: "1", hex: "0x1" }, state: "decoded", value_label: "Enabled", note: null,
          mapping: "inferred", default_raw: { dec: "0", hex: "0x0" }, default_label: "Disabled",
          differs_from_default: true, evidence: ["ev:map:r2"],
        },
        ...Array.from({ length: opts.extra ?? 0 }, (_, i): ExperimentalField => ({
          ...base, key: `${setup}@extra${i}`, store: setup, record: "r0", label: `Extra setting ${i}`,
          menu_path: [`Page ${i % 9}`], menu_order: [i % 9, i], section: null, kind: "numeric", store_offset: 100 + i,
          width_bits: 8, span: null, raw: { dec: String(i % 3), hex: `0x${(i % 3).toString(16)}` }, state: "decoded",
          value_label: null, note: null, mapping: "confirmed", default_raw: null, default_label: null,
          differs_from_default: null, evidence: ["ev:map:r0"],
        })),
      ]
    : [];

  if (opts.baseline === "applied") {
    // [raw, label, differs from baseline, baseline off its own default]
    const stored: Record<string, [string, string | null, boolean, boolean]> = {
      "Bootup NumLock State": ["0", "Off", true, true],
      "Fan select Main page": ["0", null, false, false],
      "DMI Gen3 RTCO Cpre Lane0": ["5", null, false, true],
      "PCH Thermal Throttling": ["0", "Disabled", true, false],
    };
    for (const f of fields) {
      const b = stored[f.label];
      if (!b) continue; // not compared: differs_from_baseline stays absent
      f.baseline_raw = { dec: b[0], hex: `0x${Number(b[0]).toString(16)}` };
      f.baseline_label = b[1];
      f.differs_from_baseline = b[2];
      f.baseline_off_default = b[3];
      f.baseline_match = opts.cross ? "menu_place" : "key";
    }
  }

  if (opts.noDiffs) for (const f of fields) if (f.differs_from_default === true) f.differs_from_default = false;

  const storeFieldCount = (k: string) => fields.filter((f) => f.store === k).length;
  const storesWithCounts = stores.map((st) => ({ ...st, field_count: storeFieldCount(st.key) }));

  const evidence: Evidence[] = [
    {
      id: "ev:container:known_layout", kind: "container", basis: "file_structure", summary: "known_layout fits the file",
      observations: ["magic: Pass ($MOS$ at 0x00)", "record_chain: Pass (4 records end at 0x1c60)"],
      contradictions: [], limitations: ["Layout established on the profiles examined so far; elsewhere a candidate only"],
    },
    {
      id: "ev:identity", kind: "identity", basis: "file_structure", summary: hasBios ? "profile and BIOS identifiers agree" : "no BIOS image to compare with",
      observations: [`header: ${PROFILE_BIOS_ID} at 0x05..0x11`],
      contradictions: [], limitations: ["Identifiers say which BIOS saved the file; they are not a cryptographic proof"],
    },
    {
      id: "ev:integrity", kind: "integrity", basis: "file_structure", summary: "no checksum rule applies to this layout",
      observations: [], contradictions: [], limitations: ["Integrity is not established: values are not confirmed as stored by the BIOS"],
    },
    {
      id: "ev:map:r0", kind: "mapping", basis: "reviewed_binding", summary: "setup_size_binding/1: Setup is the only record of its size and the BIOS's variable list names it",
      observations: ["record r0 is 5081 bytes, the size of the Setup store"], contradictions: [],
      limitations: ["Confirmed only for the reviewed adapter boards"],
    },
    {
      id: "ev:map:r1", kind: "mapping", basis: "size_only", summary: "three 7-byte stores fit this record",
      observations: ["SecureBootSetup, SecureVarPresent and SiSetup are all 7 bytes"], contradictions: [],
      limitations: ["Order and the values stored do not decide between them"],
    },
    {
      id: "ev:map:r2", kind: "mapping", basis: "size_only", summary: "unique_size_pair/1: PchSetup",
      observations: ["the only 2063-byte record and the only 2063-byte store"], contradictions: [],
      limitations: ["No reviewed rule for this scope; inferred only"],
    },
    {
      id: "ev:map:r3", kind: "mapping", basis: "firmware_code", summary: "no store has this record's size",
      observations: ["record r3 is 64 bytes"], contradictions: [], limitations: [],
    },
    {
      id: "ev:identity:ok", kind: "rule", basis: "file_structure", summary: "rule: identifiers must agree before values are read",
      observations: [], contradictions: [], limitations: [],
    },
  ];

  const candidates: ContainerCandidateInfo[] = [
    {
      id: "known_layout", rule: "known_layout/1",
      assumptions: ["Layout established on the profiles examined so far; elsewhere a candidate only", "12-byte identifier at 0x05"],
      header_id: PROFILE_BIOS_ID, header_id_span: SECTION(5, 17), checksum_at: 17, count_at: 21, records_at: 25,
      record_count: records.length, records_span: SECTION(25, 7264), chain_error: null, trailers: [],
      checks: [
        { name: "magic", status: "pass", detail: "$MOS$ at 0x00" },
        { name: "record_chain", status: "pass", detail: `${records.length} records end at 0x1c60` },
        { name: "oci_timestamp", status: "not_evaluated", detail: "no $OCI$ trailer in this excerpt" },
      ],
      eligible: true, contradictions: [],
      integrity: {
        status: "unknown", rule: null, scope: "no checksum rule for this layout", checksum_at: null, stored: null,
        computed: null, limitations: ["No checksum rule is established for this layout"],
      },
    },
    {
      id: "alternate_layout", rule: "alternate/0", assumptions: ["Assumes a 16-byte identifier"],
      header_id: PROFILE_BIOS_ID, header_id_span: SECTION(5, 21), checksum_at: 21, count_at: 25, records_at: 29,
      record_count: null, records_span: null, chain_error: "record 2 runs past the end of the file", trailers: [],
      checks: [{ name: "record_chain", status: "fail", detail: "record 2 runs past the end of the file" }],
      eligible: false, contradictions: ["record ids do not increase at record 1"],
      integrity: {
        status: "unknown", rule: null, scope: "not applicable", checksum_at: null, stored: null, computed: null, limitations: [],
      },
    },
  ];

  const mappedBytes = records.filter((r) => r.mapping === "confirmed" || r.mapping === "inferred").reduce((n, r) => n + r.size, 0);
  const coverage: Coverage = {
    records_total: records.length,
    records_confirmed: records.filter((r) => r.mapping === "confirmed").length,
    records_inferred: records.filter((r) => r.mapping === "inferred").length,
    records_ambiguous: records.filter((r) => r.mapping === "ambiguous").length,
    records_unmapped: records.filter((r) => r.mapping === "unmapped").length,
    fields_total: fields.length,
    fields_decoded: fields.filter((f) => f.state === "decoded").length,
    fields_unknown_value: fields.filter((f) => f.state === "unknown_value").length,
    fields_unsupported: fields.filter((f) => f.state === "unsupported_encoding").length,
    fields_out_of_bounds: fields.filter((f) => f.state === "out_of_bounds").length,
    profile_bytes: 11916,
    mapped_bytes: mappedBytes,
    interpreted_bytes: fields.filter((f) => f.state === "decoded").length,
    opaque_bytes: 11916 - mappedBytes,
    ...(opts.baseline === "applied"
      ? {
          fields_compared_with_baseline: fields.filter((f) => typeof f.differs_from_baseline === "boolean").length,
          fields_differing_from_baseline: fields.filter((f) => f.differs_from_baseline === true).length,
          baseline_differing_at_default: fields.filter((f) => f.differs_from_baseline === true && f.baseline_off_default === false).length,
          baseline_differing_off_default: fields.filter((f) => f.differs_from_baseline === true && f.baseline_off_default === true).length,
          baseline_differing_no_default: fields.filter((f) => f.differs_from_baseline === true && f.baseline_off_default == null).length,
          ...(opts.cross ? { fields_not_compared_with_baseline: 3 } : {}),
        }
      : {}),
  };

  const reasons = !hasBios
    ? ["No BIOS image: file structure only"]
    : !interpreted
      ? [`No firmware set chosen: this BIOS has ${sets.length} sets, so settings are not interpreted until one is chosen`]
      : [
          `${fields.length} settings interpreted from ${coverage.records_confirmed} confirmed and ${coverage.records_inferred} inferred record; mappings marked inferred are not confirmed`,
          "Integrity is not established for this layout",
        ];

  const payload: ExperimentalInspectPayload = {
    analysis: {
      analyzer: "ocb-experimental-inspect", version: "0.1.0", mode: "experimental_read_only",
      rules: ["known_layout/1", "unique_size_pair/1", "setup_size_binding/1"], completion: "complete",
    },
    inputs: {
      profile_sha256: PROFILE_SHA,
      profile_size: 11916,
      bios: hasBios
        ? { sha256: BIOS_SHA, size: 33554432, bios_id: PROFILE_BIOS_ID, set, sets, family: "Intel LGA1700, 700 series", profiles_supported: true }
        : null,
      ...(opts.baseline
        ? {
            baseline: {
              sha256: BASELINE_SHA,
              size: 11916,
              status: opts.baseline,
              reason: opts.baseline === "applied" ? "Applied." : BLOCKED_REASON,
              ...(opts.baseline === "applied" && opts.cross
                ? {
                    mode: "cross_version" as const,
                    bios: { sha256: BASELINE_BIOS_SHA, size: 33554432, bios_id: "E7D91IMS.HJ0", set: null, sets: [], family: "Intel LGA1700, 700 series", profiles_supported: true },
                  }
                : {}),
            },
          }
        : {}),
    },
    container: "established",
    status_reasons: reasons,
    identity: {
      status: hasBios ? "match" : "unknown",
      reason: hasBios ? `profile and BIOS are both ${PROFILE_BIOS_ID}` : `profile was saved on ${PROFILE_BIOS_ID}; no BIOS image supplied. Open that BIOS image (MSI version V17.I) to read the settings.`,
      profile_bios_id: PROFILE_BIOS_ID,
      observed: [
        { source: "header", text: PROFILE_BIOS_ID, spans: [SECTION(5, 17)], board: "7D91", flavor: "IMS", revision: "HI0" },
        { source: "oci", text: PROFILE_BIOS_ID, spans: [SECTION(11827, 11835)], board: "7D91", flavor: "IMS", revision: "HI0" },
      ],
      limitations: ["Identifiers say which BIOS saved the file; they are not a cryptographic proof"],
    },
    integrity: {
      status: "unknown", rule: null, scope: "no checksum rule for this layout", checksum_at: null, stored: null,
      computed: null, limitations: ["No checksum rule is established for this layout"],
    },
    container_candidates: candidates,
    selected_candidate: "known_layout",
    records,
    stores: storesWithCounts,
    fields,
    evidence,
    coverage,
    observations: [{ kind: "signature", span: SECTION(0, 5), text: "$MOS$" }],
    capabilities: {
      profile_write: false, change_planning: false, migration: false, profile_synthesis: false, analysis_export: true,
    },
  };

  return {
    kind: "ocb.report",
    schema_version: "1.0",
    operation: "experimental_inspect",
    payload_version: "1.0",
    payload,
    diagnostics: [],
  };
}
