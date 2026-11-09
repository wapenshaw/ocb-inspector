import { Fragment, useEffect, useImperativeHandle, useMemo, useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";
import { api, errorMessage } from "../api";
import type { BiosField, BrowsePayload, Report } from "../api/types";
import { fileName, hex, shortHash } from "../lib/format";
import { menuPages, menuSort, pageBefore, pageKey, sectionBefore, type Page, pageLabel } from "../lib/menu";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import { MenuTree } from "./MenuTree";
import { moveRow } from "../lib/rows";
import { MoreRows } from "./MoreRows";
import { Status } from "./Status";
import css from "./Browse.module.css";

// A BIOS image on its own: every setting its menus define, with the optimized default.
// No profile is needed, so this works for every family, including browse-only ones.

export interface BrowseHandle {
  focusSearch(): void;
}

interface BrowseProps {
  bios: string;
  onClose: () => void;
  /** Offered when no profile is open; shown only if profiles from this BIOS can be opened. */
  onProfile?: () => void;
  closeLabel: string;
  ref?: Ref<BrowseHandle>;
}

type Row = BiosField & { key: string; menu: { path: string[]; section: string | null; order: number[] } };

function rowsOf(p: BrowsePayload, hidden: boolean): Row[] {
  return p.fields
    .filter((f) => hidden || !f.hidden)
    .map((f) => ({
      ...f,
      key: `${f.formset}:${f.store}:${f.offset}:${f.width_bits}:${f.prompt}`,
      menu: { path: f.menu_path, section: f.section, order: f.menu_order },
    }));
}

function defaultText(f: BiosField): string {
  if (f.default_raw === null || f.default_raw === undefined) return "";
  return f.default_label ?? String(f.default_raw);
}

function allowed(f: BiosField): string {
  if (f.kind === "checkbox") return "Enabled or Disabled";
  if (f.kind === "one_of") return `${f.options.length} ${f.options.length === 1 ? "choice" : "choices"}`;
  if (f.min !== null && f.max !== null) return `${f.min} to ${f.max}${f.step && f.step > 1 ? `, step ${f.step}` : ""}`;
  return "Number";
}

const LIMIT = 600;

export function Browse({ bios, onClose, onProfile, closeLabel, ref }: BrowseProps) {
  const [report, setReport] = useState<Report<BrowsePayload> | null>(null);
  const [loadError, setLoadError] = useState("");
  const [group, setGroup] = useState("");
  const [query, setQuery] = useState("");
  const [showHidden, setShowHidden] = useState(false);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  /** A different firmware set is loading; the current one stays on screen. */
  const [switching, setSwitching] = useState(false);
  const openRef = useRef({ group: "", selected: null as string | null });
  openRef.current = { group, selected: selectedKey };
  /** Firmware set to show (images with several complete sets). */
  const [set, setSet] = useState(0);
  const searchEl = useRef<HTMLInputElement>(null);

  useImperativeHandle(ref, () => ({
    focusSearch() {
      searchEl.current?.focus();
    },
  }));

  // A new image remounts this component (keyed by path), so this runs for the first load and for
  // each switch of firmware set.
  useEffect(() => {
    // Switching sets keeps the current view (dimmed) until the other set arrives, so the menu,
    // table and set picker stay where they are.
    const switchingSet = report !== null;
    // A set switched while a read is in flight: ignore the stale answer.
    let stale = false;
    setSwitching(switchingSet);
    setLoadError("");
    api
      .browse(bios, set)
      .then((r) => {
        if (stale) return;
        setReport(r);
        setSwitching(false);
        const sorted = menuSort(rowsOf(r.payload, false));
        // Switching sets keeps the open page and setting when the other set has them.
        if (switchingSet && sorted.some((x) => pageKey(x.menu.path) === openRef.current.group)) {
          if (!sorted.some((x) => x.key === openRef.current.selected)) setSelectedKey(null);
          return;
        }
        // Start where tuning lives when the BIOS has an Overclocking menu.
        const first = sorted.find((x) => x.menu.path[0] === "Overclocking") ?? sorted[0];
        setGroup(first ? pageKey(first.menu.path) : "");
        setSelectedKey(first?.key ?? null);
      })
      .catch((e: unknown) => {
        if (stale) return;
        setSwitching(false);
        setLoadError(errorMessage(e));
      });
    return () => {
      stale = true;
    };
  }, [bios, set]);

  const p = report?.payload ?? null;
  const plat = p?.bios.platform ?? null;
  // Sorted once per report; each keystroke only filters (order is kept).
  const all = useMemo(() => (p ? menuSort(rowsOf(p, showHidden)) : []), [p, showHidden]);
  const hiddenCount = p?.fields.filter((f) => f.hidden).length ?? 0;
  const pages = useMemo<Page[]>(() => menuPages(all), [all]);
  const counts = useMemo(() => {
    const m = new Map<string, number>();
    for (const r of all) m.set(pageKey(r.menu.path), (m.get(pageKey(r.menu.path)) ?? 0) + 1);
    return m;
  }, [all]);

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return all.filter((r) =>
      q ? `${r.prompt} ${r.menu.path.join(" ")} ${r.store} ${r.help}`.toLowerCase().includes(q) : pageKey(r.menu.path) === group,
    );
  }, [all, query, group]);
  const spansPages = new Set(rows.map((r) => pageKey(r.menu.path))).size > 1;
  const shown = rows.slice(0, LIMIT);

  // The same prompt more than once on a page at different offsets: alternatives the BIOS picks
  // between at runtime (AMD boards carry one copy per processor family).
  const variants = useMemo(() => {
    const by = new Map<string, Row[]>();
    for (const r of all) {
      const k = `${pageKey(r.menu.path)}|${r.prompt}`;
      by.set(k, [...(by.get(k) ?? []), r]);
    }
    const out = new Map<string, { n: number; of: number }>();
    for (const list of by.values()) {
      if (list.length < 2) continue;
      list.forEach((r, i) => out.set(r.key, { n: i + 1, of: list.length }));
    }
    return out;
  }, [all]);

  const selected = all.find((r) => r.key === selectedKey) ?? null;
  const formset = selected && p ? (p.formsets[selected.formset] ?? null) : null;
  const selectedVariant = selected ? variants.get(selected.key) : undefined;

  function openPage(key: string) {
    setGroup(key);
    setQuery("");
  }

  function rowKey(e: KeyboardEvent, idx: number) {
    moveRow(e, rows, idx, (n) => setSelectedKey(n.key), (n) => `b-${n.key}`);
  }

  function renderTable(): ReactNode {
    if (!p) return null;
    return (
      <div className="tablewrap">
        <table className={css.grid} aria-label="BIOS settings">
          <colgroup>
            <col style={{ width: "40%" }} />
            <col style={{ width: "20%" }} />
            <col style={{ width: "20%" }} />
            <col />
          </colgroup>
          <thead>
            <tr>
              <th>Setting</th>
              <th>Optimized default</th>
              <th>Allowed</th>
              <th>Stored at</th>
            </tr>
          </thead>
          <tbody>
            {shown.length === 0 && (
              <tr>
                <td colSpan={4} className={`quiet empty`}>
                  {query ? "No setting matches this search." : "This page has no settings."}
                </td>
              </tr>
            )}
            {shown.map((r, i) => {
              const pageHead = spansPages ? pageBefore(rows, i) : null;
              const section = sectionBefore(rows, i);
              const v = variants.get(r.key);
              return (
                <Fragment key={r.key}>
                  {pageHead && (
                    <tr className={css.pagehead}>
                      <th colSpan={4} scope="colgroup">
                        {pageLabel(pageHead)}
                      </th>
                    </tr>
                  )}
                  {section && (
                    <tr className={css.section}>
                      <th colSpan={4} scope="colgroup">
                        {section}
                      </th>
                    </tr>
                  )}
                  <tr
                    id={`b-${r.key}`}
                    tabIndex={0}
                    className={cx(selectedKey === r.key && css.sel, r.hidden && css.dim)}
                    aria-selected={selectedKey === r.key}
                    onClick={() => setSelectedKey(r.key)}
                    onKeyDown={(e) => rowKey(e, i)}
                  >
                    <td title={r.prompt}>
                      {r.prompt}
                      {v && <span className={`${css.tag} faint`}>{v.n} of {v.of}</span>}
                      {r.hidden && <span className={`${css.tag} faint`}>hidden</span>}
                    </td>
                    <td>{defaultText(r) || <span className="faint">—</span>}</td>
                    <td className="quiet">{allowed(r)}</td>
                    <td className="mono quiet">
                      {r.store}+{hex(r.offset)}
                    </td>
                  </tr>
                </Fragment>
              );
            })}
          </tbody>
        </table>
        <MoreRows total={rows.length} limit={LIMIT} />
      </div>
    );
  }

  return (
    <div className="fill col">
      <div className="strip" role="group" aria-label="Open BIOS">
        <span className={`mono strong`}>{p?.bios.bios_id ?? fileName(bios)}</span>
        {plat && (
          <>
            <span className="strong">{plat.version}</span>
            <span className="quiet">
              {plat.family}
              {plat.chipset ? `, ${plat.chipset}` : ""}
            </span>
            {plat.models && (
              <span className={`quiet ${css.models}`} title={plat.models}>
                {plat.models}
              </span>
            )}
          </>
        )}
        <span className="spacer"></span>
        {p && p.sets.length > 1 && (
          <label className={css.setpick} title={p.sets[p.set]?.note ?? ""}>
            <span className="faint">Firmware set</span>
            <select value={set} onChange={(e) => setSet(Number(e.currentTarget.value))}>
              {p.sets.map((info, i) => (
                <option key={i} value={i}>
                  {i + 1}: {info.label}
                </option>
              ))}
            </select>
          </label>
        )}
        {p &&
          (onProfile && p.bios.profiles_supported !== false ? (
            <button className="btn" onClick={onProfile}>
              Open a profile with this BIOS
            </button>
          ) : (
            <Status tone="neutral" text="Browse only" title={plat?.profiles_note ?? ""} />
          ))}
        <button className="btn ghost" onClick={onClose}>
          {closeLabel}
        </button>
      </div>

      {loadError ? (
        <div className={css.pad}>
          <div className="problem" role="alert">
            <Status tone="blocked" text="Could not open" />
            <p>{loadError}</p>
          </div>
        </div>
      ) : !p ? (
        <div className={`${css.pad} quiet`}>Reading every menu in {fileName(bios)}…</div>
      ) : (
        <div className={css.work} aria-busy={switching || undefined} style={switching ? { opacity: 0.6 } : undefined}>
          <nav className="rail" aria-label="BIOS menu">
            <p className="railhead faint">BIOS menu</p>
            <MenuTree
              pages={pages}
              active={query ? null : group}
              onOpen={openPage}
              count={(pg: Page) => counts.get(pg.key) ?? 0}
            />
          </nav>

          <div className={css.main}>
            <div className={css.tools}>
              <label className={css.search}>
                <Icon name="search" />
                <span className="visually-hidden">Search every setting</span>
                <input
                  ref={searchEl}
                  type="search"
                  placeholder="Search every setting in this BIOS"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
              </label>
              {hiddenCount ? (
                <label className={css.check}>
                  <input type="checkbox" checked={showHidden} onChange={(e) => setShowHidden(e.target.checked)} />
                  Include fields the BIOS never shows <span className="mono faint">{hiddenCount}</span>
                </label>
              ) : null}
            </div>
            <p className={`${css.legend} quiet`}>
              {p.bios.defaults.available ? (
                "Defaults are the values “Load Optimized Defaults” restores, read from the image."
              ) : (
                <span className="faint">No optimized defaults: {p.bios.defaults.source}.</span>
              )}
            </p>

            {renderTable()}
          </div>

          <aside className="inspector" aria-label="Setting details">
            {selected && (
              <>
                <p className={`${css.crumb} faint`}>
                  {pageLabel(selected.menu.path)}
                  {selected.section ? `, ${selected.section}` : ""}
                </p>
                <h2>{selected.prompt}</h2>

                <div className={css.defbox}>
                  {defaultText(selected) ? (
                    <>
                      <span className="faint">Optimized default</span>
                      <strong>{defaultText(selected)}</strong>
                    </>
                  ) : (
                    <span className="faint">No optimized default in the image for the {selected.store} store.</span>
                  )}
                </div>

                {selected.help && (
                  <>
                    <h3>BIOS help</h3>
                    <p className={css.bioshelp}>{selected.help}</p>
                  </>
                )}

                {selected.kind === "one_of" ? (
                  <>
                    <h3>Choices</h3>
                    <ul className={css.choices}>
                      {selected.options.map((o) => (
                        <li key={o.raw} className={cx(o.raw === selected.default_raw && css.def)}>
                          <span>{o.label || "(no label)"}</span>
                          <span className="mono faint">{o.raw}</span>
                        </li>
                      ))}
                    </ul>
                  </>
                ) : selected.kind === "numeric" ? (
                  <>
                    <h3>Range</h3>
                    <p className="quiet">{allowed(selected)}</p>
                  </>
                ) : null}

                {selectedVariant && (
                  <p className={`${css.note} quiet`}>
                    This page defines “{selected.prompt}” {selectedVariant.of} times at different locations. The BIOS shows one of
                    them depending on other settings or the installed processor; this is copy {selectedVariant.n}.
                  </p>
                )}
                {selected.hidden && <p className={`${css.note} quiet`}>The firmware defines this field but never displays it.</p>}

                <h3>Where it is stored</h3>
                <dl className={css.ev}>
                  <dt>Location</dt>
                  <dd>
                    <span className="mono">
                      {selected.store}+{hex(selected.offset)}
                    </span>
                    , {selected.width_bits} bits
                  </dd>
                  <dt>Firmware</dt>
                  <dd>
                    {[
                      selected.callback ? "handled by a firmware callback" : "plain stored value",
                      selected.reset_required ? "needs a reset" : "",
                      selected.read_only ? "read only" : "",
                      selected.conditional ? "shown or enabled depending on other settings" : "",
                    ]
                      .filter(Boolean)
                      .join(", ")}
                  </dd>
                  {formset && (
                    <>
                      <dt>Defined in</dt>
                      <dd>
                        {formset.title || "Setup"} <span className="faint">({formset.module})</span>
                      </dd>
                    </>
                  )}
                </dl>
                {formset?.placement === "title_match" && (
                  <p className={`${css.note} quiet`}>
                    The BIOS does not link {formset.title} from its menus. It is shown under {pageKey(formset.placed_under)} because
                    that page has the same title.
                  </p>
                )}
                {formset?.note && <p className={`${css.note} quiet`}>{formset.note}</p>}
              </>
            )}

            <h3>This BIOS</h3>
            <dl className={css.ev}>
              {plat && (
                <>
                  <dt>Family</dt>
                  <dd>
                    {plat.family}
                    {plat.chipset ? `, ${plat.chipset}` : ""}
                  </dd>
                  {plat.models && (
                    <>
                      <dt>Boards</dt>
                      <dd>{plat.models}</dd>
                    </>
                  )}
                  <dt>Version</dt>
                  <dd>
                    {plat.version} <span className="faint">({p?.bios.revision}, built {p?.bios.build_date})</span>
                  </dd>
                  <dt>Parser</dt>
                  <dd className="mono">{plat.parser}</dd>
                  <dt>Profiles</dt>
                  <dd>{plat.profiles_note}</dd>
                </>
              )}
              {p && p.sets.length > 1 && (
                <>
                  <dt>Firmware set</dt>
                  <dd>
                    {p.set + 1} of {p.sets.length}: {p.sets[p.set]?.note}
                  </dd>
                </>
              )}
              <dt>Settings</dt>
              <dd>
                {p ? p.fields.length - hiddenCount : 0} shown{hiddenCount ? `, ${hiddenCount} hidden` : ""}
              </dd>
              <dt>Defaults</dt>
              <dd>{p?.bios.defaults.source}</dd>
              <dt>Image</dt>
              <dd className="mono">{shortHash(p?.bios.sha256 ?? "")}</dd>
            </dl>
          </aside>
        </div>
      )}
    </div>
  );
}
