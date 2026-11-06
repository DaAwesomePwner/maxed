export type StatsView = 'maps' | 'sessions' | 'dungeons';
export type LogsView = 'chat' | 'keys' | 'events' | 'alerts';
export type ChatTab = 'all' | 'guild' | 'party' | 'whisper';
export type LootView = 'feed' | 'stats' | 'collection';
export type Route =
  | { kind: 'stats'; view: StatsView; sessionId?: string }
  | { kind: 'character' | 'home' | 'runs' | 'alerts' | 'settings' | 'setup' | 'not-found' }
  | { kind: 'logs'; view: LogsView; channel: ChatTab; runId?: string; partner?: string; seedSender?: string }
  | { kind: 'loot'; view: LootView; seed?: { place?: string; item?: number } }
  | { kind: 'run'; id: string; point?: 'death' }
  | { kind: 'fight'; id: string; runId?: string }
  | { kind: 'player'; fightId: string; objectId: number; runId?: string }
  | { kind: 'run-player'; runId: string; playerId: string }
  | { kind: 'party-live' | 'party-players' | 'party-presets' }
  | { kind: 'party-run'; runId: string }
  | { kind: 'party-player'; key: string }
  | { kind: 'item'; type: number };

export interface Features { lootBrowsing: boolean; logs: boolean; stats: boolean; party: boolean; homeRedesign: boolean; character: boolean }
/** Saved relationship reported by the current detail route, never inferred from its URL. */
export interface FightRelationship {
  entryKey: string;
  scopeKey: string;
  streamId: string | null;
  fightId: string | null;
  runId: string | null;
}
export const productionFeatures: Features = { lootBrowsing: true, logs: true, stats: true, party: true, homeRedesign: true, character: true };
const validId = (id: string) => id.length > 0 && id.length <= 256 && !/[\u0000-\u001f\u007f]/.test(id);
const validName = (name: string) => name.length > 0 && new TextEncoder().encode(name).length <= 32 && !/[\u0000-\u001f\u007f-\u009f]/.test(name);
const numeric = (id: string, max: number) => /^\d+$/.test(id) && Number.isSafeInteger(Number(id)) && Number(id) <= max;

/** Object IDs are fight-scoped signed i32 values with exactly one decimal spelling. */
export function parseObjectId(value: string): number | null {
  if (!/^(?:0|[1-9]\d*|-[1-9]\d*)$/.test(value)) return null;
  const id = Number(value);
  return Number.isInteger(id) && id >= -2147483648 && id <= 2147483647 ? id : null;
}

export function routeHref(route: Route): string {
  const encode = encodeURIComponent;
  switch (route.kind) {
    case 'logs': {
      const run = route.runId ? `/run/${encode(route.runId)}` : '';
      if (route.view === 'chat') {
        if (route.seedSender !== undefined) return `#/logs/from/${encode(route.seedSender)}`;
        if (route.partner !== undefined) return `#/logs/chat/whisper/${encode(route.partner)}`;
        return route.channel === 'all' ? `#/logs${run}` : `#/logs/chat/${route.channel}${run}`;
      }
      return `#/logs/${route.view}${route.view === 'events' ? run : ''}`;
    }
    case 'stats':
      if (route.view === 'sessions' && route.sessionId) return `#/stats/sessions/${encode(route.sessionId)}`;
      return route.view === 'maps' ? '#/stats' : `#/stats/${route.view}`;
    case 'home': return '#/';
    case 'party-live': return '#/party';
    case 'party-run': return `#/party/runs/${encode(route.runId)}`;
    case 'party-players': return '#/party/players';
    case 'party-player': return `#/party/players/${encode(route.key)}`;
    case 'party-presets': return '#/party/presets';
    case 'run': return `#/runs/${encode(route.id)}${route.point ? `/${route.point}` : ''}`;
    case 'run-player': return `#/runs/${encode(route.runId)}/players/${encode(route.playerId)}`;
    case 'fight': return `${route.runId ? `#/runs/${encode(route.runId)}` : '#'}/fights/${encode(route.id)}`;
    case 'player': return `${routeHref({ kind: 'fight', id: route.fightId, runId: route.runId })}/players/${route.objectId}`;
    case 'loot':
      if (route.view === 'feed' && route.seed?.place !== undefined) return `#/loot/in/${encode(route.seed.place)}`;
      if (route.view === 'feed' && route.seed?.item !== undefined) return `#/loot/with/${route.seed.item}`;
      return route.view === 'feed' ? '#/loot' : `#/loot/${route.view}`;
    case 'item': return `#/loot/item/${route.type}`;
    default: return `#/${route.kind}`;
  }
}

export function parseRoute(hash: string, features: Features = productionFeatures): Route {
  try {
    if (hash === '' || hash === '#' || hash === '#/') return { kind: 'home' };
    if (!hash.startsWith('#/')) return { kind: 'not-found' };
    const parts = hash.slice(2).split('/').map(decodeURIComponent);
    if (!parts.every(validId)) return { kind: 'not-found' };
    const [root, id] = parts;
    if (root === 'character' && parts.length === 1) return { kind: features.character ? 'character' : 'home' };
    if (root === 'party') {
      if (!features.party) return { kind: 'home' };
      if (parts.length === 1) return { kind: 'party-live' };
      if (parts.length === 2 && id === 'players') return { kind: 'party-players' };
      if (parts.length === 2 && id === 'presets') return { kind: 'party-presets' };
      if (parts.length === 3 && id === 'runs') return { kind: 'party-run', runId: parts[2] };
      if (parts.length === 3 && id === 'players') return { kind: 'party-player', key: parts[2] };
    }
    if (features.logs && parts.length === 1 && root === 'alerts') return { kind: 'logs', view: 'alerts', channel: 'all' };
    if (root === 'logs' && features.logs) {
      if (parts.length === 3 && id === 'from' && validName(parts[2])) return { kind: 'logs', view: 'chat', channel: 'all', seedSender: parts[2] };
      if (parts.length === 4 && id === 'chat' && parts[2] === 'whisper' && validName(parts[3])) return { kind: 'logs', view: 'chat', channel: 'whisper', partner: parts[3] };
      if (parts.length === 5 && id === 'chat' && ['guild', 'party', 'whisper'].includes(parts[2]) && parts[3] === 'run') return { kind: 'logs', view: 'chat', channel: parts[2] as ChatTab, runId: parts[4] };
      if (parts.length === 1) return { kind: 'logs', view: 'chat', channel: 'all' };
      if (parts.length === 3 && id === 'chat' && ['guild', 'party', 'whisper'].includes(parts[2])) return { kind: 'logs', view: 'chat', channel: parts[2] as ChatTab };
      if (parts.length === 3 && id === 'run') return { kind: 'logs', view: 'chat', channel: 'all', runId: parts[2] };
      if (parts.length === 2 && (id === 'keys' || id === 'events' || id === 'alerts')) return { kind: 'logs', view: id, channel: 'all' };
      if (parts.length === 4 && id === 'events' && parts[2] === 'run') return { kind: 'logs', view: 'events', channel: 'all', runId: parts[3] };
    }
    if (root === 'stats' && features.stats) {
      if (parts.length === 1) return { kind: 'stats', view: 'maps' };
      if (parts.length === 2 && (id === 'sessions' || id === 'dungeons')) return { kind: 'stats', view: id };
      if (parts.length === 3 && id === 'sessions') return { kind: 'stats', view: 'sessions', sessionId: parts[2] };
    }
    if (parts.length === 1 && ['home', 'runs', 'alerts', 'settings', 'setup'].includes(root)) return { kind: root as 'home' | 'runs' | 'alerts' | 'settings' | 'setup' };
    if (root === 'loot' && features.lootBrowsing) {
      if (parts.length === 3 && id === 'in') return { kind: 'loot', view: 'feed', seed: { place: parts[2] } };
      if (parts.length === 3 && id === 'with' && numeric(parts[2], 65535)) return { kind: 'loot', view: 'feed', seed: { item: Number(parts[2]) } };
      if (parts.length === 1) return { kind: 'loot', view: 'feed' };
      if (parts.length === 2 && (id === 'stats' || id === 'collection')) return { kind: 'loot', view: id };
      if (parts.length === 3 && id === 'item' && numeric(parts[2], 65535)) return { kind: 'item', type: Number(parts[2]) };
      if (parts.length === 2 && numeric(id, 65535)) return { kind: 'item', type: Number(id) };
    }
    if (root === 'runs' && parts.length === 2) return { kind: 'run', id };
    if (parts.length === 4 && root === 'runs' && parts[2] === 'players') return { kind: 'run-player', runId: id, playerId: parts[3] };
    if (root === 'runs' && parts.length === 3 && parts[2] === 'death') return { kind: 'run', id, point: 'death' };
    const nested = root === 'runs' && parts[2] === 'fights';
    const fight = nested ? parts.slice(2) : parts;
    const runId = nested ? id : undefined;
    if (fight[0] === 'fights') {
      if (fight.length === 2) return { kind: 'fight', id: fight[1], runId };
      if (fight.length === 4 && fight[2] === 'players') {
        const objectId = parseObjectId(fight[3]);
        if (objectId !== null) return { kind: 'player', fightId: fight[1], objectId, runId };
      }
    }
  } catch { /* Malformed percent encoding is a recoverable address. */ }
  return { kind: 'not-found' };
}

export function routeTitle(route: Route): string {
  if (route.kind === 'party-live' || route.kind === 'party-run' || route.kind === 'party-players' || route.kind === 'party-player' || route.kind === 'party-presets') return 'Party';
  return ({ character: 'Character', stats: 'Stats', home: 'Home', runs: 'Runs', run: 'Run', fight: 'Fight', player: 'Player', 'run-player': 'Player', loot: 'Loot', item: 'Item', alerts: 'Alerts', logs: 'Logs', settings: 'Settings', setup: 'Setup', 'not-found': 'Page Not Found' })[route.kind];
}

export function breadcrumbs(route: Route, dungeon?: string): { label: string; href?: string }[] {
  const result = [{ label: 'Home', href: '#/' }];
  if (route.kind === 'home') return result;
  if (route.kind.startsWith('party-')) {
    result.push({ label: 'Party', href: '#/party' });
    if (route.kind === 'party-run') result.push(
      { label: dungeon ?? 'Run', href: routeHref({ kind: 'run', id: route.runId }) },
      { label: 'Party run', href: routeHref(route) },
    );
    return result;
  }
  if (['run', 'fight', 'player', 'run-player'].includes(route.kind)) result.push({ label: 'Runs', href: '#/runs' });
  if ((route.kind === 'fight' || route.kind === 'player' || route.kind === 'run-player') && route.runId) result.push({ label: 'Run', href: routeHref({ kind: 'run', id: route.runId }) });
  if (route.kind === 'player') result.push({ label: 'Fight', href: routeHref({ kind: 'fight', id: route.fightId, runId: route.runId }) });
  if (route.kind === 'item' || (route.kind === 'loot' && route.view !== 'feed')) result.push({ label: 'Loot', href: '#/loot' });
  const logsChild = route.kind === 'logs' && !(route.view === 'chat' && route.channel === 'all' && !route.runId && !route.seedSender);
  if (logsChild) result.push({ label: 'Logs', href: '#/logs' });
  const logsLabels: Record<LogsView, string> = { chat: 'Chat', keys: 'Key Pops', events: 'Events', alerts: 'Alerts' };
  const statsChild = route.kind === 'stats' && route.view !== 'maps';
  if (statsChild) result.push({ label: 'Stats', href: '#/stats' });
  if (route.kind === 'stats' && route.sessionId) result.push({ label: 'Sessions', href: '#/stats/sessions' });
  const label = route.kind === 'stats' ? (route.sessionId ? 'Session' : route.view === 'sessions' ? 'Sessions' : route.view === 'dungeons' ? 'Dungeons' : 'Stats') : logsChild ? logsLabels[route.view] : route.kind === 'loot' && route.view !== 'feed' ? (route.view === 'stats' ? 'Drop rates' : 'Collection') : routeTitle(route);
  result.push({ label, href: routeHref(route) });
  return result;
}
