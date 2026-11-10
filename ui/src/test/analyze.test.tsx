import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ExperimentalInspectPayload, Report } from "../api/types";
import { analysisFixture } from "./analysisFixture";

const pickFile = vi.fn();
const pickSave = vi.fn();
const analyzeProfile = vi.fn();
const saveAnalysis = vi.fn();
const openPage = vi.fn();
vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  pickFile,
  pickSave,
  api: { analyzeProfile, saveAnalysis, openPage },
}));
const { Analyze } = await import("../components/Analyze");

type Result = Report<ExperimentalInspectPayload>;

/** A synthetic result whose profile identifier is `id`, so tests can tell answers apart on screen. */
function result(id: string, opts: Parameters<typeof analysisFixture>[0] = {}): Result {
  const r = analysisFixture(opts);
  r.payload.identity.profile_bios_id = id;
  return r;
}

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Pick a file through the button with this name; the dialog answers with `file`. */
async function choose(name: RegExp, file: string) {
  pickFile.mockResolvedValueOnce(file);
  await userEvent.click(screen.getByRole("button", { name }));
  await act(async () => {});
}

const saveButton = () => screen.getByRole("button", { name: "Save analysis" });
const PROFILE = /Choose OC profile|Change profile/;
const BIOS = /Add BIOS image|Change BIOS/;

beforeEach(() => {
  pickFile.mockReset();
  pickSave.mockReset();
  analyzeProfile.mockReset();
  saveAnalysis.mockReset();
  openPage.mockReset();
});

describe("Analyze: stale answers", () => {
  it("drops an answer for the old profile, success or error, and never saves it", async () => {
    const old = deferred<Result>();
    const current = deferred<Result>();
    analyzeProfile.mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);

    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await choose(PROFILE, "b.ocb");
    expect(analyzeProfile).toHaveBeenLastCalledWith({ profile: "b.ocb", bios: null, set: null });
    expect(saveButton()).toBeDisabled();

    // The old profile answers first: nothing it says reaches the screen, and nothing can be saved.
    await act(async () => old.resolve(result("OLD-ID")));
    expect(screen.queryByText("OLD-ID")).not.toBeInTheDocument();
    expect(saveButton()).toBeDisabled();
    expect(screen.getByText("Analyzing…")).toBeInTheDocument();

    await act(async () => current.resolve(result("NEW-ID")));
    expect(await screen.findByText("NEW-ID")).toBeInTheDocument();
    expect(saveButton()).toBeEnabled();
  });

  it("a stale error does not replace the result for a newer BIOS", async () => {
    const first = deferred<Result>();
    const second = deferred<Result>();
    analyzeProfile.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await choose(BIOS, "E7D91IMS.HI0");
    expect(analyzeProfile).toHaveBeenLastCalledWith({ profile: "a.ocb", bios: "E7D91IMS.HI0", set: null });

    await act(async () => first.reject({ code: "io", exit_code: 3, message: "stale failure" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText("stale failure")).not.toBeInTheDocument();

    await act(async () => second.resolve(result("BIOS-ID", { bios: true })));
    expect(await screen.findByText("BIOS-ID")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("disables Save while the replacement is pending, and saves only the shown result", async () => {
    analyzeProfile.mockResolvedValueOnce(result("FIRST-ID")).mockReturnValueOnce(new Promise(() => {}));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    expect(await screen.findByText("FIRST-ID")).toBeInTheDocument();
    expect(saveButton()).toBeEnabled();

    await choose(PROFILE, "c.ocb");
    expect(saveButton()).toBeDisabled();
    expect(screen.queryByText("FIRST-ID")).not.toBeInTheDocument();
  });

  it("saves the shown result through the engine with its own inputs", async () => {
    const r = result("SAVE-ID");
    analyzeProfile.mockResolvedValueOnce(r);
    pickSave.mockResolvedValueOnce("C:/out/a.analysis.json");
    saveAnalysis.mockResolvedValueOnce("C:/out/a.analysis.json");

    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await screen.findByText("SAVE-ID");
    await userEvent.click(saveButton());

    expect(pickSave).toHaveBeenCalledWith("a.analysis.json", "json", "Save analysis");
    await waitFor(() =>
      expect(saveAnalysis).toHaveBeenCalledWith({ profile: "a.ocb", bios: null, set: null }, r.payload.inputs, "C:/out/a.analysis.json"),
    );
    expect(await screen.findByText("C:/out/a.analysis.json")).toBeInTheDocument();
  });
});

describe("Analyze: retry", () => {
  it("retry re-sends the same input snapshot", async () => {
    analyzeProfile
      .mockResolvedValueOnce(result("PROFILE-ONLY"))
      .mockRejectedValueOnce({ code: "io", exit_code: 3, message: "cannot read the BIOS image" })
      .mockResolvedValueOnce(result("RETRIED-ID", { bios: true }));

    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await screen.findByText("PROFILE-ONLY");
    await choose(BIOS, "bios.HI0");

    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("Could not read a file")).toBeInTheDocument();
    expect(within(alert).getByText("cannot read the BIOS image")).toBeInTheDocument();

    await userEvent.click(within(alert).getByRole("button", { name: "Try again" }));
    await screen.findByText("RETRIED-ID");
    expect(analyzeProfile).toHaveBeenCalledTimes(3);
    expect(analyzeProfile.mock.calls[2]).toEqual(analyzeProfile.mock.calls[1]);
    expect(analyzeProfile.mock.calls[2]).toEqual([{ profile: "a.ocb", bios: "bios.HI0", set: null }]);
  });
});

describe("Analyze: rendering", () => {
  it("shows inferred, ambiguous and unknown states with the banner and no verified badge or edit control", async () => {
    analyzeProfile.mockResolvedValueOnce(result("E7D91IMS.HI0", { set: 0 }));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await screen.findByRole("radio", { name: "Settings" });

    expect(
      screen.getByText("Experimental, read-only interpretation. Some record mappings are inferred. Nothing here can change a profile or a BIOS image."),
    ).toBeInTheDocument();
    expect(screen.getByText("Integrity not established")).toBeInTheDocument();
    expect(screen.queryByText(/Match verified/)).not.toBeInTheDocument();
    expect(screen.queryByText(/checksum valid/i)).not.toBeInTheDocument();
    expect(document.body.textContent ?? "").not.toMatch(/verified/i);
    expect(screen.queryByRole("button", { name: /^(Apply|Build|Export|Port|Edit)/ })).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    expect(screen.getAllByText("Inferred mapping").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Unknown value")[0]).toBeInTheDocument();
    expect(screen.getByText("BIOS optimized default", { selector: "th" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("radio", { name: "Records" }));
    expect(screen.getByText("Ambiguous mapping")).toBeInTheDocument();
    // Ambiguous records list their candidate stores and the reason, and nothing more.
    expect(screen.getByText("SecureBootSetup")).toBeInTheDocument();
    expect(screen.getByText("SiSetup")).toBeInTheDocument();
    expect(screen.getAllByText(/3 stores are 7 bytes/).length).toBeGreaterThan(0);
  });

  it("asks for a firmware set when the BIOS has several and none is chosen", async () => {
    analyzeProfile
      .mockResolvedValueOnce(result("E7D91IMS.HI0", { bios: true, set: null }))
      .mockResolvedValueOnce(result("E7D91IMS.HI0", { bios: true, set: null }));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await choose(BIOS, "bios.HI0");
    await screen.findByText("No firmware set chosen");
    expect(screen.getByRole("combobox", { name: /Firmware set/ })).toBeInTheDocument();
    expect(screen.getByText("None chosen")).toBeInTheDocument();
  });

  it("offers Help improve support through the repository link only", async () => {
    analyzeProfile.mockResolvedValueOnce(result("E7D91IMS.HI0"));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await screen.findByRole("radio", { name: "Settings" });
    expect(screen.getByText("Sharing is your choice: nothing is sent automatically.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Help improve support" }));
    expect(openPage).toHaveBeenCalledWith("https://github.com/wapenshaw/ocb-inspector/issues/new?template=profile_report.yml");
  });
});

describe("Analyze: settings table", () => {
  it("renders at most ROW_LIMIT settings and says how many are left out", async () => {
    analyzeProfile.mockResolvedValueOnce(analysisFixture({ set: 0, extra: 1200 }));
    const { container } = render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await userEvent.click(await screen.findByRole("radio", { name: "Settings" }));
    // The list starts on the two settings that differ from their default; the rest are reachable under All.
    await userEvent.click(within(screen.getByRole("radiogroup", { name: "Compared with the BIOS optimized default" })).getByRole("radio", { name: "All" }));

    expect(await screen.findByText(/Showing 500 of 1206/)).toBeInTheDocument();
    expect(container.querySelectorAll("tr[data-row]")).toHaveLength(500);
  });

  it("shows the evidence chain for a selected setting in the side panel", async () => {
    analyzeProfile.mockResolvedValueOnce(analysisFixture({ set: 0 }));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await userEvent.click(await screen.findByRole("radio", { name: "Settings" }));
    await userEvent.click(screen.getByText("PCH Thermal Throttling"));

    const panel = screen.getByRole("complementary", { name: "How this value is found" });
    expect(within(panel).getByText("PchSetup")).toBeInTheDocument();
    expect(within(panel).getAllByText(/unique_size_pair/).length).toBeGreaterThan(0);
    expect(within(panel).getByText("ev:map:r2")).toBeInTheDocument();
  });
});
describe("Analyze: firmware variable names", () => {
  it("names a record from the BIOS's variable list and says what each piece of evidence rests on", async () => {
    analyzeProfile.mockResolvedValueOnce(analysisFixture({ set: 0 }));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await userEvent.click(await screen.findByRole("radio", { name: "Records" }));
    expect(screen.getByText("NetworkStackVar")).toBeInTheDocument();
    expect(screen.getByText(/no BIOS menu reads this variable/)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    await userEvent.click(screen.getByText("PCH Thermal Throttling"));
    const panel = screen.getByRole("complementary", { name: "How this value is found" });
    expect(within(panel).getByText("Based on: matching sizes only")).toBeInTheDocument();
  });
});

describe("Analyze: comparison with the BIOS optimized default", () => {
  const group = () => screen.getByRole("radiogroup", { name: "Compared with the BIOS optimized default" });
  const picked = () => within(group()).getByRole("radio", { checked: true });
  const open = async (r: Result) => {
    analyzeProfile.mockResolvedValueOnce(r);
    const view = render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await screen.findByRole("radio", { name: "Settings" });
    return view;
  };

  it("starts the Settings view on the differing settings when some differ", async () => {
    const { container } = await open(analysisFixture({ set: 0, extra: 3 }));
    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    expect(picked()).toHaveAccessibleName("Differs from default");
    expect(container.querySelectorAll("tr[data-row]")).toHaveLength(2);
    expect(screen.getByTestId("default-counts")).toHaveTextContent("2 differ · 5 have no default");
  });

  it("starts on all settings when none differs", async () => {
    const { container } = await open(analysisFixture({ set: 0, noDiffs: true }));
    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    expect(picked()).toHaveAccessibleName("All");
    expect(container.querySelectorAll("tr[data-row]")).toHaveLength(6);
    expect(screen.getByTestId("default-counts")).toHaveTextContent("0 differ · 2 have no default");
  });

  it("lists the settings with no default, and keeps a manual choice within one result", async () => {
    const { container } = await open(analysisFixture({ set: 0 }));
    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    await userEvent.click(within(group()).getByRole("radio", { name: "No default" }));
    expect(container.querySelectorAll("tr[data-row]")).toHaveLength(2);
    expect(screen.getByText("Vendor option 9")).toBeInTheDocument();
    expect(screen.getByText("Legacy option")).toBeInTheDocument();
    // Leaving and returning to the view keeps the choice.
    await userEvent.click(screen.getByRole("radio", { name: "Summary" }));
    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    expect(picked()).toHaveAccessibleName("No default");
  });

  it("re-applies the rule for a new result", async () => {
    analyzeProfile.mockResolvedValueOnce(analysisFixture({ set: 0 })).mockResolvedValueOnce(analysisFixture({ set: 0, noDiffs: true }));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await userEvent.click(await screen.findByRole("radio", { name: "Settings" }));
    expect(picked()).toHaveAccessibleName("Differs from default");
    await choose(PROFILE, "b.ocb");
    await screen.findByRole("radiogroup", { name: "Compared with the BIOS optimized default" });
    expect(picked()).toHaveAccessibleName("All");
  });

  it("the Summary line states both counts and opens Settings with the differs filter on", async () => {
    await open(analysisFixture({ set: 0 }));
    expect(
      screen.getByRole("button", {
        name: "2 settings differ from the BIOS optimized default; 2 have no optimized default and cannot be judged.",
      }),
    ).toBeInTheDocument();
    // Move off the filter first, so the link visibly sets it.
    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    await userEvent.click(within(group()).getByRole("radio", { name: "All" }));
    await userEvent.click(screen.getByRole("radio", { name: "Summary" }));
    await userEvent.click(screen.getByRole("button", { name: /settings differ from the BIOS optimized default/ }));
    expect(screen.getByRole("radio", { name: "Settings" })).toBeChecked();
    expect(picked()).toHaveAccessibleName("Differs from default");
    expect(document.body.textContent ?? "").not.toMatch(/you changed|changed from/i);
  });
});

describe("Analyze: baseline profile", () => {
  const ADD_BASELINE = /Add baseline profile|Change baseline/;
  const filterGroup = () => screen.getByRole("radiogroup", { name: "Compared with the BIOS optimized default" });

  /** Three requests: the profile alone, then with the BIOS (`first`), then with the baseline (`second`). */
  async function withBaseline(first: Result, second: Result) {
    analyzeProfile.mockResolvedValueOnce(result("PROFILE-ONLY", { bios: false })).mockResolvedValueOnce(first).mockResolvedValueOnce(second);
    const view = render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await choose(BIOS, "bios.HI0");
    await screen.findByRole("radio", { name: "Settings" });
    await choose(ADD_BASELINE, "base.ocb");
    await screen.findByText(second.payload.identity.profile_bios_id ?? "");
    return view;
  }

  it("is offered only once a BIOS is chosen", async () => {
    analyzeProfile.mockResolvedValueOnce(result("PROFILE-ONLY", { bios: false }));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await screen.findByText("PROFILE-ONLY");
    expect(screen.queryByRole("button", { name: ADD_BASELINE })).not.toBeInTheDocument();
  });

  it("sends the baseline in a new request, and a stale earlier result cannot replace the new one", async () => {
    const earlier = deferred<Result>();
    const newer = deferred<Result>();
    analyzeProfile
      .mockResolvedValueOnce(result("PROFILE-ONLY", { bios: false }))
      .mockResolvedValueOnce(result("PLAIN-ID"))
      .mockReturnValueOnce(earlier.promise)
      .mockReturnValueOnce(newer.promise);
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await choose(BIOS, "bios.HI0");
    await screen.findByText("PLAIN-ID");

    await choose(ADD_BASELINE, "base1.ocb");
    expect(analyzeProfile).toHaveBeenLastCalledWith({ profile: "a.ocb", bios: "bios.HI0", set: null, baseline: "base1.ocb" });
    expect(screen.queryByText("PLAIN-ID")).not.toBeInTheDocument();
    expect(saveButton()).toBeDisabled();

    await choose(ADD_BASELINE, "base2.ocb");
    expect(analyzeProfile).toHaveBeenLastCalledWith({ profile: "a.ocb", bios: "bios.HI0", set: null, baseline: "base2.ocb" });

    await act(async () => earlier.resolve(result("EARLIER-ID", { set: 0, baseline: "applied" })));
    expect(screen.queryByText("EARLIER-ID")).not.toBeInTheDocument();
    expect(saveButton()).toBeDisabled();

    await act(async () => newer.resolve(result("NEWER-ID", { set: 0, baseline: "applied" })));
    expect(await screen.findByText("NEWER-ID")).toBeInTheDocument();
    expect(saveButton()).toBeEnabled();
  });

  it("clearing the baseline starts a request without it", async () => {
    await withBaseline(result("PLAIN-ID", { set: 0 }), result("WITH-ID", { set: 0, baseline: "applied" }));
    analyzeProfile.mockResolvedValueOnce(result("CLEARED-ID", { set: 0 }));
    await userEvent.click(screen.getAllByRole("button", { name: "Remove baseline" })[0]!);
    expect(analyzeProfile).toHaveBeenLastCalledWith({ profile: "a.ocb", bios: "bios.HI0", set: null });
    expect(await screen.findByText("CLEARED-ID")).toBeInTheDocument();
  });

  it("a blocked baseline shows its reason calmly and no baseline column or filter", async () => {
    await withBaseline(result("PLAIN-ID", { set: 0 }), result("BLOCKED-ID", { set: 0, baseline: "blocked" }));
    const note = screen.getByText("Baseline profile not used").closest("[role=note]") as HTMLElement;
    expect(note).toHaveTextContent("The baseline profile was saved by a different BIOS release.");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText(/of \d+ compared settings differ from the baseline/)).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    expect(screen.queryByRole("columnheader", { name: "Baseline" })).not.toBeInTheDocument();
    expect(within(filterGroup()).queryByRole("radio", { name: "Differs from baseline" })).not.toBeInTheDocument();
    expect(within(filterGroup()).getByRole("radio", { checked: true })).toHaveAccessibleName("Differs from default");
  });

  const rowLabels = (container: HTMLElement) =>
    Array.from(container.querySelectorAll("tr[data-row]")).map((r) => r.querySelector("td")?.textContent ?? "");

  it("an applied baseline adds its column and starts on the differences where the baseline is at its default", async () => {
    const { container } = await withBaseline(result("PLAIN-ID", { set: 0 }), result("APPLIED-ID", { set: 0, baseline: "applied" }));
    expect(screen.getByRole("button", { name: "2 of 4 compared settings differ from the baseline" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    expect(screen.getByRole("columnheader", { name: "Baseline" })).toBeInTheDocument();
    expect(within(filterGroup()).getByRole("radio", { checked: true })).toHaveAccessibleName("Likely your settings");
    expect(rowLabels(container)).toHaveLength(1);
    expect(rowLabels(container)[0]).toContain("PCH Thermal Throttling");
    expect(screen.getByTestId("baseline-counts")).toHaveTextContent("1 with the baseline at its default, 1 with it off its default, 0 unknown");
    // The tooltip states the reasoning and its caveat.
    const tip = within(filterGroup()).getByRole("radio", { name: "Likely your settings" }).closest("label");
    expect(tip?.getAttribute("title")).toMatch(/most likely a setting made in the BIOS menus.*only holds if the baseline was saved with the BIOS at its defaults/);

    // The other two views list the other rows.
    await userEvent.click(within(filterGroup()).getByRole("radio", { name: "Filled in by firmware?" }));
    expect(rowLabels(container)).toHaveLength(1);
    expect(rowLabels(container)[0]).toContain("Bootup NumLock State");
    expect(screen.getByText("Off")).toBeInTheDocument(); // the baseline's label for NumLock
    await userEvent.click(within(filterGroup()).getByRole("radio", { name: "Differs from baseline" }));
    expect(rowLabels(container)).toHaveLength(2);
    expect(rowLabels(container).join("|")).toContain("Bootup NumLock State");
    expect(rowLabels(container).join("|")).toContain("PCH Thermal Throttling");
    expect(screen.queryByTestId("cross-note")).not.toBeInTheDocument();

    // Fields the baseline cannot be compared on are "not compared", never "differs".
    await userEvent.click(within(filterGroup()).getByRole("radio", { name: "All" }));
    expect(screen.getAllByText("Not compared")).toHaveLength(2);
    expect(document.body.textContent ?? "").not.toMatch(/you changed|changed from|was changed/i);
  });

  it("the Summary line opens Settings on the baseline filter", async () => {
    await withBaseline(result("PLAIN-ID", { set: 0 }), result("APPLIED-ID", { set: 0, baseline: "applied" }));
    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    await userEvent.click(within(filterGroup()).getByRole("radio", { name: "All" }));
    await userEvent.click(screen.getByRole("radio", { name: "Summary" }));
    await userEvent.click(screen.getByRole("button", { name: /compared settings differ from the baseline/ }));
    expect(within(filterGroup()).getByRole("radio", { checked: true })).toHaveAccessibleName("Differs from baseline");
  });

  it("the Summary lists the split and opens Settings on each view", async () => {
    await withBaseline(result("PLAIN-ID", { set: 0 }), result("APPLIED-ID", { set: 0, baseline: "applied" }));
    await userEvent.click(screen.getByRole("button", { name: /1 perhaps filled in by the firmware/ }));
    expect(within(filterGroup()).getByRole("radio", { checked: true })).toHaveAccessibleName("Filled in by firmware?");
    await userEvent.click(screen.getByRole("radio", { name: "Summary" }));
    await userEvent.click(screen.getByRole("button", { name: /1 likely your settings/ }));
    expect(within(filterGroup()).getByRole("radio", { checked: true })).toHaveAccessibleName("Likely your settings");
  });

  it("keeps the baseline and its BIOS when the main BIOS changes, and offers the baseline BIOS only with a baseline", async () => {
    analyzeProfile.mockResolvedValue(result("ANY-ID", { set: 0 }));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await choose(BIOS, "bios.HI0");
    await screen.findByText("ANY-ID");
    expect(screen.queryByRole("button", { name: /baseline BIOS/ })).not.toBeInTheDocument();
    await choose(/Add baseline profile/, "base.ocb");
    await choose(/Add baseline BIOS/, "old.HI0");
    expect(analyzeProfile).toHaveBeenLastCalledWith({ profile: "a.ocb", bios: "bios.HI0", set: null, baseline: "base.ocb", baselineBios: "old.HI0" });

    await choose(/Change BIOS/, "bios2.HJ0");
    expect(analyzeProfile).toHaveBeenLastCalledWith({
      profile: "a.ocb",
      bios: "bios2.HJ0",
      set: null,
      baseline: "base.ocb",
      baselineBios: "old.HI0",
    });
    expect(screen.getByRole("button", { name: "Change baseline" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Change baseline BIOS" })).toBeInTheDocument();
  });

  it("a new baseline BIOS starts a new request and invalidates the pending and the shown result", async () => {
    const pending = deferred<Result>();
    await withBaseline(result("PLAIN-ID", { set: 0 }), result("APPLIED-ID", { set: 0, baseline: "applied" }));
    analyzeProfile.mockReturnValueOnce(pending.promise);
    await choose(/Add baseline BIOS/, "old.HI0");
    expect(analyzeProfile).toHaveBeenLastCalledWith({ profile: "a.ocb", bios: "bios.HI0", set: null, baseline: "base.ocb", baselineBios: "old.HI0" });
    expect(screen.queryByText("APPLIED-ID")).not.toBeInTheDocument();
    expect(saveButton()).toBeDisabled();

    // Another change while it is pending: the first answer must not appear.
    analyzeProfile.mockResolvedValueOnce(result("LATEST-ID", { set: 0, baseline: "applied" }));
    await userEvent.click(screen.getByRole("button", { name: "Remove baseline BIOS" }));
    expect(analyzeProfile).toHaveBeenLastCalledWith({ profile: "a.ocb", bios: "bios.HI0", set: null, baseline: "base.ocb" });
    expect(await screen.findByText("LATEST-ID")).toBeInTheDocument();
    await act(async () => pending.resolve(result("STALE-ID", { set: 0, baseline: "applied", cross: true })));
    expect(screen.queryByText("STALE-ID")).not.toBeInTheDocument();
    expect(screen.getByText("LATEST-ID")).toBeInTheDocument();
  });

  it("saves the baseline BIOS in the request", async () => {
    const applied = result("CROSS-ID", { set: 0, baseline: "applied", cross: true });
    await withBaseline(result("PLAIN-ID", { set: 0 }), result("APPLIED-ID", { set: 0, baseline: "applied" }));
    analyzeProfile.mockResolvedValueOnce(applied);
    await choose(/Add baseline BIOS/, "old.HI0");
    await screen.findByText("CROSS-ID");
    pickSave.mockResolvedValueOnce("C:/out/a.analysis.json");
    saveAnalysis.mockResolvedValueOnce("C:/out/a.analysis.json");
    await userEvent.click(saveButton());
    await waitFor(() =>
      expect(saveAnalysis).toHaveBeenCalledWith(
        { profile: "a.ocb", bios: "bios.HI0", set: null, baseline: "base.ocb", baselineBios: "old.HI0" },
        applied.payload.inputs,
        "C:/out/a.analysis.json",
      ),
    );
  });

  it("shows the cross-version note above the table only for a cross-version baseline", async () => {
    await withBaseline(result("PLAIN-ID", { set: 0 }), result("CROSS-ID", { set: 0, baseline: "applied", cross: true }));
    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    expect(screen.getByTestId("cross-note")).toHaveTextContent(
      "Matched across two BIOS versions by menu place and name; 3 settings could not be compared.",
    );
  });

  it("puts the facts and reasons in the Summary, not above the tabs", async () => {
    analyzeProfile.mockResolvedValueOnce(result("E7D91IMS.HI0", { set: 0 }));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await screen.findByRole("radio", { name: "Settings" });
    expect(screen.getByText("Why the result looks like this")).toBeInTheDocument();
    expect(screen.getByText("Identifier in profile")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("radio", { name: "Settings" }));
    expect(screen.queryByText("Why the result looks like this")).not.toBeInTheDocument();
    expect(screen.queryByText("Identifier in profile")).not.toBeInTheDocument();
    expect(screen.queryByText(/Sharing is your choice/)).not.toBeInTheDocument();
    // What every tab keeps: the tabs and the one-line notice.
    expect(screen.getByRole("radio", { name: "Summary" })).toBeInTheDocument();
    expect(screen.getByText("Integrity not established")).toBeInTheDocument();
  });

  it("saves with the baseline in the request", async () => {
    const applied = result("APPLIED-ID", { set: 0, baseline: "applied" });
    await withBaseline(result("PLAIN-ID", { set: 0 }), applied);
    pickSave.mockResolvedValueOnce("C:/out/a.analysis.json");
    saveAnalysis.mockResolvedValueOnce("C:/out/a.analysis.json");
    await userEvent.click(saveButton());
    await waitFor(() =>
      expect(saveAnalysis).toHaveBeenCalledWith(
        { profile: "a.ocb", bios: "bios.HI0", set: null, baseline: "base.ocb" },
        applied.payload.inputs,
        "C:/out/a.analysis.json",
      ),
    );
  });
});

describe("Analyze: shareable report", () => {
  const shareButton = () => screen.getByRole("button", { name: "Save shareable report" });
  const NOTE = "Leaves out the CPU name, the save time and raw record bytes.";

  it("says what it leaves out and is disabled until the result for these files is shown", async () => {
    const first = deferred<Result>();
    analyzeProfile.mockReturnValueOnce(first.promise).mockReturnValueOnce(new Promise(() => {}));
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    expect(shareButton()).toBeDisabled();
    expect(screen.getByText(NOTE)).toBeInTheDocument();
    expect(shareButton()).toHaveAccessibleDescription(NOTE);

    await act(async () => first.resolve(result("FIRST-ID")));
    await screen.findByText("FIRST-ID");
    expect(shareButton()).toBeEnabled();
    await choose(PROFILE, "c.ocb");
    expect(shareButton()).toBeDisabled();
  });

  it("calls the bridge with shareable = true for the shown result", async () => {
    const r = result("SHARE-ID");
    analyzeProfile.mockResolvedValueOnce(r);
    pickSave.mockResolvedValueOnce("C:/out/a.shareable.analysis.json");
    saveAnalysis.mockResolvedValueOnce("C:/out/a.shareable.analysis.json");
    render(<Analyze />);
    await choose(PROFILE, "a.ocb");
    await screen.findByText("SHARE-ID");
    await userEvent.click(shareButton());

    expect(pickSave).toHaveBeenCalledWith("a.shareable.analysis.json", "json", "Save shareable report");
    await waitFor(() =>
      expect(saveAnalysis).toHaveBeenCalledWith({ profile: "a.ocb", bios: null, set: null }, r.payload.inputs, "C:/out/a.shareable.analysis.json", true),
    );
    expect(await screen.findByText("C:/out/a.shareable.analysis.json")).toBeInTheDocument();
  });
});
