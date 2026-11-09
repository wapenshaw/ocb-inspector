// A button label that changes while something runs ("Export profile" → "Exporting…") without
// changing the button's width: both labels share one grid cell and the inactive one is hidden.
export function SwapLabel({ busy, idle, working }: { busy: boolean; idle: string; working: string }) {
  return (
    <span className="swap">
      <span aria-hidden={busy || undefined}>{idle}</span>
      <span aria-hidden={!busy || undefined}>{working}</span>
    </span>
  );
}
