<script lang="ts">
  import { untrack } from "svelte";
  import { api, errorMessage, pickSave } from "./api";
  import type { BuildResult, InspectPayload, MigrationEntry, Patch, Report, ResolutionKind, Value } from "./types";
  import { display, fileName, sameIntent, savedAt } from "./format";
  import PairPicker from "./PairPicker.svelte";
  import ResultPanel from "./ResultPanel.svelte";
  import Status from "./Status.svelte";
  import ValueText from "./ValueText.svelte";
  import ValueEditor from "./ValueEditor.svelte";
  import Icon from "./Icon.svelte";
  import { menuSort, pageBefore, pageKey, sectionBefore } from "./menu";

  let sBios = $state("");
  let sProfile = $state("");
  let tBios = $state("");
  let tProfile = $state("");
  let sReport = $state<Report<InspectPayload> | null>(null);
  let tReport = $state<Report<InspectPayload> | null>(null);

  let groups = $state<string[]>(["cpu.ratios", "cpu.voltage", "cpu.power", "cpu.loadline", "platform"]);
  let plan = $state<Patch | null>(null);
  let planning = $state(false);
  let planError = $state("");
  let building = $state(false);
  let buildError = $state("");
  let result = $state<BuildResult | null>(null);
  let saved = $state("");

  const ready = $derived(!!sReport && !!tReport);
  const m = $derived(plan?.migration ?? null);
  const paths = $derived({ sourceBios: sBios, sourceProfile: sProfile, targetBios: tBios, targetProfile: tProfile });

  async function replan(prior: Patch | null) {
    if (!ready) return;
    planning = true;
    planError = "";
    try {
      plan = await api.migratePlan(paths, { groups, settings: [] }, prior);
    } catch (e) {
      planError = errorMessage(e);
      plan = null;
    } finally {
      planning = false;
    }
  }

  // Plan afresh only when the pair of pairs changes; later edits re-plan with the current plan as prior.
  $effect(() => {
    const r = ready;
    untrack(() => {
      if (r) replan(null);
      else plan = null;
    });
  });

  function withEntries(f: (e: MigrationEntry) => MigrationEntry): Patch | null {
    if (!plan?.migration) return null;
    return { ...plan, migration: { ...plan.migration, selection: { groups, settings: [] }, entries: plan.migration.entries.map(f) } };
  }

  function toggleGroup(id: string, on: boolean) {
    groups = on ? [...groups, id] : groups.filter((g) => g !== id);
    replan(withEntries((e) => (e.group === id ? { ...e, selection: on ? "selected" : "unselected" } : e)));
  }

  function toggleEntry(id: string, on: boolean) {
    replan(withEntries((e) => (e.setting_id === id ? { ...e, selection: on ? "selected" : "unselected", resolution: { kind: "none" } } : e)));
  }

  function resolve(id: string, kind: ResolutionKind, value?: Value) {
    replan(withEntries((e) => (e.setting_id === id ? { ...e, resolution: value ? { kind, value } : { kind } } : e)));
  }

  function targetConstraints(id: string) {
    return tReport?.payload.settings.find((s) => s.setting_id === id) ?? null;
  }
  const sourceSetting = (id: string) => sReport?.payload.settings.find((s) => s.setting_id === id) ?? null;

  // Lay the ledger out like the target BIOS menus; settings missing there fall back to the source's page.
  const located = $derived(
    m ? menuSort(m.entries.map((e) => ({ e, menu: targetConstraints(e.setting_id)?.menu ?? sourceSetting(e.setting_id)?.menu ?? null }))) : [],
  );
  const nameOf = (id: string, fallback: string) => targetConstraints(id)?.menu?.prompt || sourceSetting(id)?.menu?.prompt || fallback;
  /** How the setting's name or page differs between the two BIOS versions. */
  function moved(id: string): string {
    const a = sourceSetting(id)?.menu;
    const b = targetConstraints(id)?.menu;
    if (!a || !b || !sReport) return "";
    const rev = sReport.payload.bios.revision;
    const notes = [];
    if (a.prompt !== b.prompt) notes.push(`called “${a.prompt}” in ${rev}`);
    if (pageKey(a.path) !== pageKey(b.path)) notes.push(`on ${a.path.join(" › ")} in ${rev}`);
    return notes.join(", ");
  }

  type Prov = "source" | "target" | "replaced" | "blocked" | "none";
  function provenance(e: MigrationEntry): Prov {
    if (e.selection === "unselected") return "none";
    switch (e.final_action) {
      case "transfer":
        // A value equal to the target baseline changes nothing; show it as kept.
        return sameIntent(e.proposed_value, e.target_baseline_value) ? "target" : "source";
      case "set":
        return "replaced";
      case "blocked":
        return "blocked";
      default:
        return "target";
    }
  }

  function status(e: MigrationEntry): { tone: "ok" | "attention" | "blocked" | "neutral" | "accent"; text: string } {
    if (e.selection === "unselected") return { tone: "neutral", text: "Not selected" };
    switch (e.final_action) {
      case "blocked":
        return { tone: "blocked", text: "Needs a decision" };
      case "set":
        return { tone: "attention", text: "Replaced" };
      case "retain_target":
        return { tone: "neutral", text: "Keeps target" };
      case "transfer":
        return sameIntent(e.source_value, e.target_baseline_value)
          ? { tone: "neutral", text: "Already matches" }
          : { tone: "accent", text: "Carried over" };
      default:
        return { tone: "neutral", text: "" };
    }
  }

  const resolutionText: Record<ResolutionKind, string> = {
    none: "Carry the source value",
    keep_target: "Keep the target's value",
    omit: "Leave this setting out",
    set_value: "Use a different value",
  };

  const changing = $derived(
    m?.entries.filter((e) => e.selection === "selected" && (e.final_action === "transfer" || e.final_action === "set") && !sameIntent(e.proposed_value, e.target_baseline_value)).length ?? 0,
  );
  const blocked = $derived(m?.accounting.blocked ?? 0);
  const autoCarried = $derived(m?.entries.some((e) => e.final_action === "transfer" && e.proposed_value?.kind === "auto" && !sameIntent(e.proposed_value, e.target_baseline_value)) ?? false);

  async function build() {
    if (!plan) return;
    const stem = fileName(sProfile).replace(/\.ocb$/i, "");
    const out = await pickSave(`${stem}-for-${tReport?.payload.bios.revision ?? "target"}.ocb`, "ocb");
    if (!out) return;
    building = true;
    buildError = "";
    try {
      result = await api.migrateBuild(paths, plan, out);
    } catch (e) {
      buildError = errorMessage(e);
    } finally {
      building = false;
    }
  }

  async function savePlan() {
    if (!plan) return;
    const out = await pickSave(`${fileName(sProfile).replace(/\.ocb$/i, "")}.plan.json`, "json");
    if (!out) return;
    try {
      saved = await api.savePlan(plan, out);
    } catch (e) {
      buildError = errorMessage(e);
    }
  }

  function reset() {
    result = null;
    sReport = null;
    tReport = null;
    plan = null;
  }
</script>

{#if result}
  <div class="scroll"><ResultPanel {result} ondone={reset} /></div>
{:else if !ready || !sReport || !tReport}
  <div class="scroll">
    <div class="setup">
      <div class="intro">
        <h1>Carry your tuning to another BIOS version</h1>
        <p class="quiet">
          Pair the profile you want to keep with the BIOS that saved it. Then open a profile saved by the destination BIOS:
          it becomes the base of the new file, so anything this tool cannot carry keeps that profile's value.
        </p>
      </div>
      <div class="pairs">
        <PairPicker heading="Carry from" hint="Your old profile and the BIOS that saved it." bind:bios={sBios} bind:profile={sProfile} bind:report={sReport} />
        <PairPicker heading="Into" hint="A profile saved by the new BIOS, ideally right after loading defaults." bind:bios={tBios} bind:profile={tProfile} bind:report={tReport} />
      </div>
    </div>
  </div>
{:else}
  <div class="strip" role="group" aria-label="Migration pairs">
    <span class="mono strong">{sReport.payload.bios.bios_id}</span>
    <span class="quiet">{fileName(sProfile)}</span>
    <span class="flow" aria-label="to"><Icon name="arrow" size={16} /></span>
    <span class="mono strong">{tReport.payload.bios.bios_id}</span>
    <span class="quiet">{fileName(tProfile)}, saved {savedAt(tReport.payload.profile.saved_at)}</span>
    <span class="spacer"></span>
    <button class="btn ghost" onclick={reset}>Change files</button>
  </div>

  <div class="body">
    <div class="groups" role="group" aria-label="Setting groups to carry">
      <span class="quiet">Carry</span>
      {#each tReport.payload.groups as g (g.id)}
        <label class="chip" class:on={groups.includes(g.id)}>
          <input type="checkbox" checked={groups.includes(g.id)} onchange={(e) => toggleGroup(g.id, (e.target as HTMLInputElement).checked)} />
          {g.label}
        </label>
      {/each}
      {#if planning}<span class="quiet working">Recalculating…</span>{/if}
    </div>

    {#if planError}
      <div class="problem" role="alert"><Status tone="blocked" text="Cannot plan this migration" /><p>{planError}</p></div>
    {:else if m}
      <div class="tablewrap">
        <table class="ledger" aria-label="Migration plan">
          <colgroup><col style="width:36px" /><col style="width:27%" /><col style="width:20%" /><col style="width:20%" /><col style="width:18%" /><col /></colgroup>
          <thead>
            <tr>
              <th class="sel"><span class="visually-hidden">Carry</span></th>
              <th>Setting</th>
              <th>Source <span class="faint mono">{sReport.payload.bios.revision}</span></th>
              <th>Target baseline <span class="faint mono">{tReport.payload.bios.revision}</span></th>
              <th>Proposed</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {#each located as row, i (row.e.setting_id)}
              {@const e = row.e}
              {@const st = status(e)}
              {@const prov = provenance(e)}
              {@const pageHead = pageBefore(located, i)}
              {@const section = sectionBefore(located, i)}
              {@const mv = moved(e.setting_id)}
              {#if pageHead}
                <tr class="pagehead"><th colspan="6" scope="colgroup">{pageHead.length ? pageHead.join(" › ") : `Not in ${tReport.payload.bios.revision}`}</th></tr>
              {/if}
              {#if section}
                <tr class="section"><th colspan="6" scope="colgroup">{section}</th></tr>
              {/if}
              <tr class:off={e.selection === "unselected"} class:blockedrow={e.final_action === "blocked"}>
                <td class="sel">
                  <input type="checkbox" aria-label="Carry {e.label}" checked={e.selection === "selected"} onchange={(ev) => toggleEntry(e.setting_id, (ev.target as HTMLInputElement).checked)} />
                </td>
                <td class="name" title={e.label}>
                  {nameOf(e.setting_id, e.label)}
                  {#if mv}<span class="moved"><Icon name="arrow" size={11} /> {mv}</span>{/if}
                </td>
                <td><ValueText value={e.source_value} /></td>
                <td><ValueText value={e.target_baseline_value} /></td>
                <td class="proposed {prov}" title={prov === "source" ? "Value comes from the source profile" : prov === "replaced" ? "Value you chose" : prov === "target" ? "Value kept from the target baseline" : ""}>
                  {#if e.final_action === "blocked"}<span class="quiet">Waiting for your decision</span>{:else}<ValueText value={e.proposed_value} />{/if}
                </td>
                <td><Status tone={st.tone} text={st.text} /></td>
              </tr>
              {#if e.selection === "selected" && e.assessment?.status === "unavailable"}
                {@const tc = targetConstraints(e.setting_id)}
                <tr class="decision" class:resolved={e.final_action !== "blocked"}>
                  <td></td>
                  <td colspan="5">
                    <div class="decide">
                      <p class="why">
                        {#if e.assessment.reason === "missing_setting"}This setting does not exist in {tReport.payload.bios.bios_id}.
                        {:else if e.assessment.reason === "unsupported_value"}{tReport.payload.bios.bios_id} cannot represent {display(e.source_value)}.
                        {:else if e.assessment.reason === "ambiguous_semantics"}Its meaning in the target is ambiguous, so it cannot be carried.
                        {:else}This setting cannot be written to the target.{/if}
                        <span class="quiet">{e.assessment.detail}</span>
                      </p>
                      <div class="options" role="radiogroup" aria-label="Decision for {e.label}">
                        {#each e.assessment.permitted_resolutions.filter((k) => k !== "none" && !(k === "keep_target" && !e.target_baseline_value)) as k (k)}
                          <label class="opt">
                            <input type="radio" name="res-{e.setting_id}" checked={e.resolution.kind === k}
                              onchange={() => resolve(e.setting_id, k, k === "set_value" ? (tc?.value ?? undefined) : undefined)} />
                            {resolutionText[k]}{#if k === "keep_target"} <span class="quiet">({display(e.target_baseline_value)})</span>{/if}
                          </label>
                        {/each}
                        {#if e.resolution.kind === "set_value" && tc?.constraints}
                          <div class="setter">
                            <ValueEditor label={e.label} value={e.resolution.value ?? null} constraints={tc.constraints} autoOnly={tc.editability.auto_only}
                              onchange={(v) => resolve(e.setting_id, "set_value", v)} />
                          </div>
                        {/if}
                      </div>
                    </div>
                  </td>
                </tr>
              {/if}
            {/each}
          </tbody>
        </table>

        {#if m.target_only.length}
          <h3 class="sub">Only in {tReport.payload.bios.bios_id}</h3>
          <p class="quiet note">These settings have no counterpart in the source. The new profile keeps the target's values.</p>
          <table class="ledger compact" aria-label="Settings only in the target">
            <colgroup><col style="width:36px" /><col style="width:27%" /><col style="width:20%" /><col style="width:20%" /><col style="width:18%" /><col /></colgroup>
            <tbody>
              {#each m.target_only as t (t.setting_id)}
                <tr><td class="sel"></td><td>{nameOf(t.setting_id, t.label)}{#if targetConstraints(t.setting_id)?.menu}<span class="moved">{targetConstraints(t.setting_id)?.menu?.path.join(" › ")}</span>{/if}</td><td colspan="2" class="quiet">{t.note}</td><td class="proposed target"><ValueText value={t.retained_value} /></td><td><Status text="Keeps target" /></td></tr>
              {/each}
            </tbody>
          </table>
        {/if}

        <div class="legend" aria-label="Proposed column key">
          <span><i class="key source"></i>From the source profile, changes the target value</span>
          <span><i class="key target"></i>Target value kept, no change</span>
          <span><i class="key replaced"></i>A value you chose</span>
          <span><i class="key blocked"></i>Blocked, needs a decision</span>
        </div>
        <p class="quiet note">
          {m.accounting.other_differing_fields} other BIOS fields differ between the two profiles. They are outside the reviewed
          settings, so the new file keeps the target's values for all of them.
          {#if autoCarried}Auto is carried as Auto: the new BIOS decides what it means at boot, which may differ from the old one.{/if}
        </p>
      </div>
    {/if}
  </div>

  <footer class="bar">
    <div class="summary">
      {#if m}
        {#if blocked > 0}
          <Status tone="blocked" text="{blocked} {blocked === 1 ? 'setting needs' : 'settings need'} a decision" />
        {:else}
          <Status tone="ok" text="Plan complete" />
        {/if}
        <span class="quiet">{changing} {changing === 1 ? "value changes" : "values change"} from the target baseline</span>
        {#if saved}<span class="quiet">Plan saved to <span class="mono">{fileName(saved)}</span></span>{/if}
        {#if buildError}<Status tone="blocked" text={buildError} />{/if}
      {/if}
    </div>
    <button class="btn" onclick={savePlan} disabled={!plan}>Save plan</button>
    <button class="btn primary" onclick={build} disabled={!m?.can_build || building || planning}>
      {building ? "Building…" : "Build and verify"}
    </button>
  </footer>
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
  .strip {
    display: flex;
    align-items: center;
    gap: var(--s3);
    height: 48px;
    padding: 0 var(--s4);
    border-bottom: 1px solid var(--rule);
    background: var(--surface);
  }
  .flow {
    color: var(--accent);
    display: inline-flex;
  }
  .strong {
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .body {
    height: calc(100% - 48px - 56px);
    display: flex;
    flex-direction: column;
  }
  .groups {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--s2);
    padding: var(--s3) var(--s4);
    border-bottom: 1px solid var(--rule);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 var(--s3) 0 var(--s2);
    border: 1px solid var(--rule-strong);
    border-radius: 15px;
    cursor: pointer;
  }
  .chip.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .working {
    margin-left: var(--s2);
  }
  .tablewrap {
    flex: 1;
    overflow: auto;
    padding: 0 var(--s4) var(--s5);
  }
  .ledger {
    width: 100%;
    max-width: 1440px;
    border-collapse: collapse;
    table-layout: fixed;
  }
  .ledger th {
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
  }
  .ledger td {
    overflow: hidden;
    text-overflow: ellipsis;
    height: var(--row);
    padding: 0 var(--s3);
    border-bottom: 1px solid var(--rule);
  }
  .ledger .sel {
    width: 36px;
    padding-right: 0;
  }
  .ledger tr.pagehead th,
  .ledger tr.section th {
    position: static;
    background: transparent;
    text-align: left;
  }
  .ledger tr.pagehead th {
    height: 40px;
    padding-top: var(--s3);
    color: var(--ink);
    font-size: var(--fs-body);
  }
  .ledger tr.section th {
    height: 30px;
    padding-top: var(--s2);
    border-bottom-color: var(--rule);
  }
  .ledger td.name {
    white-space: normal;
    line-height: 1.3;
    padding-top: 6px;
    padding-bottom: 6px;
  }
  .moved {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: var(--fs-small);
    color: var(--ink-faint);
  }
  .ledger tr.off td:not(.sel) {
    color: var(--ink-faint);
  }
  /* The provenance trace: where each proposed value comes from. Hot = changes the target value, cold = already the
     same, gold = a value you chose, a broken red trace = blocked. */
  .ledger td.proposed {
    position: relative;
    font-weight: 600;
    padding-left: 34px;
  }
  .proposed::before,
  .proposed::after {
    content: "";
    position: absolute;
    top: 50%;
  }
  .proposed::before {
    left: 4px;
    width: 16px;
    height: 3px;
    margin-top: -1.5px;
    border-radius: 2px;
    background: transparent;
  }
  .proposed::after {
    left: 17px;
    width: 9px;
    height: 9px;
    margin-top: -4.5px;
    border-radius: 50%;
    background: transparent;
  }
  .proposed.source::before,
  .proposed.source::after {
    background: var(--c3);
  }
  .proposed.target::before,
  .proposed.target::after {
    background: var(--c1);
  }
  .proposed.replaced::before,
  .proposed.replaced::after {
    background: var(--c4);
  }
  .proposed.blocked::before {
    background: repeating-linear-gradient(90deg, var(--red) 0 4px, transparent 4px 7px);
  }
  .proposed.blocked::after {
    box-sizing: border-box;
    border: 2px solid var(--red);
  }
  .proposed.target,
  .proposed.none,
  .proposed.blocked {
    font-weight: 400;
  }
  tr.blockedrow td {
    border-bottom-color: transparent;
  }
  tr.decision td {
    height: auto;
    padding-top: 0;
    padding-bottom: var(--s3);
  }
  .decide {
    display: flex;
    gap: var(--s5);
    padding: var(--s3) var(--s4);
    border-left: 3px solid var(--red);
    background: var(--red-soft);
    border-radius: 0 var(--radius) var(--radius) 0;
  }
  tr.decision.resolved .decide {
    border-left-color: var(--amber);
    background: var(--amber-soft);
  }
  .why {
    flex: 1 1 50%;
    max-width: 62ch;
  }
  .why .quiet {
    display: block;
    margin-top: 2px;
    font-size: var(--fs-small);
  }
  .options {
    flex: 1 1 50%;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .opt {
    display: inline-flex;
    align-items: center;
    gap: var(--s2);
  }
  .setter {
    padding-left: 24px;
    max-width: 320px;
  }
  .sub {
    font-size: var(--fs-lead);
    margin-top: var(--s5);
  }
  .note {
    margin: var(--s2) 0;
    max-width: 90ch;
  }
  .compact td {
    height: 38px;
  }
  .legend {
    display: flex;
    flex-wrap: wrap;
    gap: var(--s2) var(--s5);
    margin-top: var(--s4);
    font-size: var(--fs-small);
    color: var(--ink-quiet);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .key {
    width: 18px;
    height: 3px;
    border-radius: 2px;
    display: inline-block;
  }
  .key.source {
    background: var(--c3);
  }
  .key.target {
    background: var(--c1);
  }
  .key.replaced {
    background: var(--c4);
  }
  .key.blocked {
    background: repeating-linear-gradient(90deg, var(--red) 0 4px, transparent 4px 7px);
  }
  .problem {
    margin: var(--s4);
    padding: var(--s3) var(--s4);
    border-left: 3px solid var(--red);
    background: var(--red-soft);
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
    gap: var(--s4);
    min-width: 0;
    overflow: hidden;
  }
  @media (max-width: 1100px) {
    .pairs {
      grid-template-columns: 1fr;
    }
  }
</style>
