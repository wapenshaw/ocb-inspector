import { createRef } from "react";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { analysisFixture } from "./analysisFixture";
import { FORMAT_BIOS_SHA, profileFormatFixture } from "./profileFormatFixture";

const pickFile = vi.fn();
const pickSave = vi.fn();
const analyzeProfile = vi.fn();
const saveAnalysis = vi.fn();
const profileFormat = vi.fn();
const saveProfileFormat = vi.fn();
const openPage = vi.fn();
vi.mock("../api", async (original) => ({
  ...(await original<typeof import("../api")>()),
  pickFile,
  pickSave,
  api: { analyzeProfile, saveAnalysis, profileFormat, saveProfileFormat, openPage },
}));
const { Analyze } = await import("../components/Analyze");
import type { AnalyzeHandle } from "../components/Analyze";

beforeEach(() => {
  for (const f of [pickFile, pickSave, analyzeProfile, saveAnalysis, profileFormat, saveProfileFormat, openPage]) f.mockReset();
});

const onClose = vi.fn();

// The fallback view is opened with a profile; the format is then one click away.
async function openFormat() {
  const handle = createRef<AnalyzeHandle>();
  render(<Analyze ref={handle} onClose={onClose} />);
  analyzeProfile.mockResolvedValue(analysisFixture({ set: 0 }));
  await act(async () => handle.current?.open("C:/bios/E7D91IMS.HJ0", "a.ocb"));
  await userEvent.click(screen.getByRole("button", { name: "Profile format" }));
  await act(async () => {});
}

describe("Profile format (BIOS only)", () => {
  it("is offered once a BIOS is given, and reads it without opening the profile again", async () => {
    profileFormat.mockResolvedValue(profileFormatFixture());
    await openFormat();
    expect(profileFormat).toHaveBeenCalledWith("C:/bios/E7D91IMS.HJ0", null);
    expect(analyzeProfile).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Change BIOS" })).toBeNull();
  });

  it("is not offered when no BIOS was given", async () => {
    const handle = createRef<AnalyzeHandle>();
    render(<Analyze ref={handle} onClose={onClose} />);
    analyzeProfile.mockResolvedValue(analysisFixture({ bios: false }));
    await act(async () => handle.current?.open("", "a.ocb"));
    expect(screen.queryByRole("button", { name: "Profile format" })).toBeNull();
  });

  it("labels each part of the layout with what it rests on, and marks skipped positions on a traced release", async () => {
    profileFormat.mockResolvedValue(profileFormatFixture());
    await openFormat();
    const layout = screen.getByRole("list", { name: "File layout, in file order" });
    const parts = within(layout).getAllByRole("listitem");
    expect(parts.map((p) => p.querySelector(".mono")?.textContent)).toEqual(["header", "checksum", "records", "skipped", "cmos", "trailer"]);
    expect(within(layout).getAllByText("Traced in code")).toHaveLength(6);
    expect(screen.getByText("Traced for this release")).toBeTruthy();
    const row = screen.getByText("UserDefaults").closest("tr")!;
    expect(within(row).getByText("Not saved")).toBeTruthy();
    expect(within(screen.getByText("Setup").closest("tr")!).getByText("Saved")).toBeTruthy();
  });

  it("shows untraced parts as expected and every position as unknown", async () => {
    profileFormat.mockResolvedValue(profileFormatFixture({ traced: false }));
    await openFormat();
    expect(screen.getByText("Not traced for this release")).toBeTruthy();
    expect(screen.getAllByText("Expected, not traced").length).toBeGreaterThan(0);
    expect(screen.queryByText("Saved")).toBeNull();
    expect(screen.getAllByText("Unknown").length).toBe(7);
  });

  it("filters the list by name or record id", async () => {
    profileFormat.mockResolvedValue(profileFormatFixture());
    await openFormat();
    const find = screen.getByRole("searchbox", { name: "Find a variable or record id" });
    await userEvent.type(find, "5");
    expect(screen.getByText("CpuSetup")).toBeTruthy();
    expect(screen.queryByText("Timeout")).toBeNull();
    await userEvent.clear(find);
    await userEvent.click(screen.getByRole("checkbox", { name: "Only variables with BIOS settings" }));
    expect(screen.queryByText("UserDefaults")).toBeNull();
    expect(screen.getByText("PlatformLang")).toBeTruthy();
  });

  it("saves through the engine with the digest of the image shown", async () => {
    profileFormat.mockResolvedValue(profileFormatFixture());
    pickSave.mockResolvedValue("C:/out/f.json");
    saveProfileFormat.mockResolvedValue("C:/out/f.json");
    await openFormat();
    await userEvent.click(screen.getByRole("button", { name: "Save profile format" }));
    await act(async () => {});
    expect(saveProfileFormat).toHaveBeenCalledWith("C:/bios/E7D91IMS.HJ0", null, FORMAT_BIOS_SHA, "C:/out/f.json");
    expect(screen.getByText(/Saved to/).textContent).toContain("C:/out/f.json");
  });

  it("goes back to the results", async () => {
    profileFormat.mockResolvedValue(profileFormatFixture());
    await openFormat();
    await userEvent.click(screen.getByRole("button", { name: "Back to results" }));
    expect(screen.getByRole("radio", { name: "Settings" })).toBeTruthy();
  });
});
