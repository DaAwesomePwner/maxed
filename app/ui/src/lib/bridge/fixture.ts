import type { CharacterCalculation } from '../bindings/CharacterCalculation';
import type { CharacterCalculationRequest } from '../bindings/CharacterCalculationRequest';
import type { EnchantInfo } from '../bindings/EnchantInfo';
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
import type { BossHistory } from '../bindings/BossHistory';
import type { HomeBossBaselines } from '../bindings/HomeBossBaselines';
import type { BossHistoryRequest } from '../bindings/BossHistoryRequest';
import type { DungeonProfile } from '../bindings/DungeonProfile';
import type { DungeonProfileRequest } from '../bindings/DungeonProfileRequest';
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
import type { RunRecap } from '../bindings/RunRecap';
import type { RunRecapRequest } from '../bindings/RunRecapRequest';
import type { VisitRecap } from '../bindings/VisitRecap';
import type { VisitRecapRequest } from '../bindings/VisitRecapRequest';
import type { FightView } from '../bindings/FightView';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import type { RunPlayerView } from '../bindings/RunPlayerView';
import type { PlayerContextView } from '../bindings/PlayerContextView';
import type { AppSettings } from '../bindings/AppSettings';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { HomeSummary } from '../bindings/HomeSummary';
import type { HomeHud } from '../bindings/HomeHud';
import type { HomeDungeonStreak } from '../bindings/HomeDungeonStreak';
import type { HomeDungeonStreakRequest } from '../bindings/HomeDungeonStreakRequest';
import type { SetupStatus } from '../bindings/SetupStatus';
import type { VisualPage } from '../bindings/VisualPage';
import type { EngineEvent } from '../bindings/EngineEvent';
import type { HomeScope } from '../bindings/HomeScope';
import type { ScopeLootPage } from '../bindings/ScopeLootPage';
import type { DungeonBest } from '../bindings/DungeonBest';
import type { Client, Commands, Events, Transport } from './client';
import type { HistoryUpgradeStatus } from '../bindings/HistoryUpgradeStatus';
import type { StorageStatus } from '../bindings/StorageStatus';
import type { StorageFailure } from '../bindings/StorageFailure';
import type { ManagedStorageBackup } from '../bindings/ManagedStorageBackup';

export type CharacterCalculationFixtureQuery = (request: CharacterCalculationRequest) => CharacterCalculation | Promise<CharacterCalculation>;

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
  visitRecap?: (request: VisitRecapRequest) => VisitRecap | null;
  runRecap?: (request: RunRecapRequest) => RunRecap | null;
  homeHud?: (scope: HomeScope) => HomeHud;
  homeDungeonStreak?: (request: HomeDungeonStreakRequest) => HomeDungeonStreak;
  bossHistory?: (request: BossHistoryRequest) => BossHistory;
  homeBossBaselines?: (request: BossHistoryRequest) => HomeBossBaselines;
  dungeonProfile?: (request: DungeonProfileRequest) => DungeonProfile;
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
  getRunPlayer?(runId: string, playerId: string): Promise<HistoryReply<RunPlayerView | null>>;
  getPlayerContext?(scopeId: string, playerId: string, from: string | null, until: string | null): Promise<HistoryReply<PlayerContextView | null>>;
}
export interface AlertFixtureQueries {
  /** Range-scoped KPI data, independent of the alert-list filters. */
  getAlertSummary?(request: AlertSummaryRequest): Promise<HistoryReply<AlertSummary>>;
  listAlerts(filter: AlertFilter): Promise<HistoryReply<AlertPage>>;
}
export type PartyFixtureQueries = Partial<Pick<Client,
  'getPartyLive' | 'getPartyRun' | 'getPartyPlayers' | 'getPartyPlayer' | 'setPartyMark'
  | 'clearPartyHistory' | 'setPartyPresetPick' | 'openRealmeye' | 'updatePartySettings'>>;

export function createFixtureTransport(initial: LiveSnapshot, visuals?: VisualPage, home?: HomeSummary,
  runs?: RunFixtureQueries, alerts?: AlertFixtureQueries, scopeLoot?: ScopeLootFixtureQuery,
  dungeonBests?: DungeonBestsFixtureQuery, preview: { characterCalculation?: CharacterCalculationFixtureQuery; enchants?: EnchantInfo[]; requireHistoryFixtures?: boolean; lootSettings?: LootSettings; catalogDungeons?: string[] } = {}, loot?: LootFixtureQueries, logs?: LogsFixtureQueries, stats?: StatsFixtureQueries, party?: PartyFixtureQueries) {
  let snapshot = structuredClone(initial);
  const seed = structuredClone(initial);
  const presentation = structuredClone(visuals);
  const enchants = structuredClone(preview.enchants ?? []);
  const history = structuredClone(home);
  const listeners: Listeners = {
    'storage-maintenance-changed': new Set(),
    'capture-status': new Set(),
    'presentation-visible': new Set(),
    'engine-event': new Set(),
    'live-snapshot': new Set(),
    'party-changed': new Set(),
  };
  const setup: SetupStatus = {
    npcap: 'unchecked', access: 'unknown', assets: 'missing', extraction_stage: null,
    client_found: false, live_confirmed: false, demo: true,
    message: 'Preview',
  };
  const settings: AppSettings = {
    recap_image: { names: true, loot: true, fights: true },
    party: { presets: [], default_preset_id: null },
    loot: structuredClone(preview.lootSettings ?? { chase: [] }),
    chat: { store: { public: true, guild: true, party: true, whisper: true }, retention_days: 90, hidden_senders: [], hidden_keywords: [], filter_spam: true },
    alerts: { whisper: 'toast', guild_message: 'quiet_log_only', party_message: 'quiet_log_only', white_bag: 'banner', personal_best: 'banner', key_popped: 'off', key_pop_dungeons: [], key_pop_excluded_dungeons: [], death: 'banner', flawless_run: 'off', muted: false, windows_notifications: false, volume_percent: 35 },
    version: 1, adapter_id: null, assets_path: null, save_recordings: false,
    recording_limit_bytes: 1_000_000_000, update_channel: 'stable', hardware_acceleration: false,
  };
  let upgrade: HistoryUpgradeStatus = { phase: 'ready', schema_version: 16, required_bytes: null, free_bytes: null, failure: null, last_failure: null };
  let backups: ManagedStorageBackup[] = [];
  const storage: StorageStatus = {
    measured_at: '1', database: { state: 'absent', main_bytes: null, wal_bytes: null, shm_bytes: null,
      page_size: null, page_count: null, freelist_count: null, allocated_bytes: null, reusable_bytes: null, auto_vacuum: null, journal_mode: null },
    backups: { retained_bytes: '0', temporary_bytes: '0' },
    recordings: { physical_bytes: '0', accounted_bytes: '0', quota_bytes: '1000000000' },
    receipt_estimate: { bytes: null, basis: 'legacy_receipt_logical_bytes', measured_at: null },
    maintenance: { jobs: [], totals: { eligible_records: null, eligible_bytes: null, processed_records: '0', processed_bytes: '0', blocked_records: null, blocked_bytes: null }, last_success_at: null },
    failure: null,
  };
  function storageChanged() {
    storage.measured_at = (BigInt(storage.measured_at) + 1n).toString();
    emit('storage-maintenance-changed', { measured_at: storage.measured_at, maintenance: storage.maintenance, failure: storage.failure });
  }
  function upgradeSpace() {
    if (upgrade.schema_version === null || upgrade.schema_version === 0 || upgrade.schema_version >= 11) return;
    upgrade.failure = upgrade.free_bytes === null ? 'space_unknown'
      : BigInt(upgrade.free_bytes) < BigInt(upgrade.required_bytes ?? '0') ? 'insufficient_space' : null;
    upgrade.phase = upgrade.failure === null ? 'required' : 'blocked';
  }
  const unavailable = async (): Promise<never> => {
    throw new Error('Preview Only');
  };
  const missingLoot = (name: string) => {
    if (preview.requireHistoryFixtures) throw new Error(`${name} was not supplied for this synthetic fixture preview`);
  };
  const handlers: Handlers = {
    get_home_boss_baselines: async ({ request }) => {
      if (stats?.homeBossBaselines) return { status: 'ready', value: structuredClone(stats.homeBossBaselines(structuredClone(request))) };
      missingLoot('Home boss baselines');
      return { status: 'ready', value: { rows: [], truncated: false } };
    },
    get_boss_history: async ({ request }) => ({ status: 'ready',
      value: stats?.bossHistory ? structuredClone(stats.bossHistory(structuredClone(request)))
        : { rows: [], truncated: false } }),
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
    get_dungeon_profile: async ({ request }) => {
      if (stats?.dungeonProfile) return { status: 'ready', value: structuredClone(stats.dungeonProfile(structuredClone(request))) };
      missingLoot('Dungeon profile');
      return { status: 'ready', value: { dungeon: request.dungeon, clears: 0, bosses: [],
        usual_run_dps: null, run_dps_samples: 0, usual_clear_micros: null, clear_samples: 0 } };
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
    get_database_size: async () => '0',
    get_history_upgrade_status: async () => {
      if (upgrade.phase !== 'backing_up' && upgrade.phase !== 'upgrading') upgradeSpace();
      return structuredClone(upgrade);
    },
    backup_and_upgrade_history: async () => {
      if (upgrade.phase === 'ready' || upgrade.phase === 'backing_up' || upgrade.phase === 'upgrading') return structuredClone(upgrade);
      upgradeSpace();
      upgrade.last_failure = upgrade.failure;
      if (upgrade.failure === null) upgrade.phase = 'backing_up';
      return structuredClone(upgrade);
    },
    get_storage_status: async (options) => {
      if (options?.estimate && !storage.maintenance?.jobs.some(job => !job.needs_resume && job.phase !== 'reclaimable')) {
        storage.receipt_estimate = { bytes: '216', basis: 'legacy_receipt_logical_bytes', measured_at: storage.measured_at };
      }
      return structuredClone(storage);
    },
    compact_storage: async () => {
      if (storage.failure && storage.maintenance?.jobs[0]?.phase === 'awaiting_backup') {
        storage.failure = null;
        storage.maintenance.jobs[0].reason = 'awaiting_backup';
        storageChanged();
      }
      if (storage.maintenance && storage.maintenance.jobs.length === 0) {
        storage.maintenance.jobs.push({ phase: 'awaiting_backup', paused: false, needs_resume: false, reason: 'awaiting_backup',
          requested_at: storage.measured_at, updated_at: storage.measured_at,
          counters: { eligible_records: '3', eligible_bytes: '216', processed_records: '0', processed_bytes: '0', blocked_records: '0', blocked_bytes: '0' } });
        storage.maintenance.totals = structuredClone(storage.maintenance.jobs[0].counters);
        storageChanged();
      }
      return 'accepted';
    },
    cancel_storage_maintenance: async () => {
      if (upgrade.phase === 'backing_up' || upgrade.phase === 'upgrading') {
        upgrade.phase = 'blocked'; upgrade.failure = 'cancelled'; upgrade.last_failure = 'cancelled';
      }
      const job = storage.maintenance?.jobs[0];
      if (job && job.phase !== 'reclaimable') { job.paused = true; job.needs_resume = true; job.reason = 'cancelled'; }
      for (const backup of backups) backup.in_use = false;
      storageChanged();
      return 'accepted';
    },
    resume_storage_maintenance: async () => {
      const job = storage.maintenance?.jobs[0];
      if (job?.needs_resume) {
        job.paused = false; job.needs_resume = false; job.reason = null;
        if (backups.length === 0) job.phase = 'awaiting_backup';
        for (const backup of backups) backup.in_use = true;
        storageChanged();
      }
      return 'accepted';
    },
    list_storage_backups: async () => structuredClone(backups),
    delete_storage_backup: async ({ id }) => {
      if (!/^[1-9][0-9]*$/.test(id) || BigInt(id) > 9223372036854775807n) throw new Error('Storage backup ID is invalid.');
      const backup = backups.find(backup => backup.id === id);
      if (!backup) throw new Error('Storage backup ID is invalid.');
      if (backup.in_use) throw new Error('History backup is in use');
      backups = backups.filter(backup => backup.id !== id);
      storage.backups.retained_bytes = backups.reduce((sum, backup) => sum + BigInt(backup.bytes ?? '0'), 0n).toString();
    },
    get_storage_backup_location: async () => 'C:\\Synthetic\\Maxed\\backups\\live',
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
    get_character_calculation: async ({ request }) => {
      if (!preview.characterCalculation) throw new Error('Character calculation was not supplied for this synthetic fixture preview');
      return structuredClone(await preview.characterCalculation(structuredClone(request)));
    },
    set_document_visible: async ({ visible }) => visible,
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
    get_home_hud: async ({ scope }) => {
      if (stats?.homeHud) return { status: 'ready', value: structuredClone(stats.homeHud(structuredClone(scope))) };
      missingLoot('Home HUD');
      return { status: 'ready', value: { fame_per_hour: null, combat: null, whites: 0,
        ut_st: [], ut_st_count: 0, potions: [] } };
    },
    get_home_dungeon_streak: async ({ request }) => {
      if (stats?.homeDungeonStreak) return { status: 'ready', value: structuredClone(stats.homeDungeonStreak(structuredClone(request))) };
      missingLoot('Home dungeon streak');
      return { status: 'ready', value: { dungeon: request.dungeon, runs_ended: 0,
        runs_since_last_white: null, avg_runs_per_white: null, last_white_at: null } };
    },
    get_run_recap: async ({ request }) => {
      if (stats?.runRecap) return { status: 'ready', value: structuredClone(stats.runRecap(structuredClone(request))) };
      missingLoot('Run recap');
      return { status: 'ready', value: null };
    },
    get_visit_recap: async ({ request }) => {
      if (stats?.visitRecap) return { status: 'ready', value: structuredClone(stats.visitRecap(structuredClone(request))) };
      missingLoot('Visit recap');
      return { status: 'ready', value: null };
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
    get_run_player: async ({ runId, playerId }) => {
      if (!runs?.getRunPlayer) return unavailable();
      return structuredClone(await runs.getRunPlayer(runId, playerId));
    },
    get_party_live: async () => {
      if (!party?.getPartyLive) return unavailable();
      return structuredClone(await party.getPartyLive());
    },
    get_party_run: async ({ runId }) => {
      if (!party?.getPartyRun) return unavailable();
      return structuredClone(await party.getPartyRun(runId));
    },
    get_party_players: async ({ query, filter, sort, cursor }) => {
      if (!party?.getPartyPlayers) return unavailable();
      return structuredClone(await party.getPartyPlayers(query, filter, sort, structuredClone(cursor)));
    },
    get_party_player: async ({ key, cursor }) => {
      if (!party?.getPartyPlayer) return unavailable();
      return structuredClone(await party.getPartyPlayer(key, structuredClone(cursor)));
    },
    set_party_mark: async ({ key, name, mark, note }) => {
      if (!party?.setPartyMark) return unavailable();
      await party.setPartyMark(key, name, mark, note);
    },
    clear_party_history: async ({ options }) => {
      if (!party?.clearPartyHistory) return unavailable();
      await party.clearPartyHistory(structuredClone(options));
    },
    set_party_preset_pick: async ({ presetId }) => {
      if (!party?.setPartyPresetPick) return unavailable();
      await party.setPartyPresetPick(presetId);
    },
    open_realmeye: async ({ name }) => {
      if (!party?.openRealmeye) return unavailable();
      await party.openRealmeye(name);
    },
    update_party_settings: async ({ settings: next }) => {
      if (!party?.updatePartySettings) return unavailable();
      await party.updatePartySettings(structuredClone(next));
      settings.party = structuredClone(next);
    },
    get_player_context: async ({ scopeId, playerId, from, until }) => {
      if (!runs?.getPlayerContext) return { status: 'ready', value: null };
      return structuredClone(await runs.getPlayerContext(scopeId, playerId, from, until));
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
    get_item_details: async ({ objectTypes, enchantIds }) => {
      const unique = [...new Set(objectTypes)];
      const ids = [...new Set(enchantIds)];
      if (unique.length > 128 || unique.some(type => !Number.isInteger(type) || type < 0 || type > 65535)) {
        throw new Error('Request at most 128 unique object types in the u16 range');
      }
      if (ids.length > 128 || ids.some(id => !Number.isInteger(id) || id < -2147483648 || id > 2147483647)) {
        throw new Error('Request at most 128 unique enchant IDs in the i32 range');
      }
      if (!presentation) throw new Error('Fixture catalog presentation is unavailable');
      return {
        client_fingerprint: presentation.client_fingerprint,
        items: unique.map(object_type => {
          const visual = presentation.objects.find(row => row.object_type === object_type);
          return { object_type, name: visual?.name ?? null, tier: visual?.item_tier ?? null, slot_type: null, slot_label: null, soulbound: null };
        }),
        enchants: ids.map(id => structuredClone(enchants.find(row => row.id === id) ?? { id, name: null, description: null })),
      };
    },
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
    setStorageFixture(patch: Partial<StorageStatus>, inventory?: ManagedStorageBackup[]) {
      Object.assign(storage, structuredClone(patch));
      if (inventory) backups = structuredClone(inventory);
      storageChanged();
    },
    setHistoryUpgradeFixture(options: { schemaVersion: number | null; freeBytes: string | null }) {
      upgrade = { phase: 'ready', schema_version: options.schemaVersion, required_bytes: '104857600', free_bytes: options.freeBytes, failure: null, last_failure: null };
      upgradeSpace();
    },
    setHistoryUpgradeStateFixture(patch: Partial<HistoryUpgradeStatus>) {
      upgrade = { ...upgrade, ...structuredClone(patch) };
    },
    advanceHistoryUpgrade(failure: StorageFailure | null = null) {
      if (upgrade.phase !== 'backing_up' && upgrade.phase !== 'upgrading') return;
      if (failure) { upgrade.phase = 'blocked'; upgrade.failure = failure; upgrade.last_failure = failure; return; }
      if (upgrade.phase === 'backing_up') {
        upgrade.phase = 'upgrading';
        backups = [{ id: '1', role: 'initial', completed_at: storage.measured_at, bytes: '4096', ready: true, in_use: true }];
        storage.backups.retained_bytes = '4096';
      }
      else { upgrade.phase = 'ready'; upgrade.schema_version = 16; upgrade.failure = null; upgrade.last_failure = null; }
      if (upgrade.phase === 'ready') for (const backup of backups) backup.in_use = false;
    },
    advanceStorageMaintenance(failure: StorageFailure | null = null) {
      const job = storage.maintenance?.jobs[0];
      if (!job || job.needs_resume || job.phase === 'reclaimable') return;
      if (failure) {
        storage.failure = failure;
        if (failure === 'blocked_stream' || failure === 'owner_poisoned') {
          job.reason = failure === 'blocked_stream' ? 'blocked_stream' : 'backup_poisoned';
          job.needs_resume = true;
          for (const backup of backups) backup.in_use = false;
        }
        storageChanged(); return;
      }
      if (job.phase === 'awaiting_backup') {
        job.phase = 'copying_receipts'; job.reason = null;
        const role = backups.some(backup => backup.role === 'initial') ? 'rolling' : 'initial';
        const id = (backups.reduce((max, backup) => BigInt(backup.id) > max ? BigInt(backup.id) : max, 0n) + 1n).toString();
        backups = backups.filter(backup => backup.role !== role);
        backups.push({ id, role, completed_at: storage.measured_at, bytes: '4096', ready: true, in_use: true });
        storage.backups.retained_bytes = backups.reduce((sum, backup) => sum + BigInt(backup.bytes ?? '0'), 0n).toString();
      } else {
        storage.failure = null;
        job.phase = 'reclaimable'; job.reason = null;
        job.counters.processed_records = '3'; job.counters.processed_bytes = '216';
        if (storage.maintenance) { storage.maintenance.totals = structuredClone(job.counters); storage.maintenance.last_success_at = storage.measured_at; }
        for (const backup of backups) backup.in_use = false;
      }
      storageChanged();
    },
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
    publishPartyChanged(next: Events['party-changed']) { emit('party-changed', next); },
  };
}
