import { expect, it, vi } from 'vitest';
import { createNexusHistory, type NexusHistory } from '../../src/lib/home/v3/nexus-history';
import type { HomeHistoryInput } from '../../src/lib/home/summary';
import { deferred, flush, homeSummary, saved } from '../home/fixtures';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { ScopeLootPage } from '../../src/lib/bindings/ScopeLootPage';
import { historyWaiting } from '../../src/lib/runs/policy';

const input: HomeHistoryInput = { mode: 'nexus', scope: { session: { id: 's' } }, streamId: 'live-synthetic-home', required: '12', store: saved() };
const page = (id: number, next: string | null = null): ScopeLootPage => ({ items: [{ item_type: id, tier: null, count: 5000, best_enchant: null, last_at: '100' }], next, total_bags: 5000 });

it.each(['in flight', 'queued behind loot'] as const)('keeps a summary %s valid when the blocked reason changes', async position => {
  const summaryGate = deferred<HistoryReply<ReturnType<typeof homeSummary>>>();
  const lootGate = deferred<HistoryReply<ScopeLootPage>>();
  const refreshed = homeSummary({ runs: 42 });
  const queryHome = vi.fn();
  if (position === 'queued behind loot') queryHome.mockResolvedValueOnce({ status: 'ready', value: homeSummary() });
  queryHome.mockReturnValueOnce(summaryGate.promise).mockResolvedValue({ status: 'ready', value: refreshed });
  const client = { queryHome,
    queryScopeLoot: vi.fn().mockReturnValueOnce(lootGate.promise).mockResolvedValue({ status: 'ready', value: page(2) }),
    queryScopeLootPage: vi.fn() };
  const states: NexusHistory[] = [];
  const history = createNexusHistory(client, state => states.push(state));
  if (position === 'queued behind loot') {
    history.update(input); await flush(); await flush();
    expect(client.queryScopeLoot).toHaveBeenCalledOnce();
  }
  // A new stream may read ahead while its matching store catches up.
  const blocked: HomeHistoryInput = { ...input, streamId: 'next-stream', scope: { session: { id: 'next' } },
    required: '20', initial: true, store: saved('19', 'next-stream') };
  history.update(blocked); await flush(); await flush();
  expect(queryHome).toHaveBeenCalledOnce();
  const backlogged: HomeHistoryInput = { ...blocked, store: { ...blocked.store!, state: 'backpressured' } };
  expect(historyWaiting(blocked)).not.toBeNull();
  expect(historyWaiting(backlogged)).not.toBeNull();
  expect(historyWaiting(blocked)).not.toBe(historyWaiting(backlogged));
  history.update(backlogged); await flush();
  if (position === 'queued behind loot') {
    lootGate.resolve({ status: 'ready', value: page(1) });
    await flush(); await flush();
    expect(queryHome).toHaveBeenCalledTimes(2);
    summaryGate.resolve({ status: 'ready', value: refreshed });
  } else {
    // Native replacement must still retry the current read-ahead request.
    summaryGate.resolve({ status: 'superseded' });
  }
  await flush(); await flush();
  expect(queryHome).toHaveBeenCalledTimes(2);
  expect(states.some(state => state.summary.phase === 'superseded')).toBe(false);
  expect(states.at(-1)?.summary.summary?.runs).toBe(42);
  expect(states.at(-1)?.summary.phase).toBe('stale');
  history.update({ ...blocked, store: saved('20', 'next-stream') });
  await flush(); await flush();
  expect(states.at(-1)?.summary.phase).toBe('ready');
  expect(states.at(-1)?.summary.summary?.runs).toBe(42);
  history.dispose();
  lootGate.resolve({ status: 'ready', value: page(1) }); await flush();
});
it('keeps summary ready during loot loading and pages without re-querying summary', async () => {
  const gate = deferred<HistoryReply<ScopeLootPage>>();
  const client = { queryHome: vi.fn().mockResolvedValue({ status: 'ready', value: homeSummary() }),
    queryScopeLoot: vi.fn().mockReturnValueOnce(gate.promise).mockResolvedValue({ status: 'ready', value: page(1, 'next') }),
    queryScopeLootPage: vi.fn().mockResolvedValue({ status: 'ready', value: page(2) }) };
  const states: NexusHistory[] = [];
  const history = createNexusHistory(client, state => states.push(state));
  history.update(input); await flush(); await flush();
  expect(client.queryHome).toHaveBeenCalledOnce();
  expect(client.queryScopeLoot).toHaveBeenCalledWith(input.scope, 32);
  expect(states.at(-1)?.summary.phase).toBe('ready');
  expect(states.at(-1)?.loot.phase).toBe('loading');
  const retained = states.at(-1)?.summary.summary;
  gate.resolve({ status: 'ready', value: page(1, 'next') }); await flush();
  for (let i = 0; i < 20; i++) history.update({ ...input, store: saved(String(13 + i)) });
  history.next(); await flush(); await flush();
  expect(client.queryScopeLootPage).toHaveBeenCalledWith(input.scope, 'next', 32);
  expect(states.at(-1)?.loot.value?.items.map(item => item.item_type)).toEqual([2]);
  expect(states.at(-1)?.page).toBe(2);
  history.first(); await flush(); await flush();
  expect(states.at(-1)?.page).toBe(1);
  expect(states.at(-1)?.summary.summary).toBe(retained);
  expect(client.queryHome).toHaveBeenCalledOnce();
  history.dispose();
});
it.each(['busy', 'error'] as const)('keeps a ready summary when loot is %s', async status => {
  const queryScopeLoot = status === 'error' ? vi.fn().mockRejectedValue(new Error('failed loot'))
    : vi.fn().mockResolvedValue({ status });
  const client = { queryHome: vi.fn().mockResolvedValue({ status: 'ready', value: homeSummary() }), queryScopeLoot, queryScopeLootPage: vi.fn() };
  const states: NexusHistory[] = [];
  const history = createNexusHistory(client, state => states.push(state));
  history.update(input); await flush(); await flush(); await flush();
  expect(states.at(-1)?.summary.phase).toBe('ready');
  expect(states.at(-1)?.summary.summary).not.toBeNull();
  expect(states.at(-1)?.loot.phase).toBe(status);
  expect(queryScopeLoot).toHaveBeenCalledTimes(status === 'busy' ? 2 : 1);
  expect(client.queryHome).toHaveBeenCalledOnce();
  {
    queryScopeLoot.mockResolvedValue({ status: 'ready', value: page(1) });
    history.retryLoot(); await flush(); await flush();
    expect(states.at(-1)?.loot.phase).toBe('ready');
    expect(client.queryHome).toHaveBeenCalledOnce();
  }
  history.dispose();
});
it('discards retired scopes and serializes queued reads without publishing after disposal', async () => {
  const gate = deferred<HistoryReply<ScopeLootPage>>();
  const client = { queryHome: vi.fn().mockResolvedValue({ status: 'ready', value: homeSummary() }),
    queryScopeLoot: vi.fn().mockReturnValueOnce(gate.promise).mockResolvedValue({ status: 'ready', value: page(2) }), queryScopeLootPage: vi.fn() };
  const states: NexusHistory[] = [];
  const history = createNexusHistory(client, state => states.push(state));
  history.update(input); await flush(); await flush();
  history.update({ ...input, scope: { today: { range: { start: '0', end: '1000' } } } });
  await flush(); expect(client.queryHome).toHaveBeenCalledOnce();
  gate.resolve({ status: 'ready', value: page(999) }); await flush(); await flush(); await flush();
  expect(states.some(state => state.loot.value?.items.some(item => item.item_type === 999))).toBe(false);
  expect(states.at(-1)?.page).toBe(1);
  expect(states.at(-1)?.loot.value?.items[0].item_type).toBe(2);
  history.dispose(); const count = states.length;
  history.retry(); history.retryLoot(); history.next(); history.update(input); await flush();
  expect(states).toHaveLength(count);
});

it('reissues superseded summary and loot reads serially without an input change', async () => {
  const summaryGate = deferred<HistoryReply<ReturnType<typeof homeSummary>>>();
  const lootGate = deferred<HistoryReply<ScopeLootPage>>();
  const client = {
    queryHome: vi.fn().mockResolvedValueOnce({ status: 'superseded' }).mockReturnValueOnce(summaryGate.promise),
    queryScopeLoot: vi.fn().mockResolvedValueOnce({ status: 'superseded' }).mockReturnValueOnce(lootGate.promise),
    queryScopeLootPage: vi.fn().mockResolvedValueOnce({ status: 'superseded' }).mockResolvedValue({ status: 'ready', value: page(2) }),
  };
  const states: NexusHistory[] = [];
  const history = createNexusHistory(client, state => states.push(state));
  history.update(input); await flush(); await flush();
  expect(client.queryHome).toHaveBeenCalledTimes(2);
  expect(client.queryScopeLoot).not.toHaveBeenCalled();
  summaryGate.resolve({ status: 'ready', value: homeSummary() }); await flush(); await flush();
  expect(client.queryScopeLoot).toHaveBeenCalledTimes(2);
  expect(states.at(-1)?.summary.phase).toBe('ready');
  lootGate.resolve({ status: 'ready', value: page(1, 'next') }); await flush();
  history.next(); await flush(); await flush();
  expect(client.queryScopeLootPage).toHaveBeenCalledTimes(2);
  expect(states.at(-1)?.loot.value?.items[0].item_type).toBe(2);
  expect(states.at(-1)?.page).toBe(2);
  history.dispose();
});
