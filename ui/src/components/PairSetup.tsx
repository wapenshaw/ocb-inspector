// The first screen of Compare and Port: two pairs (a profile and the BIOS that saved it) chosen
// side by side. `usePair` holds one pair's files and its report.
import { useState, type ReactNode } from "react";
import type { InspectPayload, Report } from "../api/types";
import { PairPicker } from "./PairPicker";
import s from "./PairSetup.module.css";

export function usePair() {
  const [bios, setBios] = useState("");
  const [profile, setProfile] = useState("");
  const [report, setReport] = useState<Report<InspectPayload> | null>(null);
  return { bios, setBios, profile, setProfile, report, setReport };
}

export type Pair = ReturnType<typeof usePair>;

interface Slot {
  heading: string;
  hint: string;
  pair: Pair;
}

export function PairSetup({ title, intro, slots }: { title: string; intro: ReactNode; slots: [Slot, Slot] }) {
  return (
    <div className="fill scroller">
      <div className={s.setup}>
        <div className={s.intro}>
          <h1>{title}</h1>
          <p className="quiet">{intro}</p>
        </div>
        <div className={s.pairs}>
          {slots.map(({ heading, hint, pair }) => (
            <PairPicker
              key={heading}
              heading={heading}
              hint={hint}
              bios={pair.bios}
              onBiosChange={pair.setBios}
              profile={pair.profile}
              onProfileChange={pair.setProfile}
              report={pair.report}
              onReportChange={pair.setReport}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
