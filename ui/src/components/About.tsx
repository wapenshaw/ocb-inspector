// About: version, the update check's result with the newer releases' notes, and this
// version's own changelog (built in, so it shows offline).
import { Fragment, useMemo } from "react";
import { api } from "../api";
import { checkForUpdates, parseChangelog, useAbout, type ChangeEntry } from "../lib/updates";
import { cx } from "../lib/cx";
import { Settings } from "./Settings";
import { Status } from "./Status";
import s from "./About.module.css";

function when(d: Date | null) {
  return d ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "";
}
function day(iso: string) {
  if (!iso) return "";
  const d = new Date(`${iso}T00:00:00`);
  return isNaN(d.getTime()) ? iso : d.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
}

function Entries({ list, fresh }: { list: ChangeEntry[]; fresh: boolean }) {
  return (
    <ol className={s.log}>
      {list.map((e) => (
        <li key={e.version} className={cx(fresh && s.fresh)}>
          <h3>
            <span className="mono">v{e.version}</span>
            {e.date && <span className="faint">{day(e.date)}</span>}
          </h3>
          {e.sections.map((sec) => (
            <Fragment key={sec.title}>
              <h4>{sec.title}</h4>
              <ul>
                {sec.items.map((item, i) => (
                  <li key={i}>{item}</li>
                ))}
              </ul>
            </Fragment>
          ))}
        </li>
      ))}
    </ol>
  );
}

export function About() {
  const about = useAbout();
  const history = useMemo(() => (about.info ? parseChangelog(about.info.changelog) : []), [about.info]);
  const upcoming = useMemo(
    () =>
      about.newer.flatMap((r) => {
        const parsed = parseChangelog(r.notes);
        return parsed.length ? parsed : [{ version: r.version, date: r.published_at.slice(0, 10), sections: [] }];
      }),
    [about.newer],
  );
  const latest = about.newer[0] ?? null;
  const repo = about.info?.repo_url ?? "https://github.com/wapenshaw/ocb-inspector";

  return (
    <div className="fill scroller">
      <div className={s.page}>
        <div className={s.top}>
          <header className={s.id}>
            <img src="/icon.svg" width="72" height="72" alt="" />
            <div>
              <h1>OCB Inspector</h1>
              <p className="quiet">Thermal vision for your BIOS profiles</p>
              <p className={s.ver}>
                <span className="mono">v{about.info?.version ?? "…"}</span>
                <span className="faint">MIT licence</span>
              </p>
            </div>
          </header>
          <section className={s.update} aria-live="polite">
            <div className={s.state}>
              {about.state === "checking" || about.state === "idle" ? (
                <>
                  <Status tone="neutral" text="Checking GitHub for a newer version…" />
                  <p className="quiet">One request for the public list of releases.</p>
                </>
              ) : about.state === "available" && latest ? (
                <>
                  <Status tone="attention" text={`Version ${latest.version} is available`} />
                  <p className="quiet">
                    {about.newer.length === 1 ? "One release" : `${about.newer.length} releases`} since yours. Download the zip
                    from the release page and unzip it over this folder; your <span className="mono">config</span> folder stays
                    as it is.
                  </p>
                </>
              ) : about.state === "current" ? (
                <>
                  <Status tone="ok" text="You have the latest version" />
                  <p className="quiet">Checked at {when(about.checkedAt)}.</p>
                </>
              ) : about.state === "error" ? (
                <>
                  <Status tone="blocked" text="Could not check for updates" />
                  <p className="quiet">{about.error}</p>
                </>
              ) : about.state === "off" ? (
                <>
                  <Status tone="neutral" text="Update check is off" />
                  <p className="quiet">
                    Turned off in Settings. You can still check now.
                  </p>
                </>
              ) : null}
            </div>
            <div className={s.actions}>
              {latest && (
                <button className="btn primary" onClick={() => void api.openPage(latest.url)}>
                  Open the v{latest.version} release
                </button>
              )}
              <button className="btn" onClick={() => void checkForUpdates()} disabled={about.state === "checking"}>
                Check now
              </button>
            </div>
          </section>
        </div>

        <div className={s.cols}>
          <div className={s.main}>
            <section className={s.fine}>
              <h2>About this app</h2>
              <p>
                OCB Inspector reads MSI BIOS OC profiles and BIOS images on this computer. Your files are never uploaded. The one
                thing it sends over the network is the update check: a request for the public list of releases on GitHub, which
                carries nothing about you or your files. Turn it off in Settings.
              </p>
              <p>
                Free and open source under the MIT licence. The third-party code inside the app and its licences are listed in{" "}
                <span className="mono">THIRD-PARTY-NOTICES.txt</span> beside the program.
              </p>
              <p>
                <button className={s.link} onClick={() => void api.openPage(repo)}>
                  Source code, issues and profile reports on GitHub
                </button>
              </p>
            </section>

            {upcoming.length > 0 && (
              <section>
                <h2>What's new since v{about.info?.version}</h2>
                <Entries list={upcoming} fresh />
              </section>
            )}

            <section>
              <h2>Changelog</h2>
              {history.length ? <Entries list={history} fresh={false} /> : <p className="quiet">No changelog in this build.</p>}
              <p className={s.more}>
                <button className={s.link} onClick={() => void api.openPage(`${repo}/releases`)}>
                  All releases on GitHub
                </button>
              </p>
            </section>
          </div>
          <aside className={s.side}>
            <Settings />
          </aside>
        </div>
      </div>
    </div>
  );
}
