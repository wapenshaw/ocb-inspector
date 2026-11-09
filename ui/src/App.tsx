import { useState } from "react";
import { pickFile } from "./api";
import { Inspect } from "./components/Inspect";
import { Migrate } from "./components/Migrate";
import { Compare } from "./components/Compare";
import { About } from "./components/About";
import { Icon } from "./components/Icon";
import { Status } from "./components/Status";
import { useAbout } from "./lib/updates";
import { setTheme, useApp } from "./state/app";
import { cx } from "./lib/cx";
import { THEME_MODES } from "./lib/theme";
import s from "./App.module.css";

type View = "inspect" | "compare" | "migrate" | "about";

const WRITING_NOTE = "Experimental writing is on: Edit and Port can write new profiles. Turn it off under Settings (the version button).";

export function App() {
  const app = useApp();
  const about = useAbout();
  const [view, setView] = useState<View>("inspect");
  const [browsing, setBrowsing] = useState<string | null>(null);

  const current = view !== "inspect" ? view : browsing ? "browse" : "inspect";

  async function goBrowse() {
    setView("inspect");
    if (browsing) return;
    const path = await pickFile("bios");
    if (path) setBrowsing(path);
  }
  function goInspect() {
    setView("inspect");
    setBrowsing(null);
  }

  const tab = (on: boolean) => ({ className: cx(on && s.on), "aria-current": on ? ("page" as const) : undefined });

  return (
    <div className={s.app}>
      <header className={s.top}>
        <div className={s.brand}>
          {/* Same artwork as the window and taskbar icon (generated from public/icon.svg). */}
          <img className={s.logo} src="/icon.svg" width="30" height="30" alt="" />
          <span className={s.title}>
            <span className={s.name}>OCB Inspector</span>
            <span className={s.tagline}>Thermal vision for your BIOS profiles</span>
          </span>
        </div>
        <span className={`${s.offline} quiet`}>
          {app.writing ? (
            <>
              <span className={s.wide}>
                <Status tone="attention" text="Experimental writing on" title={WRITING_NOTE} />
              </span>
              {/* Too narrow for words: the warning sign alone, with the text for screen readers and on hover. */}
              <span className={`${s.narrow} ${s.warn}`} title={WRITING_NOTE}>
                <Icon name="alert" size={15} />
                <span className="visually-hidden">Experimental writing on</span>
              </span>
            </>
          ) : (
            <span className={s.wide} title={app.writingNote}>
              Read-only. Your files never leave this computer.
            </span>
          )}
        </span>
        {/* Version; opens About. Styled like the colour-mode switch beside it, with a badge when a newer release exists. */}
        <div className={`${s.theme} ${s.version}`}>
          <button {...tab(view === "about")} onClick={() => setView("about")}
            title={about.state === "available" ? `Version ${about.newer[0]?.version} is available` : "About OCB Inspector and its changelog"}>
            <span className="mono">v{about.info?.version ?? "…"}</span>
            {about.state === "available" && <span className={s.badge}>Update</span>}
          </button>
        </div>
        <div className={s.theme} role="radiogroup" aria-label="Colour mode">
          {THEME_MODES.map((t) => (
            <button key={t.mode} role="radio" aria-checked={app.theme === t.mode} className={cx(app.theme === t.mode && s.on)} onClick={() => setTheme(t.mode)}>
              {t.label}
            </button>
          ))}
        </div>
        <nav className={s.views} aria-label="Workspace">
          <button {...tab(current === "browse")} onClick={() => void goBrowse()}>Browse BIOS</button>
          <button {...tab(current === "inspect")} onClick={goInspect}>{app.writing ? "Inspect and edit" : "Inspect"}</button>
          <button {...tab(current === "compare")} onClick={() => setView("compare")}>Compare</button>
          {app.writing && (
            <button {...tab(current === "migrate")} onClick={() => setView("migrate")}>Port to another BIOS</button>
          )}
        </nav>
      </header>
      <div className="ramp" aria-hidden="true"></div>
      <main className={s.main}>
        {/* Both views stay mounted so switching keeps their state. */}
        <div className={s.view} hidden={view !== "inspect"}>
          <Inspect browsing={browsing} onBrowsingChange={setBrowsing} active={view === "inspect"} />
        </div>
        <div className={s.view} hidden={view !== "compare"}>
          <Compare />
        </div>
        {app.writing && (
          <div className={s.view} hidden={view !== "migrate"}>
            <Migrate />
          </div>
        )}
        {view === "about" && (
          <div className={s.view}>
            <About />
          </div>
        )}
      </main>
    </div>
  );
}
