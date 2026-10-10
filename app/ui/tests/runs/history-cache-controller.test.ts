import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createRunHistory, historyKey, type HistoryInput, type HistoryState, type HistoryValue } from '../../src/lib/runs/history';
import { createHistoryCache } from '../../src/lib/runs/history-cache';
import { historyWaiting } from '../../src/lib/runs/policy';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import type { Route } from '../../src/lib/navigation/routes';
import { deferred, flush, homeSnapshot, homeSummary, saved } from '../home/fixtures';

const disposals: Array<() => void> = [];
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const story = (id: string): RunStory => ({ run: { ...homeSummary().recent_runs[0], id }, character: null,
  fights: [], drops: [], timeline: [], comparison: null });
const ready = (id: string): HistoryReply<RunStory | null> => ({ status: 'ready', value: story(id) });
function harness(withRoute = true) {
  let clock = 0;
  let route: Route | undefined = withRoute ? { kind: 'run', id: 'first' } : undefined;
  const cache = createHistoryCache({ now: () => clock, route: () => route, invalidates: () => true });
  const pending: Array<ReturnType<typeof deferred<HistoryReply<RunStory | null>>>> = [];
  const getRun = vi.fn(async (_id: string) => {
    const job = deferred<HistoryReply<RunStory | null>>(); pending.push(job); return job.promise;
  });
  const client = createClient(createFixtureTransport(homeSnapshot(), undefined, undefined, {
    getRun, listRuns: async () => ({ status: 'ready', value: { rows: [], next: null } }),
    getFight: async () => ({ status: 'ready', value: null }), getPlayerBreakdown: async () => ({ status: 'ready', value: null }),
  }));
  const states: HistoryState[] = [];
  const controller = createRunHistory(client, state => states.push(state), { cache });
  disposals.push(controller.dispose);
  const input: HistoryInput = { query: { kind: 'run', id: 'first' }, scopeKey: 'live',
    streamId: 'live-synthetic-home', required: '12', store: saved('12') };
  const cached: HistoryValue = { kind: 'run', story: story('cached') };
  const key = historyKey(input);
  const put = (required: string | null = '12') => cache.put(key, { value: cached, query: input.query, route: { kind: 'run', id: 'first' }, required });
  const dirty = (sequence: string) => cache.observe({ stream_id: input.streamId!, sequence, at: '1',
    event: { bag_dropped: homeSnapshot().drops[0] } });
  return { client, controller, getRun, pending, states, input, cache, cached, key, put, dirty,
    advance: (ms: number) => { clock += ms; }, navigate: () => { route = { kind: 'runs' }; }, last: () => states.at(-1)! };
}

it('fresh_hit_renders_ready_without_reading', async () => {
  const h = harness(); h.put(); h.controller.update(h.input);
  expect(h.states[0]).toMatchObject({ phase: 'ready', value: h.cached });
  expect(h.last().value).toBe(h.cached);
  await flush(); await vi.advanceTimersByTimeAsync(2000);
  expect(h.getRun).not.toHaveBeenCalled();
});

it('fresh_hit_keeps_the_cached_gate_when_durability_catches_up', async () => {
  const h = harness(); h.put();
  h.controller.update({ ...h.input, required: '13' });
  expect(h.last().phase).toBe('ready');
  h.controller.update({ ...h.input, required: '13', store: saved('13') });
  await flush(); await vi.advanceTimersByTimeAsync(2000);
  expect(h.getRun).not.toHaveBeenCalled(); expect(h.last().phase).toBe('ready');
});

it('fresh_hit_then_requirement_change_reads', async () => {
  const h = harness(); h.put(); h.controller.update(h.input);
  h.controller.update({ ...h.input, required: '13', store: saved('12') }); await flush();
  expect(h.getRun).not.toHaveBeenCalled(); expect(h.last().phase).toBe('stale');
  h.controller.update({ ...h.input, required: '13', store: saved('13') }); await flush();
  expect(h.getRun).toHaveBeenCalledOnce();
});

it.each([['12', '15'], [null, '15'], ['9007199254740993', '9007199254740992']])(
  'stale_hit_waits_for_maximum_sequence_%s_%s', async (required, dirty) => {
    const h = harness(); h.put(); h.dirty(dirty!);
    const gate = required !== null && BigInt(required) > BigInt(dirty!) ? required : dirty!;
    const input = { ...h.input, required, store: saved(String(BigInt(gate) - 1n)) };
    h.controller.update(input); await flush();
    expect(h.last()).toMatchObject({ phase: 'stale', value: h.cached }); expect(h.getRun).not.toHaveBeenCalled();
    h.controller.update({ ...input, store: saved(gate) }); await flush();
    expect(h.getRun).toHaveBeenCalledOnce();
    h.pending[0].resolve(ready('refreshed')); await flush();
    expect(h.last()).toMatchObject({ phase: 'ready', value: { story: { run: { id: 'refreshed' } } } });
    expect(h.cache.get(h.key)).toMatchObject({ required: gate, dirtySince: null });
  });

it.each(['age', 'dirtyAll'])('%s_entry_refreshes_in_background', async mode => {
  const h = harness(); h.put();
  if (mode === 'age') h.advance(300_000); else h.cache.invalidateAll();
  h.controller.update(h.input);
  expect(h.states[0]).toMatchObject({ phase: 'stale', value: h.cached, message: 'Refreshing saved observations.' });
  await flush(); expect(h.getRun).toHaveBeenCalledOnce();
});

it.each(['failed', 'backpressured'] as const)('store_%s_over_fresh_hit_shows_warning_with_value', async state => {
  const h = harness(); h.put(); const input = { ...h.input, store: { ...saved('12'), state } };
  h.controller.update(input); await flush();
  expect(h.last()).toMatchObject({ phase: 'stale', value: h.cached, message: historyWaiting(input) });
  expect(h.getRun).not.toHaveBeenCalled();
});

it('retry_reads_on_fresh_hit', async () => {
  const h = harness(); h.put(); h.controller.update({ ...h.input, required: '13' }); h.controller.retry(); await flush();
  expect(h.getRun).not.toHaveBeenCalled();
  h.controller.update({ ...h.input, required: '13', store: saved('13') }); await flush();
  expect(h.getRun).toHaveBeenCalledOnce();
});

it('non_ready_replies_are_not_cached', async () => {
  const h = harness(); h.controller.update(h.input); await flush();
  h.pending[0].resolve({ status: 'busy' }); await flush();
  h.pending[1].resolve({ status: 'busy' }); await flush();
  expect(h.cache.get(h.key)).toBeUndefined();
  h.controller.retry(); await flush(); h.pending[2].reject(new Error('synthetic')); await flush();
  expect(h.cache.get(h.key)).toBeUndefined();
  h.controller.retry(); await flush(); h.pending[3].resolve({ status: 'superseded' }); await flush();
  expect(h.cache.get(h.key)).toBeUndefined();
});

it('no_route_means_no_cache_read_or_write', async () => {
  const h = harness(false); h.put(); const get = vi.spyOn(h.cache, 'get'), put = vi.spyOn(h.cache, 'put');
  h.controller.update(h.input); await flush(); h.pending[0].resolve(ready('read')); await flush();
  expect(h.getRun).toHaveBeenCalledOnce(); expect(get).not.toHaveBeenCalled(); expect(put).not.toHaveBeenCalled();
});

it('accepted_reply_uses_request_identity_and_captured_route', async () => {
  const h = harness(); h.controller.update(h.input); await flush(); h.navigate();
  h.controller.update({ ...h.input, required: '13', store: saved('13') });
  h.pending[0].resolve(ready('accepted')); await flush();
  expect(h.cache.get(h.key)).toMatchObject({ required: '12', query: h.input.query, route: { kind: 'run', id: 'first' }, value: h.last().value });
});

it.each(['identity', 'dispose', 'storage'])('rejected_ready_reply_after_%s_is_not_cached', async reason => {
  const h = harness(); h.controller.update(h.input); await flush();
  if (reason === 'identity') h.controller.update({ ...h.input, query: { kind: 'run', id: 'second' } });
  else if (reason === 'dispose') h.controller.dispose();
  else h.controller.update({ ...h.input, store: { ...saved('12'), state: 'failed' } });
  h.pending[0].resolve(ready('obsolete')); await flush();
  expect(h.cache.get(h.key)).toBeUndefined();
});

it('fresh_null_gate_does_not_fall_back_to_input_requirement', async () => {
  const h = harness(); h.put(null); h.controller.update({ ...h.input, store: saved(null) }); await flush();
  expect(h.last().phase).toBe('ready'); expect(h.getRun).not.toHaveBeenCalled();
});


it('another_route_hit_renders_cached_value_and_refreshes', async () => {
  const h = harness();
  h.cache.put(h.key, { value: h.cached, query: h.input.query, route: { kind: 'loot', view: 'feed' }, required: '12' });
  h.controller.update(h.input);
  expect(h.last()).toMatchObject({ phase: 'stale', value: h.cached });
  await flush(); expect(h.getRun).toHaveBeenCalledOnce();
});

it.each(['event', 'all'])('in_flight_%s_survives_ready_and_refreshes_next_mount', async mode => {
  const h = harness(); h.controller.update(h.input); await flush();
  if (mode === 'event') h.dirty('13'); else h.cache.invalidateAll();
  h.pending[0].resolve(ready('read')); await flush();
  expect(h.cache.get(h.key)).toMatchObject({ dirtySince: mode === 'event' ? '13' : null, dirtyAll: mode === 'all' });
  h.controller.dispose();
  const states: HistoryState[] = [];
  const next = createRunHistory(h.client, state => states.push(state), { cache: h.cache });
  disposals.push(next.dispose);
  next.update({ ...h.input, store: saved('13') });
  expect(states[0]).toMatchObject({ phase: 'stale', value: h.cache.get(h.key)!.value });
  await flush(); expect(h.getRun).toHaveBeenCalledTimes(2);
});

it.each(['dispose', 'identity'])('%s_releases_in_flight_reservation_immediately', async reason => {
  const h = harness(), begin = vi.spyOn(h.cache, 'begin');
  h.controller.update(h.input); await flush();
  const token = begin.mock.results[0].value as number;
  if (reason === 'dispose') h.controller.dispose();
  else h.controller.update({ ...h.input, query: { kind: 'run', id: 'second' } });
  h.dirty('13'); h.cache.invalidateAll();
  h.cache.put(h.key, { value: h.cached, query: h.input.query, route: { kind: 'run', id: 'first' }, required: '12' }, token);
  expect(h.cache.get(h.key)).toMatchObject({ dirtySince: null, dirtyAll: false });
});
