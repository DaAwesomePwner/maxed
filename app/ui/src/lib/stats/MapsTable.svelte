<script lang="ts">
  import type { StatsMapRow } from '../bindings/StatsMapRow';
  import type { StatsMapSort } from '../navigation/state';
  import { sortMaps } from './filters';
  import { NO_VALUE, durationText, fameText, rateText } from './presentation';
  let { rows, sort = null, onSort, unrecorded = null }: {
    rows: StatsMapRow[]; sort?: { key: StatsMapSort; descending: boolean } | null;
    onSort?: (key: StatsMapSort) => void; unrecorded?: number | null;
  } = $props();
  const columns: Array<[StatsMapSort, string]> = [['map', 'Map'], ['visits', 'Visits'], ['time', 'Time'], ['fame', 'Fame'],
    ['rate', 'Fame/hour'], ['clears', 'Clears'], ['deaths', 'Deaths']];
  let shown = $derived(sort ? sortMaps(rows, sort.key, sort.descending) : rows);
  const ariaSort = (key: StatsMapSort) => sort?.key !== key ? undefined : sort.descending ? 'descending' : 'ascending';
</script>

<table>
  <thead><tr>{#each columns as [key, label]}
    <th scope="col" aria-sort={ariaSort(key)}>{#if onSort}<button type="button" onclick={() => onSort(key)}>{label}</button>{:else}{label}{/if}</th>
  {/each}</tr></thead>
  <tbody>
    {#each shown as row (`${row.group}:${row.map}`)}
      <tr>
        <th scope="row">{row.map}</th>
        <td>{row.visits}</td><td>{durationText(row.time_micros)}</td>
        <td>{fameText(row.fame, row.missing_baseline)}</td><td>{rateText(row.fame_per_hour)}</td>
        <td>{row.clears ?? NO_VALUE}</td><td>{row.deaths ?? NO_VALUE}</td>
      </tr>
    {/each}
    {#if unrecorded}
      <tr class="unrecorded"><th scope="row">Unrecorded</th><td>{NO_VALUE}</td><td>{NO_VALUE}</td>
        <td>{fameText(unrecorded, false)}</td><td>{NO_VALUE}</td><td>{NO_VALUE}</td><td>{NO_VALUE}</td></tr>
    {/if}
  </tbody>
</table>

<style>
  table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  th, td { padding: var(--space-1) var(--space-2); text-align: right; border-bottom: 1px solid var(--border); }
  th[scope="row"], th:first-child { text-align: left; }
  th button { font: inherit; color: inherit; background: none; border: 0; padding: 0; cursor: pointer; }
  .unrecorded { color: var(--text-secondary); }
</style>
