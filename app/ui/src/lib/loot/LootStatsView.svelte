<script lang="ts">
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import { getContext, tick, untrack } from 'svelte';
  import type { TimeRange } from '../bindings/TimeRange';
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { LootStatsFilter } from '../bindings/LootStatsFilter';
  import { normalizeListState, type ListState } from '../navigation/state';
  import { createRunHistory, type HistoryController, type HistoryState } from '../runs/history';
  import { createHomeVisuals, emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import { objectVisualTypes } from '../bridge/visuals';
  import HistoryStatus from '../runs/HistoryStatus.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { historyStatus } from '../status/registry';
  import { HISTORY_CATCH_UP } from '../runs/policy';
  import EmptyState from '../components/EmptyState.svelte';
  import LootStatsTables from './LootStatsTables.svelte';
  import KpiStrip from '../home/v3/KpiStrip.svelte';
  import { bestDungeon, statsKpis, statsSortKeys, type StatsSort } from './stats-screen';
  import { bagLabel } from './presentation';
  import '../styles/controls.css';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { range = null, client, streamId = null, scopeKey = '', required = null, store = null,
    entryKey = '', onRowsReady, enabled = true, savedState, onStateChange }: {
    range?: TimeRange | null;
    client?: Client; streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; enabled?: boolean;
  } = $props();
  let root: HTMLDivElement;
  let lastRange = untrack(() => JSON.stringify(range));
  let sortState = $derived(normalizeListState(savedState).lootPosition?.statsSort ?? { key: 'runs_ended', descending: true });
  let sort = $derived(statsSortKeys.includes(sortState.key as StatsSort) ? sortState.key as StatsSort : 'runs_ended');
  function changeSort(key: StatsSort) {
    sortState = { key, descending: key === sort ? !sortState.descending : false };
    const position = normalizeListState({ lootPosition: {} }).lootPosition!;
    onStateChange?.({ lootPosition: { ...position, ...normalizeListState(savedState).lootPosition, range, statsSort: sortState } });
  }
  $effect(() => {
    const next = JSON.stringify(range);
    if (next !== lastRange) { const pane = root?.closest('main'); if (pane) pane.scrollTop = 0; }
    lastRange = next;
  });
  let portals = $state.raw<Map<string, number>>(new Map());
  let filter = $derived<LootStatsFilter>({ range });
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading loot stats.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let visuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let stats = $derived(history.value?.kind === 'loot-stats' ? history.value.stats : null);
  let hasData = $derived(stats && (stats.dungeons.length > 0 || stats.sources.length > 0 || stats.totals.length > 0 || stats.whites > 0));
  let available = $derived(!client || stats !== null || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  let releasedEntry: string | undefined;
  let activeClient = $derived(enabled ? client : undefined);
  $effect(() => {
    const owner = activeClient;
    if (!owner) {
      controller = undefined;
      history = { phase: 'loading', value: null, message: 'Loading loot stats.', retryAvailable: false };
      return;
    }
    const instance = createRunHistory(owner, next => { history = next; }, { cache: historyCache });
    controller = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = controller;
    const input = { query: { kind: 'loot-stats' as const, filter }, scopeKey, streamId, required, store };
    if (enabled) untrack(() => instance?.update(input));
  });
  $effect(() => {
    const owner = activeClient;
    if (!owner) { visualController = undefined; visuals = emptyHomeVisuals(); return; }
    const instance = createHomeVisuals(owner, next => { visuals = next; });
    visualController = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    visualController?.update(objectVisualTypes([
      ...(stats?.sources.map(row => row.source_enemy) ?? []),
      ...(stats?.totals.map(row => row.bag_type ?? null) ?? []),
      ...(stats?.dungeons.flatMap(row => [row.best?.bag_type ?? null, ...row.bags.map(tier => tier.bag_type ?? null)]) ?? []),
      ...(stats?.dungeons.map(row => portals.get(row.dungeon) ?? null) ?? []),
    ]));
  });
  $effect(() => {
    const owner = activeClient;
    portals = new Map();
    if (!owner) return;
    let current = true, revision = 0;
    async function load() {
      const request = ++revision;
      try {
        const rows = await owner!.listCatalogDungeonOptions();
        if (current && request === revision) portals = new Map(rows.filter(row => row.portal_type != null).map(row => [row.name, row.portal_type!]));
      } catch { if (current && request === revision) portals = new Map(); }
    }
    void load();
    const release = owner.onVisualsChanged(() => { void load(); });
    return () => { current = false; release(); };
  });
  let kpis = $derived.by(() => {
    if (!stats) return [];
    const cells = statsKpis(stats);
    const best = bestDungeon(stats.dungeons)?.best;
    if (best) cells[4].value = bagLabel(best.bag_rank, visuals.objects.find(row => row.object_type === best.bag_type) ?? null);
    return cells;
  });
  $effect(() => {
    const ready = available, key = entryKey;
    if (!ready || (client && !enabled) || releasedEntry === key) return;
    let current = true;
    void tick().then(() => { if (current) { releasedEntry = key; onRowsReady?.(key); } });
    return () => { current = false; };
  });
</script>

<BusyStatus message={client ? historyStatus(history, 'loot stats') : null} />
<div class="loot-stats" bind:this={root}>
  <HistoryStatus {history} />
  {#if !client}
    <EmptyState state="unavailable" heading="Loot unavailable" description="" />
  {:else}
    {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<p>{history.message}</p>{/if}
    {#if history.retryAvailable && (history.phase === 'error' || history.phase === 'busy')}<button class="ctl" type="button" onclick={() => controller?.retry()}>Retry</button>{/if}
    {#if history.phase === 'error'}<EmptyState state="error" heading="Couldn't load bags." description="" />
    {:else if history.phase === 'busy' && history.retryAvailable}<EmptyState state="unavailable" heading="Loot history busy" description="" />
    {:else if !stats}<EmptyState state="loading" heading="Loading loot stats" description="" />{/if}
    {#if stats}
      <KpiStrip cells={kpis} />
      {#if hasData}<LootStatsTables {stats} {visuals} {portals} {sort} descending={sortState.descending} onSort={changeSort} ranged={filter.range !== null} />
      {:else if history.phase !== 'error' && history.phase !== 'busy'}<EmptyState heading={filter.range ? 'No bags in this range' : 'No bags yet'} description={filter.range ? 'Choose All time to see more bags.' : 'Capture a run to see its bags here.'} />{/if}
    {/if}
  {/if}
</div>

<style>
  .loot-stats { overflow-anchor: none; display: grid; gap: 10px; min-width: 0; }
  p { margin: 0; color: var(--text-secondary); }
  button { justify-self: start; }
  .loot-stats :global([data-kpi="dry"] .value) { color: var(--estimated); }
  .loot-stats :global([data-kpi="best"] .value) { font-size: 20px; overflow: hidden; text-overflow: ellipsis; }
  .loot-stats :global(.kpi-strip .sub) { white-space: normal; overflow: visible; overflow-wrap: anywhere; }
  @media (max-width: 1000px) { .loot-stats :global(.kpi-strip) { grid-template-columns: repeat(3, minmax(0, 1fr)) !important; } }
</style>
