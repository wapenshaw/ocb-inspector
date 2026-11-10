import type { ReviewedInterpretation, Value } from "../api/types";
import { display } from "../lib/format";
import s from "./ReviewedReading.module.css";

function hasNumericMeaning(value: Value | null): boolean {
  return value?.kind === "integer" && !!value.unit && value.unit !== "raw";
}

/** Raw comparisons continue to describe each component alone. Enum aliases stay in details. */
export function ReviewedReading({ reading, source, originalText }: { reading: ReviewedInterpretation | null | undefined; source: "saved" | "default" | "baseline"; originalText?: string }) {
  if (!reading) return null;
  const numericRecipe = [reading.value, reading.default_value, reading.baseline_value].some(hasNumericMeaning);
  const compositeAuto = reading.components.length > 1 && reading.value.kind === "auto";
  if (!numericRecipe && !compositeAuto) return null;
  const value = source === "saved" ? reading.value : source === "default" ? reading.default_value : reading.baseline_value;
  if (value && value.kind !== "integer" && value.kind !== "auto") return null;
  const text = value ? display(value) : "Unknown";
  if (text === originalText) return null;
  return <div className={s.reading}><small className={`quiet ${s.label}`}>{reading.board_evidence === "examined" ? "Reviewed reading: " : "Recipe reading (not checked on this board): "}</small><strong className={s.value}>{text}</strong></div>;
}

export function ReviewedDetails({ reading }: { reading: ReviewedInterpretation | null | undefined }) {
  if (!reading) return null;
  return <section>
    <h3>{reading.board_evidence === "examined" ? "Reviewed recipe interpretation" : "Recipe interpretation (not checked on this board)"}</h3>
    <p>{reading.label}: {display(reading.value)}</p>
    <p className="quiet">Recipe interpretation is shown separately from the BIOS label and may use different aliases. {reading.board_evidence === "examined" ? "This recipe was examined for this board." : "This recipe has not been checked on this board."}</p>
    <p className="quiet">Default: {reading.default_value ? display(reading.default_value) : "Unknown"}. This reading uses the recipe components below. Difference markers compare each row's original stored value; interpretation does not establish record binding, hardware verification or BIOS import acceptance.</p>
    <ul>{reading.components.map((c, i) => <li key={i} className="mono">{c.store}+0x{c.store_offset.toString(16)}, {c.width_bits} bits, profile bytes {c.span.start}–{c.span.end}</li>)}</ul>
    <ul>{reading.evidence.map((e, i) => <li key={i}>{e}</li>)}</ul>
  </section>;
}
