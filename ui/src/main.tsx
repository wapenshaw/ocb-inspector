import "@fontsource-variable/oxanium/wght.css";
import "@fontsource-variable/azeret-mono/wght.css";
import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { applyTheme, loadSettings } from "./lib/theme";
import { app as appState } from "./lib/app.svelte";
import { loadAbout } from "./lib/updates.svelte";

// Apply the saved colour mode and writing switch before the first paint, then mount.
const target = document.getElementById("app")!;
void loadSettings().then((s) => {
  applyTheme(s.theme);
  appState.writing = s.writing;
  appState.writingNote = s.writingNote;
  mount(App, { target });
  // After the first paint: the version, then the update check (one request to GitHub).
  void loadAbout();
});
