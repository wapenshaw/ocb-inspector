<script lang="ts">
  import type { Value } from "./types";
  import { parts } from "./format";
  // Numbers in monospace, units and labels in the UI face; Auto is a tag, not a number.
  let { value, strike = false }: { value: Value | null | undefined; strike?: boolean } = $props();
  const p = $derived(parts(value));
</script>

<span class="v" class:auto={p.auto} class:strike>
  {#if p.num}<span class="mono">{p.num}</span>{/if}{#if p.num && p.text}&thinsp;{/if}{#if p.text}<span class:unit={!!p.num}>{p.text}</span>{/if}
</span>

<style>
  .v {
    white-space: nowrap;
  }
  .unit {
    color: var(--ink-quiet);
    font-size: var(--fs-small);
  }
  .auto {
    color: var(--ink-quiet);
    font-style: italic;
  }
  .strike {
    text-decoration: line-through;
    text-decoration-color: var(--ink-faint);
  }
</style>
