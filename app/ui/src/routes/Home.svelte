<script module lang="ts">
  import { createEndedStreaks, dungeonGeneration, type EndedStreaks } from '../lib/home/v4/streaks';
  // Navigation remounts Home. The shell connection owns session memory, while
  // the area banner queue remains local to the currently mounted screen.
  const sessionStreaks = new WeakMap<object, { session: string | null; value: EndedStreaks }>();
</script>

<script lang="ts">
  import '../lib/home/v4/home-v4.css';
  import { isPresentationVisible, observePresentationVisibility } from '../lib/bridge/visibility';
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
  import SessionHud from '../lib/home/v4/SessionHud.svelte';
  import { createHomeV4Reads, type HomeV4ReadState } from '../lib/home/v4/reads';
  import { potionNames, potionStat, WHITE_BAG_TYPE } from '../lib/home/v4/hud';
  import { spriteUrl } from '../lib/bridge/visuals';
  import { routeHref } from '../lib/navigation/routes';
  import { bossOnly } from '../lib/home/v3/screen-data';
  import { isBossFight, scopedFights } from '../lib/shell/rail';
  import { bestItem } from '../lib/home/v3/loot-rank';
  import { objectVisual } from '../lib/bridge/visual-index';
  import DungeonScreen from '../lib/home/v4/DungeonScreen.svelte';
  import RealmScreen from '../lib/home/v4/RealmScreen.svelte';
  import RecapPlaceholder from '../lib/home/v4/RecapPlaceholder.svelte';
  import RecapScreen from '../lib/home/v4/RecapScreen.svelte';
  import VisitRecapScreen from '../lib/home/v4/VisitRecapScreen.svelte';
  import RecapCopyButton from '../lib/home/v4/RecapCopyButton.svelte';
  import { recapDate } from '../lib/home/v4/recap-image';
  import { recapVisualTypes, runRecapView, visitRecapView, visitRecapVisualTypes } from '../lib/home/v4/recap-view';
  import RecapReadyBanner from '../lib/home/v4/RecapReadyBanner.svelte';
  import { homeScreen, storedVisitId, type RecapState } from '../lib/home/v4/recap';
  import { createRecapReads, type RecapLoad } from '../lib/home/v4/recap-reads';
  import { createWhiteQueue, newWhites, type WhiteBannerItem, type WhiteBannerState } from '../lib/home/v4/white-queue';

  let { snapshot, statsEnabled = false, redesign = false, capture = null, liveState = null, event = null, historyReady = null,
    client, store = null, required = null, initial = false, syntheticClock = false, clockReady = true, savedScope = null, onScopeChange, onReady, onClockRetry, entryKey = '', onRowsReady }: {
    snapshot: LiveSnapshot | null; statsEnabled?: boolean; redesign?: boolean; capture?: CaptureStatus | null; liveState?: LiveState | null;
    event?: EventEnvelope | null; historyReady?: StoreStatus | null;
    client?: Client; store?: StoreStatus | null; required?: string | null; initial?: boolean; syntheticClock?: boolean; clockReady?: boolean;
    savedScope?: ScopeChoice | null; onScopeChange?: (choice: ScopeChoice) => void; onReady?: () => void;
    onClockRetry?: () => void;
    entryKey?: string; onRowsReady?: (key: string) => void;
  } = $props();
  const connection = getContext<ShellConnection | undefined>(SHELL_CONNECTION);
  const homeWindow = connection?.homeWindow ?? readable(emptyHomeLiveWindow());
  const homeRecap = connection?.homeRecap ?? readable<RecapState | null>(null);
  const recap = $derived(redesign ? $homeRecap : null);
  const recapPresentation = connection?.recapPresentation ?? readable({ banner: false, open: false });
  let lastRecapButton = $state<HTMLButtonElement>();
  $effect(() => {
    if (!redesign || !connection?.recapPresentation) return;
    const presentation = connection.recapPresentation;
    const release = observePresentationVisibility(presentation.setActive);
    return () => { release(); presentation.setActive(false); };
  });
  let mode = $state(initialHomeMode());
  const screen = $derived(redesign ? homeScreen(mode, recap, $recapPresentation.open) : mode.mode);
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

  let nowMs = $state(Date.now());
  const hudNowMs = $derived(syntheticClock ? Number(snapshot?.at ?? NaN) / 1000 : nowMs);
  let v4State = $state.raw<HomeV4ReadState | null>(null);
  const emptyV4Reads: HomeV4ReadState = { hud: null, streak: null, profile: null, usual: new Map(), failedUsualTypes: new Set(),
    errors: { hud: false, streak: false, profile: false, usual: false } };
  let v4Reads = $state.raw<ReturnType<typeof createHomeV4Reads> | null>(null);
  let v4Owner: Client | undefined;
  const v4Client = $derived(redesign ? client : undefined);
  let recapLoad = $state.raw<RecapLoad | null>(null);
  const savedRunRecap = $derived(recapLoad?.status === 'saved' && recapLoad.data.kind === 'run'
    && recap?.source.kind === 'run' && recapLoad.data.story.run.id === recap.source.runId
    ? runRecapView(recapLoad.data) : null);
  const savedRunRecapTypes = $derived(savedRunRecap ? [...recapVisualTypes(savedRunRecap),
    ...savedRunRecap.party.rows.filter(row => row.rank !== null).slice(0, 5)
      .flatMap(row => row.classId === null ? [] : [row.classId])] : []);
  const recapVisitId = $derived(recap?.source.kind === 'visit' ? storedVisitId(recap.source) : null);
  const savedVisitRecap = $derived(recapLoad?.status === 'saved' && recapLoad.data.kind === 'visit'
    && recapVisitId !== null && (recapLoad.data.visit.visit_id === recapVisitId
      || recapLoad.data.visit.periods.some(period => period.visit_id === recapVisitId))
    ? visitRecapView(recapLoad.data, undefined, untrack(() => recap?.summary.title)) : null);
  const savedVisitRecapTypes = $derived(savedVisitRecap ? visitRecapVisualTypes(savedVisitRecap) : []);
  const visitRecapCharacter = $derived(savedVisitRecap?.characterId != null && snapshot?.character?.identity_verified
    && snapshot.character.id === savedVisitRecap.characterId ? snapshot.character : null);
  let recapReads = $state.raw<ReturnType<typeof createRecapReads> | null>(null);
  let recapOwner: Client | undefined;
  $effect(() => {
    const owner = v4Client;
    untrack(() => {
      if (owner === recapOwner) return;
      recapReads?.dispose(); recapOwner = owner; recapLoad = null;
      recapReads = owner ? createRecapReads(owner, next => { recapLoad = next; }) : null;
    });
  });
  $effect(() => {
    const instance = recapReads;
    const input = { recap, store: store ?? historyReady, streamId: snapshot?.stream_id ?? mode.streamId };
    untrack(() => instance?.update(input));
  });
  onDestroy(() => recapReads?.dispose());
  $effect(() => {
    const owner = v4Client;
    untrack(() => {
      if (owner === v4Owner) return;
      v4Reads?.dispose();
      v4Owner = owner;
      v4State = null;
      v4Reads = owner ? createHomeV4Reads(owner, next => { v4State = next; }) : null;
    });
  });
  onDestroy(() => v4Reads?.dispose());
  $effect(() => {
    if (!redesign || syntheticClock) return;
    nowMs = Date.now();
    const timer = setInterval(() => { nowMs = Date.now(); }, 30_000);
    return () => clearInterval(timer);
  });
  $effect(() => {
    const instance = v4Reads;
    if (!instance) return;
    return connection?.onEvent?.(envelope => instance.onEvent(envelope));
  });
  $effect(() => {
    const instance = v4Reads, observed = connection?.onEvent ? null : event;
    if (instance && observed) untrack(() => instance.onEvent(observed));
  });
  $effect(() => {
    const instance = v4Reads;
    if (!instance) return;
    const window = $homeWindow;
    const fights = snapshot && window.streamId === snapshot.stream_id && window.runId === (snapshot.run?.id ?? null)
      ? window.fights : [];
    const input = { scope: homeScope('session', snapshot, new Date(hudNowMs)),
      dungeon: snapshot?.area_kind === 'dungeon' ? snapshot.run?.dungeon ?? null : null,
      runId: snapshot?.run?.id ?? null,
      profileDungeon: snapshot?.run?.ended === null ? snapshot.run.dungeon : null,
      generation: dungeonGeneration(snapshot), drops: snapshot?.drops ?? [],
      characterId: snapshot?.character?.identity_verified ? snapshot.character.id : null,
      bossTypes: fights.filter(snapshot?.area_kind === 'realm' ? isBossFight : bossOnly).map(fight => fight.target_type!), nowMicros: hudNowMs * 1000 };
    untrack(() => instance.update(input));
  });
  let potionTypes = $state.raw<number[]>([]);
  $effect(() => {
    const owner = v4Client;
    potionTypes = [];
    if (!owner) return;
    let disposed = false, revision = 0;
    const refresh = async () => {
      const request = ++revision;
      try {
        // Native search accepts at most 25 hits. Query each stat so unrelated
        // consumables cannot crowd a representative potion out of the results.
        const items = (await Promise.all(Object.values(potionNames).map(name =>
          owner.searchItems(`Potion of ${name}`, 1).catch(() => [])))).flat();
        if (!disposed && request === revision) potionTypes = items.filter(item => potionStat(item.name)).map(item => item.object_type);
      } catch { if (!disposed && request === revision) potionTypes = []; }
    };
    const release = owner.onVisualsChanged(() => { potionTypes = []; void refresh(); });
    void refresh();
    return () => { disposed = true; release(); };
  });

  let receivedHistory = $state(false);
  let visuals = $state.raw<HomeVisuals>(emptyHomeVisuals());
  let visualController = $state.raw<ReturnType<typeof createHomeVisuals> | null>(null);
  let visualOwner: Client | undefined;
  const visualClient = $derived(client);
  const scopedWindow = $derived(displayed && mode.mode !== 'nexus'
    && $homeWindow.streamId === displayed.stream_id
    && $homeWindow.runId === (mode.mode === 'dungeon' ? displayed.run?.id : null)
    ? $homeWindow : null);
  let white = $state.raw<WhiteBannerState>({ current: null, overflow: 0 });
  const streakEntry = untrack(() => {
    const previous = connection && sessionStreaks.get(connection);
    if (previous) return previous;
    const entry = { session: null as string | null, value: createEndedStreaks() };
    if (connection) sessionStreaks.set(connection, entry);
    return entry;
  });
  let endedStreaks = $state.raw<EndedStreaks>(streakEntry.value);
  let whiteQueue: ReturnType<typeof createWhiteQueue> | null = null;
  let whiteScope: string | null = null;
  let seenWhites = new Set<string>();
  let hasBanneredWhite = false;
  const whiteKey = $derived(redesign && displayed && scopedWindow && mode.mode !== 'nexus'
    ? JSON.stringify([displayed.stream_id, displayed.session?.id, mode.mode, displayed.map,
      mode.mode === 'dungeon' ? displayed.run?.id : scopedWindow.visitKey]) : null);
  const bannerWhite = $derived.by(() => {
    const current = white.current;
    if (!current) return white;
    // Catalog reads may finish after the drop arrives. Resolve names as they
    // load, while retaining the queue's push-time streak and original timer.
    const drop = scopedWindow?.drops.find(drop => drop.id === current.dropId);
    const source = objectVisual(visuals.objects, drop?.source_enemy)?.name;
    return { ...white, current: { ...current,
      itemName: objectVisual(visuals.objects, current.itemType)?.name ?? current.itemName,
      sourceName: source ? `${source}${drop?.source_estimated ? ' (estimated)' : ''}` : current.sourceName } };
  });
  $effect(() => {
    const scope = whiteKey, window = scopedWindow, session = snapshot?.session?.id ?? null;
    untrack(() => {
      if (session !== null) {
        if (streakEntry.session !== null && session !== streakEntry.session) {
          endedStreaks.reset();
          endedStreaks = { ...endedStreaks };
        }
        streakEntry.session = session;
      }
      if (scope !== whiteScope) {
        whiteQueue?.dispose();
        whiteQueue = scope === null ? null : createWhiteQueue(next => { white = next; });
        white = { current: null, overflow: 0 };
        whiteScope = scope;
        hasBanneredWhite = false;
        // Include unclassified bags: later rank enrichment is not a new drop.
        seenWhites = new Set(window?.drops.map(drop => drop.id) ?? []);
        return;
      }
      if (!whiteQueue || !window) return;
      const dungeon = mode.mode === 'dungeon' ? displayed?.run?.dungeon ?? null : null;
      const generation = dungeonGeneration(displayed), cached = v4State?.streak;
      const stats = !v4State?.errors.streak && cached?.generation === generation && cached?.value.dungeon === dungeon
        ? cached.value : null;
      for (const drop of newWhites(window.drops, seenWhites)) {
        let streak: WhiteBannerItem['streak'] = null;
        // Only the first banner in this scope can end a dry streak. Later
        // reads may already include it, or still contain the previous streak.
        if (!hasBanneredWhite && dungeon && stats) {
          if (stats.runs_since_last_white !== null && stats.runs_since_last_white > 0) {
            streak = { kind: 'ended', runs: stats.runs_since_last_white };
            endedStreaks.record(dungeon, stats.runs_since_last_white);
            endedStreaks = { ...endedStreaks };
          } else if (stats.runs_since_last_white === null && stats.runs_ended > 0) {
            streak = { kind: 'first', runs: stats.runs_ended };
          }
        }
        hasBanneredWhite = true;
        if (generation !== null) v4Reads?.consumeStreak(generation);
        const item = bestItem(drop, visuals), source = objectVisual(visuals.objects, drop.source_enemy)?.name;
        whiteQueue.push({ dropId: drop.id, itemType: item?.itemType ?? null,
          itemName: objectVisual(visuals.objects, item?.itemType)?.name ?? '—', bagType: drop.bag_type,
          areaName: dungeon ?? displayed?.map ?? 'Realm',
          sourceName: source ? `${source}${drop.source_estimated ? ' (estimated)' : ''}` : null,
          at: drop.at, streak });
      }
      for (const drop of window.drops) seenWhites.add(drop.id);
    });
  });
  onDestroy(() => whiteQueue?.dispose());
  let portalType = $state<number | null>(null);
  let portalController: ReturnType<typeof createDungeonPortal> | null = null;
  const portalTitle = $derived((screen === 'recap' || (redesign && $recapPresentation.banner)) ? recap?.summary.kind === 'run' ? savedRunRecap?.title ?? recap.summary.title : null
    : mode.mode === 'dungeon' ? displayed?.run?.dungeon ?? null : null);
  const portalDungeon = $derived(portalTitle === '—' ? null : portalTitle);
  const visualKey = $derived([...homeVisualTypes(mode.mode !== 'nexus' ? displayed : null,
    mode.mode === 'nexus' ? history.summary : null, scopedWindow),
  ...(mode.mode === 'nexus' ? scopeLoot?.items.map(item => item.item_type) ?? [] : []),
  ...(redesign ? [WHITE_BAG_TYPE, ...(v4State?.hud?.ut_st ?? []), ...potionTypes,
    ...(!v4State?.errors.profile ? v4State?.profile?.bosses.map(boss => boss.target_type) ?? [] : [])] : []),
  ...(redesign && mode.mode === 'realm' ? scopedFights(displayed, scopedWindow)
    .flatMap(fight => fight.boss_type === null ? [] : [fight.boss_type]) : []),
  ...(recap?.summary.best ? [recap.summary.best.drop.bag_type,
    ...recap.summary.best.drop.contents.map(item => item.item_type),
    ...(recap.summary.best.drop.contents_known ? recap.summary.best.drop.items : [])] : []),
  ...(recap?.summary.fights?.flatMap(fight => fight.target_type === null ? [] : [fight.target_type]) ?? []),
  ...savedRunRecapTypes,
  ...savedVisitRecapTypes,
  ...(screen === 'recap' && visitRecapCharacter?.class_id != null ? [visitRecapCharacter.class_id] : []),
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
  $effect.pre(() => {
    if ($recapPresentation.banner) return;
    const focused = document.activeElement === document.body ? lastRegionFocus : document.activeElement;
    if (focused instanceof HTMLElement && focused.closest('.recap-ready')) {
      lastRecapButton?.focus({ preventScroll: true });
    }
  });
  let previousMode: string | null = null;
  let previousBody: string | null = null;
  let previousStale = false;
  // Within recap, the placeholder and saved screens replace each other (load, Retry).
  const body = $derived(screen !== 'recap' ? screen : savedRunRecap ? 'recap:run' : savedVisitRecap ? 'recap:visit' : 'recap:placeholder');
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
  let rowsAvailable = $derived(screen !== 'nexus' || !client || (receivedHistory
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
    const releaseVisibility = observePresentationVisibility(visible => {
      clearTimeout(timer);
      if (visible) { refreshDay(); schedule(); }
    });
    return () => { releaseVisibility(); clearTimeout(timer); };
  });
  onMount(() => {
    window.addEventListener('focus', refreshDay);
    const releaseVisibility = observePresentationVisibility(visible => { if (visible) refreshDay(); });
    return () => { window.removeEventListener('focus', refreshDay); releaseVisibility(); };
  });
  $effect.pre(() => {
    const next = screen, nextBody = body;
    if (previousBody !== null && previousBody !== nextBody) {
      // Window revisions can remove a child before this mode effect runs.
      const active = document.activeElement;
      const focused = active === document.body ? lastRegionFocus : active;
      const owned = focused instanceof HTMLElement && (region?.contains(focused)
        || (focused === lastRegionFocus && !focused.isConnected));
      if (previousMode !== next) {
        announcement = next === 'dungeon' ? 'Dungeon' : next === 'realm' ? 'Realm' : next === 'recap' ? 'Recap' : 'Session';
      }
      void tick().then(() => {
        if (owned && focused && !focused.isConnected && region?.isConnected
          && (document.activeElement === document.body || document.activeElement === focused)) {
          region.closest('main')?.querySelector<HTMLElement>('h1')?.focus({ preventScroll: true });
          lastRegionFocus = null;
        }
      });
    }
    if (previousMode === next && next !== 'recap' && mode.stale !== previousStale) {
      announcement = mode.stale ? 'Recovering'
        : next === 'nexus' ? 'Session' : 'Live';
    }
    previousMode = next; previousBody = nextBody; previousStale = mode.stale;
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
      if (isPresentationVisible() && observation.delay !== null) timer = setTimeout(update, observation.delay);
    });
    update();
    return () => clearTimeout(timer);
  });
</script>
<div class="home" class:home-v4={redesign} data-home={redesign ? 'v4' : 'v3'} bind:this={region} onfocusin={rememberFocus} onfocusout={releaseFocus}>
{#if redesign}
  <SessionHud hud={v4State?.errors.hud ? null : v4State?.hud ?? null} sessionStarted={snapshot?.session?.started ?? null} nowMs={hudNowMs}
    sessionEnded={snapshot?.session?.ended ?? null}
    {visuals} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite) ?? ''}
    lootHref={routeHref({ kind: 'loot', view: 'feed' })} />
  {#if recap}
    <div class="last-recap"><button type="button" bind:this={lastRecapButton} onclick={() => connection?.recapPresentation?.open()}>Last recap: {recap.summary.title}</button></div>
  {/if}
  <p class="sr-only" role="status" aria-live="polite" aria-atomic="true">{$recapPresentation.banner && recap ? `Recap ready: ${recap.summary.title}` : ''}</p>
  {#if recap && $recapPresentation.banner && connection?.recapPresentation}
    <RecapReadyBanner {recap} view={savedRunRecap ?? savedVisitRecap} {visuals} {portalType}
      resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite) ?? ''}
      presentation={connection.recapPresentation} />
  {/if}
{/if}
{#if client && visuals.phase === 'unavailable'}<button type="button" onclick={() => visualController?.retry()}>Retry Sprites</button>{/if}
<p class="sr-only" role="status" aria-live="polite" aria-atomic="true">{announcement}</p>
{#if screen === 'dungeon'}
  {#if redesign}
  <DungeonScreen snapshot={displayed} stale={mode.stale} {visuals} {portalType} liveWindow={scopedWindow}
    dungeonBests={pbBests} reads={v4State ?? emptyV4Reads} {endedStreaks} white={bannerWhite} onSelectFight={connection?.selectHomeFight} />
  {:else}
  <DungeonHome snapshot={displayed} stale={mode.stale} {visuals} {portalType} liveWindow={scopedWindow}
    dungeonBests={pbBests} onSelectFight={connection?.selectHomeFight} />
  {/if}
{:else if screen === 'realm'}
  {#if redesign}
  <RealmScreen snapshot={displayed} stale={mode.stale} window={scopedWindow ?? emptyHomeLiveWindow()} {visuals}
    reads={v4State ?? emptyV4Reads} white={bannerWhite} />
  {:else}
  <RealmHome snapshot={displayed} stale={mode.stale} window={scopedWindow ?? emptyHomeLiveWindow()} {visuals} />
  {/if}
{:else if screen === 'recap' && recap}
  {#if savedRunRecap}
  <RecapScreen view={savedRunRecap} {visuals} {portalType} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite) ?? ''}
    recordsUnavailable={recapLoad?.status === 'saved' && recapLoad.data.kind === 'run' && recapLoad.data.recordsUnavailable}
    onRetry={() => recapReads?.retry()}
    onDismiss={() => connection?.dismissHomeRecap?.()}>
    {#snippet actions()}
      {#if client}<RecapCopyButton view={savedRunRecap} settings={client} {visuals} context={{
        character: savedRunRecap.character?.name ?? null,
        className: objectVisual(visuals.objects, savedRunRecap.character?.class_id)?.name ?? null,
        date: recapDate(recap.summary.endedAt ?? recap.summary.startedAt),
      }} />{/if}
    {/snippet}
  </RecapScreen>
  {:else if savedVisitRecap}
  <VisitRecapScreen view={savedVisitRecap} {visuals} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite) ?? ''}
    onDismiss={() => connection?.dismissHomeRecap?.()}>
    {#snippet actions()}
      {#if client}<RecapCopyButton view={savedVisitRecap} settings={client} {visuals} context={{
        character: visitRecapCharacter?.name ?? null,
        className: objectVisual(visuals.objects, visitRecapCharacter?.class_id)?.name ?? null,
        date: recapDate(recap.summary.endedAt ?? recap.summary.startedAt),
      }} />{/if}
    {/snippet}
  </VisitRecapScreen>
  {:else}
  <RecapPlaceholder {recap} {visuals} resolveSpriteUrl={sprite => spriteUrl(visuals.client_fingerprint, sprite) ?? ''}
    status={recapLoad?.status === 'partial' ? 'partial' : 'saving'} onRetry={() => recapReads?.retry()}
    reason={recapLoad?.status === 'partial' ? recapLoad.reason : undefined}
    message={recapLoad?.status === 'partial' ? recapLoad.message : null}
    onDismiss={() => connection?.dismissHomeRecap?.()} />
  {/if}
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
