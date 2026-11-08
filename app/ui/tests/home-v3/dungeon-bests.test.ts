import { afterEach, expect, it, vi } from 'vitest';
import type { DungeonBest } from '../../src/lib/bindings/DungeonBest';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { StoreStatus } from '../../src/lib/bindings/StoreStatus';
import { createDungeonBests, createHomeReads, pbBaseline } from '../../src/lib/home/v3/dungeon-bests';
import { homeSnapshot } from '../home/fixtures';

afterEach(() => { vi.useRealTimers(); });

const storeAt = (durable: string, stream_id = homeSnapshot().stream_id, state: StoreStatus['state'] = 'ready'): StoreStatus =>
  ({ state, queued: 0, durable, stream_id, message: '' });

it('refreshes thousands of completions without replaying evicted or retired inputs', async () => {
  const queryDungeonBests = vi.fn().mockResolvedValue({ status: 'ready', value: [] });
  const controller = createDungeonBests({ queryDungeonBests }, vi.fn());
  const flush = async () => { for (let i = 0; i < 5; i++) await Promise.resolve(); };
  await flush();
  for (let i = 1; i <= 1000; i++) {
    const snapshot = homeSnapshot({ sequence: String(i) });
    snapshot.run = { ...snapshot.run!, id: `run-${i}`, ended: snapshot.at };
    controller.update(snapshot, null, storeAt(String(i)));
    await flush();
  }
  expect(queryDungeonBests).toHaveBeenCalledTimes(1001);
  const stale = homeSnapshot({ sequence: '1' });
  stale.run = { ...stale.run!, id: 'run-1', ended: stale.at };
  controller.update(stale, null, storeAt('1000'));
  const latest = { ...stale, sequence: '1001', run: { ...stale.run, id: 'run-1000' } };
  controller.update(latest, null, storeAt('1001'));
  await flush();
  expect(queryDungeonBests).toHaveBeenCalledTimes(1001);
  controller.update(homeSnapshot({ stream_id: 'new', run: null }), null, storeAt('1', 'new'));
  controller.update(null, { stream_id: stale.stream_id, sequence: '9999', at: stale.at,
    event: { run_ended: stale.run } }, storeAt('1', 'new'));
  await flush();
  expect(queryDungeonBests).toHaveBeenCalledTimes(1001);
  controller.dispose();
});

it('reads on mount and once on completion, independently of Session/Today or numeric churn', async () => {
  const snapshot = homeSnapshot();
  const before = [{ dungeon: snapshot.run!.dungeon, best_clear_micros: 719_000_000 }];
  const after = [{ ...before[0], best_clear_micros: 700_000_000 }];
  const queryDungeonBests = vi.fn().mockResolvedValueOnce({ status: 'ready', value: before })
    .mockResolvedValue({ status: 'ready', value: after });
  const publish = vi.fn();
  const controller = createDungeonBests({ queryDungeonBests }, publish);
  await vi.waitFor(() => expect(publish).toHaveBeenLastCalledWith(before));
  for (let i = 0; i < 40; i++) controller.update({ ...snapshot, at: String(900000000 + i) }, null, storeAt('90'));
  expect(queryDungeonBests).toHaveBeenCalledTimes(1);
  const completed = { ...snapshot.run!, completed_at: '701000000', ended: '702000000' };
  controller.update({ ...snapshot, run: completed }, null, storeAt('90'));
  // The snapshot and engine event refer to the same completion, not two reads.
  controller.update({ ...snapshot, run: null }, { stream_id: snapshot.stream_id, sequence: '90',
    at: completed.ended!, event: { run_ended: completed } }, storeAt('90'));
  await vi.waitFor(() => expect(publish).toHaveBeenLastCalledWith(after));
  expect(queryDungeonBests).toHaveBeenCalledTimes(2);
  controller.dispose();
});
it('queues a completion during mount loading and ignores results after disposal', async () => {
  let resolve!: (reply: HistoryReply<DungeonBest[]>) => void;
  const queryDungeonBests = vi.fn().mockImplementationOnce(() => new Promise<HistoryReply<DungeonBest[]>>(done => resolve = done))
    .mockResolvedValue({ status: 'ready', value: [] });
  const publish = vi.fn();
  const controller = createDungeonBests({ queryDungeonBests }, publish);
  const snapshot = homeSnapshot(); snapshot.run!.completed_at = snapshot.at;
  controller.update(snapshot, null, storeAt(snapshot.sequence));
  expect(queryDungeonBests).toHaveBeenCalledTimes(1);
  resolve({ status: 'ready', value: [] });
  await vi.waitFor(() => expect(queryDungeonBests).toHaveBeenCalledTimes(2));
  controller.dispose();
  const late = vi.fn();
  const old = createDungeonBests({ queryDungeonBests: () => new Promise(done => resolve = done) }, late);
  old.dispose(); resolve({ status: 'ready', value: [{ dungeon: 'old source', best_clear_micros: 1 }] });
  await Promise.resolve(); await Promise.resolve();
  expect(late).not.toHaveBeenCalled();
});
it.each(['busy', 'error'])('keeps the last baseline after %s without polling', async status => {
  const saved = [{ dungeon: 'all time', best_clear_micros: 1000000 }];
  const queryDungeonBests = vi.fn().mockResolvedValueOnce({ status: 'ready', value: saved });
  if (status === 'error') queryDungeonBests.mockRejectedValue(new Error('offline'));
  else queryDungeonBests.mockResolvedValue({ status });
  const publish = vi.fn(); const controller = createDungeonBests({ queryDungeonBests }, publish);
  await vi.waitFor(() => expect(publish).toHaveBeenCalledWith(saved));
  const snapshot = homeSnapshot(); snapshot.run!.ended = snapshot.at;
  controller.update(snapshot, null, storeAt(snapshot.sequence));
  await vi.waitFor(() => expect(queryDungeonBests).toHaveBeenCalledTimes(2));
  expect(publish).toHaveBeenCalledTimes(1);
  controller.dispose();
});

it('reissues superseded PB reads without waiting for a new snapshot', async () => {
  vi.useFakeTimers();
  const saved = [{ dungeon: 'all time', best_clear_micros: 1000000 }];
  const queryDungeonBests = vi.fn().mockResolvedValueOnce({ status: 'superseded' })
    .mockResolvedValueOnce({ status: 'superseded' }).mockResolvedValue({ status: 'ready', value: saved });
  const publish = vi.fn(), controller = createDungeonBests({ queryDungeonBests }, publish);
  await vi.advanceTimersByTimeAsync(3999);
  expect(queryDungeonBests).toHaveBeenCalledTimes(2);
  expect(publish).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  expect(publish).toHaveBeenCalledWith(saved);
  expect(queryDungeonBests).toHaveBeenCalledTimes(3);
  controller.dispose();
});
it('serializes summary, scope loot and PB reads and returns supersession to the retrying owner', async () => {
  const calls: string[] = [];
  let release!: (reply: HistoryReply<never[]>) => void;
  const queryHome = vi.fn(() => { calls.push('summary'); return new Promise<HistoryReply<never[]>>(done => release = done); });
  const queryScopeLoot = vi.fn().mockImplementationOnce(async () => { calls.push('loot'); return { status: 'superseded' }; })
    .mockImplementation(async () => { calls.push('loot'); return { status: 'ready', value: { items: [], next: null, total_bags: 0 } }; });
  const queryDungeonBests = vi.fn(async () => { calls.push('bests'); return { status: 'ready' as const, value: [] }; });
  const owner = { queryHome, queryScopeLoot, queryScopeLootPage: queryScopeLoot, queryDungeonBests };
  const reads = createHomeReads(owner as unknown as import('../../src/lib/bridge/client').Client);
  const scope = { session: { id: 's' } };
  const summary = reads.client.queryHome(scope), loot = reads.client.queryScopeLoot(scope, 20), bests = reads.client.queryDungeonBests();
  await vi.waitFor(() => expect(calls).toEqual(['summary']));
  release({ status: 'ready', value: [] });
  await Promise.all([summary, loot, bests]);
  expect(calls).toEqual(['summary', 'loot', 'bests']);
  expect(await loot).toEqual({ status: 'superseded' });
  reads.dispose();
});

it('reads a completion back only after the store has saved through it', async () => {
  const queryDungeonBests = vi.fn().mockResolvedValue({ status: 'ready', value: [] });
  const controller = createDungeonBests({ queryDungeonBests }, vi.fn());
  await vi.waitFor(() => expect(queryDungeonBests).toHaveBeenCalledTimes(1));
  const snapshot = homeSnapshot(); snapshot.run!.completed_at = snapshot.at;
  controller.update(snapshot, null, storeAt('11'));
  controller.update(snapshot, null, null);
  controller.update(snapshot, null, storeAt('20', 'live-synthetic-home', 'failed'));
  controller.update(snapshot, null, storeAt('20', 'live-synthetic-home', 'backpressured'));
  await Promise.resolve();
  expect(queryDungeonBests).toHaveBeenCalledTimes(1);
  controller.update(snapshot, null, storeAt('12'));
  await vi.waitFor(() => expect(queryDungeonBests).toHaveBeenCalledTimes(2));
  controller.update(snapshot, null, storeAt('13'));
  await Promise.resolve();
  expect(queryDungeonBests).toHaveBeenCalledTimes(2);
  controller.dispose();
});
it('reads a completion once the writer has moved on to a newer stream', async () => {
  const queryDungeonBests = vi.fn().mockResolvedValue({ status: 'ready', value: [] });
  const controller = createDungeonBests({ queryDungeonBests }, vi.fn());
  await vi.waitFor(() => expect(queryDungeonBests).toHaveBeenCalledTimes(1));
  const snapshot = homeSnapshot(); snapshot.run!.ended = snapshot.at;
  controller.update(snapshot, null, storeAt('3'));
  await Promise.resolve();
  expect(queryDungeonBests).toHaveBeenCalledTimes(1);
  // A store status from an unrelated stream says nothing about this run.
  controller.update(snapshot, null, storeAt('1', 'older-stream'));
  await Promise.resolve();
  expect(queryDungeonBests).toHaveBeenCalledTimes(1);
  const next = homeSnapshot({ stream_id: 'next-stream', run: null });
  controller.update(next, null, storeAt('1', 'next-stream'));
  await vi.waitFor(() => expect(queryDungeonBests).toHaveBeenCalledTimes(2));
  controller.dispose();
});
it('does not read a new stream completion while the store still reports the previous stream', async () => {
  const queryDungeonBests = vi.fn().mockResolvedValue({ status: 'ready', value: [] });
  const controller = createDungeonBests({ queryDungeonBests }, vi.fn());
  await vi.waitFor(() => expect(queryDungeonBests).toHaveBeenCalledTimes(1));
  const snapshot = homeSnapshot({ stream_id: 'new-stream' }); snapshot.run!.completed_at = snapshot.at;
  controller.update(snapshot, null, storeAt('999', 'old-stream'));
  await Promise.resolve();
  expect(queryDungeonBests).toHaveBeenCalledTimes(1);
  controller.update(snapshot, null, storeAt(snapshot.sequence, 'new-stream'));
  await vi.waitFor(() => expect(queryDungeonBests).toHaveBeenCalledTimes(2));
  controller.dispose();
});
it('judges a run against the bests read before it completed, not a read that includes it', () => {
  const snapshot = homeSnapshot();
  const dungeon = snapshot.run!.dungeon;
  const loading = pbBaseline(null, snapshot, null);
  expect(loading.rows).toBeNull();
  const old = [{ dungeon, best_clear_micros: 719_000_000 }];
  const live = pbBaseline(loading, snapshot, old);
  expect(live.rows).toBe(old);
  expect(pbBaseline(live, { ...snapshot, at: '6000000' }, old)).toBe(live);
  const finished = { ...snapshot, run: { ...snapshot.run!, completed_at: '701000000' } };
  const includesRun = [{ dungeon, best_clear_micros: 700_000_000 }];
  expect(pbBaseline(live, finished, includesRun).rows).toBe(old);
  const next = { ...snapshot, run: { ...snapshot.run!, id: 'next-run' } };
  expect(pbBaseline(pbBaseline(live, finished, includesRun), next, includesRun).rows).toBe(includesRun);
  // A baseline that never loaded before completion takes the first read.
  expect(pbBaseline(loading, finished, includesRun).rows).toBe(includesRun);
});
