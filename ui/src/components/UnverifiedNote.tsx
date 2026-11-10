// A persistent note under the open-pair strip when the pair was read by the experimental reader: the
// values use BIOS labels; recipe readings carry their own board evidence qualifier.
import type { UnverifiedView } from "../lib/unverified";
import { Icon } from "./Icon";
import s from "./UnverifiedNote.module.css";

export function unverifiedNoteText(v: Pick<UnverifiedView, "recordsWithSettings" | "recordsTotal" | "noDefault">): string {
  return [
    "Values use the BIOS labels, with additional recipe readings where available; mapping evidence and profile integrity are reported separately.",
    `${v.recordsWithSettings} of ${v.recordsTotal} profile records were tied to settings.`,
    v.noDefault > 0
      ? `${v.noDefault} ${v.noDefault === 1 ? "setting has" : "settings have"} no BIOS default to compare with; add a baseline profile under “All settings, baseline and sharing” to compare those.`
      : "",
  ]
    .filter(Boolean)
    .join(" ");
}

export function UnverifiedNote({ view }: { view: UnverifiedView }) {
  return (
    <p className={`quiet ${s.note}`} role="note">
      <Icon name="alert" size={15} />
      {unverifiedNoteText(view)}
    </p>
  );
}
