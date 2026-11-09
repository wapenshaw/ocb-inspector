// Colour mode: light by default whatever the OS uses; dark and "follow the system" are opt-in.
// It is saved with the other settings (state/app.ts); app.css reads data-theme on <html>.
export type ThemeMode = "light" | "dark" | "system";

export const THEME_MODES: { mode: ThemeMode; label: string }[] = [
  { mode: "light", label: "Light" },
  { mode: "dark", label: "Dark" },
  { mode: "system", label: "System" },
];

export const isMode = (v: unknown): v is ThemeMode => THEME_MODES.some((t) => t.mode === v);

export function applyTheme(mode: ThemeMode) {
  document.documentElement.dataset.theme = mode;
}
