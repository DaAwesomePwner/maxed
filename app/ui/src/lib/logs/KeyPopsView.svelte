<script lang="ts">
  import { filteredByRules, includesKeyPopDungeon } from '../alerts/preferences';
  import { dungeonCatalog, dungeonMetadata, watchDungeonCatalog } from '../alerts/dungeon-catalog';
  $effect(() => { if (client) return watchDungeonCatalog(client); });
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
  import LogsHeader from './LogsHeader.svelte';
  import RangeSwitch from '../loot/RangeSwitch.svelte';
  import Pager from './Pager.svelte';
  import NewDataPill from './NewDataPill.svelte';
  import ErrorBar from './ErrorBar.svelte';
  import { countLine } from './logs-format';
  import KpiStrip from '../home/v3/KpiStrip.svelte';
  import DungeonRanking from './DungeonRanking.svelte';
  import { keyKpis, mutedDungeons } from './keys-screen';
  import type { AlertPreferences } from '../bindings/AlertPreferences';
  import { createLogsSummaries } from './summaries.svelte';
  import { serializeEventReads } from './events-reader';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { pendingCount = 0, onHistoryApplied, client, runId = null, streamId = null, scopeKey = '', required = null, store = null, savedState = emptyListState(), onStateChange,
    entryKey = '', onRowsReady, enabled = true }: {
    runId?: string | null; client?: Client; streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    pendingCount?: number; onHistoryApplied?: (key: string, required: string | null) => void;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; enabled?: boolean;
  } = $props();
  const defaults = (): EventFilter => ({ kinds: ['portal_opened'], run_id: runId, opener_local: null, dungeon: null, cursor: null, limit: 50 });
  const restored = untrack(() => normalizeListState(savedState));
  const savedEvents = untrack(() => {
    const saved = restored.logsPosition?.keys;
    return saved && saved.filter.run_id === runId && saved.filter.kinds.length === 1 && saved.filter.kinds[0] === 'portal_opened' ? saved : null;
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
  const search = createLogsSearch({ query: () => filter.query, apply: query => apply({ ...filter, query, cursor: null }) });
  let validation = $state('');
  let rangeKey = $derived(JSON.stringify([scopeKey, streamId, runId, filter.range, list.appliedRequired]));
  const summaries = createLogsSummaries({
    cache: historyCache,
    reader: () => reader, ready: () => history.phase === 'ready', key: () => rangeKey,
    filter: () => ({ ...defaults(), range: filter.range }),
    scopeKey: () => scopeKey, streamId: () => streamId, store: () => store,
  });
  let dungeons = $state<string[]>([]), catalogMessage = $state('');
  $effect(() => {
    const owner = activeClient;
    dungeons = []; catalogMessage = '';
    if (!owner) return;
    let disposed = false, revision = 0;
    async function load() {
      const request = ++revision;
      try {
        const result = await owner!.listCatalogDungeons();
        if (!disposed && request === revision) { dungeons = result; catalogMessage = ''; }
      } catch {
        if (!disposed && request === revision) { dungeons = []; catalogMessage = 'Dungeon catalog unavailable. Check assets in Settings.'; }
      }
    }
    void load();
    const release = owner.onVisualsChanged(() => { void load(); });
    return () => { disposed = true; release(); };
  });
  let prefs = $state.raw<AlertPreferences | null>(null), prefsRead = $state(false);
  let prefsMessage = $state('');
  let muted = $derived(prefs ? mutedDungeons(prefs, dungeons, $dungeonCatalog) : new Set<string>());
  let rankingRows = $derived(summaries.summaryKey === rangeKey ? summaries.summary?.dungeons ?? [] : []);
  let rankedMuted = $derived(new Set(rankingRows.filter(row => prefs && !includesKeyPopDungeon(prefs, row.dungeon)).map(row => row.dungeon)));
  let ruleFiltered = $derived(new Set(rankingRows.filter(row => prefs && filteredByRules(prefs, row.dungeon, dungeonMetadata(row.dungeon, $dungeonCatalog))).map(row => row.dungeon)));

  $effect(() => {
    const owner = activeClient, ready = summaries.allTimeRead;
    if (!owner || !ready || prefsRead) return;
    let current = true;
    void owner.querySettings().then(settings => { if (current) { prefs = settings.alerts; prefsRead = true; } })
      .catch(() => { if (current) { prefsMessage = 'Alert settings unavailable. Check Settings.'; prefsRead = true; } });
    return () => { current = false; };
  });
  function toggleDungeon(dungeon: string) { apply({ ...filter, dungeon: filter.dungeon === dungeon ? null : dungeon, cursor: null }); }
  function select(id: string) {
    selectedId = id; persist(false);
  }
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
      keys: { filter: $state.snapshot(filter), previous: $state.snapshot(list.previous), selectedId: selectedId ?? selected?.id ?? null },
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

<LogsHeader view="keys" subline={countLine(summaries.allTime?.by_kind.portal_opened ?? null, 'key pops seen', summaries.allTime?.first_at ?? null)}><RangeSwitch range={filter.range} label="Logs date range" onChange={range => apply({ ...filter, range, cursor: null })} /></LogsHeader>
<div class="key-kpis"><KpiStrip cells={keyKpis(summaries.summaryKey === rangeKey ? summaries.summary : null, filter.range, muted, new Date(), prefs)} valueTitles /></div>
<BusyStatus message={client ? historyStatus(history, 'events') : null} />
<div class="feed" bind:this={region}>
  <HistoryStatus {history} />
  <p class="sr-only" role="status" aria-live="polite">{list.missingRow}</p>
  <div class="toolbar">
    <input class="ctl" aria-label="Find who opened it" placeholder="Find who opened it" bind:value={search.query} />
    {#if filter.dungeon}<button class="ctl chip" aria-label="Remove dungeon filter" title={filter.dungeon} onclick={() => apply({ ...filter, dungeon: null, cursor: null })}>{filter.dungeon} ×</button>{/if}
    <button class="ctl" aria-pressed={filter.opener_local === true} onclick={() => apply({ ...filter, opener_local: filter.opener_local === true ? null : true, cursor: null })}>Opened by me</button>
    <span class="hint">Click a row to open its run</span>
  </div>
  {#if catalogMessage}<p role="status">{catalogMessage}</p>{/if}
  {#if prefsMessage}<p role="status">{prefsMessage}</p>{/if}
  {#if validation}<p role="alert">{validation}</p>{/if}
  {#if history.phase === 'error'}<ErrorBar message="Couldn't load key pops. Your filters are kept." onRetry={() => controller?.retry()} />{/if}
  <div class="layout"><section class="list">
    {#if list.hasNew}<NewDataPill count={pendingCount} noun="key pops" showCount={!filter.range && !filter.dungeon && !filter.query && !filter.opener_local} onClick={list.showNew} />{/if}
    {#if !client}<div class="empty"><h2>Key pops unavailable</h2></div>{:else}
      {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<p>{history.message}</p>{/if}
      {#if history.phase === 'busy' && history.retryAvailable}<p>Event history busy</p>{:else if !page && history.phase !== 'error'}<p>Loading key pops</p>{/if}
      {#if page}
        {#if !page.rows.length && history.phase !== 'error' && history.phase !== 'busy'}
          <div class="empty"><h2>{filter.range ? 'No key pops in this range' : filter.query || filter.dungeon || filter.opener_local ? 'No key pops match your filters' : 'No key pops'}</h2>
            {#if filter.range}<button class="ctl" onclick={() => apply({ ...filter, range: null, cursor: null })}>Show all time</button>{:else if filter.query || filter.dungeon || filter.opener_local}<button class="ctl" onclick={() => apply({ ...filter, query: '', dungeon: null, opener_local: null, cursor: null })}>Clear filters</button>{:else}<p>Portals will appear when Maxed sees them open in-game.</p>{/if}</div>
        {:else if page.rows.length}<EventTable rows={page.rows} selectedId={selected?.id ?? null} onSelect={select} client={activeClient} mode="keys" enteredRuns={page.entered_runs ?? []} />{/if}
      {/if}
    {/if}
    <footer><span>Portals Maxed saw open. The key's owner is the player who opened it.</span><Pager atLatest={filter.cursor === null} hasOlder={!!page?.next} loading={list.paging} noun="key pops" onLatest={list.first} onNewer={list.back} onOlder={list.next} /></footer>
  </section><aside><DungeonRanking rows={rankingRows} muted={rankedMuted} filtered={ruleFiltered} selected={filter.dungeon} range={filter.range} onSelect={toggleDungeon} /></aside></div>
</div>
<style>
  .feed { overflow-anchor: none; overflow-wrap: anywhere; display: grid; gap: 10px; min-width: 0; }
  .toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; min-width: 0; } .toolbar input { flex: 1; min-width: 160px; max-width: 260px; }
  .layout { display: grid; grid-template-columns: minmax(0, 1fr) 300px; gap: 12px; align-items: start; min-width: 0; }
  .list { position: relative; min-width: 0; padding: 8px 6px 6px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-panel); }
  aside { display: grid; gap: 10px; min-width: 0; padding: 0; margin: 0; border: 0; background: none; }
  p { margin: 8px; color: var(--text-secondary); font-size: 12px; }
  .empty { padding: 20px 14px; margin: 6px; border: 1px dashed var(--border); border-radius: 6px; text-align: center; font-size: 12px; } .empty h2 { font-size: 13px; }
  footer { display: flex; align-items: center; flex-wrap: wrap; justify-content: space-between; gap: 8px; border-top: 1px solid var(--border); margin-top: 8px; padding: 8px 4px 0; color: var(--text-muted); font-size: 11px; }
  @media (max-width: 1100px) { .layout { grid-template-columns: minmax(0, 1fr); } }
  .key-kpis { margin-bottom: 12px; min-width: 0; } .key-kpis :global(.value) { overflow: hidden; text-overflow: ellipsis; } .key-kpis :global([data-kpi="most"] .value) { font-size: 20px; line-height: 1.38; }
  .hint { margin-left: auto; font-size: 11px; color: var(--text-muted); } .chip { max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
