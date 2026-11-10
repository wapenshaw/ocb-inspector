// A small, hand-written profile-format report in the engine's shape, with no proprietary data.
// It covers a traced release (saved and skipped positions, a size-tolerant entry, an entry no menu
// reads) and an untraced one (every position "unknown", parts marked expected). Tests use it, and
// the browser preview falls back to it when recorded reports are absent (ui/public/mock is ignored).
import type { ListedEntryInfo, ProfileFormatPayload, Report } from "../api/types";

export const FORMAT_BIOS_SHA = "5e4d3c2b".repeat(8);

const SETUP_GUID = "EC87D643-EBA4-4BB5-A1E5-3F3E36B20DA9";
const OTHER_GUID = "11111111-2222-3333-4444-555555555555";

function entry(index: number, name: string, declared: number, store: number | null, fields: number, saved: ListedEntryInfo["saved"], note: string | null = null): ListedEntryInfo {
  return { index, guid: index === 0 ? SETUP_GUID : OTHER_GUID, name, attributes: 7, declared_size: declared, store_size: store, field_count: fields, saved, note };
}

export function profileFormatFixture(opts: { traced?: boolean } = {}): Report<ProfileFormatPayload> {
  const traced = opts.traced ?? true;
  const saved = (v: ListedEntryInfo["saved"]) => (traced ? v : "unknown");
  const basis = traced ? "firmware_code" : "expected";
  return {
    kind: "ocb.report",
    schema_version: "1.0",
    operation: "profile_format",
    payload_version: "1.0",
    diagnostics: [],
    payload: {
      bios: {
        sha256: FORMAT_BIOS_SHA,
        size: 33554432,
        bios_id: "E7D91IMS.HJ0",
        set: 0,
        sets: [],
        family: "Intel LGA1700, 700 series",
        profiles_supported: true,
      },
      traced,
      code: [
        { name: "AMITSE", guid: "B1DA0ADF-4F77-4070-A88E-BFFE1C60529A", sha256: "aa".repeat(32), holds_mos: true, holds_oci: false },
        { name: "OcProfileInfo", guid: "A526D2DC-0000-0000-0000-000000000000", sha256: "bb".repeat(32), holds_mos: true, holds_oci: true },
      ],
      layout: [
        { part: "header", detail: "$MOS$, then the BIOS id over its full length (12 bytes: E7D91IMS.HJ0), a 32-bit checksum and the record count", basis },
        { part: "checksum", detail: "Minus the 32-bit wrapping sum of every other byte of the file", basis },
        { part: "records", detail: "Each record is a u32 id, a u16 size and the variable's bytes; the id is the variable's position in the list below, and records follow list order", basis },
        ...(traced ? [{ part: "skipped", detail: "Positions 3, 4 and the variable OcInfoPassToSetup are never saved", basis: "firmware_code" as const }] : []),
        { part: "cmos", detail: "256 bytes: the CMOS contents at save time; loading writes bytes 0x40-0xFF back", basis },
        { part: "trailer", detail: "$OCI$, then board, revision, CPU brand and save time, then six numbers of unknown meaning (code in OcProfileInfo)", basis: traced ? "firmware_code" : "image_structure" },
      ],
      variable_list: {
        container_path: "fv@0x1090000/FE612B72-203C-47B1-8560-A66D946EB371/lzma",
        sha256: "762c7246".repeat(8),
        entries: [
          entry(0, "Setup", 5081, 5081, 2723, saved("saved")),
          entry(1, "PlatformLang", 6, 2, 1, saved("saved"), traced ? "loading accepts a different size for this record" : null),
          entry(2, "Timeout", 2, 2, 1, saved("saved")),
          entry(3, "UserDefaults", 0, null, 0, saved("skipped"), "the setup data declares no size"),
          entry(4, "ErrorManager", 0, null, 0, saved("skipped"), "the setup data declares no size"),
          entry(5, "CpuSetup", 961, 961, 540, saved("saved")),
          entry(6, "OcInfoPassToSetup", 4, null, 0, saved("skipped")),
        ],
      },
      list_problem: null,
      limitations: [
        "Exported profiles have been checked against this layout only on MS-7D91",
        ...(traced ? [] : ["This release's profile code was not traced: parts marked expected follow the traced releases, and which positions are saved is unknown"]),
        "Record ids belong to this exact release: another release's list can shift them",
      ],
    },
  };
}
