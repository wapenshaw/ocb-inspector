// App-wide state read once at start-up from config/settings.json beside the program (the app is
// portable and writes nothing elsewhere; the browser preview uses localStorage, see api/mock.ts).
// Writing is experimental and off unless the settings file turns it on; changing it needs a restart.
import { api } from "../api";
import type { SavedSwitches, SettingsChange } from "../api/types";
import { applyTheme, isMode, type ThemeMode } from "../lib/theme";
import { createStore, useStore } from "./store";

export interface AppState {
  theme: ThemeMode;
  /** Edit and Port to another BIOS are available (as read at start-up). */
  writing: boolean;
  /** Why writing is off (shown on hover in read-only mode). */
  writingNote: string;
  /** The settings file, for display. */
  settingsPath: string;
  /** The Settings switches as saved in the file now; they apply at the next start. */
  saved: SavedSwitches;
  /** Why the settings file cannot be read or changed, or empty. */
  settingsError: string;
}

export const appStore = createStore<AppState>({
  theme: "light",
  writing: false,
  writingNote: "",
  settingsPath: "",
  saved: { update_check: true, writing: false },
  settingsError: "",
});

export const useApp = () => useStore(appStore);

/** Read the settings file into the store and apply the colour mode. Unreadable: defaults (light, read-only). */
export async function loadSettings() {
  try {
    const s = await api.settingsLoad();
    appStore.set({
      theme: isMode(s.theme) ? s.theme : "light",
      writing: s.writing,
      writingNote: s.writing_note,
      settingsPath: s.path,
      saved: s.saved,
      settingsError: s.error,
    });
  } catch {
    // Defaults.
  }
  applyTheme(appStore.get().theme);
}

/** Save a Settings switch. Throws when the file cannot be changed. */
export async function saveSwitch(change: Omit<SettingsChange, "theme">) {
  appStore.set({ saved: await api.settingsSave(change) });
}

/** Apply a colour mode now and save it; if saving fails it still applies for this session. */
export function setTheme(mode: ThemeMode) {
  appStore.set({ theme: mode });
  applyTheme(mode);
  api.settingsSave({ theme: mode }).catch(() => {});
}
