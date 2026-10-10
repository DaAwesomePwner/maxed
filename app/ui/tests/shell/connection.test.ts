import { afterEach, expect, it, vi } from 'vitest';
import { connectShell } from '../../src/lib/shell/connection';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import type { StoreStatus } from '../../src/lib/bindings/StoreStatus';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import fixture from '../fixtures/t0-live';

const callbacks = () => ({ snapshot: vi.fn(), capture: vi.fn(), visuals: vi.fn(), store: vi.fn(), state: vi.fn(), error: vi.fn() });
const flush = async () => { for (let i = 0; i < 40; i++) await Promise.resolve(); };
const disposals: (() => void)[] = [];
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); });
const ready = (durable = '12'): StoreStatus => ({ state: 'ready', queued: 0, durable, stream_id: fixture.stream_id, message: '' });

it('registers one complete subscription before querying and retains a newer buffered snapshot', async () => {
  const transport = createFixtureTransport(fixture); const client = createClient(transport); const receive = callbacks();
  const calls: string[] = [];
  const original = { ...client };
  client.onSnapshot = receive => { calls.push('onSnapshot'); return original.onSnapshot(receive); };
  client.onEngineEvent = (receive, batch) => { calls.push('onEngineEvent'); return original.onEngineEvent(receive, batch); };
  client.onCapture = receive => { calls.push('onCapture'); return original.onCapture(receive); };
  let resolve!: (snapshot: LiveSnapshot) => void;
  client.querySnapshot = vi.fn(() => { calls.push('query'); return new Promise<LiveSnapshot>((done) => { resolve = done; }); });
  const dispose = connectShell(client, receive); disposals.push(dispose); await flush();
  expect(calls).toEqual(['onSnapshot', 'onEngineEvent', 'onCapture', 'query']);
  transport.publishSnapshot({ ...fixture, sequence: '9007199254740993' });
  resolve(fixture); await flush();
  expect(receive.snapshot).toHaveBeenCalledTimes(1);
  expect(receive.snapshot.mock.calls[0][0].sequence).toBe('9007199254740993');
  transport.publishSnapshot({ ...fixture, sequence: '9007199254740992' });
  expect(receive.snapshot).toHaveBeenCalledTimes(1);
  dispose(); transport.publishSnapshot({ ...fixture, sequence: '9007199254740994' });
  expect(receive.snapshot).toHaveBeenCalledTimes(1);
});

it('releases a delayed registration after teardown without starting queries or more listeners', async () => {
  const client = createClient(createFixtureTransport(fixture)); const receive = callbacks(); const release = vi.fn();
  let registered!: (release: () => void) => void;
  client.onSnapshot = () => new Promise((resolve) => { registered = resolve; });
  const query = vi.spyOn(client, 'queryStore'); const events = vi.spyOn(client, 'onEngineEvent');
  const dispose = connectShell(client, receive); await flush();
  expect(query).not.toHaveBeenCalled();
  dispose(); registered(release); await flush();
  expect(release).toHaveBeenCalledOnce();
  expect(events).not.toHaveBeenCalled();
});

it('reports registration failure safely and releases the partial subscription', async () => {
  const client = createClient(createFixtureTransport(fixture)); const receive = callbacks(); const release = vi.fn();
  client.onSnapshot = async () => release;
  client.onEngineEvent = vi.fn().mockRejectedValue(new Error('PRIVATE'));
  disposals.push(connectShell(client, receive)); await flush();
  expect(release).toHaveBeenCalledOnce();
  expect(receive.error).toHaveBeenCalledWith('snapshot', 'Disconnected. Reconnect.');
  expect(receive.state).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'failed', historyStale: true }));
});

it('keeps live snapshots flowing when storage fails even with a matching watermark', async () => {
  vi.useFakeTimers();
  const transport = createFixtureTransport(fixture); const client = createClient(transport); const receive = callbacks();
  const failed: StoreStatus = { ...ready(), state: 'failed', queued: 1 };
  client.queryStore = vi.fn().mockResolvedValueOnce(ready()).mockResolvedValue(failed);
  const dispose = connectShell(client, receive); disposals.push(dispose); await flush();
  expect(receive.state).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'live', historyStale: false, historyFailed: false, message: '' }));
  await vi.advanceTimersByTimeAsync(10_000);
  expect(receive.store).toHaveBeenLastCalledWith(failed);
  expect(receive.state).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'live', historyStale: true, historyFailed: true }));
  transport.publishSnapshot({ ...fixture, sequence: '13', map: 'Still live' });
  expect(receive.snapshot).toHaveBeenLastCalledWith(expect.objectContaining({ map: 'Still live' }));
  expect(receive.error.mock.calls.every(([, message]) => message === '')).toBe(true);
  dispose(); await vi.advanceTimersByTimeAsync(20_000);
  expect(client.queryStore).toHaveBeenCalledTimes(2);
});

it('gates ordinary events on durability, resyncs a gap and retires the previous stream', async () => {
  vi.useFakeTimers();
  const transport = createFixtureTransport(fixture); const client = createClient(transport); const receive = callbacks();
  client.queryStore = vi.fn().mockResolvedValue(ready());
  disposals.push(connectShell(client, receive)); await flush();
  const event = (sequence: string): EventEnvelope => ({ stream_id: fixture.stream_id, sequence, at: fixture.at, event: { run_ended: { ...fixture.run, ended: fixture.at } } });
  const storeCalls = vi.mocked(client.queryStore).mock.calls.length;
  transport.publishEngineEvent({ stream_id: fixture.stream_id, sequence: '13', at: fixture.at, event: { capture_state_changed: fixture.capture } });
  await flush();
  expect(receive.state).toHaveBeenLastCalledWith(expect.objectContaining({ historyStale: false }));
  transport.publishEngineEvent({ stream_id: fixture.stream_id, sequence: '14', at: fixture.at, event: { fight_updated: fixture.fights[0] } });
  await flush();
  expect(receive.state).toHaveBeenLastCalledWith(expect.objectContaining({ historyStale: false }));
  expect(client.queryStore).toHaveBeenCalledTimes(storeCalls);
  transport.publishEngineEvent(event('15')); await flush();
  expect(receive.state).toHaveBeenLastCalledWith(expect.objectContaining({ historyStale: true }));
  client.queryStore = vi.fn().mockResolvedValue(ready('15'));
  await vi.advanceTimersByTimeAsync(1000);
  expect(receive.state).toHaveBeenLastCalledWith(expect.objectContaining({ historyStale: false }));
  client.querySnapshot = vi.fn().mockResolvedValue({ ...fixture, sequence: '17', map: 'Resynced map' });
  transport.publishEngineEvent(event('17')); await flush();
  expect(client.querySnapshot).toHaveBeenCalledOnce();
  expect(receive.snapshot).toHaveBeenLastCalledWith(expect.objectContaining({ map: 'Resynced map' }));
  const restarted = { ...fixture, stream_id: 'live-new', sequence: '1', character: null, fights: [] };
  client.querySnapshot = vi.fn().mockResolvedValue(restarted);
  transport.publishSnapshot(restarted); await flush();
  const count = receive.snapshot.mock.calls.length;
  transport.publishSnapshot({ ...fixture, sequence: '999' }); await flush();
  expect(receive.snapshot).toHaveBeenCalledTimes(count);
  expect(receive.snapshot).toHaveBeenLastCalledWith(restarted);
});

it('discards retired visual replies and retries failures on a timer, not on snapshots', async () => {
  vi.useFakeTimers();
  const transport = createFixtureTransport(fixture); const client = createClient(transport); const receive = callbacks();
  let resolve!: (page: import('../../src/lib/bindings/VisualPage').VisualPage) => void;
  client.getObjectVisuals = vi.fn().mockImplementationOnce(() => new Promise(done => { resolve = done; }))
    .mockRejectedValueOnce(new Error('PRIVATE old assets'))
    .mockResolvedValue({ client_fingerprint: 'b'.repeat(64), objects: [] });
  disposals.push(connectShell(client, receive)); await flush();
  const next = { ...fixture, stream_id: 'live-restarted', sequence: '1' };
  client.querySnapshot = vi.fn().mockResolvedValue(next);
  transport.publishSnapshot(next); await flush();
  resolve({ client_fingerprint: 'a'.repeat(64), objects: [] }); await flush();
  expect(receive.visuals.mock.calls.some(([page]) => page.client_fingerprint === 'a'.repeat(64))).toBe(false);
  expect(receive.visuals).toHaveBeenLastCalledWith({ client_fingerprint: '', objects: [] });
  transport.publishSnapshot({ ...next, sequence: '2' }); await flush();
  expect(client.getObjectVisuals).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1000);
  expect(receive.visuals).toHaveBeenLastCalledWith({ client_fingerprint: 'b'.repeat(64), objects: [] });
});

it('refreshes a stopped rail after asset commands without needing another snapshot and unsubscribes on disposal', async () => {
  const transport = createFixtureTransport(fixture); const client = createClient(transport); const receive = callbacks();
  let fingerprint = 'a'.repeat(64);
  client.getObjectVisuals = vi.fn(async () => ({ client_fingerprint: fingerprint, objects: [] }));
  const dispose = connectShell(client, receive); await flush();
  expect(receive.visuals).toHaveBeenLastCalledWith({ client_fingerprint: fingerprint, objects: [] });
  vi.spyOn(transport, 'invoke').mockResolvedValue(undefined);
  fingerprint = 'b'.repeat(64);
  await client.setAssetsPath('synthetic'); await flush();
  expect(receive.visuals).toHaveBeenLastCalledWith({ client_fingerprint: fingerprint, objects: [] });
  expect(receive.snapshot).toHaveBeenCalledOnce();
  dispose(); const calls = receive.visuals.mock.calls.length;
  await client.setAssetsPath('synthetic'); await flush();
  expect(receive.visuals).toHaveBeenCalledTimes(calls);
});

it('does not refetch visuals on unchanged snapshots and bounds unavailable-catalog retries', async () => {
  vi.useFakeTimers();
  const transport = createFixtureTransport(fixture); const client = createClient(transport); const receive = callbacks();
  client.getObjectVisuals = vi.fn().mockResolvedValue({ client_fingerprint: 'a'.repeat(64), objects: [] });
  const dispose = connectShell(client, receive); disposals.push(dispose); await flush();
  for (let i = 13; i <= 52; i++) {
    transport.publishSnapshot({ ...fixture, sequence: String(i) });
    await vi.advanceTimersByTimeAsync(250);
  }
  expect(client.getObjectVisuals).toHaveBeenCalledOnce();
  client.getObjectVisuals = vi.fn().mockRejectedValue(new Error('private catalog path'));
  transport.publishSnapshot({ ...fixture, sequence: '53', character: { ...fixture.character, class_id: 101 } });
  await flush();
  expect(client.getObjectVisuals).toHaveBeenCalledOnce();
  for (let i = 54; i <= 85; i++) {
    transport.publishSnapshot({ ...fixture, sequence: String(i), character: { ...fixture.character, class_id: 101 } });
    await vi.advanceTimersByTimeAsync(250);
  }
  expect(client.getObjectVisuals).toHaveBeenCalledTimes(4); // initial failure, then 1, 3, 7 seconds
  await vi.advanceTimersByTimeAsync(23_000);
  expect(client.getObjectVisuals).toHaveBeenCalledTimes(6); // then 15 and 31 seconds
  await vi.advanceTimersByTimeAsync(30_000);
  expect(client.getObjectVisuals).toHaveBeenCalledTimes(7); // capped at 30 seconds
  dispose(); await vi.advanceTimersByTimeAsync(60_000);
  expect(client.getObjectVisuals).toHaveBeenCalledTimes(7);
});
