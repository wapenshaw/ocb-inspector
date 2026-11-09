import { useEffect, useMemo, useRef, useState } from "react";
import { pageKey } from "../lib/menu";
import type { Page } from "../lib/menu";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import s from "./MenuTree.module.css";

export interface MenuTreeProps {
  pages: Page[];
  /** Key of the open page, or null when none is (searching, filtering). */
  active: string | null;
  onOpen: (key: string) => void;
  count?: (pg: Page) => React.ReactNode;
}

const ancestors = (pg: Page) => pg.path.slice(0, -1).map((_, i) => pageKey(pg.path.slice(0, i + 1)));

// BIOS menu tree for the Inspect and Browse rails, drawn as a board trace: one trunk, each
// child page branching off it to its own via. Branches collapse; the open page's branch
// always shows.
export function MenuTree({ pages, active, onOpen, count }: MenuTreeProps) {
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());

  const hasKids = useMemo(
    () => new Set(pages.filter((pg, i) => (pages[i + 1]?.depth ?? -1) > pg.depth).map((pg) => pg.key)),
    [pages],
  );
  const visible = useMemo(
    () => pages.filter((pg) => ancestors(pg).every((k) => expanded.has(k))),
    [pages, expanded],
  );

  // Top-level branches start open, deeper ones folded. Each branch is opened by default only the
  // first time it appears, so later list changes (e.g. showing hidden fields) keep the user's folds.
  const seeded = useRef(new Set<string>());
  useEffect(() => {
    const fresh: string[] = [];
    for (const pg of pages) {
      if (pg.depth !== 0 || !hasKids.has(pg.key) || seeded.current.has(pg.key)) continue;
      seeded.current.add(pg.key);
      fresh.push(pg.key);
    }
    if (fresh.length > 0) setExpanded((prev) => new Set([...prev, ...fresh]));
  }, [pages, hasKids]);

  // Opening a page (from search, a link or the first load) unfolds the branch it is on.
  useEffect(() => {
    const pg = active ? pages.find((p) => p.key === active) : undefined;
    if (!pg) return;
    const keys = ancestors(pg);
    setExpanded((prev) => (keys.every((k) => prev.has(k)) ? prev : new Set([...prev, ...keys])));
  }, [active, pages]);

  function toggle(key: string) {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  return (
    <ul className={s.tree}>
      {visible.map((pg) => {
        const kids = hasKids.has(pg.key);
        const open = expanded.has(pg.key);
        const on = active === pg.key;
        return (
          <li
            key={pg.key}
            className={cx(s.node, on && s.on, pg.heading && s.heading)}
            style={{ "--depth": pg.depth } as React.CSSProperties}
          >
            {kids ? (
              <button
                className={cx(s.twist, open && s.open)}
                aria-expanded={open}
                aria-label={`${open ? "Collapse" : "Expand"} ${pg.title}`}
                onClick={() => toggle(pg.key)}
              >
                <Icon name="chevron" size={14} />
              </button>
            ) : (
              <span className={s.twist} aria-hidden="true"></span>
            )}
            {pg.heading ? (
              <button
                className={s.row}
                aria-expanded={kids ? open : undefined}
                onClick={() => {
                  if (kids) toggle(pg.key);
                }}
              >
                <span className={s.title}>{pg.title}</span>
              </button>
            ) : (
              <button className={s.row} aria-current={on ? "page" : undefined} onClick={() => onOpen(pg.key)}>
                <span className={s.title}>{pg.title || "Setup"}</span>
                {count && <span className={`${s.count} mono`}>{count(pg)}</span>}
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
