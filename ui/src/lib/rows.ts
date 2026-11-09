// Keyboard movement in a table of selectable rows (Inspect, Browse).
import type { KeyboardEvent } from "react";

/**
 * Arrow Up/Down: select the neighbouring row and focus it (its element id comes from `idOf`).
 * Returns true when the key was handled.
 */
export function moveRow<T>(e: KeyboardEvent, rows: T[], index: number, select: (row: T) => void, idOf: (row: T) => string): boolean {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return false;
  e.preventDefault();
  const next = rows[Math.min(rows.length - 1, Math.max(0, index + (e.key === "ArrowDown" ? 1 : -1)))];
  if (next) {
    select(next);
    document.getElementById(idOf(next))?.focus();
  }
  return true;
}
