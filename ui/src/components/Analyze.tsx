// The automatic fallback of Inspect: a read-only reading of one profile with the BIOS image that
// saved it, used when the checked reader does not cover the board (or on request). Files are chosen
// in Inspect's picker; this view only receives them through `open`. Nothing here can edit, port,
// build or export a profile. Results are interpretations: they are labelled as such, and no
// verified badge is ever shown here.
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
  /** `fromProfile`: opened from a pair Inspect already shows, so closing goes back to it. */
  open(bios: string, profile: string, fromProfile?: boolean, set?: number | null): void;
}

interface AnalyzeProps {
  ref?: Ref<AnalyzeHandle>;
  /** Back to the picker, to choose other files. */
  onClose: () => void;
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
const MISMATCH_TITLE = "Profile and BIOS do not match";

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
  /** Extra controls after the file's own buttons. */
  children?: ReactNode;
}

function OptionalFile({ label, hint, path, name, mono, addText, changeText, removeText, onChoose, onRemove, children }: OptionalFileProps) {
  const choose = () => void onChoose();
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

export function Analyze({ ref, onClose }: AnalyzeProps) {
  const [inputs, setInputs] = useState<Inputs>(NONE);
  // Latest inputs for handlers that await a file dialog first.
  const inputsRef = useRef<Inputs>(NONE);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [failure, setFailure] = useState<Failure | null>(null);
  // Firmware sets seen for the current BIOS. Kept apart from the result, so the selector stays
  // while a chosen set is being analyzed.
  const [knownSets, setKnownSets] = useState<string[]>([]);
  const [fromProfile, setFromProfile] = useState(false);
  const [view, setView] = useState<View>("settings");
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
    open(bios, profile, fromProfile = false, set = null) {
      setFromProfile(fromProfile);
      setView("settings");
      setFormat(false);
      // A new pair starts clean: sets seen for an earlier profile are not offered again.
      setKnownSets([]);
      start({ profile, bios, set, baseline: "", baselineBios: "" });
    },
  }));

  async function choose(kind: "profile" | "bios", patch: (path: string) => Partial<Inputs>) {
    const p = await pickFile(kind);
    if (p) start({ ...inputsRef.current, ...patch(p) });
  }
  function clear(patch: Partial<Inputs>) {
    start({ ...inputsRef.current, ...patch });
  }
  // The baseline and its BIOS stay when the BIOS changes: the engine says "blocked" if the baseline no longer fits.
  const chooseBaseline = () => choose("profile", (baseline) => ({ baseline }));
  const chooseBaselineBios = () => choose("bios", (baselineBios) => ({ baselineBios }));
  const removeBaseline = () => clear({ baseline: "", baselineBios: "" });
  const removeBaselineBios = () => clear({ baselineBios: "" });
  function chooseSet(v: string) {
    start({ ...inputsRef.current, set: Number(v) });
  }
  function close() {
    setFormat(false);
    start(NONE);
    onClose();
  }
  function retry() {
    if (failure) start(toInputs(failure.req));
  }

  // The result shown belongs to these inputs only; anything else cannot be saved.
  const shown = outcome && sameRequest(toRequest(inputs), outcome.req) ? outcome : null;
  const payload = shown?.report.payload ?? null;
  const canSave = !!shown && shown.report.payload.identity.status !== "mismatch" && shown.report.payload.capabilities.analysis_export && !analysis.busy && !saver.busy;

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

  // Files are chosen in the picker; without a profile there is nothing to show.
  if (!inputs.profile) return null;

  if (format && inputs.bios) {
    return <ProfileFormat bios={inputs.bios} closeText="Back to results" onClose={() => setFormat(false)} />;
  }

  const bios = payload?.inputs.bios ?? null;
  const needsSet = knownSets.length > 1 && bios !== null && bios.set === null;
  // The gate only applies when a BIOS was supplied; the engine may report it or refuse the request.
  const identity = inputs.bios ? (payload?.identity ?? null) : null;
  const unconfirmed = identity !== null && (identity.status === "unknown" || identity.status === "conflicting");
  const mismatch =
    identity?.status === "mismatch"
      ? identity.reason
      : inputs.bios && failure && !analysis.busy && failure.title === MISMATCH_TITLE
        ? failure.message.replace(/^BIOS\/profile identity mismatch: /, "")
        : null;
  const emptyReason = !bios
    ? "Choose the BIOS image that saved this profile to see its settings."
    : needsSet
      ? "Choose a firmware set to see its settings."
      : unconfirmed
        ? "Settings are read only once this BIOS is confirmed as the one that saved the profile."
        : "No setting could be interpreted. The reasons above say why.";

  const filesStrip = (
    <div className={`strip ${s.inputs}`} role="group" aria-label="Files read">
      <span className="quiet">Profile</span>
      <span className={s.name} title={inputs.profile}>{fileName(inputs.profile)}</span>
      {inputs.bios && (
        <>
          <span className={s.sep} aria-hidden="true" />
          <span className="quiet">BIOS</span>
          <span className={`${s.name} mono`} title={inputs.bios}>{bios?.bios_id ?? fileName(inputs.bios)}</span>
        </>
      )}
      {mismatch === null && knownSets.length > 1 && (
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
      {inputs.bios && mismatch === null && (
        <button className="btn ghost" onClick={() => setFormat(true)} title="What this BIOS writes into an OC profile, read from the image alone">
          Profile format
        </button>
      )}
      <button className="btn ghost" onClick={close}>{fromProfile ? "Back to profile" : "Change files"}</button>
      <button
        className="btn primary"
        disabled={!canSave}
        title={canSave ? "Save this analysis as a JSON report" : "Available once the analysis of these exact files is shown"}
        onClick={() => shown && save(shown, false)}
      >
        Save analysis
      </button>
    </div>
  );

  if (mismatch !== null) {
    return (
      <>
        {filesStrip}
        <div className={`problem ${s.inset}`} role="alert">
          <Status tone="blocked" text="Profile and BIOS do not match" />
          <p>{mismatch}</p>
          <p className="quiet">Settings stay closed until the profile is paired with the BIOS image that saved it.</p>
          <button className="btn" onClick={close}>
            Choose matching BIOS
          </button>
        </div>
      </>
    );
  }

  return (
    <>
      {filesStrip}

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
        text={
          fromProfile
            ? "Every setting, read from the BIOS's own definitions and shown as the BIOS labels them. Mappings marked inferred are not verified. Nothing here can change a profile or a BIOS image."
            : "Not verified on this board: settings are read from the BIOS's own definitions, shown as the BIOS labels them. Nothing here can change a profile or a BIOS image."
        }
      />

      {payload && (
        <div className={s.head}>
          {payload.inputs.baseline?.status === "blocked" && (
            <Note
              lead={<Status text="Baseline profile not used" />}
              text={`${payload.inputs.baseline.reason} Settings are compared with the BIOS image's defaults only.`}
            />
          )}

          {unconfirmed && identity && (
            <Note lead={<Status tone="attention" text="Could not confirm that this BIOS saved the profile" />} text={identity.reason} />
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
