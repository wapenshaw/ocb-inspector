<script lang="ts">
  import { api, errorMessage, pickFile, pickSave } from "./api";
  import type { BuildResult, DecodedSetting, EditPreview, InspectPayload, Operation, Report, Value } from "./types";
  import { cpuShort, display, fileName, hex, sameIntent, savedAt, shortHash } from "./format";
  import { menuPages, menuSort, NOT_IN_BIOS, pageBefore, pageKey, sectionBefore, type Page } from "./menu";
  import PairPicker from "./PairPicker.svelte";
  import Browse from "./Browse.svelte";
  import Status from "./Status.svelte";
  import ValueText from "./ValueText.svelte";
  import ValueEditor from "./ValueEditor.svelte";
  import Checks from "./Checks.svelte";
  import ResultPanel from "./ResultPanel.svelte";
  import Icon from "./Icon.svelte";
  import MenuTree from "./MenuTree.svelte";
  import { app } from "./app.svelte";

  let bios = $state("");
  let profile = $state("");
  let report = $state<Report<InspectPayload> | null>(null);
  let picker = $state<PairPicker | null>(null);
  // A BIOS open on its own (no profile needed), or null. Bindable so the header can show "Browse BIOS" as current.
  let { browsing = $bindable(null) }: { browsing?: string | null } = $props();
  let browser = $state<Browse | null>(null);

  // A BIOS menu page key, "raw" for every BIOS field, or NOT_IN_BIOS.
  let group = $state<string>("");
  let query = $state("");
  // all = everything on the page; defaults = differs from optimized defaults; edited = staged here.
  let filter = $state<"all" | "defaults" | "edited">("all");
  let selectedId = $state<string | null>(null);
  let searchEl = $state<HTMLInputElement | null>(null);

  // Staged edits with undo/redo history.
  let history = $state<Record<string, Value>[]>([{}]);
  let cursor = $state(0);
  const staged = $derived(history[cursor]);
  const changeCount = $derived(Object.keys(staged).length);

  let check = $state<{ state: "idle" | "checking" | "ok" | "error"; message: string; preview: EditPreview | null }>({
    state: "idle",
    message: "",
    preview: null,
  });
  let mode = $state<"table" | "review" | "result">("table");
  let result = $state<BuildResult | null>(null);
  let exporting = $state(false);
  let exportError = $state("");

  const p = $derived(report?.payload ?? null);
  // Columns in the settings table: Proposed only exists when writing is on.
  const cols = $derived(app.writing ? 5 : 4);
  const writable = $derived(!!p?.capabilities.image_approved_for_writing);
  /** Why the image is inspect-only, including where the approvals file was looked for. */
  const approvalNote = $derived(report?.diagnostics.find((d) => d.code === "image_not_approved")?.message ?? "");

  export function openFile() {
    browsing = null;
    report = null;
    picker?.chooseBios();
  }
  export function focusSearch() {
    if (browsing) browser?.focusSearch();
    else searchEl?.focus();
  }
  export function undo() {
    if (cursor > 0) {
      cursor -= 1;
      revalidate();
    }
  }
  export function redo() {
    if (cursor < history.length - 1) {
      cursor += 1;
      revalidate();
    }
  }

  /** Choose a BIOS image and browse it without a profile. */
  export async function browseBios() {
    const path = await pickFile("bios");
    if (path) await openBrowse(path);
  }

  let browsingProfiles = $state(true);
  async function openBrowse(path: string) {
    const s = await api.biosSummary(path).catch(() => null);
    browsingProfiles = s?.profiles_supported !== false;
    browsing = path;
  }

  function loaded(r: Report<InspectPayload> | null) {
    history = [{}];
    cursor = 0;
    mode = "table";
    check = { state: "idle", message: "", preview: null };
    const first = r ? menuSort(r.payload.settings)[0] : null;
    selectedId = first?.setting_id ?? null;
    group = first?.menu ? pageKey(first.menu.path) : "";
    filter = "all";
    query = "";
  }

  const hasDefaults = $derived(!!p?.bios.defaults?.available);
  // Reviewed settings plus every other setting the BIOS menus show (read only).
  const all = $derived<DecodedSetting[]>(p ? [...p.settings, ...(p.bios_settings ?? [])] : []);
  const reviewed = (s: DecodedSetting) => s.group !== "bios";
  const pages = $derived<Page[]>(menuPages(all));
  const absent = $derived(all.filter((s) => !s.menu));
  const keyOf = (s: DecodedSetting) => (s.menu ? pageKey(s.menu.path) : NOT_IN_BIOS);
  const offDefault = (s: DecodedSetting) => s.differs_from_default === true;
  const name = (s: DecodedSetting) => s.menu?.prompt || s.label;

  // Sorted and made searchable once per report; each keystroke only filters (order is kept).
  const sorted = $derived(
    menuSort(all).map((s) => ({
      s,
      key: keyOf(s),
      hay: `${name(s)} ${s.label} ${s.setting_id} ${(s.menu?.path ?? []).join(" ")} ${s.fields.flatMap((f) => f.prompts).join(" ")}`.toLowerCase(),
    })),
  );
  const rows = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const across = !!q || filter !== "all";
    return sorted
      .filter(({ s, key, hay }) => {
        if (q && !hay.includes(q)) return false;
        if (filter === "edited" && !(s.setting_id in staged)) return false;
        if (filter === "defaults" && !offDefault(s)) return false;
        if (!across && key !== group) return false;
        return true;
      })
      .map(({ s }) => s);
  });
  // Rows span several pages when searching or filtering; then each page gets a heading row.
  const spansPages = $derived(new Set(rows.map(keyOf)).size > 1);

  const rawRows = $derived.by(() => {
    if (!p?.raw_fields) return [];
    const q = query.trim().toLowerCase();
    const located = p.raw_fields.map((f) => ({
      f,
      differs: f.default_raw !== null && f.default_raw !== undefined && f.default_raw !== f.raw,
      menu: { path: f.menu_path ?? [], section: f.section ?? null, order: f.menu_order ?? [] },
    }));
    return menuSort(
      located.filter(
        (r) =>
          (!q || `${r.f.prompt} ${r.f.store} ${r.f.offset} ${r.menu.path.join(" ")}`.toLowerCase().includes(q)) &&
          (filter !== "defaults" || r.differs),
      ),
    );
  });
  const rawDiffers = $derived(
    p?.raw_fields?.filter((f) => f.default_raw !== null && f.default_raw !== undefined && f.default_raw !== f.raw).length ?? 0,
  );

  const selected = $derived(all.find((s) => s.setting_id === selectedId) ?? null);
  // Long lists (search, "Not at default" across every page) render in slices.
  const ROW_LIMIT = 500;
  const shownRows = $derived(rows.slice(0, ROW_LIMIT));

  function stage(id: string, v: Value) {
    const s = p?.settings.find((x) => x.setting_id === id);
    if (!s) return;
    const next = { ...staged };
    if (sameIntent(v, s.value)) delete next[id];
    else next[id] = v;
    history = [...history.slice(0, cursor + 1), next];
    cursor = history.length - 1;
    revalidate();
  }

  function revert(id: string) {
    const next = { ...staged };
    delete next[id];
    history = [...history.slice(0, cursor + 1), next];
    cursor = history.length - 1;
    revalidate();
  }

  function revertAll() {
    history = [...history.slice(0, cursor + 1), {}];
    cursor = history.length - 1;
    revalidate();
  }

  function operations(): Operation[] {
    if (!p) return [];
    return Object.entries(staged).map(([id, value]) => ({
      op: "set",
      setting_id: id,
      expected: p.settings.find((s) => s.setting_id === id)!.value!,
      value,
    }));
  }

  let timer: ReturnType<typeof setTimeout> | undefined;
  function revalidate() {
    clearTimeout(timer);
    if (!p || changeCount === 0) {
      check = { state: "idle", message: "", preview: null };
      return;
    }
    check = { ...check, state: "checking" };
    timer = setTimeout(async () => {
      try {
        const preview = await api.editValidate(bios, profile, p.binding, operations());
        check = { state: "ok", message: "", preview };
      } catch (e) {
        check = { state: "error", message: errorMessage(e).replace(/^setting validation failed: /, ""), preview: null };
      }
    }, 200);
  }

  async function exportProfile() {
    if (!p) return;
    const stem = fileName(profile).replace(/\.ocb$/i, "");
    const out = await pickSave(`${stem}-edited.ocb`, "ocb");
    if (!out) return;
    exporting = true;
    exportError = "";
    try {
      result = await api.editBuild(bios, profile, p.binding, operations(), out);
      mode = "result";
    } catch (e) {
      exportError = errorMessage(e);
    } finally {
      exporting = false;
    }
  }

  function rowKey(e: KeyboardEvent, idx: number) {
    const list = rows;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = list[Math.min(list.length - 1, Math.max(0, idx + (e.key === "ArrowDown" ? 1 : -1)))];
      if (n) {
        selectedId = n.setting_id;
        document.getElementById(`row-${n.setting_id}`)?.focus();
      }
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      selectedId = list[idx].setting_id;
      document.getElementById("inspector-edit")?.focus();
    }
  }

  function statusOf(s: DecodedSetting): { tone: "ok" | "attention" | "blocked" | "neutral" | "accent" | "mod"; text: string } {
    if (s.setting_id in staged) return { tone: "accent", text: "Edited" };
    if (!s.available) return { tone: "neutral", text: "Not in this BIOS" };
    if (s.value?.kind === "unknown") return { tone: "attention", text: "Unknown value" };
    if (offDefault(s)) return { tone: "mod", text: "Not at default" };
    if (app.writing && !s.editability.writable) return { tone: "neutral", text: "Read only" };
    if (s.activity === "inactive") return { tone: "neutral", text: "Inactive" };
    return { tone: "neutral", text: "" };
  }
  /** Heat level for a row: 4 staged, 3 off stock, 0 stock Auto, 1 stock fixed value ("" when defaults are unknown). */
  function heatOf(s: DecodedSetting): string {
    if (s.setting_id in staged) return "heat-4";
    if (!hasDefaults || !s.available) return "";
    if (offDefault(s)) return "heat-3";
    return s.value?.kind === "auto" || (s.value?.kind === "enum" && s.value.label === "Auto") ? "heat-0" : "heat-1";
  }
  const HEAT_TEXT: Record<string, string> = {
    "heat-4": "edit staged",
    "heat-3": "off stock",
    "heat-0": "stock, Auto",
    "heat-1": "stock, fixed value",
  };
  const byPage = $derived.by(() => {
    const m = new Map<string, DecodedSetting[]>();
    for (const s of all) {
      const k = keyOf(s);
      const list = m.get(k);
      if (list) list.push(s);
      else m.set(k, [s]);
    }
    return m;
  });
  const inPage = (key: string) => byPage.get(key) ?? [];
  const stagedIn = (key: string) => inPage(key).filter((s) => s.setting_id in staged).length;
  const differsIn = (key: string) => inPage(key).filter(offDefault).length;
  const differsAll = $derived(all.filter(offDefault).length);
  function openPage(key: string) {
    group = key;
    query = "";
    filter = "all";
  }
</script>

{#if browsing}
  <Browse
    bind:this={browser}
    bios={browsing}
    closeLabel={p ? "Back to profile" : "Change files"}
    onclose={() => (browsing = null)}
    onprofile={p ? undefined : browsingProfiles ? () => { browsing = null; picker?.chooseProfile(); } : undefined}
  />
{/if}
<!-- The picker stays mounted while browsing so the chosen BIOS is kept. -->
{#if !p}
  <div class="open" hidden={!!browsing}>
    <div class="intro">
      <h1>See what an OC profile really stores</h1>
      <p class="quiet">
        Pair a profile with the BIOS image that saved it, or open a BIOS on its own to browse every setting it defines.
        Settings are decoded from the firmware's own definitions, not from the file name.
        {app.writing ? "Nothing is changed until you export a new file." : "This app only reads: it never changes or writes a file."}
      </p>
    </div>
    <PairPicker bind:this={picker} heading="Profile to inspect" hint="Both files stay on this computer." bind:bios bind:profile bind:report onloaded={loaded} onbrowse={openBrowse} />
  </div>
{:else if browsing}
  <!-- profile view resumes when browsing closes -->
{:else if mode === "result" && result}
  <div class="scroll"><ResultPanel {result} ondone={() => { mode = "table"; result = null; report = null; }} /></div>
{:else}
  <div class="strip" role="group" aria-label="Open pair">
    <span class="mono strong">{p.bios.bios_id}</span>
    <Icon name="arrow" />
    <span class="strong">{fileName(profile)}</span>
    <Status tone="ok" text="Match verified" />
    <span class="quiet">Saved {savedAt(p.profile.saved_at)}</span>
    <span class="quiet">{cpuShort(p.profile.cpu_brand)}</span>
    <span class="spacer"></span>
    {#if app.writing && !writable}<Status tone="attention" text="Inspect only: image not approved" title={approvalNote} />{/if}
    <button class="btn ghost" onclick={() => openBrowse(bios)}>Browse whole BIOS</button>
    <button class="btn ghost" onclick={() => { report = null; }}>Change files</button>
  </div>

  {#if mode === "review"}
    <div class="scroll">
      <section class="review" aria-labelledby="review-title">
        <h2 id="review-title">Review {changeCount} {changeCount === 1 ? "change" : "changes"}</h2>
        <p class="quiet">
          A new profile is written next to the original, which stays untouched. Only the bytes of these settings and the
          checksum change.
        </p>
        <table class="changes">
          <thead><tr><th>Setting</th><th>Saved</th><th>New</th></tr></thead>
          <tbody>
            {#each Object.entries(staged) as [id, v] (id)}
              {@const s = p.settings.find((x) => x.setting_id === id)}
              <tr>
                <td>{s ? name(s) : id}{#if s?.menu}<span class="crumb faint">{s.menu.path.join(" › ")}</span>{/if}</td>
                <td><ValueText value={s?.value} strike /></td><td class="strong"><ValueText value={v} /></td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if check.state === "ok" && check.preview}
          <h3>Dry run</h3>
          <Checks checks={check.preview.checks} />
        {:else if check.state === "error"}
          <div class="problem" role="alert"><Status tone="blocked" text="Cannot export yet" /><p>{check.message}</p></div>
        {/if}
        {#if exportError}<div class="problem" role="alert"><Status tone="blocked" text="Export failed" /><p>{exportError}</p></div>{/if}
        <div class="actions">
          <button class="btn" onclick={() => (mode = "table")}>Back to settings</button>
          <button class="btn primary" disabled={check.state !== "ok" || exporting || !writable} onclick={exportProfile}>
            {exporting ? "Exporting…" : "Export profile"}
          </button>
        </div>
      </section>
    </div>
  {:else}
    <div class="work" class:nobar={!app.writing}>
      <nav class="rail" aria-label="BIOS menu">
        <p class="railhead faint">BIOS menu</p>
        <MenuTree {pages} active={!query && filter === "all" ? group : null} onopen={openPage}>
          {#snippet count(pg)}
            {@const n = inPage(pg.key).length}
            {@const d = differsIn(pg.key)}
            {@const e = stagedIn(pg.key)}
            {#if e}<span class="dotc" title="{e} edited">{e}</span>{/if}
            {#if d}<span class="modc" title="{d} not at default">≠{d}</span>{/if}
            <span title="{n} settings">{n}</span>
          {/snippet}
        </MenuTree>
        {#if absent.length}
          <button class="grp" class:on={group === NOT_IN_BIOS && !query && filter === "all"} onclick={() => openPage(NOT_IN_BIOS)}>
            <span class="title">Not in this BIOS</span><span class="count mono">{absent.length}</span>
          </button>
        {/if}
        <div class="sep"></div>
        <button class="grp" class:on={group === "raw"} onclick={() => { group = "raw"; query = ""; filter = "all"; }}>
          <span class="title">All BIOS fields</span>
          <span class="count mono">{#if rawDiffers}<span class="modc" title="{rawDiffers} not at default">≠{rawDiffers}</span>{/if}{p.raw_fields?.length ?? 0}</span>
        </button>
        <p class="railnote faint">Every named Setup and CpuSetup field, including ones the BIOS never shows.</p>
      </nav>

      <div class="main">
        <div class="tools">
          <label class="search">
            <Icon name="search" />
            <span class="visually-hidden">Search settings</span>
            <input bind:this={searchEl} type="search" placeholder={group === "raw" ? "Search all BIOS fields" : "Search settings"} bind:value={query} />
          </label>
          <div class="seg" role="radiogroup" aria-label="Show">
            <label class:on={filter === "all"}><input type="radio" name="flt" value="all" bind:group={filter} />All</label>
            <label class:on={filter === "defaults"} class:off={!hasDefaults}>
              <input type="radio" name="flt" value="defaults" bind:group={filter} disabled={!hasDefaults} />Not at default
              <span class="mono faint">{group === "raw" ? rawDiffers : differsAll}</span>
            </label>
            {#if group !== "raw" && app.writing}
              <label class:on={filter === "edited"}><input type="radio" name="flt" value="edited" bind:group={filter} />Edited <span class="mono faint">{changeCount}</span></label>
            {/if}
          </div>
        </div>
        {#if group !== "raw" && rows.length}
          <div class="heatstrip" role="group" aria-label="Settings shown, coloured by distance from optimized defaults">
            {#each shownRows as s (s.setting_id)}
              {@const h = heatOf(s)}
              <button class="cell {h}" class:sel={selectedId === s.setting_id} title="{name(s)}: {HEAT_TEXT[h] ?? 'no default to compare'}"
                aria-label="{name(s)}: {HEAT_TEXT[h] ?? 'no default to compare'}" onclick={() => (selectedId = s.setting_id)}></button>
            {/each}
          </div>
        {/if}
        <p class="legend">
          {#if hasDefaults}
            <span class="key"><i class="heat-0" aria-hidden="true"></i>Stock, Auto</span>
            <span class="key"><i class="heat-1" aria-hidden="true"></i>Stock, fixed value</span>
            <span class="key"><i class="heat-3" aria-hidden="true"></i>Off stock</span>
            {#if app.writing}<span class="key"><i class="heat-4" aria-hidden="true"></i>Off stock, edit staged</span>{/if}
            <span class="faint" title={p.bios.defaults.source}>Stock = the values “Load Optimized Defaults” restores</span>
          {:else}
            <span class="faint">No default comparison: {p.bios.defaults?.source ?? "this BIOS image has no readable optimized defaults"}.</span>
          {/if}
        </p>

        {#if group === "raw"}
          <div class="tablewrap">
            <table class="grid raw">
              <colgroup><col style="width:36%" /><col style="width:16%" /><col style="width:18%" /><col style="width:16%" /><col /></colgroup>
              <thead><tr><th>BIOS prompt</th><th>Location</th><th>Saved</th><th>Default</th><th>Status</th></tr></thead>
              <tbody>
                {#each rawRows.slice(0, 500) as r, i (`${r.f.store}${r.f.offset}${r.f.prompt}`)}
                  {@const pageHead = pageBefore(rawRows, i)}
                  {#if pageHead}
                    <tr class="pagehead"><th colspan="5" scope="colgroup">{pageHead.length ? pageHead.join(" › ") : "Not on a menu page"}</th></tr>
                  {/if}
                  <tr class={r.differs ? "heat-3" : ""}>
                    <td title={r.f.prompt}>{r.f.prompt}</td>
                    <td class="mono quiet">{r.f.store}+{hex(r.f.offset)}</td>
                    <td><span class="mono">{r.f.raw}</span>{#if r.f.label}&ensp;{r.f.label}{/if}</td>
                    <td class="quiet">{#if r.f.default_raw !== null && r.f.default_raw !== undefined}<span class="mono">{r.f.default_raw}</span>{#if r.f.default_label}&ensp;{r.f.default_label}{/if}{:else}—{/if}</td>
                    <td>{#if r.differs}<Status tone="mod" text="Not at default" />{/if}</td>
                  </tr>
                {:else}
                  <tr><td colspan="5" class="quiet empty">No fields match.</td></tr>
                {/each}
              </tbody>
            </table>
            {#if rawRows.length > 500}<p class="more quiet">Showing 500 of {rawRows.length}. Search to narrow the list.</p>{/if}
          </div>
        {:else}
          <div class="tablewrap">
            <table class="grid" aria-label="Settings">
              {#if app.writing}
                <colgroup><col style="width:30%" /><col style="width:17%" /><col style="width:14%" /><col style="width:15%" /><col /></colgroup>
                <thead><tr><th>Setting</th><th>Saved</th><th>Default</th><th>Proposed</th><th>Status</th></tr></thead>
              {:else}
                <colgroup><col style="width:36%" /><col style="width:22%" /><col style="width:22%" /><col /></colgroup>
                <thead><tr><th>Setting</th><th>Saved</th><th>Default</th><th>Status</th></tr></thead>
              {/if}
              <tbody>
                {#each shownRows as s, i (s.setting_id)}
                  {@const st = statusOf(s)}
                  {@const pageHead = spansPages ? pageBefore(rows, i) : null}
                  {@const section = sectionBefore(rows, i)}
                  {#if pageHead}
                    <tr class="pagehead"><th colspan={cols} scope="colgroup">{pageHead.length ? pageHead.join(" › ") : "Not in this BIOS"}</th></tr>
                  {/if}
                  {#if section}
                    <tr class="section"><th colspan={cols} scope="colgroup">{section}</th></tr>
                  {/if}
                  <tr
                    id="row-{s.setting_id}"
                    tabindex="0"
                    class={heatOf(s)}
                    class:sel={selectedId === s.setting_id}
                    class:dim={!s.available || s.activity === "inactive"}
                    aria-selected={selectedId === s.setting_id}
                    onclick={() => (selectedId = s.setting_id)}
                    onkeydown={(e) => rowKey(e, i)}
                  >
                    <td title={s.label}>{name(s)}{#if reviewed(s)}<span class="rv" title="Reviewed setting: values, units and dependencies checked">reviewed</span>{/if}</td>
                    <td><ValueText value={s.value} /></td>
                    <td class="quiet">{#if s.default_value}<ValueText value={s.default_value} />{:else}<span class="faint">—</span>{/if}</td>
                    {#if app.writing}
                      <td class:strong={s.setting_id in staged}>
                        {#if s.setting_id in staged}<ValueText value={staged[s.setting_id]} />{:else}<span class="faint">No change</span>{/if}
                      </td>
                    {/if}
                    <td>{#if st.text}<Status tone={st.tone} text={st.text} />{/if}</td>
                  </tr>
                {:else}
                  <tr><td colspan={cols} class="quiet empty">
                    {filter === "edited" ? "No edits yet." : filter === "defaults" ? "Every setting here matches the optimized defaults." : "No settings match this search."}
                  </td></tr>
                {/each}
              </tbody>
            </table>
            {#if rows.length > ROW_LIMIT}<p class="more quiet">Showing {ROW_LIMIT} of {rows.length}. Search to narrow the list.</p>{/if}
          </div>
        {/if}
      </div>

      <aside class="inspector" aria-label="Setting details">
        {#if selected && group !== "raw"}
          {#if selected.menu}<p class="crumb faint">{selected.menu.path.join(" › ")}{#if selected.menu.section}, {selected.menu.section}{/if}</p>{/if}
          <h2>{name(selected)}</h2>
          {#if reviewed(selected)}
            <p class="kind"><span class="rv">reviewed</span> Values, units and dependencies checked against real profiles.</p>
          {:else}
            <p class="kind faint">Read from the BIOS's own menu definition. Not a reviewed setting.</p>
          {/if}
          {#if selected.summary || name(selected) !== selected.label}<p class="quiet">{#if name(selected) !== selected.label}<span class="plain">{selected.label}.</span>{" "}{/if}{selected.summary}</p>{/if}

          {#if app.writing}
          <div class="editbox" id="inspector-edit" tabindex="-1">
            {#if selected.available && selected.editability.writable && selected.constraints}
              <ValueEditor
                label={name(selected)}
                value={staged[selected.setting_id] ?? selected.value}
                constraints={selected.constraints}
                autoOnly={selected.editability.auto_only}
                onchange={(v) => stage(selected.setting_id, v)}
              />
              {#if selected.setting_id in staged}
                <button class="btn ghost small" onclick={() => revert(selected.setting_id)}><Icon name="undo" /> Revert to saved</button>
              {/if}
              {#if selected.default_value && !sameIntent(staged[selected.setting_id] ?? selected.value, selected.default_value) && (!selected.editability.auto_only || selected.default_value.kind === "auto")}
                <button class="btn ghost small" onclick={() => stage(selected.setting_id, selected.default_value!)}>Set to default ({display(selected.default_value)})</button>
              {/if}
            {:else}
              <Status tone="neutral" text="Read only" />
            {/if}
            <p class="reason faint">{selected.editability.reason}</p>
          </div>
          {/if}

          {#if !app.writing && selected.available}
            <div class="savedbox">
              <span class="faint">Saved in this profile</span>
              <strong><ValueText value={selected.value} /></strong>
            </div>
          {/if}

          {#if hasDefaults && selected.available}
            <div class="defbox" class:offdef={offDefault(selected)}>
              {#if offDefault(selected)}
                <Status tone="mod" text="Not at default" />
                <p>Optimized default: <ValueText value={selected.default_value} /></p>
              {:else}
                <Status text="Matches the optimized default" />
              {/if}
            </div>
          {/if}

          {#if !app.writing && selected.constraints?.options.length}
            {@const sv = selected.value}
            {@const dv = selected.default_value}
            {@const rawOf = (v: Value | null | undefined) => (v?.kind === "enum" ? v.raw : v?.kind === "boolean" ? Number(v.value) : undefined)}
            <h3>Choices</h3>
            <ul class="choices">
              {#each selected.constraints.options as o (o.raw)}
                <li class:cur={rawOf(sv) === o.raw}>
                  <span>{o.label}</span>
                  <span class="faint small">{#if rawOf(sv) === o.raw}saved{/if}{#if rawOf(sv) === o.raw && rawOf(dv) === o.raw}{", "}{/if}{#if rawOf(dv) === o.raw}default{/if}</span>
                </li>
              {/each}
            </ul>
          {:else if !app.writing && selected.constraints && (selected.constraints.min != null || selected.constraints.max != null)}
            <h3>Allowed</h3>
            <p class="quiet"><span class="mono">{selected.constraints.min ?? "?"}</span> to <span class="mono">{selected.constraints.max ?? "?"}</span>{#if selected.constraints.unit}&ensp;{selected.constraints.unit}{/if}</p>
          {/if}

          {#if selected.bios_help}
            <h3>BIOS help</h3>
            <p class="bioshelp">{selected.bios_help}</p>
          {/if}
          {#if selected.menu?.also_on?.length}
            <p class="faint reason">Also shown on {selected.menu.also_on.map((x) => x.join(" › ")).join("; ")}.</p>
          {/if}

          <h3>When it applies</h3>
          <p class:quiet={selected.activity !== "inactive"}>{selected.activity_reason}</p>

          {#if selected.fields.length}
            <h3>Where it is stored</h3>
            {#each selected.fields as f}
              <dl class="ev">
                <dt>Field</dt><dd>{f.prompts[0] ?? ""}</dd>
                <dt>Location</dt><dd><span class="mono">{f.store}+{hex(f.store_offset)}</span>, file <span class="mono">{hex(f.file_offset)}</span>, {f.width_bits} bits</dd>
                <dt>Stored bytes</dt><dd class="bytes">{#each f.bytes_hex.split(" ") as b}<span class="solder">{b}</span>{/each} <span class="mono faint">= {f.raw}</span></dd>
                {#if f.prompts.length > 1}<dt>Also called</dt><dd>{f.prompts.slice(1).join(", ")}</dd>{/if}
                <dt>Firmware</dt><dd>{f.callback ? "Handled by a firmware callback" : "Plain stored value"}{f.reset_required ? ", needs a reset" : ""}{f.condition_count ? `, ${f.condition_count} visibility condition${f.condition_count > 1 ? "s" : ""}` : ""}</dd>
              </dl>
            {/each}
          {/if}

          <h3>Evidence</h3>
          <ul class="evidence">{#each selected.evidence as e}<li>{e}</li>{/each}</ul>
        {:else if group === "raw"}
          <h2>All BIOS fields</h2>
          <p class="quiet">
            Every named field the firmware defines in its Setup and CpuSetup stores, grouped by the BIOS page that shows it, with
            the value this profile holds.{app.writing ? " Only reviewed settings can be edited; these are shown for reference." : ""}
          </p>
          {#if hasDefaults}
            <p class="quiet">
              {rawDiffers} of {p.raw_fields?.length ?? 0} fields differ from the optimized defaults. Some are values the firmware
              fills in itself, such as detected or trained settings, not only your own changes.
            </p>
          {/if}
          <dl class="ev">
            <dt>Setup module</dt><dd>{p.bios.setup_module}</dd>
            <dt>Defaults</dt><dd>{p.bios.defaults?.source ?? "—"}</dd>
            <dt>Image</dt><dd class="mono">{shortHash(p.bios.sha256)}</dd>
            <dt>Profile</dt><dd class="mono">{shortHash(p.profile.sha256)}</dd>
            <dt>Checksum</dt><dd class="mono">{p.profile.checksum_stored}</dd>
          </dl>
        {/if}
      </aside>
    </div>

    {#if app.writing}
    <footer class="bar">
      <div class="summary">
        {#if changeCount === 0}
          <span class="quiet">No changes staged. Select a setting to edit it.</span>
        {:else}
          <strong>{changeCount} {changeCount === 1 ? "change" : "changes"} staged</strong>
          {#if check.state === "checking"}<Status text="Checking with the engine…" />
          {:else if check.state === "ok"}<Status tone="ok" text="Valid, ready to export" />
          {:else if check.state === "error"}<Status tone="blocked" text={check.message} />{/if}
        {/if}
      </div>
      <button class="btn ghost" onclick={undo} disabled={cursor === 0} title="Undo (Ctrl+Z)"><Icon name="undo" /> Undo</button>
      <button class="btn ghost" onclick={redo} disabled={cursor >= history.length - 1} title="Redo (Ctrl+Shift+Z)"><Icon name="redo" /> Redo</button>
      <button class="btn" onclick={revertAll} disabled={changeCount === 0}>Revert all</button>
      <button class="btn primary" onclick={() => (mode = "review")} disabled={changeCount === 0 || check.state !== "ok" || !writable}>Review changes</button>
    </footer>
    {/if}
  {/if}
{/if}

<style>
  .open[hidden] {
    display: none;
  }
  .open {
    max-width: 880px;
    padding: 56px var(--s6);
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
    max-width: 62ch;
    font-size: var(--fs-lead);
  }
  .scroll {
    height: 100%;
    overflow: auto;
  }
  .strip {
    display: flex;
    align-items: center;
    gap: var(--s3);
    height: 48px;
    padding: 0 var(--s4);
    border-bottom: 1px solid var(--rule);
    background: var(--surface);
    color: var(--ink-quiet);
  }
  .strong {
    color: var(--ink);
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .work {
    display: grid;
    grid-template-columns: 232px minmax(0, 1fr) 340px;
    height: calc(100% - 48px - 56px);
  }
  .kind {
    margin-top: 2px;
    font-size: var(--fs-small);
  }
  .kind .rv {
    margin: 0 4px 0 0;
  }
  .savedbox {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: var(--s3);
    padding: var(--s2) var(--s3);
    border-radius: var(--radius);
    background: var(--sunk);
    font-size: var(--fs-lead);
  }
  .savedbox .faint {
    font-size: var(--fs-small);
  }
  .choices {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .choices li {
    display: flex;
    justify-content: space-between;
    gap: var(--s2);
    padding: 3px var(--s2);
    border-radius: var(--radius);
  }
  .choices li.cur {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .small {
    font-size: var(--fs-small);
  }
  .work.nobar {
    height: calc(100% - 48px);
  }
  .grp {
    --depth: 0;
    display: flex;
    width: 100%;
    justify-content: space-between;
    align-items: center;
    gap: var(--s2);
    min-height: 34px;
    padding: 6px var(--s3);
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    cursor: pointer;
    text-align: left;
  }
  .grp:hover {
    background: var(--sunk);
  }
  .grp .title {
    min-width: 0;
    line-height: 1.25;
  }
  .modc {
    color: var(--mod);
    font-weight: 700;
  }
  .grp.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .count {
    color: var(--ink-faint);
    font-size: 11.5px;
    display: inline-flex;
    gap: 6px;
  }
  .dotc {
    color: var(--accent);
    font-weight: 700;
  }
  .sep {
    height: 1px;
    background: var(--rule);
    margin: var(--s2) var(--s3);
  }
  .railnote {
    padding: var(--s1) var(--s3);
    font-size: var(--fs-small);
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
    max-width: 380px;
    color: var(--ink-faint);
  }
  .search input {
    flex: 1;
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
    height: 30px;
    padding: 0 var(--s3);
    cursor: pointer;
    border-left: 1px solid var(--rule);
  }
  .seg label:first-child {
    border-left: 0;
  }
  .seg label.on {
    background: var(--accent-soft);
    color: var(--accent);
    font-weight: 600;
  }
  .seg label.off {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .seg input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }
  .seg label:has(input:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }
  .legend {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 var(--s4) var(--s2);
    font-size: var(--fs-small);
    color: var(--ink-quiet);
  }
  .legend {
    flex-wrap: wrap;
    column-gap: var(--s4);
  }
  .key {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .key i {
    width: 12px;
    height: 8px;
    border-radius: 2px;
    background: var(--heat);
  }
  .heatstrip {
    display: flex;
    gap: 3px;
    padding: 0 var(--s4) var(--s2);
    max-width: calc(1100px + 2 * var(--s4));
  }
  .heatstrip .cell {
    flex: 1 1 0;
    min-width: 4px;
    height: 14px;
    padding: 0;
    border: 0;
    border-radius: 2px;
    background: var(--heat, var(--rule));
    cursor: pointer;
  }
  .heatstrip .cell.sel {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .bytes {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
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
    height: var(--row);
    padding: 0 var(--s3);
    border-bottom: 1px solid var(--rule);
  }
  .grid tbody tr {
    cursor: default;
  }
  .grid tbody tr:hover {
    background: var(--sunk);
  }
  /* Heat cell: how far the row is from the optimized default (see the legend), plus "Not at default" in the status column. */
  .grid tbody td:first-child {
    box-shadow: inset 6px 0 0 var(--heat, transparent);
    padding-left: calc(var(--s3) + 6px);
  }
  .grid tbody tr.sel td {
    background: var(--accent-soft);
    border-bottom-color: var(--accent);
  }
  .grid tr.pagehead th,
  .grid tr.section th {
    position: static;
    text-align: left;
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
    font-weight: 600;
  }
  .grid td {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .grid tr.dim td:first-child {
    color: var(--ink-quiet);
  }
  .raw td {
    height: 34px;
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
  .editbox {
    margin-top: var(--s2);
    padding: var(--s3);
    border: 1px solid var(--rule);
    border-radius: var(--radius-lg);
    background: var(--paper);
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    align-items: flex-start;
  }
  .editbox :global(select) {
    width: 100%;
  }
  .reason {
    font-size: var(--fs-small);
  }
  .crumb {
    display: block;
    font-size: var(--fs-small);
  }
  .plain {
    color: var(--ink);
  }
  .defbox {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--s2) var(--s3);
    border-radius: var(--radius);
    background: var(--sunk);
  }
  .defbox.offdef {
    background: var(--mod-soft);
    box-shadow: inset 3px 0 0 var(--mod);
  }
  .bioshelp {
    white-space: pre-line;
    font-size: var(--fs-small);
    color: var(--ink-quiet);
  }
  .small {
    height: 28px;
    padding: 0 var(--s2);
  }
  .ev {
    display: grid;
    grid-template-columns: 88px 1fr;
    gap: 4px var(--s2);
    margin: 0 0 var(--s2);
    font-size: var(--fs-small);
  }
  .ev dt {
    color: var(--ink-quiet);
  }
  .ev dd {
    margin: 0;
  }
  .evidence {
    margin: 0;
    padding-left: 16px;
    font-size: var(--fs-small);
    color: var(--ink-quiet);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--s2);
    height: 56px;
    padding: 0 var(--s4);
    border-top: 1px solid var(--rule);
    background: var(--surface);
  }
  .summary {
    flex: 1;
    display: flex;
    align-items: center;
    gap: var(--s3);
    min-width: 0;
    overflow: hidden;
  }
  .review {
    max-width: 920px;
    padding: var(--s5) var(--s6);
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }
  .review h2 {
    font-size: var(--fs-title);
  }
  .review h3 {
    margin-top: var(--s3);
    font-size: var(--fs-lead);
  }
  .changes {
    border-collapse: collapse;
    width: 100%;
  }
  .changes th {
    text-align: left;
    font-size: var(--fs-small);
    color: var(--ink-quiet);
    border-bottom: 1px solid var(--rule-strong);
    padding: 6px 0;
  }
  .changes td {
    padding: 10px var(--s2) 10px 0;
    border-bottom: 1px solid var(--rule);
  }
  .problem {
    padding: var(--s3) var(--s4);
    border-left: 3px solid var(--red);
    background: var(--red-soft);
    border-radius: 0 var(--radius) var(--radius) 0;
  }
  .actions {
    display: flex;
    gap: var(--s2);
    justify-content: flex-end;
    margin-top: var(--s3);
  }
  @media (max-width: 1180px) {
    .work {
      grid-template-columns: 200px minmax(0, 1fr) 300px;
    }
  }
</style>
