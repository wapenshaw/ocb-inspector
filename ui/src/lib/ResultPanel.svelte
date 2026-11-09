<script lang="ts">
  import type { BuildResult } from "./types";
  import { fileName } from "./format";
  import Checks from "./Checks.svelte";
  import Status from "./Status.svelte";
  import ValueText from "./ValueText.svelte";
  let { result, ondone }: { result: BuildResult; ondone: () => void } = $props();
  const m = $derived(result.manifest);
  const failed = $derived(m.checks.some((c) => c.status === "fail"));
</script>

<section class="result" aria-labelledby="result-title">
  <div class="head">
    <Status tone={failed ? "blocked" : "ok"} text={failed ? "Verification failed" : "Written and verified"} />
    <h2 id="result-title">{fileName(result.output_path)}</h2>
    <p class="quiet">
      Saved for <span class="mono">{m.base.bios_id}</span>. The engine re-read the new file from disk bytes and checked it
      against your request. Importing it on the motherboard is the one check only you can run.
    </p>
  </div>

  <dl class="facts">
    <dt>Profile</dt>
    <dd class="mono">{result.output_path}</dd>
    <dt>Report</dt>
    <dd class="mono">{result.report_path}</dd>
    <dt>SHA-256</dt>
    <dd class="mono">{m.output.sha256}</dd>
    <dt>Bytes changed</dt>
    <dd><span class="mono">{m.binary_spans.reduce((n, s) => n + s.end - s.start, 0)}</span> in {m.binary_spans.length} spans, all inside the requested settings and the checksum</dd>
  </dl>

  <h3>Changes</h3>
  {#if m.changes.length === 0}
    <p class="quiet">No values differ from the baseline.</p>
  {:else}
    <table class="changes">
      <tbody>
        {#each m.changes as c (c.setting_id)}
          <tr>
            <td>{c.label}</td>
            <td><ValueText value={c.from} strike /></td>
            <td class="to"><ValueText value={c.to} /></td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  <h3>Verification</h3>
  <Checks checks={m.checks} />

  <h3>Limits of this check</h3>
  <ul class="limits">
    {#each m.limitations as l}<li>{l}</li>{/each}
  </ul>

  <div class="actions"><button class="btn" onclick={ondone}>Done</button></div>
</section>

<style>
  .result {
    max-width: 920px;
    padding: var(--s5) var(--s6) var(--s6);
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }
  .head {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    padding-bottom: var(--s4);
    border-bottom: 1px solid var(--rule);
  }
  .head h2 {
    font-size: var(--fs-title);
  }
  .head p {
    max-width: 68ch;
  }
  h3 {
    font-size: var(--fs-lead);
    margin-top: var(--s4);
  }
  .facts {
    display: grid;
    grid-template-columns: 120px 1fr;
    gap: 6px var(--s3);
    margin: 0;
  }
  dt {
    color: var(--ink-quiet);
  }
  dd {
    margin: 0;
    word-break: break-all;
  }
  .changes {
    border-collapse: collapse;
    width: 100%;
  }
  .changes td {
    padding: 8px var(--s2) 8px 0;
    border-bottom: 1px solid var(--rule);
  }
  .changes .to {
    font-weight: 600;
  }
  .limits {
    margin: 0;
    padding-left: 18px;
    color: var(--ink-quiet);
    max-width: 80ch;
  }
  .limits li {
    margin: 4px 0;
  }
  .actions {
    margin-top: var(--s4);
  }
</style>
