<script lang="ts">
  import { tick, untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { RequirementPreset } from '../bindings/RequirementPreset';
  import type { PartySettings } from '../bindings/PartySettings';
  import type { DungeonDef } from '../bindings/DungeonDef';
  import { equipSlots, presetCopyText } from './party-screen';
  let { client, preset, settings, busy, onSave, onDelete, onCancel, dirty = $bindable(false) }: {
    client: Client; preset: RequirementPreset; settings: PartySettings; busy: boolean; dirty?: boolean;
    onSave: (preset: RequirementPreset, makeDefault: boolean) => void; onDelete: () => void; onCancel: () => void;
  } = $props();
  // The keyed editor owns a draft. Neither field edits nor dungeon moves mutate saved settings.
  let draft = $state<RequirementPreset>(untrack(() => ({ ...preset, min_tier: { ...preset.min_tier }, auto_apply: [...preset.auto_apply] })));
  let makeDefault = $state(untrack(() => settings.default_preset_id === preset.id));
  let query = $state(''), copied = $state(''), catalogMessage = $state('');
  let catalog = $state.raw<DungeonDef[]>([]);
  let saved = $derived(settings.presets.some(row => row.id === draft.id));
  function comparable(value: RequirementPreset) {
    return JSON.stringify({ ...value, min_maxed: value.min_maxed ?? null,
      min_tier: equipSlots.map(slot => value.min_tier[slot] ?? null), auto_apply: [...value.auto_apply].sort() });
  }
  $effect(() => { dirty = comparable(draft) !== comparable(preset) || makeDefault !== (settings.default_preset_id === preset.id); });
  let visible = $derived.by(() => {
    const term = query.trim().toLocaleLowerCase();
    return [...new Set([...catalog.map(row => row.name), ...draft.auto_apply])].sort()
      .filter(name => [name, ...(catalog.find(row => row.name === name)?.aliases ?? [])]
        .some(value => value.toLocaleLowerCase().includes(term)));
  });
  $effect(() => {
    const owner = client;
    let disposed = false, revision = 0;
    async function load() {
      const request = ++revision;
      try {
        // Same catalog names and aliases as Settings > Alerts > Key Popped dungeons.
        const rows = await owner.listCatalogDungeonOptions();
        if (!disposed && request === revision) { catalog = rows; catalogMessage = ''; }
      } catch { if (!disposed && request === revision) catalogMessage = 'Dungeon catalog unavailable. Check assets in Settings.'; }
    }
    void load();
    const release = owner.onVisualsChanged(() => { void load(); });
    return () => { disposed = true; release(); };
  });
  function toggle(name: string, selected: boolean) {
    draft.auto_apply = selected ? [...new Set([...draft.auto_apply, name])] : draft.auto_apply.filter(value => value !== name);
  }
  function save() {
    onSave({ ...draft, min_tier: { ...draft.min_tier }, auto_apply: [...draft.auto_apply] }, makeDefault);
  }
  async function copy() {
    if (!navigator.clipboard?.writeText) return;
    try { await navigator.clipboard.writeText(presetCopyText(draft)); copied = ''; await tick(); copied = 'Copied'; }
    catch { /* Clipboard may be denied by the host. */ }
  }
</script>
<section class="editor" aria-label="Preset editor">
  <h3>{saved ? 'Edit preset' : 'New preset'}</h3>
  <form onsubmit={event => { event.preventDefault(); save(); }}>
    <fieldset disabled={busy}>
      <label class="field">Name<input type="text" maxlength="40" bind:value={draft.name} /></label>
      <label class="field">Maxed stats<select value={draft.min_maxed ?? ''} onchange={event => { draft.min_maxed = event.currentTarget.value === '' ? null : Number(event.currentTarget.value); }}>
        <option value="">Any</option>{#each [1, 2, 3, 4, 5, 6, 7, 8] as value}<option value={value}>At least {value}/8</option>{/each}
      </select></label>
      <p class="hint">Minimum tier per slot.</p>
      {#each equipSlots as slot}<label class="field">{slot[0].toUpperCase() + slot.slice(1)}<select value={draft.min_tier[slot] ?? ''}
        onchange={event => { draft.min_tier[slot] = event.currentTarget.value === '' ? null : Number(event.currentTarget.value); }}>
        <option value="">Any</option>{#each Array.from({ length: 20 }, (_, i) => i + 1) as tier}<option value={tier}>T{tier}</option>{/each}
        {#if draft.min_tier[slot] != null && (draft.min_tier[slot]! < 1 || draft.min_tier[slot]! > 20)}
          <option value={draft.min_tier[slot]}>T{draft.min_tier[slot]}</option>
        {/if}
      </select></label>{/each}
      <label class="check"><input type="checkbox" bind:checked={draft.ut_st_meets} /> UT / ST counts as meeting a tier</label>
      <label class="check"><input type="checkbox" bind:checked={makeDefault} /> Default</label>
      <p class="hint">The default is used when no dungeon matches and no preset is picked in Live.</p>
      <fieldset class="dungeons"><legend>Auto-apply in</legend>
        <div class="chips" aria-label="Selected dungeons">{#each draft.auto_apply as name (name)}
          {@const previous = settings.presets.find(row => row.id !== draft.id && row.auto_apply.includes(name))}
          <div><button class="ctl" type="button" aria-label={`Remove ${name}`} onclick={() => toggle(name, false)}>{name} ×</button>
            {#if previous}<small>Moved from {previous.name}</small>{/if}</div>
        {/each}</div>
        <label class="field">Search dungeons<input type="search" bind:value={query} /></label>
        <div class="results">{#each visible as name (name)}<label class="check"><input type="checkbox" checked={draft.auto_apply.includes(name)} onchange={event => toggle(name, event.currentTarget.checked)} />{name}</label>{/each}
          {#if !visible.length}<p class="hint">No matching dungeons.</p>{/if}
        </div>
        {#if catalogMessage}<p role="status">{catalogMessage}</p>{/if}
      </fieldset>
      <div class="actions"><button class="ctl" type="submit">{busy ? 'Saving…' : 'Save'}</button><button class="ctl" type="button" onclick={copy}>Copy as text</button>
        {#if saved}<button class="ctl" type="button" onclick={onDelete}>Delete</button>{/if}<button class="ctl" type="button" onclick={onCancel}>Cancel</button></div>
    </fieldset>
  </form>
  <p role="status">{copied}</p>
</section>
<style>
  h3 { font-size: 14px; margin-top: 0; } fieldset { border: 0; margin: 0; padding: 0; min-width: 0; } .field { display: grid; grid-template-columns: minmax(100px, 1fr) minmax(0, 1fr); align-items: center; gap: var(--space-2); margin: var(--space-2) 0; font-size: 12px; }
  input:not([type=checkbox]), select { min-width: 0; width: 100%; box-sizing: border-box; min-height: var(--target-min); font: inherit; color: var(--text-primary); background: var(--surface-raised); border: 1px solid var(--border-strong); border-radius: var(--radius-control); padding: 4px 6px; }
  input { accent-color: var(--violet); } .check { display: flex; gap: 6px; align-items: center; min-height: var(--target-min); font-size: 12px; } .hint, small { font-size: 12px; color: var(--text-secondary); } small { display: block; padding: 4px; }
  .dungeons { border: 1px solid var(--border); border-radius: var(--radius-control); padding: var(--space-2); margin: var(--space-3) 0; } legend { font-size: 12px; } .results { max-height: 180px; overflow-y: auto; }
  .chips, .actions { display: flex; flex-wrap: wrap; gap: var(--space-2); } .chips button { white-space: normal; text-align: left; } [role=status]:empty { display: none; }
</style>
