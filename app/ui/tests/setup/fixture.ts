import { vi } from 'vitest';
import type { SetupStatus } from '../../src/lib/bindings/SetupStatus';
import type { AppSettings } from '../../src/lib/bindings/AppSettings';
import type { CaptureState } from '../../src/lib/bindings/CaptureState';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import fixture from '../fixtures/t0-live';

export function setupFixture(overrides: Partial<SetupStatus> = {}) {
  const transport = createFixtureTransport({ ...structuredClone(fixture), map: null });
  const client = createClient(transport);
  client.queryHistoryUpgradeStatus = vi.fn(client.queryHistoryUpgradeStatus);
  client.backupAndUpgradeHistory = vi.fn(client.backupAndUpgradeHistory);
  client.queryStorageBackupLocation = vi.fn(client.queryStorageBackupLocation);
  let setup: SetupStatus = {
    npcap: 'compatible', access: 'allowed', assets: 'ready', extraction_stage: null,
    client_found: true, live_confirmed: false, demo: false, message: '', ...overrides,
  };
  let settings: AppSettings = {
    recap_image: { names: true, loot: true, fights: true },
    party: { presets: [], default_preset_id: null },
    loot: { chase: [] },
    chat: { store: { public: true, guild: true, party: true, whisper: true }, retention_days: 90, hidden_senders: [], hidden_keywords: [], filter_spam: true },
    alerts: { whisper: 'toast', guild_message: 'quiet_log_only', party_message: 'quiet_log_only', white_bag: 'banner', personal_best: 'banner', key_popped: 'off', key_pop_dungeons: [], key_pop_excluded_dungeons: [], key_pop_min_difficulty: null, key_pop_exalt_only: false, death: 'banner', flawless_run: 'off', muted: false, windows_notifications: false, volume_percent: 35 },
    version: 1, adapter_id: null, assets_path: null, save_recordings: false,
    recording_limit_bytes: 1_000_000_000, update_channel: 'stable', hardware_acceleration: false,
  };
  client.querySetup = vi.fn(async () => structuredClone(setup));
  client.querySettings = vi.fn(async () => structuredClone(settings));
  client.listAdapters = vi.fn(async () => [{ id: 'synthetic-ethernet', description: 'Synthetic Ethernet' }]);
  client.updateSettings = vi.fn(async (next) => { settings = structuredClone(next); });
  client.setAdapter = vi.fn(async (adapter_id) => { settings = { ...settings, adapter_id }; });
  client.setAssetsPath = vi.fn(async (assets_path) => { settings = { ...settings, assets_path }; });
  client.setDemo = vi.fn(async (demo) => { setup = { ...setup, demo, live_confirmed: false }; });
  client.startCapture = vi.fn(async () => { transport.publishCapture({ ...fixture.capture, state: 'WaitingForGame' }); });
  client.stopCapture = vi.fn(async () => { setup.live_confirmed = false; transport.publishCapture({ ...fixture.capture, state: 'Stopped' }); });
  return {
    client, transport,
    setSetup(patch: Partial<SetupStatus>) { setup = { ...setup, ...patch }; },
    setSettings(patch: Partial<AppSettings>) { settings = { ...settings, ...patch }; },
    capture(state: CaptureState) { transport.publishCapture({ ...fixture.capture, state }); },
  };
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

export async function storageFixture() {
  const f = setupFixture();
  f.transport.setHistoryUpgradeFixture({ schemaVersion: 16, freeBytes: '1000000000' });
  const status = await f.client.queryStorageStatus();
  status.database = { ...status.database, state: 'present', main_bytes: '600000000', wal_bytes: '1000', shm_bytes: '32768', reusable_bytes: '0' };
  status.measured_at = '1900000000000000';
  f.transport.setStorageFixture(status);
  return { ...f, status };
}
