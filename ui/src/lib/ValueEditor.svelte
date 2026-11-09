<script lang="ts">
  import type { Constraints, Value } from "./types";
  // Typed editor: enum dropdown, or integer with unit and an explicit Auto switch.
  // Range hints are a convenience only; Rust validates every staged value.
  let {
    value,
    constraints,
    autoOnly = false,
    disabled = false,
    label,
    onchange,
  }: {
    value: Value | null;
    constraints: Constraints;
    autoOnly?: boolean;
    disabled?: boolean;
    label: string;
    onchange: (v: Value) => void;
  } = $props();

  const isEnum = $derived(constraints.options.length > 0);
  const isAuto = $derived(value?.kind === "auto");
  const unit = $derived(constraints.unit ?? "");
  let lastNumber = $state<number | null>(null);
  const numeric = $derived(value?.kind === "integer" ? value.value : lastNumber ?? constraints.min ?? 0);

  function setAuto(on: boolean) {
    if (on) {
      if (value?.kind === "integer") lastNumber = value.value;
      onchange({ kind: "auto" });
    } else {
      const start = lastNumber ?? (unit === "mV" ? -50 : unit === "ratio" ? 1 : constraints.min ?? 1);
      onchange({ kind: "integer", value: start, unit });
    }
  }
  function setNumber(e: Event) {
    const n = Number((e.target as HTMLInputElement).value);
    if (Number.isFinite(n) && Number.isInteger(n)) onchange({ kind: "integer", value: n, unit });
  }
  function setEnum(e: Event) {
    const raw = Number((e.target as HTMLSelectElement).value);
    const o = constraints.options.find((x) => x.raw === raw);
    if (o) onchange({ kind: "enum", symbol: o.symbol, raw: o.raw, label: o.label });
  }
</script>

{#if isEnum}
  <select aria-label={label} {disabled} value={value?.kind === "enum" ? value.raw : ""} onchange={setEnum}>
    {#if value?.kind === "unknown"}<option value="">Unknown raw {value.raw}</option>{/if}
    {#each constraints.options as o (o.raw)}
      <option value={o.raw}>{o.label}</option>
    {/each}
  </select>
{:else}
  <div class="num">
    {#if constraints.auto_allowed}
      <label class="auto">
        <input type="checkbox" checked={isAuto} {disabled} onchange={(e) => setAuto((e.target as HTMLInputElement).checked)} />
        Auto
      </label>
    {/if}
    {#if !autoOnly}
      <input
        type="number"
        class="mono"
        aria-label="{label} in {unit}"
        disabled={disabled || isAuto}
        value={numeric}
        min={constraints.min}
        max={constraints.max}
        step={constraints.step ?? 1}
        onchange={setNumber}
      />
      <span class="unit">{unit === "ratio" ? "bins" : unit}</span>
    {/if}
  </div>
  {#if !autoOnly && constraints.min !== undefined && constraints.max !== undefined}
    <p class="hint">
      Range <span class="mono">{constraints.min}</span> to <span class="mono">{constraints.max}</span>{#if constraints.step && constraints.step > 1}, steps of <span class="mono">{constraints.step}</span>{/if}
    </p>
  {/if}
{/if}

<style>
  select {
    width: 100%;
  }
  .num {
    display: flex;
    align-items: center;
    gap: var(--s2);
  }
  .auto {
    display: inline-flex;
    align-items: center;
    gap: var(--s1);
    padding-right: var(--s2);
    border-right: 1px solid var(--rule);
    margin-right: var(--s1);
  }
  input[type="number"] {
    width: 96px;
    text-align: right;
  }
  .unit {
    color: var(--ink-quiet);
  }
  .hint {
    margin-top: var(--s1);
    color: var(--ink-quiet);
    font-size: var(--fs-small);
  }
</style>
