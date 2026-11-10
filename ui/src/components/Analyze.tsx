// Experimental, read-only analysis of one profile, optionally with the BIOS image that saved it.
// Nothing on this screen can edit, port, build or export a profile. Results are interpretations:
// they are labelled as such, and no verified badge is ever shown here.
//
// Every change of input starts a new numbered run (useAction): an answer, or an error, for an
// earlier run is dropped, and Save is only enabled for the result shown for the current inputs.
import { useImperativeHandle, useMemo, useRef, useState, type ReactNode, type Ref } from "react";
import { api, pickFile, pickSave } from "../api";
import type { AnalysisRequest, ExperimentalInspectPayload, Report } from "../api/types";
import { useAction } from "../lib/useAction";
import { fileName } from "../lib/format";
import { cx } from "../lib/cx";
import {
  EMPTY_LOOKUP,
  baselineApplied,
  crossNotCompared,
  defaultCounts,
  type DefaultCounts,
  failureOf,
  fieldItems,
  initialDefaultsFilter,
  lookupOf,
  sameRequest,
} from "../lib/analysis";
import { AnalyzeRecords } from "./AnalyzeRecords";
import { AnalyzeSettings, DEFAULT_FILTER, type SettingsFilter } from "./AnalyzeSettings";
import { AnalyzeSummary } from "./AnalyzeSummary";
import { ProfileFormat } from "./ProfileFormat";
import { Icon } from "./Icon";
import { SegGroup } from "./SegGroup";
import { Status } from "./Status";
import s from "./Analyze.module.css";

export interface AnalyzeHandle {
  /** Start an analysis with these paths (empty strings mean "not chosen"). */
  open(bios: string, profile: string): void;
}

type View = "summary" | "settings" | "records";
const VIEWS: { value: View; label: string }[] = [
  { value: "summary", label: "Summary" },
  { value: "settings", label: "Settings" },
  { value: "records", label: "Records" },
];

interface Inputs {
  profile: string;
  bios: string;
  set: number | null;
  /** Optional second profile, saved with the BIOS at its defaults. */
  baseline: string;
  /** Optional: the BIOS the baseline was saved on, when that is another version of the same board. */
  baselineBios: string;
}
const NONE: Inputs = { profile: "", bios: "", set: null, baseline: "", baselineBios: "" };

const BASELINE_HINT =
  "A profile saved with the BIOS at its defaults: on this same BIOS version, or on a newer one right after an update.";
const BASELINE_BIOS_HINT =
  "Only if the baseline was saved on another BIOS version of the same board, for example right after an update. Settings are then matched by menu place and name.";
const SHAREABLE_NOTE = "Leaves out the CPU name, the save time and raw record bytes.";

// The baseline keys are only present when a baseline is chosen, and only with a BIOS (it needs one).
// The baseline's firmware set is not offered: the engine picks it when that BIOS has one set.
const toRequest = (i: Inputs): AnalysisRequest => ({
  profile: i.profile,
  bios: i.bios || null,
  set: i.set,
  ...(i.bios && i.baseline ? { baseline: i.baseline, ...(i.baselineBios ? { baselineBios: i.baselineBios } : {}) } : {}),
});
const toInputs = (r: AnalysisRequest): Inputs => ({
  profile: r.profile,
  bios: r.bios ?? "",
  set: r.set,
  baseline: r.baseline ?? "",
  baselineBios: r.baselineBios ?? "",
});

interface Outcome {
  req: AnalysisRequest;
  report: Report<ExperimentalInspectPayload>;
  /** Counted once when the report arrives; the Summary and Settings views and the first filter use it. */
  counts: DefaultCounts;
}
interface Failure {
  req: AnalysisRequest;
  title: string;
  message: string;
}

/** A one-line notice above the results; the full text stays in the tooltip because it is ellipsised. */
function Note({ lead, text, className = s.note }: { lead: ReactNode; text: string; className?: string }) {
  return (
    <div className={className} role="note">
      {lead}
      <p title={text}>{text}</p>
    </div>
  );
}

interface OptionalFileProps {
  label: string;
  /** Tooltip on the label (and on the add button) saying when this file is wanted. */
  hint: string;
  /** Chosen file path, "" when none. */
  path: string;
  /** What to show for the chosen file. */
  name: string;
  mono?: boolean;
  addText: string;
  changeText: string;
  removeText: string;
  onChoose: () => void | Promise<void>;
  onRemove: () => void;
  /** Start-screen layout: the name sits in the label text and one button adds or changes. */
  inline?: boolean;
  /** Extra controls after the file's own buttons. */
  children?: ReactNode;
}

function OptionalFile({ label, hint, path, name, mono, addText, changeText, removeText, onChoose, onRemove, inline, children }: OptionalFileProps) {
  const choose = () => void onChoose();
  if (inline) {
    return (
      <>
        <span className={`${s.meta} quiet`} title={hint}>
          {label}
          {path ? `: ${name}` : ""}
        </span>
        <button className="btn ghost" onClick={choose} title={hint}>
          {path ? changeText : addText}
        </button>
        {path && (
          <button className="btn ghost" onClick={onRemove}>
            {removeText}
          </button>
        )}
        {children}
      </>
    );
  }
  return (
    <>
      <span className={cx("quiet", s.nowrap)} title={hint}>{label}</span>
      {path ? (
        <>
          <span className={cx(s.name, mono && "mono")} title={path}>{name}</span>
          <button className="btn ghost" onClick={choose}>{changeText}</button>
          <button className="btn ghost" onClick={onRemove}>{removeText}</button>
        </>
      ) : (
        <button className="btn ghost" onClick={choose} title={hint}>
          {addText}
        </button>
      )}
      {children}
    </>
  );
}

export function Analyze({ ref }: { ref?: Ref<AnalyzeHandle> }) {
  const [inputs, setInputs] = useState<Inputs>(NONE);
  // Latest inputs for handlers that await a file dialog first.
  const inputsRef = useRef<Inputs>(NONE);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [failure, setFailure] = useState<Failure | null>(null);
  // Firmware sets seen for the current BIOS. Kept apart from the result, so the selector stays
  // while a chosen set is being analyzed.
  const [knownSets, setKnownSets] = useState<string[]>([]);
  const [view, setView] = useState<View>("summary");
  const [filter, setFilter] = useState<SettingsFilter>(DEFAULT_FILTER);
  const [selected, setSelected] = useState<string | null>(null);
  const [saved, setSaved] = useState("");
  // BIOS only: what this BIOS writes into a profile (no profile is opened).
  const [format, setFormat] = useState(false);
  const analysis = useAction();
  const saver = useAction();

  // Any change of input: earlier answers are dropped, and a new run starts for a profile.
  function start(next: Inputs) {
    if (next.bios !== inputsRef.current.bios) setKnownSets([]);
    inputsRef.current = next;
    setInputs(next);
    setOutcome(null);
    setFailure(null);
    setSaved("");
    setSelected(null);
    saver.cancel();
    if (!next.profile) {
      analysis.cancel();
      return;
    }
    // The snapshot is passed in, so a retry can send exactly the same request.
    const r = toRequest(next);
    void analysis.run(async (current) => {
      try {
        const report = await api.analyzeProfile(r);
        if (!current()) return;
        const counts = defaultCounts(report.payload.fields);
        setOutcome({ req: r, report, counts });
        // Each new result starts on what differs from the BIOS default, if anything does.
        const defaults = initialDefaultsFilter(counts, baselineApplied(report.payload));
        setFilter((f) => ({ ...f, defaults }));
        const sets = report.payload.inputs.bios?.sets ?? [];
        if (sets.length > 1) setKnownSets(sets);
      } catch (e) {
        if (current()) setFailure({ req: r, ...failureOf(e) });
      }
    });
  }

  useImperativeHandle(ref, () => ({
    open(bios, profile) {
      setView("summary");
      setFormat(false);
      start({ profile, bios, set: null, baseline: "", baselineBios: "" });
    },
  }));

  async function chooseProfile() {
    const p = await pickFile("profile");
    if (p) {
      setFormat(false);
      start({ ...inputsRef.current, profile: p });
    }
  }
  async function choose(kind: "profile" | "bios", patch: (path: string) => Partial<Inputs>) {
    const p = await pickFile(kind);
    if (p) start({ ...inputsRef.current, ...patch(p) });
  }
  function clear(patch: Partial<Inputs>) {
    start({ ...inputsRef.current, ...patch });
  }
  // The baseline and its BIOS stay when the BIOS changes: the engine says "blocked" if the baseline no longer fits.
  const chooseBios = () => choose("bios", (bios) => ({ bios, set: null }));
  const chooseBaseline = () => choose("profile", (baseline) => ({ baseline }));
  const chooseBaselineBios = () => choose("bios", (baselineBios) => ({ baselineBios }));
  const removeBios = () => clear({ bios: "", set: null, baseline: "", baselineBios: "" });
  const removeBaseline = () => clear({ baseline: "", baselineBios: "" });
  const removeBaselineBios = () => clear({ baselineBios: "" });
  function chooseSet(v: string) {
    start({ ...inputsRef.current, set: Number(v) });
  }
  function startOver() {
    start(NONE);
  }
  function retry() {
    if (failure) start(toInputs(failure.req));
  }

  // The result shown belongs to these inputs only; anything else cannot be saved.
  const shown = outcome && sameRequest(toRequest(inputs), outcome.req) ? outcome : null;
  const payload = shown?.report.payload ?? null;
  const canSave = !!shown && shown.report.payload.capabilities.analysis_export && !analysis.busy && !saver.busy;

  function save(o: Outcome, shareable: boolean) {
    const stem = fileName(o.req.profile).replace(/\.ocb$/i, "") || "profile";
    const suffix = shareable ? ".shareable" : "";
    const title = shareable ? "Save shareable report" : "Save analysis";
    // The flag is sent only when set, so a plain save makes the same call as before.
    const flag: [] | [true] = shareable ? [true] : [];
    void saver.run(async (still) => {
      const out = await pickSave(`${stem}${suffix}.analysis.json`, "json", title);
      if (!out || !still()) return;
      const path = await api.saveAnalysis(o.req, o.report.payload.inputs, out, ...flag);
      if (still()) setSaved(path);
    });
  }

  const items = useMemo(() => (payload ? fieldItems(payload) : []), [payload]);
  const lookup = useMemo(() => (payload ? lookupOf(payload) : EMPTY_LOOKUP), [payload]);

  if (!inputs.profile && format && inputs.bios) {
    return <ProfileFormat bios={inputs.bios} onChooseBios={() => void chooseBios()} onClose={() => setFormat(false)} />;
  }

  if (!inputs.profile) {
    return (
      <div className="fill scroller">
        <div className={s.start}>
          <div className={s.intro}>
            <h1>Analyze a profile experimentally</h1>
            <p className="quiet">
              Reads the profile's bytes and, with the BIOS image that saved it, tries to name each setting. Nothing is changed, and this
              screen cannot write a profile. Results are interpretations, not facts about what the BIOS will do.
            </p>
          </div>
          <div className={s.slots}>
            <button className={s.slot} onClick={() => void chooseProfile()}>
              <span className={s.kind}>
                <Icon name="file" /> OC profile
              </span>
              <span className={s.cta}>Choose OC profile</span>
              <span className={`${s.meta} quiet`}>Required. Any .ocb file, including one the checked workflow cannot open.</span>
            </button>
            <button className={cx(s.slot, !!inputs.bios && s.filled)} onClick={() => void chooseBios()}>
              <span className={s.kind}>
                <Icon name="chip" /> BIOS image
              </span>
              {inputs.bios ? (
                <>
                  <span className="mono">{fileName(inputs.bios)}</span>
                  <span className={`${s.meta} quiet`}>Choose the profile to analyze with it, or show its profile format below.</span>
                </>
              ) : (
                <>
                  <span className={s.cta}>Add BIOS image</span>
                  <span className={`${s.meta} quiet`}>Optional. Without it, only the file's structure is shown.</span>
                </>
              )}
            </button>
            {inputs.bios && (
              <button className={cx(s.slot, !!inputs.baseline && s.filled)} onClick={() => void chooseBaseline()}>
                <span className={s.kind}>
                  <Icon name="file" /> Baseline profile (optional)
                </span>
                {inputs.baseline ? (
                  <span className="mono">{fileName(inputs.baseline)}</span>
                ) : (
                  <span className={s.cta}>Add baseline profile</span>
                )}
                <span className={`${s.meta} quiet`}>{BASELINE_HINT}</span>
              </button>
            )}
          </div>
          {inputs.baseline && (
            <div className={s.alt}>
              <OptionalFile
                inline
                label="Baseline BIOS (optional)"
                hint={BASELINE_BIOS_HINT}
                path={inputs.baselineBios}
                name={fileName(inputs.baselineBios)}
                addText="Add baseline BIOS"
                changeText="Change baseline BIOS"
                removeText="Remove baseline BIOS"
                onChoose={chooseBaselineBios}
                onRemove={removeBaselineBios}
              >
                <button className="btn ghost" onClick={removeBaseline}>Remove baseline</button>
              </OptionalFile>
            </div>
          )}
          {inputs.bios && (
            <div className={s.alt}>
              <p className="quiet">No profile from this BIOS at hand? You can still see what it writes into one.</p>
              <button className="btn" onClick={() => setFormat(true)}>
                Show profile format
              </button>
            </div>
          )}
        </div>
      </div>
    );
  }

  const bios = payload?.inputs.bios ?? null;
  const needsSet = knownSets.length > 1 && bios !== null && bios.set === null;
  const emptyReason = !bios
    ? "Add the BIOS image that saved this profile to see its settings."
    : needsSet
      ? "Choose a firmware set to see its settings."
      : "No setting could be interpreted. The reasons above say why.";

  return (
    <>
      <div className={`strip ${s.inputs}`} role="group" aria-label="Files analyzed">
        <span className="quiet">Profile</span>
        <span className={s.name} title={inputs.profile}>{fileName(inputs.profile)}</span>
        <button className="btn ghost" onClick={() => void chooseProfile()}>Change profile</button>
        <span className={s.sep} aria-hidden="true" />
        {inputs.bios ? (
          <>
            <span className="quiet">BIOS</span>
            <span className={`${s.name} mono`} title={inputs.bios}>{bios?.bios_id ?? fileName(inputs.bios)}</span>
            <button className="btn ghost" onClick={() => void chooseBios()}>Change BIOS</button>
            <button className="btn ghost" onClick={removeBios}>Remove BIOS</button>
          </>
        ) : (
          <button className="btn ghost" onClick={() => void chooseBios()}>Add BIOS image</button>
        )}
        {knownSets.length > 1 && (
          <label className={s.setpick}>
            Firmware set
            <select value={inputs.set ?? ""} onChange={(e) => chooseSet(e.target.value)}>
              {inputs.set === null && (
                <option value="" disabled>
                  Choose a set
                </option>
              )}
              {knownSets.map((label, i) => (
                <option key={i} value={i}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        )}
        <span className="spacer" />
        <button className="btn ghost" onClick={startOver}>Start over</button>
        <button
          className="btn primary"
          disabled={!canSave}
          title={canSave ? "Save this analysis as a JSON report" : "Available once the analysis of these exact files is shown"}
          onClick={() => shown && save(shown, false)}
        >
          Save analysis
        </button>
      </div>

      <div className={`strip ${s.inputs}`} role="group" aria-label="Optional inputs and sharing">
        {inputs.bios ? (
          <OptionalFile
            label="Baseline profile (optional)"
            hint={BASELINE_HINT}
            path={inputs.baseline}
            name={fileName(inputs.baseline)}
            addText="Add baseline profile"
            changeText="Change baseline"
            removeText="Remove baseline"
            onChoose={chooseBaseline}
            onRemove={removeBaseline}
          />
        ) : null}
        <span className={cx("quiet", s.hint, s.right)} id="shareable-note" title={SHAREABLE_NOTE}>{SHAREABLE_NOTE}</span>
        <button
          className="btn"
          disabled={!canSave}
          aria-describedby="shareable-note"
          title={canSave ? "Save a JSON report that is safer to share" : "Available once the analysis of these exact files is shown"}
          onClick={() => shown && save(shown, true)}
        >
          Save shareable report
        </button>
      </div>

      {inputs.bios && inputs.baseline && (
        <div className={`strip ${s.inputs}`} role="group" aria-label="Baseline BIOS">
          <OptionalFile
            label="Baseline BIOS (optional)"
            hint={BASELINE_BIOS_HINT}
            path={inputs.baselineBios}
            name={payload?.inputs.baseline?.bios?.bios_id ?? fileName(inputs.baselineBios)}
            mono
            addText="Add baseline BIOS"
            changeText="Change baseline BIOS"
            removeText="Remove baseline BIOS"
            onChoose={chooseBaselineBios}
            onRemove={removeBaselineBios}
          >
            <span className={cx("quiet", s.hint)} title={BASELINE_BIOS_HINT}>{BASELINE_BIOS_HINT}</span>
          </OptionalFile>
        </div>
      )}

      <Note
        className={s.banner}
        lead={<Icon name="alert" size={15} />}
        text="Experimental, read-only interpretation. Some record mappings are inferred. Nothing here can change a profile or a BIOS image."
      />

      {payload && (
        <div className={s.head}>
          {payload.inputs.baseline?.status === "blocked" && (
            <Note
              lead={<Status text="Baseline profile not used" />}
              text={`${payload.inputs.baseline.reason} Settings are compared with the BIOS image's defaults only.`}
            />
          )}

          {payload.integrity.status === "unknown" && payload.fields.length > 0 && (
            <Note
              lead={<Status tone="attention" text="Integrity not established" />}
              text="The checksum was not checked against a known rule, so these values are not confirmed as what the BIOS stores."
            />
          )}

          {knownSets.length > 1 && (
            <Note
              lead={<Status tone="attention" text={needsSet ? "No firmware set chosen" : "Firmware set chosen"} />}
              text={`${
                needsSet
                  ? `This BIOS has ${knownSets.length} firmware sets. Choose the one the profile may come from; settings are not interpreted until you do.`
                  : "Settings are read against the chosen set."
              } The choice is a hypothesis, not proof that this set saved the profile.`}
            />
          )}

          {saver.error && (
            <div className={`problem ${s.inset}`} role="alert">
              <Status tone="blocked" text="Could not save the analysis" />
              <p>{saver.error}</p>
            </div>
          )}
          {saved && (
            <p className={`quiet ${s.meta}`} role="status">
              Saved to <span className="mono">{saved}</span>
            </p>
          )}

          <div className={s.tabs}>
            <SegGroup label="Result view" name="analyze-view" options={VIEWS} value={view} onChange={setView} />
          </div>
        </div>
      )}

      <div className={`fill ${s.body}`}>
        {analysis.busy && !shown && (
          <p className={`quiet ${s.waiting}`} role="status">
            Analyzing…
          </p>
        )}
        {failure && !analysis.busy && (
          <div className={`problem ${s.inset}`} role="alert">
            <Status tone="blocked" text={failure.title} />
            <p>{failure.message}</p>
            <button className="btn" onClick={retry}>
              Try again
            </button>
          </div>
        )}
        {shown && payload && view === "summary" && (
          <AnalyzeSummary
            report={shown.report}
            payload={payload}
            counts={shown.counts}
            onShowDiffering={() => {
              setFilter((f) => ({ ...f, defaults: "differs" }));
              setView("settings");
            }}
            onShowBaselineDiffering={(defaults) => {
              setFilter((f) => ({ ...f, defaults }));
              setView("settings");
            }}
          />
        )}
        {shown && payload && view === "settings" && (
          <AnalyzeSettings
            items={items}
            lookup={lookup}
            filter={filter}
            onFilter={(patch) => setFilter((f) => ({ ...f, ...patch }))}
            selected={selected}
            onSelect={setSelected}
            emptyReason={emptyReason}
            counts={shown.counts}
            baseline={baselineApplied(payload) ? { crossNotCompared: crossNotCompared(payload) } : null}
          />
        )}
        {shown && payload && view === "records" && <AnalyzeRecords payload={payload} lookup={lookup} />}
      </div>
    </>
  );
}
