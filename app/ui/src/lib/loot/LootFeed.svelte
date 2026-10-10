<script lang="ts">
  import { getContext, tick, untrack } from 'svelte';
  import type { TimeRange } from '../bindings/TimeRange';
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { DropFilter } from '../bindings/DropFilter';
  import type { DropCursor } from '../bindings/DropCursor';
  import { emptyListState, normalizeListState, type ListState } from '../navigation/state';
  import { createRunHistory, type HistoryController, type HistoryState } from '../runs/history';
  import { createHomeVisuals, emptyHomeVisuals, type HomeVisuals } from '../home/visuals';
  import { objectVisualTypes, spriteUrl } from '../bridge/visuals';
  import HistoryStatus from '../runs/HistoryStatus.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { historyStatus } from '../status/registry';
  import { HISTORY_CATCH_UP } from '../runs/policy';
  import EmptyState from '../components/EmptyState.svelte';
  import FeedToolbar from './FeedToolbar.svelte';
  import LootBagTable from './LootBagTable.svelte';
  import BagPanel from './BagPanel.svelte';
  import KpiStrip from '../home/v3/KpiStrip.svelte';
  import type { DropSummary } from '../bindings/DropSummary';
  import type { RunStory } from '../bindings/RunStory';
  import type { Route } from '../navigation/routes';
  import { feedKpis, dayGroups, dayLine } from './feed-screen';
  let { range = null, client, streamId = null, scopeKey = '', required = null, store = null, savedState = emptyListState(), onStateChange,
    entryKey = '', onRowsReady, enabled = true, seed, onRangeChange }: {
    range?: TimeRange | null; seed?: Extract<Route, { kind: 'loot' }>['seed']; onRangeChange?: (range: TimeRange | null) => void;
    client?: Client; streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; enabled?: boolean;
  } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  const defaults = (): DropFilter => ({ min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 });
  const restored = untrack(() => normalizeListState(savedState));
  const initialSeed = untrack(() => restored.lootPosition?.feed.seedApplied ? undefined : seed);
  let filter = $state(initialSeed ? { ...defaults(), range: untrack(() => range), place: initialSeed.place !== undefined ? { kind: 'dungeon' as const, name: initialSeed.place } : null, item_type: initialSeed.item ?? null } : restored.lootPosition?.feed.filter ?? defaults());
  let previous = $state<Array<DropCursor | null>>(initialSeed ? [] : restored.lootPosition?.feed.previous ?? []);
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading saved bags.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let visuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  let page = $derived(history.value?.kind === 'drops' ? history.value.page : null);
  let selectedDropId = $state(initialSeed ? null : restored.lootPosition?.feed.selectedDropId ?? null);
  let summary = $state.raw<DropSummary | null>(null);
  const filterKey = (value: DropFilter) => JSON.stringify([value.min_rank, value.place?.kind,
    value.place?.kind === 'dungeon' ? value.place.name : null, value.item_type, value.enchanted_only,
    value.range?.start, value.range?.end]);
  let currentFilterKey = $derived(filterKey(filter));
  let loadedSummaryFilterKey = $state<string | null>(null);
  let visibleSummary = $derived((history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable))
    && loadedSummaryFilterKey !== currentFilterKey ? null : summary);
  let summaryDays = $state<string[]>([]);
  let summaryRetryAvailable = $state(false);
  let summaryController = $state.raw<HistoryController>();
  let story = $state.raw<RunStory | null>(null);
  let storyRequired = $state<string | null>(null);
  let storyController = $state.raw<HistoryController>();
  const now = new Date();
  let groups = $derived(dayGroups(page?.rows ?? [], now));
  let requestedDays = $derived(groups.slice(0, 8).map(group => group.range));
  let selected = $derived(page?.rows.find(row => row.drop.id === selectedDropId) ?? page?.rows[0] ?? null);
  let selectedRunId = $derived(selected?.drop.run_id ?? null);
  let storyScope = $derived(JSON.stringify([scopeKey, streamId]));
  let region: HTMLDivElement;
  $effect(() => { if (initialSeed) untrack(persist); });
  $effect(() => {
    const owner = activeClient;
    if (!owner) { summaryController = undefined; summary = null; summaryDays = []; loadedSummaryFilterKey = null; summaryRetryAvailable = false; return; }
    let days: string[] = [], requestedFilterKey = '';
    const instance = createRunHistory(owner, next => {
      summaryRetryAvailable = next.retryAvailable && (next.phase === 'error' || next.phase === 'busy');
      if (next.phase === 'error' || next.phase === 'superseded' || (next.phase === 'busy' && next.retryAvailable)) { summary = null; summaryDays = []; }
      if (next.phase === 'ready' && next.value?.kind === 'drop-summary') { summary = next.value.summary; summaryDays = days; loadedSummaryFilterKey = requestedFilterKey; }
    }, { cache: connection?.historyCache });
    summaryController = { ...instance, update(input) { if (input.query.kind === 'drop-summary') { days = input.query.request.days.map(day => day.start); requestedFilterKey = filterKey(input.query.request.filter); } instance.update(input); } };
    return () => instance.dispose();
  });
  $effect(() => {
    const owner = activeClient, id = selectedRunId;
    storyScope;
    // Keep the selection checkpoint stable while unrelated Feed watermarks advance.
    storyRequired = untrack(() => required);
    story = null; storyController = undefined;
    if (!owner || !id) return;
    const instance = createRunHistory(owner, next => { story = next.phase !== 'error' && next.value?.kind === 'run' ? next.value.story : null; }, { cache: connection?.historyCache });
    storyController = instance;
    return () => instance.dispose();
  });
  $effect(() => connection?.onEvent?.(envelope => {
    const id = selectedRunId;
    if (!enabled || !id || envelope.stream_id !== streamId) return;
    const invalidation = envelope.event;
    const runId = 'bag_dropped' in invalidation ? invalidation.bag_dropped.run_id
      : 'drop_updated' in invalidation ? invalidation.drop_updated.run_id
      : 'run_ended' in invalidation ? invalidation.run_ended.id : null;
    if (runId === id && (storyRequired === null || BigInt(envelope.sequence) > BigInt(storyRequired))) {
      storyRequired = envelope.sequence;
    }
  }));
  $effect(() => {
    const instance = storyController, id = selectedRunId;
    if (enabled && id) { const input = { query: { kind: 'run' as const, id }, scopeKey, streamId, required: storyRequired, store }; untrack(() => instance?.update(input)); }
  });
  let paging = $state(false), missingRow = $state('');
  let filtered = $derived(filter.min_rank !== null || filter.place !== null || filter.item_type !== null || filter.enchanted_only || filter.range !== null);
  let available = $derived(!client || page !== null || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  let releasedEntry: string | undefined;
  let activeClient = $derived(enabled ? client : undefined);
  $effect(() => {
    const nextRange = range;
    untrack(() => {
      if (filter.range?.start === nextRange?.start && filter.range?.end === nextRange?.end) return;
      previous = []; paging = false; filter = { ...filter, range: nextRange, cursor: null }; persist();
    });
  });
  $effect(() => {
    const owner = activeClient;
    if (!owner) {
      controller = undefined;
      history = { phase: 'loading', value: null, message: 'Loading saved bags.', retryAvailable: false };
      return;
    }
    const instance = createRunHistory(owner, next => {
      history = next;
      if (next.phase === 'ready' || next.phase === 'error' || next.phase === 'superseded' || (next.phase === 'busy' && next.retryAvailable)) paging = false;
    }, { cache: connection?.historyCache });
    controller = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = controller;
    const input = { query: { kind: 'drops' as const, filter: $state.snapshot(filter) }, scopeKey, streamId, required, store };
    if (enabled) untrack(() => instance?.update(input));
  });
  $effect(() => {
    const instance = summaryController;
    if (!page) return;
    const input = { query: { kind: 'drop-summary' as const, request: { filter: { ...$state.snapshot(filter), cursor: null }, days: requestedDays, include_items: true } }, scopeKey, streamId, required, store };
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
    const types = objectVisualTypes([...(story?.drops.flatMap(drop => [drop.bag_type, ...drop.contents.map(item => item.item_type)]) ?? []), filter.item_type, ...(page?.rows.flatMap(({ drop }) =>
      [drop.bag_type, drop.source_enemy, ...drop.contents.map(item => item.item_type)]) ?? [])]);
    visualController?.update(types);
  });
  $effect.pre(() => {
    const rows = page?.rows;
    if (!rows || !region) return;
    const active = document.activeElement;
    const id = active instanceof HTMLElement && region.contains(active) ? active.closest<HTMLElement>('[data-row-id]')?.dataset.rowId : null;
    if (!id) return;
    void tick().then(() => {
      if (!region?.isConnected || active?.isConnected) return;
      const pane = region.closest('main');
      const replacement = [...region.querySelectorAll<HTMLElement>('[data-row-id]')].find(row => row.dataset.rowId === id);
      if (replacement) { replacement.focus({ preventScroll: true }); return; }
      pane?.querySelector<HTMLElement>('h1')?.focus({ preventScroll: true });
      if (pane) pane.scrollTop = Math.max(0, Math.min(pane.scrollTop, pane.scrollHeight - pane.clientHeight));
      missingRow = 'Row Removed';
      onStateChange?.({ focusedRowId: null, ...(pane ? { scrollTop: pane.scrollTop } : {}) });
    });
  });
  $effect(() => {
    const ready = available, key = entryKey;
    if (!ready || (client && !enabled) || releasedEntry === key) return;
    let current = true;
    void tick().then(() => {
      if (current) { releasedEntry = key; onRowsReady?.(key); }
    });
    return () => { current = false; };
  });
  function persist(resetPosition = true) {
    missingRow = '';
    const position = normalizeListState(savedState).lootPosition ?? restored.lootPosition;
    onStateChange?.({ lootPosition: { item: null, statsRange: null, collectionDungeon: null, ...position, range,
      feed: { filter: $state.snapshot(filter), previous: $state.snapshot(previous), selectedDropId: selected?.drop.id ?? selectedDropId, ...(seed ? { seedApplied: true } : {}) } }, ...(resetPosition ? { scrollTop: 0, focusedRowId: null } : {}) });
    const pane = region?.closest('main');
    if (pane && resetPosition) pane.scrollTop = 0;
  }
  function retry() { controller?.retry(); summaryController?.retry(); }
  function apply(next: DropFilter) {
    const same = JSON.stringify(next) === JSON.stringify($state.snapshot(filter));
    previous = []; paging = false; selectedDropId = null; filter = next; persist();
    if (same) retry();
  }
  function next() {
    if (paging || !page?.next) return;
    paging = true; previous = [...previous, filter.cursor].slice(-100);
    filter = { ...filter, cursor: page.next }; persist();
  }
  function back() {
    if (paging || !previous.length) return;
    paging = true;
    const cursor = previous[previous.length - 1]; previous = previous.slice(0, -1);
    filter = { ...filter, cursor }; persist();
  }
  function first() {
    if (paging || filter.cursor === null) return;
    paging = true; previous = []; filter = { ...filter, cursor: null }; persist();
  }
</script>

<BusyStatus message={client ? historyStatus(history, 'bags') : null} />
<div class="feed" bind:this={region}>
  <HistoryStatus {history} />
  <p class="sr-only" role="status" aria-live="polite">{missingRow}</p>
  <KpiStrip cells={feedKpis(visibleSummary)} />
  <FeedToolbar {client} {filter} {visuals} {enabled} onApply={apply} />
  {#if !client}
    <EmptyState state="unavailable" heading="Loot unavailable" description="" />
  {:else}
    {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<p>{history.message}</p>{/if}
    {#if (history.retryAvailable && (history.phase === 'error' || history.phase === 'busy')) || summaryRetryAvailable}<div class="error" role="alert">{summaryRetryAvailable && history.phase === 'ready' ? "Couldn't load bag totals." : "Couldn't load bags."} <button class="ctl" type="button" onclick={retry}>Retry</button></div>{/if}
    <div class="body" class:with-panel={selected !== null}>
      <div class="table-card">
        <div class="table-scroll">
          {#if history.phase === 'error'}<EmptyState state="error" heading="Loot Error" description="" />
          {:else if history.phase === 'busy' && history.retryAvailable}<EmptyState state="unavailable" heading="Loot history busy" description="" />
          {:else if !page}<EmptyState state="loading" heading="Loading bags" description="" />{/if}
          {#if page}
            {#if page.rows.length === 0 && history.phase !== 'error' && history.phase !== 'busy'}
              <EmptyState heading={filtered ? 'No bags match these filters' : 'No bags yet'}>
                {#snippet action()}<p>{filtered ? 'Try a longer range or another bag tier.' : 'Capture a run to see its bags here.'}</p>{/snippet}
              </EmptyState>
              {#if filtered}<button type="button" class="ctl" onclick={() => { onRangeChange?.(null); apply({ ...defaults(), range: onRangeChange ? null : range }); }}>Clear filters</button>{/if}
            {:else if page.rows.length > 0}
              <LootBagTable rows={page.rows} {visuals} {now} columns="feed" resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)}
                selectedDropId={selected?.drop.id} onSelect={row => { selectedDropId = row.drop.id; persist(false); }}
                groups={groups.map(group => ({ ...group, line: dayLine(visibleSummary?.days[summaryDays.indexOf(group.key)] ?? null) }))} />
            {/if}
          {/if}
        </div>
        <footer><span>Bags Maxed saw drop. Not pickups.</span><nav aria-label="Loot pages">
          <button class="ctl ghost" type="button" aria-disabled={paging || filter.cursor === null} onclick={first}>Latest</button>
          <button class="ctl sq" type="button" aria-label="Newer bags" aria-disabled={paging || previous.length === 0} onclick={back}>‹</button>
          <button class="ctl sq" type="button" aria-label="Older bags" aria-disabled={paging || !page?.next} onclick={next}>›</button>
        </nav></footer>
      </div>
      {#if selected}<BagPanel row={selected} {visuals} {now} {story} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite)} />{/if}
    </div>
  {/if}
</div>
<style>
  .feed { overflow-anchor: none; display: grid; gap: 10px; min-width: 0; font-size: 12px; }
  .body { display: grid; grid-template-columns: minmax(0, 1fr); gap: 10px; align-items: start; min-width: 0; }
  .body.with-panel { grid-template-columns: minmax(0, 1fr) 300px; }
  .table-card { display: flex; flex-direction: column; max-height: max(300px, calc(100vh - 295px)); min-width: 0; padding: 10px 6px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); }
  .table-scroll { min-height: 0; overflow-y: auto; }
  p { margin: 0; color: var(--text-secondary); }
  footer, nav { display: flex; align-items: center; gap: 6px; } footer { flex: none; padding-top: 8px; flex-wrap: wrap; justify-content: space-between; color: var(--text-muted); }
  .error { display: flex; align-items: center; gap: 8px; color: var(--danger); }
  @media (max-width: 1100px) { .body.with-panel { grid-template-columns: minmax(0, 1fr); } .body :global(aside) { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
