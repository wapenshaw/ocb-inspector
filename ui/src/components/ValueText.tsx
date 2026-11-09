import type { Value } from "../api/types";
import { parts } from "../lib/format";
import { cx } from "../lib/cx";
import s from "./ValueText.module.css";

// Numbers in monospace, units and labels in the UI face; Auto is a tag, not a number.
export function ValueText({ value, strike = false }: { value: Value | null | undefined; strike?: boolean }) {
  const p = parts(value);
  return (
    <span className={cx(s.v, p.auto && s.auto, strike && s.strike)}>
      {p.num && <span className="mono">{p.num}</span>}
      {p.num && p.text && " "}
      {p.text && <span className={cx(!!p.num && s.unit)}>{p.text}</span>}
    </span>
  );
}
