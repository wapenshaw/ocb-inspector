<script lang="ts">
  // BIOS menu tree for the Inspect and Browse rails, drawn as a board trace: one trunk, each
  // child page branching off it to its own via. Branches collapse; the open page's branch
  // always shows.
  import type { Snippet } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import Icon from "./Icon.svelte";
  import type { Page } from "./menu";

  let {
    pages,
    active,
    onopen,
    count,
  }: {
    pages: Page[];
    /** Key of the open page, or null when none is (searching, filtering). */
    active: string | null;
    onopen: (key: string) => void;
    count?: Snippet<[Page]>;
  } = $props();

  const expanded = new SvelteSet<string>();

  const ancestors = (pg: Page) => pg.path.slice(0, -1).map((_, i) => pg.path.slice(0, i + 1).join(" › "));
  const hasKids = $derived(new Set(pages.filter((pg, i) => (pages[i + 1]?.depth ?? -1) > pg.depth).map((pg) => pg.key)));
  const visible = $derived(pages.filter((pg) => ancestors(pg).every((k) => expanded.has(k))));

  // Top-level branches start open, deeper ones folded. Each branch is opened by default only the
  // first time it appears, so later list changes (e.g. showing hidden fields) keep the user's folds.
  const seeded = new Set<string>();
  $effect(() => {
    for (const pg of pages) {
      if (pg.depth !== 0 || !hasKids.has(pg.key) || seeded.has(pg.key)) continue;
      seeded.add(pg.key);
      expanded.add(pg.key);
    }
  });

  // Opening a page (from search, a link or the first load) unfolds the branch it is on.
  $effect(() => {
    const pg = active ? pages.find((p) => p.key === active) : undefined;
    if (pg) for (const k of ancestors(pg)) expanded.add(k);
  });

  function toggle(key: string) {
    if (expanded.has(key)) expanded.delete(key);
    else expanded.add(key);
  }
</script>

<ul class="tree">
  {#each visible as pg (pg.key)}
    {@const kids = hasKids.has(pg.key)}
    {@const open = expanded.has(pg.key)}
    {@const on = active === pg.key}
    <li class="node" class:on class:heading={pg.heading} style="--depth:{pg.depth}">
      {#if kids}
        <button class="twist" class:open aria-expanded={open} aria-label="{open ? 'Collapse' : 'Expand'} {pg.title}" onclick={() => toggle(pg.key)}>
          <Icon name="chevron" size={14} />
        </button>
      {:else}
        <span class="twist" aria-hidden="true"></span>
      {/if}
      {#if pg.heading}
        <button class="row" aria-expanded={kids ? open : undefined} onclick={() => kids && toggle(pg.key)}>
          <span class="title">{pg.title}</span>
        </button>
      {:else}
        <button class="row" aria-current={on ? "page" : undefined} onclick={() => onopen(pg.key)}>
          <span class="title">{pg.title || "Setup"}</span>
          {#if count}<span class="count mono">{@render count(pg)}</span>{/if}
        </button>
      {/if}
    </li>
  {/each}
</ul>

<style>
  /* Geometry: trunk at x=8; a page at depth d has its via centred at x = 8 + d*16. */
  .tree {
    --trunk: 8px;
    --step: 16px;
    list-style: none;
    margin: 0;
    padding: 0;
    position: relative;
  }
  /* One continuous trunk for the whole menu. */
  .tree::before {
    content: "";
    position: absolute;
    left: calc(var(--trunk) - 1px);
    top: 0;
    bottom: 0;
    width: 2px;
    background: var(--trace);
  }
  .node {
    --x: calc(var(--trunk) + var(--depth) * var(--step));
    position: relative;
    display: flex;
    align-items: flex-start;
    padding-left: calc(var(--x) + 9px);
  }
  /* Branch from the trunk out to this page's via. */
  .node::after {
    content: "";
    position: absolute;
    left: var(--trunk);
    top: 16px;
    width: calc(var(--depth) * var(--step));
    height: 2px;
    background: var(--trace);
  }
  /* The via: hollow on every page, filled on the open one, a square pad on a section that only groups. */
  .node::before {
    content: "";
    position: absolute;
    z-index: 1;
    left: calc(var(--x) - 6px);
    top: 11px;
    width: 12px;
    height: 12px;
    box-sizing: border-box;
    border-radius: 50%;
    border: 2px solid var(--trace);
    background: var(--paper);
  }
  .node.heading::before {
    border-radius: 2px;
    background: var(--trace);
  }
  .node.on::before {
    background: var(--hot);
    border-color: var(--hot);
  }
  .twist {
    flex: none;
    display: grid;
    place-items: center;
    width: 20px;
    height: 34px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--ink-faint);
    cursor: pointer;
  }
  button.twist :global(svg) {
    transition: transform 0.12s ease;
  }
  button.twist.open :global(svg) {
    transform: rotate(90deg);
  }
  button.twist:hover {
    color: var(--ink);
  }
  .row {
    flex: 1;
    min-width: 0;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--s2);
    min-height: 34px;
    padding: 6px var(--s2);
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--ink);
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--sunk);
  }
  .heading .row {
    color: var(--ink-quiet);
    font-weight: 600;
  }
  /* The open page changes colour only, never weight: bolder text would rewrap and shift the menu. */
  .on .row {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .title {
    min-width: 0;
    line-height: 1.25;
  }
  .count {
    flex: none;
    display: inline-flex;
    gap: 6px;
    color: var(--ink-faint);
    font-size: 11.5px;
  }
  @media (prefers-reduced-motion: reduce) {
    button.twist :global(svg) {
      transition: none;
    }
  }
</style>
