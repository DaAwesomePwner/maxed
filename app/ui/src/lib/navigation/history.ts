import { defaultLogsPosition, emptyListState, normalizeListState, type ListState } from './state';
import { parseRoute, productionFeatures, routeHref, type Features, type Route } from './routes';

export interface HistoryAdapter {
  readonly hash: string;
  readonly state: unknown;
  push(state: unknown, hash: string): void;
  replace(state: unknown, hash: string): void;
  go(delta: number): void;
  listen(receive: () => void): () => void;
}
export function browserHistory(browser: Window): HistoryAdapter {
  return {
    get hash() { return browser.location.hash; },
    get state() { return browser.history.state; },
    push: (state, hash) => browser.history.pushState(state, '', hash),
    replace: (state, hash) => browser.history.replaceState(state, '', hash),
    go: (delta) => browser.history.go(delta),
    listen: (receive) => { browser.addEventListener('popstate', receive); return () => browser.removeEventListener('popstate', receive); },
  };
}
export interface HistoryEntry extends ListState {
  owner: string;
  scope: string;
  key: string;
  index: number;
  route: Route;
  anchor: string | null;
}
export interface NavigationState { entry: HistoryEntry; canBack: boolean; canForward: boolean; restoration: boolean; revision: number; explicit: boolean }

// History records contain JSON values only. This also accepts Svelte reactive proxies.
const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value));

export function createRouter(adapter: HistoryAdapter, features: Features = productionFeatures, initialScope = 'live:pending') {
  const owner = crypto.randomUUID();
  let scope = initialScope;
  const scopeAliases = new Set([initialScope]);
  let scopeBound = false;
  let index = 0;
  let last = 0;
  let revision = 0;
  let disposed = false;
  let traversing = false;
  let restoration = false;
  let explicit = false;
  let dirty = false;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  const cache = new Map<string, HistoryEntry>();
  const listeners = new Set<(state: NavigationState) => void>();
  const routeAnchor = (route: Route) => route.kind === 'run' && route.point === 'death' ? 'death' : null;
  const seedLogs = (route: Route, position = defaultLogsPosition(), keepWhisper = false) => {
    if (route.kind !== 'logs') return position;
    if (route.view === 'chat' && route.partner === undefined) {
      position.chat.filter.channel = route.channel === 'all' ? null : route.channel;
      position.chat.filter.run_id = route.runId ?? null;
    }
    if (route.view === 'events') position.events.filter.run_id = route.runId ?? null;
    if (route.seedSender !== undefined) position.chat = { ...defaultLogsPosition().chat,
      filter: { ...defaultLogsPosition().chat.filter, sender: route.seedSender } };
    if (route.partner !== undefined) position.whisper = { ...(keepWhisper ? position.whisper : defaultLogsPosition().whisper), partner: route.partner };
    return position;
  };
  const make = (route: Route, anchor: string | null = routeAnchor(route)): HistoryEntry => ({ ...emptyListState(),
    ...(route.kind === 'logs' ? { logsPosition: seedLogs(route) } : {}), owner, scope, key: crypto.randomUUID(), index, route, anchor });
  let entry = make(parseRoute(adapter.hash, features));
  const remember = () => {
    cache.delete(entry.key);
    cache.set(entry.key, copy(entry));
    while (cache.size > 50) cache.delete(cache.keys().next().value!);
  };
  const state = (): NavigationState => ({ entry: copy(entry), canBack: index > 0, canForward: index < last, restoration, revision, explicit });
  const publish = () => { remember(); for (const listener of listeners) listener(state()); };
  const cancelPendingSave = () => { clearTimeout(saveTimer); saveTimer = undefined; dirty = false; };
  const flush = () => {
    if (disposed || traversing || !dirty) return;
    adapter.replace(entry, routeHref(entry.route));
    cancelPendingSave();
  };
  adapter.replace(entry, routeHref(entry.route));
  remember();
  const unlisten = adapter.listen(() => {
    if (disposed) return;
    // Native traversal has already moved the browser pointer. The outgoing record
    // stays fresh in cache; never write its pending save into the destination.
    cancelPendingSave();
    traversing = false;
    const target = adapter.state as Partial<HistoryEntry> | null;
    if (target?.owner === owner && target.scope && scopeAliases.has(target.scope) && typeof target.index === 'number' && target.index >= 0 && target.index <= last && target.key) {
      entry = { ...copy(cache.get(target.key) ?? target as HistoryEntry), scope };
      entry = { ...entry, ...normalizeListState(entry) };
      index = entry.index;
      adapter.replace(entry, routeHref(entry.route));
      restoration = true;
    } else {
      // Retired demo/stream entries never restore their routes, filters or focus.
      index = 0; last = 0; cache.clear();
      entry = make({ kind: 'home' });
      adapter.replace(entry, '#/');
      restoration = false;
    }
    explicit = true; revision++;
    publish();
  });
  return {
    get state() { return state(); },
    get route(): Readonly<Route> { return entry.route; },
    // Live envelope predicates need current filters without cloning an entire entry.
    get logsPosition() { return entry.logsPosition; },
    get cacheSize() { return cache.size; },
    flush,
    subscribe(receive: (state: NavigationState) => void) { listeners.add(receive); receive(state()); return () => { listeners.delete(receive); }; },
    replaceInitial(route: Route) {
      // An asynchronous startup hint must never undo navigation made meanwhile.
      if (disposed || traversing || revision !== 0 || entry.route.kind !== 'home') return;
      cancelPendingSave();
      entry = make(parseRoute(routeHref(route), features));
      adapter.replace(entry, routeHref(entry.route));
      revision++; publish();
    },
    save(patch: Partial<ListState>) {
      if (disposed || traversing) return;
      entry = { ...entry, ...copy(patch) };
      remember();
      dirty = true;
      clearTimeout(saveTimer);
      saveTimer = setTimeout(flush, 250);
    },
    navigate(route: Route, options: { anchor?: string } = {}) {
      if (disposed || traversing) return;
      flush();
      const safe = parseRoute(routeHref(route), features);
      const sharedLootRange = (entry.route.kind === 'loot' || entry.route.kind === 'item') && (safe.kind === 'item' || safe.kind === 'loot')
        ? normalizeListState(entry).lootPosition?.range : undefined;
      const sharedLogs = (entry.route.kind === 'logs' || entry.route.kind === 'alerts') && safe.kind === 'logs'
        ? normalizeListState(entry).logsPosition : null;
      if (sharedLogs) {
        for (const slot of [sharedLogs.chat, sharedLogs.keys, sharedLogs.events]) {
          slot.filter.cursor = null; slot.previous = [];
        }
        sharedLogs.chat.selectedId = null; sharedLogs.events.selectedId = null; sharedLogs.keys.selectedId = null;
        if (entry.route.kind === 'logs' && entry.route.seedSender) sharedLogs.chat.filter.sender = null;
        sharedLogs.whisper.partner = null;
      }
      const keepWhisper = entry.route.kind === 'logs' && entry.route.view === 'chat'
        && entry.route.channel === 'whisper' && !entry.route.runId
        && safe.kind === 'logs' && safe.view === 'chat' && safe.channel === 'whisper' && !safe.runId;
      index++; last = index;
      entry = make(safe, options.anchor ?? routeAnchor(safe));
      if (sharedLootRange !== undefined) {
        entry.lootPosition = normalizeListState({ lootPosition: { range: sharedLootRange } }).lootPosition;
      }
      if (sharedLogs) entry.logsPosition = seedLogs(safe, sharedLogs, keepWhisper);
      adapter.push(entry, routeHref(safe));
      restoration = false; explicit = true; revision++; publish();
    },
    back() { if (!disposed && !traversing) { flush(); if (index > 0) { traversing = true; adapter.go(-1); } } },
    forward() { if (!disposed && !traversing) { flush(); if (index < last) { traversing = true; adapter.go(1); } } },
    // Bind the first asynchronous snapshot without discarding navigation made while loading.
    bindScope(next: string) {
      if (scopeBound || disposed) return;
      scopeBound = true; scope = next; scopeAliases.add(next);
      entry = { ...entry, scope };
      for (const [key, cached] of cache) cache.set(key, { ...cached, scope });
      adapter.replace(entry, routeHref(entry.route));
      cancelPendingSave();
    },
    setScope(next: string) {
      if (next === scope || disposed) return;
      cancelPendingSave();
      scope = next; index = 0; last = 0; cache.clear(); traversing = false;
      scopeAliases.clear(); scopeAliases.add(next); scopeBound = false;
      entry = make({ kind: 'home' });
      adapter.replace(entry, '#/'); restoration = false; explicit = false; revision++; publish();
    },
    dispose() { if (disposed) return; flush(); cancelPendingSave(); disposed = true; unlisten(); cache.clear(); listeners.clear(); },
  };
}
export type Router = ReturnType<typeof createRouter>;

export function installHistoryPersistence(target: Window, router: Pick<Router, 'flush'>) {
  const flush = () => router.flush();
  target.document.addEventListener('visibilitychange', flush);
  target.addEventListener('pagehide', flush);
  return () => {
    target.document.removeEventListener('visibilitychange', flush);
    target.removeEventListener('pagehide', flush);
  };
}

function editable(target: EventTarget | null): boolean {
  return target instanceof Element && !!target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])');
}
/** Native mouse handling owns side buttons unless release WebView2 QA proves a fallback necessary. */
export function installNavigationInput(target: Window, router: Pick<Router, 'back' | 'forward'>, mouseOwner: 'native' | 'fallback' = 'native') {
  const keydown = (event: KeyboardEvent) => {
    if (event.defaultPrevented || event.isComposing || event.repeat || editable(event.target) || !event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
    event.preventDefault();
    event.key === 'ArrowLeft' ? router.back() : router.forward();
  };
  const mouseup = (event: MouseEvent) => {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || (event.button !== 3 && event.button !== 4)) return;
    event.preventDefault();
    event.button === 3 ? router.back() : router.forward();
  };
  target.addEventListener('keydown', keydown);
  if (mouseOwner === 'fallback') target.addEventListener('mouseup', mouseup);
  return () => { target.removeEventListener('keydown', keydown); if (mouseOwner === 'fallback') target.removeEventListener('mouseup', mouseup); };
}
