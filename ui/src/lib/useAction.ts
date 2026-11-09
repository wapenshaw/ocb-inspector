// An action backed by one request at a time: busy while the newest run is out, its error when it
// fails. Starting a run (or `cancel`) makes every earlier run stale: a stale run never changes
// busy or error, and `work` gets `current()` to check before it applies its result, so an answer
// that arrives late can never overwrite a newer one.
import { useRef, useState } from "react";
import { errorMessage } from "../api";

export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const latest = useRef(0);

  async function run(work: (current: () => boolean) => Promise<void>, onError?: () => void) {
    const id = ++latest.current;
    const current = () => id === latest.current;
    setBusy(true);
    setError("");
    try {
      await work(current);
    } catch (e) {
      if (current()) {
        setError(errorMessage(e));
        onError?.();
      }
    } finally {
      if (current()) setBusy(false);
    }
  }

  /** Drop any run in flight (its result and error are ignored). */
  function cancel() {
    latest.current++;
    setBusy(false);
    setError("");
  }

  return { busy, error, setError, run, cancel };
}
