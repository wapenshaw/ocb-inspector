import { Icon, type IconName } from "./Icon";
import s from "./Status.module.css";

// Status is always icon + text, never colour alone (SPEC §11).
export type Tone = "ok" | "attention" | "blocked" | "neutral" | "accent" | "mod";

const icons: Record<Tone, IconName> = { ok: "check", attention: "alert", blocked: "block", neutral: "dot", accent: "arrow", mod: "diff" };

/** `compact` shows the icon alone: the text stays for screen readers and, with `title`, on hover. */
export function Status({ tone = "neutral", text, title = "", compact = false }: { tone?: Tone; text: string; title?: string; compact?: boolean }) {
  return (
    <span className={`${s.status} ${s[tone]}`} title={compact ? [text, title].filter(Boolean).join(". ") : title || undefined}>
      <Icon name={icons[tone]} size={compact ? 15 : 13} />
      {compact ? <span className="visually-hidden">{text}</span> : text}
    </span>
  );
}
