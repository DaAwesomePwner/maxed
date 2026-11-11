<script module lang="ts">
  // Shell normally lives for the window; remounts must not repeat the receipt.
  const reportedWindows = new WeakSet<Window>();
</script>

<script lang="ts">
  import { settingsHref } from '../navigation/routes';
  import { onDestroy, onMount, setContext, tick, untrack, type Snippet } from 'svelte';
  import { readonly, writable } from 'svelte/store';
  import type { Client } from '../bridge/client';
  import ItemHoverCard from '../components/ItemHoverCard.svelte';
  import type { CaptureStatus } from '../bindings/CaptureStatus';
  import type { LiveSnapshot } from '../bindings/LiveSnapshot';
  import type { ObjectVisual } from '../bindings/ObjectVisual';
  import type { StoreStatus } from '../bindings/StoreStatus';
  import type { EventEnvelope } from '../bindings/EventEnvelope';
  import type { SpriteRef } from '../bindings/SpriteRef';
  import type { AlertPreferences } from '../bindings/AlertPreferences';
  import type { Timestamp } from '../bindings/Timestamp';
  import type { LiveState } from '../bridge/live';
  import { invalidatesHomeHistory } from '../bridge/live';
  import { observePresentationVisibility } from '../bridge/visibility';
  import { createHistoryCache, type HistoryCache } from '../runs/history-cache';
  import { runInvalidationRoute } from '../runs/invalidation';
  import { invalidatesSavedHistory } from './history-invalidation';
  import { notifyEventObserver } from './event-observers';
  import { createLogsPending, matchesPendingView } from '../logs/pending';
  import { createAlertController, type AlertPresentationState } from '../alerts/presentation';
  import { createAlertSession, type AlertSession, type AlertSessionStatus } from '../alerts/session';
  import { spriteUrl } from '../bridge/visuals';
  import { browserHistory, createRouter, installHistoryPersistence, installNavigationInput, type HistoryAdapter, type NavigationState, type Router } from '../navigation/history';
  import { focusedRow, restorePosition } from '../navigation/state';
  import { parseRoute, productionFeatures, routeTitle, type Features, type FightRelationship } from '../navigation/routes';
  import DebugNote from '../debug/DebugNote.svelte';
  import BusyStatus from '../status/BusyStatus.svelte';
  import StatusOverlay from '../status/StatusOverlay.svelte';
  import { createStatusRegistry, STATUS_REGISTRY } from '../status/registry';
  import { createDungeonMute, DUNGEON_MUTE } from '../alerts/dungeon-mute';
  import DungeonMuteFeedback from '../alerts/DungeonMuteFeedback.svelte';
  import BannerHost from '../components/BannerHost.svelte';
  import CharacterRail from './CharacterRail.svelte';
  import RouteFrame from '../../routes/RouteFrame.svelte';
  import { createRecordingSession } from '../settings/recording';
  import { railView } from './rail';
  import { createHomeObservation } from '../home/mode';
  import { emptyHomeLiveWindow } from '../home/v2/live';
  import { createRecapTracker } from '../home/v4/recap';
  import { createCharacterController } from '../character/controller';
  import { connectShell } from './connection';
  import { SHELL_CONNECTION, type ShellConnection } from './context';
  let { client, preview = false, features = productionFeatures, historyAdapter, historyCache, content, rowsReady = () => Promise.resolve(), resolveSpriteUrl, initialSetupRequired = false }: {
    client: Client; preview?: boolean; features?: Features; historyAdapter?: HistoryAdapter; historyCache?: HistoryCache;
    content?: Snippet<[NavigationState, Router, { store: StoreStatus | null; onRowsReady: (key: string) => void }]>;
    rowsReady?: (key: string) => Promise<unknown>;
    resolveSpriteUrl?: (sprite: SpriteRef) => string | null;
    initialSetupRequired?: boolean;
  } = $props();
  let snapshot = $state.raw<LiveSnapshot | null>(null);
  let capture = $state<CaptureStatus | null>(null);
  let nativeDemo = $state<boolean | null>(null);
  let catalogAvailable = $state<boolean | null>(null);
  let catalogEpoch = 0;
  let releaseCatalog = () => {};
  let releaseHistory = () => {};
  let releaseHistoryVisuals = () => {};
  let recordingSession = $derived(createRecordingSession(client, preview));
  $effect(() => { const session = recordingSession; return () => session.dispose(); });
  let captureState = $derived(capture?.state ?? null);
  $effect(() => { captureState; void recordingSession.refresh(); });
  let recordingIndicator = $derived(!preview && nativeDemo === false && ['WaitingForGame', 'Live', 'Recovering'].includes(capture?.state ?? '')
    ? $recordingSession.status?.state === 'failed' ? 'Recording Stopped'
      : ['armed', 'writing'].includes($recordingSession.status?.state ?? '') ? 'Recording' : null
    : null);
  const statusRegistry = createStatusRegistry();
  setContext(STATUS_REGISTRY, statusRegistry);
  const characterController = createCharacterController({
    getCharacterCalculation: request => client.getCharacterCalculation(request),
  });
  onDestroy(() => characterController.dispose());
  let characterSnapshot: LiveSnapshot | null = null;
  function updateCharacter() {
    if (features.character) characterController.update(characterSnapshot, capture, liveState);
  }
  const sharedCapture = writable<CaptureStatus | null>(null);
  const sharedSnapshot = writable<LiveSnapshot | null>(null);
  const eventObservers = new Set<(event: EventEnvelope) => void>();
  let currentHomeWindow = emptyHomeLiveWindow();
  const sharedHomeWindow = writable(currentHomeWindow);
  const homeRecap = createRecapTracker();
  onDestroy(() => homeRecap.presentation.dispose());
  $effect(() => {
    if (!features.homeRedesign) return;
    return observePresentationVisibility(homeRecap.setVisible);
  });
  const sharedAlertPreferences = writable<AlertPreferences | null>(null);
  const sharedAlertStatus = writable<AlertSessionStatus>('retired');
  let releaseAlertStatus = () => {};
  let alertState = $state<AlertPresentationState>({ current: null, queued: 0, overflow: 0 });
  let alertSessionKey = $state('initial');
  let alertSession: AlertSession | null = null, alertStream: string | null = null;
  let alertPolicy: AlertPreferences | null = null, alertEpoch = 0;
  let sourceClock: Timestamp | null = null, sourceObserved = 0;
  const alertController = createAlertController(next => { alertState = next; }, () => performance.now(), () => {
    if (sourceClock === null || nativeDemo === null) return null;
    // Synthetic time advances only with its source. Live time ages even without packets.
    const elapsed = syntheticClock ? 0 : Math.max(0, Math.floor((performance.now() - sourceObserved) * 1000));
    return (BigInt(sourceClock) + BigInt(elapsed)).toString();
  });
  const alerts: AlertSession = {
    preferences: readonly(sharedAlertPreferences),
    status: readonly(sharedAlertStatus),
    refresh: () => alertSession?.refresh() ?? Promise.reject(new Error('Alert preferences are unavailable until a session is connected.')),
    update: next => alertSession?.update(next) ?? Promise.reject(new Error('Alert preferences are unavailable until a session is connected.')),
    // Borrowed by routes: only Shell's lifecycle may retire the owned session.
    dispose: () => {},
  };
  const dungeonMute = createDungeonMute(alerts, () => mounted && !preview && nativeDemo === false);
  setContext(DUNGEON_MUTE, dungeonMute);
  // Read the current relationship so run-scoped notices invalidate standalone fight-player routes.
  const cache = untrack(() => historyCache) ?? createHistoryCache({ invalidates: invalidatesSavedHistory, route: () => runInvalidationRoute(router?.route, relationship) });
  setContext<ShellConnection>(SHELL_CONNECTION, { character: characterController, capture: sharedCapture, snapshot: sharedSnapshot, historyCache: cache,
    onEvent: receive => { eventObservers.add(receive); return () => { eventObservers.delete(receive); }; },
    homeRecap, recapPresentation: homeRecap.presentation, dismissHomeRecap: homeRecap.dismiss,
    homeWindow: sharedHomeWindow, selectHomeFight: id => releaseConnection.selectFight(id), reconnect: () => connect(), alerts, get recording() { return recordingSession; } });
  let visuals = $state.raw<ObjectVisual[]>([]);
  let fingerprint = $state('');
  let cacheFingerprint = '';
  let liveState = $state<LiveState>({ phase: 'connecting', historyStale: true, historyFailed: false, historyLagging: false, message: '' });
  const resolveVisualSprite = (sprite: SpriteRef) => resolveSpriteUrl ? resolveSpriteUrl(sprite) : spriteUrl(fingerprint, sprite);
  let store = $state<StoreStatus | null>(null);
  let acceptedEvent = $state<EventEnvelope | null>(null);
  let historyReady = $state<StoreStatus | null>(null);
  let historyRequired = $state<string | null>(null);
  let routeRequired = $state<string | null>(null);
  let historyInitial = $state(false);
  let syntheticClock = $derived(preview || nativeDemo === true);
  let setupEpoch = 0;
  let setupStream: string | null = null;
  let setupRetry: ReturnType<typeof setTimeout> | undefined;
  let errors = $state({ captureSubscription: '', captureAction: '', snapshot: '', store: '' });
  let unsavedClose = $state(false);
  let unsavedDialog: HTMLDivElement | undefined = $state();
  $effect(() => { if (unsavedClose) unsavedDialog?.focus(); });
  let pendingSummaries = $state(false);
  $effect(() => {
    if (preview) return;
    let disposed = false;
    let release: (() => void) | undefined;
    void client.onUnsavedClose(() => { unsavedClose = true; }).then(stop => {
      if (disposed) stop(); else release = stop;
    }).catch(() => { /* Store failure still exposes recovery actions. */ });
    return () => { disposed = true; release?.(); };
  });
  async function recoverStore(discard = false) {
    if (busy || preview) return;
    busy = true;
    try {
      if (discard) await client.exitWithoutSaving();
      else { await client.retryStore(); store = await client.queryStore(); pendingSummaries = false; }
      unsavedClose = false; errors.captureAction = '';
    } catch { errors.captureAction = discard ? 'Couldn’t exit. Retry.' : 'Couldn’t save. Retry saving.'; }
    finally { busy = false; }
  }
  let captureError = $derived([errors.captureSubscription, errors.captureAction].filter(Boolean).join(' '));
  let busy = $state(false);
  let router = $state<Router>();
  let navigation = $state<NavigationState>();
  let relationship = $state.raw<FightRelationship | null>(null);
  function relationshipValidated(next: FightRelationship) {
    const entry = navigation?.entry;
    if (!mounted || !entry || next.entryKey !== entry.key || next.scopeKey !== entry.scope
      || next.streamId !== snapshot?.stream_id) return;
    relationship = next;
  }
  $effect(() => { if (initialSetupRequired && !preview) router?.replaceInitial({ kind: 'setup' }); });
  let homeRoute = $derived(navigation?.entry.route.kind === 'home');
  let historyRoute = $derived(['runs', 'run', 'fight', 'player', 'run-player', 'loot', 'item'].includes(navigation?.entry.route.kind ?? ''));
  let pane: HTMLElement;
  let missingRow = $state(false);
  let observedStream: string | null = null;
  const observeRail = createHomeObservation({ pauseOnCaptureRecovery: true });
  let accountRailRevision = $state(0);
  $effect(() => {
    const owner = client;
    let active = true;
    let revision: number | null = null;
    const release = owner.onAccountHistoryChanged(notice => {
      if (!active || revision === notice.revision) return;
      revision = notice.revision;
      if (notice.reason === 'account_switched') {
        observeRail.historyChanged(); railSnapshot = null; accountRailRevision++;
      }
    });
    return () => { active = false; void release.then(stop => stop()); };
  });
  let railSnapshot = $state.raw<LiveSnapshot | null>(null);
  let railPaused = $state(false);
  let view = $derived(railView(railSnapshot, visuals, railPaused));
  $effect(() => {
    const next = snapshot, status = capture, state = liveState;
    void accountRailRevision;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const update = () => untrack(() => {
      const observation = observeRail(next, status, state, performance.now());
      railSnapshot = observation.retained; railPaused = observation.paused;
      if (observation.delay !== null) timer = setTimeout(update, observation.delay);
    });
    update();
    return () => clearTimeout(timer);
  });
  let releaseConnection: ReturnType<typeof connectShell> = Object.assign(() => {}, { selectFight: (_id: string | null) => {}, setHomeActive: (_active: boolean) => {} });
  $effect(() => { releaseConnection.setHomeActive(homeRoute); });
  let mounted = false;
  let interactiveReady = $derived(snapshot !== null && capture !== null && liveState.phase === 'live'
    && router !== undefined && navigation !== undefined && !busy && !captureError && !errors.snapshot
    && !errors.store && store?.state !== 'failed'
    && !['Failed', 'NpcapMissing', 'NpcapIncompatible', 'PermissionDenied'].includes(capture.state));
  $effect(() => {
    if (!interactiveReady || !mounted || reportedWindows.has(window)) return;
    const rail = pane?.closest('.app-frame')?.querySelector<HTMLElement>('.character-rail');
    if (!rail?.isConnected) return;
    const owner = client;
    let second = 0;
    // The first callback precedes paint; the second follows the rendered rail's
    // paint opportunity. Cleanup cancels stale readiness on errors/navigation.
    const first = requestAnimationFrame(() => {
      second = requestAnimationFrame(() => {
        if (!mounted || !interactiveReady || !rail.isConnected || owner !== client || reportedWindows.has(window)) return;
        reportedWindows.add(window);
        try { void owner.reportInteractiveRail().catch(() => {}); } catch { /* Measurement only. */ }
      });
    });
    return () => { cancelAnimationFrame(first); cancelAnimationFrame(second); };
  });
  let restoring = false;
  let transition = 0;
  let routeRowsReady: Promise<void> = Promise.resolve();
  let resolveRouteRows = () => {};
  let routeRowsKey = '';
  let routeWaitExpired: Promise<void> = Promise.resolve();
  let expireRouteWait = () => {};
  let rowsDeadline: ReturnType<typeof setTimeout> | undefined;
  const logsPending = createLogsPending();
  let pendingRevision = $state(0);
  let pendingCount = $derived.by(() => { void pendingRevision; return logsPending.count(navigation?.entry.key ?? ''); });
  function historyApplied(key: string, required: string | null) {
    logsPending.applied(key, required); pendingRevision++;
  }
  const historyInvalidates = (event: EventEnvelope) => invalidatesSavedHistory(runInvalidationRoute(router?.route, relationship), event);
  const historyInvalidated = (event: EventEnvelope) => invalidatesHomeHistory(event) || historyInvalidates(event);
  function rowsRendered(key: string) {
    if (key !== routeRowsKey) return;
    resolveRouteRows();
    if (!restoring) { clearTimeout(rowsDeadline); rowsDeadline = undefined; }
  }
  let connectedClient: Client | undefined;
  let connectedPreview: boolean | undefined;

  function observeAlertClock(at: Timestamp) {
    if (!/^\d+$/.test(at)) return;
    // Repeated snapshot clocks cannot restart the age of an undelivered occurrence.
    if (sourceClock === null || BigInt(at) > BigInt(sourceClock)) {
      sourceClock = at; sourceObserved = performance.now();
    }
  }
  function configureAlerts() {
    const state = capture?.state ?? snapshot?.capture.state;
    alertController.configure(nativeDemo !== null && state === 'Live' ? alertPolicy : null, alertStream);
  }
  function retireAlerts() {
    alertEpoch++;
    releaseAlertStatus(); releaseAlertStatus = () => {};
    dungeonMute.reset();
    alertSession?.dispose(); alertSession = null;
    alertStream = null; alertPolicy = null; sourceClock = null;
    sharedAlertPreferences.set(null);
    sharedAlertStatus.set('retired');
    alertController.configure(null, null);
    alertSessionKey = `retired:${alertEpoch}`;
  }
  function enterAlerts(stream: string) {
    if (stream === alertStream || stream === '') return;
    retireAlerts(); alertStream = stream;
    alertSessionKey = `${alertEpoch}:${stream}`;
    const epoch = alertEpoch, owner = client;
    alertController.configure(null, stream);
    alertSession = createAlertSession(owner, preferences => {
      if (!mounted || epoch !== alertEpoch || alertStream !== stream) return;
      alertPolicy = preferences; sharedAlertPreferences.set(preferences); configureAlerts();
    }, () => mounted && epoch === alertEpoch && !preview && nativeDemo === false && owner === connectedClient);
    releaseAlertStatus = alertSession.status.subscribe(status => {
      if (mounted && epoch === alertEpoch) sharedAlertStatus.set(status);
    });
    void alertSession.refresh().catch(() => { /* Null policy suppresses effects; Alerts exposes Retry. */ });
  }

  $effect(() => {
    const nextClient = client, nextPreview = preview;
    untrack(() => {
      if (!mounted || (nextClient === connectedClient && nextPreview === connectedPreview)) return;
      cache.clear();
      characterSnapshot = null; characterController.update(null, null, liveState);
      snapshot = null; capture = null; visuals = []; store = null;
      logsPending.reset(); pendingRevision++;
      acceptedEvent = null; historyReady = null; historyRequired = null; historyInitial = false; routeRequired = null;
      sharedCapture.set(null); sharedSnapshot.set(null);
      observedStream = null;
      errors = { captureSubscription: '', captureAction: '', snapshot: '', store: '' };
      router?.setScope(`${nextPreview ? 'demo' : 'live'}:${crypto.randomUUID()}`);
      connect();
    });
  });

  function invalidateSetup() {
    clearTimeout(setupRetry); setupRetry = undefined;
    setupEpoch++; setupStream = null; nativeDemo = null;
  }
  function readSetup(stream: string, retry = false) {
    if (setupStream === stream && !retry) return;
    invalidateSetup();
    setupStream = stream; nativeDemo = null;
    const epoch = setupEpoch, owner = client;
    const current = () => mounted && epoch === setupEpoch && owner === connectedClient && setupStream === stream;
    // Six automatic retries per epoch; the visible Retry action starts a fresh budget.
    const delays = [1000, 2000, 4000, 8000, 16000, 30000];
    const attempt = (index: number) => {
      if (!current()) return;
      void owner.querySetup().then((setup) => {
        if (current()) { nativeDemo = setup.demo; configureAlerts(); }
      }).catch(() => {
        // Unknown provenance must not authorize a wall-clock Today read.
        if (!current()) return;
        nativeDemo = null;
        configureAlerts();
        if (index < delays.length) setupRetry = setTimeout(() => attempt(index + 1), delays[index]);
      });
    };
    attempt(0);
  }
  function retrySetup() { if (nativeDemo === null && snapshot) readSetup(snapshot.stream_id, true); }
  async function changeDemo(enabled: boolean) {
    const owner = client;
    retireAlerts();
    invalidateSetup();
    try { await owner.setDemo(enabled); }
    finally {
      if (mounted && owner === connectedClient) {
        // A transition may publish its new stream before the command settles.
        // Recheck the final native flag, including failed transition cleanup.
        invalidateSetup();
        if (snapshot) { enterAlerts(snapshot.stream_id); observeAlertClock(snapshot.at); readSetup(snapshot.stream_id); }
      }
    }
  }
  function connect() {
    cache.clear();
    releaseHistory(); releaseHistoryVisuals();
    retireAlerts();
    releaseConnection();
    releaseCatalog();
    catalogAvailable = null;
    const owner = client;
    const readCatalog = () => {
      const epoch = ++catalogEpoch;
      void owner.queryCatalogAvailable().then(available => {
        if (mounted && epoch === catalogEpoch && owner === client) catalogAvailable = available;
      }).catch(() => { /* An unavailable query does not establish a missing catalog. */ });
    };
    releaseCatalog = owner.onVisualsChanged(readCatalog);
    releaseHistory = owner.onHistoryChanged(() => cache.invalidateAll());
    releaseHistoryVisuals = owner.onVisualsChanged(() => cache.invalidateAll());
    readCatalog();
    invalidateSetup();
    logsPending.reset(); pendingRevision++;
    acceptedEvent = null; historyReady = null; historyRequired = null; historyInitial = false; routeRequired = null;
    snapshot = null; sharedSnapshot.set(null); visuals = []; store = null;

    capture = null; sharedCapture.set(null);
    connectedClient = client; connectedPreview = preview;
    // Features are startup/preview configuration, not runtime settings. Like the
    // router, observation chooses its feature set when the connection is made.
    const homeRedesign = features.homeRedesign;
    releaseConnection = connectShell(client, {
      homeWindow: (next) => { currentHomeWindow = next; sharedHomeWindow.set(next); },
      homeWindowObserved: homeRedesign ? {
        onEvent: event => homeRecap.onEvent(event),
        onWindow: (next, observedSnapshot) => {
          currentHomeWindow = next;
          homeRecap.observe(observedSnapshot, currentHomeWindow);
        },
      } : undefined,
      historyInvalidates,
      historyBaseline: (next) => {
        if (next.stream_id !== '' && (historyRequired === null || BigInt(next.sequence) > BigInt(historyRequired))) {
          historyRequired = next.sequence; historyInitial = true; routeRequired = next.sequence;
        }
      },
      snapshot: (next) => {
        // Native demo transitions create a new accepted stream; read its mode once.
        readSetup(next.stream_id);
        enterAlerts(next.stream_id);
        observeAlertClock(next.at);
        const scope = `${preview ? 'demo' : 'live'}:${next.stream_id}`;
        if (observedStream === null) router?.bindScope(scope);
        else if (observedStream !== next.stream_id) {
          cache.clear();
          logsPending.reset(); pendingRevision++;
          acceptedEvent = null; historyReady = null; historyRequired = null; historyInitial = false; routeRequired = null;
          router?.setScope(scope);
        }
        if (next.stream_id !== '' && (observedStream !== next.stream_id || liveState.phase === 'resyncing' || historyRequired === null)) {
          historyRequired = next.sequence; historyInitial = true; routeRequired = next.sequence;
        }
        observedStream = next.stream_id;
        snapshot = next;
        characterSnapshot = next; updateCharacter();
        sharedSnapshot.set(next);
      },
      capture: (next) => {
        const changed = capture?.state !== next.state;
        if (changed) readCatalog();
        capture = next; sharedCapture.set(next); updateCharacter();
        if (changed) configureAlerts();
      },
      visuals: (next) => {
        if (next.client_fingerprint) {
          if (cacheFingerprint && cacheFingerprint !== next.client_fingerprint) cache.invalidateAll();
          cacheFingerprint = next.client_fingerprint;
        }
        visuals = next.objects; fingerprint = next.client_fingerprint;
      },
      store: (next) => { store = next; },
      event: (next) => {
        if (observeRail.onEvent(next)) { railSnapshot = null; accountRailRevision++; }
        cache.observe(next);
        // Home applies ordered events within the same presentation transaction.
        // Consume every occurrence before Svelte batches the latest-event prop.
        if (!('alert_fired' in next.event)) observeAlertClock(next.at);
        notifyEventObserver(next, event => alertController.accept(event));
        for (const receive of eventObservers) notifyEventObserver(next, receive);
        acceptedEvent = next;
        const route = router?.route, key = navigation?.entry.key;
        if (key && matchesPendingView(route, { logsPosition: router?.logsPosition ?? null }, next)) {
          logsPending.accept(key, next.sequence); pendingRevision++;
        }
        if (historyInvalidated(next) && (historyRequired === null || BigInt(next.sequence) > BigInt(historyRequired))) {
          historyRequired = next.sequence; historyInitial = false;
        }
        if ((route?.kind === 'home' ? invalidatesHomeHistory(next) : historyInvalidates(next))
          && (routeRequired === null || BigInt(next.sequence) > BigInt(routeRequired))) {
          routeRequired = next.sequence;
        }
      },
      historyReady: (next) => { historyReady = next; },
      liveState: (next) => {
        const changingEpoch = next.phase === 'connecting' && liveState.phase !== 'connecting';
        if (next.phase === 'resyncing' && liveState.phase !== 'resyncing') cache.invalidateAll();
        liveState = next;
        updateCharacter();
        if (changingEpoch) {
          cache.clear();
          retireAlerts();
          invalidateSetup();
          snapshot = null; sharedSnapshot.set(null); visuals = []; store = null;
          logsPending.reset(); pendingRevision++;
          acceptedEvent = null; historyReady = null; historyRequired = null; historyInitial = false; routeRequired = null;
        }
      },
      error: (source, message) => {
        errors[source] = message;
        if (!message) return;
        if (source === 'captureSubscription') { capture = null; alertController.configure(null, alertStream); }
        if (source === 'snapshot') { snapshot = null; sharedSnapshot.set(null); visuals = []; alertController.configure(null, alertStream); }
        if (source === 'store') store = null;
        updateCharacter();
      },
    }, { homeActive: router?.route.kind === 'home' });
  }
  async function action(start: boolean) {
    if (busy || preview) return;
    busy = true; errors.captureAction = '';
    try { await (start ? client.startCapture() : client.stopCapture()); }
    catch (error) { if (mounted) { pendingSummaries = String(error).includes('Pending summaries are not saved'); errors.captureAction = 'Capture failed. Retry.'; } }
    finally { if (mounted) busy = false; }
  }
  function save() {
    if (!pane || restoring) return;
    const row = focusedRow(pane);
    router?.save({ scrollTop: pane.scrollTop, ...(row ? { focusedRowId: row } : {}) });
  }
  function click(event: MouseEvent) {
    if (event.defaultPrevented || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    const anchor = event.target instanceof Element ? event.target.closest<HTMLAnchorElement>('a[href^="#/"]') : null;
    if (!anchor || anchor.target || anchor.hasAttribute('download')) return;
    event.preventDefault(); save();
    router?.navigate(parseRoute(anchor.hash, features));
  }
  async function focusRoute(next: NavigationState) {
    const request = ++transition;
    restoring = true; missingRow = false;
    // The shared heading belongs to the outgoing page until this entry is ready.
    if (pane?.contains(document.activeElement)) pane.focus({ preventScroll: true });
    try {
      await tick();
      if (!mounted || request !== transition) return;
      const ready = Promise.race([Promise.all([rowsReady(next.entry.key), routeRowsReady]), routeWaitExpired]);
      // Anchored history entries restore their destination, not a list row.
      if (next.restoration && !next.entry.anchor) {
        const missing = await restorePosition(pane, next.entry, ready, () => mounted && request === transition);
        if (mounted && request === transition) missingRow = missing;
      } else {
        await ready; await tick();
        if (!mounted || request !== transition) return;
        // A user who entered the new content during the wait owns its focus.
        if (!next.restoration && document.activeElement !== pane && pane.contains(document.activeElement)) return;
        const anchor = next.entry.anchor ? [...pane.querySelectorAll<HTMLElement>('[data-anchor]')].find((el) => el.dataset.anchor === next.entry.anchor) : null;
        (anchor ?? pane.querySelector<HTMLElement>('h1'))?.focus({ preventScroll: true });
        if (anchor) pane.scrollTop = Math.max(0, pane.scrollTop + anchor.getBoundingClientRect().top - pane.getBoundingClientRect().top);
        else pane.scrollTop = 0;
      }
    } catch {
      if (mounted && request === transition && (next.restoration || document.activeElement === pane || !pane.contains(document.activeElement))) {
        pane.querySelector<HTMLElement>('h1')?.focus({ preventScroll: true });
      }
    } finally {
      if (request === transition) { restoring = false; clearTimeout(rowsDeadline); rowsDeadline = undefined; }
    }
  }
  onMount(() => {
    mounted = true;
    const adapter = historyAdapter ?? browserHistory(window);
    if (initialSetupRequired && !preview) adapter.replace(null, '#/setup');
    const instance = createRouter(adapter, features, preview ? 'demo:initial' : 'live:initial');
    router = instance;
    let first = true;
    const unsubscribe = instance.subscribe((next) => {
      relationship = null;
      clearTimeout(rowsDeadline); resolveRouteRows(); expireRouteWait();
      routeRowsKey = next.entry.key;
      routeRowsReady = !content ? new Promise<void>((resolve) => { resolveRouteRows = resolve; }) : Promise.resolve();
      routeWaitExpired = new Promise<void>((resolve) => { expireRouteWait = resolve; });
      rowsDeadline = setTimeout(() => { rowsRendered(next.entry.key); expireRouteWait(); }, 5000);
      routeRequired = historyRequired;
      navigation = next;
      if (!first && next.explicit) void focusRoute(next);
      else if (first && next.entry.anchor) void focusRoute(next);
      else { transition++; restoring = false; missingRow = false; }
      first = false;
    });
    const releaseInput = installNavigationInput(window, {
      back: () => { save(); instance.back(); },
      forward: () => { save(); instance.forward(); },
    });
    const releasePersistence = installHistoryPersistence(window, { flush: () => { save(); instance.flush(); } });
    const scrollRestoration = window.history.scrollRestoration;
    window.history.scrollRestoration = 'manual';
    connect();
    return () => {
      mounted = false; retireAlerts(); dungeonMute.dispose(); alertController.dispose(); transition++; catalogEpoch++; releaseCatalog(); releaseHistory(); releaseHistoryVisuals(); invalidateSetup(); clearTimeout(rowsDeadline); resolveRouteRows(); expireRouteWait(); releaseConnection(); releaseInput(); releasePersistence(); unsubscribe(); instance.dispose();
      window.history.scrollRestoration = scrollRestoration;
    };
  });
</script>

<svelte:window onclick={click} />
<a class="skip-link" href="#main-content" onclick={(event) => { event.preventDefault(); pane.focus(); }}>Skip to content</a>
<div class="app-frame">
  <CharacterRail {view} {capture} missingCatalog={!preview && catalogAvailable === false && ['WaitingForGame', 'Live', 'Recovering'].includes(capture?.state ?? '')} resolveSpriteUrl={resolveVisualSprite} route={navigation?.entry.route ?? { kind: 'home' }} {features} {preview} {busy} error={captureError} snapshotError={errors.snapshot} onStart={() => action(true)} onStop={() => action(false)} onRetry={connect}
    canBack={navigation?.canBack ?? false} canForward={navigation?.canForward ?? false}
    back={() => { save(); router?.back(); }} forward={() => { save(); router?.forward(); }}
    statusExtra={railStatusExtra} />
  <div class="content-pane">
    <DungeonMuteFeedback mute={dungeonMute} />
    {#if navigation && router}
      <BannerHost moment={alertState.current} presentation={alertState.current?.loudness}
        view={alertState.current?.view} sessionKey={alertSessionKey} overflow={alertState.overflow} showAlertsAction {client}
        onPauseChange={(paused, hidden) => alertController.setPaused(paused, hidden)}
        onDismiss={() => { if (alertState.current) alertController.dismiss(alertState.current.id); }} />
    {/if}
  <main id="main-content" class:home-page={homeRoute} aria-label="Content" tabindex="-1" bind:this={pane} onscroll={save} onfocusin={save}>
    {#if navigation && router}
      {#if preview}<p class="preview-note">Preview</p>{/if}
      {#if liveState.phase === 'resyncing'}<DebugNote text="Some updates may be missing." />{/if}
      {#if unsavedClose}
        <div bind:this={unsavedDialog} role="alertdialog" tabindex="-1" aria-label="Unsaved changes" aria-describedby="unsaved-description">
          <p id="unsaved-description">Changes are not saved. Exiting without saving will discard pending summaries.</p>
          <button disabled={busy || preview} onclick={() => recoverStore()}>Retry saving</button>
          <button disabled={busy || preview} onclick={() => recoverStore(true)}>Exit without saving</button>
          <button disabled={busy} onclick={() => { unsavedClose = false; }}>Keep Maxed open</button>
        </div>
      {/if}
      {#if store?.state === 'failed' || pendingSummaries}
        {#if pendingSummaries || (store?.queued ?? 0) > 0}
          <p class="store-warning" role="status">Couldn't save. Retry.</p>
          {#if !unsavedClose}<button disabled={busy || preview} onclick={() => recoverStore()}>Retry saving</button>{/if}
        {:else if !unsavedClose}<p class="store-warning" role="status">Couldn't save. Storage is unavailable; use Start capture to try again.</p>{/if}
        <DebugNote text={`Latest durable sequence: ${store?.durable ?? "unavailable"}`} />
      {:else if liveState.historyFailed || errors.store}<p class="storage-note">Storage error</p>
      {/if}
      <h1 class:sr-only={homeRoute || ['runs', 'run', 'fight', 'player', 'run-player', 'loot', 'item', 'logs', 'alerts'].includes(navigation.entry.route.kind)} tabindex="-1">{routeTitle(navigation.entry.route)}</h1>
      <div class:home-content={homeRoute} class:route-content={!homeRoute}>
      {#key navigation.entry.key}
        {#if content}{@render content(navigation, router, { store, onRowsReady: rowsRendered })}
        {:else}<RouteFrame {pendingCount} onHistoryApplied={historyApplied} {features} route={navigation.entry.route} {snapshot} {client} {preview} {capture} {liveState} event={acceptedEvent} {historyReady} {store} required={routeRequired} initial={historyInitial}
          {syntheticClock} clockReady={preview || nativeDemo !== null}
          onClockRetry={retrySetup}
          onDemoChange={changeDemo}
          savedScope={navigation.entry.homeScope} onScopeChange={(homeScope) => router?.save({ homeScope })}
          savedState={navigation.entry} scopeKey={navigation.entry.scope} onStateChange={(patch) => router?.save(patch)}
          entryKey={navigation.entry.key} onRowsReady={rowsRendered} onRelationship={relationshipValidated} />{/if}
      {/key}
      </div>
    {/if}
    <p class="restoration-status" role="status" aria-live="polite" aria-atomic="true">{missingRow ? 'Page heading' : ''}</p>
  </main>
  </div>
  <ItemHoverCard {client} />
</div>

<BusyStatus message={liveState.phase === 'resyncing' ? 'Refreshing' : null} />
<BusyStatus message={historyRoute && liveState.historyLagging && store?.state !== 'failed' && !liveState.historyFailed && !errors.store && store ? 'Loading history' : null} />
<StatusOverlay registry={statusRegistry} />

{#snippet railStatusExtra()}
  {#if recordingIndicator}<span class="recording-indicator" class:recording-stopped={$recordingSession.status?.state === 'failed'} role="status"><a href={settingsHref('data')}>{recordingIndicator}</a></span>{/if}
{/snippet}

<style>
  .app-frame { height: 100vh; height: 100dvh; min-width: 0; display: grid; grid-template-columns: 15rem minmax(0, 1fr); overflow: hidden; background: var(--surface-canvas); }
  .content-pane { position: relative; min-width: 0; min-height: 0; display: grid; overflow: hidden; background: var(--surface-page); }
  main { min-width: 0; min-height: 0; overflow: auto; display: block; place-items: normal; text-align: left; padding: var(--space-8); background: var(--surface-page); }
  /* Home uses the space left by real, in-flow chrome. Overflow remains on main
     for expanded evidence, long names and smaller effective viewports. */
  main.home-page { display: flex; flex-direction: column; gap: var(--space-2); padding: var(--space-3); }
  .home-page > :global(*) { flex-shrink: 0; }
  .home-page .preview-note, .home-page .store-warning, .home-page .storage-note { margin: 0; }
  .home-content { flex: 1 0 auto; min-height: 0; min-width: 0; container-type: inline-size; }
  @media (min-width: 951px) {
    .home-page > .home-content:has(:global(.area-home)) { flex: 1 1 0; }
  }
  .route-content { display: contents; }
  .restoration-status:empty { margin: 0; }
  .recording-indicator { color: var(--text-secondary); font-size: var(--text-label); white-space: nowrap; }
  .recording-indicator a { color: inherit; text-underline-offset: 3px; }
  .recording-stopped { color: var(--warning); }
  h1 { padding: 0; margin: var(--space-6) 0; font-size: var(--text-hero); line-height: var(--leading-hero); font-weight: var(--weight-medium); }
  .preview-note, .store-warning, .storage-note { margin: var(--space-3) 0; font-size: var(--text-label); line-height: var(--leading-label); overflow-wrap: anywhere; }
  .preview-note, .storage-note { color: var(--text-secondary); }
  .store-warning { color: var(--warning); padding: var(--space-3); border: 1px solid var(--warning); border-radius: var(--radius-control); }
  .skip-link { position: fixed; z-index: 10; top: var(--space-2); left: var(--space-2); transform: translateY(-200%); border-radius: var(--radius-control); background: var(--surface-raised); color: var(--text-primary); padding: var(--space-3); }
  .skip-link:focus { transform: none; }
  @media (max-width: 900px) { .app-frame { grid-template-columns: 12rem minmax(0, 1fr); } main { padding: var(--space-4); } }
</style>
