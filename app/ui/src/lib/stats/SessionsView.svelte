<script lang="ts">
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { StatsSessionCursor } from '../bindings/StatsSessionCursor';
  import type { StatsSessionFilter } from '../bindings/StatsSessionFilter';
  import type { StatsPosition } from '../navigation/state';
  import EmptyState from '../components/EmptyState.svelte';
  import StatsQuery from './StatsQuery.svelte';
  import StatsFilterBar from './StatsFilterBar.svelte';
  import ExportButton from './ExportButton.svelte';
  import { statsFilter } from './filters';
  import { durationText, fameText, rateText, whenText } from './presentation';
  let { client, enabled = true, position, onChange, streamId = null, scopeKey = '', required = null, store = null,
    entryKey = '', onRowsReady }: {
    client?: Client; enabled?: boolean; position: StatsPosition; onChange: (patch: Partial<StatsPosition>) => void;
    streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
  let scope = $derived(statsFilter(position, new Date()));
  let filter = $derived<StatsSessionFilter>({ ...scope, include_empty: position.includeEmpty, cursor: position.sessions.cursor, limit: 50 });
  const firstPage = { cursor: null, previous: [] };
  /** Any filter change returns to the first page. */
  const refilter = (patch: Partial<StatsPosition>) => onChange({ ...patch, sessions: firstPage });
  const goTo = (cursor: StatsSessionCursor | null, previous: Array<StatsSessionCursor | null>) => onChange({ sessions: { cursor, previous } });
</script>

<StatsQuery {client} {enabled} query={{ kind: 'stats-sessions', filter }} subject="sessions" {streamId} {scopeKey} {required} {store} {entryKey} {onRowsReady}>
  {#snippet children(history)}
    {@const page = history.value?.kind === 'stats-sessions' ? history.value.page : null}
    {#if page}
      <header class="bar">
        <StatsFilterBar {client} range={position.range} characterId={position.characterId} characters={page.characters} onChange={refilter} />
        <label class="toggle"><input type="checkbox" checked={position.includeEmpty}
          onchange={e => refilter({ includeEmpty: e.currentTarget.checked })} /> Show empty sessions</label>
        <ExportButton {client} request={{ kind: { type: 'sessions' }, filter: scope, include_empty: position.includeEmpty }} />
      </header>
      {#if page.rows.length}
        <table>
          <thead><tr><th scope="col">Session</th><th scope="col">Length</th><th scope="col">Fame</th><th scope="col">Fame/hour</th>
            <th scope="col">Runs</th><th scope="col">Clears</th><th scope="col">Deaths</th><th scope="col">Status</th></tr></thead>
          <tbody>
            {#each page.rows as row (row.id)}
              <tr>
                <td><a href={`#/stats/sessions/${encodeURIComponent(row.id)}`}>{whenText(row.started)}</a></td>
                <td>{durationText(row.duration_micros)}</td><td>{fameText(row.fame, row.missing_baseline)}</td>
                <td>{rateText(row.fame_per_hour)}</td><td>{row.runs}</td><td>{row.clears}</td><td>{row.deaths}</td>
                <td>{row.ended === null ? 'In progress' : ''}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}<EmptyState heading={position.includeEmpty ? 'No sessions' : 'No sessions with fame or runs'} description="" />{/if}
      <nav aria-label="Session pages">
        <button type="button" aria-disabled={position.sessions.cursor === null} onclick={() => { if (position.sessions.cursor) goTo(null, []); }}>First</button>
        <button type="button" aria-disabled={!position.sessions.previous.length} onclick={() => {
          const previous = position.sessions.previous;
          if (previous.length) goTo(previous[previous.length - 1], previous.slice(0, -1));
        }}>Previous</button>
        <button type="button" aria-disabled={!page.next} onclick={() => {
          if (page.next) goTo(page.next, [...position.sessions.previous, position.sessions.cursor].slice(-100));
        }}>Next</button>
      </nav>
    {/if}
  {/snippet}
</StatsQuery>

<style>
  .bar { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: end; gap: var(--space-3); margin-bottom: var(--space-3); }
  .toggle { display: flex; align-items: center; gap: var(--space-2); min-height: var(--target-min); }
  table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  th, td { padding: var(--space-1) var(--space-2); text-align: right; border-bottom: 1px solid var(--border); }
  th:first-child, td:first-child { text-align: left; }
  nav { display: flex; gap: var(--space-3); margin-top: var(--space-3); }
  nav button { font: inherit; color: var(--text-primary); min-height: var(--target-min); border-radius: var(--radius-control); background: var(--surface-panel); border: 1px solid var(--border-strong); padding: var(--space-2) var(--space-3); }
  nav button[aria-disabled="true"] { color: var(--text-muted); }
</style>
