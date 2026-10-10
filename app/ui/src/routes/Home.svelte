<script lang="ts">
  import { getContext, onDestroy, onMount, tick, untrack } from 'svelte';
  import { readable } from 'svelte/store';
  import { SHELL_CONNECTION, type ShellConnection } from '../lib/shell/context';
  import { emptyHomeLiveWindow } from '../lib/home/v2/live';
  import RealmHome from '../lib/home/RealmHome.svelte';
  import type { CaptureStatus } from '../lib/bindings/CaptureStatus';
  import type { EventEnvelope } from '../lib/bindings/EventEnvelope';
  import type { LiveSnapshot } from '../lib/bindings/LiveSnapshot';
  import type { StoreStatus } from '../lib/bindings/StoreStatus';
  import type { TimeRange } from '../lib/bindings/TimeRange';
  import type { LiveState } from '../lib/bridge/live';
  import type { Client } from '../lib/bridge/client';
  import EmptyState from '../lib/components/EmptyState.svelte';
  import DebugNote from '../lib/debug/DebugNote.svelte';
  import DungeonHome from '../lib/home/DungeonHome.svelte';
  import type { DungeonBest } from '../lib/bindings/DungeonBest';
  import { createDungeonBests, createHomeReads, pbBaseline, type PbBaseline } from '../lib/home/v3/dungeon-bests';
  import { initialHomeMode, createHomeObservation } from '../lib/home/mode';
  import { homeScope, localDayRange, type ScopeChoice } from '../lib/home/scope';
  import type { HomeHistoryState } from '../lib/home/summary';
  import NexusHome from '../lib/home/NexusHome.svelte';
  import { createNexusHistory, type LootHistoryState } from '../lib/home/v3/nexus-history';
  import type { ScopeLootPage } from '../lib/bindings/ScopeLootPage';
  import { createDungeonPortal, createHomeVisuals, emptyHomeVisuals, homeVisualTypes, type HomeVisuals } from '../lib/home/visuals';

  let { snapshot, statsEnabled = false, capture = null, liveState = null, event = null, historyReady = null,
    client, store = null, required = null, initial = false, syntheticClock = false, clockReady = true, savedScope = null, onScopeChange, onReady, onClockRetry, entryKey = '', onRowsReady }: {
    snapshot: LiveSnapshot | null; statsEnabled?: boolean; capture?: CaptureStatus | null; liveState?: LiveState | null;
    event?: EventEnvelope | null; historyReady?: StoreStatus | null;
    client?: Client; store?: StoreStatus | null; required?: string | null; initial?: boolean; syntheticClock?: boolean; clockReady?: boolean;
    savedScope?: ScopeChoice | null; onScopeChange?: (choice: ScopeChoice) => void; onReady?: () => void;
    onClockRetry?: () => void;
    entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  const homeWindow = connection?.homeWindow ?? readable(emptyHomeLiveWindow());
  let mode = $state(initialHomeMode());
  let captureState = $derived((capture ?? snapshot?.capture)?.state);
  let displayed = $state.raw<LiveSnapshot | null>(null);
  const observe = createHomeObservation();
  let choice = $state<ScopeChoice>(untrack(() => savedScope ?? 'session'));
  const currentDay = (): TimeRange | null => { try { return localDayRange(new Date()); } catch { return null; } };
  let day = $state.raw<TimeRange | null>(currentDay());
  let history = $state.raw<HomeHistoryState>({ phase: 'unavailable', summary: null, message: 'Saved history is unavailable.', retryAvailable: true });
  let controller = $state.raw<ReturnType<typeof createNexusHistory> | null>(null);
  let scopeLoot = $state.raw<ScopeLootPage | null>(null);
  let lootPage = $state(1);
  let lootStatus = $state.raw<LootHistoryState>({ phase: 'unavailable', value: null, message: '', retryAvailable: false });

  let receivedHistory = $state(false);
  let visuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals> | null>(null);
  let visualOwner: Client | undefined;
  const visualClient = $derived(client);
  const scopedWindow = $derived(displayed && mode.mode !== 'nexus'
    && $homeWindow.streamId === displayed.stream_id
    && $homeWindow.runId === (mode.mode === 'dungeon' ? displayed.run?.id : null)
    ? $homeWindow : null);
  let portalType = $state<number | null>(null);
  let portalController: ReturnType<typeof createDungeonPortal> | null = null;
  const portalDungeon = $derived(mode.mode === 'dungeon' ? displayed?.run?.dungeon ?? null : null);
  const visualKey = $derived([...homeVisualTypes(mode.mode !== 'nexus' ? displayed : null,
    mode.mode === 'nexus' ? history.summary : null, scopedWindow),
  ...(mode.mode === 'nexus' ? scopeLoot?.items.map(item => item.item_type) ?? [] : []),
  ...(portalDungeon !== null && portalType !== null ? [portalType] : [])].join(','));
  $effect(() => {
    const owner = visualClient;
    untrack(() => {
      // Object props can invalidate while retaining the same client. Teardown
      // belongs to an actual owner change or unmount, never a snapshot update.
      if (owner === visualOwner) return;
      visualController?.dispose();
      visualOwner = owner;
      visuals = emptyHomeVisuals();
      visualController = owner ? createHomeVisuals(owner, next => { visuals = next; }) : null;
      portalController?.dispose();
      portalType = null;
      portalController = owner ? createDungeonPortal(owner, next => { portalType = next; }) : null;
      portalController?.update(portalDungeon);
    });
  });
  onDestroy(() => { visualController?.dispose(); portalController?.dispose(); });
  $effect(() => {
    const dungeon = portalDungeon;
    untrack(() => portalController?.update(dungeon));
  });
  $effect(() => {
    const instance = visualController, key = visualKey;
    untrack(() => instance?.update(key === '' ? [] : key.split(',').map(Number)));
  });
  let dungeonBests = $state.raw<DungeonBest[] | null>(null);
  let bestsController = $state.raw<ReturnType<typeof createDungeonBests> | null>(null);
  let homeReads = $state.raw<ReturnType<typeof createHomeReads> | null>(null);
  let historyOwner: Client | undefined;
  $effect(() => {
    const owner = client;
    untrack(() => {
      if (owner === historyOwner) return;
      bestsController?.dispose(); homeReads?.dispose();
      historyOwner = owner;
      dungeonBests = null;
      homeReads = owner ? createHomeReads(owner) : null;
      bestsController = homeReads ? createDungeonBests(homeReads.client, rows => { dungeonBests = rows; }) : null;
    });
  });
  onDestroy(() => { bestsController?.dispose(); homeReads?.dispose(); });
  $effect(() => {
    const instance = bestsController;
    return connection?.onEvent?.(envelope => instance?.update(null, envelope, store ?? historyReady));
  });
  $effect(() => {
    // Standalone fixtures can supply one event; Shell supplies the ordered observer.
    const instance = bestsController, next = snapshot, observed = connection?.onEvent ? null : event, saved = store ?? historyReady;
    untrack(() => instance?.update(next, observed, saved));
  });
  let baseline: PbBaseline | null = null;
  const pbBests = $derived.by(() => (baseline = pbBaseline(baseline, displayed, dungeonBests)).rows);
  let region: HTMLDivElement;
  let announcement = $state('');
  let lastRegionFocus: HTMLElement | null = null;
  function rememberFocus(event: FocusEvent) {
    if (event.target instanceof HTMLElement) lastRegionFocus = event.target;
  }
  function releaseFocus(event: FocusEvent) {
    if (event.relatedTarget instanceof Node && !region.contains(event.relatedTarget)) lastRegionFocus = null;
  }
  let previousMode: string | null = null;
  let previousStale = false;
  let scope = $derived.by(() => {
    if (choice === 'today' && !clockReady) return null;
    if (choice === 'today' && !syntheticClock) return day ? { today: { range: day } } : null;
    let now = new Date();
    if (syntheticClock) {
      try { now = snapshot && /^\d+$/.test(snapshot.at) ? new Date(Number(BigInt(snapshot.at) / 1000n)) : new Date(NaN); }
      catch { now = new Date(NaN); }
    }
    return homeScope(choice, snapshot, now);
  });
  let rowsAvailable = $derived(mode.mode !== 'nexus' || !client || (receivedHistory
    && (['ready', 'error', 'unavailable'].includes(history.phase) || (history.phase === 'busy' && history.retryAvailable) || (history.phase === 'stale' && history.summary !== null))));
  let midnight = $derived(choice === 'today' && !syntheticClock && clockReady && mode.mode === 'nexus' ? day?.end ?? null : null);

  $effect(() => {
    const owner = homeReads?.client;
    if (!owner) return;
    receivedHistory = false;
    const instance = createNexusHistory(owner, next => {
      history = next.summary;
      scopeLoot = next.loot.value;
      lootStatus = next.loot;
      lootPage = next.page;
      receivedHistory = true;
    });
    controller = instance;
    return () => { instance.dispose(); };
  });
  $effect(() => {
    const instance = controller;
    const enteringDungeon = snapshot?.area_kind === 'dungeon' && snapshot.run?.ended === null
      && (capture ?? snapshot.capture).state === 'Live' && !['resyncing', 'failed'].includes(liveState?.phase ?? 'live');
    const enteringRealm = snapshot?.area_kind === 'realm' && snapshot.session?.ended == null
      && (capture ?? snapshot.capture).state === 'Live' && !['resyncing', 'failed'].includes(liveState?.phase ?? 'live');
    const input = { mode: enteringDungeon ? 'dungeon' as const : enteringRealm ? 'realm' as const : mode.mode, scope: liveState?.phase === 'connecting' ? null : scope,
      streamId: snapshot?.stream_id ?? mode.streamId, required, initial, store: store ?? historyReady };
    untrack(() => instance?.update(input));
  });
  $effect(() => {
    const ready = rowsAvailable, key = entryKey;
    if (!ready) return;
    let current = true;
    void tick().then(() => { if (current) { onReady?.(); onRowsReady?.(key); } });
    return () => { current = false; };
  });
  function refreshDay(): boolean {
    if (choice !== 'today' || syntheticClock) return false;
    const next = currentDay();
    if (next?.start === day?.start && next?.end === day?.end) return false;
    day = next;
    return true;
  }
  $effect(() => {
    const at = snapshot?.at, stream = snapshot?.stream_id, selected = choice, synthetic = syntheticClock;
    untrack(() => { if (at && stream && selected === 'today' && !synthetic) refreshDay(); });
  });
  $effect(() => {
    const end = midnight;
    if (end === null) return;
    let timer: ReturnType<typeof setTimeout>;
    const schedule = () => {
      const delay = Math.max(1, Math.min(2147483647, Number(BigInt(end) / 1000n) - Date.now()));
      timer = setTimeout(() => { if (!refreshDay()) schedule(); }, delay);
    };
    schedule();
    return () => clearTimeout(timer);
  });
  onMount(() => {
    window.addEventListener('focus', refreshDay);
    document.addEventListener('visibilitychange', refreshDay);
    return () => { window.removeEventListener('focus', refreshDay); document.removeEventListener('visibilitychange', refreshDay); };
  });
  $effect.pre(() => {
    const next = mode.mode;
    if (previousMode !== null && previousMode !== next) {
      // Window revisions can remove a child before this mode effect runs.
      const active = document.activeElement;
      const focused = active === document.body ? lastRegionFocus : active;
      const owned = focused instanceof HTMLElement && (region?.contains(focused)
        || (focused === lastRegionFocus && !focused.isConnected));
      announcement = next === 'dungeon' ? 'Dungeon' : next === 'realm' ? 'Realm' : 'Session';
      void tick().then(() => {
        if (owned && focused && !focused.isConnected && region?.isConnected
          && (document.activeElement === document.body || document.activeElement === focused)) {
          region.closest('main')?.querySelector<HTMLElement>('h1')?.focus({ preventScroll: true });
          lastRegionFocus = null;
        }
      });
    }
    if (previousMode === next && mode.stale !== previousStale) {
      announcement = mode.stale ? 'Recovering'
        : next === 'nexus' ? 'Session' : 'Live';
    }
    previousMode = next; previousStale = mode.stale;
  });
  function choose(next: ScopeChoice) {
    if (choice === next) return;
    choice = next; refreshDay(); onScopeChange?.(next);
  }

  $effect(() => {
    const next = snapshot, status = capture, state = liveState;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const update = () => untrack(() => {
      const observation = observe(next, status, state, performance.now());
      mode = observation.mode;
      displayed = mode.mode !== 'nexus' ? observation.retained : null;
      if (observation.delay !== null) timer = setTimeout(update, observation.delay);
    });
    update();
    return () => clearTimeout(timer);
  });
</script>
<div class="home" bind:this={region} onfocusin={rememberFocus} onfocusout={releaseFocus}>
{#if client && visuals.phase === 'unavailable'}<button type="button" onclick={() => visualController?.retry()}>Retry Sprites</button>{/if}
<p class="sr-only" role="status" aria-live="polite" aria-atomic="true">{announcement}</p>
{#if mode.mode === 'dungeon'}
  <DungeonHome snapshot={displayed} stale={mode.stale} {visuals} {portalType} liveWindow={scopedWindow}
    dungeonBests={pbBests} onSelectFight={connection?.selectHomeFight} />
{:else if mode.mode === 'realm'}
  <RealmHome snapshot={displayed} stale={mode.stale} window={scopedWindow ?? emptyHomeLiveWindow()} {visuals} />
{:else}
  <div class="home-idle">
    {#if captureState !== 'WaitingForGame' || liveState?.phase === 'failed'}
      {#if liveState?.phase === 'failed'}
        <EmptyState state="error" heading="Connection Interrupted" description="" />
        <DebugNote text="No return to Nexus or successful run is inferred." />
      {:else if (mode.stale || captureState === 'Recovering' || liveState?.phase === 'resyncing')
        && !(liveState?.phase === 'resyncing' && captureState !== 'Recovering'
          && mode.mode === 'nexus' && snapshot?.area_kind === 'nexus')}
        <EmptyState state="loading" heading="Reconnecting" description="" />
        <DebugNote text="Capture is recovering. Previous live-area values are no longer shown as live." />
      {:else if captureState && !['Stopped', 'Live', 'Recovering'].includes(captureState)}
        <EmptyState state="unavailable" heading="Capture Offline" description="" />
        <!-- Capture state alone does not establish a run outcome. -->
        <DebugNote text="A disconnect does not establish a run outcome." />
      {:else if captureState === 'Stopped' || !snapshot}
        <EmptyState heading="Ready" description="" />
      {:else if snapshot?.session?.ended != null}
        <EmptyState heading="Session Ended" description="" />
      {:else if captureState === 'Live' && snapshot?.area_kind === 'nexus'}
        <!-- NexusHome supplies the location and session heading. -->
      {:else if captureState === 'Live' && snapshot?.area_kind === 'unknown'}
        <!-- NexusHome owns the area title. -->
      {:else if snapshot?.area_kind === 'realm' || snapshot?.area_kind === 'other_hub'}
        <!-- NexusHome owns the retained area title. -->
      {:else if captureState === 'Live'}
        <!-- NexusHome owns the area title. -->
      {:else}
        <EmptyState heading="Ready" description="" />
      {/if}
    {/if}
    {#if mode.mode === 'nexus'}<NexusHome {statsEnabled} {snapshot} {visuals} history={choice === 'today' && !clockReady ? { ...history, phase: 'error', retryAvailable: true, message: 'The capture clock is unavailable. Retry to check Demo mode before reading Today.' } : history}
      loot={scopeLoot} {lootPage} {lootStatus} onRetryLoot={() => controller?.retryLoot()} onNextLoot={() => controller?.next()} onFirstLoot={() => controller?.first()}
      {choice} {scope} onScopeChange={choose} {syntheticClock} retry={() => { if (!clockReady) onClockRetry?.(); controller?.retry(); }} />{/if}
  </div>
{/if}
</div>
<style>
  .home-idle, .home { display: grid; align-content: start; gap: var(--space-2); min-width: 0; min-height: 0; }
  @media (min-width: 951px) {
    .home:has(> :global(.area-home)) { display: flex; flex-direction: column; height: 100%; }
    .home > :global(.area-home) { flex: 1; }
  }
  button { font: inherit; min-height: var(--target-min); border: 1px solid var(--border-strong); border-radius: var(--radius-control); padding: var(--space-2) var(--space-4); color: var(--text-secondary); background: var(--surface-panel); }
</style>
