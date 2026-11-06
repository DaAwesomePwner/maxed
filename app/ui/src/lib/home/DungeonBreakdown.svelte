<script lang="ts">
  import DebugNote from '../debug/DebugNote.svelte';
  import type { DungeonStatsRow } from '../bindings/DungeonStatsRow';
  import { clearDuration, DUNGEON_PAGE_SIZE, DUNGEON_SERVER_LIMIT } from './nexus';
  import { statText } from './evidence';
  let { rows = null, allTimeHref = null }: { rows?: DungeonStatsRow[] | null; allTimeHref?: string | null } = $props();
  const id = $props.id();
  let page = $state(0);
  let bounded = $derived(rows?.slice(0, DUNGEON_SERVER_LIMIT) ?? []);
  let pages = $derived(Math.max(1, Math.ceil(bounded.length / DUNGEON_PAGE_SIZE)));
  let current = $derived(Math.min(page, pages - 1));
  let visible = $derived(bounded.slice(current * DUNGEON_PAGE_SIZE, (current + 1) * DUNGEON_PAGE_SIZE));
</script>

<section class="dungeon-breakdown" aria-labelledby={`${id}-title`}>
  <header><h2 id={`${id}-title`}>By dungeon</h2>
    {#if allTimeHref}<a class="history-link" href={allTimeHref}>All time ›</a>{/if}
    {#if pages > 1}<nav aria-label="Dungeon Pages">
      <button type="button" aria-label="Previous Dungeons" aria-disabled={current === 0} onclick={() => { if (current > 0) page = current - 1; }}>&lsaquo;</button>
      <span>{current + 1}/{pages}</span>
      <button type="button" aria-label="Next Dungeons" aria-disabled={current === pages - 1} onclick={() => { if (current < pages - 1) page = current + 1; }}>&rsaquo;</button>
    </nav>{/if}
  </header>
  {#if rows == null}
    <p>&mdash;</p>
  {:else if !bounded.length}
    <p>No Dungeons</p>
  {:else}
    <table>
      <thead><tr><th scope="col">Dungeon</th><th scope="col">Runs</th><th scope="col">Avg<span class="sr-only"> Clear</span></th><th scope="col">Best<span class="sr-only"> Clear</span></th></tr></thead>
      <tbody>{#each visible as row}
        {@const bestTime = clearDuration(row.best_clear_micros, row.observed_clears)}
        <tr>
          <th scope="row"><span class="dungeon-cell"><span class="dungeon-name" title={row.dungeon}>{row.dungeon}</span></span><DebugNote inline>Timed Clears: {statText(row.observed_clears) ?? '—'}</DebugNote></th>
          <td>{statText(row.runs) ?? '—'}</td>
          <td>{clearDuration(row.mean_clear_micros, row.observed_clears)}</td>
          <td class="best-time" class:known={bestTime !== '—'}>{#if row.all_time_best_clear_micros != null && row.best_clear_micros === row.all_time_best_clear_micros && row.observed_clears > 0 && row.best_clear_micros > 0 && bestTime !== '—'}<span class="pb" title="All-time best clear matched in this scope">PB</span>{/if}{bestTime}</td>
        </tr>
      {/each}</tbody>
    </table>
  {/if}
  <!-- Clear times use observed completions; the server returns at most 20 dungeons. -->
  <DebugNote text="Observed-clear times only; missing times unavailable. Server cap: 20 dungeons, ordered by runs." />
  {#if pages > 1}<p class="sr-only" role="status">Dungeons {current * DUNGEON_PAGE_SIZE + 1}&ndash;{Math.min((current + 1) * DUNGEON_PAGE_SIZE, bounded.length)} / {bounded.length}</p>{/if}
</section>

<style>
  .dungeon-breakdown { min-width: 0; padding: var(--space-2); border: 1px solid var(--border); border-radius: var(--radius-card); background: var(--surface-panel); box-shadow: var(--elevation-card); }
  .dungeon-breakdown header, .dungeon-breakdown nav { display: flex; align-items: center; gap: var(--space-1); }
  .dungeon-breakdown header { justify-content: space-between; min-height: var(--target-min); }
  .dungeon-breakdown h2 { margin: 0; font-size: var(--text-body); line-height: var(--leading-body); }
  .dungeon-breakdown table { table-layout: fixed; width: 100%; border-collapse: collapse; font-size: var(--text-label); line-height: var(--leading-label); }
  .dungeon-breakdown th, .dungeon-breakdown td { padding: var(--space-1); text-align: right; vertical-align: middle; font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .dungeon-breakdown thead th { white-space: nowrap; }
  .dungeon-breakdown .dungeon-cell { display: flex; align-items: center; gap: var(--space-1); }
  .dungeon-breakdown .dungeon-name { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .dungeon-breakdown th:first-child { text-align: left; }
  .dungeon-breakdown thead th { color: var(--text-secondary); font-weight: var(--weight-normal); }
  .dungeon-breakdown tbody tr { border-top: 1px solid var(--border); }
  .dungeon-breakdown tbody th { font-weight: var(--weight-normal); }
  .dungeon-breakdown p { margin: var(--space-1) 0 0; font-size: var(--text-label); line-height: var(--leading-label); color: var(--text-secondary); }
  .dungeon-breakdown button { min-width: var(--target-min); min-height: var(--target-min); color: var(--text-primary); background: var(--surface-raised); border: 1px solid var(--border); border-radius: var(--radius-control); font: inherit; }
  .dungeon-breakdown button[aria-disabled="true"] { color: var(--text-muted); }
  .dungeon-breakdown nav { font-size: var(--text-label); }

  .dungeon-breakdown h2 { font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--text-secondary); }
  .dungeon-breakdown th:nth-child(2) { width: 40px; }
  .dungeon-breakdown th:nth-child(3) { width: 52px; }
  .dungeon-breakdown th:nth-child(4) { width: 74px; }
  .dungeon-breakdown th, .dungeon-breakdown td { overflow-wrap: normal; white-space: nowrap; }
  /* Debug-only caveats sit under the name and may wrap; the data cells stay on one line. */
  .dungeon-breakdown tbody th :global([data-debug-note]) { display: block; white-space: normal; overflow-wrap: anywhere; }
  .dungeon-breakdown .pb { display: inline-block; color: #2a1f05; background: var(--gold); border-radius: 3px; padding: 0 3px; margin-right: 5px; font-size: 9px; font-weight: 800; }
  .dungeon-breakdown .best-time { color: var(--text-muted); }
  .dungeon-breakdown .best-time.known { color: var(--success); }
</style>
