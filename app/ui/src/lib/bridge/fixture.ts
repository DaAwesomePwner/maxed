import type { AlertSummary } from '../bindings/AlertSummary';
import type { AlertSummaryRequest } from '../bindings/AlertSummaryRequest';
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
import type { RunFightStripRow } from '../bindings/RunFightStripRow';
import type { ChatFilter } from '../bindings/ChatFilter';
import type { ChatPage } from '../bindings/ChatPage';
import type { EventFilter } from '../bindings/EventFilter';
import type { EventPage } from '../bindings/EventPage';
import type { LootSettings } from '../bindings/LootSettings';
import type { DropFilter } from '../bindings/DropFilter';
import type { DropPage } from '../bindings/DropPage';
import type { ItemHistoryRequest } from '../bindings/ItemHistoryRequest';
import type { ItemHistory } from '../bindings/ItemHistory';
import type { LootStatsFilter } from '../bindings/LootStatsFilter';
import type { LootStats } from '../bindings/LootStats';
import type { CollectionRequest } from '../bindings/CollectionRequest';
import type { Collection } from '../bindings/Collection';
import type { ItemMatch } from '../bindings/ItemMatch';
import type { AlertFilter } from '../bindings/AlertFilter';
import type { AlertPage } from '../bindings/AlertPage';
import type { HistoryReply } from '../bindings/HistoryReply';
import type { RunFilter } from '../bindings/RunFilter';
import type { RunPage } from '../bindings/RunPage';
import type { RunStory } from '../bindings/RunStory';
import type { FightView } from '../bindings/FightView';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import type { AppSettings } from '../bindings/AppSettings';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { HomeSummary } from '../bindings/HomeSummary';
import type { SetupStatus } from '../bindings/SetupStatus';
import type { VisualPage } from '../bindings/VisualPage';
import type { EngineEvent } from '../bindings/EngineEvent';
import type { HomeScope } from '../bindings/HomeScope';
import type { ScopeLootPage } from '../bindings/ScopeLootPage';
import type { DungeonBest } from '../bindings/DungeonBest';
import type { Commands, Events, Transport } from './client';

export type DungeonBestsFixtureQuery = () => Promise<HistoryReply<DungeonBest[]>>;
export type ScopeLootFixtureQuery = (scope: HomeScope, cursor: string | null, limit: number) => Promise<HistoryReply<ScopeLootPage>>;

type Handlers = {
  [K in keyof Commands]: (args: Commands[K]['args']) => Promise<Commands[K]['result']>;
};
type Listeners = { [K in keyof Events]: Set<(payload: Events[K]) => void> };
export const fixtureScenarios = ['empty', 'waiting', 'live', 'recovery', 'unknown-identity', 'death', 'drop'] as const;
export type FixtureScenario = typeof fixtureScenarios[number];

// Development/test seam only. No fixture JSON is imported into production.
// Native mutations reject except for the in-memory preview chase list; nothing is persisted.
export interface StatsFixtureQueries {
  maps?: (filter: StatsFilter) => StatsMaps;
  sessions?: (filter: StatsSessionFilter) => StatsSessionPage;
  session?: (request: StatsSessionRequest) => StatsSessionDetail | null;
  dungeons?: (filter: StatsFilter) => StatsDungeons;
}

export interface LogsFixtureQueries {
  getEventSummary?: (request: EventSummaryRequest) => EventSummary;
  getChatOverview?: (request: ChatOverviewRequest) => ChatOverview;
  getChatSender?: (request: ChatSenderRequest) => ChatSenderSummary | null;
  listWhisperPartners?: (filter: WhisperPartnerFilter) => WhisperPartners;
  listChat?: (filter: ChatFilter) => ChatPage;
  listInstanceEvents?: (filter: EventFilter) => EventPage;
}
export interface LootFixtureQueries {
  getDropSummary?: (request: DropSummaryRequest) => DropSummary;
  listDrops?: (filter: DropFilter) => DropPage;
  itemHistory?: (request: ItemHistoryRequest) => ItemHistory;
  lootStats?: (filter: LootStatsFilter) => LootStats;
  collection?: (request: CollectionRequest) => Collection;
  searchItems?: (query: string, limit: number) => ItemMatch[];
}
export interface RunFixtureQueries {
  listRuns(filter: RunFilter): Promise<HistoryReply<RunPage>>;
  getRun(id: string): Promise<HistoryReply<RunStory | null>>;
  getRunFightStrip?(runId: string, playerId: string | null): Promise<HistoryReply<RunFightStripRow[]>>;
  getFight(id: string): Promise<HistoryReply<FightView | null>>;
  getPlayerBreakdown(fightId: string, objectId: number): Promise<HistoryReply<PlayerDamage | null>>;
}
export interface AlertFixtureQueries {
  /** Range-scoped KPI data, independent of the alert-list filters. */
  getAlertSummary?(request: AlertSummaryRequest): Promise<HistoryReply<AlertSummary>>;
  listAlerts(filter: AlertFilter): Promise<HistoryReply<AlertPage>>;
}
export function createFixtureTransport(initial: LiveSnapshot, visuals?: VisualPage, home?: HomeSummary,
  runs?: RunFixtureQueries, alerts?: AlertFixtureQueries, scopeLoot?: ScopeLootFixtureQuery,
  dungeonBests?: DungeonBestsFixtureQuery, preview: { requireHistoryFixtures?: boolean; lootSettings?: LootSettings; catalogDungeons?: string[] } = {}, loot?: LootFixtureQueries, logs?: LogsFixtureQueries, stats?: StatsFixtureQueries) {
  let snapshot = structuredClone(initial);
  const seed = structuredClone(initial);
  const presentation = structuredClone(visuals);
  const history = structuredClone(home);
  const listeners: Listeners = {
    'capture-status': new Set(),
    'engine-event': new Set(),
    'live-snapshot': new Set(),
  };
  const setup: SetupStatus = {
    npcap: 'unchecked', access: 'unknown', assets: 'missing', extraction_stage: null,
    client_found: false, live_confirmed: false, demo: true,
    message: 'Preview',
  };
  const settings: AppSettings = {
    loot: structuredClone(preview.lootSettings ?? { chase: [] }),
    chat: { store: { public: true, guild: true, party: true, whisper: true }, retention_days: 90, hidden_senders: [], hidden_keywords: [] },
    alerts: { whisper: 'toast', guild_message: 'quiet_log_only', party_message: 'quiet_log_only', white_bag: 'banner', personal_best: 'banner', key_popped: 'toast', key_pop_dungeons: null, key_pop_excluded_dungeons: [], death: 'banner', flawless_run: 'off', muted: false, windows_notifications: false, volume_percent: 35 },
    version: 1, adapter_id: null, assets_path: null, save_recordings: false,
    recording_limit_bytes: 1_000_000_000, update_channel: 'stable', hardware_acceleration: false,
  };
  const unavailable = async (): Promise<never> => {
    throw new Error('Preview Only');
  };
  const missingLoot = (name: string) => {
    if (preview.requireHistoryFixtures) throw new Error(`${name} was not supplied for this synthetic fixture preview`);
  };
  const handlers: Handlers = {
    get_stats_maps: async ({ filter }) => {
      if (stats?.maps) return { status: 'ready', value: structuredClone(stats.maps(structuredClone(filter))) };
      missingLoot('Map stats');
      return { status: 'ready', value: { rows: [], truncated: false, tracking_since: null, characters: [] } };
    },
    list_stats_sessions: async ({ filter }) => {
      if (stats?.sessions) return { status: 'ready', value: structuredClone(stats.sessions(structuredClone(filter))) };
      missingLoot('Sessions');
      return { status: 'ready', value: { rows: [], next: null, characters: [] } };
    },
    get_stats_session: async ({ request }) => {
      if (stats?.session) return { status: 'ready', value: structuredClone(stats.session(structuredClone(request))) };
      missingLoot('Session');
      return { status: 'ready', value: null };
    },
    get_stats_dungeons: async ({ filter }) => {
      if (stats?.dungeons) return { status: 'ready', value: structuredClone(stats.dungeons(structuredClone(filter))) };
      missingLoot('Dungeon stats');
      return { status: 'ready', value: { rows: [], truncated: false, characters: [] } };
    },
    export_stats_csv: unavailable,

    get_event_summary: async ({ request }) => {
      if (logs?.getEventSummary) return { status: 'ready', value: structuredClone(logs.getEventSummary(structuredClone(request))) };
      missingLoot('Event summary');
      return { status: 'ready', value: { total: 0,
        by_kind: { player_death: 0, player_left: 0, portal_opened: 0, realm_event_killed: 0 },
        dungeons: [], local_pops: 0, last_local_pop: null, flagged_since: null, first_at: null } };
    },
    get_chat_overview: async ({ request }) => {
      if (logs?.getChatOverview) return { status: 'ready', value: structuredClone(logs.getChatOverview(structuredClone(request))) };
      missingLoot('Chat overview');
      return { status: 'ready', value: { lines: 0, by_channel: { public: 0, guild: 0, party: 0, whisper: 0, unknown: 0 }, first_at: null, latest_at: null } };
    },
    get_chat_sender: async ({ request }) => {
      if (logs?.getChatSender) return { status: 'ready', value: structuredClone(logs.getChatSender(structuredClone(request))) };
      missingLoot('Chat sender');
      return { status: 'ready', value: null };
    },
    list_whisper_partners: async ({ filter }) => {
      if (logs?.listWhisperPartners) return { status: 'ready', value: structuredClone(logs.listWhisperPartners(structuredClone(filter))) };
      missingLoot('Whisper partners');
      return { status: 'ready', value: { rows: [], people: 0, lines: 0 } };
    },
    list_chat: async ({ filter }) => {
      if (logs?.listChat) return { status: 'ready', value: structuredClone(logs.listChat(structuredClone(filter))) };
      missingLoot('Chat');
      return { status: 'ready', value: { rows: [], next: null } };
    },
    list_instance_events: async ({ filter }) => {
      if (logs?.listInstanceEvents) return { status: 'ready', value: structuredClone(logs.listInstanceEvents(structuredClone(filter))) };
      missingLoot('Events');
      return { status: 'ready', value: { rows: [], next: null, entered_runs: [] } };
    },
    update_chat_settings: async ({ settings: next }) => { settings.chat = structuredClone(next); return undefined; },
    clear_chat_history: async () => undefined,
    get_drop_summary: async ({ request }) => {
      if (loot?.getDropSummary) return { status: 'ready', value: structuredClone(loot.getDropSummary(structuredClone(request))) };
      missingLoot('Drop summary');
      const counted = request.include_items !== false;
      const empty = () => ({ bags: 0, runs: 0, runs_ended: request.filter.place?.kind === 'outside' ? null : 0,
        whites: 0, items_counted: counted, ut_items: counted ? 0 : null, st_items: counted ? 0 : null,
        enchanted_items: counted ? 0 : null, best_enchants: null, last_white: null, first_at: null });
      return { status: 'ready', value: { totals: empty(), days: request.days.map(empty) } };
    },
    list_drops: async ({ filter }) => {
      if (loot?.listDrops) return { status: 'ready', value: structuredClone(loot.listDrops(structuredClone(filter))) };
      missingLoot('Drop history');
      return { status: 'ready', value: { rows: [], next: null } };
    },
    get_item_history: async ({ request }) => {
      if (loot?.itemHistory) return { status: 'ready', value: structuredClone(loot.itemHistory(structuredClone(request))) };
      missingLoot('Item history');
      return { status: 'ready', value: { item_type: request.item_type,
        totals: { appearances: 0, bags: 0, taken: 0, first_at: null, last_at: null },
        enchant_variants: [], by_dungeon: [], by_source: [], bags: { rows: [], next: null } } };
    },
    get_loot_stats: async ({ filter }) => {
      if (loot?.lootStats) return { status: 'ready', value: structuredClone(loot.lootStats(structuredClone(filter))) };
      missingLoot('Loot stats');
      return { status: 'ready', value: { dungeons: [], sources: [], totals: [], whites: 0 } };
    },
    get_collection: async ({ request }) => {
      if (loot?.collection) return { status: 'ready', value: structuredClone(loot.collection(structuredClone(request))) };
      missingLoot('Collection');
      return { status: 'ready', value: {
        chase: request.chase.map(item => ({ item: structuredClone(item), seen: 0, last_at: null, dry_runs: 0 })),
        grid_dungeon: request.dungeon, grid: [],
      } };
    },
    search_items: async ({ query, limit }) => {
      if (loot?.searchItems) return structuredClone(loot.searchItems(query, limit));
      missingLoot('Item search');
      return [];
    },
    set_chase_list: async ({ items }) => {
      const pairs = new Set(items.map(item => JSON.stringify([item.item_type, item.dungeon])));
      if (items.length > 100 || pairs.size !== items.length || items.some(item =>
        (item.dungeon !== null && (item.dungeon.length === 0 || new TextEncoder().encode(item.dungeon).length > 256
          || /[\u0000-\u001f\u007f-\u009f]/.test(item.dungeon)))
        || typeof item.pinned_at !== 'string' || item.pinned_at.length === 0 || /[^0-9]/.test(item.pinned_at)
        || BigInt(item.pinned_at) > 9223372036854775807n)) {
        throw new Error('Local settings could not be read or saved. Restore a supported settings file and retry.');
      }
      settings.loot.chase = structuredClone(items);
      return null;
    },
    get_update_status: async () => ({ phase: 'idle', channel: settings.update_channel, offer: null, downloaded_bytes: 0, total_bytes: null, failure: null }),
    get_recording_status: async () => ({ state: 'disabled', limit_bytes: settings.recording_limit_bytes, retained_bytes: 0, dropped_frames: 0, failure: null }),
    report_interactive_rail: async () => {},
    check_for_updates: unavailable,
    download_update: unavailable,
    install_update: unavailable,
    cancel_update: unavailable,
    get_alert_summary: async ({ request }) => {
      if (!alerts?.getAlertSummary) throw new Error('Alert summary was not supplied for this synthetic fixture preview');
      return structuredClone(await alerts.getAlertSummary(structuredClone(request)));
    },
    list_alerts: async ({ filter }) => {
      if (!alerts) throw new Error('Alert history was not supplied for this synthetic fixture preview');
      return structuredClone(await alerts.listAlerts(structuredClone(filter)));
    },
    get_alert_delivery_status: async () => ({ permission: 'unavailable', sound_available: false, message: 'Preview', flawless_unavailable: 'Flawless is unavailable: no approved captured-evidence definition exists.' }),
    update_alert_preferences: unavailable,
    request_notification_permission: unavailable,
    test_alert_delivery: unavailable,
    get_startup_setup_required: async () => false,
    get_catalog_available: async () => true,
    get_live_snapshot: async () => structuredClone(snapshot),
    get_dungeon_bests: async () => {
      if (dungeonBests) return structuredClone(await dungeonBests());
      if (preview.requireHistoryFixtures) throw new Error('Dungeon bests were not supplied for this synthetic fixture preview');
      return { status: 'ready', value: [] };
    },
    get_scope_loot: async ({ scope, cursor, limit }) => {
      if (scopeLoot) return structuredClone(await scopeLoot(structuredClone(scope), cursor ?? null, limit));
      if (preview.requireHistoryFixtures) throw new Error('Scope loot was not supplied for this synthetic fixture preview');
      return { status: 'ready', value: { items: [], next: null, total_bags: 0 } };
    },
    get_home_summary: async () => {
      if (!history) throw new Error('Home history was not supplied for this synthetic fixture preview');
      return { status: 'ready', value: structuredClone(history) };
    },
    list_runs: async ({ filter }) => {
      if (!runs) return unavailable();
      return structuredClone(await runs.listRuns(structuredClone(filter)));
    },
    get_run: async ({ id }) => {
      if (!runs) return unavailable();
      return structuredClone(await runs.getRun(id));
    },
    get_run_fight_strip: async ({ runId, playerId }) => {
      if (!runs?.getRunFightStrip) return { status: 'ready', value: [] };
      return structuredClone(await runs.getRunFightStrip(runId, playerId));
    },
    get_fight: async ({ id }) => {
      if (!runs) return unavailable();
      return structuredClone(await runs.getFight(id));
    },
    get_player_breakdown: async ({ fightId, objectId }) => {
      if (!runs) return unavailable();
      return structuredClone(await runs.getPlayerBreakdown(fightId, objectId));
    },
    get_capture_status: async () => structuredClone(snapshot.capture),
    get_setup_status: async () => structuredClone(setup),
    get_settings: async () => structuredClone(settings),
    get_store_status: async () => ({
      state: 'closed', queued: 0, durable: null, stream_id: null,
      message: 'Preview',
    }),
    list_adapters: unavailable,
    start_capture: unavailable,
    stop_capture: unavailable,
    set_adapter: unavailable,
    set_assets_path: unavailable,
    set_demo: unavailable,
    update_settings: unavailable,
    list_catalog_dungeon_options: async () => (preview.catalogDungeons ?? []).map(name => ({
      name, aliases: [], kind: 'dungeon', completion_index: null, portal_type: null,
    })),
    // Only explicitly supplied synthetic dungeon definitions are available.
    list_catalog_dungeons: async () => structuredClone(preview.catalogDungeons ?? []),
    get_dungeon_portal_type: async () => null,
    get_object_visuals: async ({ objectTypes }) => {
      const unique = [...new Set(objectTypes)];
      if (unique.length > 128 || unique.some((type) => !Number.isInteger(type) || type < 0 || type > 65535)) {
        throw new Error('Request at most 128 unique object types in the u16 range');
      }
      if (!presentation) throw new Error('Fixture catalog presentation is unavailable');
      return {
        client_fingerprint: presentation.client_fingerprint,
        objects: unique.map((object_type) => structuredClone(
          presentation.objects.find((row) => row.object_type === object_type)
          ?? { object_type, name: null, sprite: null, bag: null },
        )),
      };
    },
  };
  const transport: Transport = {
    invoke: (command, args) => handlers[command](args),
    listen: async (event, receive) => {
      listeners[event].add(receive);
      return () => { listeners[event].delete(receive); };
    },
  };
  function emit<K extends keyof Events>(event: K, payload: Events[K]) {
    for (const receive of listeners[event]) receive(structuredClone(payload));
  }
  return {
    ...transport,
    demo: true as const,
    externalDeliveryAllowed: false as const,
    // Explicit test controls only; no timers or production fixture imports.
    step(scenario: FixtureScenario) {
      const next = structuredClone(seed);
      next.at = (BigInt(snapshot.at) + 1n).toString();
      next.stream_id = snapshot.stream_id;
      next.sequence = snapshot.sequence;
      next.drops = [];
      next.capture = {
        ...next.capture,
        state: scenario === 'empty' ? 'Stopped' : scenario === 'waiting' ? 'WaitingForGame'
          : scenario === 'recovery' ? 'Recovering' : 'Live',
        message: `Synthetic fixture scenario: ${scenario}`,
      };
      if (scenario === 'empty' || scenario === 'waiting') {
        next.session = null; next.character = null; next.map = null;
        next.area_kind = 'unknown'; next.run = null; next.fights = []; next.issues = [];
      }
      if (scenario === 'unknown-identity') {
        next.character = null;
        next.issues = [...new Set([...next.issues, 'missing_identity'] as const)];
        if (next.run) next.run.character_id = null;
        for (const fight of next.fights) {
          for (const player of fight.players) player.is_local = false;
        }
      }
      const events: EngineEvent[] = [{ capture_state_changed: next.capture }];
      if (scenario === 'death') {
        if (next.character) next.character.died_at = next.at;
        if (next.run) {
          next.run.died_at = next.at; next.run.ended = next.at;
          next.run.completed_at = null; next.run.outcome = 'died'; next.run.evidence = [];
        }
        events.push({ player_died: { at: next.at, character_id: next.character?.id ?? null, run_id: next.run?.id ?? null } });
      }
      if (scenario === 'drop') {
        const drop = structuredClone(seed.drops[0]);
        if (!drop) throw new Error('The synthetic seed must include a drop for this scenario');
        drop.at = next.at;
        next.drops = [drop];
        events.push({ bag_dropped: drop });
      }
      // Match native ordering: all numbered events precede their snapshot watermark.
      for (const event of events) {
        next.sequence = (BigInt(next.sequence) + 1n).toString();
        emit('engine-event', { stream_id: next.stream_id, sequence: next.sequence, at: next.at, event });
      }
      snapshot = next;
      emit('capture-status', snapshot.capture);
      emit('live-snapshot', snapshot);
    },
    publishSnapshot(next: LiveSnapshot) {
      snapshot = structuredClone(next);
      emit('live-snapshot', snapshot);
    },
    // Inject loss/reordering without changing the authoritative query response.
    publishSnapshotUpdate(next: Events['live-snapshot']) { emit('live-snapshot', next); },
    publishCapture(next: Events['capture-status']) {
      snapshot = { ...snapshot, capture: structuredClone(next) };
      emit('capture-status', next);
    },
    publishEngineEvent(next: Events['engine-event']) { emit('engine-event', next); },
  };
}
