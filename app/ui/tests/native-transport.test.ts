import type { CharacterCalculation } from '../src/lib/bindings/CharacterCalculation';
import type { CharacterCalculationRequest } from '../src/lib/bindings/CharacterCalculationRequest';
import type { CharacterValue } from '../src/lib/bindings/CharacterValue';
import { expect, test, vi } from 'vitest';
import fixture from './fixtures/t0-live';
import type { LiveSnapshot } from '../src/lib/bindings/LiveSnapshot';
import { render, screen } from '@testing-library/svelte';
import App from '../src/App.svelte';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import { flushHomeRender } from './home/shell-fixture';

const mocks = vi.hoisted(() => ({ listen: vi.fn(), invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke, Channel: class { onmessage = (_value: ArrayBuffer) => {}; } }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
import { readFileSync } from 'node:fs';
import { LIVE_PAYLOAD_EVENTS, nativeTransport } from '../src/lib/bridge/native';

test.each(['live-snapshot', 'party-changed'] as const)('native %s subscriptions decode byte channels and release native ownership', async event => {
  let channel!: { onmessage(value: ArrayBuffer): void };
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === 'subscribe_live_payload') { channel = args.channel; return 19; }
  });
  mocks.listen.mockResolvedValueOnce(() => {});
  const receive = vi.fn();
  const stop = await nativeTransport.listen(event, receive);
  const payload = event === 'party-changed' ? { scope_id: null } : fixture;
  expect(mocks.invoke).toHaveBeenCalledWith('subscribe_live_payload', { event, channel });
  expect(channel).toBeDefined();
  channel.onmessage(new TextEncoder().encode(JSON.stringify(payload)).buffer);
  expect(receive).toHaveBeenCalledExactlyOnceWith(payload);
  stop();
  expect(mocks.invoke).toHaveBeenCalledWith('unsubscribe_live_payload', { id: 19 });
  channel.onmessage(new TextEncoder().encode(JSON.stringify(payload)).buffer);
  expect(receive).toHaveBeenCalledTimes(1);
});

test.each(['account-history-changed', 'storage-maintenance-changed', 'capture-status', 'presentation-visible'] as const)('%s listens directly without a live-payload channel subscription', async event => {
  mocks.invoke.mockReset();
  let callback!: (message: { payload: unknown }) => void;
  const release = vi.fn();
  mocks.listen.mockImplementationOnce((_event, receive) => { callback = receive; return Promise.resolve(release); });
  const receive = vi.fn();
  const stop = await nativeTransport.listen(event, receive);
  expect(mocks.invoke).not.toHaveBeenCalledWith('subscribe_live_payload', expect.anything());
  const payload = { revision: 2, reason: 'account_switched' };
  callback({ payload });
  expect(receive).toHaveBeenCalledExactlyOnceWith(payload);
  stop();
  expect(release).toHaveBeenCalledOnce();
  callback({ payload });
  expect(receive).toHaveBeenCalledOnce();
});

test('the production App uses the JSON-decoding native transport', async () => {
  const initial: LiveSnapshot = structuredClone(fixture);
  initial.capture.state = 'Live'; initial.character!.name = 'Initial local';
  const transport = createFixtureTransport(initial);
  const callbacks = new Map<string, (message: { payload: unknown }) => void>();
  mocks.invoke.mockImplementation((command, args) => command === 'subscribe_live_payload' ? Promise.resolve(20) : command === 'unsubscribe_live_payload' ? Promise.resolve() : transport.invoke(command, args));
  mocks.listen.mockImplementation((event, receive) => { callbacks.set(event, receive); return Promise.resolve(() => { callbacks.delete(event); }); });
  const page = render(App);
  try {
    for (let i = 0; i < 5; i++) await flushHomeRender();
    expect(screen.getByText('Initial local')).toBeTruthy();
    const next = structuredClone(initial); next.sequence = '13'; next.character!.name = 'Updated local';
    callbacks.get('live-snapshot')!({ payload: JSON.stringify(next) });
    for (let i = 0; i < 5; i++) await flushHomeRender();
    expect(screen.getByText('Updated local')).toBeTruthy();
  } finally { page.unmount(); }
});

test.each(['live-snapshot', 'engine-event', 'party-changed'] as const)('decodes JSON text for %s without changing the logical payload', async event => {
  mocks.invoke.mockResolvedValue(21);
  let callback!: (message: { payload: unknown }) => void;
  const release = vi.fn();
  mocks.listen.mockImplementationOnce((_event, receive) => { callback = receive; return Promise.resolve(release); });
  const receive = vi.fn();
  const stop = await nativeTransport.listen(event, receive);
  const payload = event === 'live-snapshot' ? fixture : event === 'party-changed' ? { scope_id: 'synth-scope' } : {
    stream_id: fixture.stream_id, sequence: '9007199254740993', at: fixture.at,
    event: { fight_updated: fixture.fights[0] },
  };
  callback({ payload: JSON.stringify(payload) });
  expect(receive).toHaveBeenLastCalledWith(payload);
  callback({ payload }); // Original before-mode and low-frequency typed events.
  expect(receive).toHaveBeenLastCalledWith(payload);
  stop(); expect(release).toHaveBeenCalledOnce();
});

test('a byte-channel event batch remains one callback with its complete range', async () => {
  let channel!: { onmessage(value: ArrayBuffer): void };
  mocks.invoke.mockImplementation(async (command, args) => {
    if (command === 'subscribe_live_payload') { channel = args.channel; return 22; }
  });
  mocks.listen.mockResolvedValueOnce(() => {});
  const receive = vi.fn(), stop = await nativeTransport.listen('engine-event', receive);
  const events = Array.from({ length: 1536 }, (_, i) => ({ stream_id: fixture.stream_id,
    sequence: String(i + 11), at: fixture.at, event: { fight_ended: fixture.fights[0] } }));
  const batch = { stream_id: fixture.stream_id, first_sequence: '11', last_sequence: '1546', events };
  channel.onmessage(new TextEncoder().encode(JSON.stringify(batch)).buffer);
  expect(receive).toHaveBeenCalledExactlyOnceWith(batch);
  stop();
});


test('forwards the typed Character command and preserves native rejection without subscribing', async () => {
  const request: CharacterCalculationRequest = {
    defense: 100, comparison_item_types: [201], expected_revision: {
      stream_id: 'synthetic-stream', character_id: 'synthetic-character',
      input_hash: 'input-a', catalog_fingerprint: null,
    },
  };
  const unknown: CharacterValue = { value: null, evidence: 'modeled', completeness: 'unavailable',
    reasons: ['missing_catalog'], exclusions: [] };
  const result: CharacterCalculation = { revision: request.expected_revision, observed_at: '100',
    catalog_fingerprint: null, formula_version: 'synthetic-v1', freshness: 'last_seen', issues: [],
    applied_defense: 100, weapons: [], regeneration: { applied_wis: null, wis: unknown, effects: [],
      hp_subtotal: unknown, mp_subtotal: unknown, hp_total: unknown, mp_total: unknown,
      base_vit: unknown, base_mp: unknown },
    exalt_damage: { status: 'deferred', value: null, reason: 'deferred' } };
  mocks.invoke.mockClear(); mocks.listen.mockClear();
  mocks.invoke.mockResolvedValueOnce(result);
  await expect(nativeTransport.invoke('get_character_calculation', { request })).resolves.toBe(result);
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('get_character_calculation', { request });
  mocks.invoke.mockClear();
  const failure = new Error('Character catalog unavailable');
  mocks.invoke.mockRejectedValueOnce(failure);
  await expect(nativeTransport.invoke('get_character_calculation', { request })).rejects.toBe(failure);
  expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith('get_character_calculation', { request });
  expect(mocks.listen).not.toHaveBeenCalled();
});

test('channel subscriptions match the native subscribe allowlist', () => {
  const nativeChannels = readFileSync('../src-tauri/src/bridge/channels.rs', 'utf8');
  const match = nativeChannels.match(/if\s*!\s*matches\s*!\s*\(\s*event\s*\.\s*as_str\s*\(\s*\)\s*,([\s\S]*?)\)\s*\{/);
  expect(match).not.toBeNull();
  const nativeEvents = [...match![1].matchAll(/(["'])([^"']+)\1/g)].map(value => value[2]).sort();
  expect(nativeEvents.length).toBeGreaterThan(0);
  expect([...LIVE_PAYLOAD_EVENTS].sort()).toEqual(nativeEvents);
});

test('existing store polls carry active live ownership IDs and preserve the loss code', async () => {
  const { LIVE_SUBSCRIPTION_LOST } = await import('../src/lib/bridge/subscription-health');
  mocks.invoke.mockReset();
  mocks.listen.mockResolvedValue(() => {});
  let nextId = 100;
  mocks.invoke.mockImplementation(async command => command === 'subscribe_live_payload' ? nextId++ : undefined);
  const snapshotStop = await nativeTransport.listen('live-snapshot', () => {});
  const eventStop = await nativeTransport.listen('engine-event', () => {});
  const partyStop = await nativeTransport.listen('party-changed', () => {});
  try {
    await nativeTransport.invoke('get_store_status', undefined);
    expect(mocks.invoke).toHaveBeenLastCalledWith('get_store_status', { subscriptionIds: [100, 101] });
    mocks.invoke.mockRejectedValueOnce(LIVE_SUBSCRIPTION_LOST);
    await expect(nativeTransport.invoke('get_store_status', undefined)).rejects.toBe(LIVE_SUBSCRIPTION_LOST);
    snapshotStop();
    await nativeTransport.invoke('get_store_status', undefined);
    expect(mocks.invoke).toHaveBeenLastCalledWith('get_store_status', { subscriptionIds: [101] });
  } finally { snapshotStop(); eventStop(); partyStop(); }
  await nativeTransport.invoke('get_store_status', undefined);
  expect(mocks.invoke).toHaveBeenLastCalledWith('get_store_status', undefined);
  const rust = readFileSync('../src-tauri/src/bridge/channels.rs', 'utf8');
  expect(rust).toContain(`pub const LIVE_SUBSCRIPTION_LOST: &str = "${LIVE_SUBSCRIPTION_LOST}";`);
});
