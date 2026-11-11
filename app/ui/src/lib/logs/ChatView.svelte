<script lang="ts">
  import { settingsHref } from '../navigation/routes';
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import { validLogsQuery } from './search.svelte';
  import { createLogsList } from './list.svelte';
  import { serializeChatReads } from './chat-reader';
  import ChatStorageOff from './ChatStorageOff.svelte';
  import { presentChat } from './spam';
  import { hideChatSender } from './chat-settings';
  import { validHiddenTerm } from './validation';
  import { getContext, untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { ChatFilter } from '../bindings/ChatFilter';
  import type { ChatCursor } from '../bindings/ChatCursor';
  import { defaultLogsPosition, emptyListState, normalizeListState, type ListState } from '../navigation/state';
  import { createRunHistory, type HistoryController, type HistoryState } from '../runs/history';
  import HistoryStatus from '../runs/HistoryStatus.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { historyStatus } from '../status/registry';
  import { HISTORY_CATCH_UP } from '../runs/policy';
  import { routeHref, type ChatTab } from '../navigation/routes';
  import ChatTable from './ChatTable.svelte';
  import ChatToolbar from './ChatToolbar.svelte';
  import SenderPanel from './SenderPanel.svelte';
  import HiddenCard from './HiddenCard.svelte';
  import LogsHeader from './LogsHeader.svelte';
  import RunScopeChip from './RunScopeChip.svelte';
  import Pager from './Pager.svelte';
  import NewDataPill from './NewDataPill.svelte';
  import ErrorBar from './ErrorBar.svelte';
  import { chatSubline } from './chat-screen';
  import type { ChatOverview } from '../bindings/ChatOverview';
  import type { ChatSenderSummary } from '../bindings/ChatSenderSummary';
  import type { ChatSettings } from '../bindings/ChatSettings';
  import type { WhisperPartner } from '../bindings/WhisperPartner';
  import type { ChatRow } from '../bindings/ChatRow';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { pendingCount = 0, onHistoryApplied, client, channel, runId = null, streamId = null, scopeKey = '', required = null, store = null, savedState = emptyListState(), onStateChange,
    entryKey = '', onRowsReady, enabled = true }: {
    channel: ChatTab; runId?: string | null; client?: Client; streamId?: string | null; scopeKey?: string; required?: string | null; store?: StoreStatus | null;
    pendingCount?: number; onHistoryApplied?: (key: string, required: string | null) => void;
    savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; enabled?: boolean;
  } = $props();
  const defaults = (): ChatFilter => ({ channel: channel === 'all' ? null : channel, sender: null, query: '',
    run_id: runId, show_npc: false, show_hidden: false, cursor: null, limit: 100 });
  const restored = untrack(() => normalizeListState(savedState));
  const savedChat = untrack(() => {
    const saved = restored.logsPosition?.chat;
    return saved?.filter.channel === defaults().channel && saved.filter.run_id === runId ? saved : null;
  });
  let filter = $state({ ...(savedChat?.filter ?? defaults()), limit: 100 });
  let validation = $state(''), hideError = $state('');
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading saved chat', retryAvailable: false });
  let readRequired: string | null = null;
  let controller = $state.raw<HistoryController>();
  let region: HTMLDivElement;
  let page = $derived(history.value?.kind === 'chat' ? history.value.page : null);
  let available = $derived(!client || page !== null || history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable));
  const list = createLogsList<ChatCursor>({
    previous: savedChat?.previous ?? [], scrollTop: restored.scrollTop,
    required: () => required, namespace: () => `${scopeKey}:${streamId}`, pendingCount: () => pendingCount,
    cursor: () => filter.cursor, setCursor: cursor => { filter = { ...filter, cursor }; }, next: () => page?.next,
    region: () => region, rows: () => page?.rows, persist: () => persist(),
    available: () => available, enabled: () => !client || enabled, entryKey: () => entryKey,
    onReady: key => onRowsReady?.(key), onStateChange: patch => onStateChange?.(patch),
  });

  let activeClient = $derived(enabled ? client : undefined);
  let reader = $derived(activeClient ? serializeChatReads(activeClient) : undefined);
  let selectedId = $state(savedChat?.selectedId ?? null);
  let selected = $state.raw<ChatRow | null>(null);
  $effect(() => {
    const row = page?.rows.find(row => row.message.id === selectedId);
    if (row) selected = row;
  });
  let selectedSender = $derived(selected?.message.sender ?? null);
  let settings = $state.raw<ChatSettings | null>(null);
  let spamSaving = $state(false);
  let shown = $derived(presentChat(page?.rows ?? [], settings?.filter_spam ?? true));
  async function toggleSpam() {
    if (!client || !settings || spamSaving) return;
    spamSaving = true; hideError = '';
    try {
      const latest = (await client.querySettings()).chat;
      const next = { ...latest, filter_spam: !latest.filter_spam };
      await client.updateChatSettings(next); settings = next;
    } catch { hideError = "Couldn't update spam filter. Retry."; }
    finally { spamSaving = false; }
  }
  let overview = $state.raw<ChatOverview | null>(null);
  let summary = $state.raw<ChatSenderSummary | null>(null);
  let partners = $state.raw<WhisperPartner[]>([]);
  let knownRows = $state.raw<ChatRow[]>([]);
  let firstReady = $state(false), pickerOpened = $state(false);
  let storageChannel = $derived(channel === 'all' ? null : channel);
  let storageOff = $derived(storageChannel !== null && settings?.store[storageChannel] === false);
  $effect(() => {
    const owner = reader;
    let current = true;
    if (owner) void owner.querySettings().then(value => { if (current) settings = value.chat; }).catch(() => {});
    return () => { current = false; };
  });
  $effect(() => {
    if (history.phase !== 'ready' || !page) return;
    const rows = page.rows;
    untrack(() => {
      firstReady = true;
      const ids = new Set(knownRows.map(row => row.message.id));
      knownRows = [...knownRows, ...rows.filter(row => !ids.has(row.message.id))].slice(-10000);
    });
  });
  // Supplementary history reads start only after the first page has completed.
  // Their dependencies deliberately exclude filters and the live checkpoint.
  $effect(() => {
    const owner = reader, ready = firstReady;
    if (!owner || !ready) return;
    const instance = createRunHistory(owner, next => {
      if (next.phase === 'ready' && next.value?.kind === 'chat-overview') overview = next.value.overview;
    }, { cache: historyCache });
    untrack(() => instance.update({ query: { kind: 'chat-overview', request: { run_id: runId } }, scopeKey, streamId, required: null, store }));
    return () => instance.dispose();
  });
  $effect(() => {
    const owner = reader, name = selectedSender, ready = firstReady;
    if (!owner || !name || !ready) return;
    const instance = createRunHistory(owner, next => {
      if (next.phase === 'ready' && next.value?.kind === 'chat-sender') summary = next.value.sender;
      if (next.phase === 'error' || next.phase === 'superseded' || (next.phase === 'busy' && next.retryAvailable)) summary = null;
    }, { cache: historyCache });
    const timer = setTimeout(() => untrack(() => instance.update({ query: { kind: 'chat-sender', request: { name } }, scopeKey, streamId, required: null, store })), 150);
    return () => { clearTimeout(timer); instance.dispose(); };
  });
  $effect(() => {
    const owner = reader, opened = pickerOpened, ready = firstReady;
    if (!owner || !opened || !ready) return;
    const instance = createRunHistory(owner, next => {
      if (next.phase === 'ready' && next.value?.kind === 'whisper-partners') partners = next.value.partners.rows;
    }, { cache: historyCache });
    untrack(() => instance.update({ query: { kind: 'whisper-partners', filter: { query: '', show_hidden: true, limit: 200 } }, scopeKey, streamId, required: null, store }));
    return () => instance.dispose();
  });
  function only(sender: string) { apply({ ...filter, sender, cursor: null }); }
  function select(id: string) { selectedId = id; selected = page?.rows.find(row => row.message.id === id) ?? selected; persist(false); }
  async function enableStorage() {
    if (!client || !settings || !storageChannel) return;
    try {
      const next = { ...settings, store: { ...settings.store, [storageChannel]: true } };
      await client.updateChatSettings(next); settings = next; controller?.retry();
    } catch { hideError = "Couldn't update chat settings. Retry."; }
  }
  $effect(() => {
    const owner = reader;
    if (!owner) {
      controller = undefined;
      history = { phase: 'loading', value: null, message: 'Loading saved chat', retryAvailable: false };
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
    const input = { query: { kind: 'chat' as const, filter: $state.snapshot(filter) }, scopeKey, streamId, required: list.appliedRequired, store };
    if (enabled) untrack(() => { readRequired = input.required; instance?.update(input); });
  });
  function persist(resetScroll = true) {
    const position = normalizeListState(savedState).logsPosition ?? restored.logsPosition;
    list.persist({ logsPosition: {
      ...(position ?? defaultLogsPosition()),
      chat: { filter: $state.snapshot(filter), previous: $state.snapshot(list.previous), selectedId: selectedId ?? selected?.message.id ?? null },
    } }, resetScroll);
  }
  function apply(next: ChatFilter) {
    if (!validLogsQuery(next.query)) {
      validation = 'Search must be at most 128 characters with no NUL.'; return;
    }
    if (next.sender !== null && !validHiddenTerm(next.sender, 32)) {
      validation = 'Sender must be at most 32 UTF-8 bytes with no control characters.'; return;
    }
    validation = '';
    list.apply(next, $state.snapshot(filter), () => { filter = next; }, () => controller?.retry());
  }

  async function hideSender(sender: string) {
    hideError = '';
    if (!validHiddenTerm(sender, 32)) {
      hideError = "Couldn't hide sender. Use 1–32 UTF-8 bytes with no control characters."; return;
    }
    try {
      if (!client) return;
      updateSettings(await hideChatSender(client, sender));
      controller?.retry();
    } catch { hideError = "Couldn't hide sender. Retry."; }
  }
  function updateSettings(next: ChatSettings) { settings = next; }
</script>

<LogsHeader view="chat" subline={chatSubline(overview, settings?.retention_days ?? null)}><a class="ctl ghost" href={settingsHref('data')}>Chat settings</a></LogsHeader>
<BusyStatus message={client ? historyStatus(history, 'messages') : null} />
<div class="feed" bind:this={region}>
  <HistoryStatus {history} />
  <p class="sr-only" role="status" aria-live="polite">{list.missingRow}</p>
  <ChatToolbar {channel} {runId} {filter} rows={knownRows} {partners} hiddenCount={page?.rows.filter(row => row.hidden).length ?? 0} latestAt={page?.rows[0]?.message.at ?? null} onApply={apply} onOpen={() => pickerOpened = true} />
  <div class="spam-controls"><label><input type="checkbox" checked={settings?.filter_spam ?? true} disabled={!settings || spamSaving} onchange={event => { event.currentTarget.checked = settings?.filter_spam ?? true; void toggleSpam(); }} /> Filter spam</label>
    {#if shown.hidden}<span role="status" title="Spam ads and repeated public messages hidden on this page. Turn off Filter spam to show them.">{shown.hidden} hidden</span>{/if}
  </div>
  {#if runId && client && firstReady}<RunScopeChip {runId} client={reader ?? client} removeHref={routeHref({ kind: 'logs', view: 'chat', channel })} links={[{ label: 'Events in this run ›', href: routeHref({ kind: 'logs', view: 'events', channel: 'all', runId }) }, { label: 'Run summary ›', href: routeHref({ kind: 'run', id: runId }) }]} />{/if}
  {#if validation}<p role="alert">{validation}</p>{/if}
  {#if hideError}<p role="alert">{hideError}</p>{/if}
  {#if history.phase === 'error'}<ErrorBar message="Couldn't load chat. Your filters are kept." onRetry={() => controller?.retry()} />{/if}
  <div class="layout">
    <section class="list">
      {#if list.hasNew}<NewDataPill count={pendingCount} noun="lines" showCount={!filter.sender && !filter.query && settings !== null && !settings.filter_spam && !settings.hidden_senders.length && !settings.hidden_keywords.length} onClick={list.showNew} />{/if}
      {#if !client}<p>Chat unavailable</p>
      {:else if storageOff}<ChatStorageOff {channel} onEnable={enableStorage} />
      {:else}
        {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<p>{history.message}</p>{/if}
        {#if history.phase === 'busy' && history.retryAvailable}<p>Chat history busy</p>
        {:else if !page && history.phase !== 'error'}<p>Loading chat</p>{/if}
        {#if page}
          {#if page.rows.length === 0 && history.phase !== 'error' && history.phase !== 'busy'}
            <div class="empty">{#if filter.query || filter.sender}<strong>No {channel === 'all' ? 'chat' : channel} lines match "{filter.query || filter.sender || ''}"</strong><p>{settings ? `Search covers the last ${settings.retention_days} days of ${channel === 'all' ? 'stored' : channel} chat.` : 'Try another search or channel.'}</p><button class="ctl" onclick={() => apply({ ...filter, query: '', sender: null, cursor: null })}>Clear search</button><!-- Search all channels only offers a different scope. -->{#if channel !== 'all'}<a class="ctl" href={routeHref({ kind: 'logs', view: 'chat', channel: 'all', runId: runId ?? undefined })}>Search all channels</a>{/if}
            {:else}<strong>No chat yet</strong><p>Saved lines will appear here when Maxed sees in-game chat.</p>{/if}</div>
          {:else if page.rows.length}<ChatTable rows={shown.rows} counts={shown.counts} selectedId={selectedId ? shown.representatives[selectedId] ?? null : null} onSelect={select} onOnly={only} onHide={hideSender} />{/if}
          {#if page.rows.length && !shown.rows.length}<p>All messages on this page are hidden by the spam filter.</p>{/if}
        {/if}
      {/if}
      <footer><span>In-game chat Maxed saw. Stored only on this PC.</span><Pager atLatest={filter.cursor === null} hasOlder={!!page?.next} loading={list.paging} noun="lines" onLatest={list.first} onNewer={list.back} onOlder={list.next} /></footer>
    </section>
    <aside><SenderPanel row={selected} {summary} onOnly={only} onHide={hideSender} /><HiddenCard {settings} hidden={filter.show_hidden && page ? page.rows.filter(row => row.hidden).length : null} /></aside>
  </div>
</div>
<style>
  .spam-controls { display: flex; align-items: center; gap: var(--space-3); font-size: var(--text-caption); color: var(--text-secondary); }
  .spam-controls label { display: flex; align-items: center; gap: var(--space-2); }
  .spam-controls input { width: 16px; height: 16px; margin: 0; accent-color: var(--text-primary); }
  .spam-controls span { color: var(--text-muted); font-size: var(--text-meta); }
  .feed { overflow-anchor: none; overflow-wrap: anywhere; display: grid; gap: var(--space-3); min-width: 0; }
  .layout { display: grid; grid-template-columns: minmax(0, 1fr) 300px; gap: var(--space-3); align-items: start; min-width: 0; }
  .list { position: relative; min-width: 0; padding: var(--space-4); border: 1px solid var(--hairline); border-radius: var(--radius-card); background: var(--surface-panel); }
  aside { display: grid; gap: var(--space-3); align-content: start; min-width: 0; padding: 0; margin: 0; border: 0; background: none; }
  p { margin: var(--space-2); color: var(--text-secondary); font-size: var(--text-caption); }
  .empty { padding: var(--space-5) var(--space-4); margin: var(--space-2); border: 1px dashed var(--hairline-strong); border-radius: var(--radius-control); text-align: center; font-size: var(--text-caption); } .empty strong { font-weight: var(--weight-medium); } .empty p { margin: var(--space-1) 0 var(--space-3); } .empty .ctl + .ctl { margin-left: var(--space-2); }
  footer { display: flex; align-items: center; flex-wrap: wrap; justify-content: space-between; gap: var(--space-2); border-top: 1px solid var(--hairline); margin-top: var(--space-2); padding-top: var(--space-3); color: var(--text-muted); font-size: var(--text-meta); line-height: var(--leading-meta); }
  @media (max-width: 1100px) { .layout { grid-template-columns: minmax(0, 1fr); } aside { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
