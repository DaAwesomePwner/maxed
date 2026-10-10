<script lang="ts">
  import { SHELL_CONNECTION, type ShellConnection } from '../lib/shell/context';
  import { getContext, untrack } from 'svelte';
  import { createRunHistory, type HistoryController } from '../lib/runs/history';
  import type { DropTotals } from '../lib/bindings/DropTotals';
  import { headerLine } from '../lib/loot/feed-screen';
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import { normalizeListState, type ListState } from '../lib/navigation/state';
  import type { TimeRange } from '../lib/bindings/TimeRange';
  import LootHeader from '../lib/loot/LootHeader.svelte';
  import RangeSwitch from '../lib/loot/RangeSwitch.svelte';
  import type { Route } from '../lib/navigation/routes';
  import CollectionView from '../lib/loot/CollectionView.svelte';
  import ItemHistoryView from '../lib/loot/ItemHistoryView.svelte';
  import LootFeed from '../lib/loot/LootFeed.svelte';
  import LootStatsView from '../lib/loot/LootStatsView.svelte';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { route, entryKey = '', onRowsReady, client, enabled, streamId, scopeKey, required, store, savedState, onStateChange }: {
    route: Extract<Route, { kind: 'loot' | 'item' }>;
    client?: Client; enabled?: boolean; streamId?: string | null; scopeKey?: string;
    required?: string | null; store?: StoreStatus | null; savedState?: ListState;
    onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
  let view = $derived(route.kind === 'item' ? null : route.view);
  let range = $derived(normalizeListState(savedState).lootPosition?.range ?? null);
  let totals = $state.raw<DropTotals | null>(null);
  let summaryController = $state.raw<HistoryController>();
  $effect(() => {
    if (!client || enabled === false || view === null) { totals = null; summaryController = undefined; return; }
    const instance = createRunHistory(client, next => { if (next.value?.kind === 'drop-summary') totals = next.value.summary.totals; }, { cache: historyCache });
    summaryController = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = summaryController;
    const input = { query: { kind: 'drop-summary' as const, request: { filter: { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 }, days: [], include_items: false } }, scopeKey: scopeKey ?? '', streamId: streamId ?? null, required: required ?? null, store: store ?? null };
    untrack(() => instance?.update(input));
  });
  // Router saves do not republish savedState; retain child patches for range changes.
  let latestPosition = untrack(() => normalizeListState(savedState).lootPosition);
  $effect(() => { latestPosition = normalizeListState(savedState).lootPosition; });
  function saveState(patch: Partial<ListState>) {
    if (patch.lootPosition) latestPosition = patch.lootPosition;
    onStateChange?.(patch);
  }
  function changeRange(next: TimeRange | null) {
    range = next;
    const position = normalizeListState({ lootPosition: {} }).lootPosition!;
    saveState({ lootPosition: { ...position, ...latestPosition, range }, scrollTop: 0, focusedRowId: null });
  }
</script>

{#if view}
  <LootHeader {view} subline={headerLine(totals)}>
    {#if view !== 'collection'}<RangeSwitch {range} onChange={changeRange} />{/if}
  </LootHeader>
{/if}
{#if route.kind === 'item'}
  {#key route.type}
    <ItemHistoryView itemType={route.type} {client} {enabled} {streamId} {scopeKey} {required} {store} {savedState} onStateChange={saveState} {entryKey} {onRowsReady} />
  {/key}
{:else if route.view === 'stats'}
  <LootStatsView {range} {client} {enabled} {streamId} {scopeKey} {required} {store} {savedState} onStateChange={saveState} {entryKey} {onRowsReady} />
{:else if route.view === 'collection'}
  <CollectionView {client} {enabled} {streamId} {scopeKey} {required} {store} {savedState} onStateChange={saveState} {entryKey} {onRowsReady} />
{:else}
  <LootFeed onRangeChange={changeRange} seed={route.seed} {range} {client} {enabled} {streamId} {scopeKey} {required} {store} {savedState} onStateChange={saveState} {entryKey} {onRowsReady} />
{/if}

