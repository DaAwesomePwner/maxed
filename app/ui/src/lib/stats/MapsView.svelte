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
      <header class="bar">
        <StatsFilterBar {client} range={position.range} characterId={position.characterId} characters={maps.characters} {onChange} />
        <ExportButton {client} request={{ kind: { type: 'maps' }, filter, include_empty: false }} />
      </header>
      {#if maps.rows.length}
        <MapsTable rows={maps.rows} sort={position.mapSort} onSort={sortBy} />
        {#if maps.truncated}<p class="note">Showing the first 200 maps.</p>{/if}
      {:else}<EmptyState heading="No recorded maps" description="" />{/if}
      <p class="note">{maps.tracking_since
        ? `Realm and hub visits are recorded from ${dayText(maps.tracking_since)}; earlier history counts dungeons only.`
        : 'Realm and hub visits are recorded from your next session; earlier history counts dungeons only.'}</p>
    {/if}
  {/snippet}
</StatsQuery>

<style>
  .bar { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: end; gap: var(--space-3); margin-bottom: var(--space-3); }
  .note { margin: var(--space-2) 0 0; color: var(--text-secondary); }
</style>
