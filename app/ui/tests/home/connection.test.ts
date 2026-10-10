import { afterEach, expect, it, vi } from 'vitest';
import { connectShell } from '../../src/lib/shell/connection';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import { flush, homeSnapshot, saved } from './fixtures';

const disposals: (() => void)[] = [];
afterEach(() => {
  try {
    disposals.splice(0).forEach(dispose => dispose());
  } finally {
    vi.useRealTimers();
    vi.restoreAllMocks();
  }
});
const snapshot = homeSnapshot();
const changes: EngineEvent[] = [
  { run_started: snapshot.run! }, { run_updated: snapshot.run! }, { run_ended: { ...snapshot.run!, ended: '6000000' } },
  { fight_ended: snapshot.fights[0] }, { bag_dropped: snapshot.drops[0] }, { drop_updated: snapshot.drops[0] },
  { player_died: { at: '6000000', character_id: snapshot.character!.id, run_id: snapshot.run!.id } },
  { character_merged: { provisional_id: 'synthetic-provisional', verified_id: snapshot.character!.id } },
  { fame_changed: { at: '6000000', character_id: snapshot.character!.id, fame: 1300 } },
];

it.each(changes)('forwards discrete invalidation only after its matching store watermark: %j', async event => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  const transport = createFixtureTransport(snapshot); const client = createClient(transport);
  const receive = { snapshot: vi.fn(), capture: vi.fn(), store: vi.fn(), visuals: vi.fn(), error: vi.fn(), event: vi.fn(), liveState: vi.fn(), historyReady: vi.fn() };
  let status = saved(); client.queryStore = vi.fn(async () => status);
  disposals.push(connectShell(client, receive)); await flush(); receive.historyReady.mockClear();
  const envelope = { stream_id: snapshot.stream_id, sequence: '13', at: '6000000', event };
  transport.publishEngineEvent(envelope); await flush();
  expect(receive.event).toHaveBeenCalledWith(envelope);
  expect(receive.liveState).toHaveBeenLastCalledWith(expect.objectContaining({ historyStale: true }));
  expect(receive.historyReady).not.toHaveBeenCalled();
  status = saved('13'); await vi.advanceTimersByTimeAsync(1000);
  expect(receive.historyReady).toHaveBeenCalledWith(status);
});

it('snapshot-before-event resync establishes the same durable target without duplicate event application', async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); const transport = createFixtureTransport(snapshot); const client = createClient(transport);
  const receive = { snapshot: vi.fn(), capture: vi.fn(), store: vi.fn(), visuals: vi.fn(), error: vi.fn(), event: vi.fn(), historyReady: vi.fn() };
  let status = saved(); client.queryStore = vi.fn(async () => status);
  disposals.push(connectShell(client, receive)); await flush(); receive.historyReady.mockClear();
  transport.publishSnapshot({ ...snapshot, sequence: '14', area_kind: 'nexus', run: null });
  transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: '14', at: '6000000', event: changes[2] });
  await flush(); expect(receive.historyReady).not.toHaveBeenCalled();
  status = saved('14'); await vi.advanceTimersByTimeAsync(1000);
  expect(receive.historyReady).toHaveBeenCalledWith(status);
  // Snapshots supply numeric state, but cannot replace discrete side effects.
  // Resync must deliver the buffered event exactly once even if already drawn.
  expect(receive.event).toHaveBeenCalledTimes(1);
  expect(receive.event).toHaveBeenCalledWith(expect.objectContaining({ sequence: '14', event: changes[2] }));
});

it('store IPC failure does not stop Dungeon snapshots and disposal releases all listeners', async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); const transport = createFixtureTransport(snapshot); const client = createClient(transport);
  const releases = [vi.fn(), vi.fn(), vi.fn()];
  const registrations = [client.onSnapshot, client.onCapture, client.onEngineEvent] as const;
  client.onSnapshot = async receive => { const release = await registrations[0](receive); return () => { release(); releases[0](); }; };
  client.onCapture = async receive => { const release = await registrations[1](receive); return () => { release(); releases[1](); }; };
  client.onEngineEvent = async (receive, batch) => { const release = await registrations[2](receive, batch); return () => { release(); releases[2](); }; };
  const receive = { snapshot: vi.fn(), capture: vi.fn(), store: vi.fn(), visuals: vi.fn(), error: vi.fn(), liveState: vi.fn() };
  client.queryStore = vi.fn().mockRejectedValue(new Error('PRIVATE storage path'));
  const dispose = connectShell(client, receive); await flush();
  transport.publishSnapshot({ ...snapshot, sequence: '13', map: 'Synthetic continued dungeon' });
  expect(receive.snapshot).toHaveBeenLastCalledWith(expect.objectContaining({ map: 'Synthetic continued dungeon' }));
  expect(receive.liveState).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'live', historyStale: true }));
  dispose(); for (const release of releases) expect(release).toHaveBeenCalledOnce();
  const calls = receive.snapshot.mock.calls.length;
  transport.publishSnapshot({ ...snapshot, sequence: '14' }); await vi.advanceTimersByTimeAsync(10000);
  expect(receive.snapshot).toHaveBeenCalledTimes(calls);
});

it('FightUpdated numeric ticks do not wake history or mark a ready summary stale', async () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); const transport = createFixtureTransport(snapshot); const client = createClient(transport);
  const receive = { snapshot: vi.fn(), capture: vi.fn(), store: vi.fn(), visuals: vi.fn(), error: vi.fn(), liveState: vi.fn() };
  client.queryStore = vi.fn(async () => saved());
  disposals.push(connectShell(client, receive)); await flush();
  for (let sequence = 13; sequence < 33; sequence++) {
    transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: String(sequence), at: String(6000000 + sequence * 250000), event: { fight_updated: snapshot.fights[0] } });
    await vi.advanceTimersByTimeAsync(250);
  }
  expect(client.queryStore).toHaveBeenCalledOnce();
  expect(receive.liveState).toHaveBeenLastCalledWith(expect.objectContaining({ historyStale: false }));
});
