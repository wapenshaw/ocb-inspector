import { act, renderHook } from "@testing-library/react";
import { useAction } from "./useAction";

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("useAction", () => {
  it("drops an older answer that arrives after a newer one", async () => {
    const { result } = renderHook(() => useAction());
    const first = deferred<string>();
    const second = deferred<string>();
    const applied: string[] = [];
    const start = (d: ReturnType<typeof deferred<string>>) =>
      result.current.run(async (current) => {
        const v = await d.promise;
        if (current()) applied.push(v);
      });

    let p1!: Promise<void>, p2!: Promise<void>;
    act(() => {
      p1 = start(first);
      p2 = start(second);
    });
    await act(async () => {
      second.resolve("new");
      await p2;
    });
    expect(result.current.busy).toBe(false);
    await act(async () => {
      first.resolve("old");
      await p1;
    });
    expect(applied).toEqual(["new"]);
  });

  it("stays busy when an older run finishes while the newest is still out", async () => {
    const { result } = renderHook(() => useAction());
    const first = deferred<void>();
    const second = deferred<void>();
    let p1!: Promise<void>, p2!: Promise<void>;
    act(() => {
      p1 = result.current.run(() => first.promise);
      p2 = result.current.run(() => second.promise);
    });
    await act(async () => {
      first.reject({ message: "old failure" });
      await p1;
    });
    expect(result.current.busy).toBe(true);
    expect(result.current.error).toBe("");
    await act(async () => {
      second.resolve();
      await p2;
    });
    expect(result.current.busy).toBe(false);
  });

  it("ignores the error of a cancelled run", async () => {
    const { result } = renderHook(() => useAction());
    const d = deferred<void>();
    let p!: Promise<void>;
    act(() => {
      p = result.current.run(() => d.promise);
    });
    act(() => result.current.cancel());
    await act(async () => {
      d.reject({ message: "too late" });
      await p;
    });
    expect(result.current.error).toBe("");
    expect(result.current.busy).toBe(false);
  });
});
