import { Icon, type IconName } from "./Icon";
import s from "./Status.module.css";

// Status is always icon + text, never colour alone (SPEC §11).
export type Tone = "ok" | "attention" | "blocked" | "neutral" | "accent" | "mod";

const icons: Record<Tone, IconName> = { ok: "check", attention: "alert", blocked: "block", neutral: "dot", accent: "arrow", mod: "diff" };

export function Status({ tone = "neutral", text, title = "" }: { tone?: Tone; text: string; title?: string }) {
  return (
    <span className={`${s.status} ${s[tone]}`} title={title || undefined}>
      <Icon name={icons[tone]} size={13} />
      {text}
    </span>
  );
}
