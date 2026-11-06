import { isPresentationVisible, observePresentationVisibility } from '../bridge/visibility';
import type { EventSummary } from '../bindings/EventSummary';
import type { EventSummaryRequest } from '../bindings/EventSummaryRequest';
import type { ChatOverviewRequest } from '../bindings/ChatOverviewRequest';
import type { ChatOverview } from '../bindings/ChatOverview';
import type { ChatSenderRequest } from '../bindings/ChatSenderRequest';
import type { ChatSenderSummary } from '../bindings/ChatSenderSummary';
import type { WhisperPartnerFilter } from '../bindings/WhisperPartnerFilter';
import type { WhisperPartners } from '../bindings/WhisperPartners';
import type { DropSummaryRequest } from '../bindings/DropSummaryRequest';
import type { DropSummary } from '../bindings/DropSummary';
import type { StatsFilter } from '../bindings/StatsFilter';
import type { StatsMaps } from '../bindings/StatsMaps';
import type { StatsSessionFilter } from '../bindings/StatsSessionFilter';
import type { StatsSessionPage } from '../bindings/StatsSessionPage';
import type { StatsSessionRequest } from '../bindings/StatsSessionRequest';
import type { StatsSessionDetail } from '../bindings/StatsSessionDetail';
import type { StatsDungeons } from '../bindings/StatsDungeons';
import type { ChatFilter } from '../bindings/ChatFilter';
import type { ChatPage } from '../bindings/ChatPage';
import type { EventFilter } from '../bindings/EventFilter';
import type { EventPage } from '../bindings/EventPage';
import type { DropFilter } from '../bindings/DropFilter';
import type { DropPage } from '../bindings/DropPage';
import type { ItemHistoryRequest } from '../bindings/ItemHistoryRequest';
import type { ItemHistory } from '../bindings/ItemHistory';
import type { LootStatsFilter } from '../bindings/LootStatsFilter';
import type { LootStats } from '../bindings/LootStats';
import type { CollectionRequest } from '../bindings/CollectionRequest';
import type { Collection } from '../bindings/Collection';
import type { Client } from '../bridge/client';
import type { RunFilter } from '../bindings/RunFilter';
import type { RunPage } from '../bindings/RunPage';
import type { RunStory } from '../bindings/RunStory';
import type { FightView } from '../bindings/FightView';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import type { RunPlayerView } from '../bindings/RunPlayerView';
import type { HistoryReply } from '../bindings/HistoryReply';
import type { StoreStatus } from '../bindings/StoreStatus';
import { historyControl, historyFailure, historyWaiting } from './policy';
import { routeHref } from '../navigation/routes';
import type { HistoryCache } from './history-cache';

export type HistoryQuery =
  | { kind: 'event-summary'; request: EventSummaryRequest }
  | { kind: 'chat-overview'; request: ChatOverviewRequest }
  | { kind: 'chat-sender'; request: ChatSenderRequest }
  | { kind: 'whisper-partners'; filter: WhisperPartnerFilter }
  | { kind: 'stats-maps'; filter: StatsFilter }
  | { kind: 'stats-sessions'; filter: StatsSessionFilter }
  | { kind: 'stats-session'; request: StatsSessionRequest }
  | { kind: 'stats-dungeons'; filter: StatsFilter }
  | { kind: 'chat'; filter: ChatFilter }
  | { kind: 'events'; filter: EventFilter }
  | { kind: 'runs'; filter: RunFilter }
  | { kind: 'run'; id: string }
  | { kind: 'fight'; id: string }
  | { kind: 'player'; fightId: string; objectId: number }
  | { kind: 'run-player'; runId: string; playerId: string }
  | { kind: 'drop-summary'; request: DropSummaryRequest }
  | { kind: 'drops'; filter: DropFilter }
  | { kind: 'item-history'; request: ItemHistoryRequest }
  | { kind: 'loot-stats'; filter: LootStatsFilter }
  | { kind: 'collection'; request: CollectionRequest };
export type HistoryValue =
  | { kind: 'event-summary'; summary: EventSummary }
  | { kind: 'chat-overview'; overview: ChatOverview }
  | { kind: 'chat-sender'; sender: ChatSenderSummary | null }
  | { kind: 'whisper-partners'; partners: WhisperPartners }
  | { kind: 'stats-maps'; maps: StatsMaps }
  | { kind: 'stats-sessions'; page: StatsSessionPage }
  | { kind: 'stats-session'; detail: StatsSessionDetail | null }
  | { kind: 'stats-dungeons'; dungeons: StatsDungeons }
  | { kind: 'chat'; page: ChatPage }
  | { kind: 'events'; page: EventPage }
  | { kind: 'runs'; page: RunPage }
  | { kind: 'run'; story: RunStory | null }
  | { kind: 'fight'; fight: FightView | null }
  | { kind: 'player'; player: PlayerDamage | null }
  | { kind: 'run-player'; player: RunPlayerView | null }
  | { kind: 'drops'; page: DropPage }
  | { kind: 'item-history'; history: ItemHistory }
  | { kind: 'loot-stats'; stats: LootStats }
  | { kind: 'drop-summary'; summary: DropSummary }
  | { kind: 'collection'; collection: Collection };
export interface HistoryInput {
  query: HistoryQuery; scopeKey: string; streamId: string | null;
  required: string | null; store: StoreStatus | null;
}
export interface HistoryState {
  phase: 'loading' | 'ready' | 'stale' | 'superseded' | 'busy' | 'error';
  value: HistoryValue | null; message: string; retryAvailable: boolean;
}
export interface HistoryController { update(input: HistoryInput): void; retry(): void; dispose(): void }
type HistoryClient = Pick<Client, 'getEventSummary' | 'getChatOverview' | 'getChatSender' | 'listWhisperPartners' | 'getStatsMaps' | 'listStatsSessions' | 'getStatsSession' | 'getStatsDungeons' | 'listChat' | 'listInstanceEvents' | 'listRuns' | 'getRun' | 'getRunFightStrip' | 'getFight' | 'getPlayerBreakdown' | 'getRunPlayer' | 'listDrops' | 'getItemHistory' | 'getLootStats' | 'getCollection' | 'getDropSummary'>;
async function read(client: HistoryClient, query: HistoryQuery): Promise<HistoryReply<HistoryValue>> {
  const wrap = <T>(reply: HistoryReply<T>, value: (result: T) => HistoryValue): HistoryReply<HistoryValue> =>
    reply.status === 'ready' ? { status: 'ready', value: value(reply.value) } : reply;
  switch (query.kind) {
    case 'event-summary': return wrap(await client.getEventSummary(query.request), summary => ({ kind: 'event-summary', summary }));
    case 'chat-overview': return wrap(await client.getChatOverview(query.request), overview => ({ kind: 'chat-overview', overview }));
    case 'chat-sender': return wrap(await client.getChatSender(query.request), sender => ({ kind: 'chat-sender', sender }));
    case 'whisper-partners': return wrap(await client.listWhisperPartners(query.filter), partners => ({ kind: 'whisper-partners', partners }));
    case 'stats-maps': return wrap(await client.getStatsMaps(query.filter), maps => ({ kind: 'stats-maps', maps }));
    case 'stats-sessions': return wrap(await client.listStatsSessions(query.filter), page => ({ kind: 'stats-sessions', page }));
    case 'stats-session': return wrap(await client.getStatsSession(query.request), detail => ({ kind: 'stats-session', detail }));
    case 'stats-dungeons': return wrap(await client.getStatsDungeons(query.filter), dungeons => ({ kind: 'stats-dungeons', dungeons }));

    case 'chat': return wrap(await client.listChat(query.filter), page => ({ kind: 'chat', page }));
    case 'events': return wrap(await client.listInstanceEvents(query.filter), page => ({ kind: 'events', page }));
    case 'drops': return wrap(await client.listDrops(query.filter), page => ({ kind: 'drops', page }));
    case 'item-history': return wrap(await client.getItemHistory(query.request), history => ({ kind: 'item-history', history }));
    case 'loot-stats': return wrap(await client.getLootStats(query.filter), stats => ({ kind: 'loot-stats', stats }));
    case 'drop-summary': return wrap(await client.getDropSummary(query.request), summary => ({ kind: 'drop-summary', summary }));
    case 'collection': return wrap(await client.getCollection(query.request), collection => ({ kind: 'collection', collection }));
    case 'runs': return wrap(await client.listRuns(query.filter), page => ({ kind: 'runs', page }));
    case 'run': return wrap(await client.getRun(query.id), story => ({ kind: 'run', story }));
    case 'fight': return wrap(await client.getFight(query.id), fight => ({ kind: 'fight', fight }));
    case 'player': return wrap(await client.getPlayerBreakdown(query.fightId, query.objectId), player => ({ kind: 'player', player }));
    case 'run-player': return wrap(await client.getRunPlayer(query.runId, query.playerId), player => ({ kind: 'run-player', player }));
  }
}

export function historyKey(input: HistoryInput): string {
  // Canonical argument order avoids object-property order becoming query identity.
  const q = input.query;
  const queryKey = q.kind === 'runs' ? [q.kind, q.filter.dungeon, q.filter.outcome, q.filter.character_id ?? null, q.filter.range?.start,
    q.filter.range?.end, q.filter.sort, q.filter.cursor?.sort_value, q.filter.cursor?.id, q.filter.limit]
    : q.kind === 'event-summary' ? [q.kind, q.request.filter.run_id, q.request.filter.range?.start, q.request.filter.range?.end]
    : q.kind === 'chat-overview' ? [q.kind, q.request.run_id ?? null]
    : q.kind === 'chat-sender' ? [q.kind, q.request.name]
    : q.kind === 'whisper-partners' ? [q.kind, q.filter.query ?? '', q.filter.show_hidden ?? false, q.filter.limit]
    : q.kind === 'chat' ? [q.kind, q.filter.channel, q.filter.sender, q.filter.query, q.filter.run_id,
      q.filter.show_npc, q.filter.show_hidden, q.filter.cursor?.at, q.filter.cursor?.id, q.filter.limit]
    : q.kind === 'events' ? [q.kind, q.filter.kinds, q.filter.run_id, q.filter.dungeon, q.filter.range?.start, q.filter.range?.end, q.filter.query ?? '', q.filter.opener_local ?? null,
      q.filter.cursor?.at, q.filter.cursor?.id, q.filter.limit]
    : q.kind === 'drops' ? [q.kind, q.filter.min_rank, q.filter.place?.kind,
      q.filter.place?.kind === 'dungeon' ? q.filter.place.name : null, q.filter.item_type,
      q.filter.enchanted_only, q.filter.range?.start, q.filter.range?.end,
      q.filter.cursor?.at, q.filter.cursor?.id, q.filter.limit]
    : q.kind === 'drop-summary' ? [q.kind, q.request.filter.min_rank, q.request.filter.place?.kind,
      q.request.filter.place?.kind === 'dungeon' ? q.request.filter.place.name : null, q.request.filter.item_type,
      q.request.filter.enchanted_only, q.request.filter.range?.start, q.request.filter.range?.end,
      q.request.days.map(day => [day.start, day.end]), q.request.include_items !== false]
    : q.kind === 'item-history' ? [q.kind, q.request.item_type, q.request.cursor?.at, q.request.cursor?.id, q.request.limit]
    : q.kind === 'loot-stats' ? [q.kind, q.filter.range?.start, q.filter.range?.end]
    : q.kind === 'collection' ? [q.kind, q.request.dungeon,
      q.request.chase.map(item => [item.item_type, item.dungeon, item.pinned_at])]
    : q.kind === 'stats-maps' || q.kind === 'stats-dungeons'
      ? [q.kind, q.filter.range?.start, q.filter.range?.end, q.filter.character_id]
    : q.kind === 'stats-sessions' ? [q.kind, q.filter.range?.start, q.filter.range?.end, q.filter.character_id,
      q.filter.include_empty, q.filter.cursor?.started, q.filter.cursor?.id, q.filter.limit]
    : q.kind === 'stats-session' ? [q.kind, q.request.id, q.request.character_id]
    : q.kind === 'run-player' ? [q.kind, q.runId, q.playerId]
    : q.kind === 'player' ? [q.kind, q.fightId, q.objectId] : [q.kind, q.id];
  return JSON.stringify([input.scopeKey, input.streamId, queryKey]);
}

/** One read and one fixed outstanding checkpoint; newer events cannot move its gate. */
export function createRunHistory(client: HistoryClient, receive: (state: HistoryState) => void, options?: { cache?: HistoryCache }): HistoryController {
  type Checkpoint = { required: string | null; version: number };
  const route = options?.cache?.route();
  const cache = route ? options?.cache : undefined;
  let input: HistoryInput | null = null;
  let identity = '', epoch = 0, version = 0;
  let value: HistoryValue | null = null;
  let pending: Checkpoint | null = null;
  let hitRequired: string | null | undefined;
  let latest: Checkpoint = { required: null, version: 0 };
  let reservation: number | undefined;
  const endRead = () => { if (reservation !== undefined) cache?.end(reservation); reservation = undefined; };
  let active = false, disposed = false, retryUsed = false;
  let control: 'superseded' | 'busy' | 'error' | null = null;
  let failure = '';
  let lastStart = -Infinity;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let emitted: HistoryState | undefined;
  const emit = (phase: HistoryState['phase'], message = '', retryAvailable = !['loading', 'superseded'].includes(phase)) => {
    if (disposed || (emitted?.phase === phase && emitted.value === value && emitted.message === message && emitted.retryAvailable === retryAvailable)) return;
    emitted = { phase, value, message, retryAvailable }; receive(emitted);
  };
  const refreshing = () => emit(value ? 'stale' : 'loading', 'Refreshing saved observations.', false);
  const gate = (next: HistoryInput) => pending?.required ?? (hitRequired !== undefined ? hitRequired : next.required);
  function pump() {
    if (disposed || !input || !isPresentationVisible()) return;
    if (control) {
      if (control === 'error') emit('error', failure);
      else { const state = historyControl(control, retryUsed); emit(control, state.message, state.retryAvailable); }
      return;
    }
    const reason = historyWaiting(input, gate(input));
    if (reason) { emit('stale', reason); return; }
    if (active) { refreshing(); return; }
    if (!pending) { emit('ready'); return; }
    const delay = Math.max(0, 1000 - (Date.now() - lastStart));
    if (delay > 0) {
      refreshing();
      timer ??= setTimeout(() => { timer = undefined; pump(); }, delay);
      return;
    }
    clearTimeout(timer); timer = undefined;
    const request = pending, token = epoch, query = input.query, key = identity;
    const readReservation = route ? cache?.begin(key, route, query) : undefined;
    reservation = readReservation;
    pending = null; active = true; lastStart = Date.now(); refreshing();
    void Promise.resolve().then(() => disposed || token !== epoch ? { status: 'superseded' as const } : read(client, query)).then(reply => {
      if (disposed || token !== epoch || !input) return;
      if (historyWaiting(input, request.required)) {
        // Storage failed or changed while SQL ran. Retain this checkpoint for recovery.
        pending = request; return;
      }
      if (reply.status === 'ready') {
        value = reply.value;
        if (route) cache?.put(key, { value, route, query, required: request.required }, readReservation);
        if (latest.version > request.version && !pending) pending = latest;
        emit(pending ? 'stale' : 'ready', pending ? 'Refreshing saved observations.' : '');
      } else if (reply.status === 'busy') {
        const state = historyControl(reply.status, retryUsed);
        if (state.retry) {
          retryUsed = true; pending ??= latest;
          // Native Busy already waited for the active reader to close.
          lastStart = -Infinity;
          emit('busy', state.message, false);
        } else { control = 'busy'; emit('busy', state.message, true); }
      } else if (!pending) {
        control = 'superseded'; emit('superseded', historyControl('superseded', retryUsed).message, false);
      }
    }).catch((error: unknown) => {
      if (disposed || token !== epoch) return;
      control = 'error'; failure = historyFailure(error); emit('error', failure);
    }).finally(() => {
      if (readReservation !== undefined) cache?.end(readReservation);
      if (reservation === readReservation) reservation = undefined;
      active = false;
      if (!disposed) pump();
    });
  }
  const releaseVisibility = observePresentationVisibility(visible => {
    if (!visible) { clearTimeout(timer); timer = undefined; }
    else {
      // A hidden backlog needs one read when its newest checkpoint is already
      // durable. Otherwise retain the fixed older checkpoint to avoid starvation.
      if (!active && pending && input && latest.version > pending.version
        && historyWaiting(input, latest.required) === null) pending = latest;
      pump();
    }
  });
  return {
    update(next) {
      if (disposed) return;
      const key = historyKey(next);
      const changed = identity !== key;
      const requirementChanged = input?.required !== next.required;
      const recovered = input && historyWaiting(input, gate(input)) !== null
        && historyWaiting(next, gate(next)) === null;
      if (changed) {
        endRead();
        epoch++; value = null; identity = key; control = null; retryUsed = false; lastStart = -Infinity;
        clearTimeout(timer); timer = undefined;
        hitRequired = undefined;
        latest = { required: next.required, version: ++version };
        pending = latest;
        const entry = cache?.get(key);
        if (entry) {
          value = entry.value;
          if (cache!.fresh(entry) && routeHref(entry.route) === routeHref(route!)) { pending = null; hitRequired = entry.required; }
          else pending = { ...latest, required: maxSequence(next.required, entry.dirtySince) };
        }
      } else if (requirementChanged || (recovered && !pending && !active)) {
        hitRequired = undefined;
        latest = { required: next.required, version: ++version };
        if (!pending) pending = latest;
        // A second Busy/error remains explicit until a changed input or manual Retry.
        control = null;
      }
      input = next; pump();
    },
    retry() {
      if (disposed || !input || control === 'superseded' || active) return;
      epoch++; retryUsed = false; control = null; lastStart = -Infinity;
      hitRequired = undefined;
      latest = { required: input.required, version: ++version }; pending = latest; pump();
    },
    dispose() {
      releaseVisibility();
      endRead();
      disposed = true; epoch++; clearTimeout(timer); input = null; pending = null; value = null; emitted = undefined;
    },
  };
}

function maxSequence(a: string | null, b: string | null): string | null {
  return a === null ? b : b === null || BigInt(a) >= BigInt(b) ? a : b;
}
