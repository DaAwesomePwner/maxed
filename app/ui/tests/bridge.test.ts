import type { CharacterCalculation } from '../src/lib/bindings/CharacterCalculation';
import type { CharacterCalculationRequest } from '../src/lib/bindings/CharacterCalculationRequest';
import type { CharacterValue } from '../src/lib/bindings/CharacterValue';
import { describe, expect, it, vi } from 'vitest';
import type { LiveSnapshot } from '../src/lib/bindings/LiveSnapshot';
import type { EventEnvelope } from '../src/lib/bindings/EventEnvelope';
import type { Transport } from '../src/lib/bridge/client';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import snapshot from './fixtures/t0-live';
import json from './fixtures/t0-live.json';

it('forwards storage commands and keeps synthetic work independent of subscriptions', async () => {
  const transport = createFixtureTransport(snapshot);
  const invoke = vi.spyOn(transport, 'invoke');
  const client = createClient(transport);
  await client.queryStorageStatus();
  await client.queryStorageStatus({ estimate: true });
  await client.compactStorage();
  await client.cancelStorageMaintenance();
  await client.resumeStorageMaintenance();
  await client.listStorageBackups();
  await client.queryStorageBackupLocation();
  expect(invoke.mock.calls).toEqual([
    ['get_storage_status', undefined], ['get_storage_status', { estimate: true }],
    ['compact_storage', undefined], ['cancel_storage_maintenance', undefined],
    ['resume_storage_maintenance', undefined], ['list_storage_backups', undefined],
    ['get_storage_backup_location', undefined],
  ]);
  const notices = vi.fn();
  const unlisten = await transport.listen('storage-maintenance-changed', notices);
  transport.advanceStorageMaintenance();
  expect(notices.mock.calls[0][0].maintenance.jobs[0].phase).toBe('copying_receipts');
  expect(JSON.stringify(notices.mock.calls)).not.toContain('Synthetic');
  const backup = (await client.listStorageBackups())[0];
  await expect(client.deleteStorageBackup(backup.id)).rejects.toThrow('in use');
  unlisten();
  transport.advanceStorageMaintenance();
  expect((await client.queryStorageStatus()).maintenance?.jobs[0].phase).toBe('reclaimable');
  expect(notices).toHaveBeenCalledTimes(1);
  await client.deleteStorageBackup(backup.id);
  expect(invoke.mock.calls.at(-1)).toEqual(['delete_storage_backup', { id: backup.id }]);
  expect(await client.listStorageBackups()).toEqual([]);
  for (const id of ['../live.sqlite3', '0', '-1', '01', '9223372036854775808', '999']) {
    await expect(client.deleteStorageBackup(id)).rejects.toThrow('invalid');
  }
});

it('storage polling never starts work or estimates and Resume preserves a failure note', async () => {
  const transport = createFixtureTransport(snapshot);
  const client = createClient(transport);
  for (let n = 0; n < 3; n += 1) {
    const status = await client.queryStorageStatus();
    expect(status.maintenance?.jobs).toEqual([]);
    expect(status.receipt_estimate.measured_at).toBeNull();
  }
  await client.compactStorage();
  await client.compactStorage();
  expect((await client.queryStorageStatus()).maintenance?.jobs).toHaveLength(1);
  expect((await client.queryStorageStatus({ estimate: true })).receipt_estimate.measured_at).toBeNull();
  transport.advanceStorageMaintenance('io');
  await client.cancelStorageMaintenance();
  expect((await client.queryStorageStatus()).maintenance?.jobs[0].needs_resume).toBe(true);
  await client.resumeStorageMaintenance();
  expect((await client.queryStorageStatus()).failure).toBe('io');
});

it('blocked fixture work uses needs_resume and renews deleted backup authorization', async () => {
  const transport = createFixtureTransport(snapshot);
  const client = createClient(transport);
  await client.compactStorage();
  transport.advanceStorageMaintenance();
  transport.advanceStorageMaintenance('blocked_stream');
  expect((await client.queryStorageStatus()).maintenance?.jobs[0]).toMatchObject({ paused: false, needs_resume: true, reason: 'blocked_stream' });
  await client.deleteStorageBackup((await client.listStorageBackups())[0].id);
  await client.resumeStorageMaintenance();
  expect((await client.queryStorageStatus()).maintenance?.jobs[0].phase).toBe('awaiting_backup');
  transport.advanceStorageMaintenance();
  expect((await client.queryStorageStatus()).failure).toBe('blocked_stream');
  transport.advanceStorageMaintenance();
  expect((await client.queryStorageStatus()).failure).toBeNull();
});

describe('generated T0 fixture', () => {
  it('matches the Rust JSON and keeps missing values and decimal strings', () => {
    expect(snapshot).toEqual(json);
    const variant = { ...snapshot, sequence: '18446744073709551615', character: null } satisfies LiveSnapshot;
    expect(BigInt(variant.sequence)).toBe(18446744073709551615n);
    expect(variant.character).toBeNull();
    expect(snapshot.fights[0].players[0].damage).toBe(300);
    expect(snapshot.fights[0].players[0].dps).toBe(150);
    expect(snapshot.character.maxed).toBeNull();
  });
});

describe('injected command client', () => {
  it('forwards command names and Tauri argument keys without wrapping payloads', async () => {
    const fixture = createFixtureTransport(snapshot, { client_fingerprint: 'synthetic', objects: [] });
    const invoke = vi.spyOn(fixture, 'invoke');
    const client = createClient(fixture);
    await client.queryCapture();
    await client.querySnapshot();
    await client.querySetup();
    const settings = await client.querySettings();
    await client.queryStore();
    await client.getObjectVisuals([12]);
    await client.getItemDetails(12, [3]);
    await expect(client.listAdapters()).rejects.toThrow('Preview Only');
    await expect(client.startCapture()).rejects.toThrow('Preview Only');
    await expect(client.stopCapture()).rejects.toThrow('Preview Only');
    await expect(client.setAdapter(null)).rejects.toThrow('Preview Only');
    await expect(client.setAssetsPath('synthetic-client')).rejects.toThrow('Preview Only');
    await expect(client.setDemo(true)).rejects.toThrow('Preview Only');
    await expect(client.updateSettings(settings)).rejects.toThrow('Preview Only');
    expect(invoke.mock.calls).toEqual([
      ['get_capture_status', undefined], ['get_live_snapshot', undefined],
      ['get_setup_status', undefined], ['get_settings', undefined],
      ['get_store_status', undefined], ['get_object_visuals', { objectTypes: [12] }],
      ['get_item_details', { objectTypes: [12], enchantIds: [3] }],
      ['list_adapters', undefined], ['start_capture', undefined], ['stop_capture', undefined],
      ['set_adapter', { adapterId: null }], ['set_assets_path', { path: 'synthetic-client' }],
      ['set_demo', { enabled: true }], ['update_settings', { settings }],
    ]);
  });

  it('propagates failed Result queries instead of returning empty defaults', async () => {
    const transport: Transport = {
      invoke: async () => { throw new Error('Safe query failure'); },
      listen: async () => { throw new Error('Safe listener failure'); },
    };
    const client = createClient(transport);
    await expect(client.listAdapters()).rejects.toThrow('Safe query failure');
    await expect(client.querySettings()).rejects.toThrow('Safe query failure');
    await expect(client.getObjectVisuals([1])).rejects.toThrow('Safe query failure');
    await expect(client.onSnapshot(() => {})).rejects.toThrow('Safe listener failure');
  });
});

describe('synthetic fixture transport', () => {
  it('never claims physical setup or persistent store completion', async () => {
    const transport = createFixtureTransport(snapshot);
    const client = createClient(transport);
    expect(transport.demo).toBe(true);
    expect(await client.querySetup()).toMatchObject({
      demo: true, live_confirmed: false, npcap: 'unchecked', access: 'unknown', assets: 'missing',
    });
    expect(await client.queryStore()).toMatchObject({ state: 'closed', durable: null, stream_id: null });
    const settings = await client.querySettings();
    await expect(client.updateSettings({ ...settings, save_recordings: true })).rejects.toThrow();
    expect((await client.querySettings()).save_recordings).toBe(false);
    await expect(client.getObjectVisuals([1])).rejects.toThrow('unavailable');
  });

  it('keeps queries and listener payloads isolated and releases all event subscriptions', async () => {
    const transport = createFixtureTransport(snapshot);
    const client = createClient(transport);
    const query = await client.querySnapshot();
    query.fights.length = 0;
    expect((await client.querySnapshot()).fights).toHaveLength(1);
    const seenSnapshot = vi.fn();
    const seenCapture = vi.fn();
    const seenEvent = vi.fn();
    const releaseMutation = await client.onSnapshot((value) => { if ('base_sequence' in value) throw new Error('Expected full fixture'); value.sequence = '0'; });
    const releaseSnapshot = await client.onSnapshot(seenSnapshot);
    const releaseCapture = await client.onCapture(seenCapture);
    const releaseEvent = await client.onEngineEvent(seenEvent, () => { throw new Error('Expected fixture single'); });
    const next = { ...snapshot, sequence: '9007199254740993', character: null } satisfies LiveSnapshot;
    const event = {
      stream_id: next.stream_id, sequence: next.sequence, at: next.at,
      event: { player_died: { at: next.at, character_id: null, run_id: null } },
    } satisfies EventEnvelope;
    transport.publishSnapshot(next);
    transport.publishCapture({ ...next.capture, state: 'Recovering' });
    transport.publishEngineEvent(event);
    expect(seenSnapshot).toHaveBeenCalledWith(next);
    expect(seenCapture).toHaveBeenCalledWith(expect.objectContaining({ state: 'Recovering' }));
    expect(seenEvent).toHaveBeenCalledWith(event);
    expect((await client.querySnapshot()).sequence).toBe(next.sequence);
    expect((await client.queryCapture()).state).toBe('Recovering');
    for (const release of [releaseMutation, releaseSnapshot, releaseCapture, releaseEvent]) {
      release();
      release();
    }
    transport.publishSnapshot(snapshot);
    transport.publishCapture(snapshot.capture);
    transport.publishEngineEvent(event);
    expect(seenSnapshot).toHaveBeenCalledTimes(1);
    expect(seenCapture).toHaveBeenCalledTimes(1);
    expect(seenEvent).toHaveBeenCalledTimes(1);
  });

  it('bounds visual requests, deduplicates types, and leaves unknown visuals null', async () => {
    const known = { object_type: 200, name: 'Synthetic Sword', sprite: null, bag: null };
    const client = createClient(createFixtureTransport(snapshot, {
      client_fingerprint: 'synthetic', objects: [known],
    }));
    expect(await client.getObjectVisuals([200, 65535, 200])).toEqual({
      client_fingerprint: 'synthetic',
      objects: [known, { object_type: 65535, name: null, sprite: null, bag: null }],
    });
    expect((await client.getObjectVisuals(Array(129).fill(200))).objects).toHaveLength(1);
    expect((await client.getObjectVisuals(Array.from({ length: 128 }, (_, i) => i))).objects).toHaveLength(128);
    for (const types of [[-1], [65536], [1.5], [NaN], Array.from({ length: 129 }, (_, i) => i)]) {
      await expect(client.getObjectVisuals(types)).rejects.toThrow('128 unique object types');
    }
  });
});

it('forwards run fight strip identifiers and defaults the fixture to an empty ready strip', async () => {
  const fixture = createFixtureTransport(snapshot);
  const invoke = vi.spyOn(fixture, 'invoke');
  const client = createClient(fixture);
  await expect(client.getRunFightStrip('demo-run', null)).resolves.toEqual({ status: 'ready', value: [] });
  await client.getRunFightStrip('demo-run', 'TestMage');
  expect(invoke.mock.calls).toEqual([
    ['get_run_fight_strip', { runId: 'demo-run', playerId: null }],
    ['get_run_fight_strip', { runId: 'demo-run', playerId: 'TestMage' }],
  ]);
});

it('serves synthetic item details and preserves unknown enchant rows', async () => {
  const transport = createFixtureTransport(snapshot, {
    client_fingerprint: 'a'.repeat(64),
    objects: [{ object_type: 10, name: 'Synthetic Sword', item_tier: 'UT', sprite: null, bag: null }],
  }, undefined, undefined, undefined, undefined, undefined, {
    enchants: [{ id: 1, name: 'Synthetic Enchant', description: 'Synthetic effect' }],
  });
  const client = createClient(transport);
  expect(await client.getItemDetails(10, [1, 2])).toEqual({
    item: { object_type: 10, name: 'Synthetic Sword', tier: 'UT', slot_type: null, slot_label: null, soulbound: null },
    enchants: [{ id: 1, name: 'Synthetic Enchant', description: 'Synthetic effect' }, { id: 2, name: null, description: null }],
  });
  expect((await client.getItemDetails(65535, [])).item).toEqual({
    object_type: 65535, name: null, tier: null, slot_type: null, slot_label: null, soulbound: null,
  });
  await expect(client.getItemDetails(10, Array.from({ length: 129 }, (_, i) => i))).rejects.toThrow('128 unique enchant IDs');
  await expect(transport.invoke('get_item_details', { objectTypes: Array.from({ length: 129 }, (_, i) => i), enchantIds: [] })).rejects.toThrow();
  await expect(transport.invoke('get_item_details', { objectTypes: [], enchantIds: Array.from({ length: 129 }, (_, i) => i) })).rejects.toThrow();
});

it('clears item details when asset-changing commands fail', async () => {
  const fixture = createFixtureTransport(snapshot, { client_fingerprint: 'a'.repeat(64), objects: [] });
  const invoke = vi.spyOn(fixture, 'invoke');
  const client = createClient(fixture);
  const settings = await client.querySettings();
  for (const action of [() => client.setAssetsPath('synthetic'), () => client.setDemo(true), () => client.updateSettings(settings)]) {
    await client.getItemDetails(10, []);
    await expect(action()).rejects.toThrow('Preview Only');
    invoke.mockClear();
    await client.getItemDetails(10, []);
    expect(invoke).toHaveBeenCalledWith('get_item_details', { objectTypes: [10], enchantIds: [] });
  }
});


const characterRequest = {
  defense: 25, comparison_item_types: [201, 202],
  expected_revision: { stream_id: 'synthetic-stream', character_id: 'synthetic-character',
    input_hash: 'input-a', catalog_fingerprint: 'synthetic-catalog' },
} satisfies CharacterCalculationRequest;
const unknownCharacterValue: CharacterValue = { value: null, evidence: 'modeled', completeness: 'unavailable',
  reasons: ['missing_stats'], exclusions: [] };
const characterCalculation: CharacterCalculation = {
  revision: characterRequest.expected_revision, observed_at: '100', catalog_fingerprint: 'synthetic-catalog',
  formula_version: 'synthetic-v1', freshness: 'live', issues: [], applied_defense: 25, weapons: [],
  regeneration: { applied_wis: null, wis: unknownCharacterValue, effects: [],
    hp_subtotal: unknownCharacterValue, mp_subtotal: unknownCharacterValue,
    hp_total: unknownCharacterValue, mp_total: unknownCharacterValue,
    base_vit: unknownCharacterValue, base_mp: unknownCharacterValue },
  exalt_damage: { status: 'deferred', value: null, reason: 'deferred' },
};

it('forwards Character requests without subscribing and returns authored fixture DTOs by value', async () => {
  const callback = vi.fn(async (_request: CharacterCalculationRequest) => characterCalculation);
  const transport = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined, undefined,
    undefined, { characterCalculation: callback });
  const invoke = vi.spyOn(transport, 'invoke'), listen = vi.spyOn(transport, 'listen');
  const result = await createClient(transport).getCharacterCalculation(characterRequest);
  expect(invoke).toHaveBeenCalledExactlyOnceWith('get_character_calculation', { request: characterRequest });
  expect(callback).toHaveBeenCalledExactlyOnceWith(characterRequest);
  expect(callback.mock.calls[0][0]).not.toBe(characterRequest);
  expect(result).toEqual(characterCalculation);
  expect(result).not.toBe(characterCalculation);
  expect(listen).not.toHaveBeenCalled();
});

it('rejects missing Character fixtures and propagates supplied command failures', async () => {
  await expect(createClient(createFixtureTransport(snapshot)).getCharacterCalculation(characterRequest))
    .rejects.toThrow(/Character calculation.*not supplied/);
  const failure = new Error('Character catalog unavailable');
  const transport = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined, undefined,
    undefined, { characterCalculation: async () => { throw failure; } });
  await expect(createClient(transport).getCharacterCalculation(characterRequest)).rejects.toBe(failure);
});
