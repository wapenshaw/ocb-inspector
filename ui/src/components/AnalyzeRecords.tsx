// Records view of the experimental analysis: every record in the profile with its file range,
// the variable the BIOS's own list names for it (when known), candidate stores and the reason
// for its mapping, plus a bounded byte preview. Ambiguous records name their candidate stores
// and never get a settings table.
import { useMemo } from "react";
import type { ExperimentalInspectPayload } from "../api/types";
import { MAPPING_LABEL, MAPPING_TONE, previewText, rangeText, type Lookup } from "../lib/analysis";
import { MoreRows, ROW_LIMIT } from "./MoreRows";
import { Status } from "./Status";
import s from "./Analyze.module.css";

export function AnalyzeRecords({ payload, lookup }: { payload: ExperimentalInspectPayload; lookup: Lookup }) {
  const shown = useMemo(() => payload.records.slice(0, ROW_LIMIT), [payload.records]);
  if (payload.records.length === 0) {
    return <p className="quiet empty">No records were found in this layout.</p>;
  }
  return (
    <div className="tablewrap">
      <table className={s.grid}>
        <colgroup>
          <col style={{ width: "13%" }} />
          <col style={{ width: "15%" }} />
          <col style={{ width: "9%" }} />
          <col style={{ width: "15%" }} />
          <col style={{ width: "28%" }} />
          <col />
        </colgroup>
        <thead>
          <tr>
            <th>Record (variable)</th>
            <th>Bytes in the profile</th>
            <th>Size</th>
            <th>Mapping</th>
            <th>Candidate stores and reason</th>
            <th>First bytes</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((r) => (
            <tr key={r.key} data-row>
              <td>
                {r.variable ? <span title={r.variable.guid}>{r.variable.name}</span> : <span className="mono">{r.key}</span>}
                <span className={`${s.at} mono`}>
                  {r.variable ? `${r.key}, ` : ""}ID {r.id}
                </span>
              </td>
              <td className="mono">
                payload {rangeText(r.payload)}
                <span className={s.at}>header {rangeText(r.header)}</span>
              </td>
              <td className="mono">{r.size} bytes</td>
              <td>
                <Status tone={MAPPING_TONE[r.mapping]} text={MAPPING_LABEL[r.mapping]} />
              </td>
              <td>
                {r.candidate_stores.length > 0 && (
                  <ul className={s.stores}>
                    {r.candidate_stores.map((k) => (
                      <li key={k}>{lookup.stores.get(k)?.name ?? k}</li>
                    ))}
                  </ul>
                )}
                <span className={s.reason}>{r.reason}</span>
              </td>
              <td className={`mono ${s.preview}`}>{r.preview_omitted ? <span className="quiet">Left out</span> : previewText(r.preview_hex, r.preview_truncated)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <MoreRows total={payload.records.length} />
    </div>
  );
}
