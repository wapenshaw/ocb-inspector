import { Fragment, memo, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { api, errorMessage, pickSave } from "../api";
import type { BuildResult, DecodedSetting, EditPreview, InspectPayload, Operation, RawField, Report, Value } from "../api/types";
import { cpuShort, display, fileName, hex, sameIntent, savedAt, shortHash } from "../lib/format";
import { menuPages, menuSort, NOT_IN_BIOS, pageBefore, pageKey, sectionBefore, type Page, pageLabel } from "../lib/menu";
import { cx } from "../lib/cx";
import { moveRow } from "../lib/rows";
import { useAction } from "../lib/useAction";
import { useApp } from "../state/app";
import { Browse, type BrowseHandle } from "./Browse";
import { Checks } from "./Checks";
import { Icon } from "./Icon";
import { MenuTree } from "./MenuTree";
import { MoreRows, ROW_LIMIT } from "./MoreRows";
import { PairPicker, type PairPickerHandle } from "./PairPicker";
import { ResultPanel } from "./ResultPanel";
import { Status, type Tone } from "./Status";
import { UntestedNote } from "./UntestedNote";
import { SwapLabel } from "./SwapLabel";
import { ValueEditor } from "./ValueEditor";
import { ValueText } from "./ValueText";
import css from "./Inspect.module.css";

// Inspect: pair a profile with its BIOS, browse the settings, stage and export edits.
// Writing-related controls appear only when the app reports writing as on.

interface InspectProps {
  /** A BIOS open on its own (no profile needed), or null. The header shows "Browse BIOS" as current. */
  browsing: string | null;
  onBrowsingChange: (v: string | null) => void;
  /** This view is on screen: its keyboard shortcuts are live. */
  active: boolean;
  /** Open the experimental analysis with these paths (empty strings mean not chosen). */
  onAnalyze?: (bios: string, profile: string) => void;
}

type Filter = "all" | "defaults" | "edited";
type Mode = "table" | "review" | "result";
interface CheckState {
  state: "idle" | "checking" | "ok" | "error";
  message: string;
  preview: EditPreview | null;
}

const IDLE: CheckState = { state: "idle", message: "", preview: null };
const EMPTY: Record<string, Value> = {};
// Settings table columns: title and width (the last takes the rest).
const READ_COLUMNS: [string, string?][] = [["Setting", "36%"], ["Saved", "22%"], ["Default", "22%"], ["Status"]];
const WRITE_COLUMNS: [string, string?][] = [["Setting", "30%"], ["Saved", "17%"], ["Default", "14%"], ["Proposed", "15%"], ["Status"]];
const HEAT_TEXT: Record<string, string> = {
  "heat-4": "edit staged",
  "heat-3": "off stock",
  "heat-0": "stock, Auto",
  "heat-1": "stock, fixed value",
};

// Settings are keyed by the menu page they sit on; "raw" is the all-fields view.
const reviewed = (x: DecodedSetting) => x.group !== "bios";
const keyOf = (x: DecodedSetting) => (x.menu ? pageKey(x.menu.path) : NOT_IN_BIOS);
const offDefault = (x: DecodedSetting) => x.differs_from_default === true;
const nameOf = (x: DecodedSetting) => x.menu?.prompt || x.label;
const rawOff = (f: RawField) => f.default_raw != null && f.default_raw !== f.raw;
const rawOf = (v: Value | null | undefined) => (v?.kind === "enum" ? v.raw : v?.kind === "boolean" ? Number(v.value) : undefined);

interface SettingRowProps {
  x: DecodedSetting;
  index: number;
  heat: string;
  selected: boolean;
  /** The staged value, when there is one. */
  staged: Value | undefined;
  tone: Tone;
  text: string;
  writing: boolean;
  onSelect: (id: string) => void;
  onKey: (e: KeyboardEvent, index: number) => void;
}

// One settings row. Memoised with plain props, so selecting a row or staging an edit re-renders
// only the rows that changed.
const SettingRow = memo(function SettingRow({ x, index, heat, selected, staged, tone, text, writing, onSelect, onKey }: SettingRowProps) {
  return (
    <tr
      id={`row-${x.setting_id}`}
      tabIndex={0}
      className={cx(heat, selected && css.sel, (!x.available || x.activity === "inactive") && css.dim)}
      aria-selected={selected}
      onClick={() => onSelect(x.setting_id)}
      onKeyDown={(e) => onKey(e, index)}
    >
      <td title={x.label}>
        {nameOf(x)}
        {reviewed(x) && (
          <span className="rv" title="Reviewed setting: values, units and dependencies checked">
            reviewed
          </span>
        )}
      </td>
      <td>
        <ValueText value={x.value} />
      </td>
      <td className="quiet">{x.default_value ? <ValueText value={x.default_value} /> : <span className="faint">—</span>}</td>
      {writing && (
        <td className={staged ? "strong" : undefined}>{staged ? <ValueText value={staged} /> : <span className="faint">No change</span>}</td>
      )}
      <td>{text && <Status tone={tone} text={text} />}</td>
    </tr>
  );
});

const HeatCell = memo(function HeatCell({ id, heat, text, selected, onSelect }: { id: string; heat: string; text: string; selected: boolean; onSelect: (id: string) => void }) {
  return <button className={cx(css.cell, heat, selected && css.sel)} title={text} aria-label={text} onClick={() => onSelect(id)}></button>;
});

function operationsOf(payload: InspectPayload, staged: Record<string, Value>): Operation[] {
  return Object.entries(staged).map(
    ([id, value]): Operation => ({
      op: "set",
      setting_id: id,
      expected: payload.settings.find((x) => x.setting_id === id)!.value!,
      value,
    }),
  );
}

export function Inspect({ browsing, onBrowsingChange, active, onAnalyze }: InspectProps) {
  const app = useApp();
  const [bios, setBios] = useState("");
  const [profile, setProfile] = useState("");
  const [report, setReport] = useState<Report<InspectPayload> | null>(null);
  const picker = useRef<PairPickerHandle>(null);
  const browser = useRef<BrowseHandle>(null);

  // A BIOS menu page key, "raw" for every BIOS field, or NOT_IN_BIOS.
  const [group, setGroup] = useState("");
  const [query, setQuery] = useState("");
  // all = everything on the page; defaults = differs from optimized defaults; edited = staged here.
  const [filter, setFilter] = useState<Filter>("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const searchEl = useRef<HTMLInputElement>(null);

  // Staged edits with undo/redo history.
  const [timeline, setTimeline] = useState<{ history: Record<string, Value>[]; cursor: number }>({
    history: [{}],
    cursor: 0,
  });
  const { history, cursor } = timeline;
  const staged = history[cursor] ?? EMPTY;
  const changeCount = Object.keys(staged).length;

  const [check, setCheck] = useState<CheckState>(IDLE);
  const [mode, setMode] = useState<Mode>("table");
  const [result, setResult] = useState<BuildResult | null>(null);
  const exporter = useAction();

  const p = report?.payload ?? null;
  // Columns in the settings table: Proposed only exists when writing is on.
  const columns = app.writing ? WRITE_COLUMNS : READ_COLUMNS;
  const cols = columns.length;
  const writable = !!p?.capabilities.image_approved_for_writing;
  /** Why the image is inspect-only, including where the approvals file was looked for. */
  const approvalNote = report?.diagnostics.find((d) => d.code === "image_not_approved")?.message ?? "";
  const hasDefaults = !!p?.bios.defaults?.available;
  // Reviewed settings plus every other setting the BIOS menus show (read only).
  const all = useMemo<DecodedSetting[]>(() => (p ? [...p.settings, ...(p.bios_settings ?? [])] : []), [p]);
  const pages = useMemo<Page[]>(() => menuPages(all), [all]);
  const absent = all.filter((x) => !x.menu);

  // Sorted and made searchable once per report; each keystroke only filters (order is kept).
  const sorted = useMemo(
    () =>
      menuSort(all).map((x) => ({
        x,
        key: keyOf(x),
        hay: `${nameOf(x)} ${x.label} ${x.setting_id} ${(x.menu?.path ?? []).join(" ")} ${x.fields.flatMap((f) => f.prompts).join(" ")}`.toLowerCase(),
      })),
    [all],
  );
  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    const across = !!q || filter !== "all";
    return sorted
      .filter(({ x, key, hay }) => {
        if (q && !hay.includes(q)) return false;
        if (filter === "edited" && !(x.setting_id in staged)) return false;
        if (filter === "defaults" && !offDefault(x)) return false;
        if (!across && key !== group) return false;
        return true;
      })
      .map(({ x }) => x);
  }, [sorted, query, filter, group, staged]);
  // Rows span several pages when searching or filtering; then each page gets a heading row.
  const spansPages = new Set(rows.map(keyOf)).size > 1;

  // All fields: sorted once per report too; filtering keeps the order.
  const rawSorted = useMemo(
    () =>
      menuSort(
        (p?.raw_fields ?? []).map((f) => {
          const path = f.menu_path ?? [];
          return {
            f,
            differs: rawOff(f),
            menu: { path, section: f.section ?? null, order: f.menu_order ?? [] },
            hay: `${f.prompt} ${f.store} ${f.offset} ${path.join(" ")}`.toLowerCase(),
          };
        }),
      ),
    [p],
  );
  const rawRows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return rawSorted.filter((r) => (!q || r.hay.includes(q)) && (filter !== "defaults" || r.differs));
  }, [rawSorted, query, filter]);
  const rawDiffers = useMemo(() => rawSorted.filter((r) => r.differs).length, [rawSorted]);

  const selected = all.find((x) => x.setting_id === selectedId) ?? null;
  // Long lists (search, "Not at default" across every page) render in slices.
  const shownRows = rows.slice(0, ROW_LIMIT);

  const byPage = useMemo(() => {
    const m = new Map<string, DecodedSetting[]>();
    for (const x of all) {
      const k = keyOf(x);
      const list = m.get(k);
      if (list) list.push(x);
      else m.set(k, [x]);
    }
    return m;
  }, [all]);
  const inPage = (key: string) => byPage.get(key) ?? [];
  const stagedIn = (key: string) => inPage(key).filter((x) => x.setting_id in staged).length;
  const differsIn = (key: string) => inPage(key).filter(offDefault).length;
  const differsAll = all.filter(offDefault).length;

  // Validation runs 200 ms after the last change to the staged edits (or to the pair). A newer
  // change cancels the pending check and ignores its answer.
  useEffect(() => {
    if (!p || Object.keys(staged).length === 0) {
      setCheck(IDLE);
      return;
    }
    setCheck((c) => ({ ...c, state: "checking" }));
    let stale = false;
    const timer = setTimeout(async () => {
      try {
        const preview = await api.editValidate(bios, profile, p.binding, operationsOf(p, staged));
        if (!stale) setCheck({ state: "ok", message: "", preview });
      } catch (e) {
        if (!stale) setCheck({ state: "error", message: errorMessage(e).replace(/^setting validation failed: /, ""), preview: null });
      }
    }, 200);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [p, bios, profile, staged]);

  /** Record a new staged map as the latest history step. */
  function commit(next: Record<string, Value>) {
    setTimeline({ history: [...history.slice(0, cursor + 1), next], cursor: cursor + 1 });
  }

  function moveTo(to: number) {
    setTimeline({ history, cursor: to });
  }

  function stage(id: string, v: Value) {
    const setting = p?.settings.find((x) => x.setting_id === id);
    if (!setting) return;
    const next = { ...staged };
    if (sameIntent(v, setting.value)) delete next[id];
    else next[id] = v;
    commit(next);
  }

  function revert(id: string) {
    const next = { ...staged };
    delete next[id];
    commit(next);
  }

  function revertAll() {
    commit({});
  }

  function undo() {
    if (cursor > 0) moveTo(cursor - 1);
  }

  function redo() {
    if (cursor < history.length - 1) moveTo(cursor + 1);
  }

  /** A new pair was read (or cleared): start fresh on its first page. */
  function loaded(r: Report<InspectPayload> | null) {
    setReport(r);
    setTimeline({ history: [{}], cursor: 0 });
    setMode("table");
    const first = r ? menuSort(r.payload.settings)[0] : null;
    setSelectedId(first?.setting_id ?? null);
    setGroup(first?.menu ? pageKey(first.menu.path) : "");
    setFilter("all");
    setQuery("");
  }

  function openFile() {
    onBrowsingChange(null);
    setReport(null);
    void picker.current?.chooseBios();
  }

  function focusSearch() {
    if (browsing) browser.current?.focusSearch();
    else searchEl.current?.focus();
  }

  // Keyboard shortcuts while this view is on screen. The listener is added once per activation and
  // calls the latest handler through a ref.
  const shortcuts = useRef<(e: globalThis.KeyboardEvent) => void>(() => {});
  shortcuts.current = (e) => {
    if (!(e.ctrlKey || e.metaKey)) return;
    const k = e.key.toLowerCase();
    if (k === "o") {
      e.preventDefault();
      openFile();
    } else if (k === "f") {
      e.preventDefault();
      focusSearch();
    } else if (k === "z" && app.writing && !(e.target instanceof HTMLInputElement)) {
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
    }
  };
  useEffect(() => {
    if (!active) return;
    const onKey = (e: globalThis.KeyboardEvent) => shortcuts.current(e);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active]);

  async function exportProfile() {
    if (!p) return;
    const stem = fileName(profile).replace(/\.ocb$/i, "");
    const out = await pickSave(`${stem}-edited.ocb`, "ocb");
    if (!out) return;
    await exporter.run(async () => {
      setResult(await api.editBuild(bios, profile, p.binding, operationsOf(p, staged), out));
      setMode("result");
    });
  }

  // Rows call this through a stable callback, so the memoised rows do not re-render when `rows` changes.
  const rowKeyRef = useRef((_e: KeyboardEvent, _idx: number) => {});
  rowKeyRef.current = (e, idx) => {
    if (moveRow(e, rows, idx, (n) => setSelectedId(n.setting_id), (n) => `row-${n.setting_id}`)) return;
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      const n = rows[idx];
      if (n) setSelectedId(n.setting_id);
      document.getElementById("inspector-edit")?.focus();
    }
  };
  const onRowKey = useCallback((e: KeyboardEvent, idx: number) => rowKeyRef.current(e, idx), []);

  function openPage(key: string) {
    setGroup(key);
    setQuery("");
    setFilter("all");
  }

  function statusOf(x: DecodedSetting): { tone: Tone; text: string } {
    if (x.setting_id in staged) return { tone: "accent", text: "Edited" };
    if (!x.available) return { tone: "neutral", text: "Not in this BIOS" };
    if (x.value?.kind === "unknown") return { tone: "attention", text: "Unknown value" };
    if (offDefault(x)) return { tone: "mod", text: "Not at default" };
    if (app.writing && !x.editability.writable) return { tone: "neutral", text: "Read only" };
    if (x.activity === "inactive") return { tone: "neutral", text: "Inactive" };
    return { tone: "neutral", text: "" };
  }

  /** Heat level for a row: 4 staged, 3 off stock, 0 stock Auto, 1 stock fixed value ("" when defaults are unknown). */
  function heatOf(x: DecodedSetting): string {
    if (x.setting_id in staged) return "heat-4";
    if (!hasDefaults || !x.available) return "";
    if (offDefault(x)) return "heat-3";
    return x.value?.kind === "auto" || (x.value?.kind === "enum" && x.value.label === "Auto") ? "heat-0" : "heat-1";
  }

  function countCell(pg: Page): ReactNode {
    const n = inPage(pg.key).length;
    const d = differsIn(pg.key);
    const e = stagedIn(pg.key);
    return (
      <>
        {e ? <span className={css.dotc} title={`${e} edited`}>{e}</span> : null}
        {d ? <span className={css.modc} title={`${d} not at default`}>≠{d}</span> : null}
        <span title={`${n} settings`}>{n}</span>
      </>
    );
  }

  function settingDetails(x: DecodedSetting): ReactNode {
    const current = staged[x.setting_id] ?? x.value;
    const alsoOn = x.menu?.also_on ?? [];
    return (
      <>
        {x.menu && (
          <p className={`${css.crumb} faint`}>
            {pageKey(x.menu.path)}
            {x.menu.section ? `, ${x.menu.section}` : ""}
          </p>
        )}
        <h2>{nameOf(x)}</h2>
        {reviewed(x) ? (
          <p className={css.kind}>
            <span className="rv">reviewed</span> Values, units and dependencies checked against real profiles.
          </p>
        ) : (
          <p className={`${css.kind} faint`}>Read from the BIOS's own menu definition. Not a reviewed setting.</p>
        )}
        {(x.summary || nameOf(x) !== x.label) && (
          <p className="quiet">
            {nameOf(x) !== x.label && (
              <>
                <span className={css.plain}>{x.label}.</span>{" "}
              </>
            )}
            {x.summary}
          </p>
        )}

        {app.writing && (
          <div className={css.editbox} id="inspector-edit" tabIndex={-1}>
            {x.available && x.editability.writable && x.constraints ? (
              <>
                <ValueEditor
                  label={nameOf(x)}
                  value={current}
                  constraints={x.constraints}
                  autoOnly={x.editability.auto_only}
                  onChange={(v) => stage(x.setting_id, v)}
                />
                {/* Both stay in place and are disabled when they do nothing, so staging never moves the panel. */}
                <button className={`btn ghost ${css.small}`} disabled={!(x.setting_id in staged)} onClick={() => revert(x.setting_id)}>
                  <Icon name="undo" /> Revert to saved
                </button>
                {x.default_value && (!x.editability.auto_only || x.default_value.kind === "auto") && (
                  <button
                    className={`btn ghost ${css.small}`}
                    disabled={sameIntent(staged[x.setting_id] ?? x.value, x.default_value)}
                    onClick={() => stage(x.setting_id, x.default_value!)}
                  >
                    Set to default ({display(x.default_value)})
                  </button>
                )}
              </>
            ) : (
              <Status tone="neutral" text="Read only" />
            )}
            <p className={`${css.reason} faint`}>{x.editability.reason}</p>
          </div>
        )}

        {!app.writing && x.available && (
          <div className={css.savedbox}>
            <span className="faint">Saved in this profile</span>
            <strong>
              <ValueText value={x.value} />
            </strong>
          </div>
        )}

        {hasDefaults && x.available && (
          <div className={cx(css.defbox, offDefault(x) && css.offdef)}>
            {offDefault(x) ? (
              <>
                <Status tone="mod" text="Not at default" />
                <p>
                  Optimized default: <ValueText value={x.default_value} />
                </p>
              </>
            ) : (
              <Status text="Matches the optimized default" />
            )}
          </div>
        )}

        {!app.writing && x.constraints?.options.length ? (
          <>
            <h3>Choices</h3>
            <ul className={css.choices}>
              {x.constraints.options.map((o) => {
                const saved = rawOf(x.value) === o.raw;
                const isDefault = rawOf(x.default_value) === o.raw;
                return (
                  <li key={o.raw} className={cx(saved && css.cur)}>
                    <span>{o.label}</span>
                    <span className={`faint ${css.small}`}>{[saved && "saved", isDefault && "default"].filter(Boolean).join(", ")}</span>
                  </li>
                );
              })}
            </ul>
          </>
        ) : !app.writing && x.constraints && (x.constraints.min != null || x.constraints.max != null) ? (
          <>
            <h3>Allowed</h3>
            <p className="quiet">
              <span className="mono">{x.constraints.min ?? "?"}</span> to <span className="mono">{x.constraints.max ?? "?"}</span>
              {x.constraints.unit ? ` ${x.constraints.unit}` : null}
            </p>
          </>
        ) : null}

        {x.bios_help && (
          <>
            <h3>BIOS help</h3>
            <p className={css.bioshelp}>{x.bios_help}</p>
          </>
        )}
        {alsoOn.length > 0 && <p className={`faint ${css.reason}`}>Also shown on {alsoOn.map(pageKey).join("; ")}.</p>}

        <h3>When it applies</h3>
        <p className={cx(x.activity !== "inactive" && "quiet")}>{x.activity_reason}</p>

        {x.fields.length > 0 && (
          <>
            <h3>Where it is stored</h3>
            {x.fields.map((f, i) => (
              <dl key={i} className={css.ev}>
                <dt>Field</dt>
                <dd>{f.prompts[0] ?? ""}</dd>
                <dt>Location</dt>
                <dd>
                  <span className="mono">
                    {f.store}+{hex(f.store_offset)}
                  </span>
                  , file <span className="mono">{hex(f.file_offset)}</span>, {f.width_bits} bits
                </dd>
                <dt>Stored bytes</dt>
                <dd className={css.bytes}>
                  {f.bytes_hex.split(" ").map((b, j) => (
                    <span key={j} className="solder">
                      {b}
                    </span>
                  ))}{" "}
                  <span className="mono faint">= {f.raw}</span>
                </dd>
                {f.prompts.length > 1 && (
                  <>
                    <dt>Also called</dt>
                    <dd>{f.prompts.slice(1).join(", ")}</dd>
                  </>
                )}
                <dt>Firmware</dt>
                <dd>
                  {f.callback ? "Handled by a firmware callback" : "Plain stored value"}
                  {f.reset_required ? ", needs a reset" : ""}
                  {f.condition_count
                    ? `, ${f.condition_count} visibility condition${f.condition_count > 1 ? "s" : ""}`
                    : ""}
                </dd>
              </dl>
            ))}
          </>
        )}

        <h3>Evidence</h3>
        <ul className={css.evidence}>
          {x.evidence.map((e, i) => (
            <li key={i}>{e}</li>
          ))}
        </ul>
      </>
    );
  }

  function rawDetails(payload: InspectPayload): ReactNode {
    return (
      <>
        <h2>All BIOS fields</h2>
        <p className="quiet">
          Every named field the firmware defines in its Setup and CpuSetup stores, grouped by the BIOS page that shows it, with
          the value this profile holds.
          {app.writing ? " Only reviewed settings can be edited; these are shown for reference." : ""}
        </p>
        {hasDefaults && (
          <p className="quiet">
            {rawDiffers} of {payload.raw_fields?.length ?? 0} fields differ from the optimized defaults. Some are values the firmware
            fills in itself, such as detected or trained settings, not only your own changes.
          </p>
        )}
        <dl className={css.ev}>
          <dt>Setup module</dt>
          <dd>{payload.bios.setup_module}</dd>
          <dt>Defaults</dt>
          <dd>{payload.bios.defaults?.source ?? "—"}</dd>
          <dt>Image</dt>
          <dd className="mono">{shortHash(payload.bios.sha256)}</dd>
          <dt>Profile</dt>
          <dd className="mono">{shortHash(payload.profile.sha256)}</dd>
          <dt>Checksum</dt>
          <dd className="mono">{payload.profile.checksum_stored}</dd>
        </dl>
      </>
    );
  }

  function rawTable(): ReactNode {
    const shown = rawRows.slice(0, ROW_LIMIT);
    return (
      <div className="tablewrap">
        <table className={`${css.grid} ${css.raw}`}>
          <colgroup>
            <col style={{ width: "36%" }} />
            <col style={{ width: "16%" }} />
            <col style={{ width: "18%" }} />
            <col style={{ width: "16%" }} />
            <col />
          </colgroup>
          <thead>
            <tr>
              <th>BIOS prompt</th>
              <th>Location</th>
              <th>Saved</th>
              <th>Default</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((r, i) => {
              const pageHead = pageBefore(rawRows, i);
              return (
                <Fragment key={`${r.f.store}${r.f.offset}${r.f.prompt}`}>
                  {pageHead && (
                    <tr className={css.pagehead}>
                      <th colSpan={5} scope="colgroup">
                        {pageLabel(pageHead, "Not on a menu page")}
                      </th>
                    </tr>
                  )}
                  <tr className={r.differs ? "heat-3" : ""}>
                    <td title={r.f.prompt}>{r.f.prompt}</td>
                    <td className="mono quiet">
                      {r.f.store}+{hex(r.f.offset)}
                    </td>
                    <td>
                      <span className="mono">{r.f.raw}</span>
                      {r.f.label && ` ${r.f.label}`}
                    </td>
                    <td className="quiet">
                      {r.f.default_raw != null ? (
                        <>
                          <span className="mono">{r.f.default_raw}</span>
                          {r.f.default_label && ` ${r.f.default_label}`}
                        </>
                      ) : (
                        "—"
                      )}
                    </td>
                    <td>{r.differs && <Status tone="mod" text="Not at default" />}</td>
                  </tr>
                </Fragment>
              );
            })}
            {rawRows.length === 0 && (
              <tr>
                <td colSpan={5} className={`quiet empty`}>
                  No fields match.
                </td>
              </tr>
            )}
          </tbody>
        </table>
        <MoreRows total={rawRows.length} />
      </div>
    );
  }

  function settingTable(): ReactNode {
    return (
      <div className="tablewrap">
        <table className={css.grid} aria-label="Settings">
          <colgroup>
            {columns.map(([title, width]) => (
              <col key={title} style={width ? { width } : undefined} />
            ))}
          </colgroup>
          <thead>
            <tr>
              {columns.map(([title]) => (
                <th key={title}>{title}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {shownRows.map((x, i) => {
              const st = statusOf(x);
              const pageHead = spansPages ? pageBefore(rows, i) : null;
              const section = sectionBefore(rows, i);
              return (
                <Fragment key={x.setting_id}>
                  {pageHead && (
                    <tr className={css.pagehead}>
                      <th colSpan={cols} scope="colgroup">
                        {pageLabel(pageHead, "Not in this BIOS")}
                      </th>
                    </tr>
                  )}
                  {section ? (
                    <tr className={css.section}>
                      <th colSpan={cols} scope="colgroup">
                        {section}
                      </th>
                    </tr>
                  ) : null}
                  <SettingRow
                    x={x}
                    index={i}
                    heat={heatOf(x)}
                    selected={selectedId === x.setting_id}
                    staged={staged[x.setting_id]}
                    tone={st.tone}
                    text={st.text}
                    writing={app.writing}
                    onSelect={setSelectedId}
                    onKey={onRowKey}
                  />
                </Fragment>
              );
            })}
            {shownRows.length === 0 && (
              <tr>
                <td colSpan={cols} className={`quiet empty`}>
                  {filter === "edited"
                    ? "No edits yet."
                    : filter === "defaults"
                      ? "Every setting here matches the optimized defaults."
                      : "No settings match this search."}
                </td>
              </tr>
            )}
          </tbody>
        </table>
        <MoreRows total={rows.length} />
      </div>
    );
  }

  const selectedSetting = selected && group !== "raw" ? selected : null;

  // The open pair stays at the top in every mode (table, review, result), so switching never moves it.
  const strip = p ? (
    <>
    <div className="strip" role="group" aria-label="Open pair">
      <span className={`mono strong`}>{p.bios.bios_id}</span>
      <Icon name="arrow" />
      <span className="strong">{fileName(profile)}</span>
      <Status tone="ok" text="Match verified" />
      <span className="quiet">Saved {savedAt(p.profile.saved_at)}</span>
      <span className="quiet">{cpuShort(p.profile.cpu_brand)}</span>
      <span className="spacer"></span>
      {app.writing && !writable && <Status tone="attention" text="Inspect only: image not approved" title={approvalNote} />}
      <button className="btn ghost" onClick={() => onBrowsingChange(bios)}>
        Browse whole BIOS
      </button>
      <button className="btn ghost" onClick={() => setReport(null)}>
        Change files
      </button>
    </div>
    <UntestedNote bioses={[p.bios]} />
    </>
  ) : null;

  let body: ReactNode;
  if (!p) {
    body = (
      <div className={css.open} hidden={!!browsing}>
        <div className={css.intro}>
          <h1>See what an OC profile really stores</h1>
          <p className="quiet">
            Pair a profile with the BIOS image that saved it, or open a BIOS on its own to browse every setting it defines. Settings
            are decoded from the firmware's own definitions, not from the file name.{" "}
            {app.writing ? "Nothing is changed until you export a new file." : "This app only reads: it never changes or writes a file."}
          </p>
        </div>
        {/* The picker stays mounted while browsing so the chosen BIOS is kept. */}
        <PairPicker
          ref={picker}
          heading="Profile to inspect"
          hint="Both files stay on this computer."
          bios={bios}
          onBiosChange={setBios}
          profile={profile}
          onProfileChange={setProfile}
          report={report}
          onReportChange={loaded}
          onBrowse={onBrowsingChange}
          onAnalyze={onAnalyze}
        />
      </div>
    );
  } else if (browsing) {
    // The profile view resumes when browsing closes.
    body = null;
  } else if (mode === "result" && result) {
    body = (
      <>
        {strip}
        <div className="fill scroller">
        <ResultPanel
          result={result}
          onDone={() => {
            setMode("table");
            setResult(null);
            setReport(null);
          }}
        />
        </div>
      </>
    );
  } else {
    body = (
      <>
        {strip}

        {mode === "review" ? (
          <div className="fill scroller">
            <section className={css.review} aria-labelledby="review-title">
              <h2 id="review-title">
                Review {changeCount} {changeCount === 1 ? "change" : "changes"}
              </h2>
              <p className="quiet">
                A new profile is written next to the original, which stays untouched. Only the bytes of these settings and the checksum
                change.
              </p>
              <table className={css.changes}>
                <thead>
                  <tr>
                    <th>Setting</th>
                    <th>Saved</th>
                    <th>New</th>
                  </tr>
                </thead>
                <tbody>
                  {Object.entries(staged).map(([id, v]) => {
                    const setting = p.settings.find((x) => x.setting_id === id);
                    return (
                      <tr key={id}>
                        <td>
                          {setting ? nameOf(setting) : id}
                          {setting?.menu && <span className={`${css.crumb} faint`}>{pageKey(setting.menu.path)}</span>}
                        </td>
                        <td>
                          <ValueText value={setting?.value} strike />
                        </td>
                        <td className="strong">
                          <ValueText value={v} />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              {check.state === "ok" && check.preview ? (
                <>
                  <h3>Dry run</h3>
                  <Checks checks={check.preview.checks} />
                </>
              ) : check.state === "error" ? (
                <div className="problem" role="alert">
                  <Status tone="blocked" text="Cannot export yet" />
                  <p>{check.message}</p>
                </div>
              ) : null}
              {exporter.error && (
                <div className="problem" role="alert">
                  <Status tone="blocked" text="Export failed" />
                  <p>{exporter.error}</p>
                </div>
              )}
              <div className={css.actions}>
                <button className="btn" onClick={() => setMode("table")}>
                  Back to settings
                </button>
                <button
                  className="btn primary"
                  disabled={check.state !== "ok" || exporter.busy || !writable}
                  onClick={() => void exportProfile()}
                >
                  <SwapLabel busy={exporter.busy} idle="Export profile" working="Exporting…" />
                </button>
              </div>
            </section>
          </div>
        ) : (
          <>
            <div className={css.work}>
              <nav className="rail" aria-label="BIOS menu">
                <p className="railhead faint">BIOS menu</p>
                <MenuTree
                  pages={pages}
                  active={!query && filter === "all" ? group : null}
                  onOpen={openPage}
                  count={countCell}
                />
                {absent.length > 0 && (
                  <button
                    className={cx(css.grp, group === NOT_IN_BIOS && !query && filter === "all" && css.on)}
                    onClick={() => openPage(NOT_IN_BIOS)}
                  >
                    <span className={css.title}>Not in this BIOS</span>
                    <span className={`${css.count} mono`}>{absent.length}</span>
                  </button>
                )}
                <div className={css.sep}></div>
                <button
                  className={cx(css.grp, group === "raw" && css.on)}
                  onClick={() => {
                    setGroup("raw");
                    setQuery("");
                    setFilter("all");
                  }}
                >
                  <span className={css.title}>All BIOS fields</span>
                  <span className={`${css.count} mono`}>
                    {rawDiffers ? (
                      <span className={css.modc} title={`${rawDiffers} not at default`}>
                        ≠{rawDiffers}
                      </span>
                    ) : null}
                    {p.raw_fields?.length ?? 0}
                  </span>
                </button>
                <p className={`${css.railnote} faint`}>Every named Setup and CpuSetup field, including ones the BIOS never shows.</p>
              </nav>

              <div className={css.main}>
                <div className={css.tools}>
                  <label className={css.search}>
                    <Icon name="search" />
                    <span className="visually-hidden">Search settings</span>
                    <input
                      ref={searchEl}
                      type="search"
                      placeholder={group === "raw" ? "Search all BIOS fields" : "Search settings"}
                      value={query}
                      onChange={(e) => setQuery(e.target.value)}
                    />
                  </label>
                  <div className="seg" role="radiogroup" aria-label="Show">
                    <label className={cx(filter === "all" && "on")}>
                      <input type="radio" name="flt" value="all" checked={filter === "all"} onChange={() => setFilter("all")} />
                      All
                    </label>
                    <label className={cx(filter === "defaults" && "on", !hasDefaults && "off")}>
                      <input
                        type="radio"
                        name="flt"
                        value="defaults"
                        checked={filter === "defaults"}
                        onChange={() => setFilter("defaults")}
                        disabled={!hasDefaults}
                      />
                      Not at default
                      <span className="mono faint">{group === "raw" ? rawDiffers : differsAll}</span>
                    </label>
                    {group !== "raw" && app.writing && (
                      <label className={cx(filter === "edited" && "on")}>
                        <input type="radio" name="flt" value="edited" checked={filter === "edited"} onChange={() => setFilter("edited")} />
                        Edited <span className="mono faint">{changeCount}</span>
                      </label>
                    )}
                  </div>
                </div>
                {/* Always there (empty when nothing is listed), so filters and searches never move the table. */}
                <div className={css.heatstrip} role="group" aria-label="Settings shown, coloured by distance from optimized defaults">
                  {group !== "raw" &&
                    shownRows.map((x) => {
                      const h = heatOf(x);
                      const text = `${nameOf(x)}: ${HEAT_TEXT[h] ?? "no default to compare"}`;
                      return <HeatCell key={x.setting_id} id={x.setting_id} heat={h} text={text} selected={selectedId === x.setting_id} onSelect={setSelectedId} />;
                    })}
                </div>
                <p className={css.legend}>
                  {hasDefaults ? (
                    <>
                      <span className={css.key}>
                        <i className="heat-0" aria-hidden="true"></i>Stock, Auto
                      </span>
                      <span className={css.key}>
                        <i className="heat-1" aria-hidden="true"></i>Stock, fixed value
                      </span>
                      <span className={css.key}>
                        <i className="heat-3" aria-hidden="true"></i>Off stock
                      </span>
                      {app.writing && (
                        <span className={css.key}>
                          <i className="heat-4" aria-hidden="true"></i>Off stock, edit staged
                        </span>
                      )}
                      <span className="faint" title={p.bios.defaults.source}>
                        Stock = the values “Load Optimized Defaults” restores
                      </span>
                    </>
                  ) : (
                    <span className="faint">
                      No default comparison: {p.bios.defaults?.source ?? "this BIOS image has no readable optimized defaults"}.
                    </span>
                  )}
                </p>

                {group === "raw" ? rawTable() : settingTable()}
              </div>

              <aside className="inspector" aria-label="Setting details">
                {selectedSetting
                  ? settingDetails(selectedSetting)
                  : group === "raw"
                    ? rawDetails(p)
                    : null}
              </aside>
            </div>

            {app.writing && (
              <footer className="bar">
                <div className={css.summary}>
                  {changeCount === 0 ? (
                    <span className="quiet">No changes staged. Select a setting to edit it.</span>
                  ) : (
                    <>
                      <strong>
                        {changeCount} {changeCount === 1 ? "change" : "changes"} staged
                      </strong>
                      {check.state === "checking" ? (
                        <Status text="Checking with the engine…" />
                      ) : check.state === "ok" ? (
                        <Status tone="ok" text="Valid, ready to export" />
                      ) : check.state === "error" ? (
                        <Status tone="blocked" text={check.message} />
                      ) : null}
                    </>
                  )}
                </div>
                <button className="btn ghost" onClick={undo} disabled={cursor === 0} title="Undo (Ctrl+Z)">
                  <Icon name="undo" /> Undo
                </button>
                <button className="btn ghost" onClick={redo} disabled={cursor >= history.length - 1} title="Redo (Ctrl+Shift+Z)">
                  <Icon name="redo" /> Redo
                </button>
                <button className="btn" onClick={revertAll} disabled={changeCount === 0}>
                  Revert all
                </button>
                <button
                  className="btn primary"
                  onClick={() => setMode("review")}
                  disabled={changeCount === 0 || check.state !== "ok" || !writable}
                >
                  Review changes
                </button>
              </footer>
            )}
          </>
        )}
      </>
    );
  }

  return (
    <>
      {browsing ? (
        <Browse
          key={browsing}
          ref={browser}
          bios={browsing}
          closeLabel={p ? "Back to profile" : "Change files"}
          onClose={() => onBrowsingChange(null)}
          onProfile={
            p
              ? undefined
              : () => {
                  onBrowsingChange(null);
                  void picker.current?.chooseProfile();
                }
          }
        />
      ) : null}
      {body}
    </>
  );
}
