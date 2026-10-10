<script lang="ts">
  import { untrack } from 'svelte';
  import type { Client } from '../lib/bridge/client';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { Route } from '../lib/navigation/routes';
  import { defaultStatsPosition, emptyListState, normalizeListState, type ListState, type StatsPosition } from '../lib/navigation/state';
  import SessionsView from '../lib/stats/SessionsView.svelte';
  import SessionDetail from '../lib/stats/SessionDetail.svelte';
  import DungeonsView from '../lib/stats/DungeonsView.svelte';
  import MapsView from '../lib/stats/MapsView.svelte';
  let { route, client, enabled, streamId, scopeKey, required, store, savedState = emptyListState(), onStateChange,
    entryKey = '', onRowsReady }: {
    route: Extract<Route, { kind: 'stats' }>; client?: Client; enabled?: boolean; streamId?: string | null; scopeKey?: string;
    required?: string | null; store?: StoreStatus | null; savedState?: ListState;
    onStateChange?: (patch: Partial<ListState>) => void; entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
  let position = $state<StatsPosition>(untrack(() => normalizeListState(savedState).statsPosition ?? defaultStatsPosition()));
  function change(patch: Partial<StatsPosition>) {
    position = { ...position, ...patch };
    onStateChange?.({ statsPosition: $state.snapshot(position) as StatsPosition });
  }
</script>

<nav aria-label="Stats views">
  <a href="#/stats" aria-current={route.view === 'maps' ? 'page' : undefined}>Maps</a>
  <a href="#/stats/sessions" aria-current={route.view === 'sessions' ? 'page' : undefined}>Sessions</a>
  <a href="#/stats/dungeons" aria-current={route.view === 'dungeons' ? 'page' : undefined}>Dungeons</a>
</nav>
{#if route.view === 'sessions' && route.sessionId}
  {#key route.sessionId}<SessionDetail id={route.sessionId} {client} {enabled} {position} onChange={change} {streamId} {scopeKey} {required} {store} {entryKey} {onRowsReady} />{/key}
{:else if route.view === 'sessions'}<SessionsView {client} {enabled} {position} onChange={change} {streamId} {scopeKey} {required} {store} {entryKey} {onRowsReady} />
{:else if route.view === 'dungeons'}<DungeonsView {client} {enabled} {position} onChange={change} {streamId} {scopeKey} {required} {store} {entryKey} {onRowsReady} />
{:else}<MapsView {client} {enabled} {position} onChange={change} {streamId} {scopeKey} {required} {store} {entryKey} {onRowsReady} />{/if}

<style>
  nav { display: flex; flex-wrap: wrap; gap: var(--space-2); margin-bottom: var(--space-3); }
  a { display: flex; align-items: center; min-height: var(--target-min); padding: var(--space-1) var(--space-3); border-radius: var(--radius-control); color: var(--text-secondary); text-decoration: none; }
  a:hover { background: var(--surface-hover); }
  a[aria-current] { background: var(--surface-raised); color: var(--violet-text); }
</style>
