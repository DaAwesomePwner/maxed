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
import type { StatsExportRequest } from '../bindings/StatsExportRequest';
import type { StatsExportResult } from '../bindings/StatsExportResult';
import type { RunFightStripRow } from '../bindings/RunFightStripRow';
import type { ChatFilter } from '../bindings/ChatFilter';
import type { ChatPage } from '../bindings/ChatPage';
import type { EventFilter } from '../bindings/EventFilter';
import type { EventPage } from '../bindings/EventPage';
import type { ChatSettings } from '../bindings/ChatSettings';
import type { DropFilter } from '../bindings/DropFilter';
import type { DropPage } from '../bindings/DropPage';
import type { ItemHistoryRequest } from '../bindings/ItemHistoryRequest';
import type { ItemHistory } from '../bindings/ItemHistory';
import type { LootStatsFilter } from '../bindings/LootStatsFilter';
import type { LootStats } from '../bindings/LootStats';
import type { CollectionRequest } from '../bindings/CollectionRequest';
import type { Collection } from '../bindings/Collection';
import type { ItemMatch } from '../bindings/ItemMatch';
import type { ChaseItem } from '../bindings/ChaseItem';
import type { DungeonDef } from '../bindings/DungeonDef';
import type { AlertFilter } from '../bindings/AlertFilter';
import type { AlertPage } from '../bindings/AlertPage';
import type { AlertPreferences } from '../bindings/AlertPreferences';
import type { AlertDeliveryStatus } from '../bindings/AlertDeliveryStatus';
import type { NotificationPermission } from '../bindings/NotificationPermission';
import type { AlertKind } from '../bindings/AlertKind';
import type { HistoryReply } from '../bindings/HistoryReply';
import type { RunFilter } from '../bindings/RunFilter';
import type { RunPage } from '../bindings/RunPage';
import type { RunStory } from '../bindings/RunStory';
import type { FightView } from '../bindings/FightView';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import type { AdapterInfo } from '../bindings/AdapterInfo';
import type { AppSettings } from '../bindings/AppSettings';
import type { CaptureStatus } from '../bindings/CaptureStatus';
import type { EventBatch } from '../bindings/EventBatch';
import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { LiveSnapshotUpdate } from '../bindings/LiveSnapshotUpdate';
import type { ScopeLootPage } from '../bindings/ScopeLootPage';
import type { DungeonBest } from '../bindings/DungeonBest';
import type { HomeScope } from '../bindings/HomeScope';
import type { HomeSummary } from '../bindings/HomeSummary';
import type { SetupStatus } from '../bindings/SetupStatus';
import type { StoreStatus } from '../bindings/StoreStatus';
import type { VisualPage } from '../bindings/VisualPage';
import { createVisualCache } from './visuals';

// These map command names and arguments only; payloads come from generated DTOs.
// Native Result failures reject the Promise; they never become empty/default data.
import type { UpdateStatus } from '../bindings/UpdateStatus';
import type { RecordingStatus } from '../bindings/RecordingStatus';

export interface Commands {
  get_event_summary: { args: { request: EventSummaryRequest }; result: HistoryReply<EventSummary> };
  get_chat_overview: { args: { request: ChatOverviewRequest }; result: HistoryReply<ChatOverview> };
  get_chat_sender: { args: { request: ChatSenderRequest }; result: HistoryReply<ChatSenderSummary | null> };
  list_whisper_partners: { args: { filter: WhisperPartnerFilter }; result: HistoryReply<WhisperPartners> };
  get_stats_maps: { args: { filter: StatsFilter }; result: HistoryReply<StatsMaps> };
  list_stats_sessions: { args: { filter: StatsSessionFilter }; result: HistoryReply<StatsSessionPage> };
  get_stats_session: { args: { request: StatsSessionRequest }; result: HistoryReply<StatsSessionDetail | null> };
  get_stats_dungeons: { args: { filter: StatsFilter }; result: HistoryReply<StatsDungeons> };
  export_stats_csv: { args: { request: StatsExportRequest }; result: StatsExportResult };

  list_chat: { args: { filter: ChatFilter }; result: HistoryReply<ChatPage> };
  list_instance_events: { args: { filter: EventFilter }; result: HistoryReply<EventPage> };
  update_chat_settings: { args: { settings: ChatSettings }; result: void };
  clear_chat_history: { args: undefined; result: void };
  get_drop_summary: { args: { request: DropSummaryRequest }; result: HistoryReply<DropSummary> };
  list_drops: { args: { filter: DropFilter }; result: HistoryReply<DropPage> };
  get_item_history: { args: { request: ItemHistoryRequest }; result: HistoryReply<ItemHistory> };
  get_loot_stats: { args: { filter: LootStatsFilter }; result: HistoryReply<LootStats> };
  get_collection: { args: { request: CollectionRequest }; result: HistoryReply<Collection> };
  search_items: { args: { query: string; limit: number }; result: ItemMatch[] };
  set_chase_list: { args: { items: ChaseItem[] }; result: null };
  get_update_status: { args: undefined; result: UpdateStatus };
  check_for_updates: { args: undefined; result: UpdateStatus };
  download_update: { args: { requestId: string }; result: UpdateStatus };
  install_update: { args: { requestId: string }; result: UpdateStatus };
  cancel_update: { args: undefined; result: UpdateStatus };
  get_recording_status: { args: undefined; result: RecordingStatus };
  report_interactive_rail: { args: undefined; result: void };
  get_alert_summary: { args: { request: AlertSummaryRequest }; result: HistoryReply<AlertSummary> };
  list_alerts: { args: { filter: AlertFilter }; result: HistoryReply<AlertPage> };
  get_alert_delivery_status: { args: undefined; result: AlertDeliveryStatus };
  update_alert_preferences: { args: { preferences: AlertPreferences }; result: void };
  request_notification_permission: { args: undefined; result: NotificationPermission };
  test_alert_delivery: { args: { kind: AlertKind }; result: AlertDeliveryStatus };
  get_startup_setup_required: { args: undefined; result: boolean };
  get_catalog_available: { args: undefined; result: boolean };
  get_capture_status: { args: undefined; result: CaptureStatus };
  start_capture: { args: undefined; result: void };
  stop_capture: { args: undefined; result: void };
  get_live_snapshot: { args: undefined; result: LiveSnapshot };
  list_runs: { args: { filter: RunFilter }; result: HistoryReply<RunPage> };
  get_run: { args: { id: string }; result: HistoryReply<RunStory | null> };
  get_run_fight_strip: { args: { runId: string; playerId: string | null }; result: HistoryReply<RunFightStripRow[]> };
  get_fight: { args: { id: string }; result: HistoryReply<FightView | null> };
  get_player_breakdown: { args: { fightId: string; objectId: number }; result: HistoryReply<PlayerDamage | null> };
  get_scope_loot: { args: { scope: HomeScope; cursor?: string | null; limit: number }; result: HistoryReply<ScopeLootPage> };
  get_dungeon_bests: { args: undefined; result: HistoryReply<DungeonBest[]> };
  get_home_summary: { args: { scope: HomeScope }; result: HistoryReply<HomeSummary> };
  get_setup_status: { args: { retry?: boolean } | undefined; result: SetupStatus };
  list_adapters: { args: undefined; result: AdapterInfo[] };
  set_adapter: { args: { adapterId: string | null }; result: void };
  set_assets_path: { args: { path: string }; result: void };
  set_demo: { args: { enabled: boolean }; result: void };
  get_settings: { args: undefined; result: AppSettings };
  update_settings: { args: { settings: AppSettings }; result: void };
  get_store_status: { args: undefined; result: StoreStatus };
  get_object_visuals: { args: { objectTypes: number[] }; result: VisualPage };
  list_catalog_dungeon_options: { args: undefined; result: DungeonDef[] };
  list_catalog_dungeons: { args: undefined; result: string[] };
  get_dungeon_portal_type: { args: { dungeon: string }; result: number | null };
}

export interface Events {
  'capture-status': CaptureStatus;
  'engine-event': EventEnvelope | EventBatch;
  'live-snapshot': LiveSnapshotUpdate;
}

export interface Transport {
  invoke<K extends keyof Commands>(command: K, args: Commands[K]['args']): Promise<Commands[K]['result']>;
  listen<K extends keyof Events>(event: K, receive: (payload: Events[K]) => void): Promise<() => void>;
}

export function createClient(transport: Transport) {
  const visuals = createVisualCache((objectTypes) => transport.invoke('get_object_visuals', { objectTypes }));
  const historyChanges = new Set<() => void>();
  const historyChange = async (action: () => Promise<void>) => {
    try { await action(); } finally {
      for (const changed of historyChanges) changed();
    }
  };
  const visualChanges = new Set<() => void>();
  const assetsChange = async (action: () => Promise<void>) => {
    visuals.clear();
    try { await action(); } finally {
      visuals.clear();
      for (const changed of visualChanges) changed();
    }
  };
  return {
    getStatsMaps: (filter: StatsFilter) => transport.invoke('get_stats_maps', { filter }),
    listStatsSessions: (filter: StatsSessionFilter) => transport.invoke('list_stats_sessions', { filter }),
    getStatsSession: (request: StatsSessionRequest) => transport.invoke('get_stats_session', { request }),
    getStatsDungeons: (filter: StatsFilter) => transport.invoke('get_stats_dungeons', { filter }),
    exportStatsCsv: (request: StatsExportRequest) => transport.invoke('export_stats_csv', { request }),
    getEventSummary: (request: EventSummaryRequest) => transport.invoke('get_event_summary', { request }),
    getChatOverview: (request: ChatOverviewRequest) => transport.invoke('get_chat_overview', { request }),
    getChatSender: (request: ChatSenderRequest) => transport.invoke('get_chat_sender', { request }),
    listWhisperPartners: (filter: WhisperPartnerFilter) => transport.invoke('list_whisper_partners', { filter }),
    listChat: (filter: ChatFilter) => transport.invoke('list_chat', { filter }),
    listInstanceEvents: (filter: EventFilter) => transport.invoke('list_instance_events', { filter }),
    updateChatSettings: (settings: ChatSettings) => historyChange(() => transport.invoke('update_chat_settings', { settings })),
    clearChatHistory: () => historyChange(() => transport.invoke('clear_chat_history', undefined)),
    queryUpdate: () => transport.invoke('get_update_status', undefined),
    checkForUpdates: () => transport.invoke('check_for_updates', undefined),
    downloadUpdate: (requestId: string) => transport.invoke('download_update', { requestId }),
    installUpdate: (requestId: string) => transport.invoke('install_update', { requestId }),
    cancelUpdate: () => transport.invoke('cancel_update', undefined),
    queryRecording: () => transport.invoke('get_recording_status', undefined),
    reportInteractiveRail: () => transport.invoke('report_interactive_rail', undefined),
    queryStartupSetupRequired: () => transport.invoke('get_startup_setup_required', undefined),
    queryCatalogAvailable: () => transport.invoke('get_catalog_available', undefined),
    querySnapshot: () => transport.invoke('get_live_snapshot', undefined),
    // KPI summaries follow the range independently of list search and kind filters.
    getAlertSummary: (request: AlertSummaryRequest) => transport.invoke('get_alert_summary', { request }),
    listAlerts: (filter: AlertFilter) => transport.invoke('list_alerts', { filter }),
    queryAlertDelivery: () => transport.invoke('get_alert_delivery_status', undefined),
    updateAlertPreferences: (preferences: AlertPreferences) => transport.invoke('update_alert_preferences', { preferences }),
    requestNotificationPermission: () => transport.invoke('request_notification_permission', undefined),
    testAlertDelivery: (kind: AlertKind) => transport.invoke('test_alert_delivery', { kind }),
    getDropSummary: (request: DropSummaryRequest) => transport.invoke('get_drop_summary', { request }),
    listDrops: (filter: DropFilter) => transport.invoke('list_drops', { filter }),
    getItemHistory: (request: ItemHistoryRequest) => transport.invoke('get_item_history', { request }),
    getLootStats: (filter: LootStatsFilter) => transport.invoke('get_loot_stats', { filter }),
    getCollection: (request: CollectionRequest) => transport.invoke('get_collection', { request }),
    searchItems: (query: string, limit: number) => transport.invoke('search_items', { query, limit }),
    setChaseList: async (items: ChaseItem[]): Promise<void> => { await transport.invoke('set_chase_list', { items }); },
    listRuns: (filter: RunFilter) => transport.invoke('list_runs', { filter }),
    getRun: (id: string) => transport.invoke('get_run', { id }),
    getRunFightStrip: (runId: string, playerId: string | null) => transport.invoke('get_run_fight_strip', { runId, playerId }),
    getFight: (id: string) => transport.invoke('get_fight', { id }),
    getPlayerBreakdown: (fightId: string, objectId: number) => transport.invoke('get_player_breakdown', { fightId, objectId }),
    queryScopeLoot: (scope: HomeScope, limit: number) => transport.invoke('get_scope_loot', { scope, cursor: null, limit }),
    queryScopeLootPage: (scope: HomeScope, cursor: string, limit: number) => transport.invoke('get_scope_loot', { scope, cursor, limit }),
    queryDungeonBests: () => transport.invoke('get_dungeon_bests', undefined),
    queryHome: (scope: HomeScope) => transport.invoke('get_home_summary', { scope }),
    onSnapshot: (receive: (snapshot: LiveSnapshotUpdate) => void) => transport.listen('live-snapshot', receive),
    onEngineEvent: (receive: (event: EventEnvelope) => void, batch: (batch: EventBatch) => void) => transport.listen('engine-event', value => {
      if ('events' in value) batch(value);
      else receive(value); // Only perf-before and fixture transports use singles.
    }),
    queryCapture: () => transport.invoke('get_capture_status', undefined),
    onCapture: (receive: (capture: CaptureStatus) => void) => transport.listen('capture-status', receive),
    querySetup: (options?: { retry?: boolean }) => transport.invoke('get_setup_status', options),
    querySettings: () => transport.invoke('get_settings', undefined),
    queryStore: () => transport.invoke('get_store_status', undefined),
    startCapture: () => transport.invoke('start_capture', undefined),
    stopCapture: () => transport.invoke('stop_capture', undefined),
    listAdapters: () => transport.invoke('list_adapters', undefined),
    setAdapter: (adapterId: string | null) => transport.invoke('set_adapter', { adapterId }),
    setAssetsPath: (path: string) => assetsChange(() => transport.invoke('set_assets_path', { path })),
    setDemo: (enabled: boolean) => assetsChange(() => transport.invoke('set_demo', { enabled })),
    updateSettings: (settings: AppSettings) => assetsChange(() => transport.invoke('update_settings', { settings })),
    getObjectVisuals: (objectTypes: number[]) => visuals.get(objectTypes),
    listCatalogDungeonOptions: () => transport.invoke('list_catalog_dungeon_options', undefined),
    listCatalogDungeons: () => transport.invoke('list_catalog_dungeons', undefined),
    getDungeonPortalType: (dungeon: string) => transport.invoke('get_dungeon_portal_type', { dungeon }),
    onHistoryChanged: (changed: () => void) => {
      historyChanges.add(changed);
      return () => { historyChanges.delete(changed); };
    },
    // Local invalidation only: stopped capture need not publish a new snapshot
    // when settings replace the visual generation. This creates no IPC listener.
    onVisualsChanged: (changed: () => void) => {
      visualChanges.add(changed);
      return () => { visualChanges.delete(changed); };
    },
  };
}

export type Client = ReturnType<typeof createClient>;
