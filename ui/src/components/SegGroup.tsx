// A segmented radio group (the global `seg` class): one choice of several, shown side by side.
import { cx } from "../lib/cx";

export interface SegOption<V extends string> {
  value: V;
  label: string;
  title?: string;
}

interface SegGroupProps<V extends string> {
  /** Accessible name of the group. */
  label: string;
  /** Radio group name, unique on the screen. */
  name: string;
  options: readonly SegOption<V>[];
  value: V;
  onChange: (value: V) => void;
}

export function SegGroup<V extends string>({ label, name, options, value, onChange }: SegGroupProps<V>) {
  return (
    <div className="seg" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <label key={o.value} className={cx(value === o.value && "on")} title={o.title}>
          <input type="radio" name={name} value={o.value} checked={value === o.value} onChange={() => onChange(o.value)} />
          {o.label}
        </label>
      ))}
    </div>
  );
}
