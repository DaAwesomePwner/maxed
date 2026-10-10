import { describe, expect, it, vi } from 'vitest';
import type { LiveSnapshot } from '../src/lib/bindings/LiveSnapshot';
import type { EventEnvelope } from '../src/lib/bindings/EventEnvelope';
import type { Transport } from '../src/lib/bridge/client';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import snapshot from './fixtures/t0-live';
import json from './fixtures/t0-live.json';

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
