<script lang="ts">
  import { api, errorMessage, isCommandError, pickFile } from "./api";
  import type { BiosSummary, InspectPayload, Report } from "./types";
  import { cpuShort, fileName, savedAt, shortHash } from "./format";
  import Icon from "./Icon.svelte";
  import Status from "./Status.svelte";

  let {
    heading,
    hint,
    bios = $bindable(""),
    profile = $bindable(""),
    report = $bindable(null),
    onloaded,
    onbrowse,
  }: {
    heading: string;
    hint: string;
    bios?: string;
    profile?: string;
    report?: Report<InspectPayload> | null;
    onloaded?: (r: Report<InspectPayload> | null) => void;
    /** Open the chosen BIOS on its own. When set, images that cannot take a profile open directly. */
    onbrowse?: (bios: string) => void;
  } = $props();

  const uid = $derived(heading.toLowerCase().replace(/\W+/g, "-"));
  let summary = $state<BiosSummary | null>(null);
  let busy = $state<"" | "bios" | "pair">("");
  let error = $state<{ text: string; mismatch: boolean } | null>(null);
  const profilesOk = $derived(summary?.profiles_supported !== false);

  export async function chooseBios() {
    const p = await pickFile("bios");
    if (!p) return;
    bios = p;
    summary = null;
    error = null;
    busy = "bios";
    try {
      summary = await api.biosSummary(p);
    } catch (e) {
      error = { text: errorMessage(e), mismatch: false };
      bios = "";
    } finally {
      busy = "";
    }
    if (summary && summary.profiles_supported === false && onbrowse) {
      onbrowse(p);
      return;
    }
    await pair();
  }


  export async function chooseProfile() {
    const p = await pickFile("profile");
    if (!p) return;
    profile = p;
    await pair();
  }

  async function pair() {
    report = null;
    onloaded?.(null);
    if (!bios || !profile || !profilesOk) return;
    error = null;
    busy = "pair";
    try {
      report = await api.inspect(bios, profile);
      summary = report.payload.bios;
      onloaded?.(report);
    } catch (e) {
      error = { text: errorMessage(e).replace(/^BIOS\/profile identity mismatch: /, ""), mismatch: isCommandError(e) && e.exit_code === 4 };
    } finally {
      busy = "";
    }
  }
</script>

<section class="pair" aria-label={heading}>
  <header>
    <h2>{heading}</h2>
    <p class="quiet">{hint}</p>
  </header>

  <div class="slots">
    <button class="slot" class:filled={!!summary} onclick={chooseBios} disabled={busy !== ""} aria-describedby="{uid}-bios-desc">
      <span class="kind"><Icon name="chip" /> BIOS image</span>
      {#if busy === "bios"}
        <span class="quiet">Reading firmware volumes…</span>
      {:else if summary}
        <span class="id mono">{summary.bios_id}</span>
        {#if summary.platform}
          <span class="meta">{summary.platform.version}, {summary.platform.family}{summary.platform.chipset ? ` ${summary.platform.chipset}` : ""}</span>
          {#if summary.platform.models}<span class="meta quiet">{summary.platform.models}</span>{/if}
        {/if}
        <span class="meta quiet" id="{uid}-bios-desc">
          Built {summary.build_date}, <span class="mono">{shortHash(summary.sha256)}</span>
        </span>
        {#if summary.approved_for_writing}
          <Status tone="ok" text="Approved release image" />
        {:else if !profilesOk}
          <Status tone="neutral" text="Browse only" />
        {:else}
          <Status tone="attention" text="Not an approved image: inspect only" />
        {/if}
      {:else}
        <span class="cta">Choose BIOS image</span>
        <span class="meta quiet" id="{uid}-bios-desc">The vendor release file, e.g. E7D91IMS.HJ0</span>
      {/if}
    </button>

    <span class="join" aria-hidden="true"></span>

    <button class="slot" class:filled={!!report} class:bad={error?.mismatch} onclick={chooseProfile} disabled={busy !== "" || !profilesOk}>
      <span class="kind"><Icon name="file" /> OC profile</span>
      {#if !profilesOk}
        <span class="quiet">Not available for this BIOS</span>
        <span class="meta quiet">{summary?.platform?.profiles_note ?? ""}</span>
      {:else if busy === "pair"}
        <span class="quiet">Checking the pair…</span>
      {:else if report}
        <span class="id">{fileName(profile)}</span>
        <span class="meta quiet">Saved {savedAt(report.payload.profile.saved_at)}, {cpuShort(report.payload.profile.cpu_brand)}</span>
        <Status tone="ok" text="Match verified, checksum valid" />
      {:else if profile}
        <span class="id">{fileName(profile)}</span>
        <span class="meta quiet">{bios ? "" : "Choose the BIOS image that saved it"}</span>
      {:else}
        <span class="cta">Choose OC profile</span>
        <span class="meta quiet">A .ocb file saved from the BIOS</span>
      {/if}
    </button>
  </div>

  {#if summary && onbrowse && !report}
    <div class="browseoffer">
      <button class="btn primary" onclick={() => onbrowse?.(bios)}><Icon name="chip" /> Browse BIOS settings</button>
      <span class="quiet">
        {profilesOk ? "See every setting and its optimized default without a profile." : "Profiles cannot be opened for this BIOS, but every setting can be browsed."}
      </span>
    </div>
  {/if}

  {#if error}
    <div class="error" role="alert">
      <Status tone="blocked" text={error.mismatch ? "Profile and BIOS do not match" : "Could not open"} />
      <p>{error.text}</p>
      {#if error.mismatch}
        <p class="quiet">Settings stay closed until the profile is paired with the BIOS image that saved it.</p>
        <button class="btn" onclick={chooseBios}>Choose matching BIOS</button>
      {/if}
    </div>
  {/if}
</section>

<style>
  .pair {
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }
  header h2 {
    font-size: var(--fs-lead);
  }
  header p {
    margin-top: 2px;
  }
  .slots {
    display: grid;
    grid-template-columns: 1fr 16px 1fr;
    align-items: stretch;
  }
  .join {
    align-self: center;
    height: 1px;
    background: var(--rule-strong);
  }
  .slot {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    min-height: 104px;
    padding: var(--s3) var(--s4);
    text-align: left;
    background: transparent;
    border: 1px dashed var(--rule-strong);
    border-radius: var(--radius-lg);
    cursor: pointer;
  }
  .slot:hover:not(:disabled) {
    border-color: var(--accent);
  }
  .slot.filled {
    border-style: solid;
    border-color: var(--rule);
    background: var(--surface);
  }
  .slot.bad {
    border-color: var(--red);
  }
  .kind {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-quiet);
    font-size: var(--fs-small);
  }
  .id {
    font-size: var(--fs-lead);
    font-weight: 600;
  }
  .id.mono {
    font-size: 15px;
  }
  .cta {
    color: var(--accent);
    font-weight: 600;
    font-size: var(--fs-lead);
  }
  .meta {
    font-size: var(--fs-small);
  }
  .browseoffer {
    display: flex;
    align-items: center;
    gap: var(--s3);
  }
  .slot:disabled {
    cursor: not-allowed;
  }
  .error {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--s2);
    padding: var(--s3) var(--s4);
    border-left: 3px solid var(--red);
    background: var(--red-soft);
    border-radius: 0 var(--radius) var(--radius) 0;
  }
</style>
