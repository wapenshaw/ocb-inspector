<script lang="ts">
  // Two profiles side by side: every setting, reviewed or not, that differs between them.
  // Works across BIOS releases (settings are matched by menu page and name). Read-only.
  import { untrack } from "svelte";
  import { api, errorMessage } from "./api";
  import type { CompareEntry, DiffPayload, InspectPayload, Report, Value } from "./types";
  import { display, fileName, savedAt } from "./format";
  import { menuSort, pageBefore, sectionBefore } from "./menu";
  import PairPicker from "./PairPicker.svelte";
  import Status from "./Status.svelte";
  import ValueText from "./ValueText.svelte";
  import Icon from "./Icon.svelte";

  let aBios = $state("");
  let aProfile = $state("");
  let bBios = $state("");
  let bProfile = $state("");
  let aReport = $state<Report<InspectPayload> | null>(null);
  let bReport = $state<Report<InspectPayload> | null>(null);

  let diff = $state<Report<DiffPayload> | null>(null);
  let loading = $state(false);
  let error = $state("");
  let query = $state("");
  let show = $state<"all" | "reviewed" | "one">("all");

  const ready = $derived(!!aReport && !!bReport);

  // Keyed on the paired reports, not the paths: a pair whose BIOS was just changed has no report
  // until its profile matches, so a half-chosen pair never triggers a compare.
  $effect(() => {
    const [a, b] = [aReport, bReport];
    untrack(() => {
      if (a && b) load(aBios, aProfile, bBios, bProfile);
      else diff = null;
    });
  });

  async function load(lb: string, lp: string, rb: string, rp: string) {
    loading = true;
    error = "";
    try {
      diff = await api.diff(lb, lp, rb, rp);
    } catch (e) {
      error = errorMessage(e);
      diff = null;
    } finally {
      loading = false;
    }
  }

  function reset() {
    aReport = null;
    bReport = null;
    diff = null;
  }

  const entries = $derived(diff?.payload.all_differences ?? []);
  const sorted = $derived(
    menuSort(entries).map((e) => ({
      e,
      hay: `${e.label} ${(e.menu?.path ?? []).join(" ")} ${display(e.left)} ${display(e.right)}`.toLowerCase(),
    })),
  );
  const rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return sorted
      .filter(({ e, hay }) => {
        if (show === "reviewed" && !e.reviewed) return false;
        if (show === "one" && !e.only) return false;
        return !q || hay.includes(q);
      })
      .map(({ e }) => e);
  });
  const counts = $derived({
    reviewed: entries.filter((e) => e.reviewed).length,
    one: entries.filter((e) => e.only).length,
  });
  const aId = $derived(aReport?.payload.bios.bios_id ?? "");
  const bId = $derived(bReport?.payload.bios.bios_id ?? "");

  const sameStock = (e: CompareEntry) => display(e.left_default) === display(e.right_default);
  const stockOf = (v: Value | null) => (v ? display(v) : "—");
</script>

{#if !ready || !aReport || !bReport}
  <div class="scroll">
    <div class="setup">
      <div class="intro">
        <h1>Compare two profiles</h1>
        <p class="quiet">
          Open two profiles, each with the BIOS that saved it. Every setting is compared by its place in the BIOS menus, so
          profiles from different BIOS versions line up too. Nothing is changed.
        </p>
      </div>
      <div class="pairs">
        <PairPicker heading="First profile" hint="A profile and the BIOS that saved it." bind:bios={aBios} bind:profile={aProfile} bind:report={aReport} />
        <PairPicker heading="Second profile" hint="Another profile, from the same BIOS or a different version." bind:bios={bBios} bind:profile={bProfile} bind:report={bReport} />
      </div>
    </div>
  </div>
{:else}
  <div class="strip" role="group" aria-label="Profiles compared">
    <span class="mono strong">{aId}</span>
    <span class="quiet">{fileName(aProfile)}, saved {savedAt(aReport.payload.profile.saved_at)}</span>
    <span class="flow" aria-label="compared with">⇄</span>
    <span class="mono strong">{bId}</span>
    <span class="quiet">{fileName(bProfile)}, saved {savedAt(bReport.payload.profile.saved_at)}</span>
    <span class="spacer"></span>
    <button class="btn ghost" onclick={reset}>Change files</button>
  </div>

  <div class="body">
    <div class="tools">
      <label class="search">
        <Icon name="search" />
        <span class="visually-hidden">Search differences</span>
        <input type="search" placeholder="Search differences" bind:value={query} />
      </label>
      <div class="seg" role="radiogroup" aria-label="Show">
        <label class:on={show === "all"}><input type="radio" name="cmp" value="all" bind:group={show} />All differences <span class="mono faint">{entries.length}</span></label>
        <label class:on={show === "reviewed"}><input type="radio" name="cmp" value="reviewed" bind:group={show} />Reviewed <span class="mono faint">{counts.reviewed}</span></label>
        <label class:on={show === "one"} class:off={!counts.one}><input type="radio" name="cmp" value="one" bind:group={show} disabled={!counts.one} />In one BIOS only <span class="mono faint">{counts.one}</span></label>
      </div>
      <span class="summary quiet">
        {#if loading}Comparing…{:else if diff}{entries.length} of {diff.payload.compared ?? 0} settings differ{#if !diff.payload.same_bios}, across two BIOS versions{/if}{/if}
      </span>
    </div>

    {#if error}
      <div class="problem" role="alert"><Status tone="blocked" text="Cannot compare" /><p>{error}</p></div>
    {:else if diff}
      <div class="tablewrap">
        <table class="grid">
          <colgroup><col style="width:34%" /><col style="width:20%" /><col style="width:20%" /><col /></colgroup>
          <thead>
            <tr>
              <th>Setting</th>
              <th><span class="mono">{aId}</span> <span class="faint">{fileName(aProfile)}</span></th>
              <th><span class="mono">{bId}</span> <span class="faint">{fileName(bProfile)}</span></th>
              <th>Stock</th>
            </tr>
          </thead>
          <tbody>
            {#each rows as e, i (e.key)}
              {@const page = pageBefore(rows, i)}
              {@const section = sectionBefore(rows, i)}
              {#if page}
                <tr class="pagehead"><th colspan="4" scope="colgroup">{page.length ? page.join(" › ") : "Not on a menu page"}</th></tr>
              {/if}
              {#if section}
                <tr class="section"><th colspan="4" scope="colgroup">{section}</th></tr>
              {/if}
              <tr class:heat-3={!e.only} class:heat-0={!!e.only}>
                <td class="name">{e.label}{#if e.reviewed}<span class="rv" title="Reviewed setting: values, units and dependencies checked">reviewed</span>{/if}</td>
                <td>{#if e.only === "right"}<Status text="Not in {aId}" />{:else}<ValueText value={e.left} />{/if}</td>
                <td>{#if e.only === "left"}<Status text="Not in {bId}" />{:else}<ValueText value={e.right} />{/if}</td>
                <td class="quiet">
                  {#if sameStock(e) || e.only}{stockOf(e.left_default ?? e.right_default)}{:else}{stockOf(e.left_default)} / {stockOf(e.right_default)}{/if}
                </td>
              </tr>
            {:else}
              <tr><td colspan="4" class="quiet empty">{entries.length ? "No differences match." : "These profiles store the same value for every setting."}</td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
{/if}

<style>
  .scroll {
    height: 100%;
    overflow: auto;
  }
  .setup {
    padding: 56px var(--s6);
    max-width: 1240px;
    display: flex;
    flex-direction: column;
    gap: var(--s6);
  }
  .intro h1 {
    font-size: 26px;
    font-weight: 600;
    letter-spacing: -0.015em;
    line-height: 1.2;
  }
  .intro p {
    margin-top: var(--s2);
    max-width: 66ch;
    font-size: var(--fs-lead);
  }
  .pairs {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--s6);
  }
  @media (max-width: 1100px) {
    .pairs {
      grid-template-columns: 1fr;
    }
  }
  .strip {
    display: flex;
    align-items: center;
    gap: var(--s3);
    height: 48px;
    padding: 0 var(--s4);
    border-bottom: 1px solid var(--rule);
    background: var(--surface);
  }
  .strong {
    font-weight: 600;
  }
  .flow {
    color: var(--trace);
    font-weight: 700;
  }
  .spacer {
    flex: 1;
  }
  .body {
    height: calc(100% - 48px);
    display: flex;
    flex-direction: column;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s3);
    padding: var(--s3) var(--s4);
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--s2);
    width: 320px;
    height: 36px;
    padding: 0 var(--s3);
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--ink-faint);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--ink);
    font: inherit;
  }
  .seg {
    display: inline-flex;
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .seg label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 34px;
    padding: 0 var(--s3);
    border-left: 1px solid var(--rule);
    cursor: pointer;
  }
  .seg label:first-child {
    border-left: 0;
  }
  .seg label.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .seg label.off {
    color: var(--ink-faint);
    cursor: default;
  }
  .seg input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }
  .seg label:focus-within {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  .summary {
    margin-left: auto;
  }
  .problem {
    margin: var(--s4);
  }
  .tablewrap {
    flex: 1;
    overflow: auto;
    padding: 0 var(--s4) var(--s5);
  }
  .grid {
    width: 100%;
    max-width: 1440px;
    border-collapse: collapse;
    table-layout: fixed;
  }
  .grid thead th {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--paper);
    text-align: left;
    font-size: var(--fs-small);
    font-weight: 600;
    color: var(--ink-quiet);
    height: 34px;
    padding: 0 var(--s3);
    border-bottom: 1px solid var(--rule-strong);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .grid td {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    height: var(--row);
    padding: 0 var(--s3);
    border-bottom: 1px solid var(--rule);
  }
  .grid tbody td:first-child {
    box-shadow: inset 6px 0 0 var(--heat, transparent);
    padding-left: calc(var(--s3) + 6px);
  }
  .grid td.name {
    white-space: normal;
    line-height: 1.3;
    padding-top: 6px;
    padding-bottom: 6px;
  }
  .grid tr.pagehead th,
  .grid tr.section th {
    text-align: left;
    font-weight: 600;
  }
  .grid tr.pagehead th {
    height: 40px;
    padding: var(--s3) var(--s3) 0;
    color: var(--ink);
    border-bottom: 1px solid var(--rule-strong);
  }
  .grid tr.section th {
    height: 30px;
    padding: var(--s2) var(--s3) 0;
    font-size: var(--fs-small);
    color: var(--ink-quiet);
    border-bottom: 1px solid var(--rule);
  }
  .empty {
    padding: var(--s5) var(--s3);
  }
</style>
