<script lang="ts">
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { ListState } from '../lib/navigation/state';
  import type { Route } from '../lib/navigation/routes';
  import WhisperView from '../lib/logs/WhisperView.svelte';
  import ChatView from '../lib/logs/ChatView.svelte';
  import KeyPopsView from '../lib/logs/KeyPopsView.svelte';
  import EventsView from '../lib/logs/EventsView.svelte';
  import AlertsView from '../lib/alerts/AlertsView.svelte';
  let { pendingCount = 0, onHistoryApplied, route, client, preview = false, enabled, streamId, scopeKey, required, store, savedState, onStateChange, entryKey = '', onRowsReady }: {
    pendingCount?: number; onHistoryApplied?: (key: string, required: string | null) => void;
    route: Extract<Route, { kind: 'logs' }>; client?: Client; preview?: boolean; enabled?: boolean; streamId?: string | null;
    scopeKey?: string; required?: string | null; store?: StoreStatus | null; savedState?: ListState;
    onStateChange?: (patch: Partial<ListState>) => void; entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
</script>

{#if route.view === 'chat'}
  {#key `${entryKey}:${route.channel}:${route.runId ?? ''}:${route.seedSender ?? ''}:${route.partner ?? ''}`}
    {#if route.channel === 'whisper' && !route.runId}
      <WhisperView partner={route.partner ?? null} {pendingCount} {onHistoryApplied} runId={route.runId ?? null} {client} {enabled} {streamId} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />
    {:else}
    <ChatView {pendingCount} {onHistoryApplied} channel={route.channel} runId={route.runId ?? null} {client} {enabled} {streamId} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />
    {/if}
  {/key}
{:else if route.view === 'keys'}{#key entryKey}<KeyPopsView {pendingCount} {onHistoryApplied} {client} {enabled} {streamId} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />{/key}
{:else if route.view === 'events'}
  {#key `${entryKey}:${route.runId ?? ''}`}<EventsView {pendingCount} {onHistoryApplied} runId={route.runId ?? null} {client} {enabled} {streamId} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />{/key}
{:else}{#key entryKey}<AlertsView {pendingCount} {onHistoryApplied} {client} {enabled} {streamId} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />{/key}{/if}
