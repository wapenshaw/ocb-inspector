import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { BiosSummary, DiffPayload, InspectPayload, Report } from "../api/types";

const pickFile = vi.fn();
const biosSummary = vi.fn();
const inspect = vi.fn();
const diff = vi.fn();
vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), pickFile, api: { biosSummary, inspect, diff } }));
const { Compare } = await import("../components/Compare");

const summary = (id: string) =>
  ({ bios_id: id, profiles_supported: true, platform: null, build_date: "2026-10-01", sha256: "ab".repeat(32), approved_for_writing: false }) as unknown as BiosSummary;
const report = (id: string) =>
  ({ payload: { bios: { bios_id: id, revision: id }, profile: { saved_at: "20261001120000", cpu_brand: "Test CPU" } } }) as unknown as Report<InspectPayload>;
const diffOf = (compared: number) => ({ payload: { all_differences: [], compared, same_bios: true } }) as unknown as Report<DiffPayload>;

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}

const slot = (name: string) => within(screen.getByRole("region", { name }));

async function choose(region: string, button: RegExp, file: string) {
  pickFile.mockResolvedValueOnce(file);
  await userEvent.click(slot(region).getByRole("button", { name: button }));
}

describe("Compare", () => {
  it("never shows the answer for files that were changed while it was on its way", async () => {
    biosSummary.mockImplementation(async (b: string) => summary(b));
    inspect.mockImplementation(async (b: string) => report(b));
    const old = deferred<Report<DiffPayload>>();
    const newer = deferred<Report<DiffPayload>>();
    diff.mockReturnValueOnce(old.promise).mockReturnValueOnce(newer.promise);

    render(<Compare />);
    await choose("First profile", /BIOS image/, "A.BIN");
    await choose("First profile", /OC profile/, "a.ocb");
    await choose("Second profile", /BIOS image/, "B.BIN");
    await choose("Second profile", /OC profile/, "b.ocb");
    expect(diff).toHaveBeenLastCalledWith("A.BIN", "a.ocb", "B.BIN", "b.ocb");
    expect(await screen.findByText("Comparing…")).toBeInTheDocument();

    // Change the second profile while the first compare is still out.
    await userEvent.click(screen.getByRole("button", { name: "Change files" }));
    await choose("First profile", /OC profile/, "a.ocb");
    await choose("Second profile", /OC profile/, "c.ocb");
    expect(diff).toHaveBeenLastCalledWith("A.BIN", "a.ocb", "B.BIN", "c.ocb");
    expect(await screen.findByText("Comparing…")).toBeInTheDocument();

    // The old answer arrives first: it is dropped and the new compare is still running.
    await act(async () => old.resolve(diffOf(111)));
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    expect(screen.getByText("Comparing…")).toBeInTheDocument();

    await act(async () => newer.resolve(diffOf(222)));
    expect(await screen.findByText(/0 of 222 settings differ/)).toBeInTheDocument();
  });
});
