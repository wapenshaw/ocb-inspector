<script lang="ts">
  import Inspect from "./lib/Inspect.svelte";
  import Migrate from "./lib/Migrate.svelte";
  import Compare from "./lib/Compare.svelte";
  import Status from "./lib/Status.svelte";
  import { app } from "./lib/app.svelte";
  import { applyTheme, currentTheme, saveTheme, THEME_MODES, type ThemeMode } from "./lib/theme";

  let view = $state<"inspect" | "compare" | "migrate">("inspect");
  let inspect = $state<Inspect | null>(null);
  let browsing = $state<string | null>(null);
  let theme = $state<ThemeMode>(currentTheme());

  const current = $derived(view !== "inspect" ? view : browsing ? "browse" : "inspect");

  function setTheme(mode: ThemeMode) {
    theme = mode;
    applyTheme(mode);
    void saveTheme(mode);
  }

  function goBrowse() {
    view = "inspect";
    if (!browsing) inspect?.browseBios();
  }
  function goInspect() {
    view = "inspect";
    browsing = null;
  }

  function keys(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    if (!mod) return;
    const k = e.key.toLowerCase();
    if (k === "o" && view === "inspect") {
      e.preventDefault();
      inspect?.openFile();
    } else if (k === "f" && view === "inspect") {
      e.preventDefault();
      inspect?.focusSearch();
    } else if (k === "z" && app.writing && view === "inspect" && !(e.target instanceof HTMLInputElement)) {
      e.preventDefault();
      if (e.shiftKey) inspect?.redo();
      else inspect?.undo();
    }
  }
</script>

<svelte:window onkeydown={keys} />

<div class="app">
  <header class="top">
    <div class="brand">
      <!-- Same artwork as the window and taskbar icon (generated from public/icon.svg). -->
      <img class="logo" src="/icon.svg" width="30" height="30" alt="" />
      <span class="title">
        <span class="name">OCB Inspector</span>
        <span class="tagline">Thermal vision for your BIOS profiles</span>
      </span>
    </div>
    <span class="offline quiet">
      {#if app.writing}
        <span class="wide"><Status tone="attention" text="Experimental writing on" title="Edit and Port can write new profiles. Turn it off in config/settings.json." /></span>
        <span class="narrow"><Status tone="attention" text="Experimental" title="Writing is on: Edit and Port can write new profiles. Turn it off in config/settings.json." /></span>
      {:else}
        <span class="wide" title={app.writingNote}>Read-only and offline. Files never leave this computer.</span>
      {/if}
    </span>
    <div class="theme" role="radiogroup" aria-label="Colour mode">
      {#each THEME_MODES as t (t.mode)}
        <button role="radio" aria-checked={theme === t.mode} class:on={theme === t.mode} onclick={() => setTheme(t.mode)}>{t.label}</button>
      {/each}
    </div>
    <nav class="views" aria-label="Workspace">
      <button class:on={current === "browse"} aria-current={current === "browse" ? "page" : undefined} onclick={goBrowse}>
        Browse BIOS
      </button>
      <button class:on={current === "inspect"} aria-current={current === "inspect" ? "page" : undefined} onclick={goInspect}>
        {app.writing ? "Inspect and edit" : "Inspect"}
      </button>
      <button class:on={current === "compare"} aria-current={current === "compare" ? "page" : undefined} onclick={() => (view = "compare")}>
        Compare
      </button>
      {#if app.writing}
        <button class:on={current === "migrate"} aria-current={current === "migrate" ? "page" : undefined} onclick={() => (view = "migrate")}>
          Port to another BIOS
        </button>
      {/if}
    </nav>
  </header>
  <div class="ramp" aria-hidden="true"></div>
  <main>
    <!-- Both views stay mounted so switching keeps their state. -->
    <div class="view" hidden={view !== "inspect"}><Inspect bind:this={inspect} bind:browsing /></div>
    <div class="view" hidden={view !== "compare"}><Compare /></div>
    {#if app.writing}<div class="view" hidden={view !== "migrate"}><Migrate /></div>{/if}
  </main>
</div>

<style>
  .app {
    display: grid;
    grid-template-rows: 52px 3px 1fr;
    height: 100%;
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--s4);
    padding: 0 var(--s4);
    background: var(--surface);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--s2);
    padding-right: var(--s4);
    height: 100%;
    border-right: 1px solid var(--rule);
  }
  .logo {
    display: block;
    border-radius: 7px;
    /* The tile is near the dark header colour; the outline keeps its edge visible. */
    box-shadow: 0 0 0 1px var(--rule-strong);
  }
  .title {
    display: flex;
    flex-direction: column;
    line-height: 1.15;
  }
  .name {
    font-weight: 700;
    font-size: var(--fs-lead);
    letter-spacing: 0.01em;
  }
  .tagline {
    font-size: var(--fs-small);
    color: var(--ink-faint);
  }
  .theme {
    display: inline-flex;
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .theme button {
    height: 30px;
    padding: 0 var(--s3);
    border: 0;
    border-left: 1px solid var(--rule);
    background: transparent;
    color: var(--ink-quiet);
    font-size: var(--fs-small);
    cursor: pointer;
  }
  .theme button:first-child {
    border-left: 0;
  }
  .theme button.on {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  .theme button:focus-visible {
    outline-offset: -2px;
  }
  .views {
    display: flex;
    gap: var(--s2);
  }
  .views button,
  .theme button {
    white-space: nowrap;
  }
  .theme,
  .views,
  .brand {
    flex: none;
  }
  .offline {
    font-size: var(--fs-small);
    margin-right: auto;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .narrow {
    display: none;
  }
  /* Down to the 1024px minimum window: tighter header, short labels. */
  @media (max-width: 1240px) {
    .top {
      gap: var(--s2);
    }
    .tagline,
    .wide {
      display: none;
    }
    .narrow {
      display: inline;
    }
    .views {
      gap: 6px;
    }
    .views button {
      padding: 0 10px;
    }
    .theme button {
      padding: 0 var(--s2);
    }
  }
  .views button {
    height: 36px;
    padding: 0 var(--s3);
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius);
    background: transparent;
    color: var(--ink);
    font-weight: 600;
    cursor: pointer;
  }
  .views button:hover {
    border-color: var(--ink-faint);
  }
  .views button.on {
    background: var(--hot);
    border-color: var(--hot);
    color: var(--on-hot);
  }
  main {
    min-height: 0;
  }
  .view {
    height: 100%;
  }
  .view[hidden] {
    display: none;
  }
</style>
