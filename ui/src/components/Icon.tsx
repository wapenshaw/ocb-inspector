// Small stroke icon set. Decorative: callers always pair icons with text.
export type IconName =
  | "check" | "alert" | "block" | "arrow" | "dot" | "lock" | "file" | "chip"
  | "undo" | "redo" | "minus" | "diff" | "chevron" | "search";

const paths: Record<IconName, React.ReactNode> = {
  check: <path d="M3 8.5l3 3 7-7" />,
  alert: (<><path d="M8 2.2l6.2 11H1.8L8 2.2z" /><path d="M8 6.5v3" /><path d="M8 11.6v.1" /></>),
  block: (<><circle cx="8" cy="8" r="6" /><path d="M3.8 12.2l8.4-8.4" /></>),
  arrow: (<><path d="M2.5 8h11" /><path d="M9.5 4l4 4-4 4" /></>),
  dot: <circle cx="8" cy="8" r="2.2" fill="currentColor" stroke="none" />,
  lock: (<><rect x="3" y="7" width="10" height="7" rx="1.5" /><path d="M5.5 7V5a2.5 2.5 0 015 0v2" /></>),
  file: (<><path d="M4 1.8h5l3 3V14.2H4z" /><path d="M9 1.8v3h3" /></>),
  chip: (<><rect x="4" y="4" width="8" height="8" rx="1" /><path d="M6 1.8v2.2M10 1.8v2.2M6 12v2.2M10 12v2.2M1.8 6h2.2M1.8 10h2.2M12 6h2.2M12 10h2.2" /></>),
  undo: (<><path d="M5 4L2 7l3 3" /><path d="M2 7h7.5a4 4 0 010 8H8" /></>),
  redo: (<><path d="M11 4l3 3-3 3" /><path d="M14 7H6.5a4 4 0 000 8H8" /></>),
  minus: <path d="M3.5 8h9" />,
  diff: (<><path d="M3 6h10M3 10h10" /><path d="M10.5 3.5l-5 9" /></>),
  chevron: <path d="M6 4l4 4-4 4" />,
  search: (<><circle cx="7" cy="7" r="4.5" /><path d="M10.4 10.4L14 14" /></>),
};

export function Icon({ name, size = 14 }: { name: IconName; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
      {paths[name]}
    </svg>
  );
}
