import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { BiosSummary, ExperimentalInspectPayload, Report } from "../api/types";
import { analysisFixture } from "./analysisFixture";

const pickFile = vi.fn();
const biosSummary = vi.fn();
const inspect = vi.fn();
const analyzeProfile = vi.fn();
vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), pickFile, api: { biosSummary, inspect, analyzeProfile } }));
const { PairPicker } = await import("../components/PairPicker");

const bios = {
  bios_id: "E7D25IMS.HP0",
  profiles_supported: true,
  approved_for_writing: false,
  build_date: "2026-08-25",
  sha256: "ab".repeat(32),
} as unknown as BiosSummary;

// Picks a BIOS and a profile; the pairing fails with the given engine error.
async function failPair(error: { code: string; exit_code: number; message: string }) {
  biosSummary.mockResolvedValue(bios);
  inspect.mockRejectedValue(error);
  const onAnalyze = vi.fn();
  function Host() {
    return <PairPicker heading="Profile to inspect" hint="" bios="b" profile="p.ocb" report={null} onAnalyze={onAnalyze} />;
  }
  render(<Host />);
  pickFile.mockResolvedValueOnce("b");
  await userEvent.click(screen.getByRole("button", { name: /BIOS image/ }));
  await screen.findByRole("alert");
  return onAnalyze;
}

const analyzeButton = () => screen.queryByRole("button", { name: "Show what can be read from the file" });

describe("offer to read the file anyway after a failed pairing", () => {
  it("is not offered when the profile was saved by another BIOS", async () => {
    await failPair({
      code: "identity_mismatch",
      exit_code: 4,
      message: "BIOS/profile identity mismatch: profile was saved by E7D91IMS.HJ0 but the BIOS image is E7D25IMS.HP0.",
    });
    expect(screen.getByText("Profile and BIOS do not match")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose matching BIOS" })).toBeInTheDocument();
    expect(analyzeButton()).toBeNull();
  });

  it("is offered when the profile's layout is not one the reader knows", async () => {
    const onAnalyze = await failPair({ code: "malformed_input", exit_code: 4, message: "malformed OCB: record 0 (id 0) truncated" });
    expect(screen.getByText("Could not open")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Choose matching BIOS" })).toBeNull();
    await userEvent.click(analyzeButton()!);
    expect(onAnalyze).toHaveBeenCalledWith("b", "p.ocb", null);
  });
});

const unsupported = {
  ...bios,
  bios_id: "E7C77IMS.1C0",
  profiles_supported: false,
  platform: { profiles_note: "Profiles are not examined for this board." },
} as unknown as BiosSummary;

describe("an image that cannot take a profile", () => {
  beforeEach(() => {
    pickFile.mockReset();
    biosSummary.mockReset();
    inspect.mockReset();
    analyzeProfile.mockReset();
    biosSummary.mockResolvedValue(unsupported);
  });

  const slot = (name: RegExp) => screen.getByRole("button", { name });

  const matched = (opts: Parameters<typeof analysisFixture>[0] = { set: 0 }): Report<ExperimentalInspectPayload> => analysisFixture(opts);

  // Renders the picker the way Inspect does; both files are chosen through the slots.
  async function pairBoth(report: Report<ExperimentalInspectPayload> | Error | { code: string; exit_code: number; message: string }) {
    if ("payload" in report) analyzeProfile.mockResolvedValue(report);
    else analyzeProfile.mockRejectedValue(report);
    const onAnalyze = vi.fn();
    const onUnverified = vi.fn();
    const onBrowse = vi.fn();
    function Host() {
      const [profile, setProfile] = useState("");
      const [b, setB] = useState("");
      return (
        <PairPicker heading="Profile to inspect" hint="" bios={b} onBiosChange={setB} profile={profile} onProfileChange={setProfile} onAnalyze={onAnalyze} onUnverified={onUnverified} onBrowse={onBrowse} />
      );
    }
    render(<Host />);
    pickFile.mockResolvedValueOnce("c.1C0");
    await userEvent.click(slot(/^BIOS image/));
    expect(await screen.findByText("Settings read from this BIOS's own definitions")).toBeInTheDocument();
    pickFile.mockResolvedValueOnce("p.ocb");
    await userEvent.click(slot(/OC profile/));
    return { onAnalyze, onUnverified, onBrowse };
  }

  it("reads the pair with the experimental reader, never the validated one, and hands a confirmed match on", async () => {
    const { onAnalyze, onUnverified, onBrowse } = await pairBoth(matched());
    await waitFor(() => expect(onUnverified).toHaveBeenCalledTimes(1));
    expect(analyzeProfile).toHaveBeenCalledWith({ profile: "p.ocb", bios: "c.1C0", set: null });
    expect(inspect).not.toHaveBeenCalled();
    expect(onAnalyze).not.toHaveBeenCalled();
    expect(onBrowse).not.toHaveBeenCalled();
    const view = onUnverified.mock.calls[0]![0];
    expect(view.bios).toBe(unsupported);
    expect(view.settings.length).toBeGreaterThan(0);
    expect(document.body.textContent ?? "").not.toMatch(/verified/i);
  });

  it("says a profile saved by another BIOS does not match, from the report or from a thrown error", async () => {
    const r = matched();
    r.payload.identity.status = "mismatch";
    r.payload.identity.reason = "profile was saved by E7D91IMS.HJ0 but the BIOS image is E7C77IMS.1C0.";
    const { onUnverified, onAnalyze } = await pairBoth(r);
    expect(await screen.findByText("Profile and BIOS do not match")).toBeInTheDocument();
    expect(screen.getByText(/saved by E7D91IMS.HJ0/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose matching BIOS" })).toBeInTheDocument();
    expect(analyzeButton()).toBeNull();
    expect(onUnverified).not.toHaveBeenCalled();
    expect(onAnalyze).not.toHaveBeenCalled();
  });

  it("shows the same box when the engine refuses with an identity_mismatch error", async () => {
    await pairBoth({ code: "identity_mismatch", exit_code: 4, message: "BIOS/profile identity mismatch: profile was saved by E7D91IMS.HJ0." });
    expect(await screen.findByText("Profile and BIOS do not match")).toBeInTheDocument();
    expect(screen.getByText("profile was saved by E7D91IMS.HJ0.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose matching BIOS" })).toBeInTheDocument();
  });

  it.each(["unknown", "conflicting"] as const)("says an identity that is %s could not be confirmed, with the reason, and opens no settings", async (status) => {
    const r = matched();
    r.payload.identity.status = status;
    r.payload.identity.reason = "The profile names no BIOS.";
    const { onUnverified, onAnalyze } = await pairBoth(r);
    expect(await screen.findByText("Could not confirm that this BIOS saved the profile")).toBeInTheDocument();
    expect(screen.getByText("The profile names no BIOS.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Choose matching BIOS" })).toBeNull();
    expect(analyzeButton()).toBeNull();
    expect(onUnverified).not.toHaveBeenCalled();
    expect(onAnalyze).not.toHaveBeenCalled();
  });

  it("keeps firmware set selection inline", async () => {
    const { onAnalyze, onUnverified } = await pairBoth(matched({ set: null }));
    expect(onAnalyze).not.toHaveBeenCalled();
    expect(screen.getByRole("combobox", { name: "Firmware set" })).toBeInTheDocument();
    expect(onUnverified).not.toHaveBeenCalled();
  });

  it("falls back to the analysis view when a confirmed match yields no settings", async () => {
    const r = matched();
    r.payload.fields = [];
    const { onAnalyze, onUnverified } = await pairBoth(r);
    expect(onAnalyze).not.toHaveBeenCalled();
    expect(screen.getByText("No settings could be read")).toBeInTheDocument();
    expect(onUnverified).not.toHaveBeenCalled();
  });

  it("reads the pair when the profile comes first, and when only a new profile is chosen after the picker was re-created", async () => {
    analyzeProfile.mockResolvedValue(matched());
    const onUnverified = vi.fn();
    function Host() {
      const [profile, setProfile] = useState("");
      return <PairPicker heading="Profile to inspect" hint="" bios="c.1C0" onBiosChange={() => {}} profile={profile} onProfileChange={setProfile} onAnalyze={vi.fn()} onUnverified={onUnverified} />;
    }
    render(<Host />);
    pickFile.mockResolvedValueOnce("p.ocb");
    await userEvent.click(slot(/OC profile/));
    await waitFor(() => expect(onUnverified).toHaveBeenCalledTimes(1));
    // The BIOS was chosen before, but this picker never read its summary: inspect is not tried on it.
    expect(biosSummary).toHaveBeenCalledWith("c.1C0");
    expect(inspect).not.toHaveBeenCalled();
  });

  it("shows BIOS match confirmed and the integrity wording on the profile slot of an opened pair", async () => {
    const { onUnverified } = await pairBoth(matched());
    await waitFor(() => expect(onUnverified).toHaveBeenCalled());
    const view = onUnverified.mock.calls[0]![0];
    const { unmount } = render(<PairPicker heading="Opened" hint="" bios="c.1C0" profile="p.ocb" unverified={view} onAnalyze={vi.fn()} />);
    expect(screen.getByText("BIOS match confirmed")).toBeInTheDocument();
    expect(screen.getByText("Not established")).toBeInTheDocument();
    expect(screen.queryByText(/Match verified|checksum valid/)).toBeNull();
    unmount();
  });

  it("without onAnalyze (Compare, Port) keeps the profile slot closed", async () => {
    render(<PairPicker heading="Left" hint="" bios="c.1C0" report={null} />);
    pickFile.mockResolvedValueOnce("c.1C0");
    await userEvent.click(slot(/^BIOS image/));
    expect(await screen.findByText("Browse only")).toBeInTheDocument();
    expect(screen.getByText("Not available for this BIOS")).toBeInTheDocument();
    expect(screen.getByText("Profiles are not examined for this board.")).toBeInTheDocument();
    expect(slot(/OC profile/)).toBeDisabled();
  });
});
