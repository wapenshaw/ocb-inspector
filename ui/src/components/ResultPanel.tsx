import type { BuildResult } from "../api/types";
import { fileName } from "../lib/format";
import { Checks } from "./Checks";
import { Status } from "./Status";
import { ValueText } from "./ValueText";
import s from "./ResultPanel.module.css";

export interface ResultPanelProps {
  result: BuildResult;
  onDone: () => void;
}

export function ResultPanel({ result, onDone }: ResultPanelProps) {
  const m = result.manifest;
  const failed = m.checks.some((c) => c.status === "fail");

  return (
    <section className={s.result} aria-labelledby="result-title">
      <div className={s.head}>
        <Status tone={failed ? "blocked" : "ok"} text={failed ? "Verification failed" : "Written and verified"} />
        <h2 id="result-title">{fileName(result.output_path)}</h2>
        <p className="quiet">
          Saved for <span className="mono">{m.base.bios_id}</span>. The engine re-read the new file from disk bytes and checked it
          against your request. Importing it on the motherboard is the one check only you can run.
        </p>
      </div>

      <dl className={s.facts}>
        <dt>Profile</dt>
        <dd className="mono">{result.output_path}</dd>
        <dt>Report</dt>
        <dd className="mono">{result.report_path}</dd>
        <dt>SHA-256</dt>
        <dd className="mono">{m.output.sha256}</dd>
        <dt>Bytes changed</dt>
        <dd>
          <span className="mono">{m.binary_spans.reduce((n, sp) => n + sp.end - sp.start, 0)}</span> in {m.binary_spans.length} spans,
          all inside the requested settings and the checksum
        </dd>
      </dl>

      <h3>Changes</h3>
      {m.changes.length === 0 ? (
        <p className="quiet">No values differ from the baseline.</p>
      ) : (
        <table className={s.changes}>
          <tbody>
            {m.changes.map((c) => (
              <tr key={c.setting_id}>
                <td>{c.label}</td>
                <td>
                  <ValueText value={c.from} strike />
                </td>
                <td className={s.to}>
                  <ValueText value={c.to} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h3>Verification</h3>
      <Checks checks={m.checks} />

      <h3>Limits of this check</h3>
      <ul className={s.limits}>
        {m.limitations.map((l, i) => (
          <li key={i}>{l}</li>
        ))}
      </ul>

      <div className={s.actions}>
        <button className="btn" onClick={onDone}>
          Done
        </button>
      </div>
    </section>
  );
}
