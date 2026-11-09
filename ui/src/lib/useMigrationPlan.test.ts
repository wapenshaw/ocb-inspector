import { act, renderHook } from "@testing-library/react";
import type { MigrationEntry, Patch } from "../api/types";

const migratePlan = vi.fn();
vi.mock("../api", async (original) => ({ ...(await original<typeof import("../api")>()), api: { migratePlan } }));
const { useMigrationPlan } = await import("./useMigrationPlan");

const paths = { sourceBios: "s.bin", sourceProfile: "s.ocb", targetBios: "t.bin", targetProfile: "t.ocb" };
const entry = (id: string): MigrationEntry => ({ setting_id: id, selection: "selected", resolution: { kind: "none" } }) as MigrationEntry;
const planOf = (tag: string, entries: MigrationEntry[]): Patch => ({ tag, migration: { entries, selection: { groups: [], settings: [] } } }) as unknown as Patch;
const unselect = (id: string) => (e: MigrationEntry) => (e.setting_id === id ? { ...e, selection: "unselected" as const } : e);

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => (resolve = res));
  return { promise, resolve };
}

describe("useMigrationPlan", () => {
  it("keeps both of two quick changes and shows only the newest answer", async () => {
    const first = planOf("first", [entry("a"), entry("b")]);
    migratePlan.mockResolvedValueOnce(first);
    const { result } = renderHook(() => useMigrationPlan(paths, true, ["cpu"]));
    await act(async () => {});
    expect(result.current.plan).toBe(first);

    // Two changes before either answer arrives.
    const answerA = deferred<Patch>();
    const answerB = deferred<Patch>();
    migratePlan.mockReturnValueOnce(answerA.promise).mockReturnValueOnce(answerB.promise);
    let pa!: Promise<void>, pb!: Promise<void>;
    act(() => {
      pa = result.current.change(unselect("a"));
    });
    act(() => {
      pb = result.current.change(unselect("b"));
    });

    // The second request carries the first change too: it builds on the newest choices.
    const sentB = migratePlan.mock.calls[2]![2] as Patch;
    expect(sentB.migration!.entries.map((e) => e.selection)).toEqual(["unselected", "unselected"]);
    // While it is out, the controls show those choices.
    expect(result.current.pending).toBe(sentB);

    // Answers arrive out of order: the newest first, then the stale one.
    const newest = planOf("newest", sentB.migration!.entries);
    await act(async () => {
      answerB.resolve(newest);
      await pb;
    });
    await act(async () => {
      answerA.resolve(planOf("stale", [entry("a"), entry("b")]));
      await pa;
    });
    expect(result.current.plan).toBe(newest);
    expect(result.current.planning).toBe(false);
    expect(result.current.pending).toBeNull();
  });

  it("keeps the choices when a re-plan fails, and sends them again on retry", async () => {
    const first = planOf("first", [entry("a"), entry("b")]);
    migratePlan.mockResolvedValueOnce(first);
    const { result } = renderHook(() => useMigrationPlan(paths, true, ["cpu"]));
    await act(async () => {});

    // A change whose request fails: the plan is gone (nothing to save or build), the error shows.
    migratePlan.mockRejectedValueOnce({ message: "cannot read t.ocb" });
    await act(async () => {
      await result.current.change(unselect("a"));
    });
    expect(result.current.plan).toBeNull();
    expect(result.current.error).toBe("cannot read t.ocb");

    // Retrying sends the same choices; a further change still builds on them.
    migratePlan.mockImplementation(async (_p, _sel, prior: Patch) => planOf("again", prior.migration!.entries));
    await act(async () => {
      await result.current.retry();
    });
    const retried = migratePlan.mock.lastCall![2] as Patch;
    expect(retried.migration!.entries.map((e) => e.selection)).toEqual(["unselected", "selected"]);
    expect(result.current.error).toBe("");
    expect(result.current.plan!.migration!.entries.map((e) => e.selection)).toEqual(["unselected", "selected"]);

    migratePlan.mockRejectedValueOnce({ message: "cannot read t.ocb" });
    await act(async () => {
      await result.current.change(unselect("b"));
    });
    await act(async () => {
      await result.current.retry();
    });
    expect((migratePlan.mock.lastCall![2] as Patch).migration!.entries.map((e) => e.selection)).toEqual(["unselected", "unselected"]);
    migratePlan.mockReset();
  });

  it("drops an answer that arrives after the pairs are closed", async () => {
    const answer = deferred<Patch>();
    migratePlan.mockReturnValueOnce(answer.promise);
    const { result, rerender } = renderHook(({ ready }) => useMigrationPlan(paths, ready, ["cpu"]), { initialProps: { ready: true } });
    rerender({ ready: false });
    await act(async () => {
      answer.resolve(planOf("late", [entry("a")]));
    });
    expect(result.current.plan).toBeNull();
    expect(result.current.planning).toBe(false);
  });
});
