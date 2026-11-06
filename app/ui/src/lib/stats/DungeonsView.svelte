<script lang="ts">
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { StatsPosition } from '../navigation/state';
  import EmptyState from '../components/EmptyState.svelte';
  import StatsQuery from './StatsQuery.svelte';
  import StatsFilterBar from './StatsFilterBar.svelte';
  import ExportButton from './ExportButton.svelte';
  import { statsFilter } from './filters';
  import { clearText, dayText, durationText, percentText } from './presentation';
  let { client, enabled = true, position, onChange, streamId = null, scopeKey = '', required = null, store = null,
    entryKey = '', onRowsReady }: {
    client?: Client; enabled?: boolean; position: StatsPosition; onChange: (patch: Partial<StatsPosition>) => void;
    streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
  let filter = $derived(statsFilter(position, new Date()));
</script>

<StatsQuery {client} {enabled} query={{ kind: 'stats-dungeons', filter }} subject="dungeon records" {streamId} {scopeKey} {required} {store} {entryKey} {onRowsReady}>
  {#snippet children(history)}
    {@const dungeons = history.value?.kind === 'stats-dungeons' ? history.value.dungeons : null}
    {#if dungeons}
      <header class="bar">
        <StatsFilterBar {client} range={position.range} characterId={position.characterId} characters={dungeons.characters} {onChange} />
        <ExportButton {client} request={{ kind: { type: 'dungeons' }, filter, include_empty: false }} />
      </header>
      {#if dungeons.rows.length}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex (keyboard scrolling for wide tables) -->
        <div class="stats-table-scroll" role="region" aria-label="Scrollable statistics" tabindex="0"><table>
          <thead><tr><th scope="col">Dungeon</th><th scope="col">Entries</th><th scope="col">Time</th><th scope="col">Clears</th>
            <th scope="col">Clear rate</th><th scope="col">Deaths</th><th scope="col">Mean clear</th><th scope="col">Best clear</th>
            <th scope="col">Last played</th></tr></thead>
          <tbody>{#each dungeons.rows as row (row.dungeon)}
            <tr><th scope="row">{row.dungeon}</th><td>{row.runs}</td><td>{durationText(row.total_micros)}</td><td>{row.clears}</td>
              <td>{percentText(row.clears, row.runs)}</td><td>{row.deaths}</td><td>{clearText(row.mean_clear_micros)}</td>
              <td>{clearText(row.best_clear_micros)}</td><td>{dayText(row.last_started)}</td></tr>
          {/each}</tbody>
        </table></div>
        {#if dungeons.truncated}<p class="note">Showing the 200 most-entered dungeons.</p>{/if}
      {:else}<EmptyState heading="No dungeon runs" description="" />{/if}
    {/if}
  {/snippet}
</StatsQuery>

<style>
  .bar { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: end; gap: var(--space-3); margin-bottom: var(--space-3); }
  table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  th, td { padding: var(--space-1) var(--space-2); text-align: right; border-bottom: 1px solid var(--border); }
  th[scope="row"], th:first-child { text-align: left; }
  .note { color: var(--text-secondary); }
</style>
