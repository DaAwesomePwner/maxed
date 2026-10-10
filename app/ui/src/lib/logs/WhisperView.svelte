<script lang="ts">
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import { validLogsQuery } from './search.svelte';
  import { releaseLogsRows } from './list.svelte';
  import { getContext, tick, untrack } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { ChatFilter } from '../bindings/ChatFilter';
  import type { ChatCursor } from '../bindings/ChatCursor';
  import type { ChatRow } from '../bindings/ChatRow';
  import type { ChatSettings } from '../bindings/ChatSettings';
  import type { WhisperPartners } from '../bindings/WhisperPartners';
  import { defaultLogsPosition, emptyListState, normalizeListState, type ListState } from '../navigation/state';
  import { createRunHistory, type HistoryController, type HistoryQuery, type HistoryState, type HistoryValue } from '../runs/history';
  import HistoryStatus from '../runs/HistoryStatus.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { historyStatus } from '../status/registry';
  import LogsHeader from './LogsHeader.svelte';
  import ChatToolbar from './ChatToolbar.svelte';
  import ChatStorageOff from './ChatStorageOff.svelte';
  import PeopleList from './PeopleList.svelte';
  import WhisperThread from './WhisperThread.svelte';
  import ErrorBar from './ErrorBar.svelte';
  import NewDataPill from './NewDataPill.svelte';
  import { hideChatSender } from './chat-settings';
  import { serializeChatReads } from './chat-reader';
  import { appendNewest, newestRows, reachesLoadedRows } from './whisper-screen';

  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { client, partner = null, runId = null, pendingCount = 0, onHistoryApplied, streamId = null, scopeKey = '', required = null,
    store = null, savedState = emptyListState(), onStateChange, entryKey = '', onRowsReady, enabled = true }: {
    client?: Client; partner?: string | null; runId?: string | null; pendingCount?: number;
    onHistoryApplied?: (key: string, required: string | null) => void; streamId?: string | null; scopeKey?: string;
    required?: string | null; store?: StoreStatus | null; savedState?: ListState; onStateChange?: (patch: Partial<ListState>) => void;
    entryKey?: string; onRowsReady?: (key: string) => void; enabled?: boolean;
  } = $props();
  const restored = untrack(() => normalizeListState(savedState).logsPosition?.whisper ?? defaultLogsPosition().whisper);
  let query = $state(restored.query), showHidden = $state(restored.showHidden);
  let selected = $state(untrack(() => partner ?? restored.partner));
  let partners = $state.raw<WhisperPartners>({ rows: [], people: 0, lines: 0 });
  let totals = $state.raw<{ people: number; lines: number } | null>(null);
  let settings = $state.raw<ChatSettings | null>(null);
  let pages = $state.raw<ChatRow[][]>([]), next = $state.raw<ChatCursor | null>(null);
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: 'Loading saved whispers.', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let peopleController = $state.raw<HistoryController>();
  let request = $state.raw<HistoryQuery | null>(null);
  let appliedRequired = $state(untrack(() => required)), pendingRequired = $state<string | null>(null);
  let isBottom = $state(true), revision = $state(0), loading = $state(true), error = $state(''), missingRow = $state('');
  let thread = $state<WhisperThread>();
  let stage: 'people' | 'thread' = 'people', mode: 'reset' | 'refresh' | 'earlier' = 'reset';
  let resetThread = false;
  let anchor: ReturnType<WhisperThread['anchor']> = null;
  let handled: HistoryValue | null = null;
  let lastFilters = '', lastCycle = '', namespace = '';
  let lastRevision = 0;
  let excludedPartner: string | null = null;
  let observedPending = 0, matchingRequired: string | null = null;
  const person = $derived(partners.rows.find(row => row.name === selected) ?? null);
  const capped = $derived(pages.flat().length >= 500);
  const filter = $derived<ChatFilter>({ channel: 'whisper', sender: selected, query: threadQuery(query, selected), run_id: runId, show_hidden: showHidden, show_npc: false, cursor: null, limit: 100 });
  const subline = $derived(totals?.lines ? `${totals.lines.toLocaleString('en-US')} whispers with ${totals.people.toLocaleString('en-US')} people${settings ? ` · kept ${settings.retention_days} days` : ''}` : null);

  const activeClient = $derived(enabled ? client : undefined);
  const reader = $derived(activeClient ? serializeChatReads(activeClient) : undefined);
  function threadQuery(text: string, name: string | null) {
    return name?.toLowerCase().includes(text.toLowerCase()) ? '' : text;
  }
  function persist() {
    onStateChange?.({ logsPosition: { ...(normalizeListState(savedState).logsPosition ?? defaultLogsPosition()),
      whisper: { query, showHidden, partner: selected } } });
  }
  function removed() { missingRow = 'Row Removed'; onStateChange?.({ focusedRowId: null }); }
  function complete() { loading = false; onHistoryApplied?.(entryKey, appliedRequired); }
  function retain(rows: ChatRow[]) {
    pages = Array.from({ length: Math.ceil(rows.length / 100) }, (_, i) => rows.slice(i * 100, (i + 1) * 100));
  }
  $effect(() => {
    const owner = reader;
    if (!owner) return;
    let current = true;
    void owner.querySettings().then(value => { if (current) settings = value.chat; }).catch(() => {});
    const receive = (state: HistoryState) => untrack(() => {
      history = state;
      if (state.phase !== 'ready' || !state.value || state.value === handled) {
        if (state.phase === 'error' || state.phase === 'superseded' || state.phase === 'busy' && state.retryAvailable) loading = false;
        return;
      }
      handled = state.value;
      if (stage === 'people' && state.value.kind === 'whisper-partners') {
        partners = state.value.partners;
        if (!query && !showHidden) totals = { people: partners.people, lines: partners.lines };
        if (!selected || excludedPartner === selected) selected = partners.rows.find(row => row.name !== excludedPartner)?.name ?? null;
        excludedPartner = null;
        persist();
        if (!selected) { pages = []; next = null; complete(); return; }
        stage = 'thread'; resetThread = mode === 'reset';
        request = { kind: 'chat', filter: { ...filter, sender: selected, query: threadQuery(query, selected) } };
      } else if (stage === 'thread' && state.value.kind === 'chat') {
        const page = state.value.page;
        if (mode === 'earlier') {
          retain(newestRows([...pages.flat(), ...page.rows]).slice(0, 500)); next = page.next;
          void thread?.restore(anchor);
        } else if (mode === 'refresh' && pages.length && reachesLoadedRows(pages, page)) {
          const merged = appendNewest(pages, page.rows);
          retain(merged);
          void thread?.bottom();
        } else {
          pages = [page.rows]; next = page.next;
          void tick().then(() => thread?.bottom());
        }
        complete();
      }
    });
    const instance = createRunHistory(owner, receive, { cache: historyCache });
    const people = createRunHistory(owner, receive, { cache: historyCache });
    controller = instance; peopleController = people;
    return () => { current = false; instance.dispose(); people.dispose(); };
  });
  $effect(() => {
    const latest = required, bottom = isBottom, matching = pendingCount, scope = `${scopeKey}:${streamId}`;
    untrack(() => {
      if (matching > observedPending) matchingRequired = latest;
      if (matching === 0) matchingRequired = null;
      observedPending = matching;
      if (scope !== namespace) { namespace = scope; pages = []; appliedRequired = latest; pendingRequired = null; matchingRequired = null; }
      else if (bottom && (thread?.atBottom() ?? true)) {
        if (matchingRequired !== null && (appliedRequired === null || BigInt(matchingRequired) > BigInt(appliedRequired))) appliedRequired = matchingRequired;
        pendingRequired = null;
      }
      else if (latest !== appliedRequired) pendingRequired = latest;
    });
  });
  $effect(() => {
    const identity = JSON.stringify([query, showHidden, scopeKey, streamId, runId]);
    const cycle = JSON.stringify([identity, appliedRequired, revision]);
    untrack(() => {
      if (cycle === lastCycle) return;
      lastCycle = cycle;
      mode = identity === lastFilters ? 'refresh' : 'reset'; lastFilters = identity;
      if (mode === 'reset') { pages = []; next = null; }
      loading = true; stage = 'people'; handled = null;
      request = { kind: 'whisper-partners', filter: { query, show_hidden: showHidden, limit: 200 } };
      if (revision !== lastRevision) { lastRevision = revision; peopleController?.retry(); }
    });
  });
  $effect(() => {
    const read = request;
    const instance = read?.kind === 'whisper-partners' ? peopleController : controller;
    const input = read ? { query: read, scopeKey, streamId, required: untrack(() => appliedRequired), store } : null;
    if (enabled && instance && input) untrack(() => {
      const retry = read?.kind === 'chat' && resetThread;
      if (retry) resetThread = false;
      instance.update(input);
      // Different partner-name searches can resolve to the same thread query.
      // A reset cleared the rows, so a deduplicated ready emission is insufficient.
      if (retry) instance.retry();
    });
  });
  releaseLogsRows({
    ready: () => !client || !loading || history.phase === 'error', enabled: () => enabled,
    entryKey: () => entryKey, onReady: key => onRowsReady?.(key),
  });
  function apply(value: ChatFilter) {
    if (!validLogsQuery(value.query)) {
      error = 'Search must be at most 128 characters with no NUL.'; return;
    }
    error = ''; query = value.query; showHidden = value.show_hidden; persist();
  }
  function earlier() {
    if (loading || !next || capped) return;
    anchor = thread?.anchor() ?? null; mode = 'earlier'; loading = true;
    request = { kind: 'chat', filter: { ...filter, cursor: next } };
  }
  function showNew() { appliedRequired = required; pendingRequired = null; isBottom = true; }
  async function hide() {
    if (!client || !selected) return;
    try {
      settings = await hideChatSender(client, selected);
      const index = partners.rows.findIndex(row => row.name === selected);
      excludedPartner = selected;
      selected = partners.rows[index + 1]?.name ?? partners.rows[index - 1]?.name ?? null;
      pages = []; next = null; persist(); revision++;
    } catch (reason) { error = reason instanceof Error && reason.message.startsWith('Could not hide sender.') ? reason.message : 'Could not hide sender. Retry.'; }
  }
  async function enableStorage() {
    if (!client || !settings) return;
    try { const updated = { ...settings, store: { ...settings.store, whisper: true } }; await client.updateChatSettings(updated); settings = updated; (stage === 'people' ? peopleController : controller)?.retry(); }
    catch { error = 'Could not update chat settings. Retry.'; }
  }
</script>

<LogsHeader view="chat" {subline}><a class="ctl ghost" href="#/settings">Chat settings</a></LogsHeader>
<BusyStatus message={client ? historyStatus(history, 'messages') : null} />
<div class="whispers">
  <HistoryStatus {history} />
  <p class="sr-only" role="status" aria-live="polite">{missingRow}</p>
  <ChatToolbar whisper channel="whisper" {runId} filter={{ ...filter, query }} rows={[]} partners={partners.rows} hiddenCount={pages.flat().filter(row => row.hidden).length} latestAt={null} onApply={apply} onOpen={() => {}} />
  {#if error}<p role="alert">{error}</p>{/if}
  {#if history.phase === 'error'}<ErrorBar message="Couldn't load whispers. Your filters are kept." onRetry={() => { loading = true; (stage === 'people' ? peopleController : controller)?.retry(); }} />{/if}
  {#if !client}<p>Chat unavailable</p>
  {:else if settings?.store.whisper === false}<ChatStorageOff channel="whisper" onEnable={enableStorage} />
  {:else if !partners.rows.length && !pages.flat().length && !loading && history.phase === 'ready'}<div class="empty">{#if query}<strong>No whispers match "{query}"</strong><p>Try another search.</p><button class="ctl" onclick={() => apply({ ...filter, query: '' })}>Clear search</button>{:else}<strong>No whispers yet</strong><p>Saved whispers will appear here when Maxed sees in-game chat.</p>{/if}</div>
  {:else}<div class="layout">
    <PeopleList rows={partners.rows} count={partners.people} selected={selected} />
    <div class="thread-wrap">
      {#if pendingRequired && pendingCount > 0}<NewDataPill count={pendingCount} noun="whispers" showCount={false} onClick={showNew} />{/if}
      {#if selected}<WhisperThread bind:this={thread} name={selected} {person} {pages} hasOlder={!!next} {loading} {capped} onEarlier={earlier} onHide={hide} onBottomChange={bottom => isBottom = bottom} onRemoved={removed} />
      {:else}<p>{loading ? 'Loading whispers' : 'Pick a person'}</p>{/if}
    </div>
  </div>{/if}
</div>
<style>
  .whispers { --ch-guild: #75dfa1; --ch-party: #8de4e5; --ch-whisper: #f2a6cf; min-width: 0; display: grid; gap: 10px; }
  .layout { display: grid; grid-template-columns: 300px minmax(0, 1fr); gap: 12px; height: max(360px, calc(100dvh - 210px)); min-width: 0; }
  .thread-wrap { position: relative; display: grid; min-width: 0; min-height: 0; }
  .empty { padding: 20px; border: 1px dashed var(--border); border-radius: 6px; font-size: 12px; } p { color: var(--text-secondary); font-size: 12px; }
  @media (max-width: 1100px) { .layout { grid-template-columns: 240px minmax(0, 1fr); } }
  @media (max-width: 900px) { .layout { grid-template-columns: 200px minmax(0, 1fr); } }
</style>
