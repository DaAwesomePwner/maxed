import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { vi } from 'vitest';
import type { HistoryCache } from '../../src/lib/runs/history-cache';
import Shell from '../../src/lib/shell/Shell.svelte';
import { createClient, type Client } from '../../src/lib/bridge/client';
import { createFixtureTransport, type RunFixtureQueries } from '../../src/lib/bridge/fixture';
import type { AlertPreferences } from '../../src/lib/bindings/AlertPreferences';
import type { AlertView } from '../../src/lib/bindings/AlertView';
import type { AppSettings } from '../../src/lib/bindings/AppSettings';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { RunTotals } from '../../src/lib/bindings/RunTotals';
import type { StoreStatus } from '../../src/lib/bindings/StoreStatus';
import type { Features } from '../../src/lib/navigation/routes';
import { homeSnapshot, homeSummary, saved } from '../home/fixtures';
import { fakeHistory } from '../navigation/fake-history';
import { fight, player, ready, run, runPage, story } from '../runs/fixtures';
export { advance, click, deferred, edit, fight, fightSummary, flush, follow, injectChicagoDate,
  largeStory, main, player, ready, region, run, runPage, source, story } from '../runs/fixtures';
export { homeSnapshot, saved };

// Invented data only. No recordings, asset extraction, native delivery, or network.
export const preferences = (patch: Partial<AlertPreferences> = {}): AlertPreferences => ({
  whisper: 'toast', guild_message: 'quiet_log_only', party_message: 'quiet_log_only', white_bag: 'banner', personal_best: 'banner', key_popped: 'toast', key_pop_dungeons: null, key_pop_excluded_dungeons: [], death: 'banner', flawless_run: 'off',
  muted: false, windows_notifications: false, volume_percent: 35, ...patch,
});
export const alert = (patch: Partial<AlertView> = {}): AlertView => ({
  id: 'moment-1', at: '4000000', kind: 'personal_best', run_id: 'run-1',
  explanation: 'Invented personal best', presentation: 'banner',
  details: { type: 'personal_best', fight_id: 'fight-1', character_id: 'character-1',
    boss_type: 2000, dps: 73, previous_dps: 61 }, ...patch,
});
export const envelope = (id: string, sequence: number, patch: Partial<AlertView> = {}, at = '10000000'): EventEnvelope => ({
  stream_id: 'live-phase2', sequence: String(sequence), at, event: { alert_fired: alert({ id, ...patch }) },
});
export const totals = (patch: Partial<RunTotals> = {}): RunTotals => ({
  fights: 19, kills: 7, first_hit: '3000000', last_hit: '7000000', total_damage: 987654,
  unattributed_damage: 0, players: [], local: { player_id: 'player-1', name: 'Invented Archer',
    class_id: 100, is_local: true, damage: 123456, hits: 0, deaths: 0, rank: 17, share: 0.125, dps: 30864 },
  fame_delta: -9, partial: true, ...patch,
});
// Settings is an adjacent rail link, outside the Navigation landmark.
export const primary = () => within(screen.getByRole('complementary', { name: 'Character' }));
export const history = () => within(screen.getByRole('group', { name: 'History' }));
export const disabled = (element: HTMLElement) => element.getAttribute('aria-disabled') === 'true'
  || ('disabled' in element && element.disabled === true);

/** Flush rendering without advancing the rows deadline or any other timer. */
export async function microtasks() {
  for (let index = 0; index < 12; index++) { await Promise.resolve(); await tick(); }
}

/** Traverse until the actual final window, with a fixture-derived termination guard. */
export async function traverse(next: () => HTMLElement, inspect: () => void, maximumWindows: number) {
  for (let index = 0; index < maximumWindows; index++) {
    inspect();
    const control = next();
    if (disabled(control)) return;
    await fireEvent.click(control); await microtasks();
  }
  throw new Error(`Window traversal did not terminate within ${maximumWindows} windows`);
}

export function mountPhase2(options: {
  hash?: string; demo?: boolean; features?: Features; queries?: Partial<RunFixtureQueries>;
  historyCache?: HistoryCache; alerts?: Client['listAlerts']; configure?: (client: Client) => void;
} = {}) {
  let snapshot = homeSnapshot({ stream_id: 'live-phase2', area_kind: 'nexus', run: null, at: '10000000' });
  let store: StoreStatus = saved(snapshot.sequence, snapshot.stream_id);
  let settings: AppSettings = { party: { presets: [], default_preset_id: null }, version: 1, adapter_id: null, assets_path: null, save_recordings: false,
    recap_image: { names: true, loot: true, fights: true },
    recording_limit_bytes: 1_000_000_000, update_channel: 'stable', hardware_acceleration: false, alerts: preferences(), chat: { store: { public: true, guild: true, party: true, whisper: true }, retention_days: 90, hidden_senders: [], hidden_keywords: [], filter_spam: true }, loot: { chase: [] } };
  const queries = {
    listRuns: vi.fn<RunFixtureQueries['listRuns']>(options.queries?.listRuns ?? (async () => ready(runPage()))),
    getRun: vi.fn<RunFixtureQueries['getRun']>(options.queries?.getRun ?? (async id => ready(story({ run: run({ id }) })))),
    getRunFightStrip: vi.fn<NonNullable<RunFixtureQueries['getRunFightStrip']>>(options.queries?.getRunFightStrip ?? (async () => ready([]))),
    getFight: vi.fn<RunFixtureQueries['getFight']>(options.queries?.getFight ?? (async id => ready(fight({ id })))),
    getPlayerBreakdown: vi.fn<RunFixtureQueries['getPlayerBreakdown']>(options.queries?.getPlayerBreakdown ?? (async (_, object_id) => ready(player({ object_id })))),
  };
  const listAlerts = vi.fn<Client['listAlerts']>(options.alerts ?? (async () => ready({ rows: [alert()], next: null })));
  const transport = createFixtureTransport(snapshot, { client_fingerprint: 'phase2-invented', objects: [] }, homeSummary(), queries, { listAlerts });
  const client = createClient(transport);
  const setup = client.querySetup;
  client.querySetup = vi.fn(async () => ({ ...await setup(), demo: options.demo ?? false, live_confirmed: true }));
  client.queryStore = vi.fn(async () => structuredClone(store));
  client.querySettings = vi.fn(async () => structuredClone(settings));
  client.updateSettings = vi.fn(async next => { settings = structuredClone(next); });
  client.updateAlertPreferences = vi.fn(async next => { settings = { ...settings, alerts: structuredClone(next) }; });
  client.listAdapters = vi.fn(async () => []);
  client.setAdapter = vi.fn(async adapter_id => { settings = { ...settings, adapter_id }; });
  client.queryAlertDelivery = vi.fn<Client['queryAlertDelivery']>(async () => ({ permission: 'unavailable', sound_available: false,
    message: 'Synthetic native sound unavailable', flawless_unavailable: 'Flawless has no approved evidence definition.' }));
  const permission = vi.spyOn(client, 'requestNotificationPermission');
  const sound = vi.spyOn(client, 'testAlertDelivery');
  const visuals = vi.spyOn(client, 'getObjectVisuals');
  const listeners = [vi.spyOn(client, 'onSnapshot'), vi.spyOn(client, 'onCapture'), vi.spyOn(client, 'onEngineEvent')];
  options.configure?.(client);
  const browser = fakeHistory(options.hash ?? '#/runs');
  const page = render(Shell, { client, historyAdapter: browser.adapter, features: options.features, historyCache: options.historyCache });
  return { ...page, client, transport, queries, listAlerts, permission, sound, visuals, listeners, browser,
    get snapshot() { return snapshot; },
    get settings() { return settings; },
    setSettings(next: Partial<AppSettings>) { settings = { ...settings, ...next }; },
    setStore(next: StoreStatus) { store = next; },
    event(event: EngineEvent, sequence: string, at = snapshot.at) {
      transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence, at, event });
    },
    publish(next: typeof snapshot) { snapshot = next; transport.publishSnapshot(next); },
  };
}
