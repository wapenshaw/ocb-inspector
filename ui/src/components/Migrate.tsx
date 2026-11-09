import { Fragment, useMemo, useState } from "react";
import { api, errorMessage, pickSave, type PairPaths } from "../api";
import type {
  BuildResult,
  DecodedSetting,
  InspectPayload,
  MenuLocation,
  MigrationEntry,
  Reason,
  Report,
  ResolutionKind,
  Value,
} from "../api/types";
import { display, fileName, sameIntent, savedAt } from "../lib/format";
import { menuSort, pageBefore, pageKey, sectionBefore, pageLabel } from "../lib/menu";
import { cx } from "../lib/cx";
import { useAction } from "../lib/useAction";
import { useMigrationPlan } from "../lib/useMigrationPlan";
import { PairSetup, usePair } from "./PairSetup";
import { ResultPanel } from "./ResultPanel";
import { SwapLabel } from "./SwapLabel";
import { Status, type Tone } from "./Status";
import { ValueText } from "./ValueText";
import { ValueEditor } from "./ValueEditor";
import { Icon } from "./Icon";
import s from "./Migrate.module.css";

type Prov = "source" | "target" | "replaced" | "blocked" | "none";

/** Where the proposed value comes from, for its tooltip. */
const PROVENANCE_TEXT: Record<Prov, string> = {
  source: "Value comes from the source profile",
  replaced: "Value you chose",
  target: "Value kept from the target baseline",
  blocked: "",
  none: "",
};

const resolutionText: Record<ResolutionKind, string> = {
  none: "Carry the source value",
  keep_target: "Keep the target's value",
  omit: "Leave this setting out",
  set_value: "Use a different value",
};

function settingIn(r: Report<InspectPayload> | null, id: string): DecodedSetting | null {
  return r?.payload.settings.find((x) => x.setting_id === id) ?? null;
}

function blockedWhy(reason: Reason | undefined, tid: string, source: Value | null): string {
  switch (reason) {
    case "missing_setting":
      return `This setting does not exist in ${tid}.`;
    case "unsupported_value":
      return `${tid} cannot represent ${display(source)}.`;
    case "ambiguous_semantics":
      return "Its meaning in the target is ambiguous, so it cannot be carried.";
    default:
      return "This setting cannot be written to the target.";
  }
}

export function Migrate() {
  const src = usePair();
  const tgt = usePair();

  const [groups, setGroups] = useState<string[]>(["cpu.ratios", "cpu.voltage", "cpu.power", "cpu.loadline", "platform"]);
  const builder = useAction();
  const [result, setResult] = useState<BuildResult | null>(null);
  const [saved, setSaved] = useState("");

  const ready = !!src.report && !!tgt.report;
  const paths: PairPaths = { sourceBios: src.bios, sourceProfile: src.profile, targetBios: tgt.bios, targetProfile: tgt.profile };
  const planner = useMigrationPlan(paths, ready, groups);
  const plan = planner.plan;
  const m = plan?.migration ?? null;
  // While a re-plan is out, the controls show the choices it carries rather than the last answer's.
  const asked = useMemo(() => new Map((planner.pending?.migration?.entries ?? []).map((e) => [e.setting_id, e])), [planner.pending]);
  const chosen = (e: MigrationEntry) => asked.get(e.setting_id) ?? e;

  function toggleGroup(id: string, on: boolean) {
    const next = on ? [...groups, id] : groups.filter((g) => g !== id);
    setGroups(next);
    void planner.change((e) => (e.group === id ? { ...e, selection: on ? "selected" : "unselected" } : e), next);
  }

  function toggleEntry(id: string, on: boolean) {
    void planner.change((e) => (e.setting_id === id ? { ...e, selection: on ? "selected" : "unselected", resolution: { kind: "none" } } : e));
  }

  function resolve(id: string, kind: ResolutionKind, value?: Value) {
    void planner.change((e) => (e.setting_id === id ? { ...e, resolution: value ? { kind, value } : { kind } } : e));
  }

  const targetSetting = (id: string) => settingIn(tgt.report, id);
  const sourceSetting = (id: string) => settingIn(src.report, id);

  // Lay the ledger out like the target BIOS menus; settings missing there fall back to the source's page.
  const located = useMemo<{ e: MigrationEntry; menu: MenuLocation | null }[]>(
    () =>
      m
        ? menuSort(m.entries.map((e) => ({ e, menu: settingIn(tgt.report, e.setting_id)?.menu ?? settingIn(src.report, e.setting_id)?.menu ?? null })))
        : [],
    [m, src.report, tgt.report],
  );
  const nameOf = (id: string, fallback: string) => targetSetting(id)?.menu?.prompt || sourceSetting(id)?.menu?.prompt || fallback;
  /** How the setting's name or page differs between the two BIOS versions. */
  function moved(id: string): string {
    const a = sourceSetting(id)?.menu;
    const b = targetSetting(id)?.menu;
    if (!a || !b || !src.report) return "";
    const rev = src.report.payload.bios.revision;
    const notes = [];
    if (a.prompt !== b.prompt) notes.push(`called “${a.prompt}” in ${rev}`);
    if (pageKey(a.path) !== pageKey(b.path)) notes.push(`on ${pageKey(a.path)} in ${rev}`);
    return notes.join(", ");
  }

  function provenance(e: MigrationEntry): Prov {
    if (e.selection === "unselected") return "none";
    switch (e.final_action) {
      case "transfer":
        // A value equal to the target baseline changes nothing; show it as kept.
        return sameIntent(e.proposed_value, e.target_baseline_value) ? "target" : "source";
      case "set":
        return "replaced";
      case "blocked":
        return "blocked";
      default:
        return "target";
    }
  }

  function status(e: MigrationEntry): { tone: Tone; text: string } {
    if (e.selection === "unselected") return { tone: "neutral", text: "Not selected" };
    switch (e.final_action) {
      case "blocked":
        return { tone: "blocked", text: "Needs a decision" };
      case "set":
        return { tone: "attention", text: "Replaced" };
      case "retain_target":
        return { tone: "neutral", text: "Keeps target" };
      case "transfer":
        return sameIntent(e.source_value, e.target_baseline_value)
          ? { tone: "neutral", text: "Already matches" }
          : { tone: "accent", text: "Carried over" };
      default:
        return { tone: "neutral", text: "" };
    }
  }

  const changing = m?.entries.filter((e) => e.selection === "selected" && (e.final_action === "transfer" || e.final_action === "set") && !sameIntent(e.proposed_value, e.target_baseline_value)).length ?? 0;
  const blocked = m?.accounting.blocked ?? 0;
  const autoCarried = m?.entries.some((e) => e.final_action === "transfer" && e.proposed_value?.kind === "auto" && !sameIntent(e.proposed_value, e.target_baseline_value)) ?? false;

  async function build() {
    if (!plan) return;
    const stem = fileName(src.profile).replace(/\.ocb$/i, "");
    const out = await pickSave(`${stem}-for-${tgt.report?.payload.bios.revision ?? "target"}.ocb`, "ocb");
    if (!out) return;
    await builder.run(async () => setResult(await api.migrateBuild(paths, plan, out)));
  }

  async function savePlan() {
    if (!plan) return;
    const out = await pickSave(`${fileName(src.profile).replace(/\.ocb$/i, "")}.plan.json`, "json");
    if (!out) return;
    try {
      setSaved(await api.savePlan(plan, out));
    } catch (e) {
      builder.setError(errorMessage(e));
    }
  }

  function reset() {
    setResult(null);
    src.setReport(null);
    tgt.setReport(null);
  }

  // The pairs stay at the top while planning and after building, so the result never moves them.
  const strip =
    src.report && tgt.report ? (
      <div className="strip" role="group" aria-label="Migration pairs">
        <span className={`mono strong`}>{src.report.payload.bios.bios_id}</span>
        <span className="quiet">{fileName(src.profile)}</span>
        <span className={s.flow} aria-label="to"><Icon name="arrow" size={16} /></span>
        <span className={`mono strong`}>{tgt.report.payload.bios.bios_id}</span>
        <span className="quiet">{fileName(tgt.profile)}, saved {savedAt(tgt.report.payload.profile.saved_at)}</span>
        <span className="spacer"></span>
        <button className="btn ghost" onClick={reset}>Change files</button>
      </div>
    ) : null;

  if (result) {
    return (
      <>
        {strip}
        <div className="fill scroller">
          <ResultPanel result={result} onDone={reset} />
        </div>
      </>
    );
  }

  if (!src.report || !tgt.report) {
    return (
      <PairSetup
        title="Carry your tuning to another BIOS version"
        intro="Pair the profile you want to keep with the BIOS that saved it. Then open a profile saved by the destination BIOS: it becomes the base of the new file, so anything this tool cannot carry keeps that profile's value."
        slots={[
          { heading: "Carry from", hint: "Your old profile and the BIOS that saved it.", pair: src },
          { heading: "Into", hint: "A profile saved by the new BIOS, ideally right after loading defaults.", pair: tgt },
        ]}
      />
    );
  }

  const tid = tgt.report.payload.bios.bios_id;
  const targetRevision = tgt.report.payload.bios.revision;

  return (
    <>
      {strip}

      <div className="fill col">
        <div className={s.groups} role="group" aria-label="Setting groups to carry">
          <span className="quiet">Carry</span>
          {tgt.report.payload.groups.map((g) => {
            const on = groups.includes(g.id);
            return (
              <label key={g.id} className={cx(s.chip, on && s.on)}>
                <input type="checkbox" checked={on} onChange={(e) => toggleGroup(g.id, e.target.checked)} />
                {g.label}
              </label>
            );
          })}
          {planner.planning && <span className={`quiet ${s.working}`}>Recalculating…</span>}
        </div>

        {planner.error ? (
          <div className={`problem ${s.inset}`} role="alert">
            <Status tone="blocked" text="Cannot plan this migration" />
            <p>{planner.error}</p>
            <p className="quiet">Your choices are kept. Try again once the files can be read.</p>
            <button className="btn" onClick={() => void planner.retry()} disabled={planner.planning}>Try again</button>
          </div>
        ) : m ? (
          <div className="tablewrap">
            <table className={s.ledger} aria-label="Migration plan">
              <colgroup><col style={{ width: "36px" }} /><col style={{ width: "27%" }} /><col style={{ width: "20%" }} /><col style={{ width: "20%" }} /><col style={{ width: "18%" }} /><col /></colgroup>
              <thead>
                <tr>
                  <th className={s.sel}><span className="visually-hidden">Carry</span></th>
                  <th>Setting</th>
                  <th>Source <span className="faint mono">{src.report.payload.bios.revision}</span></th>
                  <th>Target baseline <span className="faint mono">{tgt.report.payload.bios.revision}</span></th>
                  <th>Proposed</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                {located.map((row, i) => {
                  const e = row.e;
                  const st = status(e);
                  const prov = provenance(e);
                  const pageHead = pageBefore(located, i);
                  const section = sectionBefore(located, i);
                  const mv = moved(e.setting_id);
                  const tc = targetSetting(e.setting_id);
                  const a = e.assessment;
                  const proposedTitle = PROVENANCE_TEXT[prov];
                  return (
                    <Fragment key={e.setting_id}>
                      {pageHead && (
                        <tr className={s.pagehead}><th colSpan={6} scope="colgroup">{pageLabel(pageHead, `Not in ${targetRevision}`)}</th></tr>
                      )}
                      {section && (
                        <tr className={s.section}><th colSpan={6} scope="colgroup">{section}</th></tr>
                      )}
                      <tr className={cx(e.selection === "unselected" && s.off, e.final_action === "blocked" && s.blockedrow)}>
                        <td className={s.sel}>
                          <input type="checkbox" aria-label={`Carry ${e.label}`} checked={chosen(e).selection === "selected"} onChange={(ev) => toggleEntry(e.setting_id, ev.target.checked)} />
                        </td>
                        <td className={s.name} title={e.label}>
                          {nameOf(e.setting_id, e.label)}
                          {mv ? <span className={s.moved}><Icon name="arrow" size={11} /> {mv}</span> : null}
                        </td>
                        <td><ValueText value={e.source_value} /></td>
                        <td><ValueText value={e.target_baseline_value} /></td>
                        <td className={cx(s.proposed, s[prov])} title={proposedTitle}>
                          {e.final_action === "blocked" ? <span className="quiet">Waiting for your decision</span> : <ValueText value={e.proposed_value} />}
                        </td>
                        <td><Status tone={st.tone} text={st.text} /></td>
                      </tr>
                      {a && a.status === "unavailable" && e.selection === "selected" && (
                        <tr className={cx(s.decision, e.final_action !== "blocked" && s.resolved)}>
                          <td></td>
                          <td colSpan={5}>
                            <div className={s.decide}>
                              <p className={s.why}>
                                {blockedWhy(a.reason, tid, e.source_value)}
                                {" "}
                                <span className={`${s.quiet} quiet`}>{a.detail}</span>
                              </p>
                              <div className={s.options} role="radiogroup" aria-label={`Decision for ${e.label}`}>
                                {a.permitted_resolutions
                                  .filter((k) => k !== "none" && !(k === "keep_target" && !e.target_baseline_value))
                                  .map((k) => (
                                    <label key={k} className={s.opt}>
                                      <input
                                        type="radio"
                                        name={`res-${e.setting_id}`}
                                        checked={chosen(e).resolution.kind === k}
                                        onChange={() => resolve(e.setting_id, k, k === "set_value" ? (tc?.value ?? undefined) : undefined)}
                                      />
                                      {resolutionText[k]}
                                      {k === "keep_target" && (
                                        <>
                                          {" "}
                                          <span className="quiet">({display(e.target_baseline_value)})</span>
                                        </>
                                      )}
                                    </label>
                                  ))}
                                {chosen(e).resolution.kind === "set_value" && tc && tc.constraints && (
                                  <div className={s.setter}>
                                    <ValueEditor
                                      label={e.label}
                                      value={chosen(e).resolution.value ?? null}
                                      constraints={tc.constraints}
                                      autoOnly={tc.editability.auto_only}
                                      onChange={(v) => resolve(e.setting_id, "set_value", v)}
                                    />
                                  </div>
                                )}
                              </div>
                            </div>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>

            {m.target_only.length > 0 && (
              <>
                <h3 className={s.sub}>Only in {tid}</h3>
                <p className={`quiet ${s.note}`}>These settings have no counterpart in the source. The new profile keeps the target's values.</p>
                <table className={`${s.ledger} ${s.compact}`} aria-label="Settings only in the target">
                  <colgroup><col style={{ width: "36px" }} /><col style={{ width: "27%" }} /><col style={{ width: "20%" }} /><col style={{ width: "20%" }} /><col style={{ width: "18%" }} /><col /></colgroup>
                  <tbody>
                    {m.target_only.map((t) => {
                      const tm = targetSetting(t.setting_id)?.menu;
                      return (
                        <tr key={t.setting_id}>
                          <td className={s.sel}></td>
                          <td>
                            {nameOf(t.setting_id, t.label)}
                            {tm && <span className={s.moved}>{pageKey(tm.path)}</span>}
                          </td>
                          <td colSpan={2} className="quiet">{t.note}</td>
                          <td className={cx(s.proposed, s.target)}><ValueText value={t.retained_value} /></td>
                          <td><Status text="Keeps target" /></td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </>
            )}

            <div className={s.legend} aria-label="Proposed column key">
              <span><i className={`${s.key} ${s.source}`}></i>From the source profile, changes the target value</span>
              <span><i className={`${s.key} ${s.target}`}></i>Target value kept, no change</span>
              <span><i className={`${s.key} ${s.replaced}`}></i>A value you chose</span>
              <span><i className={`${s.key} ${s.blocked}`}></i>Blocked, needs a decision</span>
            </div>
            <p className={`quiet ${s.note}`}>
              {m.accounting.other_differing_fields} other BIOS fields differ between the two profiles. They are outside the reviewed
              settings, so the new file keeps the target's values for all of them.{" "}
              {autoCarried && "Auto is carried as Auto: the new BIOS decides what it means at boot, which may differ from the old one."}
            </p>
          </div>
        ) : null}
      </div>

      <footer className="bar">
        <div className={s.summary}>
          {m && (
            <>
              {blocked > 0 ? (
                <Status tone="blocked" text={`${blocked} ${blocked === 1 ? "setting needs" : "settings need"} a decision`} />
              ) : (
                <Status tone="ok" text="Plan complete" />
              )}
              <span className="quiet">{changing} {changing === 1 ? "value changes" : "values change"} from the target baseline</span>
              {saved && <span className="quiet">Plan saved to <span className="mono">{fileName(saved)}</span></span>}
              {builder.error && <Status tone="blocked" text={builder.error} />}
            </>
          )}
        </div>
        <button className="btn" onClick={savePlan} disabled={!plan || planner.planning}>Save plan</button>
        <button className="btn primary" onClick={build} disabled={!m?.can_build || builder.busy || planner.planning}>
          <SwapLabel busy={builder.busy} idle="Build and verify" working="Building…" />
        </button>
      </footer>
    </>
  );
}
