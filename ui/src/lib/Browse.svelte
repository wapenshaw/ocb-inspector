<script lang="ts">
  // A BIOS image on its own: every setting its menus define, with the optimized default.
  // No profile is needed, so this works for every family, including browse-only ones.
  import { api, errorMessage } from "./api";
  import type { BiosField, BrowsePayload, Report } from "./types";
  import { fileName, hex, shortHash } from "./format";
  import MenuTree from "./MenuTree.svelte";
  import { menuPages, menuSort, pageBefore, pageKey, sectionBefore, type Page } from "./menu";
  import Status from "./Status.svelte";
  import Icon from "./Icon.svelte";

  let {
    bios,
    onclose,
    onprofile,
    closeLabel = "Change BIOS",
  }: {
    bios: string;
    onclose: () => void;
    /** Present when profiles from this BIOS can be opened. */
    onprofile?: () => void;
    closeLabel?: string;
  } = $props();

  let report = $state<Report<BrowsePayload> | null>(null);
  let loadError = $state("");
  let group = $state("");
  let query = $state("");
  let showHidden = $state(false);
  let selectedKey = $state<string | null>(null);
  /** Firmware set to show (images with several complete sets). */
  let set = $state(0);
  let lastBios = "";
  let searchEl = $state<HTMLInputElement | null>(null);

  export function focusSearch() {
    searchEl?.focus();
  }

  type Row = BiosField & { key: string; menu: { path: string[]; section: string | null; order: number[] } };

  $effect(() => {
    const path = bios;
    const want = set;
    if (path !== lastBios) {
      lastBios = path;
      if (want !== 0) {
        set = 0; // a new image starts on its default set; this effect reruns
        return;
      }
    }
    report = null;
    loadError = "";
    api
      .browse(path, want)
      .then((r) => {
        report = r;
        // Start where tuning lives when the BIOS has an Overclocking menu.
        const sorted = menuSort(rowsOf(r.payload, false));
        const first = sorted.find((x) => x.menu.path[0] === "Overclocking") ?? sorted[0];
        group = first ? pageKey(first.menu.path) : "";
        selectedKey = first?.key ?? null;
      })
      .catch((e) => (loadError = errorMessage(e)));
  });

  function rowsOf(p: BrowsePayload, hidden: boolean): Row[] {
    return p.fields
      .filter((f) => hidden || !f.hidden)
      .map((f) => ({
        ...f,
        key: `${f.formset}:${f.store}:${f.offset}:${f.width_bits}:${f.prompt}`,
        menu: { path: f.menu_path, section: f.section, order: f.menu_order },
      }));
  }

  const p = $derived(report?.payload ?? null);
  const plat = $derived(p?.bios.platform ?? null);
  const all = $derived(p ? rowsOf(p, showHidden) : []);
  const hiddenCount = $derived(p?.fields.filter((f) => f.hidden).length ?? 0);
  const pages = $derived<Page[]>(menuPages(all));
  const counts = $derived.by(() => {
    const m = new Map<string, number>();
    for (const r of all) m.set(pageKey(r.menu.path), (m.get(pageKey(r.menu.path)) ?? 0) + 1);
    return m;
  });

  const rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return menuSort(
      all.filter((r) =>
        q
          ? `${r.prompt} ${r.menu.path.join(" ")} ${r.store} ${r.help}`.toLowerCase().includes(q)
          : pageKey(r.menu.path) === group,
      ),
    );
  });
  const spansPages = $derived(new Set(rows.map((r) => pageKey(r.menu.path))).size > 1);
  const LIMIT = 600;

  // The same prompt more than once on a page at different offsets: alternatives the BIOS picks
  // between at runtime (AMD boards carry one copy per processor family).
  const variants = $derived.by(() => {
    const by = new Map<string, Row[]>();
    for (const r of all) {
      const k = `${pageKey(r.menu.path)}|${r.prompt}`;
      by.set(k, [...(by.get(k) ?? []), r]);
    }
    const out = new Map<string, { n: number; of: number }>();
    for (const list of by.values()) {
      if (list.length < 2) continue;
      list.forEach((r, i) => out.set(r.key, { n: i + 1, of: list.length }));
    }
    return out;
  });

  const selected = $derived(all.find((r) => r.key === selectedKey) ?? null);
  const formset = $derived(selected && p ? p.formsets[selected.formset] : null);

  function defaultText(f: BiosField): string {
    if (f.default_raw === null || f.default_raw === undefined) return "";
    return f.default_label ?? String(f.default_raw);
  }

  function allowed(f: BiosField): string {
    if (f.kind === "checkbox") return "Enabled or Disabled";
    if (f.kind === "one_of") return `${f.options.length} ${f.options.length === 1 ? "choice" : "choices"}`;
    if (f.min !== null && f.max !== null) return `${f.min} to ${f.max}${f.step && f.step > 1 ? `, step ${f.step}` : ""}`;
    return "Number";
  }

  function openPage(key: string) {
    group = key;
    query = "";
  }

  function rowKey(e: KeyboardEvent, idx: number) {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const n = rows[Math.min(rows.length - 1, Math.max(0, idx + (e.key === "ArrowDown" ? 1 : -1)))];
    if (n) {
      selectedKey = n.key;
      document.getElementById(`b-${n.key}`)?.focus();
    }
  }
</script>

<div class="browse">
  <div class="strip" role="group" aria-label="Open BIOS">
    <span class="mono strong">{p?.bios.bios_id ?? fileName(bios)}</span>
    {#if plat}
      <span class="strong">{plat.version}</span>
      <span class="quiet">{plat.family}{plat.chipset ? `, ${plat.chipset}` : ""}</span>
      {#if plat.models}<span class="quiet models" title={plat.models}>{plat.models}</span>{/if}
    {/if}
    <span class="spacer"></span>
    {#if p && p.sets.length > 1}
      <label class="setpick" title={p.sets[p.set]?.note ?? ""}>
        <span class="faint">Firmware set</span>
        <select value={p.set} onchange={(e) => (set = Number((e.currentTarget as HTMLSelectElement).value))}>
          {#each p.sets as s, i (i)}
            <option value={i}>{i + 1}: {s.label}</option>
          {/each}
        </select>
      </label>
    {/if}
    {#if p}
      {#if onprofile}
        <button class="btn" onclick={onprofile}>Open a profile with this BIOS</button>
      {:else}
        <Status tone="neutral" text="Browse only" title={plat?.profiles_note ?? ""} />
      {/if}
    {/if}
    <button class="btn ghost" onclick={onclose}>{closeLabel}</button>
  </div>

  {#if loadError}
    <div class="pad"><div class="problem" role="alert"><Status tone="blocked" text="Could not open" /><p>{loadError}</p></div></div>
  {:else if !p}
    <div class="pad quiet">Reading every menu in {fileName(bios)}…</div>
  {:else}
    <div class="work">
      <nav class="rail" aria-label="BIOS menu">
        <p class="railhead faint">BIOS menu</p>
        <MenuTree {pages} active={query ? null : group} onopen={openPage}>
          {#snippet count(pg)}{counts.get(pg.key) ?? 0}{/snippet}
        </MenuTree>
      </nav>

      <div class="main">
        <div class="tools">
          <label class="search">
            <Icon name="search" />
            <span class="visually-hidden">Search every setting</span>
            <input bind:this={searchEl} type="search" placeholder="Search every setting in this BIOS" bind:value={query} />
          </label>
          {#if hiddenCount}
            <label class="check">
              <input type="checkbox" bind:checked={showHidden} />
              Include fields the BIOS never shows <span class="mono faint">{hiddenCount}</span>
            </label>
          {/if}
        </div>
        <p class="legend quiet">
          {#if p.bios.defaults.available}
            Defaults are the values “Load Optimized Defaults” restores, read from the image.
          {:else}
            <span class="faint">No optimized defaults: {p.bios.defaults.source}.</span>
          {/if}
        </p>

        <div class="tablewrap">
          <table class="grid" aria-label="BIOS settings">
            <colgroup><col style="width:40%" /><col style="width:20%" /><col style="width:20%" /><col /></colgroup>
            <thead><tr><th>Setting</th><th>Optimized default</th><th>Allowed</th><th>Stored at</th></tr></thead>
            <tbody>
              {#each rows.slice(0, LIMIT) as r, i (r.key)}
                {@const pageHead = spansPages ? pageBefore(rows, i) : null}
                {@const section = sectionBefore(rows, i)}
                {@const v = variants.get(r.key)}
                {#if pageHead}
                  <tr class="pagehead"><th colspan="4" scope="colgroup">{pageHead.length ? pageHead.join(" › ") : "Setup"}</th></tr>
                {/if}
                {#if section}
                  <tr class="section"><th colspan="4" scope="colgroup">{section}</th></tr>
                {/if}
                <tr id="b-{r.key}" tabindex="0" class:sel={selectedKey === r.key} class:dim={r.hidden}
                  aria-selected={selectedKey === r.key} onclick={() => (selectedKey = r.key)} onkeydown={(e) => rowKey(e, i)}>
                  <td title={r.prompt}>{r.prompt}{#if v}<span class="tag faint">{v.n} of {v.of}</span>{/if}{#if r.hidden}<span class="tag faint">hidden</span>{/if}</td>
                  <td>{#if defaultText(r)}{defaultText(r)}{:else}<span class="faint">—</span>{/if}</td>
                  <td class="quiet">{allowed(r)}</td>
                  <td class="mono quiet">{r.store}+{hex(r.offset)}</td>
                </tr>
              {:else}
                <tr><td colspan="4" class="quiet empty">{query ? "No setting matches this search." : "This page has no settings."}</td></tr>
              {/each}
            </tbody>
          </table>
          {#if rows.length > LIMIT}<p class="more quiet">Showing {LIMIT} of {rows.length}. Search to narrow the list.</p>{/if}
        </div>
      </div>

      <aside class="inspector" aria-label="Setting details">
        {#if selected}
          <p class="crumb faint">{selected.menu.path.join(" › ") || "Setup"}{#if selected.section}, {selected.section}{/if}</p>
          <h2>{selected.prompt}</h2>

          <div class="defbox">
            {#if defaultText(selected)}
              <span class="faint">Optimized default</span>
              <strong>{defaultText(selected)}</strong>
            {:else}
              <span class="faint">No optimized default in the image for the {selected.store} store.</span>
            {/if}
          </div>

          {#if selected.help}
            <h3>BIOS help</h3>
            <p class="bioshelp">{selected.help}</p>
          {/if}

          {#if selected.kind === "one_of"}
            <h3>Choices</h3>
            <ul class="choices">
              {#each selected.options as o (o.raw)}
                <li class:def={o.raw === selected.default_raw}><span>{o.label || "(no label)"}</span><span class="mono faint">{o.raw}</span></li>
              {/each}
            </ul>
          {:else if selected.kind === "numeric"}
            <h3>Range</h3>
            <p class="quiet">{allowed(selected)}</p>
          {/if}

          {@const v = variants.get(selected.key)}
          {#if v}
            <p class="note quiet">
              This page defines “{selected.prompt}” {v.of} times at different locations. The BIOS shows one of them depending on
              other settings or the installed processor; this is copy {v.n}.
            </p>
          {/if}
          {#if selected.hidden}
            <p class="note quiet">The firmware defines this field but never displays it.</p>
          {/if}

          <h3>Where it is stored</h3>
          <dl class="ev">
            <dt>Location</dt><dd><span class="mono">{selected.store}+{hex(selected.offset)}</span>, {selected.width_bits} bits</dd>
            <dt>Firmware</dt><dd>{[
              selected.callback ? "handled by a firmware callback" : "plain stored value",
              selected.reset_required ? "needs a reset" : "",
              selected.read_only ? "read only" : "",
              selected.conditional ? "shown or enabled depending on other settings" : "",
            ].filter(Boolean).join(", ")}</dd>
            {#if formset}<dt>Defined in</dt><dd>{formset.title || "Setup"} <span class="faint">({formset.module})</span></dd>{/if}
          </dl>
          {#if formset?.placement === "title_match"}
            <p class="note quiet">
              The BIOS does not link {formset.title} from its menus. It is shown under {formset.placed_under.join(" › ")} because that
              page has the same title.
            </p>
          {/if}
          {#if formset?.note}
            <p class="note quiet">{formset.note}</p>
          {/if}
        {/if}

        <h3>This BIOS</h3>
        <dl class="ev">
          {#if plat}
            <dt>Family</dt><dd>{plat.family}{plat.chipset ? `, ${plat.chipset}` : ""}</dd>
            {#if plat.models}<dt>Boards</dt><dd>{plat.models}</dd>{/if}
            <dt>Version</dt><dd>{plat.version} <span class="faint">({p.bios.revision}, built {p.bios.build_date})</span></dd>
            <dt>Parser</dt><dd class="mono">{plat.parser}</dd>
            <dt>Profiles</dt><dd>{plat.profiles_note}</dd>
          {/if}
          {#if p.sets.length > 1}
            <dt>Firmware set</dt><dd>{p.set + 1} of {p.sets.length}: {p.sets[p.set]?.note}</dd>
          {/if}
          <dt>Settings</dt><dd>{p.fields.length - hiddenCount} shown{hiddenCount ? `, ${hiddenCount} hidden` : ""}</dd>
          <dt>Defaults</dt><dd>{p.bios.defaults.source}</dd>
          <dt>Image</dt><dd class="mono">{shortHash(p.bios.sha256)}</dd>
        </dl>
      </aside>
    </div>
  {/if}
</div>

<style>
  .browse {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .pad {
    padding: var(--s5) var(--s6);
  }
  .strip {
    display: flex;
    align-items: center;
    gap: var(--s3);
    height: 48px;
    flex: none;
    padding: 0 var(--s4);
    border-bottom: 1px solid var(--rule);
    background: var(--surface);
    color: var(--ink-quiet);
    min-width: 0;
  }
  .models {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .strong {
    color: var(--ink);
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .setpick {
    display: flex;
    align-items: center;
    gap: var(--s2);
    white-space: nowrap;
  }
  .setpick select {
    max-width: 220px;
  }
  .work {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: 232px minmax(0, 1fr) 340px;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: var(--s4);
    padding: var(--s3) var(--s4);
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--s2);
    flex: 1;
    max-width: 420px;
    color: var(--ink-faint);
  }
  .search input {
    flex: 1;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-small);
    color: var(--ink-quiet);
    cursor: pointer;
  }
  .legend {
    padding: 0 var(--s4) var(--s2);
    font-size: var(--fs-small);
  }
  .tablewrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 var(--s4) var(--s4);
  }
  .grid {
    width: 100%;
    max-width: 1100px;
    border-collapse: separate;
    border-spacing: 0;
    table-layout: fixed;
  }
  .grid th {
    position: sticky;
    top: 0;
    background: var(--paper);
    text-align: left;
    font-weight: 600;
    font-size: var(--fs-small);
    color: var(--ink-quiet);
    height: 32px;
    border-bottom: 1px solid var(--rule-strong);
    padding: 0 var(--s3);
  }
  .grid td {
    height: 36px;
    padding: 0 var(--s3);
    border-bottom: 1px solid var(--rule);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .grid tbody tr:hover {
    background: var(--sunk);
  }
  .grid tbody tr.sel td {
    background: var(--accent-soft);
  }
  .grid tbody tr.sel td:first-child {
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .grid tr.dim td:first-child {
    color: var(--ink-quiet);
  }
  .grid tr.pagehead th,
  .grid tr.section th {
    position: static;
    background: transparent;
    border-bottom: 1px solid var(--rule-strong);
  }
  .grid tr.pagehead th {
    height: 40px;
    padding-top: var(--s3);
    color: var(--ink);
    font-size: var(--fs-body);
  }
  .grid tr.section th {
    height: 32px;
    padding-top: var(--s2);
    color: var(--ink-quiet);
  }
  .tag {
    margin-left: var(--s2);
    font-size: 11.5px;
  }
  .empty {
    padding: var(--s5) var(--s3);
  }
  .more {
    padding: var(--s3);
  }
  .inspector {
    border-left: 1px solid var(--rule);
    background: var(--surface);
    padding: var(--s4) var(--s4) var(--s5);
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }
  .inspector h2 {
    font-size: var(--fs-lead);
  }
  .inspector h3 {
    font-size: var(--fs-body);
    margin-top: var(--s3);
  }
  .crumb {
    font-size: var(--fs-small);
  }
  .defbox {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--s2) var(--s3);
    border-radius: var(--radius);
    background: var(--sunk);
  }
  .bioshelp {
    white-space: pre-line;
    font-size: var(--fs-small);
    color: var(--ink-quiet);
  }
  .choices {
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--fs-small);
  }
  .choices li {
    display: flex;
    justify-content: space-between;
    gap: var(--s2);
    padding: 4px var(--s2);
    border-radius: var(--radius);
  }
  .choices li.def {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  .note {
    font-size: var(--fs-small);
  }
  .ev {
    display: grid;
    grid-template-columns: 80px 1fr;
    gap: 4px var(--s2);
    margin: 0;
    font-size: var(--fs-small);
  }
  .ev dt {
    color: var(--ink-quiet);
  }
  .ev dd {
    margin: 0;
  }
  .problem {
    padding: var(--s3) var(--s4);
    border-left: 3px solid var(--red);
    background: var(--red-soft);
    border-radius: 0 var(--radius) var(--radius) 0;
  }
  @media (max-width: 1180px) {
    .work {
      grid-template-columns: 190px minmax(0, 1fr) 290px;
    }
  }
</style>
