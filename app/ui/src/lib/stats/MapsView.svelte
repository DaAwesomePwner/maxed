<script lang="ts">
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { StatsMapSort, StatsPosition } from '../navigation/state';
  import EmptyState from '../components/EmptyState.svelte';
  import StatsQuery from './StatsQuery.svelte';
  import StatsFilterBar from './StatsFilterBar.svelte';
  import ExportButton from './ExportButton.svelte';
  import MapsTable from './MapsTable.svelte';
  import { statsFilter } from './filters';
  import { dayText } from './presentation';
  let { client, enabled = true, position, onChange, streamId = null, scopeKey = '', required = null, store = null,
    entryKey = '', onRowsReady }: {
    client?: Client; enabled?: boolean; position: StatsPosition; onChange: (patch: Partial<StatsPosition>) => void;
    streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
  let filter = $derived(statsFilter(position, new Date()));
  function sortBy(key: StatsMapSort) {
    const same = position.mapSort.key === key;
    onChange({ mapSort: { key, descending: same ? !position.mapSort.descending : key !== 'map' } });
  }
</script>

<StatsQuery {client} {enabled} query={{ kind: 'stats-maps', filter }} subject="map stats" {streamId} {scopeKey} {required} {store} {entryKey} {onRowsReady}>
  {#snippet children(history)}
    {@const maps = history.value?.kind === 'stats-maps' ? history.value.maps : null}
    {#if maps}
      {@const dungeons = maps.rows.filter(row => row.group === 'dungeon' && row.visits >= position.minMapRuns)}
      {@const otherMaps = maps.rows.filter(row => row.group !== 'dungeon')}
      <header class="bar">
        <StatsFilterBar {client} range={position.range} characterId={position.characterId} characters={maps.characters} {onChange} />
        <label class="min-runs">Minimum runs <input class="ctl" type="number" min="1" max="10000" step="1" value={position.minMapRuns}
          onchange={event => { const count = event.currentTarget.valueAsNumber; if (Number.isInteger(count) && count >= 1 && count <= 10000) onChange({ minMapRuns: count }); }} /></label>
        <ExportButton {client} request={{ kind: { type: 'maps' }, filter, include_empty: false }} />
      </header>
      {#if maps.rows.length}
        <h3>Dungeons</h3>
        {#if dungeons.length}<MapsTable rows={dungeons} label="Dungeon rankings" sort={position.mapSort} onSort={sortBy} />
        {:else}<p class="note">No dungeons have at least {position.minMapRuns} recorded runs. Lower Minimum runs to see more.</p>{/if}
        {#if otherMaps.length}<section aria-label="Other maps"><h3>Realm, hubs and other maps</h3><MapsTable rows={otherMaps} label="Other map statistics" /></section>{/if}
        <p class="note">Dungeon rankings require at least {position.minMapRuns} recorded runs. Fame/hour also requires five minutes of observed time. CSV exports include all maps.</p>
        {#if maps.truncated}<p class="note">Showing the first 200 maps.</p>{/if}
      {:else}<EmptyState heading="No recorded maps" description="" />{/if}
      <p class="note">{maps.tracking_since
        ? `Realm and hub visits are recorded from ${dayText(maps.tracking_since)}; earlier history counts dungeons only.`
        : 'Realm and hub visits are recorded from your next session; earlier history counts dungeons only.'}</p>
    {/if}
  {/snippet}
</StatsQuery>

<style>
  .min-runs { display: grid; gap: var(--space-1); }
  .min-runs input { width: 72px; }
  h3 { margin: var(--space-3) 0 var(--space-2); }
  .bar { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: end; gap: var(--space-3); margin-bottom: var(--space-3); }
  .note { margin: var(--space-2) 0 0; color: var(--text-secondary); }
</style>
