// What a BIOS image says about the OC profiles it writes, without opening a profile: the file
// layout part by part (each labelled with what it rests on), the modules holding the profile
// code, and the setup browser's variable list, whose positions are the record ids. Read-only.
//
// Every change of BIOS or set starts a new run (useAction); Save is enabled only for the report
// shown for the current inputs, and the engine re-reads the image before writing it.
import { useEffect, useMemo, useState } from "react";
import { api, pickSave } from "../api";
import type { FormatBasis, ListedEntryInfo, ProfileFormatPayload, Report } from "../api/types";
import { useAction } from "../lib/useAction";
import { failureOf } from "../lib/analysis";
import { fileName, shortHash } from "../lib/format";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import { MoreRows, ROW_LIMIT } from "./MoreRows";
import { Status, type Tone } from "./Status";
import a from "./Analyze.module.css";
import s from "./ProfileFormat.module.css";

export const BASIS: Record<FormatBasis, { text: string; tone: Tone; title: string }> = {
  firmware_code: { text: "Traced in code", tone: "ok", title: "Read from this release's profile save and load code, or the same code in a traced release" },
  profiles: { text: "Seen in profiles", tone: "ok", title: "Observed in exported profiles" },
  image_structure: { text: "Found in this image", tone: "neutral", title: "Found in this BIOS image's structure; the code using it was not traced" },
  expected: { text: "Expected, not traced", tone: "attention", title: "Follows the traced releases; this release's code was not read" },
};

const SAVED: Record<ListedEntryInfo["saved"], { text: string; tone: Tone }> = {
  saved: { text: "Saved", tone: "ok" },
  skipped: { text: "Not saved", tone: "neutral" },
  unknown: { text: "Unknown", tone: "attention" },
};

interface Shown {
  bios: string;
  set: number | null;
  report: Report<ProfileFormatPayload>;
}

interface ProfileFormatProps {
  bios: string;
  /** Without it the BIOS is fixed and no "Change BIOS" button is shown. */
  onChooseBios?: () => void;
  onClose: () => void;
  closeText?: string;
}

export function ProfileFormat({ bios, onChooseBios, onClose, closeText = "Analyze a profile" }: ProfileFormatProps) {
  const [set, setSet] = useState<number | null>(null);
  const [shown, setShown] = useState<Shown | null>(null);
  const [failure, setFailure] = useState<{ title: string; message: string } | null>(null);
  const [query, setQuery] = useState("");
  const [withSettings, setWithSettings] = useState(false);
  const [saved, setSaved] = useState("");
  const reader = useAction();
  const saver = useAction();

  // A new BIOS starts from its default set.
  const [forBios, setForBios] = useState(bios);
  if (forBios !== bios) {
    setForBios(bios);
    setSet(null);
  }

  useEffect(() => {
    setShown(null);
    setFailure(null);
    setSaved("");
    saver.cancel();
    void reader.run(async (current) => {
      try {
        const report = await api.profileFormat(bios, set);
        if (current()) setShown({ bios, set, report });
      } catch (e) {
        if (current()) setFailure(failureOf(e));
      }
    });
    // Only the inputs start a run; the action helpers are recreated each render.
  }, [bios, set]);

  const report = shown && shown.bios === bios && shown.set === set ? shown.report : null;
  const p = report?.payload ?? null;
  const entries = useMemo(() => {
    const all = p?.variable_list?.entries ?? [];
    const q = query.trim().toLowerCase();
    return all.filter((e) => (!withSettings || e.field_count > 0) && (!q || e.name.toLowerCase().includes(q) || String(e.index) === q));
  }, [p, query, withSettings]);

  function save(r: Report<ProfileFormatPayload>) {
    void saver.run(async (still) => {
      const out = await pickSave(`${r.payload.bios.bios_id}.profile-format.json`, "json", "Save profile format");
      if (!out || !still()) return;
      const path = await api.saveProfileFormat(bios, set, r.payload.bios.sha256, out);
      if (still()) setSaved(path);
    });
  }

  const sets = p?.bios.sets ?? [];
  const canSave = !!report && !reader.busy && !saver.busy;
  const total = p?.variable_list?.entries.length ?? 0;

  return (
    <>
      <div className={`strip ${a.inputs}`} role="group" aria-label="BIOS image read">
        <span className="quiet">BIOS</span>
        <span className={`${a.name} mono`} title={bios}>{p?.bios.bios_id ?? fileName(bios)}</span>
        {onChooseBios && <button className="btn ghost" onClick={onChooseBios}>Change BIOS</button>}
        {sets.length > 1 && (
          <label className={a.setpick}>
            Firmware set
            <select value={set ?? 0} onChange={(e) => setSet(Number(e.target.value))}>
              {sets.map((label, i) => (
                <option key={i} value={i}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        )}
        <span className="spacer" />
        <button className="btn ghost" onClick={onClose}>{closeText}</button>
        <button
          className="btn primary"
          disabled={!canSave}
          title={canSave ? "Save this profile format as a JSON report" : "Available once the format of this BIOS is shown"}
          onClick={() => report && save(report)}
        >
          Save profile format
        </button>
      </div>

      <div className={a.banner} role="note">
        <Icon name="alert" size={15} />
        <div>
          <p>What this BIOS writes into an OC profile, read from the image alone.</p>
          <p className="quiet">No profile is opened. Parts not traced in this release's code are marked as expected.</p>
        </div>
      </div>

      <div className={`fill scroller ${s.body}`}>
        {reader.busy && !report && (
          <p className={`quiet ${a.waiting}`} role="status">
            Reading the BIOS image…
          </p>
        )}
        {failure && !reader.busy && (
          <div className={`problem ${a.inset}`} role="alert">
            <Status tone="blocked" text={failure.title} />
            <p>{failure.message}</p>
          </div>
        )}
        {p && (
          <>
            <dl className={a.facts}>
              <Fact label="BIOS image">
                <span className="mono">{p.bios.bios_id}</span> <span className="mono faint">{shortHash(p.bios.sha256)}</span>
              </Fact>
              <Fact label="Platform">{p.bios.family}</Fact>
              <Fact label="Profile code">
                {p.traced ? (
                  <Status tone="ok" text="Traced for this release" />
                ) : (
                  <Status tone="attention" text="Not traced for this release" />
                )}
              </Fact>
              <Fact label="Variable list">
                {p.variable_list ? (
                  <>
                    <span className="mono">{total}</span> variables <span className="mono faint">{shortHash(p.variable_list.sha256)}</span>
                  </>
                ) : (
                  "Not found"
                )}
              </Fact>
            </dl>

            {saver.error && (
              <div className={`problem ${a.inset}`} role="alert">
                <Status tone="blocked" text="Could not save the profile format" />
                <p>{saver.error}</p>
              </div>
            )}
            {saved && (
              <p className={`quiet ${a.meta}`} role="status">
                Saved to <span className="mono">{saved}</span>
              </p>
            )}

            <section aria-labelledby="pf-layout">
              <h2 id="pf-layout" className={s.h}>File layout, in file order</h2>
              <ol className={s.layout} aria-labelledby="pf-layout">
                {p.layout.map((f) => (
                  <li key={f.part} className={cx(s.part, f.basis === "expected" && s.expected)}>
                    <span className={`mono ${s.partName}`}>{f.part.replace("_", " ")}</span>
                    <span>{f.detail}</span>
                    <Status tone={BASIS[f.basis].tone} text={BASIS[f.basis].text} title={BASIS[f.basis].title} />
                  </li>
                ))}
              </ol>
              {p.code.length > 0 && (
                <p className={`quiet ${s.code}`}>
                  Profile code in{" "}
                  {p.code.map((m, i) => (
                    <span key={m.sha256}>
                      {i > 0 && ", "}
                      <span className="mono" title={`${m.guid}\n${m.sha256}`}>{m.name || m.guid}</span> (
                      {[m.holds_mos && "$MOS$", m.holds_oci && "$OCI$"].filter(Boolean).join(" and ")})
                    </span>
                  ))}
                  .
                </p>
              )}
            </section>

            <section aria-labelledby="pf-vars">
              <h2 id="pf-vars" className={s.h}>Record ids: the setup browser's variable list</h2>
              {p.variable_list ? (
                <>
                  <div className={s.filters}>
                    <input
                      type="search"
                      placeholder="Find a variable or record id"
                      aria-label="Find a variable or record id"
                      value={query}
                      onChange={(e) => setQuery(e.target.value)}
                    />
                    <label className={s.toggle}>
                      <input type="checkbox" checked={withSettings} onChange={(e) => setWithSettings(e.target.checked)} />
                      Only variables with BIOS settings
                    </label>
                    <span className="quiet" role="status">
                      {entries.length} of {total}
                    </span>
                  </div>
                  {entries.length === 0 ? (
                    <p className="quiet empty">No variable matches.</p>
                  ) : (
                    <div className="tablewrap">
                      <table className={a.grid}>
                        <colgroup>
                          <col style={{ width: "9%" }} />
                          <col style={{ width: "27%" }} />
                          <col style={{ width: "11%" }} />
                          <col style={{ width: "11%" }} />
                          <col style={{ width: "10%" }} />
                          <col style={{ width: "12%" }} />
                          <col />
                        </colgroup>
                        <thead>
                          <tr>
                            <th>Record id</th>
                            <th>Variable</th>
                            <th>Declared size</th>
                            <th>Menu store</th>
                            <th>Settings</th>
                            <th>In a profile</th>
                            <th>Note</th>
                          </tr>
                        </thead>
                        <tbody>
                          {entries.slice(0, ROW_LIMIT).map((e) => (
                            <tr key={e.index}>
                              <td className="mono">{e.index}</td>
                              <td>
                                {e.name}
                                <span className={`${a.at} mono`}>{e.guid}</span>
                              </td>
                              <td className="mono">{e.declared_size || "—"}</td>
                              <td className="mono">{e.store_size ?? "—"}</td>
                              <td className="mono">{e.field_count || "—"}</td>
                              <td>
                                <Status tone={SAVED[e.saved].tone} text={SAVED[e.saved].text} />
                              </td>
                              <td>{e.note && <span className={a.reason}>{e.note}</span>}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                      <MoreRows total={entries.length} />
                    </div>
                  )}
                </>
              ) : (
                <p className="quiet">{p.list_problem}</p>
              )}
            </section>

            <section aria-labelledby="pf-limits">
              <h2 id="pf-limits" className={s.h}>Limits of this reading</h2>
              <ul className={s.limits}>
                {p.limitations.map((l, i) => (
                  <li key={i}>{l}</li>
                ))}
              </ul>
            </section>
          </>
        )}
      </div>
    </>
  );
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}
