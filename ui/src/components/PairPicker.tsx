import { useImperativeHandle, useState } from "react";
import { api, errorMessage, isCommandError, pickFile } from "../api";
import type { BiosSummary, InspectPayload, Report } from "../api/types";
import { cpuShort, fileName, savedAt, shortHash } from "../lib/format";
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
  /** Open the experimental analysis with these paths (empty strings mean not chosen). Shown only when set. */
  onAnalyze?: (bios: string, profile: string) => void;
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
  ref,
}: PairPickerProps) {
  const uid = heading.toLowerCase().replace(/\W+/g, "-");
  const [summary, setSummary] = useState<BiosSummary | null>(null);
  const [busy, setBusy] = useState<"" | "bios" | "pair">("");
  const [error, setError] = useState<{ text: string; mismatch: boolean } | null>(null);
  const profilesOk = summary?.profiles_supported !== false;

  // Each async step receives the values it needs as arguments, because a closure only sees the
  // props of the render it came from.
  async function pair(b: string, pr: string, ok: boolean) {
    onReportChange?.(null);
    if (!b || !pr || !ok) return;
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
    if (next && next.profiles_supported === false && onBrowse) {
      onBrowse(p);
      return;
    }
    await pair(chosen, profile, next?.profiles_supported !== false);
  }

  async function chooseProfile() {
    const p = await pickFile("profile");
    if (!p) return;
    onProfileChange?.(p);
    await pair(bios, p, profilesOk);
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
              {summary.approved_for_writing ? (
                <Status tone="ok" text="Approved release image" />
              ) : !profilesOk ? (
                <Status tone="neutral" text="Browse only" />
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
          className={cx(s.slot, !!report && s.filled, error?.mismatch && s.bad)}
          onClick={chooseProfile}
          disabled={busy !== "" || !profilesOk}
        >
          <span className={s.kind}>
            <Icon name="file" /> OC profile
          </span>
          {!profilesOk ? (
            <>
              <span className="quiet">Not available for this BIOS</span>
              <span className={`${s.meta} quiet`}>{summary?.platform?.profiles_note ?? ""}</span>
            </>
          ) : busy === "pair" ? (
            <span className="quiet">Checking the pair…</span>
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
            {profilesOk
              ? "See every setting and its optimized default without a profile."
              : "Profiles cannot be opened for this BIOS, but every setting can be browsed."}
          </span>
        </div>
      )}

      {error && (
        <div className={s.error} role="alert">
          <Status tone="blocked" text={error.mismatch ? "Profile and BIOS do not match" : "Could not open"} />
          <p>{error.text}</p>
          {error.mismatch && (
            <>
              <p className="quiet">Settings stay closed until the profile is paired with the BIOS image that saved it.</p>
              <button className="btn" onClick={chooseBios}>
                Choose matching BIOS
              </button>
            </>
          )}
          {/* Not on a mismatch: the analysis follows the same identity rule and would read no setting. */}
          {onAnalyze && !error.mismatch && (
            <button className="btn" onClick={() => onAnalyze(bios, profile)}>
              <Icon name="file" /> Analyze profile experimentally
            </button>
          )}
        </div>
      )}

      {onAnalyze && !report && !error && (
        <div className={s.analyze}>
          <button className="btn" onClick={() => onAnalyze(bios, profile)}>
            <Icon name="file" /> Analyze profile experimentally
          </button>
          <span className="quiet">Read-only and experimental. Works with any profile, including ones the checked workflow cannot open.</span>
        </div>
      )}
    </section>
  );
}
