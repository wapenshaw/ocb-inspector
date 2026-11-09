// Two profiles side by side: every setting, reviewed or not, that differs between them.
// Works across BIOS releases (settings are matched by menu page and name). Read-only.
import { Fragment, useEffect, useMemo, useState } from "react";
import { api } from "../api";
import type { CompareEntry, DiffPayload, Report, Value } from "../api/types";
import { useAction } from "../lib/useAction";
import { display, fileName, savedAt } from "../lib/format";
import { cx } from "../lib/cx";
import { menuSort, pageBefore, sectionBefore, pageLabel } from "../lib/menu";
import { MoreRows, ROW_LIMIT } from "./MoreRows";
import { PairSetup, usePair } from "./PairSetup";
import { Status } from "./Status";
import { ValueText } from "./ValueText";
import { Icon } from "./Icon";
import s from "./Compare.module.css";

type Show = "all" | "reviewed" | "one";

const sameStock = (e: CompareEntry) => display(e.left_default) === display(e.right_default);
const stockOf = (v: Value | null) => (v ? display(v) : "—");

export function Compare() {
  const a = usePair();
  const b = usePair();

  const [diff, setDiff] = useState<Report<DiffPayload> | null>(null);
  const comparing = useAction();
  const [query, setQuery] = useState("");
  const [show, setShow] = useState<Show>("all");

  // Keyed on the paired reports, not the paths: a pair whose BIOS was just changed has no report
  // until its profile matches, so a half-chosen pair never triggers a compare.
  // Deps: the two reports only; the profile paths are read from the render that changed a report. A path
  // is set before its report, since the report comes from inspecting that path, so the closure's path is current.
  useEffect(() => {
    if (a.report && b.report) {
      load(a.bios, a.profile, b.bios, b.profile);
    } else {
      comparing.cancel();
      setDiff(null);
    }
  }, [a.report, b.report]);

  // A newer pair (or closing one) makes an answer still on its way stale; it is dropped.
  function load(lb: string, lp: string, rb: string, rp: string) {
    void comparing.run(
      async (current) => {
        const d = await api.diff(lb, lp, rb, rp);
        if (current()) setDiff(d);
      },
      () => setDiff(null),
    );
  }

  function reset() {
    a.setReport(null);
    b.setReport(null);
    setDiff(null);
  }

  const entries = useMemo(() => diff?.payload.all_differences ?? [], [diff]);
  const sorted = useMemo(
    () =>
      menuSort(entries).map((e) => ({
        e,
        hay: `${e.label} ${(e.menu?.path ?? []).join(" ")} ${display(e.left)} ${display(e.right)}`.toLowerCase(),
      })),
    [entries],
  );
  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return sorted
      .filter(({ e, hay }) => {
        if (show === "reviewed" && !e.reviewed) return false;
        if (show === "one" && !e.only) return false;
        return !q || hay.includes(q);
      })
      .map(({ e }) => e);
  }, [sorted, query, show]);
  const counts = {
    reviewed: entries.filter((e) => e.reviewed).length,
    one: entries.filter((e) => e.only).length,
  };
  const aId = a.report?.payload.bios.bios_id ?? "";
  const bId = b.report?.payload.bios.bios_id ?? "";

  if (!a.report || !b.report) {
    return (
      <PairSetup
        title="Compare two profiles"
        intro="Open two profiles, each with the BIOS that saved it. Every setting is compared by its place in the BIOS menus, so profiles from different BIOS versions line up too. Nothing is changed."
        slots={[
          { heading: "First profile", hint: "A profile and the BIOS that saved it.", pair: a },
          { heading: "Second profile", hint: "Another profile, from the same BIOS or a different version.", pair: b },
        ]}
      />
    );
  }

  return (
    <>
      <div className="strip" role="group" aria-label="Profiles compared">
        <span className={`mono strong`}>{aId}</span>
        <span className="quiet">{fileName(a.profile)}, saved {savedAt(a.report.payload.profile.saved_at)}</span>
        <span className={s.flow} aria-label="compared with">⇄</span>
        <span className={`mono strong`}>{bId}</span>
        <span className="quiet">{fileName(b.profile)}, saved {savedAt(b.report.payload.profile.saved_at)}</span>
        <span className="spacer"></span>
        <button className="btn ghost" onClick={reset}>Change files</button>
      </div>

      <div className="fill col">
        <div className={s.tools}>
          <label className={s.search}>
            <Icon name="search" />
            <span className="visually-hidden">Search differences</span>
            <input type="search" placeholder="Search differences" value={query} onChange={(e) => setQuery(e.target.value)} />
          </label>
          <div className="seg" role="radiogroup" aria-label="Show">
            <label className={cx(show === "all" && "on")}>
              <input type="radio" name="cmp" value="all" checked={show === "all"} onChange={() => setShow("all")} />
              All differences <span className="mono faint">{entries.length}</span>
            </label>
            <label className={cx(show === "reviewed" && "on")}>
              <input type="radio" name="cmp" value="reviewed" checked={show === "reviewed"} onChange={() => setShow("reviewed")} />
              Reviewed <span className="mono faint">{counts.reviewed}</span>
            </label>
            <label className={cx(show === "one" && "on", !counts.one && "off")}>
              <input type="radio" name="cmp" value="one" checked={show === "one"} onChange={() => setShow("one")} disabled={!counts.one} />
              In one BIOS only <span className="mono faint">{counts.one}</span>
            </label>
          </div>
          <span className={`${s.summary} quiet`}>
            {comparing.busy
              ? "Comparing…"
              : diff && (
                  <>
                    {entries.length} of {diff.payload.compared ?? 0} settings differ
                    {!diff.payload.same_bios && ", across two BIOS versions"}
                  </>
                )}
          </span>
        </div>

        {comparing.error ? (
          <div className={`problem ${s.inset}`} role="alert"><Status tone="blocked" text="Cannot compare" /><p>{comparing.error}</p></div>
        ) : diff ? (
          <div className="tablewrap">
            <table className={s.grid}>
              <colgroup><col style={{ width: "34%" }} /><col style={{ width: "20%" }} /><col style={{ width: "20%" }} /><col /></colgroup>
              <thead>
                <tr>
                  <th>Setting</th>
                  <th><span className="mono">{aId}</span> <span className="faint">{fileName(a.profile)}</span></th>
                  <th><span className="mono">{bId}</span> <span className="faint">{fileName(b.profile)}</span></th>
                  <th>Stock</th>
                </tr>
              </thead>
              <tbody>
                {rows.length === 0 ? (
                  <tr>
                    <td colSpan={4} className={`quiet empty`}>
                      {entries.length ? "No differences match." : "These profiles store the same value for every setting."}
                    </td>
                  </tr>
                ) : (
                  rows.slice(0, ROW_LIMIT).map((e, i) => {
                    const page = pageBefore(rows, i);
                    const section = sectionBefore(rows, i);
                    return (
                      <Fragment key={e.key}>
                        {page && (
                          <tr className={s.pagehead}><th colSpan={4} scope="colgroup">{pageLabel(page, "Not on a menu page")}</th></tr>
                        )}
                        {section && (
                          <tr className={s.section}><th colSpan={4} scope="colgroup">{section}</th></tr>
                        )}
                        <tr className={cx(!e.only && "heat-3", !!e.only && "heat-0")}>
                          <td className={s.name}>
                            {e.label}
                            {e.reviewed ? <span className="rv" title="Reviewed setting: values, units and dependencies checked">reviewed</span> : null}
                          </td>
                          <td>{e.only === "right" ? <Status text={`Not in ${aId}`} /> : <ValueText value={e.left} />}</td>
                          <td>{e.only === "left" ? <Status text={`Not in ${bId}`} /> : <ValueText value={e.right} />}</td>
                          <td className="quiet">
                            {sameStock(e) || !!e.only
                              ? stockOf(e.left_default ?? e.right_default)
                              : `${stockOf(e.left_default)} / ${stockOf(e.right_default)}`}
                          </td>
                        </tr>
                      </Fragment>
                    );
                  })
                )}
              </tbody>
            </table>
            <MoreRows total={rows.length} />
          </div>
        ) : null}
      </div>
    </>
  );
}
