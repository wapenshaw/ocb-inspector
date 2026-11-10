// A persistent, calm note under the open-pair strip when the paired BIOS belongs to a board no
// profile has been examined from: the profile layout is then an assumption. Shown for "untested"
// only; "examined" and an absent field show nothing.
import type { BiosSummary } from "../api/types";
import { Status } from "./Status";
import s from "./UntestedNote.module.css";

export const UNTESTED_TEXT =
  "No profile from this board has been examined: the layout is assumed from MS-7D91. Check a few values against your BIOS screens.";

const isUntested = (bios: Pick<BiosSummary, "platform"> | null | undefined): boolean =>
  bios?.platform?.profile_evidence === "untested";

export function UntestedNote({ bioses }: { bioses: (Pick<BiosSummary, "platform"> | null | undefined)[] }) {
  if (!bioses.some(isUntested)) return null;
  return (
    <p className={`quiet ${s.note}`} role="note">
      <Status tone="attention" text="Layout assumed" />
      {UNTESTED_TEXT}
    </p>
  );
}
