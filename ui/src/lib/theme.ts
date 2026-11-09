// Colour mode: light by default whatever the OS uses; dark and "follow the system" are opt-in.
// In the desktop app the choice is saved in config/settings.json beside the executable (the app
// is portable and writes nothing elsewhere). The browser preview keeps it in localStorage.
// app.css reads data-theme on <html>.
import { inTauri } from "./api";

export type ThemeMode = "light" | "dark" | "system";

const KEY = "ocb.theme";
export const THEME_MODES: { mode: ThemeMode; label: string }[] = [
  { mode: "light", label: "Light" },
  { mode: "dark", label: "Dark" },
  { mode: "system", label: "System" },
];

const isMode = (v: unknown): v is ThemeMode => v === "light" || v === "dark" || v === "system";

export interface StartupSettings {
  theme: ThemeMode;
  /** Experimental writing switch from config/settings.json. */
  writing: boolean;
  writingNote: string;
}

/** Colour mode and the writing switch. Defaults: light, read-only. */
export async function loadSettings(): Promise<StartupSettings> {
  const out: StartupSettings = { theme: "light", writing: false, writingNote: "" };
  try {
    if (inTauri) {
      const { invoke } = await import("@tauri-apps/api/core");
      const s = await invoke<{ theme?: string | null; writing: boolean; writing_note: string }>("settings_load");
      if (isMode(s.theme)) out.theme = s.theme;
      out.writing = !!s.writing;
      out.writingNote = s.writing_note ?? "";
    } else {
      // Browser preview: localStorage "ocb.writing" = "1" shows the experimental writing UI.
      const v = localStorage.getItem(KEY);
      if (isMode(v)) out.theme = v;
      out.writing = localStorage.getItem("ocb.writing") === "1";
      out.writingNote = "Preview: set localStorage ocb.writing = 1 to show writing.";
    }
  } catch {
    // Unreadable settings: defaults.
  }
  return out;
}

/** The mode currently applied to the page. */
export function currentTheme(): ThemeMode {
  const v = document.documentElement.dataset.theme;
  return isMode(v) ? v : "light";
}

export function applyTheme(mode: ThemeMode) {
  document.documentElement.dataset.theme = mode;
}

export async function saveTheme(mode: ThemeMode) {
  try {
    if (inTauri) {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("settings_save", { settings: { theme: mode } });
    } else {
      localStorage.setItem(KEY, mode);
    }
  } catch {
    // Not saved; the mode still applies for this session.
  }
}
