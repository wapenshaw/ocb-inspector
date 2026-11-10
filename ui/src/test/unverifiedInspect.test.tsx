import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { BiosSummary, ExperimentalInspectPayload, Report } from "../api/types";
import { analysisFixture } from "./analysisFixture";

const pickFile = vi.fn();
const biosSummary = vi.fn();
const inspect = vi.fn();
const analyzeProfile = vi.fn();
const editValidate = vi.fn();
const editBuild = vi.fn();
vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  pickFile,
  api: { biosSummary, inspect, analyzeProfile, editValidate, editBuild },
}));
const { appStore } = await import("../state/app");
const { Inspect } = await import("../components/Inspect");
const { App } = await import("../App");
const { unverifiedNoteText } = await import("../components/UnverifiedNote");

const unsupported = {
  bios_id: "E7D91IMS.HI0",
  profiles_supported: false,
  approved_for_writing: false,
  build_date: "2026-10-01",
  sha256: "ab".repeat(32),
  platform: { profiles_note: "not examined" },
} as unknown as BiosSummary;

type Result = Report<ExperimentalInspectPayload>;

/** Opens the pair through the picker, the way a user would. */
async function open(report: Result, onAnalyze = vi.fn()) {
  analyzeProfile.mockResolvedValue(report);
  render(<Inspect browsing={null} onBrowsingChange={() => {}} active onAnalyze={onAnalyze} />);
  pickFile.mockResolvedValueOnce("c.1C0");
  await userEvent.click(screen.getByRole("button", { name: /BIOS image/ }));
  pickFile.mockResolvedValueOnce("a.ocb");
  await userEvent.click(await screen.findByRole("button", { name: /OC profile/ }));
  await screen.findByRole("group", { name: "Open pair" });
  return onAnalyze;
}

const fixture = (): Result => {
  const r = analysisFixture({ set: 0, extra: 3 });
  // The same store and offset on a second menu page, as AMD boards repeat a setting.
  const first = r.payload.fields[0]!;
  r.payload.fields.push({ ...first, key: `${first.key}#again`, menu_path: ["Overclocking"], menu_order: [9, 9] });
  return r;
};
const bodyRows = () => within(screen.getByRole("table", { name: "Settings" })).getAllByRole("row").slice(1);
const rowOf = (name: string) => bodyRows().find((r) => within(r).queryByText(name))!;
const showAll = () => userEvent.click(screen.getByRole("radio", { name: /^All$/ }));
// Searching lists matches from every page, which is how both copies of a repeated setting are seen.
const search = (q: string) => userEvent.type(screen.getByRole("searchbox"), q);

beforeEach(() => {
  pickFile.mockReset();
  biosSummary.mockReset().mockResolvedValue(unsupported);
  inspect.mockReset();
  analyzeProfile.mockReset();
  editValidate.mockReset();
  editBuild.mockReset();
  appStore.set({ writing: false });
});

describe("an unverified pair in the normal Inspect layout", () => {
  it("adds a signed reviewed reading to a sign-only difference without replacing raw comparisons or other stores", async () => {
    const r = fixture();
    const field = r.payload.fields.find((f) => f.label === "PCH Thermal Throttling")!;
    field.label = "Offset sign";
    field.raw = { dec: "1", hex: "01" };
    field.value_label = "Minus";
    field.default_raw = { dec: "0", hex: "00" };
    field.default_label = "Plus";
    field.reviewed = {
      setting_id: "cpu.offset", label: "CPU voltage offset", board_evidence: "examined",
      value: { kind: "integer", value: -100, unit: "mV" },
      default_value: { kind: "integer", value: 100, unit: "mV" }, baseline_value: null,
      differs_from_default: true, evidence: ["Reviewed signed magnitude codec"],
      components: [{ store: field.store, store_offset: field.store_offset, width_bits: 8, span: field.span! }],
    };
    await open(r);
    const row = rowOf("Offset sign");
    expect(row).toHaveTextContent("Minus");
    expect(row).toHaveTextContent("Reviewed reading: -100 mV");
    expect(row).toHaveTextContent("Reviewed reading: 100 mV");
    await userEvent.click(row);
    const details = screen.getByRole("complementary", { name: "Setting details" });
    expect(details).toHaveTextContent("1 (01)");
    expect(details).toHaveTextContent("Reviewed signed magnitude codec");
    expect(inspect).not.toHaveBeenCalled();
    await showAll();
    await search("NumLock");
    expect(rowOf("Bootup NumLock State")).not.toHaveTextContent("Reviewed reading");
  });

  it("uses BIOS-derived mapping for an originally supported board and every reported store", async () => {
    biosSummary.mockResolvedValue({ ...unsupported, profiles_supported: true, approved_for_writing: false });
    await open(fixture());
    expect(analyzeProfile).toHaveBeenCalledWith({ profile: "a.ocb", bios: "c.1C0", set: null });
    expect(inspect).not.toHaveBeenCalled();
    expect(screen.queryByText("Not an approved image: inspect only")).toBeNull();
    await showAll();
    await search("Throttling");
    await userEvent.click(rowOf("PCH Thermal Throttling"));
    expect(screen.getByRole("complementary", { name: "Setting details" })).toHaveTextContent("PchSetup");
  });

  it("keeps firmware set selection in Inspect and carries it into baseline and sharing", async () => {
    analyzeProfile.mockResolvedValueOnce(analysisFixture()).mockResolvedValueOnce(analysisFixture({ set: 1 }));
    const onAnalyze = vi.fn();
    render(<Inspect browsing={null} onBrowsingChange={() => {}} active onAnalyze={onAnalyze} />);
    pickFile.mockResolvedValueOnce("c.1C0");
    await userEvent.click(screen.getByRole("button", { name: /BIOS image/ }));
    pickFile.mockResolvedValueOnce("a.ocb");
    await userEvent.click(screen.getByRole("button", { name: /OC profile/ }));
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Firmware set" }), "1");
    await screen.findByRole("group", { name: "Open pair" });
    expect(analyzeProfile).toHaveBeenLastCalledWith({ profile: "a.ocb", bios: "c.1C0", set: 1 });
    expect(onAnalyze).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "All settings, baseline and sharing" }));
    expect(onAnalyze).toHaveBeenCalledWith("c.1C0", "a.ocb", true, 1);
  });

  it("keeps all settings visible and disables default comparison when no defaults exist", async () => {
    const r = fixture();
    r.payload.fields.forEach((field) => { field.default_raw = null; field.default_label = null; field.differs_from_default = null; });
    await open(r);
    expect(screen.getByRole("radio", { name: /^All$/ })).toBeChecked();
    expect(screen.getByRole("radio", { name: /Not at default/ })).toBeDisabled();
    expect(screen.getByText(/No default comparison:/)).toBeInTheDocument();
  });

  it("keeps settings closed after a failed integrity check", async () => {
    const r = fixture();
    r.payload.integrity.status = "failed";
    analyzeProfile.mockResolvedValue(r);
    render(<Inspect browsing={null} onBrowsingChange={() => {}} active onAnalyze={vi.fn()} />);
    pickFile.mockResolvedValueOnce("c.1C0");
    await userEvent.click(screen.getByRole("button", { name: /BIOS image/ }));
    pickFile.mockResolvedValueOnce("a.ocb");
    await userEvent.click(screen.getByRole("button", { name: /OC profile/ }));
    expect(screen.getByText("Profile integrity check failed")).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: "Settings" })).toBeNull();
    expect(screen.getByRole("button", { name: "Show what can be read from the file" })).toBeInTheDocument();
  });

  it("uses the menu rail and the settings table, with a confirmed match and no verified wording", async () => {
    await open(fixture());
    expect(inspect).not.toHaveBeenCalled();
    expect(screen.getByRole("navigation", { name: "BIOS menu" })).toBeInTheDocument();
    const strip = screen.getByRole("group", { name: "Open pair" });
    expect(within(strip).getByText("BIOS match confirmed")).toBeInTheDocument();
    // The integrity wording comes from the engine's status, not from a claim of the page.
    expect(within(strip).getByText("Not established")).toBeInTheDocument();
    expect(document.body.textContent ?? "").not.toMatch(/verified|checksum valid/i);
    expect(screen.queryByText("All BIOS fields")).toBeNull();
  });

  it("opens on the rows that differ from the BIOS default, with a way to see all of them", async () => {
    await open(fixture());
    expect(screen.getByRole("radio", { name: /Not at default/ })).toBeChecked();
    const names = bodyRows().filter((r) => !within(r).queryByRole("columnheader"));
    expect(names.map((r) => r.textContent).join("|")).toMatch(/PCH Thermal Throttling/);
    expect(screen.queryByText("Bootup NumLock State")).toBeNull();
    await showAll();
    await search("NumLock");
    expect(screen.getAllByText("Bootup NumLock State")).toHaveLength(2);
  });

  it("selects a visible differing setting when initially opening the default comparison", async () => {
    await open(fixture());
    expect(screen.getByRole("radio", { name: /Not at default/ })).toBeChecked();
    const selected = bodyRows().find((row) => row.getAttribute("aria-selected") === "true");
    expect(selected).toBeDefined();
    expect(selected).toHaveTextContent("Not at default");
    const details = screen.getByRole("complementary", { name: "Setting details" });
    const title = within(details).getByRole("heading", { level: 2 });
    expect(selected).toHaveTextContent(title.textContent!);
    expect(details).not.toHaveTextContent("Not checked on this board");
  });

  it("keeps a setting that sits on two menu pages as two rows", async () => {
    await open(fixture());
    await showAll();
    await search("NumLock");
    expect(screen.getAllByText("Bootup NumLock State")).toHaveLength(2);
    const ids = bodyRows().map((r) => r.id).filter(Boolean);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("does not show a row without a BIOS default as stock", async () => {
    await open(fixture());
    await showAll();
    await search("setting");
    const none = rowOf("Extra setting 1");
    expect(none.className).not.toMatch(/heat-/);
    expect(within(none).getByText("No default")).toBeInTheDocument();
    // A row that does have a default keeps its heat.
    await userEvent.click(none);
    expect(screen.getByText("No BIOS default to compare with")).toBeInTheDocument();
    expect(screen.queryByText("Matches the optimized default")).toBeNull();
    // A row that does have a default keeps its heat.
    await userEvent.clear(screen.getByRole("searchbox"));
    await search("Throttling");
    expect(rowOf("PCH Thermal Throttling").className).toMatch(/heat-3/);
  });

  it("shows the mapping state of the selected row, and where its value sits", async () => {
    await open(fixture());
    await userEvent.click(rowOf("PCH Thermal Throttling"));
    const details = screen.getByRole("complementary", { name: "Setting details" });
    expect(within(details).getByText("Inferred mapping")).toBeInTheDocument();
    expect(within(details).getByText(/PchSetup\+0x0001/)).toBeInTheDocument();
    expect(within(details).getByText("unique_size_pair/1: PchSetup")).toBeInTheDocument();
    expect(within(details).queryByText("reviewed")).toBeNull();
  });

  it("states the counts in one note under the strip", async () => {
    const r = fixture();
    await open(r);
    const note = screen.getByRole("note");
    const records = new Set(r.payload.fields.map((f) => f.record)).size;
    const none = r.payload.fields.filter((f) => f.differs_from_default === null).length;
    expect(note).toHaveTextContent(
      `Values use the BIOS labels, with additional recipe readings where available; mapping evidence and profile integrity are reported separately. ${records} of ${r.payload.coverage.records_total} profile records were tied to settings. ${none} settings have no BIOS default to compare with`,
    );
    expect(note).toHaveTextContent("All settings, baseline and sharing");
  });

  it("offers no editing, staging, export or validation, even with writing switched on", async () => {
    appStore.set({ writing: true });
    await open(fixture());
    expect(screen.queryByRole("columnheader", { name: "Proposed" })).toBeNull();
    expect(screen.queryByRole("radio", { name: /Edited/ })).toBeNull();
    expect(screen.queryByText(/No changes staged/)).toBeNull();
    expect(screen.queryByRole("button", { name: /Review changes|Undo|Redo|Revert/ })).toBeNull();
    await showAll();
    await userEvent.click(rowOf("Bootup NumLock State"));
    const details = screen.getByRole("complementary", { name: "Setting details" });
    expect(within(details).queryByRole("button")).toBeNull();
    expect(within(details).queryByRole("combobox")).toBeNull();
    expect(within(details).queryByRole("spinbutton")).toBeNull();
    expect(screen.queryByText("Inspect only: image not approved")).toBeNull();
    await userEvent.keyboard("{Control>}z{/Control}");
    expect(editValidate).not.toHaveBeenCalled();
    expect(editBuild).not.toHaveBeenCalled();
  });

  it("keeps Browse whole BIOS, All settings, baseline and sharing, and Change files", async () => {
    const onAnalyze = await open(fixture());
    expect(screen.getByRole("button", { name: "Browse whole BIOS" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "All settings, baseline and sharing" }));
    expect(onAnalyze).toHaveBeenCalledWith("c.1C0", "a.ocb", true, 0);
    await userEvent.click(screen.getByRole("button", { name: "Change files" }));
    expect(screen.getByRole("heading", { name: "See what an OC profile really stores" })).toBeInTheDocument();
  });

  it("starts on all rows when none differs from the default", async () => {
    await open(analysisFixture({ set: 0, noDiffs: true }));
    expect(screen.getByRole("radio", { name: /^All$/ })).toBeChecked();
  });
});

describe("the note text", () => {
  it("leaves out the no-default sentence and the baseline hint when every row has a default", () => {
    expect(unverifiedNoteText({ recordsWithSettings: 18, recordsTotal: 116, noDefault: 0 })).toBe(
      "Values use the BIOS labels, with additional recipe readings where available; mapping evidence and profile integrity are reported separately. 18 of 116 profile records were tied to settings.",
    );
    expect(unverifiedNoteText({ recordsWithSettings: 1, recordsTotal: 2, noDefault: 1 })).toMatch(/1 setting has no BIOS default/);
  });
});

describe("in the whole app", () => {
  it("opens the pair in Inspect, and the analysis view only on request", async () => {
    analyzeProfile.mockResolvedValue(fixture());
    render(<App />);
    pickFile.mockResolvedValueOnce("c.1C0");
    await userEvent.click(screen.getByRole("button", { name: /BIOS image/ }));
    pickFile.mockResolvedValueOnce("a.ocb");
    await userEvent.click(await screen.findByRole("button", { name: /OC profile/ }));
    const strip = await screen.findByRole("group", { name: "Open pair" });
    expect(strip).toBeVisible();
    expect(screen.queryByRole("group", { name: "Files read" })).toBeNull();
    expect(analyzeProfile).toHaveBeenCalledTimes(1);

    await userEvent.click(screen.getByRole("button", { name: "All settings, baseline and sharing" }));
    expect(await screen.findByRole("group", { name: "Files read" })).toBeVisible();
    expect(analyzeProfile).toHaveBeenCalledTimes(2);
  });
});
