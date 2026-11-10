import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { BiosSummary } from "../api/types";

const pickFile = vi.fn();
const biosSummary = vi.fn();
const inspect = vi.fn();
vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), pickFile, api: { biosSummary, inspect } }));
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

const analyzeButton = () => screen.queryByRole("button", { name: /Analyze profile experimentally/ });

describe("experimental analysis offer after a failed pairing", () => {
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
    expect(onAnalyze).toHaveBeenCalledWith("b", "p.ocb");
  });
});
