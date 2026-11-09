// Group settings the way the BIOS does: by menu page, in menu order, with section headings.
// Everything here is presentation; the engine supplies paths and order keys.

export interface Located {
  menu: { path: string[]; section: string | null; order: number[] } | null;
}

export interface Page {
  /** Stable key, the joined path. */
  key: string;
  path: string[];
  title: string;
  depth: number;
  /** True for ancestors shown only to keep the tree readable (no settings of their own). */
  heading: boolean;
}

export const NOT_IN_BIOS = "__absent";

export const pageKey = (path: string[]) => path.join(" › ");
/** A page's path for display, or `empty` when the setting is on no page. */
export const pageLabel = (path: string[], empty = "Setup") => (path.length ? pageKey(path) : empty);

function cmp(a: number[], b: number[]): number {
  for (let i = 0; i < Math.min(a.length, b.length); i++) if (a[i] !== b[i]) return a[i]! - b[i]!;
  return a.length - b.length;
}

/** Sort key of a page: the smallest order key among its settings. */
function pageOrders<T extends Located>(items: T[]): Map<string, number[]> {
  const m = new Map<string, number[]>();
  for (const s of items) {
    if (!s.menu) continue;
    const k = pageKey(s.menu.path);
    const prev = m.get(k);
    if (!prev || cmp(s.menu.order, prev) < 0) m.set(k, s.menu.order);
  }
  return m;
}

/** Items sorted page by page (pages in menu order), then by position on the page. */
export function menuSort<T extends Located>(items: T[]): T[] {
  const po = pageOrders(items);
  // Each item's page order, looked up once rather than per comparison.
  const page = new Map<T, number[]>();
  for (const s of items) if (s.menu) page.set(s, po.get(pageKey(s.menu.path)) ?? []);
  return [...items].sort((a, b) => {
    if (!a.menu || !b.menu) return a.menu ? -1 : b.menu ? 1 : 0;
    return cmp(page.get(a)!, page.get(b)!) || cmp(a.menu.order, b.menu.order);
  });
}

/** Pages that hold settings, as an indented tree in menu order. */
export function menuPages<T extends Located>(items: T[]): Page[] {
  const po = pageOrders(items);
  const keys = [...po.keys()].sort((a, b) => cmp(po.get(a)!, po.get(b)!));
  const paths = new Map<string, string[]>();
  for (const s of items) if (s.menu) paths.set(pageKey(s.menu.path), s.menu.path);
  const out: Page[] = [];
  const seen = new Set<string>();
  for (const k of keys) {
    const path = paths.get(k)!;
    // Show missing ancestors as headings so a child never floats without its parent.
    for (let d = 1; d < path.length; d++) {
      const ak = pageKey(path.slice(0, d));
      if (!seen.has(ak) && !po.has(ak)) {
        seen.add(ak);
        out.push({ key: ak, path: path.slice(0, d), title: path[d - 1]!, depth: d - 1, heading: true });
      }
    }
    seen.add(k);
    out.push({ key: k, path, title: path[path.length - 1] ?? "Setup", depth: Math.max(0, path.length - 1), heading: false });
  }
  return out;
}

/** Section heading to print before item i of an already-sorted list, if it starts a new section. */
export function sectionBefore<T extends Located>(list: T[], i: number): string | null {
  const s = list[i]?.menu ?? null;
  if (!s?.section) return null;
  const prev = i > 0 ? (list[i - 1]?.menu ?? null) : null;
  const samePage = prev && pageKey(prev.path) === pageKey(s.path);
  return samePage && prev.section === s.section ? null : s.section;
}

/** Page heading to print before item i (when a list spans several pages). */
export function pageBefore<T extends Located>(list: T[], i: number): string[] | null {
  const s = list[i]?.menu ?? null;
  const prev = i > 0 ? (list[i - 1]?.menu ?? null) : null;
  if (!s) return prev || i === 0 ? [] : null;
  return prev && pageKey(prev.path) === pageKey(s.path) ? null : s.path;
}
