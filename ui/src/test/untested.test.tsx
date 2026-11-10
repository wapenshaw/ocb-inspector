import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { BiosSummary, InspectPayload, Report } from "../api/types";
import { UNTESTED_TEXT } from "../components/UntestedNote";

const pickFile = vi.fn();
const biosSummary = vi.fn();
const inspect = vi.fn();
vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), pickFile, api: { biosSummary, inspect } }));
const { appStore } = await import("../state/app");
const { Inspect } = await import("../components/Inspect");

type Evidence = "examined" | "untested" | undefined;

const bios = (evidence: Evidence) =>
  ({
    bios_id: "E7D91IMS.HI0",
    revision: "HI0",
    profiles_supported: true,
    approved_for_writing: false,
    build_date: "2026-10-01",
    sha256: "ab".repeat(32),
    setup_module: "Setup",
    defaults: { available: false, source: "none" },
    platform: {
      vendor: "intel",
      family: "Intel LGA1700",
      chipset: null,
      models: null,
      version: "V17.J",
      parser: "msi-intel-setup",
      profiles_note: "note",
      ...(evidence ? { profile_evidence: evidence } : {}),
    },
  }) as unknown as BiosSummary;

const report = (evidence: Evidence) =>
  ({
    diagnostics: [],
    payload: {
      bios: bios(evidence),
      profile: { saved_at: "20261001120000", cpu_brand: "Test CPU" },
      settings: [],
      bios_settings: [],
      raw_fields: [],
      capabilities: { image_approved_for_writing: false },
    },
  }) as unknown as Report<InspectPayload>;

async function open(evidence: Evidence) {
  biosSummary.mockResolvedValue(bios(evidence));
  inspect.mockResolvedValue(report(evidence));
  render(<Inspect browsing={null} onBrowsingChange={() => {}} active />);
  pickFile.mockResolvedValueOnce("E7D91IMS.HI0");
  await userEvent.click(screen.getByRole("button", { name: /BIOS image/ }));
  pickFile.mockResolvedValueOnce("a.ocb");
  await userEvent.click(screen.getByRole("button", { name: /OC profile/ }));
  await screen.findByRole("group", { name: "Open pair" });
}

describe("untested board note", () => {
  beforeEach(() => {
    appStore.set({ writing: true });
    pickFile.mockReset();
    biosSummary.mockReset();
    inspect.mockReset();
  });

  it("Inspect shows it for an untested board, next to the verified status", async () => {
    await open("untested");
    expect(screen.getByText(UNTESTED_TEXT)).toBeInTheDocument();
    expect(UNTESTED_TEXT).toBe(
      "No profile from this board has been examined: the layout is assumed from the boards examined so far. Check a few values against your BIOS screens.",
    );
    expect(screen.getByText("Match verified")).toBeInTheDocument();
  });

  it("Inspect shows nothing for an examined board or when the field is absent", async () => {
    await open("examined");
    expect(screen.queryByText(UNTESTED_TEXT)).not.toBeInTheDocument();
  });

  it("Inspect shows nothing when the field is absent", async () => {
    await open(undefined);
    expect(screen.queryByText(UNTESTED_TEXT)).not.toBeInTheDocument();
  });
});
