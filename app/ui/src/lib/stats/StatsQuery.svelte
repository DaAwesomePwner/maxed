<script lang="ts">
  import { SHELL_CONNECTION, type ShellConnection } from '../shell/context';
  import { getContext, tick, untrack, type Snippet } from 'svelte';
  import type { Client } from '../bridge/client';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import { createRunHistory, type HistoryController, type HistoryQuery, type HistoryState } from '../runs/history';
  import HistoryStatus from '../runs/HistoryStatus.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import { historyStatus } from '../status/registry';
  import { HISTORY_CATCH_UP } from '../runs/policy';
  import EmptyState from '../components/EmptyState.svelte';
  const historyCache = getContext<ShellConnection | undefined>(SHELL_CONNECTION)?.historyCache;
  let { client, enabled = true, query, subject, streamId = null, scopeKey = '', required = null, store = null,
    entryKey = '', onRowsReady, children }: {
    client?: Client; enabled?: boolean; query: HistoryQuery; subject: string; streamId?: string | null; scopeKey?: string;
    required?: string | null; store?: StoreStatus | null; entryKey?: string; onRowsReady?: (key: string) => void;
    children: Snippet<[HistoryState]>;
  } = $props();
  let history = $state.raw<HistoryState>({ phase: 'loading', value: null, message: '', retryAvailable: false });
  let controller = $state.raw<HistoryController>();
  let activeClient = $derived(enabled ? client : undefined);
  let releasedEntry: string | undefined;
  $effect(() => {
    const owner = activeClient;
    if (!owner) { controller = undefined; return; }
    const instance = createRunHistory(owner, next => { history = next; }, { cache: historyCache });
    controller = instance;
    return () => instance.dispose();
  });
  // Stats tables refresh in place: a new Shell watermark (`required`) applies at once.
  $effect(() => {
    const instance = controller;
    const input = { query: $state.snapshot(query) as HistoryQuery, scopeKey, streamId, required, store };
    if (enabled) untrack(() => instance?.update(input));
  });
  let available = $derived(!client || history.value !== null || history.phase === 'error'
    || (history.phase === 'busy' && history.retryAvailable));
  $effect(() => {
    const ready = available, key = entryKey;
    if (!ready || (client && !enabled) || releasedEntry === key) return;
    let current = true;
    void tick().then(() => { if (current) { releasedEntry = key; onRowsReady?.(key); } });
    return () => { current = false; };
  });
</script>

<BusyStatus message={client ? historyStatus(history, subject) : null} />
<HistoryStatus {history} />
{#if !client}<EmptyState state="unavailable" heading="Stats unavailable" description="" />
{:else}
  {#if history.message && history.message !== HISTORY_CATCH_UP && !/^(Refreshing|Loading)\b/.test(history.message)}<p class="message">{history.message}</p>{/if}
  {#if history.phase === 'error' || (history.phase === 'busy' && history.retryAvailable)}
    <button type="button" onclick={() => controller?.retry()}>Retry</button>
  {/if}
  {#if history.phase === 'error'}<EmptyState state="error" heading="Stats error" description="" />
  {:else if history.phase === 'busy' && history.retryAvailable}<EmptyState state="unavailable" heading="Stats history busy" description="" />
  {:else if !history.value}<EmptyState state="loading" heading={`Loading ${subject}`} description="" />
  {:else}{@render children(history)}{/if}
{/if}

<style>
  .message { margin: 0; color: var(--text-secondary); }
  button { justify-self: start; font: inherit; color: var(--text-primary); min-height: var(--target-min); border-radius: var(--radius-control); background: var(--surface-raised); border: 1px solid var(--border-strong); padding: var(--space-2) var(--space-3); cursor: pointer; }
</style>
