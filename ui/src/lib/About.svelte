<script lang="ts">
  // About: version, the update check's result with the newer releases' notes, and this
  // version's own changelog (built in, so it shows offline).
  import { api } from "./api";
  import { about, checkForUpdates, parseChangelog, type ChangeEntry } from "./updates.svelte";
  import Status from "./Status.svelte";

  const history = $derived<ChangeEntry[]>(about.info ? parseChangelog(about.info.changelog) : []);
  const upcoming = $derived<ChangeEntry[]>(
    about.newer.flatMap((r) => {
      const parsed = parseChangelog(r.notes);
      return parsed.length ? parsed : [{ version: r.version, date: r.published_at.slice(0, 10), sections: [] }];
    }),
  );
  const latest = $derived(about.newer[0] ?? null);
  const repo = $derived(about.info?.repo_url ?? "https://github.com/wapenshaw/ocb-inspector");

  function when(d: Date | null) {
    return d ? d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : "";
  }
  function day(iso: string) {
    if (!iso) return "";
    const d = new Date(`${iso}T00:00:00`);
    return isNaN(d.getTime()) ? iso : d.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
  }
</script>

<div class="scroll">
  <div class="page">
    <header class="id">
      <img src="/icon.svg" width="72" height="72" alt="" />
      <div>
        <h1>OCB Inspector</h1>
        <p class="quiet">Thermal vision for your BIOS profiles</p>
        <p class="ver"><span class="mono">v{about.info?.version ?? "…"}</span><span class="faint">MIT licence</span></p>
      </div>
    </header>

    <section class="update" aria-live="polite">
      <div class="state">
        {#if about.state === "checking"}
          <Status tone="neutral" text="Checking GitHub for a newer version…" />
        {:else if about.state === "available" && latest}
          <Status tone="attention" text="Version {latest.version} is available" />
          <p class="quiet">
            {about.newer.length === 1 ? "One release" : `${about.newer.length} releases`} since yours. Download the zip from the release
            page and unzip it over this folder; your <span class="mono">config</span> folder stays as it is.
          </p>
        {:else if about.state === "current"}
          <Status tone="ok" text="You have the latest version" />
          <p class="quiet">Checked at {when(about.checkedAt)}.</p>
        {:else if about.state === "error"}
          <Status tone="blocked" text="Could not check for updates" />
          <p class="quiet">{about.error}</p>
        {:else if about.state === "off"}
          <Status tone="neutral" text="Update check is off" />
          <p class="quiet">Turned off in <span class="mono">config/settings.json</span>. You can still check now.</p>
        {/if}
      </div>
      <div class="actions">
        {#if latest}
          <button class="btn primary" onclick={() => api.openPage(latest.url)}>Open the v{latest.version} release</button>
        {/if}
        <button class="btn" onclick={checkForUpdates} disabled={about.state === "checking"}>Check now</button>
      </div>
    </section>

    {#if upcoming.length}
      <section>
        <h2>What's new since v{about.info?.version}</h2>
        {@render entries(upcoming, true)}
      </section>
    {/if}

    <section>
      <h2>Changelog</h2>
      {#if history.length}
        {@render entries(history, false)}
      {:else}
        <p class="quiet">No changelog in this build.</p>
      {/if}
      <p class="more"><button class="link" onclick={() => api.openPage(`${repo}/releases`)}>All releases on GitHub</button></p>
    </section>

    <section class="fine">
      <h2>About this app</h2>
      <p>
        OCB Inspector reads MSI BIOS OC profiles and BIOS images on this computer. Your files are never uploaded. The one
        thing it sends over the network is the update check: a request for the public list of releases on GitHub,
        which carries nothing about you or your files. Add <span class="mono">"update_check": false</span> to
        <span class="mono">config/settings.json</span> to stop it.
      </p>
      <p>
        Free and open source under the MIT licence. The third-party code inside the app and its licences are listed in
        <span class="mono">THIRD-PARTY-NOTICES.txt</span> beside the program.
      </p>
      <p><button class="link" onclick={() => api.openPage(repo)}>Source code, issues and profile reports on GitHub</button></p>
    </section>
  </div>
</div>

{#snippet entries(list: ChangeEntry[], fresh: boolean)}
  <ol class="log">
    {#each list as e (e.version)}
      <li class:fresh>
        <h3><span class="mono">v{e.version}</span>{#if e.date}<span class="faint">{day(e.date)}</span>{/if}</h3>
        {#each e.sections as s (s.title)}
          <h4>{s.title}</h4>
          <ul>
            {#each s.items as item, i (i)}<li>{item}</li>{/each}
          </ul>
        {/each}
      </li>
    {/each}
  </ol>
{/snippet}

<style>
  .scroll {
    height: 100%;
    overflow: auto;
  }
  .page {
    max-width: 760px;
    padding: 48px var(--s6) 64px;
    display: flex;
    flex-direction: column;
    gap: var(--s6);
  }
  .id {
    display: flex;
    align-items: center;
    gap: var(--s4);
  }
  .id img {
    border-radius: 14px;
    box-shadow: 0 0 0 1px var(--rule-strong);
  }
  h1 {
    font-size: 26px;
    font-weight: 700;
    letter-spacing: -0.01em;
    line-height: 1.2;
  }
  .ver {
    display: flex;
    gap: var(--s3);
    margin-top: var(--s1);
    font-size: var(--fs-small);
  }
  .update {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s4);
    padding: var(--s4);
    border: 1px solid var(--rule-strong);
    border-radius: var(--radius);
    background: var(--surface);
  }
  .state {
    display: flex;
    flex-direction: column;
    gap: var(--s1);
    min-width: 0;
  }
  .state p {
    font-size: var(--fs-small);
    max-width: 52ch;
  }
  .actions {
    display: flex;
    gap: var(--s2);
    flex: none;
  }
  h2 {
    font-size: var(--fs-lead);
    font-weight: 600;
    margin-bottom: var(--s3);
  }
  .log {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--s4);
    padding: 0;
    margin: 0;
  }
  .log > li {
    padding-left: var(--s4);
    border-left: 3px solid var(--rule-strong);
  }
  .log > li.fresh {
    border-left-color: var(--hot);
  }
  h3 {
    display: flex;
    align-items: baseline;
    gap: var(--s3);
    font-size: var(--fs-body);
    font-weight: 600;
  }
  h3 .faint {
    font-weight: 400;
    font-size: var(--fs-small);
  }
  h4 {
    margin-top: var(--s2);
    font-size: var(--fs-small);
    font-weight: 600;
    color: var(--ink-quiet);
  }
  .log ul {
    margin: var(--s1) 0 0;
    padding-left: 1.2em;
  }
  .log ul li {
    line-height: 1.45;
  }
  .more {
    margin-top: var(--s3);
  }
  .fine p {
    max-width: 66ch;
    line-height: 1.5;
    margin-bottom: var(--s2);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
</style>
