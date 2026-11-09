// The Port plan. The user's choices (carry this setting, this decision) are kept as they were
// last asked for, apart from the engine's answers: each change builds on the newest request, not
// on the last answer, so two quick changes never drop one another, and only the newest answer is
// shown.
import { useEffect, useRef, useState } from "react";
import { api, type PairPaths } from "../api";
import type { MigrationEntry, Patch } from "../api/types";
import { useAction } from "./useAction";

export function useMigrationPlan(paths: PairPaths, ready: boolean, groups: string[]) {
  /** The engine's newest answer. */
  const [plan, setPlan] = useState<Patch | null>(null);
  /** The choices sent with a request still out, shown until its answer arrives. */
  const [pending, setPending] = useState<Patch | null>(null);
  /** The newest choices: the last request's, or the last answer's. */
  const choices = useRef<Patch | null>(null);
  /** The groups sent with the newest request. */
  const asked = useRef<string[]>(groups);
  const planner = useAction();

  function request(prior: Patch | null, sel: string[]) {
    choices.current = prior;
    asked.current = sel;
    setPending(prior);
    return planner.run(
      async (current) => {
        const next = await api.migratePlan(paths, { groups: sel, settings: [] }, prior);
        if (!current()) return;
        choices.current = next;
        setPlan(next);
        setPending(null);
      },
      // A failed request keeps its choices in `choices` for the next change or `retry`; only the
      // plan goes, so nothing can be saved or built until a new answer arrives.
      () => {
        setPlan(null);
        setPending(null);
      },
    );
  }

  // Plan afresh when both pairs are open; closing either drops any answer still on its way.
  // Deps: `ready` only. The effect is created in the render that changed `ready`, so it reads that
  // render's paths and groups.
  useEffect(() => {
    if (ready) {
      void request(null, groups);
    } else {
      planner.cancel();
      choices.current = null;
      setPlan(null);
      setPending(null);
    }
  }, [ready]);

  /** Apply `f` to every entry of the newest choices and re-plan (with `sel` as the groups). */
  function change(f: (e: MigrationEntry) => MigrationEntry, sel: string[] = groups) {
    const base = choices.current;
    const prior = base?.migration
      ? { ...base, migration: { ...base.migration, selection: { groups: sel, settings: [] }, entries: base.migration.entries.map(f) } }
      : null;
    return request(prior, sel);
  }

  /** Send the newest choices again, after a failed request. */
  function retry() {
    return request(choices.current, asked.current);
  }

  return { plan, pending, planning: planner.busy, error: planner.error, change, retry };
}
