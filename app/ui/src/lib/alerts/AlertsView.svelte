<script lang="ts">
  import { createLogsSearch, validLogsQuery } from '../logs/search.svelte';
  import { createLogsList } from '../logs/list.svelte';
  import { getContext, untrack } from 'svelte';
  import { readable } from 'svelte/store';
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { AlertFilter } from '../bindings/AlertFilter';
  import type { AlertCursor } from '../bindings/AlertCursor';
  import type { AlertSummary } from '../bindings/AlertSummary';
  import type { AlertPreferences } from '../bindings/AlertPreferences';
  import type { AlertKind } from '../bindings/AlertKind';
  import { defaultLogsPosition, emptyListState, normalizeListState, type ListState } from '../navigation/state';
  import { createAlertHistory, type AlertHistoryController, type AlertHistoryState } from './history';
  import { defaultAlertFilter, alertKindDisplayOrder } from './filters';
  import { alertKpis, tileFilter, tiles, kindLabel, type AlertTile } from './alerts-screen';
  import { DUNGEON_MUTE, type DungeonMute } from './dungeon-mute';
  import HistoryStatus from '../runs/HistoryStatus.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { historyStatus } from '../status/registry';
  import { HISTORY_CATCH_UP } from '../runs/policy';
  import AlertTable from './AlertTable.svelte';
  import AlertPanel from './AlertPanel.svelte';
  import LogsHeader from '../logs/LogsHeader.svelte';
  import RangeSwitch from '../loot/RangeSwitch.svelte';
  import Pager from '../logs/Pager.svelte';
  import NewDataPill from '../logs/NewDataPill.svelte';
  import ErrorBar from '../logs/ErrorBar.svelte';
  import { countLine } from '../logs/logs-format';
  import { createHomeVisuals, emptyHomeVisuals } from '../home/visuals';
  let { pendingCount = 0, onHistoryApplied, client, streamId = null, scopeKey = '', required = null, store = null, savedState = emptyListState(), onStateChange,
    entryKey = '', onRowsReady, enabled = true }: {
    client?: Client; streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    pendingCount?: number; onHistoryApplied?: (key: string, required: string | null) => void;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; enabled?: boolean;
  } = $props();
  const restored = untrack(() => normalizeListState(savedState));
  const saved = restored.alertsPosition;
  let filter = $state<AlertFilter>({ ...defaultAlertFilter(), ...saved?.filter, range: restored.logsPosition?.range ?? null });
  let tile = $state<AlertTile>(saved?.tile ?? (saved?.filter.kind === 'white_bag' ? 'white' : saved?.filter.kind === 'personal_best' ? 'pb' : saved?.filter.kind === 'key_popped' ? 'keys' : 'all'));
  let history = $state.raw<AlertHistoryState>({ phase: 'loading', page: null, message: '', retryAvailable: false });
  let readRequired: string | null = null, readIdentity: string | null = null;
  let readyIdentity = $state<string | null>(null);
  let controller = $state.raw<AlertHistoryController>();
  let region: HTMLDivElement;
  let page = $derived(history.page);
  let available = $derived(!client || page !== null || history.phase === 'error' || history.phase === 'superseded' || (history.phase === 'stale' && store?.state === 'failed') || (history.phase === 'busy' && history.retryAvailable));
  const list = createLogsList<AlertCursor>({
    previous: saved?.previous ?? [], scrollTop: restored.scrollTop,
    required: () => required, namespace: () => `${scopeKey}:${streamId}`, pendingCount: () => pendingCount,
    cursor: () => filter.cursor, setCursor: cursor => { filter = { ...filter, cursor }; }, next: () => page?.next,
    region: () => region, rows: () => page?.rows, persist: () => persist(),
    available: () => available, enabled: () => !client || enabled, entryKey: () => entryKey,
    onReady: key => onRowsReady?.(key), onStateChange: patch => onStateChange?.(patch),
    innerScroll: true, removedMessage: 'Alert removed. Heading focused.',
  });

  let requestIdentity = $derived(JSON.stringify([scopeKey, streamId, filter, list.appliedRequired]));
  let activeClient = $derived(enabled ? client : undefined);
  let reader = $derived(activeClient ? serialize(activeClient) : undefined);
  function serialize(owner: Client) {
    let tail: Promise<unknown> = Promise.resolve();
    function queue<T>(read: () => Promise<T>) { const result = tail.then(read); tail = result.catch(() => {}); return result; }
    return { listAlerts: (input: AlertFilter) => queue(() => owner.listAlerts(input)),
      getAlertSummary: (range: AlertFilter['range']) => queue(() => owner.getAlertSummary({ range })),
      querySettings: () => queue(() => owner.querySettings()), listCatalogDungeons: () => queue(() => owner.listCatalogDungeons()) };
  }
  let selectedId = $state(saved?.selectedId ?? null);
  let selected = $derived(page?.rows.find(row => row.id === selectedId) ?? page?.rows[0] ?? null);
  let summary = $state.raw<AlertSummary | null>(null), allTime = $state.raw<AlertSummary | null>(null);
  let summaryKey = $state<string | null>(null), allTimeRead = $state(false);
  const search = createLogsSearch({ query: () => filter.query, apply: query => apply({ ...filter, query, cursor: null }) });
  let validation = $state('');
  let rangeKey = $derived(JSON.stringify([scopeKey, streamId, filter.range, list.appliedRequired]));
  let prefs = $state.raw<AlertPreferences | null>(null), prefsRead = $state(false);
  let dungeons = $state<string[] | null>(null), prefsMessage = $state('');
  const mute = getContext<DungeonMute | undefined>(DUNGEON_MUTE);
  const mutePreferences = mute?.preferences ?? readable<AlertPreferences | null>(null);
  const muteStatus = mute?.status ?? readable('loading');
  const muteFeedback = mute?.state ?? readable({ busy: false });
  let preferences = $derived($mutePreferences ?? prefs);
  let mutedCount = $derived(!preferences ? -1 : preferences.key_pop_dungeons === null ? preferences.key_pop_excluded_dungeons.length : dungeons === null ? -1 : dungeons.filter(name => !preferences.key_pop_dungeons!.includes(name)).length);
  const muted = (dungeon: string) => !!preferences && (preferences.key_pop_dungeons !== null ? !preferences.key_pop_dungeons.includes(dungeon) : preferences.key_pop_excluded_dungeons.includes(dungeon));
  function toggleMute(dungeon: string) { if (muted(dungeon)) void mute?.unmute(dungeon); else void mute?.mute(dungeon); }

  let generation = 0, summaryRequested: string | null = null, allTimeRequested = false, prefsRequested = false;
  $effect(() => {
    const owner = reader, namespace = `${scopeKey}:${streamId}`;
    void owner; void namespace;
    generation++; summaryRequested = null; allTimeRequested = false; prefsRequested = false;
    summary = null; allTime = null; summaryKey = null; allTimeRead = false; prefs = null; prefsRead = false; dungeons = null; prefsMessage = '';
    return () => { generation++; };
  });
  // Wait for the page before queuing summary reads. Search/kind changes leave their range key intact.
  $effect(() => {
    const owner = reader, ready = history.phase === 'ready' && readyIdentity === requestIdentity, key = rangeKey, range = filter.range;
    if (!owner || !ready || summaryKey === key || summaryRequested === key) return;
    summaryRequested = key;
    const token = generation;
    const current = () => token === generation && key === rangeKey;
    void owner.getAlertSummary(range ? { ...range } : null).then(reply => {
      if (current()) {
        summary = reply.status === 'ready' ? reply.value : null; summaryKey = key;
        if (!range && !allTimeRead) { allTime = summary; allTimeRead = true; }
      }
    }).catch(() => { if (current()) { summary = null; summaryKey = key; } });
  });
  $effect(() => {
    const owner = reader, ready = history.phase === 'ready' && readyIdentity === requestIdentity && summaryKey === rangeKey;
    if (!owner || !ready || allTimeRead || allTimeRequested) return;
    allTimeRequested = true;
    const token = generation;
    void owner.getAlertSummary(null).then(reply => { if (token === generation) { allTime = reply.status === 'ready' ? reply.value : null; allTimeRead = true; } })
      .catch(() => { if (token === generation) allTimeRead = true; });
  });
  $effect(() => {
    const owner = reader, ready = allTimeRead;
    if (!owner || !ready || prefsRead || prefsRequested) return;
    prefsRequested = true;
    const token = generation;
    void owner.querySettings().then(async settings => {
      if (token !== generation) return;
      prefs = settings.alerts;
      if (prefs.key_pop_dungeons !== null) {
        const catalog = await owner.listCatalogDungeons();
        if (token !== generation) return;
        dungeons = catalog;
      }
      prefsRead = true;
    }).catch(() => { if (token === generation) { prefsMessage = 'Alert settings unavailable. Check Settings.'; prefsRead = true; } });
  });
  let visuals = $state.raw(emptyHomeVisuals()), visualController = $state.raw<ReturnType<typeof createHomeVisuals>>();
  $effect(() => { const owner = activeClient; if (!owner) return; const instance = createHomeVisuals(owner, value => { visuals = value; }); visualController = instance; return () => instance.dispose(); });
  $effect(() => { const detail = selected?.details, instance = visualController; const types = detail?.type === 'white_bag' ? [detail.bag_type, ...(detail.source_enemy === null ? [] : [detail.source_enemy])] : detail?.type === 'personal_best' ? [detail.boss_type] : []; untrack(() => instance?.update(types)); });
  function select(id: string) { selectedId = id; persist(false); }
  function chooseTile(next: AlertTile) { tile = next; apply({ ...filter, ...tileFilter(next), ...(next === 'keys' ? { exclude_kinds: [] } : {}), cursor: null }); }
  function chooseKind(value: string) { if (value === 'messages') { chooseTile('messages'); return; } const kind = (value || null) as AlertKind | null; tile = kind === 'white_bag' ? 'white' : kind === 'personal_best' ? 'pb' : kind === 'key_popped' ? 'keys' : 'all'; apply({ ...filter, kind, kinds: [], ...(kind === 'key_popped' ? { exclude_kinds: [] } : {}), cursor: null }); }
  $effect(() => {
    const owner = reader;
    if (!owner) { controller = undefined; readyIdentity = null; history = { phase: 'loading', page: null, message: '', retryAvailable: false }; return; }
    const instance = createAlertHistory(owner, next => {
      history = next;
      if (next.phase === 'ready') { readyIdentity = readIdentity; onHistoryApplied?.(entryKey, readRequired); }
      if (['ready', 'error', 'superseded'].includes(next.phase) || (next.phase === 'busy' && next.retryAvailable) || next.message.startsWith('Saving failed.')) list.paging = false;
    });
    controller = instance;
    return () => instance.dispose();
  });
  $effect(() => {
    const instance = controller;
    const input = { filter: $state.snapshot(filter), scopeKey, streamId, required: list.appliedRequired, store };
    const identity = requestIdentity;
    if (enabled) untrack(() => { readRequired = input.required; readIdentity = identity; instance?.update(input); });
  });
  function persist(resetScroll = true) {
    const position = normalizeListState(savedState).logsPosition ?? restored.logsPosition;
    list.persist({ logsPosition: {
      ...(position ?? defaultLogsPosition()),
      range: filter.range,
    }, alertsPosition: { filter: $state.snapshot(filter), previous: $state.snapshot(list.previous), tile, selectedId: selectedId ?? selected?.id ?? null } }, resetScroll);
  }
  function apply(next: AlertFilter) {
    if (next.exclude_kinds?.includes('key_popped') && next.kind === 'key_popped') {
      tile = 'all'; next = { ...next, kind: null, kinds: [] };
    }
    const text = next.query.trim();
    if (!validLogsQuery(text)) { validation = 'Invalid search.'; return; }
    validation = '';
    list.apply(next, $state.snapshot(filter), () => {
      search.accept(next.query);
      selectedId = null; filter = { ...next, range: next.range ?? null, query: text };
    }, () => controller?.retry());
  }
</script>
<LogsHeader view="alerts" subline={countLine(allTime?.total ?? null, 'alerts', allTime?.first_at ?? null)}><RangeSwitch range={filter.range} label="Logs date range" onChange={range => apply({ ...filter, range, cursor: null })} /></LogsHeader>
<div class="kpis" aria-label="Alert kinds">{#each alertKpis(summaryKey === rangeKey ? summary : null, filter.range, mutedCount, new Date()) as cell, index}
  <button class="ctl ghost" class:chosen={tile === cell.key} aria-pressed={tile === cell.key} onclick={() => chooseTile(tiles[index])}><span class="label">{cell.label}</span><strong class:gold={cell.tone === 'fame'}>{cell.value}</strong><span class="sub" title={cell.sub ?? undefined}>{cell.sub ?? '\u00a0'}</span></button>
{/each}</div>
<BusyStatus message={client && enabled ? historyStatus(history, 'alerts') : null} />
<div class="feed" bind:this={region}>
  <HistoryStatus history={{ ...history, value: null }} />
  <p class="sr-only" role="status" aria-live="polite">{list.missingRow}</p>
  <div class="toolbar"><input class="ctl" type="search" aria-label="Search alerts" placeholder="Search alerts" bind:value={search.query} />
    <select class="ctl" aria-label="Kind" value={tile === 'messages' ? 'messages' : filter.kind ?? ''} onchange={event => chooseKind(event.currentTarget.value)}><option value="">Any kind</option><option value="messages">Messages</option>{#each alertKindDisplayOrder as kind}<option value={kind}>{kindLabel[kind]}</option>{/each}</select>
    <button class="ctl" aria-pressed={filter.exclude_kinds?.includes('key_popped') ?? false} onclick={() => apply({ ...filter, exclude_kinds: filter.exclude_kinds?.includes('key_popped') ? [] : ['key_popped'], cursor: null })}>Hide key pops</button><a class="ctl ghost settings" href="#/settings">Alert settings</a>
  </div>
  {#if validation}<p role="alert">{validation}</p>{/if}{#if prefsMessage}<p role="status">{prefsMessage}</p>{/if}
  {#if history.phase === 'error' || history.phase === 'busy' && history.retryAvailable || history.message.startsWith('Saving failed.')}<ErrorBar message="Couldn't load alerts. Your filters are kept." onRetry={() => controller?.retry()} />{/if}
  <div class="layout"><section class="list" aria-label="Alert log">
    {#if list.hasNew}<NewDataPill count={pendingCount} noun="alerts" showCount={false} onClick={list.showNew} />{/if}
    {#if !client}<div class="empty"><h2>Alerts unavailable</h2></div>{:else}
      {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<p>{history.message}</p>{/if}
      {#if page}
        {#if !page.rows.length}<div class="empty"><h2>{filter.range ? 'No alerts in this range' : filter.query || filter.kind || filter.kinds?.length || filter.exclude_kinds?.length ? 'No alerts match your filters' : 'No alerts'}</h2>
          {#if filter.range}<button class="ctl" onclick={() => apply({ ...filter, range: null, cursor: null })}>Show all time</button>{:else if filter.query || filter.kind || filter.kinds?.length || filter.exclude_kinds?.length}<button class="ctl" onclick={() => { tile = 'all'; apply(defaultAlertFilter()); }}>Clear filters</button>{:else}<p>Alerts appear when Maxed observes them in-game.</p><a class="ctl ghost" href="#/settings">Alert settings</a>{/if}</div>
        {:else}<AlertTable rows={page.rows} selectedId={selected?.id ?? null} onSelect={select} {muted} onMute={toggleMute} canMute={!!mute && $muteStatus === 'ready' && !$muteFeedback.busy && mute.available()} />{/if}
      {:else if history.phase === 'loading'}<p>Loading alerts</p>{/if}
    {/if}
    <footer><span>Alerts are kept with your runs. Off and log-only alerts are listed too.</span><Pager atLatest={filter.cursor === null} hasOlder={!!page?.next} loading={list.paging} noun="alerts" onLatest={list.first} onNewer={list.back} onOlder={list.next} /></footer>
  </section><aside><AlertPanel alert={selected} {visuals} /></aside></div>
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
  .settings { margin-left: auto; }
  .kpis { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); background: var(--surface-panel); border: 1px solid var(--border); border-radius: 8px; margin-bottom: 12px; min-width: 0; }
  .kpis button { display: block; text-align: left; min-width: 0; padding: 9px 14px 10px; border-radius: 0; height: auto; } .kpis button + button { border-left: 1px solid var(--border); } .chosen { box-shadow: inset 0 -2px var(--gold); } .label { display: block; font-size: 11px; text-transform: uppercase; letter-spacing: .07em; color: var(--text-secondary); } strong { display: block; font-size: 24px; line-height: 1.15; color: var(--text-primary); font-variant-numeric: tabular-nums; } .gold { color: var(--gold); } .sub { display: block; font-size: 11px; color: var(--text-secondary); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  @media (max-width: 900px) { .kpis button { padding-inline: 8px; } }
</style>
