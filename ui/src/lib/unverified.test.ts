import type { BiosSummary, ExperimentalInspectPayload } from "../api/types";
import { analysisFixture } from "../test/analysisFixture";
import { unverifiedView } from "./unverified";

const bios = { bios_id: "E7D91IMS.HI0", profiles_supported: false } as unknown as BiosSummary;
const payload = (opts: Parameters<typeof analysisFixture>[0] = { set: 0 }): ExperimentalInspectPayload => analysisFixture(opts).payload;
const view = (p = payload()) => {
  const v = unverifiedView(p, bios);
  if (!v) throw new Error("no view");
  return v;
};
const row = (v: ReturnType<typeof view>, label: string) => {
  const s = v.settings.find((x) => x.label === label);
  if (!s) throw new Error(`no row ${label}`);
  return s;
};

describe("unverifiedView", () => {
  it("gives one row per field with a unique id, and keeps the same store and offset on several menu pages", () => {
    const p = payload();
    const first = p.fields[0]!;
    p.fields.push({ ...first, key: `${first.key}#second`, menu_path: ["Overclocking"], menu_order: [9, 9] });
    const v = view(p);
    expect(v.settings).toHaveLength(p.fields.length);
    expect(new Set(v.settings.map((s) => s.setting_id)).size).toBe(p.fields.length);
    const same = v.settings.filter((s) => s.label === first.label);
    expect(same.map((s) => s.menu?.path.join("/"))).toEqual(["Boot", "Overclocking"]);
  });

  it("shows the BIOS's own label, else the raw number, and adds no unit or sign", () => {
    const v = view();
    expect(row(v, "Bootup NumLock State").value).toEqual({ kind: "enum", symbol: "1", raw: 1, label: "On" });
    expect(row(v, "DMI Gen3 RTCO Cpre Lane0").value).toEqual({ kind: "integer", value: 5, unit: "" });
    expect(row(v, "PCH Thermal Throttling").default_value).toEqual({ kind: "enum", symbol: "0", raw: 0, label: "Disabled" });
  });

  it("keeps fields that were not decoded, as unknown or without a value", () => {
    const v = view();
    expect(row(v, "Fan select Main page").value).toEqual({ kind: "unknown", raw: 0 });
    expect(row(v, "Vendor option 9").value).toEqual({ kind: "unknown", raw: 9 });
    expect(v.rows.get(row(v, "Vendor option 9").setting_id)?.state).toBe("unsupported_encoding");
    const p = payload();
    const out = p.fields.find((f) => f.state === "out_of_bounds")!;
    out.raw = null;
    expect(row(view(p), out.label).value).toBeNull();
  });

  it("does not turn a raw number beyond the safe integers into a rounded one", () => {
    const p = payload();
    const f = p.fields.find((x) => x.label === "DMI Gen3 RTCO Cpre Lane0")!;
    f.raw = { dec: "18446744073709551615", hex: "0xFFFFFFFFFFFFFFFF" };
    expect(row(view(p), f.label).value).toEqual({ kind: "enum", symbol: "18446744073709551615", label: "18446744073709551615" });
  });

  it("leaves a setting with no BIOS default without one, neither at it nor off it", () => {
    const v = view();
    const none = row(v, "Vendor option 9");
    expect(none.default_value).toBeNull();
    expect(none.differs_from_default).toBeNull();
    expect(v.noDefault).toBe(v.settings.filter((s) => s.differs_from_default === null).length);
    expect(v.noDefault).toBeGreaterThan(0);
    expect(v.differs).toBe(v.settings.filter((s) => s.differs_from_default === true).length);
  });

  it("claims nothing: read only, no constraints, no stored-byte evidence", () => {
    for (const s of view().settings) {
      expect(s.group).toBe("bios");
      expect(s.editability.writable).toBe(false);
      expect(s.constraints).toBeNull();
      expect(s.fields).toEqual([]);
      expect(s.menu).not.toBeNull();
    }
  });

  it("carries the mapping, where the value sits and the engine's evidence text beside the rows", () => {
    const v = view();
    const pch = v.rows.get(row(v, "PCH Thermal Throttling").setting_id)!;
    expect(pch).toMatchObject({ mapping: "inferred", store: "PchSetup", storeOffset: 1, widthBits: 8, span: { start: 5132, end: 5133 } });
    expect(pch.evidence).toEqual(["unique_size_pair/1: PchSetup"]);
    expect(v.rows.get(row(v, "Bootup NumLock State").setting_id)?.mapping).toBe("confirmed");
  });

  it("counts the records that gave settings against all records, and carries identity and integrity", () => {
    const p = payload();
    const v = view(p);
    expect(v.recordsTotal).toBe(p.coverage.records_total);
    expect(v.recordsWithSettings).toBe(new Set(p.fields.map((f) => f.record)).size);
    expect(v.integrity).toBe("unknown");
    expect(v.identityReason).toBe(p.identity.reason);
  });

  it("reads the save time and CPU from the profile's trailer when there is one", () => {
    const p = payload();
    expect(view(p)).toMatchObject({ savedAt: null, cpuBrand: null });
    const c = p.container_candidates.find((x) => x.id === p.selected_candidate)!;
    c.trailers = [{ at: 1, gap: 0, strings: ["E7D91IMS", "HI0", "Test CPU @ 3.0GHz", "20261001120000"], spans: [], tail_len: 24, timestamp_valid: true }];
    expect(view(p)).toMatchObject({ savedAt: "20261001120000", cpuBrand: "Test CPU @ 3.0GHz" });
  });

  it("gives nothing to show without a confirmed identity, a chosen firmware set or any field", () => {
    const mismatch = payload();
    mismatch.identity.status = "mismatch";
    expect(unverifiedView(mismatch, bios)).toBeNull();
    for (const status of ["unknown", "conflicting"] as const) {
      const p = payload();
      p.identity.status = status;
      expect(unverifiedView(p, bios)).toBeNull();
    }
    expect(unverifiedView(payload({ set: null }), bios)).toBeNull();
    const empty = payload();
    empty.fields = [];
    expect(unverifiedView(empty, bios)).toBeNull();
  });
});
