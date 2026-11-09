import { useEffect, useRef, useState } from "react";
import type { Constraints, Value } from "../api/types";
import s from "./ValueEditor.module.css";

export interface ValueEditorProps {
  value: Value | null;
  constraints: Constraints;
  autoOnly?: boolean;
  label: string;
  onChange: (v: Value) => void;
}

// Typed editor: enum dropdown, or integer with unit and an explicit Auto switch.
// Range hints are a convenience only; Rust validates every staged value.
export function ValueEditor({ value, constraints, autoOnly = false, label, onChange }: ValueEditorProps) {
  const isEnum = constraints.options.length > 0;
  const isAuto = value?.kind === "auto";
  const unit = constraints.unit ?? "";
  const [lastNumber, setLastNumber] = useState<number | null>(null);
  const numeric = value?.kind === "integer" ? value.value : lastNumber ?? constraints.min ?? 0;
  const showNumber = !isEnum && !autoOnly;

  const numRef = useRef<HTMLInputElement>(null);

  // The number box keeps what the user typed until the value it shows changes, so it is not a
  // controlled input.
  useEffect(() => {
    if (numRef.current) numRef.current.value = String(numeric);
  }, [numeric]);

  // Commit on the native change event (Enter, the arrows, or leaving the box), not per keystroke.
  // The latest onChange is read through a ref so the listener is bound once.
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  useEffect(() => {
    const el = numRef.current;
    if (!el) return;
    const commit = () => {
      const n = Number(el.value);
      if (Number.isFinite(n) && Number.isInteger(n)) onChangeRef.current({ kind: "integer", value: n, unit });
    };
    el.addEventListener("change", commit);
    return () => el.removeEventListener("change", commit);
  }, [showNumber, unit]);

  function setAuto(on: boolean) {
    if (on) {
      if (value?.kind === "integer") setLastNumber(value.value);
      onChange({ kind: "auto" });
    } else {
      const start = lastNumber ?? (unit === "mV" ? -50 : unit === "ratio" ? 1 : constraints.min ?? 1);
      onChange({ kind: "integer", value: start, unit });
    }
  }

  function setEnum(e: React.ChangeEvent<HTMLSelectElement>) {
    const raw = Number(e.target.value);
    const o = constraints.options.find((x) => x.raw === raw);
    if (o) onChange({ kind: "enum", symbol: o.symbol, raw: o.raw, label: o.label });
  }

  if (isEnum) {
    return (
      <select className={s.select} aria-label={label} value={value?.kind === "enum" ? (value.raw ?? "") : ""} onChange={setEnum}>
        {value?.kind === "unknown" && <option value="">Unknown raw {value.raw}</option>}
        {constraints.options.map((o) => (
          <option key={o.raw} value={o.raw}>{o.label}</option>
        ))}
      </select>
    );
  }

  return (
    <>
      <div className={s.num}>
        {constraints.auto_allowed && (
          <label className={s.auto}>
            <input type="checkbox" checked={isAuto} onChange={(e) => setAuto(e.target.checked)} />
            Auto
          </label>
        )}
        {!autoOnly && (
          <input
            ref={numRef}
            type="number"
            className="mono"
            aria-label={`${label} in ${unit}`}
            disabled={isAuto}
            defaultValue={numeric}
            min={constraints.min}
            max={constraints.max}
            step={constraints.step ?? 1}
          />
        )}
        {!autoOnly && <span className={s.unit}>{unit === "ratio" ? "bins" : unit}</span>}
      </div>
      {!autoOnly && constraints.min !== undefined && constraints.max !== undefined && (
        <p className={s.hint}>
          Range <span className="mono">{constraints.min}</span> to <span className="mono">{constraints.max}</span>
          {(constraints.step ?? 0) > 1 && (
            <>, steps of <span className="mono">{constraints.step}</span></>
          )}
        </p>
      )}
    </>
  );
}
