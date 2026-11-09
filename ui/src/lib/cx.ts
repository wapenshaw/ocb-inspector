/** Join the truthy class names. */
export function cx(...names: (string | false | null | undefined)[]): string | undefined {
  const out = names.filter(Boolean).join(" ");
  return out || undefined;
}
