import "@fontsource-variable/oxanium/wght.css";
import "@fontsource-variable/azeret-mono/wght.css";
import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { applyTheme, loadSettings } from "./lib/theme";
import { app as appState } from "./lib/app.svelte";

// Apply the saved colour mode and writing switch before the first paint, then mount.
const target = document.getElementById("app")!;
void loadSettings().then((s) => {
  applyTheme(s.theme);
  appState.writing = s.writing;
  appState.writingNote = s.writingNote;
  return mount(App, { target });
});
