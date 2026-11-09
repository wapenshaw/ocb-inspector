import type { Value } from "./types";

/** Split a value into a number part (monospace) and a unit/label part (UI font). */
export function parts(v: Value | null | undefined): { num?: string; text?: string; auto?: boolean } {
  if (!v) return { text: "—" };
  switch (v.kind) {
    case "auto":
      return { text: "Auto", auto: true };
    case "integer":
      if (v.unit === "ratio") return { num: (v.value > 0 ? "+" : "") + v.value, text: "bins" };
      if (v.unit === "raw") return { num: String(v.value), text: "raw" };
      return { num: String(v.value), text: v.unit };
    case "enum":
      return { text: (v.label ?? v.symbol).trim() };
    case "boolean":
      return { text: v.value ? "On" : "Off" };
    case "unknown":
      return { num: String(v.raw), text: "unknown raw value" };
  }
}

export function display(v: Value | null | undefined): string {
  const p = parts(v);
  return [p.num, p.text].filter(Boolean).join(" ");
}

export function sameIntent(a: Value | null | undefined, b: Value | null | undefined): boolean {
  if (!a || !b) return a === b;
  if (a.kind === "enum" && b.kind === "enum") return a.symbol === b.symbol;
  return JSON.stringify(a) === JSON.stringify(b);
}

/** YYYYMMDDHHMMSS → "8 Nov 2026, 11:19" (no timezone is stored). */
export function savedAt(ts: string): string {
  if (!/^\d{14}$/.test(ts)) return ts;
  const months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const m = months[Number(ts.slice(4, 6)) - 1] ?? "?";
  return `${Number(ts.slice(6, 8))} ${m} ${ts.slice(0, 4)}, ${ts.slice(8, 10)}:${ts.slice(10, 12)}`;
}

export function shortHash(h: string): string {
  return h ? `${h.slice(0, 8)}…${h.slice(-4)}` : "";
}

export function fileName(p: string | null | undefined): string {
  return (p ?? "").split(/[\\/]/).pop() ?? "";
}

export function cpuShort(brand: string): string {
  return brand.replace(/\(R\)|\(TM\)/g, "").replace(/\s+/g, " ").trim();
}

export function hex(n: number): string {
  return "0x" + n.toString(16).toUpperCase().padStart(4, "0");
}
