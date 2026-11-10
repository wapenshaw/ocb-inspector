// Settings view of the experimental analysis: fields grouped by BIOS menu page, filtered in memory,
// at most ROW_LIMIT rows rendered. Selecting a row shows its chain (file range, record, store,
// question, evidence) in the side panel.
import { memo, useDeferredValue, useMemo, Fragment, type KeyboardEvent } from "react";
import type { ExperimentalField, Evidence } from "../api/types";
import { cx } from "../lib/cx";
import { pageBefore, pageLabel, pageKey, sectionBefore } from "../lib/menu";
import {
  BASELINE_FILTERS,
  DEFAULTS_FILTERS,
  type DefaultCounts,
  type DefaultsFilter,
  FIELD_STATE_LABEL,
  MAPPING_LABEL,
  MAPPING_TONE,
  STATE_FILTERS,
  filterTest,
  rangeText,
  type FieldItem,
  type Lookup,
  type StateFilter,
} from "../lib/analysis";
import { hex } from "../lib/format";
import { moveRow } from "../lib/rows";
import { MoreRows, ROW_LIMIT } from "./MoreRows";
import { Icon } from "./Icon";
import { SegGroup } from "./SegGroup";
import { Status } from "./Status";
import s from "./Analyze.module.css";

export interface SettingsFilter {
  query: string;
  state: StateFilter;
  defaults: DefaultsFilter;
}

export const DEFAULT_FILTER: SettingsFilter = { query: "", state: "all", defaults: "all" };

interface SettingsProps {
  items: FieldItem[];
  lookup: Lookup;
  filter: SettingsFilter;
  onFilter: (patch: Partial<SettingsFilter>) => void;
  selected: string | null;
  onSelect: (key: string) => void;
  /** Shown when no setting is interpreted at all: why there are none. */
  emptyReason: string;
  /** Counts over every field of the report, computed once by the caller. */
  counts: DefaultCounts;
  /** Set while a baseline profile is applied: shows its column and the "Differs from baseline" filter. */
  baseline?: BaselineView | null;
}

export interface BaselineView {
  /** Cross-version baseline: how many settings could not be compared (null: not a cross-version comparison). */
  crossNotCompared: number | null;
}

const rowId = (key: string) => `analyze-row-${key}`;

export function AnalyzeSettings({ items, lookup, filter, onFilter, selected, onSelect, emptyReason, counts, baseline = null }: SettingsProps) {
  const hasBaseline = baseline !== null;
  const cols = hasBaseline ? 6 : 5;
  const query = useDeferredValue(filter.query);
  const matching = useMemo(() => {
    const q = query.trim().toLowerCase();
    const stateTest = filterTest(STATE_FILTERS, filter.state);
    const defaultsTest = filterTest(BASELINE_FILTERS, filter.defaults);
    return items.filter((it) => stateTest(it.f) && defaultsTest(it.f) && (!q || it.hay.includes(q)));
  }, [items, query, filter.state, filter.defaults]);
  const shown = useMemo(() => matching.slice(0, ROW_LIMIT), [matching]);
  const chosen = useMemo(() => (selected ? (items.find((it) => it.f.key === selected) ?? null) : null), [items, selected]);

  function onKeyDown(e: KeyboardEvent<HTMLTableElement>) {
    const i = shown.findIndex((it) => it.f.key === selected);
    moveRow(e, shown, i, (it) => onSelect(it.f.key), (it) => rowId(it.f.key));
  }

  return (
    <div className={s.split}>
      <div className={s.listpane}>
        <div className={s.toolbar}>
          <label className={s.search}>
            <Icon name="search" />
            <span className="visually-hidden">Search settings</span>
            <input type="search" placeholder="Search settings" value={filter.query} onChange={(e) => onFilter({ query: e.target.value })} />
          </label>
          <SegGroup
            label="Show"
            name="analyze-state"
            options={STATE_FILTERS}
            value={filter.state}
            onChange={(state) => onFilter({ state })}
          />
          <SegGroup
            label="Compared with the BIOS optimized default"
            name="analyze-defaults"
            options={hasBaseline ? BASELINE_FILTERS : DEFAULTS_FILTERS}
            value={filter.defaults}
            onChange={(defaults) => onFilter({ defaults })}
          />
          <span className="quiet" data-testid="default-counts">
            {counts.differs} differ · {counts.noDefault} have no default
            {hasBaseline && ` · ${counts.baselineDiffers} differ from baseline`}
          </span>
          {hasBaseline && counts.baselineDiffers > 0 && (
            <span className="quiet" data-testid="baseline-counts">
              Of those: {counts.baselineAtDefault} with the baseline at its default, {counts.baselineOffDefault} with it off its default,{" "}
              {counts.baselineNoDefault} unknown
            </span>
          )}
          <span className="quiet">
            <span className="mono">{matching.length}</span> of <span className="mono">{items.length}</span> settings
          </span>
        </div>

        {baseline?.crossNotCompared != null && (
          <p className={`quiet ${s.crossnote}`} data-testid="cross-note">
            Matched across two BIOS versions by menu place and name; {baseline.crossNotCompared} settings could not be compared.
          </p>
        )}

        <div className="tablewrap">
          <table className={s.grid} onKeyDown={onKeyDown}>
            <colgroup>
              <col style={{ width: hasBaseline ? "25%" : "30%" }} />
              <col style={{ width: hasBaseline ? "16%" : "20%" }} />
              <col style={{ width: "9%" }} />
              <col style={{ width: hasBaseline ? "14%" : "16%" }} />
              {hasBaseline && <col style={{ width: "14%" }} />}
              <col />
            </colgroup>
            <thead>
              <tr>
                <th>Setting</th>
                <th>Stored value</th>
                <th>Raw</th>
                <th>BIOS optimized default</th>
                {hasBaseline && <th>Baseline</th>}
                <th>Mapping</th>
              </tr>
            </thead>
            <tbody>
              {shown.length === 0 ? (
                <tr>
                  <td colSpan={cols} className="quiet empty">
                    {items.length ? "No settings match." : emptyReason}
                  </td>
                </tr>
              ) : (
                shown.map((it, i) => {
                  const page = pageBefore(shown, i);
                  const section = sectionBefore(shown, i);
                  return (
                    <Fragment key={it.f.key}>
                      {page && (
                        <tr className={s.pagehead}>
                          <th colSpan={cols} scope="colgroup">{pageLabel(page, "Not on a menu page")}</th>
                        </tr>
                      )}
                      {section && (
                        <tr className={s.section}>
                          <th colSpan={cols} scope="colgroup">{section}</th>
                        </tr>
                      )}
                      <FieldRow it={it} selected={it.f.key === selected} onSelect={onSelect} baseline={hasBaseline} />
                    </Fragment>
                  );
                })
              )}
            </tbody>
          </table>
          <MoreRows total={matching.length} />
        </div>
      </div>

      <aside className={cx("inspector", s.inspect)} aria-label="How this value is found">
        {chosen ? (
          <Chain it={chosen} lookup={lookup} />
        ) : (
          <p className="quiet">
            Select a setting to see how its value is found: the bytes in the profile, the record they belong to, the BIOS store, the
            question and the evidence.
          </p>
        )}
      </aside>
    </div>
  );
}

const FieldRow = memo(function FieldRow({ it, selected, onSelect, baseline }: { it: FieldItem; selected: boolean; onSelect: (key: string) => void; baseline: boolean }) {
  const f: ExperimentalField = it.f;
  const numeric = f.state === "decoded" && f.value_label === null;
  return (
    <tr
      id={rowId(f.key)}
      data-row
      tabIndex={0}
      aria-selected={selected}
      onClick={() => onSelect(f.key)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelect(f.key);
        }
      }}
    >
      <td>
        <span className={s.label}>{f.label}</span>
        <span className={`${s.at} mono`}>offset {hex(f.store_offset)}</span>
      </td>
      <td>
        <span className={cx(numeric && "mono", f.state !== "decoded" && s.notDecoded)}>{it.value}</span>
        {f.state !== "decoded" && (
          <span className={s.reason}>
            {FIELD_STATE_LABEL[f.state]}
            {f.note ? `: ${f.note}` : ""}
          </span>
        )}
      </td>
      <td className="mono">{f.raw?.hex ?? "—"}</td>
      <td>
        <span className={numeric ? "mono" : undefined}>{it.defaultText}</span>
        {f.differs_from_default === true && (
          <span className={s.reason}>
            <Status tone="mod" text="Differs" />
          </span>
        )}
      </td>
      {baseline && (
        <td>
          {typeof f.differs_from_baseline === "boolean" ? (
            <>
              <span className={numeric ? "mono" : undefined}>{it.baselineText}</span>
              {f.differs_from_baseline && (
                <span className={s.reason}>
                  <Status tone="mod" text="Differs" />
                </span>
              )}
            </>
          ) : (
            <span className="quiet">Not compared</span>
          )}
        </td>
      )}
      <td>
        <Status tone={MAPPING_TONE[f.mapping]} text={MAPPING_LABEL[f.mapping]} />
      </td>
    </tr>
  );
});

function Chain({ it, lookup }: { it: FieldItem; lookup: Lookup }) {
  const f = it.f;
  const rec = lookup.records.get(f.record);
  const st = lookup.stores.get(f.store);
  const flags = [f.hidden && "hidden in the menus", f.conditional && "shown only under a condition", f.callback && "changed by BIOS callback code"].filter(
    (x): x is string => typeof x === "string",
  );
  return (
    <>
      <h2>{f.label}</h2>
      {f.help && <p className="quiet">{f.help}</p>}
      <ol className={s.chain}>
        <li>
          <h3>Bytes in the profile</h3>
          {f.span ? (
            <p className="mono">{rangeText(f.span)}</p>
          ) : (
            <p className="quiet">No bytes: the offset lies outside the record.</p>
          )}
        </li>
        <li>
          <h3>Record</h3>
          {rec ? (
            <>
              <p>
                <span className="mono">{rec.key}</span>, record ID <span className="mono">{rec.id}</span>, payload{" "}
                <span className="mono">{rangeText(rec.payload)}</span>
              </p>
              <p>
                <Status tone={MAPPING_TONE[rec.mapping]} text={MAPPING_LABEL[rec.mapping]} />
              </p>
              <p className="quiet">{rec.reason}</p>
            </>
          ) : (
            <p className="quiet">Record {f.record} is not in this report.</p>
          )}
        </li>
        <li>
          <h3>BIOS store</h3>
          {st ? (
            <>
              <p>
                <strong>{st.name}</strong> <span className="mono">{st.size} bytes</span>, set {st.set}
              </p>
              <p className={`guid mono faint`}>{st.guid}</p>
              <p className="quiet">
                Declared by{" "}
                {st.declared_by.length
                  ? st.declared_by.map((d) => `${d.formset_title || "formset"} (module ${d.module})`).join("; ")
                  : "no formset recorded"}
              </p>
              <p>
                <Status tone={MAPPING_TONE[st.mapping]} text={MAPPING_LABEL[st.mapping]} />
              </p>
              <p className="quiet">{st.reason}</p>
            </>
          ) : (
            <p className="quiet">Store {f.store} is not in this report.</p>
          )}
        </li>
        <li>
          <h3>BIOS question</h3>
          <p>
            {f.label}: store offset <span className="mono">{hex(f.store_offset)}</span>, <span className="mono">{f.width_bits}</span> bits
          </p>
          <p className="quiet">Menu: {f.menu_path.length ? pageKey(f.menu_path) : "not on a menu page"}</p>
          {flags.length > 0 && <p className="quiet">{flags.join("; ")}</p>}
        </li>
        <li>
          <h3>Evidence</h3>
          {f.evidence.length === 0 && <p className="quiet">No evidence recorded.</p>}
          {f.evidence.map((id) => (
            <EvidenceItem key={id} id={id} entry={lookup.evidence.get(id)} />
          ))}
        </li>
      </ol>
    </>
  );
}

const BASIS_LABEL: Record<Evidence["basis"], string> = {
  file_structure: "the file's own structure",
  reviewed_binding: "a reviewed binding",
  firmware_code: "the BIOS's own code and setup data",
  file_comparison: "comparing two files through their BIOS menus and defaults",
  size_only: "matching sizes only",
};

function EvidenceItem({ id, entry }: { id: string; entry: Evidence | undefined }) {
  if (!entry) return <p className="quiet mono">{id}</p>;
  return (
    <div>
      <p>
        <span className="mono faint">{entry.id}</span>
      </p>
      <p>{entry.summary}</p>
      <p className="quiet">Based on: {BASIS_LABEL[entry.basis]}</p>
      {entry.observations.length > 0 && (
        <>
          <p className="quiet">Observed</p>
          <ul className={s.list}>
            {entry.observations.map((o, i) => <li key={i}>{o}</li>)}
          </ul>
        </>
      )}
      {entry.contradictions.length > 0 && (
        <>
          <p className="quiet">Contradicted by</p>
          <ul className={s.list}>
            {entry.contradictions.map((o, i) => <li key={i}>{o}</li>)}
          </ul>
        </>
      )}
      {entry.limitations.length > 0 && (
        <ul className={s.limits}>
          {entry.limitations.map((o, i) => <li key={i}>{o}</li>)}
        </ul>
      )}
    </div>
  );
}
