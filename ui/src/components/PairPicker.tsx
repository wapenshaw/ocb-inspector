import { useImperativeHandle, useState } from "react";
import { api, errorMessage, isCommandError, pickFile } from "../api";
import type { BiosSummary, ExperimentalInspectPayload, InspectPayload, Report } from "../api/types";
import { INTEGRITY_LABEL } from "../lib/analysis";
import { cpuShort, fileName, savedAt, shortHash } from "../lib/format";
import { unverifiedView, type UnverifiedView } from "../lib/unverified";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import { Status } from "./Status";
import s from "./PairPicker.module.css";

export interface PairPickerProps {
  heading: string;
  hint: string;
  bios?: string;
  onBiosChange?: (bios: string) => void;
  profile?: string;
  onProfileChange?: (profile: string) => void;
  report?: Report<InspectPayload> | null;
  onReportChange?: (report: Report<InspectPayload> | null) => void;
  /** Open the chosen BIOS on its own. When set, images that cannot take a profile open directly. */
  onBrowse?: (bios: string) => void;
  /**
   * Opens detailed evidence, baseline comparison and sharing on request. Inspect normally reads
   * all pairs with the BIOS-derived analyzer and presents firmware-set selection inline.
   */
  onAnalyze?: (bios: string, profile: string, set?: number | null) => void;
  legacyReader?: boolean;
  onUnverified?: (view: UnverifiedView) => void;
  /** The unverified pair Inspect shows, for the profile slot. */
  unverified?: UnverifiedView | null;
  ref?: React.Ref<PairPickerHandle>;
}

export interface PairPickerHandle {
  chooseBios(): Promise<void>;
  chooseProfile(): Promise<void>;
}

export function PairPicker({
  heading,
  hint,
  bios = "",
  onBiosChange,
  profile = "",
  onProfileChange,
  report = null,
  onReportChange,
  onBrowse,
  onAnalyze,
  onUnverified,
  legacyReader = false,
  unverified = null,
  ref,
}: PairPickerProps) {
  const uid = heading.toLowerCase().replace(/\W+/g, "-");
  const [summary, setSummary] = useState<BiosSummary | null>(null);
  const [busy, setBusy] = useState<"" | "bios" | "pair">("");
  const [error, setError] = useState<{ text: string; mismatch: boolean; title?: string; unconfirmed?: boolean } | null>(null);
  const [sets, setSets] = useState<string[]>([]);
  const [selectedSet, setSelectedSet] = useState<number | null>(null);
  const profilesOk = summary?.profiles_supported !== false;

  // Each async step receives the values it needs as arguments, because a closure only sees the
  // props of the render it came from.
  async function pair(b: string, pr: string, ok: boolean, sum: BiosSummary | null) {
    onReportChange?.(null);
    if (!b || !pr) return;
    if (onUnverified && !legacyReader && sum) {
      await pairUnverified(b, pr, sum);
      return;
    }
    if (!ok) {
      if (onAnalyze && sum) await pairUnverified(b, pr, sum);
      return;
    }
    setError(null);
    setBusy("pair");
    try {
      const r = await api.inspect(b, pr);
      onReportChange?.(r);
      setSummary(r.payload.bios);
    } catch (e) {
      setError({
        text: errorMessage(e).replace(/^BIOS\/profile identity mismatch: /, ""),
        mismatch: isCommandError(e) && e.code === "identity_mismatch",
        unconfirmed: isCommandError(e) && /checksum|integrity/.test(e.code),
      });
    } finally {
      setBusy("");
    }
  }

  // Read every normal Inspect pair from the BIOS definitions. Only a
  // confirmed identity opens settings; anything else is said here, as for a validated pair.
  async function pairUnverified(b: string, pr: string, sum: BiosSummary, set: number | null = null) {
    setError(null);
    setBusy("pair");
    try {
      const r = await api.analyzeProfile({ profile: pr, bios: b, set });
      const payload = r.payload;
      const availableSets = payload.inputs.bios?.sets ?? [];
      setSets(availableSets);
      setSelectedSet(payload.inputs.bios?.set ?? set);
      const id = payload.identity;
      if (id.status === "mismatch") {
        setError({ text: id.reason, mismatch: true });
      } else if (id.status !== "match") {
        setError({ title: "Could not confirm that this BIOS saved the profile", text: id.reason, mismatch: false, unconfirmed: true });
      } else if (payload.integrity.status === "failed") {
        setError({ title: "Profile integrity check failed", text: [...payload.status_reasons, ...payload.integrity.limitations].join(" ") || "The stored checksum does not match the computed checksum. Settings stay closed.", mismatch: false });
      } else if (availableSets.length > 1 && payload.inputs.bios?.set === null) {
        // Selection stays in the normal Inspect flow.
      } else {
        const view = unverifiedView(payload, sum);
        if (view && onUnverified) onUnverified(view);
        else setError({ title: "No settings could be read", text: analysisDiagnostic(payload), mismatch: false });
      }
    } catch (e) {
      setError({
        text: errorMessage(e).replace(/^BIOS\/profile identity mismatch: /, ""),
        mismatch: isCommandError(e) && e.code === "identity_mismatch",
        unconfirmed: isCommandError(e) && /checksum|integrity/.test(e.code),
      });
    } finally {
      setBusy("");
    }
  }

  async function chooseBios() {
    const p = await pickFile("bios");
    if (!p) return;
    onBiosChange?.(p);
    setSummary(null);
    setSets([]);
    setSelectedSet(null);
    setError(null);
    setBusy("bios");
    let next: BiosSummary | null = null;
    let chosen = p;
    try {
      next = await api.biosSummary(p);
      setSummary(next);
    } catch (e) {
      setError({ text: errorMessage(e), mismatch: false });
      onBiosChange?.("");
      chosen = "";
    } finally {
      setBusy("");
    }
    if (next && next.profiles_supported === false && onBrowse && !onAnalyze) {
      onBrowse(p);
      return;
    }
    await pair(chosen, profile, next?.profiles_supported !== false, next);
  }

  async function chooseProfile() {
    const p = await pickFile("profile");
    if (!p) return;
    onProfileChange?.(p);
    // After "Change files" this picker is new: its BIOS is still chosen, but its summary is not.
    let sum = summary;
    if (onAnalyze && bios && !sum) {
      try {
        sum = await api.biosSummary(bios);
        setSummary(sum);
      } catch (e) {
        setError({ text: errorMessage(e), mismatch: false });
        return;
      }
    }
    await pair(bios, p, sum?.profiles_supported !== false, sum);
  }

  useImperativeHandle(ref, () => ({ chooseBios, chooseProfile }));

  return (
    <section className={s.pair} aria-label={heading}>
      <header>
        <h2>{heading}</h2>
        <p className="quiet">{hint}</p>
      </header>

      <div className={s.slots}>
        <button
          className={cx(s.slot, !!summary && s.filled)}
          onClick={chooseBios}
          disabled={busy !== ""}
          aria-describedby={`${uid}-bios-desc`}
        >
          <span className={s.kind}>
            <Icon name="chip" /> BIOS image
          </span>
          {busy === "bios" ? (
            <span className="quiet">Reading firmware volumes…</span>
          ) : summary ? (
            <>
              <span className={`${s.id} mono`}>{summary.bios_id}</span>
              {summary.platform && (
                <>
                  <span className={s.meta}>
                    {summary.platform.version}, {summary.platform.family}
                    {summary.platform.chipset ? ` ${summary.platform.chipset}` : ""}
                  </span>
                  {summary.platform.models && <span className={`${s.meta} quiet`}>{summary.platform.models}</span>}
                </>
              )}
              <span className={`${s.meta} quiet`} id={`${uid}-bios-desc`}>
                Built {summary.build_date}, <span className="mono">{shortHash(summary.sha256)}</span>
              </span>
              {onUnverified && !legacyReader ? (
                <Status tone="neutral" text="Settings read from this BIOS's own definitions" />
              ) : summary.approved_for_writing ? (
                <Status tone="ok" text="Approved release image" />
              ) : !profilesOk ? (
                <Status tone="neutral" text={onAnalyze ? "Settings read from this BIOS's own definitions" : "Browse only"} />
              ) : (
                <Status tone="attention" text="Not an approved image: inspect only" />
              )}
            </>
          ) : (
            <>
              <span className={s.cta}>Choose BIOS image</span>
              <span className={`${s.meta} quiet`} id={`${uid}-bios-desc`}>
                The vendor release file, e.g. E7D91IMS.HJ0
              </span>
            </>
          )}
        </button>

        <span className={s.join} aria-hidden="true"></span>

        <button
          className={cx(s.slot, (!!report || !!unverified) && s.filled, error?.mismatch && s.bad)}
          onClick={chooseProfile}
          disabled={busy !== "" || (!profilesOk && !onAnalyze)}
        >
          <span className={s.kind}>
            <Icon name="file" /> OC profile
          </span>
          {!profilesOk && !onAnalyze ? (
            <>
              <span className="quiet">Not available for this BIOS</span>
              <span className={`${s.meta} quiet`}>{summary?.platform?.profiles_note ?? ""}</span>
            </>
          ) : busy === "pair" ? (
            <span className="quiet">Checking the pair…</span>
          ) : unverified ? (
            <>
              <span className={s.id}>{fileName(profile)}</span>
              <span className={`${s.meta} quiet`}>
                {unverified.savedAt ? `Saved ${savedAt(unverified.savedAt)}` : ""}
                {unverified.cpuBrand ? `, ${cpuShort(unverified.cpuBrand)}` : ""}
              </span>
              <Status tone="neutral" text="BIOS match confirmed" />
              <span className={`${s.meta} quiet`}>{INTEGRITY_LABEL[unverified.integrity]}</span>
            </>
          ) : report ? (
            <>
              <span className={s.id}>{fileName(profile)}</span>
              <span className={`${s.meta} quiet`}>
                Saved {savedAt(report.payload.profile.saved_at)}, {cpuShort(report.payload.profile.cpu_brand)}
              </span>
              <Status tone="ok" text="Match verified, checksum valid" />
            </>
          ) : profile ? (
            <>
              <span className={s.id}>{fileName(profile)}</span>
              <span className={`${s.meta} quiet`}>{bios ? "" : "Choose the BIOS image that saved it"}</span>
            </>
          ) : (
            <>
              <span className={s.cta}>Choose OC profile</span>
              <span className={`${s.meta} quiet`}>A .ocb file saved from the BIOS</span>
            </>
          )}
        </button>
      </div>

      {summary && onBrowse && !report && (
        <div className={s.browseoffer}>
          <button className="btn primary" onClick={() => onBrowse?.(bios)}>
            <Icon name="chip" /> Browse BIOS settings
          </button>
          <span className="quiet">
            {profilesOk || onAnalyze
              ? "See every setting and its optimized default without a profile."
              : "Profiles cannot be opened for this BIOS, but every setting can be browsed."}
          </span>
        </div>
      )}

      {sets.length > 1 && (
        <label>
          Firmware set
          <select aria-label="Firmware set" value={selectedSet ?? ""} disabled={busy !== ""}
            onChange={(event) => {
              const value = Number(event.target.value);
              if (summary && event.target.value !== "") void pairUnverified(bios, profile, summary, value);
            }}>
            <option value="" disabled>Choose the firmware set used to save this profile</option>
            {sets.map((label, index) => <option key={index} value={index}>{label}</option>)}
          </select>
          <p className="quiet">The image contains several firmware sets. Settings are read only after you choose one.</p>
        </label>
      )}

      {error && (
        <div className={s.error} role="alert">
          <Status tone="blocked" text={error.mismatch ? "Profile and BIOS do not match" : (error.title ?? "Could not open")} />
          <p>{error.text}</p>
          {error.mismatch && (
            <>
              <p className="quiet">Settings stay closed until the profile is paired with the BIOS image that saved it.</p>
              <button className="btn" onClick={chooseBios}>
                Choose matching BIOS
              </button>
            </>
          )}
          {/* Not when the identity is in doubt: the analysis follows the same rule and would read no setting. */}
          {onAnalyze && !error.mismatch && !error.unconfirmed && (
            <button className="btn" onClick={() => onAnalyze(bios, profile, selectedSet)}>
              <Icon name="file" /> Show what can be read from the file
            </button>
          )}
        </div>
      )}
    </section>
  );
}

function analysisDiagnostic(payload: ExperimentalInspectPayload): string {
  return [...payload.status_reasons, ...payload.stores.filter((store) => store.mapping === "unmapped" || store.mapping === "ambiguous").map((store) => `${store.name}: ${store.reason}`)]
    .filter(Boolean).join(" ") || "The BIOS identity matches, but no profile records could be tied to readable BIOS settings. Open the detailed analysis to inspect the container and mapping evidence.";
}
