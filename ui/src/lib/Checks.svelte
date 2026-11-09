<script lang="ts">
  import type { Check } from "./types";
  import Status from "./Status.svelte";
  let { checks }: { checks: Check[] } = $props();

  const names: Record<string, string> = {
    reparse_and_pair: "Re-read and paired with the BIOS",
    pair_identity: "Profile matches the BIOS",
    checksum: "Checksum",
    layout_preserved: "Record layout unchanged",
    trailer_preserved: "Clock block and metadata unchanged",
    mutation_audit: "Only intended bytes changed",
    intended_values: "Requested values read back",
    retained_values: "Everything else kept",
    dependency_rules: "Dependent settings consistent",
    store_binding: "Settings stores located",
    lossless_roundtrip: "Lossless re-serialization",
    reviewed_settings_decode: "Reviewed settings decode",
    import_acceptance: "Accepted by the BIOS on import",
  };
</script>

<ul class="checks">
  {#each checks as c (c.name)}
    <li class:fail={c.status === "fail"}>
      <span class="state">
        {#if c.status === "pass"}<Status tone="ok" text="Passed" />{:else if c.status === "fail"}<Status
            tone="blocked"
            text="Failed"
          />{:else}<Status tone="attention" text="Not tested" />{/if}
      </span>
      <span class="name">{names[c.name] ?? c.name}</span>
      <span class="detail quiet">{c.detail}</span>
    </li>
  {/each}
</ul>

<style>
  .checks {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: grid;
    grid-template-columns: 96px minmax(180px, 240px) 1fr;
    gap: var(--s3);
    align-items: baseline;
    padding: 9px 0;
    border-bottom: 1px solid var(--rule);
  }
  li:last-child {
    border-bottom: 0;
  }
  .name {
    font-weight: 600;
  }
  .detail {
    font-size: var(--fs-small);
  }
  .fail .detail {
    color: var(--red);
  }
</style>
