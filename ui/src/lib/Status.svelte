<script lang="ts">
  import Icon from "./Icon.svelte";
  // Status is always icon + text, never colour alone (SPEC §11).
  type Tone = "ok" | "attention" | "blocked" | "neutral" | "accent" | "mod";
  let { tone = "neutral", text, title = "" }: { tone?: Tone; text: string; title?: string } = $props();
  const icon = $derived(
    { ok: "check", attention: "alert", blocked: "block", neutral: "dot", accent: "arrow", mod: "diff" }[tone],
  );
</script>

<span class="status {tone}" {title}><Icon name={icon} size={13} />{text}</span>

<style>
  .status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: var(--fs-small);
    white-space: nowrap;
    color: var(--ink-quiet);
  }
  .ok {
    color: var(--green);
  }
  .attention {
    color: var(--amber);
  }
  .blocked {
    color: var(--red);
    font-weight: 600;
  }
  /* Staged edits: a filled "hot" chip, the brightest step of the heat ramp. */
  .accent {
    padding: 1px 8px 1px 6px;
    border-radius: 3px;
    background: var(--hot);
    color: var(--on-hot);
    font-weight: 600;
  }
  .mod {
    color: var(--mod);
  }
</style>
