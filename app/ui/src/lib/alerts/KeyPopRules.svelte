<script lang="ts">
  import type { Client } from '../bridge/client';
  import type { AlertPreferences } from '../bindings/AlertPreferences';
  import { dungeonCatalog, watchDungeonCatalog } from './dungeon-catalog';
  import { keyPopAllows } from './preferences';
  let { client, preferences, disabled = false, onEdit }: {
    client?: Client; preferences: AlertPreferences; disabled?: boolean;
    onEdit: (patch: Partial<AlertPreferences>) => void;
  } = $props();
  const id = $props.id();
  $effect(() => { if (client) return watchDungeonCatalog(client); });
  let selected = $derived(preferences.key_pop_dungeons !== null);
  let preset = $derived(preferences.key_pop_min_difficulty != null ? 'custom' : preferences.key_pop_exalt_only ? 'exalt' : 'everything');
  let allowed = $derived($dungeonCatalog?.filter(row => keyPopAllows(preferences, row.name, row)));
  let unknown = $derived(allowed?.filter(row => row.difficulty == null).length ?? 0);
  function threshold(value: string) {
    const number = Number(value);
    if (Number.isFinite(number)) onEdit({ key_pop_min_difficulty: Math.round(Math.max(0, Math.min(10, number)) * 2) / 2 });
  }
</script>
<section aria-label="Key-pop rules">
  <h4>Dungeon rules</h4>
  {#if selected}<p id={`${id}-selected`}>Only selected: the selected list overrides these rules.</p>{/if}
  <fieldset aria-describedby={selected ? `${id}-selected` : undefined} disabled={disabled || selected}>
    <legend>Notify for</legend>
    <label>Preset
      <select value={preset} onchange={event => {
        const value = event.currentTarget.value;
        onEdit(value === 'custom' ? { key_pop_min_difficulty: preferences.key_pop_min_difficulty ?? 7 }
          : { key_pop_exalt_only: value === 'exalt', key_pop_min_difficulty: null });
      }}><option value="everything">Everything</option><option value="exalt">Exalt dungeons</option><option value="custom">Custom</option></select>
    </label>
    <label>Minimum difficulty
      <select aria-label="Minimum difficulty" value={preferences.key_pop_min_difficulty == null ? 'off' : 'on'} onchange={event => onEdit({ key_pop_min_difficulty: event.currentTarget.value === 'off' ? null : 7 })}><option value="off">Off</option><option value="on">At least</option></select>
    </label>
    <label class="slider">Minimum difficulty rating
      <input aria-label="Minimum difficulty rating" aria-valuetext={preferences.key_pop_min_difficulty == null ? 'Off' : `${preferences.key_pop_min_difficulty}+ / 10`} type="range" min="0" max="10" step="0.5" disabled={preferences.key_pop_min_difficulty == null} value={preferences.key_pop_min_difficulty ?? 0} oninput={event => threshold(event.currentTarget.value)} />
      <output>{preferences.key_pop_min_difficulty == null ? 'Off' : `${preferences.key_pop_min_difficulty}+ / 10`}</output>
    </label>
    <label><input type="checkbox" checked={preferences.key_pop_exalt_only} onchange={event => onEdit({ key_pop_exalt_only: event.currentTarget.checked })} /> Exalt dungeons only</label>
    <p>Fungal Cavern counts as the gateway to Crystal Cavern.</p>
  </fieldset>
  <p aria-live="polite">{#if $dungeonCatalog}{allowed?.length} of {$dungeonCatalog.length} dungeons will notify.
    {#if preferences.key_pop_min_difficulty != null}{unknown} with unknown difficulty still notify under a threshold.{/if}
    {:else}Dungeon preview unavailable until the catalog loads.{/if}</p>
</section>
<style>
  section { background: var(--surface-raised); border: 1px solid var(--border); border-radius: var(--radius-card); padding: var(--space-2); margin-block: var(--space-2); }
  h4 { margin: 0; } fieldset { border: 0; padding: 0; min-width: 0; } legend, p { color: var(--text-secondary); font-size: var(--text-label); }
  label { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-2); min-height: var(--target-min); }
  select { color: var(--text-primary); background: var(--surface-panel); border: 1px solid var(--border-strong); border-radius: var(--radius-control); min-height: var(--target-min); font: inherit; max-width: 100%; }
  input { accent-color: var(--violet); } .slider input { flex: 1; min-width: 5rem; } output { min-width: 4rem; }
</style>
