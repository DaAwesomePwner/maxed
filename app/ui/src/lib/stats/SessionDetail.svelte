<script lang="ts">
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { StatsPosition } from '../navigation/state';
  import { routeHref } from '../navigation/routes';
  import EmptyState from '../components/EmptyState.svelte';
  import StatsQuery from './StatsQuery.svelte';
  import StatsFilterBar from './StatsFilterBar.svelte';
  import ExportButton from './ExportButton.svelte';
  import MapsTable from './MapsTable.svelte';
  import { durationText, fameText, rateText, whenText } from './presentation';
  let { id, client, enabled = true, position, onChange, streamId = null, scopeKey = '', required = null, store = null,
    entryKey = '', onRowsReady }: {
    id: string; client?: Client; enabled?: boolean; position: StatsPosition; onChange: (patch: Partial<StatsPosition>) => void;
    streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
  let request = $derived({ id, character_id: position.characterId });
</script>

<StatsQuery {client} {enabled} query={{ kind: 'stats-session', request }} subject="session" {streamId} {scopeKey} {required} {store} {entryKey} {onRowsReady}>
  {#snippet children(history)}
    {@const detail = history.value?.kind === 'stats-session' ? history.value.detail : undefined}
    {#if detail === null}<EmptyState heading="Session not found" description="" />
    {:else if detail}
      {@const s = detail.session}
      <header class="bar">
        <h2>{whenText(s.started)}{s.ended === null ? ' · In progress' : ''}</h2>
        <StatsFilterBar {client} range={position.range} showRange={false} characterId={position.characterId} characters={detail.characters} {onChange} />
        <ExportButton {client} label="Export runs with party"
          request={{ kind: { type: 'session_runs', id }, filter: { range: null, character_id: position.characterId }, include_empty: false }} />
      </header>
      <dl class="totals">
        <div><dt>Length</dt><dd>{durationText(s.duration_micros)}</dd></div>
        <div><dt>Fame</dt><dd>{fameText(s.fame, s.missing_baseline)}</dd></div>
        <div><dt>Fame/hour</dt><dd>{rateText(s.fame_per_hour)}</dd></div>
        <div><dt>Runs</dt><dd>{s.runs}</dd></div><div><dt>Clears</dt><dd>{s.clears}</dd></div><div><dt>Deaths</dt><dd>{s.deaths}</dd></div>
      </dl>
      {#if s.ended === null}<p><a href="#/">Open in Home</a> for this session's fame chart.</p>{/if}
      <h3>Maps</h3>
      {#if detail.maps.length || detail.unrecorded_fame}<MapsTable rows={detail.maps} unrecorded={detail.unrecorded_fame} />
      {:else}<p>No recorded maps in this session.</p>{/if}
      <h3>Runs</h3>
      {#if detail.runs.length}
        <ul class="runs">{#each detail.runs as run (run.id)}
          <li><a href={routeHref({ kind: 'run', id: run.id })}>{run.dungeon}</a> · {whenText(run.started)}{run.outcome ? ` · ${run.outcome}` : ''}</li>
        {/each}</ul>
        {#if detail.runs_truncated}<p>Showing the first 500 runs.</p>{/if}
      {:else}<p>No runs in this session.</p>{/if}
    {/if}
  {/snippet}
</StatsQuery>

<style>
  .bar { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: end; gap: var(--space-3); margin-bottom: var(--space-3); }
  h2 { margin: 0; font-size: var(--text-title); }
  .totals { display: flex; flex-wrap: wrap; gap: var(--space-4, 24px); margin: 0 0 var(--space-3); }
  .totals div { display: grid; } dt { color: var(--text-secondary); } dd { margin: 0; font-variant-numeric: tabular-nums; }
  .runs { padding-left: var(--space-3); }
</style>
