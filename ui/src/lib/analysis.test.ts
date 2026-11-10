import type { ExperimentalField } from "../api/types";
import { analysisFixture } from "../test/analysisFixture";
import {
  BASELINE_FILTERS,
  DEFAULTS_FILTERS,
  STATE_FILTERS,
  baselineApplied,
  baselineSplit,
  baselineSummary,
  baselineText,
  crossNotCompared,
  defaultCounts,
  filterTest,
  failureOf,
  fieldItems,
  initialDefaultsFilter,
  previewText,
  rangeText,
  sameRequest,
  type DefaultsFilter,
  type StateFilter,
  valueText,
} from "./analysis";
import { pageLabel } from "./menu";

const stateMatches = (f: ExperimentalField, v: StateFilter) => filterTest(STATE_FILTERS, v)(f);
const defaultsMatches = (f: ExperimentalField, v: DefaultsFilter) => filterTest(BASELINE_FILTERS, v)(f);

describe("analysis helpers", () => {
  it("shows the interpreted value, or the raw value, and never a default for unknown values", () => {
    const p = analysisFixture({ set: 0 }).payload;
    const byLabel = (label: string) => p.fields.find((f) => f.label === label);
    expect(valueText(byLabel("Bootup NumLock State")!)).toBe("On");
    expect(valueText(byLabel("DMI Gen3 RTCO Cpre Lane0")!)).toBe("5");
    expect(valueText(byLabel("Fan select Main page")!)).toBe("Unknown value");
    expect(valueText(byLabel("Legacy option")!)).toBe("Not interpreted");
  });

  it("orders settings by BIOS menu page, with fields on no page last or first in a stable place", () => {
    const items = fieldItems(analysisFixture({ set: 0, extra: 3 }).payload);
    expect(items.length).toBe(9);
    const pages = items.map((it) => pageLabel(it.menu.path, "Not on a menu page"));
    // Every page's settings are contiguous, so headings are printed once per page.
    const firstIndex = (page: string) => pages.indexOf(page);
    for (const page of new Set(pages)) {
      const idx = pages.map((p, i) => (p === page ? i : -1)).filter((i) => i >= 0);
      expect(idx[idx.length - 1]! - idx[0]!).toBe(idx.length - 1);
      expect(firstIndex(page)).toBeGreaterThanOrEqual(0);
    }
  });

  it("filters states without merging unsupported encodings into unknown values", () => {
    const fields = analysisFixture({ set: 0 }).payload.fields;
    const unknown = fields.filter((f) => stateMatches(f, "unknown_value"));
    const notDecoded = fields.filter((f) => stateMatches(f, "not_decoded"));
    expect(unknown.map((f) => f.label)).toEqual(["Fan select Main page"]);
    expect(notDecoded.map((f) => f.state).sort()).toEqual(["out_of_bounds", "unsupported_encoding"]);
  });

  it("maps failures to plain titles and keeps the engine's message", () => {
    expect(failureOf({ code: "io", exit_code: 3, message: "no such file" })).toEqual({
      title: "Could not read a file",
      message: "no such file",
    });
    expect(failureOf({ code: "identity_mismatch", exit_code: 4, message: "x" }).title).toBe("Profile and BIOS do not match");
    expect(failureOf(new Error("boom")).title).toBe("The analysis did not finish");
  });

  it("formats ranges as half-open hex and previews as spaced bytes", () => {
    expect(rangeText({ start: 31, end: 32 })).toBe("[0x001F, 0x0020)");
    expect(previewText("0a0b0c", true)).toBe("0a 0b 0c …");
    expect(previewText("", false)).toBe("");
  });
});

describe("comparison with the BIOS optimized default", () => {
  const fields = analysisFixture({ set: 0, extra: 3 }).payload.fields;

  it("counts settings that differ and settings without a default apart from unchanged ones", () => {
    // Differs: DMI lane, PCH throttling. No default: vendor option, legacy option, 3 extras.
    const none = { baselineDiffers: 0, baselineCompared: 0, baselineAtDefault: 0, baselineOffDefault: 0, baselineNoDefault: 0 };
    expect(defaultCounts(fields)).toEqual({ differs: 2, noDefault: 5, ...none });
    expect(defaultCounts([])).toEqual({ differs: 0, noDefault: 0, ...none });
  });

  it("filters by the three choices; a setting with no default is never counted as equal", () => {
    const pick = (d: "all" | "differs" | "none") => fields.filter((f) => defaultsMatches(f, d)).length;
    expect(pick("all")).toBe(9);
    expect(pick("differs")).toBe(2);
    expect(pick("none")).toBe(5);
    // 2 differ + 5 without a default + 2 equal to the default
    expect(9 - pick("differs") - pick("none")).toBe(2);
  });

  it("starts on the differing settings only when some differ", () => {
    const c = (differs: number, baselineDiffers = 0) => ({
      differs,
      noDefault: 0,
      baselineDiffers,
      baselineCompared: 4,
      baselineAtDefault: 0,
      baselineOffDefault: 0,
      baselineNoDefault: 0,
    });
    expect(initialDefaultsFilter(c(1))).toBe("differs");
    expect(initialDefaultsFilter({ ...c(0), noDefault: 9, baselineCompared: 0 })).toBe("all");
    expect(initialDefaultsFilter(c(0))).toBe("all");
  });
});

describe("comparison with a baseline profile", () => {
  const applied = analysisFixture({ set: 0, extra: 3, baseline: "applied" }).payload;
  const blocked = analysisFixture({ set: 0, extra: 3, baseline: "blocked" }).payload;

  it("counts only fields with a comparison; an absent flag is not compared, never a difference", () => {
    const c = defaultCounts(applied.fields);
    expect(c.baselineDiffers).toBe(2);
    expect(c.baselineCompared).toBe(4);
    const vendor = applied.fields.find((f) => f.label === "Vendor option 9")!;
    expect(vendor.differs_from_baseline).toBeUndefined();
    expect(defaultsMatches(vendor, "baseline")).toBe(false);
  });

  it("starts on the baseline differences only when a baseline is applied and some differ", () => {
    const c = defaultCounts(applied.fields);
    expect(initialDefaultsFilter(c, true)).toBe("baseline_at");
    expect(initialDefaultsFilter(c, false)).toBe("differs");
    // Differences that are all off the baseline's default (or unclassified) start on the full list.
    expect(initialDefaultsFilter({ ...c, baselineAtDefault: 0 }, true)).toBe("baseline");
    expect(initialDefaultsFilter({ ...c, baselineDiffers: 0, baselineAtDefault: 0, baselineOffDefault: 0 }, true)).toBe("differs");
    expect(initialDefaultsFilter({ ...c, differs: 0, baselineDiffers: 0, baselineAtDefault: 0, baselineOffDefault: 0 }, true)).toBe("all");
  });

  it("splits the differences by whether the baseline is at its own BIOS default", () => {
    const c = defaultCounts(applied.fields);
    // NumLock: baseline off its default. PCH throttling: baseline at its default.
    expect([c.baselineAtDefault, c.baselineOffDefault, c.baselineNoDefault]).toEqual([1, 1, 0]);
    expect(baselineSplit(applied, c)).toEqual({ atDefault: 1, offDefault: 1, noDefault: 0 });
    const label = (d: "baseline_at" | "baseline_off" | "baseline") =>
      applied.fields.filter((f) => defaultsMatches(f, d)).map((f) => f.label);
    expect(label("baseline_at")).toEqual(["PCH Thermal Throttling"]);
    expect(label("baseline_off")).toEqual(["Bootup NumLock State"]);
    expect(label("baseline")).toHaveLength(2);
  });

  it("counts a difference whose baseline field has no default as unknown, in neither named view", () => {
    const f = { ...applied.fields.find((x) => x.label === "PCH Thermal Throttling")! };
    delete f.baseline_off_default;
    expect(defaultsMatches(f, "baseline")).toBe(true);
    expect(defaultsMatches(f, "baseline_at")).toBe(false);
    expect(defaultsMatches(f, "baseline_off")).toBe(false);
    expect(defaultCounts([f]).baselineNoDefault).toBe(1);
    // The engine's own counts win when present.
    const p = { ...applied, coverage: { ...applied.coverage, baseline_differing_at_default: 7 } };
    expect(baselineSplit(p, defaultCounts(p.fields)).atDefault).toBe(7);
  });

  it("reports the not-compared count only for an applied cross-version baseline", () => {
    expect(crossNotCompared(applied)).toBeNull();
    expect(crossNotCompared(analysisFixture({ set: 0, baseline: "applied", cross: true }).payload)).toBe(3);
    expect(crossNotCompared(analysisFixture({ set: 0, baseline: "blocked", cross: true }).payload)).toBeNull();
    expect(crossNotCompared(analysisFixture({ set: 0 }).payload)).toBeNull();
  });

  it("offers the baseline filter only with an applied baseline", () => {
    expect(DEFAULTS_FILTERS.map((o) => o.label)).toEqual(["All", "Differs from default", "No default"]);
    expect(BASELINE_FILTERS.map((o) => o.label)).toEqual([
      "All",
      "Differs from default",
      "No default",
      "Likely your settings",
      "Filled in by firmware?",
      "Differs from baseline",
    ]);
    expect(BASELINE_FILTERS.find((o) => o.value === "baseline_at")?.title).toMatch(/only holds if the baseline was saved with the BIOS at its defaults/);
    expect(baselineApplied(applied)).toBe(true);
    expect(baselineApplied(blocked)).toBe(false);
    expect(baselineApplied(analysisFixture({ set: 0 }).payload)).toBe(false);
  });

  it("summarises from the engine's counts, with the baseline's label before its raw value", () => {
    expect(baselineSummary(applied, defaultCounts(applied.fields))).toEqual({ differing: 2, compared: 4 });
    const f = applied.fields.find((x) => x.label === "DMI Gen3 RTCO Cpre Lane0")!;
    expect(baselineText(f)).toBe("5");
    expect(baselineText(applied.fields.find((x) => x.label === "PCH Thermal Throttling")!)).toBe("Disabled");
  });

  it("treats the baseline as part of the request", () => {
    const a = { profile: "a.ocb", bios: "b.bin", set: null };
    expect(sameRequest(a, { ...a })).toBe(true);
    expect(sameRequest(a, { ...a, baseline: null })).toBe(true);
    expect(sameRequest(a, { ...a, baseline: "base.ocb" })).toBe(false);
    expect(sameRequest({ ...a, baseline: "x.ocb" }, { ...a, baseline: "y.ocb" })).toBe(false);
    const b = { ...a, baseline: "x.ocb" };
    expect(sameRequest(b, { ...b, baselineBios: null })).toBe(true);
    expect(sameRequest(b, { ...b, baselineBios: "old.bin" })).toBe(false);
    expect(sameRequest({ ...b, baselineBios: "old.bin" }, { ...b, baselineBios: "new.bin" })).toBe(false);
  });
});
