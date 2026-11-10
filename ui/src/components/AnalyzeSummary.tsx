// Summary view of the experimental analysis: identity, integrity, the container candidates with
// their checks, coverage, diagnostics and what this analysis can and cannot do.
import type { ReactNode } from "react";
import { api } from "../api";
import type { ExperimentalInspectPayload, Report } from "../api/types";
import { hex, shortHash } from "../lib/format";
import {
  CHECK_LABEL,
  CHECK_TONE,
  CONTAINER_LABEL,
  baselineApplied,
  baselineSplit,
  baselineSummary,
  type DefaultCounts,
  IDENTITY_LABEL,
  IDENTITY_TONE,
  INTEGRITY_LABEL,
  INTEGRITY_TONE,
  rangeText,
} from "../lib/analysis";
import { Status } from "./Status";
import s from "./Analyze.module.css";

const SUPPORT_URL = "https://github.com/wapenshaw/ocb-inspector/issues/new?template=profile_report.yml";

function Panel({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className={s.panel}>
      <h2>{title}</h2>
      {children}
    </section>
  );
}

function Limits({ items }: { items: string[] }) {
  if (items.length === 0) return null;
  return (
    <ul className={s.limits}>
      {items.map((x, i) => (
        <li key={i}>{x}</li>
      ))}
    </ul>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <>
      <dt>{label}</dt>
      <dd className="mono">{value}</dd>
    </>
  );
}

interface SummaryProps {
  report: Report<ExperimentalInspectPayload>;
  payload: ExperimentalInspectPayload;
  /** Counts over every field of the report, computed once by the caller. */
  counts: DefaultCounts;
  /** Switch to the Settings view with the "differs from default" filter on. */
  onShowDiffering: () => void;
  /** Switch to the Settings view with this baseline filter on. */
  onShowBaselineDiffering: (filter: "baseline" | "baseline_at" | "baseline_off") => void;
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

export function AnalyzeSummary({ report, payload, counts, onShowDiffering, onShowBaselineDiffering }: SummaryProps) {
  const { identity, integrity, container, container_candidates, selected_candidate, coverage, analysis } = payload;
  const applied = baselineApplied(payload);
  const baseline = applied ? baselineSummary(payload, counts) : null;
  const split = applied ? baselineSplit(payload, counts) : null;
  const bios = payload.inputs.bios;
  return (
    <div className={`fill scroller ${s.scrollpad}`}>
      <section className={s.panel} aria-label="This analysis">
        <dl className={s.facts}>
          <Fact label="Profile">
            <span className="mono" title={payload.inputs.profile_sha256}>{shortHash(payload.inputs.profile_sha256)}</span>
          </Fact>
          <Fact label="BIOS image">
            {bios ? (
              <>
                <span className="mono">{bios.bios_id}</span> <span className="mono faint">{shortHash(bios.sha256)}</span>
              </>
            ) : (
              "None: file structure only"
            )}
          </Fact>
          <Fact label="Firmware set">
            {!bios
              ? "—"
              : bios.sets.length === 0
                ? "Only set"
                : bios.set === null
                  ? "None chosen"
                  : (bios.sets[bios.set] ?? `Set ${bios.set + 1}`)}
          </Fact>
          <Fact label="Identifier in profile">
            <span className="mono">{payload.identity.profile_bios_id ?? "Not found"}</span>
          </Fact>
          <Fact label="Identity">{IDENTITY_LABEL[identity.status]}</Fact>
          <Fact label="Integrity">{INTEGRITY_LABEL[integrity.status]}</Fact>
          <Fact label="Layout">{CONTAINER_LABEL[container]}</Fact>
          <Fact label="Coverage">
            <span className="mono">{coverage.fields_decoded}</span> of <span className="mono">{coverage.fields_total}</span> settings decoded,{" "}
            <span className="mono">{coverage.records_confirmed + coverage.records_inferred}</span> of{" "}
            <span className="mono">{coverage.records_total}</span> records mapped
          </Fact>
          <Fact label="Analyzer">
            <span className="mono">
              {analysis.analyzer} {analysis.version}
            </span>
          </Fact>
        </dl>

        <div className={s.reasons}>
          <strong>Why the result looks like this</strong>
          <ul>
            {payload.status_reasons.map((r, i) => (
              <li key={i}>{r}</li>
            ))}
          </ul>
        </div>

        <div className={s.help}>
          <span className="quiet">Sharing is your choice: nothing is sent automatically.</span>
          <button className="btn ghost" onClick={() => void api.openPage(SUPPORT_URL)}>
            Help improve support
          </button>
        </div>
      </section>

      {payload.fields.length > 0 && (
        <Panel title="Compared with the BIOS optimized default">
          <button className={s.linkline} onClick={onShowDiffering}>
            {counts.differs} settings differ from the BIOS optimized default; {counts.noDefault} have no optimized default and cannot be
            judged.
          </button>
        </Panel>
      )}

      {baseline && (
        <Panel title="Compared with the baseline profile">
          <button className={s.linkline} onClick={() => onShowBaselineDiffering("baseline")}>
            {baseline.differing} of {baseline.compared} compared settings differ from the baseline
          </button>
          {split && baseline.differing > 0 && (
            <>
              <button className={s.linkline} onClick={() => onShowBaselineDiffering("baseline_at")}>
                {split.atDefault} likely your settings (the baseline is at its BIOS default there)
              </button>
              <button className={s.linkline} onClick={() => onShowBaselineDiffering("baseline_off")}>
                {split.offDefault} perhaps filled in by the firmware (the baseline is off its BIOS default there)
              </button>
              <p className="quiet">
                {split.noDefault} have no BIOS default to judge by. The split only means something if the baseline was saved with the BIOS
                at its defaults.
              </p>
            </>
          )}
        </Panel>
      )}

      <Panel title="Identity">
        <Status tone={IDENTITY_TONE[identity.status]} text={IDENTITY_LABEL[identity.status]} />
        <p>{identity.reason}</p>
        {identity.observed.length > 0 && (
          <ul className={s.list}>
            {identity.observed.map((o, i) => (
              <li key={i}>
                {o.source === "header" ? "File header" : "Metadata saved with the profile"}: <span className="mono">{o.text}</span>{" "}
                <span className="quiet">
                  {[o.board, o.flavor, o.revision].filter(Boolean).join(" ")}, at{" "}
                  {o.spans.map((r) => rangeText(r)).join(", ")}
                </span>
              </li>
            ))}
          </ul>
        )}
        <Limits items={identity.limitations} />
      </Panel>

      <Panel title="Integrity">
        <Status tone={INTEGRITY_TONE[integrity.status]} text={INTEGRITY_LABEL[integrity.status]} />
        {integrity.rule && (
          <p>
            Rule <span className="mono">{integrity.rule}</span>, {integrity.scope}
          </p>
        )}
        {integrity.stored !== null && integrity.computed !== null && (
          <p>
            Stored <span className="mono">{integrity.stored}</span>, computed <span className="mono">{integrity.computed}</span>
            {integrity.checksum_at !== null ? <> at <span className="mono">{hex(integrity.checksum_at)}</span></> : null}
          </p>
        )}
        <Limits items={integrity.limitations} />
      </Panel>

      <Panel title="Layout">
        <p>
          <Status tone={container === "established" ? "neutral" : "attention"} text={CONTAINER_LABEL[container]} />
        </p>
        {container_candidates.map((c) => (
          <div key={c.id} className={s.candidate}>
            <h3>
              <span className="mono">{c.id}</span>{" "}
              <span className="quiet">
                {c.id === selected_candidate ? "selected" : c.eligible ? "eligible" : "not selected"}
              </span>
            </h3>
            <p className="quiet">
              Rule <span className="mono">{c.rule}</span>
              {c.record_count !== null ? <>, <span className="mono">{c.record_count}</span> records</> : null}
            </p>
            <ul className={s.checks}>
              {c.checks.map((k, i) => (
                <li key={i}>
                  <Status tone={CHECK_TONE[k.status]} text={CHECK_LABEL[k.status]} />
                  <span className="mono">{k.name}</span>
                  <span className="quiet">{k.detail}</span>
                </li>
              ))}
            </ul>
            {c.chain_error && <p className="quiet">Chain error: {c.chain_error}</p>}
            {c.contradictions.length > 0 && (
              <>
                <p className="quiet">Contradicted by</p>
                <Limits items={c.contradictions} />
              </>
            )}
            {c.assumptions.length > 0 && (
              <>
                <p className="quiet">Assumes</p>
                <Limits items={c.assumptions} />
              </>
            )}
          </div>
        ))}
      </Panel>

      <Panel title="Coverage">
        <dl className={s.stats}>
            <Stat label="Records" value={coverage.records_total} />
            <Stat label="Cross-checked mapping" value={coverage.records_confirmed} />
            <Stat label="Inferred mapping" value={coverage.records_inferred} />
            <Stat label="Ambiguous mapping" value={coverage.records_ambiguous} />
            <Stat label="Not mapped" value={coverage.records_unmapped} />
            <Stat label="Settings" value={coverage.fields_total} />
            <Stat label="Decoded" value={coverage.fields_decoded} />
            <Stat label="Unknown value" value={coverage.fields_unknown_value} />
            <Stat label="Encoding not supported" value={coverage.fields_unsupported} />
            <Stat label="Outside the record" value={coverage.fields_out_of_bounds} />
            <Stat label="Profile bytes" value={coverage.profile_bytes} />
            <Stat label="Bytes in mapped records" value={coverage.mapped_bytes} />
            <Stat label="Bytes interpreted" value={coverage.interpreted_bytes} />
            <Stat label="Bytes not interpreted" value={coverage.opaque_bytes} />
        </dl>
      </Panel>

      {report.diagnostics.length > 0 && (
        <Panel title="Diagnostics">
          <ul className={s.list}>
            {report.diagnostics.map((d, i) => (
              <li key={i}>{d.message}</li>
            ))}
          </ul>
        </Panel>
      )}

      <Panel title="About this analysis">
        <p>
          {analysis.analyzer} {analysis.version}, experimental and read-only. Completion: {analysis.completion}.
        </p>
        <Limits items={analysis.rules.map((r) => `Rule ${r}`)} />
        <p className="quiet">This analysis cannot change, plan, migrate or create a profile.</p>
      </Panel>
    </div>
  );
}
