// The switches in config/settings.json beside the program. Each change is saved at once and keeps
// every other key in the file; the app reads the file at start, so changes apply after a restart.
import { useState } from "react";
import type { SettingsChange } from "../api/types";
import { saveSwitch, useApp } from "../state/app";
import { useAbout } from "../lib/updates";
import { useAction } from "../lib/useAction";
import { Status } from "./Status";
import s from "./Settings.module.css";

export function Settings() {
  const app = useApp();
  const about = useAbout();
  const saving = useAction();
  const [confirmWriting, setConfirmWriting] = useState(false);

  const blocked = !!app.settingsError;
  const startedWithChecks = about.info?.update_check ?? true;
  const restart = app.saved.writing !== app.writing;

  async function save(change: Omit<SettingsChange, "theme">) {
    await saving.run(() => saveSwitch(change));
    setConfirmWriting(false);
  }

  function toggleWriting(on: boolean) {
    if (on) setConfirmWriting(true);
    else void save({ writing: false });
  }

  return (
    <section className={s.settings} aria-labelledby="settings-title">
      <h2 id="settings-title">Settings</h2>
      <div className={s.card}>
        <label className={s.row}>
          <input
            type="checkbox"
            checked={app.saved.update_check}
            disabled={blocked || saving.busy}
            onChange={(e) => void save({ update_check: e.currentTarget.checked })}
          />
          <span className={s.text}>
            <span className={s.name}>Check for updates when the app starts</span>
            <span className="quiet">
              One request for the public list of releases on GitHub. "Check now" works either way.
              {!startedWithChecks && app.saved.update_check && " Starts with the next launch."}
            </span>
          </span>
        </label>

        <label className={s.row}>
          <input
            type="checkbox"
            checked={app.saved.writing || confirmWriting}
            disabled={blocked || saving.busy || confirmWriting}
            onChange={(e) => toggleWriting(e.currentTarget.checked)}
          />
          <span className={s.text}>
            <span className={s.name}>Experimental writing</span>
            <span className="quiet">
              Lets Edit and Port to another BIOS write new profiles, for BIOS images listed in{" "}
              <span className="mono">config/approved-images.toml</span>. Off by default.
            </span>
            {restart && (
              <Status tone="attention" text={`Restart OCB Inspector to turn writing ${app.saved.writing ? "on" : "off"}`} />
            )}
          </span>
        </label>

        {confirmWriting && (
          <div className={s.confirm} role="alertdialog" aria-labelledby="confirm-writing">
            <p id="confirm-writing">
              Writing profiles is experimental. The app checks every file it writes, but only loading it on your motherboard
              shows whether the BIOS accepts it. Your original files are never changed.
            </p>
            <div className={s.buttons}>
              <button className="btn primary" disabled={saving.busy} onClick={() => void save({ writing: true })}>
                Turn on writing
              </button>
              <button className="btn" disabled={saving.busy} onClick={() => setConfirmWriting(false)}>
                Cancel
              </button>
            </div>
          </div>
        )}

        <p className={`${s.foot} faint`}>
          Saved in <span className="mono">{app.settingsPath || "config/settings.json"}</span>, beside the program. Colour mode is
          the switch in the header.
        </p>
        {(saving.error || app.settingsError) && <Status tone="blocked" text={saving.error || app.settingsError} />}
      </div>
    </section>
  );
}
