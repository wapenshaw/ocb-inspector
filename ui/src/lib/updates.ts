// The app's version and the update check, shared by the header button and the About screen.
// The check is one request for the public release list on GitHub (made by the Rust side); the
// user downloads a new release themselves from its page.
import { api, errorMessage } from "../api";
import type { AppInfo, ReleaseInfo } from "../api/types";
import { createStore, useStore } from "../state/store";

export type UpdateState = "idle" | "off" | "checking" | "current" | "available" | "error";

export interface AboutState {
  info: AppInfo | null;
  state: UpdateState;
  newer: ReleaseInfo[];
  error: string;
  checkedAt: Date | null;
}

export const aboutStore = createStore<AboutState>({ info: null, state: "idle", newer: [], error: "", checkedAt: null });

export const useAbout = () => useStore(aboutStore);

export async function loadAbout(): Promise<void> {
  let info: AppInfo;
  try {
    info = await api.appInfo();
  } catch (e) {
    aboutStore.set({ error: errorMessage(e) });
    return;
  }
  aboutStore.set({ info });
  if (info.update_check) await checkForUpdates();
  else aboutStore.set({ state: "off" });
}

/** Ask GitHub for newer releases. Also used by "Check now", even when the start-up check is off. */
export async function checkForUpdates(): Promise<void> {
  aboutStore.set({ state: "checking", error: "" });
  try {
    const u = await api.checkUpdates();
    aboutStore.set({ newer: u.newer, state: u.newer.length ? "available" : "current", checkedAt: new Date() });
  } catch (e) {
    aboutStore.set({ error: errorMessage(e), state: "error", checkedAt: new Date() });
  }
}

export interface ChangeSection {
  title: string;
  items: string[];
}
export interface ChangeEntry {
  version: string;
  date: string;
  sections: ChangeSection[];
}

/**
 * release-please's changelog Markdown as plain entries: "## [1.2.0](…) (2026-11-20)" starts a
 * version, "### Features" a section, "* **scope:** text ([abc1234](…))" an item. Links, commit
 * hashes and emphasis are dropped; scopes stay as a prefix.
 */
export function parseChangelog(md: string): ChangeEntry[] {
  const out: ChangeEntry[] = [];
  let section: ChangeSection | null = null;
  for (const raw of md.split(/\r?\n/)) {
    const line = raw.trim();
    const ver = line.match(/^##\s+\[?v?(\d+\.\d+\.\d+)\]?(?:\([^)]*\))?\s*(?:\((\d{4}-\d{2}-\d{2})\))?/);
    if (ver) {
      out.push({ version: ver[1]!, date: ver[2] ?? "", sections: [] });
      section = null;
      continue;
    }
    const sec = line.match(/^###\s+(.+)$/);
    if (sec && out.length) {
      section = { title: sec[1]!, items: [] };
      out[out.length - 1]!.sections.push(section);
      continue;
    }
    const item = line.match(/^[*-]\s+(.+)$/);
    if (item && section) {
      const text = item[1]!
        .replace(/\s*\(\[[0-9a-f]{7,}\]\([^)]*\)\)/g, "")
        .replace(/,?\s*closes \[#\d+\]\([^)]*\)/gi, "")
        .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
        .replace(/\*\*([^*]+):\*\*\s*/, "$1: ")
        .replace(/[*_`]/g, "")
        .trim();
      if (text) section.items.push(text);
    }
  }
  return out;
}
