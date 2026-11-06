<script lang="ts">
  import type { PartyView } from '../bindings/PartyView';
  import type { RequirementPreset } from '../bindings/RequirementPreset';
  import type { RunView } from '../bindings/RunView';
  import { presetSummary } from './party-screen';
  import { routeHref } from '../navigation/routes';
  let { view, saved, presets, recentRun, onPick, busy = false }: {
    view: PartyView; saved: boolean; presets: RequirementPreset[]; recentRun: RunView | null;
    onPick: (id: string | null) => void; busy?: boolean;
  } = $props();
  let active = $derived(presets.find(preset => preset.id === view.preset?.id));
  let picked = $derived(view.preset?.source === 'picked');
  let presetLabel = $derived(view.preset ? `Preset: ${view.preset.name}${active ? ` · ${presetSummary(active)}` : ''}` : 'Preset: None');
  let scopeLink: HTMLAnchorElement;
</script>
<div class="context">
  <a bind:this={scopeLink} href="#/party" hidden tabindex="-1">Open scope</a>
  <label class="sr-only" for="party-scope">Party scope</label>
  <select id="party-scope" class="ctl" value={saved ? view.run_id ?? '' : 'live'} onchange={event => {
    const value = event.currentTarget.value;
    scopeLink.href = value === 'live' ? '#/party' : routeHref({ kind: 'party-run', runId: value });
    scopeLink.click();
  }}>
    <option value="live">{saved ? 'Current map' : view.map ?? 'Current map'}</option>
    {#if saved && view.run_id}<option value={view.run_id}>{view.map ?? 'Saved run'}</option>{/if}
    {#if recentRun && (!saved || recentRun.id !== view.run_id)}<option value={recentRun.id}>Last run: {recentRun.dungeon}</option>{/if}
  </select>
  {#if !saved && view.live}<span class="live">Live</span>{/if}
  <span class="caption">{saved ? 'Players in this run' : 'Players in view'}</span>
  <div class="preset">
    {#if !saved}
      <label class="sr-only" for="party-preset">Preset</label>
      <select id="party-preset" aria-describedby={view.preset ? 'party-preset-source' : undefined} class="ctl" disabled={busy} value={picked ? view.preset?.id ?? '' : ''}
        onchange={event => onPick(event.currentTarget.value || null)}>
        <option value="">{picked ? 'Automatic' : presetLabel}</option>
        {#if view.preset && !active}<option value={view.preset.id}>{picked ? presetLabel : view.preset.name}</option>{/if}
        {#each presets as preset}<option value={preset.id}>{picked ? 'Preset: ' : ''}{preset.name} · {presetSummary(preset)}</option>{/each}
      </select>
    {:else}
      <span>{presetLabel}</span>
    {/if}
    {#if view.preset}<span class="caption" id="party-preset-source">{view.preset.source}</span>{/if}
  </div>
</div>
<style>
  .context, .preset { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); }
  .context { margin-bottom: var(--space-3); } .preset { margin-left: auto; }
  .caption { color: var(--text-secondary); font-size: 12px; }
  .live { color: var(--success); border: 1px solid var(--success); border-radius: var(--radius-control); padding: 1px 5px; font-size: 11px; }
</style>
