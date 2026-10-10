import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createRunHistory, type HistoryInput, type HistoryState } from '../../src/lib/runs/history';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import { deferred, flush, homeSnapshot, homeSummary, saved } from '../home/fixtures';

const disposals: Array<() => void> = [];
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });
const story = (id: string): RunStory => ({ run: { ...homeSummary().recent_runs[0], id }, character: null,
  fights: [], drops: [], timeline: [], comparison: null });
const ready = (id: string): HistoryReply<RunStory | null> => ({ status: 'ready', value: story(id) });
function harness() {
  const pending: Array<ReturnType<typeof deferred<HistoryReply<RunStory | null>>>> = [];
  const getRun = vi.fn(async (_id: string) => {
    const job = deferred<HistoryReply<RunStory | null>>(); pending.push(job); return job.promise;
  });
  const client = createClient(createFixtureTransport(homeSnapshot(), undefined, undefined, {
    getRun, listRuns: async () => ({ status: 'ready', value: { rows: [], next: null } }),
    getFight: async () => ({ status: 'ready', value: null }), getPlayerBreakdown: async () => ({ status: 'ready', value: null }),
  }));
  const states: HistoryState[] = [];
  const controller = createRunHistory(client, state => states.push(state));
  disposals.push(controller.dispose);
  const input: HistoryInput = { query: { kind: 'run', id: 'first' }, scopeKey: 'live',
    streamId: 'live-synthetic-home', required: '12', store: saved('12') };
  return { controller, getRun, pending, states, input, last: () => states.at(-1)! };
}

it('holds the first dirty checkpoint while newer requirements arrive and converges after quiet', async () => {
  const h = harness();
  h.controller.update({ ...h.input, store: saved('11') });
  h.controller.update({ ...h.input, required: '13', store: saved('11') });
  await flush(); expect(h.getRun).not.toHaveBeenCalled();
  h.controller.update({ ...h.input, required: '14', store: saved('12') }); await flush();
  expect(h.getRun).toHaveBeenCalledOnce();
  h.pending[0].resolve(ready('checkpoint-12')); await flush();
  expect(h.last()).toMatchObject({ phase: 'stale', value: { story: { run: { id: 'checkpoint-12' } } } });
  h.controller.update({ ...h.input, required: '14', store: saved('14') }); await flush();
  expect(h.getRun).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2);
  h.pending[1].resolve(ready('checkpoint-14')); await flush();
  expect(h.last()).toMatchObject({ phase: 'ready', value: { story: { run: { id: 'checkpoint-14' } } } });
  await vi.advanceTimersByTimeAsync(3000); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2);
});

it('durability reaching an unchanged checkpoint releases exactly one read', async () => {
  const h = harness(); h.controller.update({ ...h.input, store: saved('11') }); await flush();
  expect(h.getRun).not.toHaveBeenCalled();
  h.controller.update(h.input); await flush();
  h.pending[0].resolve(ready('once')); await flush();
  await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(h.getRun).toHaveBeenCalledOnce(); expect(h.last().phase).toBe('ready');
});

it('keeps a ready value stale during a same-stream resync checkpoint and drains one read', async () => {
  const h = harness(); h.controller.update(h.input); await flush();
  h.pending[0].resolve(ready('before-resync')); await flush();
  expect(h.last().phase).toBe('ready');
  const visible = h.last().value;
  const baseline = { ...h.input, required: '15' };
  h.controller.update(baseline); await flush();
  expect(h.last()).toMatchObject({ phase: 'stale', value: visible });
  expect(h.last().value).toBe(visible);
  expect(h.getRun).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(1000); await flush();
  h.controller.update({ ...baseline, store: saved('15') }); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2);
  expect(h.last().value).toBe(visible);
  h.pending[1].resolve(ready('after-resync')); await flush();
  expect(h.last()).toMatchObject({ phase: 'ready', value: { story: { run: { id: 'after-resync' } } } });
  h.controller.update({ ...baseline, store: saved('15') });
  await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2);
});

it('retires changed query and namespace replies, including return to the same key', async () => {
  const h = harness(); h.controller.update(h.input); await flush();
  h.controller.update({ ...h.input, query: { kind: 'run', id: 'second' } });
  h.controller.update({ ...h.input, scopeKey: 'new-baseline' });
  h.pending[0].resolve(ready('obsolete')); await flush();
  expect(h.states.some(state => state.value?.kind === 'run' && state.value.story?.run.id === 'obsolete')).toBe(false);
  expect(h.getRun).toHaveBeenCalledTimes(2);
  h.pending[1].resolve(ready('current')); await flush();
  expect(h.last()).toMatchObject({ phase: 'ready', value: { story: { run: { id: 'current' } } } });
});

it('keeps Superseded inert until a new eligible input and never offers Retry', async () => {
  const h = harness(); h.controller.update(h.input); await flush();
  h.pending[0].resolve({ status: 'superseded' }); await flush();
  h.controller.update(h.input); h.controller.retry();
  await vi.advanceTimersByTimeAsync(5000); await flush();
  expect(h.getRun).toHaveBeenCalledOnce();
  expect(h.last()).toMatchObject({ phase: 'superseded', retryAvailable: false, value: null });
  h.controller.update({ ...h.input, required: '13', store: saved('13') }); await flush();
  h.pending[1].resolve(ready('accepted')); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2); expect(h.last().phase).toBe('ready');
});

it('automatically retries deferred Busy once, then waits for manual Retry', async () => {
  const h = harness(); h.controller.update(h.input); await flush();
  expect(h.getRun).toHaveBeenCalledOnce();
  h.pending[0].resolve({ status: 'busy' }); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2);
  h.pending[1].resolve({ status: 'busy' }); await flush();
  h.controller.update(h.input); await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2); expect(h.last()).toMatchObject({ phase: 'busy', retryAvailable: true });
  h.controller.retry(); await flush(); h.pending[2].resolve(ready('retried')); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(3); expect(h.last().phase).toBe('ready');
});

it.each(['failed', 'backpressured'] as const)('manual Retry cannot bypass %s storage, and recovery retains one checkpoint', async state => {
  const h = harness(); h.controller.update(h.input); await flush(); h.pending[0].resolve(ready('saved')); await flush();
  const next = { ...h.input, required: '13', store: { ...saved('13'), state } };
  h.controller.update(next); h.controller.retry(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(h.getRun).toHaveBeenCalledOnce();
  expect(h.last()).toMatchObject({ phase: 'stale', value: { story: { run: { id: 'saved' } } } });
  h.controller.update({ ...next, store: saved('13') }); await flush(); h.pending[1].resolve(ready('recovered')); await flush();
  await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2); expect(h.last().phase).toBe('ready');
});

it('keeps fixed schema errors explicit, stops on failure, and accepts a manually retried result', async () => {
  const h = harness(); h.controller.update(h.input); await flush();
  const message = 'History schema upgrade failed; close other Maxed instances or resolve local storage access, then Retry.';
  h.pending[0].reject(message); await flush(); h.controller.update(h.input);
  await vi.advanceTimersByTimeAsync(5000); await flush();
  expect(h.getRun).toHaveBeenCalledOnce(); expect(h.last()).toMatchObject({ phase: 'error', retryAvailable: true, message });
  h.controller.retry(); await flush(); h.pending[1].resolve(ready('upgraded')); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2); expect(h.last().phase).toBe('ready');
});

it('accepts large decimal checkpoints only from the matching stream', async () => {
  const h = harness(), required = '9007199254740993';
  h.controller.update({ ...h.input, required, store: saved('9007199254740992') });
  h.controller.update({ ...h.input, required, store: saved(required, 'wrong-stream') });
  h.controller.update({ ...h.input, required, store: saved(null) }); await flush();
  expect(h.getRun).not.toHaveBeenCalled();
  h.controller.update({ ...h.input, required, store: saved(required) }); await flush();
  h.pending[0].resolve(ready('large')); await flush();
  expect(h.getRun).toHaveBeenCalledOnce(); expect(h.last().phase).toBe('ready');
});

it('disposal cannot requeue or publish an old route after its read finishes', async () => {
  const h = harness(); h.controller.update(h.input); await flush(); h.pending[0].resolve(ready('visible')); await flush();
  expect(h.last().phase).toBe('ready');
  h.controller.retry(); await flush(); h.controller.dispose(); const count = h.states.length;
  h.pending[1].resolve(ready('retired')); await flush(); await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(h.getRun).toHaveBeenCalledTimes(2); expect(h.states).toHaveLength(count);
});
