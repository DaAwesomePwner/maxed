import { afterEach, describe, expect, it, vi } from 'vitest';
import { createHomeHistory, type HomeHistoryInput, type HomeHistoryState } from '../../src/lib/home/summary';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';
import { deferred, flush, homeSummary, saved } from './fixtures';

const cleanups: (() => void)[] = [];
afterEach(() => {
  try {
    for (const dispose of cleanups.splice(0)) dispose();
  } finally {
    vi.useRealTimers();
    vi.restoreAllMocks();
  }
});
function harness() {
  const pending: ReturnType<typeof deferred<HomeSummary>>[] = [];
  const states: HomeHistoryState[] = [];
  const queryHome = vi.fn<() => Promise<HistoryReply<HomeSummary>>>(() => { const job = deferred<HomeSummary>(); pending.push(job); return job.promise.then(value => ({ status: 'ready' as const, value })); });
  const history = createHomeHistory({ queryHome }, state => states.push(structuredClone(state)));
  cleanups.push(history.dispose);
  const input: HomeHistoryInput = { mode: 'nexus', scope: { session: { id: 'demo-session' } }, streamId: 'live-synthetic-home', required: null, store: saved() };
  return { history, queryHome, pending, states, input, last: () => states.at(-1)! };
}

describe('bounded durable Home reads', () => {
  it('run_end_waits_for_matching_durable_watermark', async () => {
    const h = harness();
    h.history.update(h.input); await flush();
    h.pending[0].resolve(homeSummary()); await flush();
    const ended = { ...h.input, required: '9007199254740993' };
    h.history.update({ ...ended, mode: 'dungeon', store: saved('9007199254740993') });
    h.history.update({ ...ended, store: saved('9007199254740992') });
    h.history.update({ ...ended, store: saved('9007199254740999', 'retired-stream') });
    await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(1);
    expect(h.last().phase).toBe('stale');
    h.history.update({ ...ended, store: saved('9007199254740993') }); await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(2);
    const corrected = homeSummary({ runs: 2 });
    h.pending[1].resolve(corrected); await flush();
    expect(h.last()).toMatchObject({ phase: 'ready', summary: corrected });
    h.history.update({ ...ended, store: saved('9007199254740993') }); await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(2);
  });

  it.each(['closed', 'ready'] as const)('null_durable_waits_without_query_or_throw (%s)', async state => {
    const h = harness();
    const input = { ...h.input, required: '13', store: { ...saved(null), state } };
    expect(() => h.history.update(input)).not.toThrow(); await flush();
    expect(h.queryHome).not.toHaveBeenCalled();
    h.history.update({ ...input, store: saved('13') }); await flush();
    expect(h.queryHome).toHaveBeenCalledOnce();
  });

  it('coalesces rapid Session Today Session and discards the first session response', async () => {
    const h = harness(); h.history.update(h.input); await flush();
    h.history.update({ ...h.input, scope: { today: { range: { start: '0', end: '86400000000' } } } });
    h.history.update(h.input); await flush();
    expect(h.queryHome).toHaveBeenCalledOnce();
    h.pending[0].resolve(homeSummary({ runs: 99 })); await flush();
    expect(h.states.some(state => state.summary?.runs === 99)).toBe(false);
    expect(h.queryHome).toHaveBeenCalledTimes(2);
    h.pending[1].resolve(homeSummary({ runs: 2 })); await flush();
    expect(h.last().summary?.runs).toBe(2);
  });

  it('retired demo and old-day responses never appear in a new namespace or scope', async () => {
    const h = harness();
    h.history.update({ ...h.input, streamId: 'demo-old' }); await flush();
    const today = { ...h.input, scope: { today: { range: { start: '86400000000', end: '172800000000' } } } };
    h.history.update(today);
    expect(h.last().summary).toBeNull();
    h.pending[0].resolve(homeSummary({ runs: 99 })); await flush();
    expect(h.states.some(state => state.summary?.runs === 99)).toBe(false);
    h.history.update({ ...today, scope: { today: { range: { start: '172800000000', end: '259200000000' } } } });
    h.pending[1].resolve(homeSummary({ runs: 88 })); await flush();
    expect(h.states.some(state => state.summary?.runs === 88)).toBe(false);
    expect(h.queryHome).toHaveBeenCalledTimes(3);
  });

  it('a newer invalidation during a read waits and schedules exactly one follow-up', async () => {
    const h = harness(); h.history.update(h.input); await flush();
    for (const required of ['13', '14', '15']) h.history.update({ ...h.input, required });
    h.pending[0].resolve(homeSummary({ runs: 99 })); await flush();
    expect(h.last().phase).not.toBe('ready');
    expect(h.queryHome).toHaveBeenCalledOnce();
    h.history.update({ ...h.input, required: '15', store: saved('15') }); await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(2);
  });

  it('failed storage keeps same-scope data stale and never manufactures an empty success', async () => {
    const h = harness(); h.history.update(h.input); await flush();
    h.pending[0].resolve(homeSummary()); await flush();
    h.history.update({ ...h.input, required: '13', store: { ...saved('13'), state: 'failed', queued: 1 } });
    expect(h.last()).toMatchObject({ phase: 'stale', summary: { runs: 1 } });
    expect(h.last().message).not.toBe('');
    expect(h.queryHome).toHaveBeenCalledOnce();
  });

  it('sanitizes rejected reads, supports retry and discards completion after disposal', async () => {
    const h = harness(); h.history.update(h.input); await flush();
    h.pending[0].reject(new Error('PRIVATE C:/owner.sqlite SELECT secret')); await flush();
    expect(h.last()).toMatchObject({ phase: 'error', summary: null });
    expect(h.last().message).not.toMatch(/PRIVATE|owner|SELECT/);
    h.history.retry(); await flush(); expect(h.queryHome).toHaveBeenCalledTimes(2);
    h.history.dispose(); const count = h.states.length;
    h.pending[1].resolve(homeSummary()); await flush(); expect(h.states).toHaveLength(count);
  });

  it('four Hz Dungeon updates and unavailable session issue zero history reads', async () => {
    vi.useFakeTimers(); const h = harness();
    h.history.update({ ...h.input, scope: null });
    expect(h.last().phase).toBe('unavailable');
    for (let i = 0; i < 80; i++) {
      h.history.update({ ...h.input, mode: 'dungeon', required: String(13 + i), store: saved(String(13 + i)) });
      await vi.advanceTimersByTimeAsync(250);
    }
    expect(h.queryHome).not.toHaveBeenCalled();
  });
  it('keeps supersession transient without error, retry or a read loop', async () => {
    const h = harness(); h.history.update(h.input); await flush();
    h.pending[0].resolve(homeSummary()); await flush();
    expect(h.last().phase).toBe('ready'); // Positive control: the same query can publish history.
    h.queryHome.mockResolvedValueOnce({ status: 'superseded' });
    h.history.retry(); await flush();
    expect(h.last()).toMatchObject({ phase: 'superseded', retryAvailable: false, summary: { runs: 1 } });
    expect(h.last().message).not.toMatch(/error|failed|retry/i);
    h.history.update(h.input); h.history.retry(); await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(2);
    const next = { ...h.input, scope: { today: { range: { start: '0', end: '86400000000' } } } };
    h.history.update(next); await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(3);
    h.pending[1].resolve(homeSummary({ runs: 2 })); await flush();
    expect(h.last()).toMatchObject({ phase: 'ready', retryAvailable: true, summary: { runs: 2 } });
  });

  it('retries Busy once and retains the spent budget across refresh ticks until manual retry', async () => {
    const h = harness();
    h.queryHome.mockResolvedValueOnce({ status: 'busy' }).mockResolvedValueOnce({ status: 'busy' });
    h.history.update(h.input); await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(2);
    expect(h.states.some(state => state.phase === 'busy' && !state.retryAvailable)).toBe(true);
    expect(h.last()).toMatchObject({ phase: 'busy', retryAvailable: true, summary: null });
    h.history.update(h.input); await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(2);
    h.queryHome.mockResolvedValueOnce({ status: 'busy' });
    h.history.update({ ...h.input, required: '13', store: saved('13') }); await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(3);
    expect(h.last()).toMatchObject({ phase: 'busy', retryAvailable: true });
    h.queryHome.mockResolvedValueOnce({ status: 'busy' }).mockResolvedValueOnce({ status: 'ready', value: homeSummary() });
    h.history.retry(); await flush();
    expect(h.queryHome).toHaveBeenCalledTimes(5);
    expect(h.last()).toMatchObject({ phase: 'ready', retryAvailable: true, summary: { runs: 1 } });
  });

  it.each([
    'History schema upgrade failed; close other Maxed instances or resolve local storage access, then Retry.',
    'History schema is newer than this Maxed build; update Maxed, then Retry.',
  ])('preserves only the fixed schema error: %s', async message => {
    const h = harness(); h.history.update(h.input); await flush();
    expect(h.queryHome).toHaveBeenCalledOnce();
    h.pending[0].reject(message); await flush();
    expect(h.last()).toMatchObject({ phase: 'error', message, retryAvailable: true });
    h.history.update(h.input); await flush();
    expect(h.last().message).toBe(message);
    expect(h.queryHome).toHaveBeenCalledOnce();
    h.history.retry(); await flush(); h.pending[1].resolve(homeSummary()); await flush();
    expect(h.last().phase).toBe('ready');
  });

});
