import "@fontsource-variable/oxanium/wght.css";
import "@fontsource-variable/azeret-mono/wght.css";
import "./app.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { loadSettings } from "./state/app";
import { loadAbout } from "./lib/updates";

// Apply the saved colour mode and writing switch before the first paint, then render.
const root = createRoot(document.getElementById("root")!);
void loadSettings().then(() => {
  root.render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
  // After the first paint: the version, then the update check (one request to GitHub). It runs
  // here, outside React, so it happens once even where StrictMode repeats effects.
  void loadAbout();
});
