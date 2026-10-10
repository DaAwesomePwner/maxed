<script lang="ts">
  import { tick } from 'svelte';
  import { emptyListState, type ListState } from '../lib/navigation/state';
  import { productionFeatures, type Features, type FightRelationship, type Route } from '../lib/navigation/routes';
  import type { LiveSnapshot } from '../lib/bindings/LiveSnapshot';
  import type { CaptureStatus } from '../lib/bindings/CaptureStatus';
  import type { EventEnvelope } from '../lib/bindings/EventEnvelope';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { LiveState } from '../lib/bridge/live';
  import type { Client } from '../lib/bridge/client';
  import EmptyState from '../lib/components/EmptyState.svelte';
  import Home from './Home.svelte';
  import Runs from './Runs.svelte';
  import Fight from './Fight.svelte';
  import PlayerBreakdown from './PlayerBreakdown.svelte';
  import Stats from './Stats.svelte';
  import Logs from './Logs.svelte';
  import Loot from './Loot.svelte';
  import Alerts from './Alerts.svelte';
  import Settings from './Settings.svelte';
  import Setup from './Setup.svelte';
  import RunStory from './RunStory.svelte';
  import type { ScopeChoice } from '../lib/home/scope';
  let { pendingCount = 0, onHistoryApplied, route, snapshot, client, features = productionFeatures, preview = false, capture = null, liveState = null, event = null, historyReady = null,
    store = null, required = null, initial = false, syntheticClock = preview, clockReady = true, savedScope = null, onScopeChange, onRowsReady, onDemoChange, onClockRetry, entryKey = '', savedState = emptyListState(), scopeKey = '', onStateChange, onRelationship }: {
    pendingCount?: number; onHistoryApplied?: (key: string, required: string | null) => void;
    features?: Features; route: Route; snapshot: LiveSnapshot | null; client?: Client; preview?: boolean;
    capture?: CaptureStatus | null; liveState?: LiveState | null; event?: EventEnvelope | null; historyReady?: StoreStatus | null;
    store?: StoreStatus | null; required?: string | null; initial?: boolean; savedScope?: ScopeChoice | null;
    syntheticClock?: boolean; clockReady?: boolean;
    onScopeChange?: (choice: ScopeChoice) => void; onRowsReady?: (key: string) => void; entryKey?: string;
    onDemoChange?: (enabled: boolean) => Promise<void>;
    onClockRetry?: () => void;
    savedState?: ListState; scopeKey?: string; onStateChange?: (patch: Partial<ListState>) => void;
    onRelationship?: (relationship: FightRelationship) => void;
  } = $props();
  let settingsClient = $derived(client && onDemoChange ? { ...client, setDemo: onDemoChange } : client);
  $effect(() => {
    const key = entryKey, kind = route.kind;
    if (kind === 'home' || kind === 'runs' || kind === 'run' || kind === 'fight' || kind === 'player' || kind === 'alerts' || kind === 'loot' || kind === 'item' || kind === 'logs' || kind === 'stats') return;
    // Routes without history rows are ready as soon as they render.
    let current = true;
    void tick().then(() => { if (current) onRowsReady?.(key); });
    return () => { current = false; };
  });
</script>
{#if route.kind === 'home'}<Home statsEnabled={features.stats} {snapshot} {capture} {liveState} {event} {historyReady} {client} {syntheticClock} {clockReady} {onClockRetry} {store} {required} {initial} {savedScope} {onScopeChange} {entryKey} {onRowsReady} />
{:else if route.kind === 'run'}<RunStory {features} runId={route.id} point={route.point} {client} enabled={liveState?.phase !== 'connecting'} streamId={snapshot?.stream_id ?? null} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />
{:else if route.kind === 'runs'}<Runs {client} enabled={liveState?.phase !== 'connecting'} streamId={snapshot?.stream_id ?? null} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />
{:else if route.kind === 'fight'}<Fight {client} fightId={route.id} runId={route.runId} enabled={liveState?.phase !== 'connecting'} streamId={snapshot?.stream_id ?? null} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} {onRelationship} />
{:else if route.kind === 'player'}<PlayerBreakdown {client} fightId={route.fightId} objectId={route.objectId} runId={route.runId} enabled={liveState?.phase !== 'connecting'} streamId={snapshot?.stream_id ?? null} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} {onRelationship} />
{:else if route.kind === 'loot' || route.kind === 'item'}<Loot {route} {client} enabled={liveState?.phase !== 'connecting'} streamId={snapshot?.stream_id ?? null} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />
{:else if route.kind === 'logs'}<Logs {pendingCount} {onHistoryApplied} {route} {client} {preview} enabled={liveState?.phase !== 'connecting'} streamId={snapshot?.stream_id ?? null} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />
{:else if route.kind === 'stats'}<Stats {route} {client} enabled={liveState?.phase !== 'connecting'} streamId={snapshot?.stream_id ?? null} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />
{:else if route.kind === 'alerts'}<Alerts {onHistoryApplied} {client} {preview} enabled={liveState?.phase !== 'connecting'} streamId={snapshot?.stream_id ?? null} {scopeKey} {required} {store} {savedState} {onStateChange} {entryKey} {onRowsReady} />
{:else if route.kind === 'settings' || route.kind === 'setup'}
  {#if client}
    {#if route.kind === 'settings' && settingsClient}<Settings client={settingsClient} {preview} {features} enabled={liveState?.phase !== 'connecting'} streamId={snapshot?.stream_id ?? null} {store} />{:else}<Setup {client} {preview} />{/if}
  {:else}<EmptyState state="unavailable" heading="No Connection" />{/if}
{:else if route.kind === 'not-found'}
  <EmptyState state="error" heading="Page Not Found">
    {#snippet action()}<a href="#/">Return Home</a>{/snippet}
  </EmptyState>
{:else}
  <EmptyState state="unavailable" heading="No Details" />
{/if}
