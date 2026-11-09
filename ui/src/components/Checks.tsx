import type { Check } from "../api/types";
import { cx } from "../lib/cx";
import { Status } from "./Status";
import s from "./Checks.module.css";

const names: Record<string, string> = {
  reparse_and_pair: "Re-read and paired with the BIOS",
  pair_identity: "Profile matches the BIOS",
  checksum: "Checksum",
  layout_preserved: "Record layout unchanged",
  trailer_preserved: "Clock block and metadata unchanged",
  mutation_audit: "Only intended bytes changed",
  intended_values: "Requested values read back",
  retained_values: "Everything else kept",
  dependency_rules: "Dependent settings consistent",
  store_binding: "Settings stores located",
  lossless_roundtrip: "Lossless re-serialization",
  reviewed_settings_decode: "Reviewed settings decode",
  import_acceptance: "Accepted by the BIOS on import",
};

export function Checks({ checks }: { checks: Check[] }) {
  return (
    <ul className={s.checks}>
      {checks.map((c) => (
        <li key={c.name} className={cx(c.status === "fail" && s.fail)}>
          <span>
            {c.status === "pass" ? (
              <Status tone="ok" text="Passed" />
            ) : c.status === "fail" ? (
              <Status tone="blocked" text="Failed" />
            ) : (
              <Status tone="attention" text="Not tested" />
            )}
          </span>
          <span className={s.name}>{names[c.name] ?? c.name}</span>
          <span className={`${s.detail} quiet`}>{c.detail}</span>
        </li>
      ))}
    </ul>
  );
}
