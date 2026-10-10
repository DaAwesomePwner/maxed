<script lang="ts">
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import { createLogsSearch, validLogsQuery } from './search.svelte';
  import { createLogsList } from './list.svelte';
  import { getContext, untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { EventFilter } from '../bindings/EventFilter';
  import type { EventCursor } from '../bindings/EventCursor';
  import { defaultLogsPosition, emptyListState, normalizeListState, type ListState } from '../navigation/state';
  import { createRunHistory, type HistoryController, type HistoryState } from '../runs/history';
  import HistoryStatus from '../runs/HistoryStatus.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { historyStatus } from '../status/registry';
  import { HISTORY_CATCH_UP } from '../runs/policy';
  import EventTable from './EventTable.svelte';
  import EventPanel from './EventPanel.svelte';
  import RunCard from './RunCard.svelte';
  import LogsHeader from './LogsHeader.svelte';
  import RangeSwitch from '../loot/RangeSwitch.svelte';
  import RunScopeChip from './RunScopeChip.svelte';
  import Pager from './Pager.svelte';
  import NewDataPill from './NewDataPill.svelte';
  import ErrorBar from './ErrorBar.svelte';
  import { kindSegments, eventKinds } from './events-screen';
  import { countLine } from './logs-format';
  import { routeHref } from '../navigation/routes';
  import { createLogsSummaries } from './summaries.svelte';
  import type { RunStory } from '../bindings/RunStory';
  import type { ChatOverview } from '../bindings/ChatOverview';
  import type { ObjectVisual } from '../bindings/ObjectVisual';
  import { objectVisual } from '../bridge/visual-index';
  import { objectVisualTypes } from '../bridge/visuals';
  import { serializeEventReads } from './events-reader';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { pendingCount = 0, onHistoryApplied, client, runId = null, streamId = null, scopeKey = '', required = null, store = null, savedState = emptyListState(), onStateChange,
    entryKey = '', onRowsReady, enabled = true }: {
    runId?: string | null; client?: Client; streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    pendingCount?: number; onHistoryApplied?: (key: string, required: string | null) => void;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; enabled?: boolean;
  } = $props();
  const defaults = (): EventFilter => ({ kinds: [], run_id: runId, dungeon: null, cursor: null, limit: 50 });
  const restored = untrack(() => normalizeListState(savedState));
  const savedEvents = untrack(() => {
    const saved = restored.logsPosition?.events;
    return saved && saved.filter.run_id === runId && saved.filter.dungeon === null ? saved : null;
  });
  let filter = $state({ ...defaults(), ...savedEvents?.filter, range: restored.logsPosition?.range ?? null, query: (savedEvents?.filter.query ?? '').trim() });
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading saved events.', retryAvailable: false });
  let readRequired: string | null = null;
  let controller = $state.raw<HistoryController>();
  let region: HTMLDivElement;
  let page = $derived(history.value?.kind === 'events' ? history.value.page : null);
  let available = $derived(!client || page !== null || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  const list = createLogsList<EventCursor>({
    previous: savedEvents?.previous ?? [], scrollTop: restored.scrollTop,
    required: () => required, namespace: () => `${scopeKey}:${streamId}`, pendingCount: () => pendingCount,
    cursor: () => filter.cursor, setCursor: cursor => { filter = { ...filter, cursor }; }, next: () => page?.next,
    region: () => region, rows: () => page?.rows, persist: () => persist(),
    available: () => available, enabled: () => !client || enabled, entryKey: () => entryKey,
    onReady: key => onRowsReady?.(key), onStateChange: patch => onStateChange?.(patch),
  });

  let activeClient = $derived(enabled ? client : undefined);
  let reader = $derived(activeClient ? serializeEventReads(activeClient) : undefined);
  let selectedId = $state(savedEvents?.selectedId ?? null);
  let selected = $derived(page?.rows.find(row => row.id === selectedId) ?? page?.rows[0] ?? null);
  let selectedRunId = $derived(selected?.run_id ?? null);
  let story = $state.raw<RunStory | null>(null), chat = $state.raw<ChatOverview | null>(null);
  let loadedRunId = $state<string | null>(null);
  let visuals = $state.raw<ObjectVisual[]>([]);
  const className = (id: number) => objectVisual(visuals, id)?.name ?? null;
  const search = createLogsSearch({ query: () => filter.query, apply: query => apply({ ...filter, query, cursor: null }) });
  let validation = $state('');
  let rangeKey = $derived(JSON.stringify([scopeKey, streamId, runId, filter.range, list.appliedRequired]));
  // A failed supplementary summary must not prevent the selected run loading.
  const summaries = createLogsSummaries({
    cache: historyCache,
    reader: () => reader, ready: () => history.phase === 'ready', key: () => rangeKey,
    filter: () => ({ ...defaults(), range: filter.range }),
    scopeKey: () => scopeKey, streamId: () => streamId, store: () => store,
  });
  $effect(() => {
    const owner = reader, id = selectedRunId ?? runId, ready = summaries.settled && history.phase === 'ready';
    if (!owner || !ready || loadedRunId === id) return;
    let overview: HistoryController | undefined;
    const instance = createRunHistory(owner, next => {
      if (next.phase === 'ready' && next.value?.kind === 'run') {
        story = next.value.story;
        overview = createRunHistory(owner, next => {
          if (next.phase === 'ready' && next.value?.kind === 'chat-overview') { chat = next.value.overview; loadedRunId = id; }
        }, { cache: historyCache });
        untrack(() => overview?.update({ query: { kind: 'chat-overview', request: { run_id: id } }, scopeKey, streamId, required: null, store }));
      }
    }, { cache: historyCache });
    const timer = setTimeout(() => {
      story = null; chat = null;
      if (id) untrack(() => instance.update({ query: { kind: 'run', id }, scopeKey, streamId, required: null, store }));
      else loadedRunId = null;
    }, 150);
    return () => { clearTimeout(timer); instance.dispose(); overview?.dispose(); };
  });
  $effect(() => {
    const owner = activeClient;
    const types = objectVisualTypes(page?.rows.map(row => 'class_id' in row.kind ? row.kind.class_id : null) ?? []);
    let current = true;
    visuals = [];
    if (!owner || !types.length) return;
    const load = () => { void owner.getObjectVisuals(types).then(value => { if (current) visuals = value.objects; }).catch(() => {}); };
    load(); const release = owner.onVisualsChanged(load);
    return () => { current = false; release(); };
  });
  function select(id: string) { selectedId = id; persist(false); }
  $effect(() => {
    const owner = reader;
    if (!owner) {
      controller = undefined;
      history = { phase: 'loading', value: null, message: 'Loading saved events.', retryAvailable: false };
      return;
    }
    const instance = createRunHistory(owner, next => {
      history = next;
      if (next.phase === 'ready') onHistoryApplied?.(entryKey, readRequired);
      if (next.phase === 'ready' || next.phase === 'error' || next.phase === 'superseded' || (next.phase === 'busy' && next.retryAvailable)) list.paging = false;
    }, { cache: historyCache });
    controller = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = controller;
    const input = { query: { kind: 'events' as const, filter: $state.snapshot(filter) }, scopeKey, streamId, required: list.appliedRequired, store };
    if (enabled) untrack(() => { readRequired = input.required; instance?.update(input); });
  });
  function persist(resetScroll = true) {
    const position = normalizeListState(savedState).logsPosition ?? restored.logsPosition;
    list.persist({ logsPosition: {
      ...(position ?? defaultLogsPosition()),
      range: filter.range,
      events: { filter: $state.snapshot(filter), previous: $state.snapshot(list.previous), selectedId: selectedId ?? selected?.id ?? null },
    } }, resetScroll);
  }
  function apply(next: EventFilter) {
    const text = (next.query ?? '').trim();
    if (!validLogsQuery(text)) { validation = 'Search must be at most 128 characters with no NUL.'; return; }
    validation = '';
    list.apply(next, $state.snapshot(filter), () => {
      search.accept(next.query);
      selectedId = null; filter = { ...next, range: next.range ?? null, query: text };
    }, () => controller?.retry());
  }
</script>

<LogsHeader view="events" subline={countLine(summaries.allTime?.total ?? null, 'events seen', summaries.allTime?.first_at ?? null)}><RangeSwitch range={filter.range} label="Logs date range" onChange={range => apply({ ...filter, range, cursor: null })} /></LogsHeader>
<BusyStatus message={client ? historyStatus(history, 'events') : null} />
<div class="feed" bind:this={region}>
  <HistoryStatus {history} />
  <p class="sr-only" role="status" aria-live="polite">{list.missingRow}</p>
  <div class="toolbar">
    <div class="seg" role="group" aria-label="Event kind">{#each kindSegments(summaries.summaryKey === rangeKey ? summaries.summary : null) as segment}<button aria-pressed={segment.key === 'all' ? !filter.kinds.length : filter.kinds[0] === eventKinds[segment.key]} onclick={() => apply({ ...filter, kinds: segment.key === 'all' ? [] : [eventKinds[segment.key]], cursor: null })}>{segment.label} <span>{segment.count}</span></button>{/each}</div>
    <input class="ctl" aria-label="Find a player or boss" placeholder="Find a player or boss" bind:value={search.query} />
  </div>
  {#if runId && activeClient}<RunScopeChip {runId} client={activeClient} suppliedRun={story?.run.id === runId ? story.run : null} removeHref="#/logs/events" links={[{ label: 'Chat in this run ›', href: routeHref({ kind: 'logs', view: 'chat', channel: 'all', runId }) }, { label: 'Run summary ›', href: routeHref({ kind: 'run', id: runId }) }]} />{/if}
  <p class="legend"><span class="unconf">unconfirmed</span> Seen without a confirming observation.</p>
  {#if validation}<p role="alert">{validation}</p>{/if}
  {#if history.phase === 'error'}<ErrorBar message="Couldn't load events. Your filters are kept." onRetry={() => controller?.retry()} />{/if}
  <div class="layout"><section class="list">
    {#if list.hasNew}<NewDataPill count={pendingCount} noun="events" showCount={!filter.range && !filter.kinds.length && !filter.query} onClick={list.showNew} />{/if}
    {#if !client}<div class="empty"><h2>Events unavailable</h2></div>{:else}
      {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<p>{history.message}</p>{/if}
      {#if history.phase === 'busy' && history.retryAvailable}<p>Event history busy</p>{:else if !page && history.phase !== 'error'}<p>Loading events</p>{/if}
      {#if page}
        {#if !page.rows.length && history.phase !== 'error' && history.phase !== 'busy'}
          <div class="empty"><h2>{filter.range ? 'No events in this range' : filter.query || filter.kinds.length ? 'No events match your filters' : 'No events yet'}</h2>
            {#if filter.range}<button class="ctl" onclick={() => apply({ ...filter, range: null, cursor: null })}>Show all time</button>{:else if filter.query || filter.kinds.length}<button class="ctl" onclick={() => apply({ ...filter, query: '', kinds: [], cursor: null })}>Clear filters</button>{:else}<p>Events will appear when Maxed sees them in-game.</p>{/if}</div>
        {:else if page.rows.length}<EventTable rows={page.rows} selectedId={selected?.id ?? null} onSelect={select} client={activeClient} {className} />{/if}
      {/if}
    {/if}
    <footer><span>Events Maxed saw in the instances you were in.</span><Pager atLatest={filter.cursor === null} hasOlder={!!page?.next} loading={list.paging} noun="events" onLatest={list.first} onNewer={list.back} onOlder={list.next} /></footer>
  </section><aside><EventPanel view={selected} {className} />{#if selectedRunId}<RunCard runId={selectedRunId} story={story?.run.id === selectedRunId ? story : null} chat={loadedRunId === selectedRunId ? chat : null} />{/if}</aside></div>
</div>
<style>
  .feed { overflow-anchor: none; overflow-wrap: anywhere; display: grid; gap: 10px; min-width: 0; }
  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; min-width: 0; } .toolbar input { flex: 1; min-width: 160px; max-width: 260px; } .seg { flex-wrap: wrap; } .seg span { margin-left: 4px; color: var(--text-muted); font-variant-numeric: tabular-nums; }
  .layout { display: grid; grid-template-columns: minmax(0, 1fr) 300px; gap: 12px; align-items: start; min-width: 0; }
  .list { position: relative; min-width: 0; padding: 8px 6px 6px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); }
  aside { display: grid; gap: 10px; min-width: 0; padding: 0; margin: 0; border: 0; background: none; }
  p { margin: 8px; color: var(--text-secondary); font-size: 12px; } .legend { margin: 0; font-size: 11px; color: var(--text-muted); }
  .empty { padding: 20px 14px; margin: 6px; border: 1px dashed var(--border); border-radius: 6px; text-align: center; font-size: 12px; } .empty h2 { font-size: 13px; }
  footer { display: flex; align-items: center; flex-wrap: wrap; justify-content: space-between; gap: 8px; border-top: 1px solid var(--border); margin-top: 8px; padding: 8px 4px 0; color: var(--text-muted); font-size: 11px; }
  @media (max-width: 1100px) { .layout { grid-template-columns: minmax(0, 1fr); } aside { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
