import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createHomeHistory, type HomeHistoryState } from '../../src/lib/home/summary';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';
import { deferred, flush, homeSummary, saved } from './fixtures';

let dispose = () => {};
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
});
afterEach(() => { dispose(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('accepts each issued checkpoint while coalescing newer requirements into one follow-up', async () => {
  const pending: ReturnType<typeof deferred<HomeSummary>>[] = [];
  const states: HomeHistoryState[] = [];
  const queryHome = vi.fn(() => { const result = deferred<HomeSummary>(); pending.push(result); return result.promise.then(value => ({ status: 'ready' as const, value })); });
  const controller = createHomeHistory({ queryHome }, state => states.push(state));
  dispose = controller.dispose;
  const input = { mode: 'nexus' as const, scope: { session: { id: 'synthetic-session' } }, streamId: 'live-synthetic-home', required: '12', store: saved('12') };
  controller.update(input); await flush();
  expect(queryHome).toHaveBeenCalledOnce();
  controller.update({ ...input, required: '13' });
  controller.update({ ...input, required: '14' });
  pending[0].resolve(homeSummary({ runs: 2 })); await flush();
  expect(states.at(-1)).toMatchObject({ phase: 'stale', summary: { runs: 2 } });
  expect(queryHome).toHaveBeenCalledOnce();
  controller.update({ ...input, required: '14', store: saved('14') }); await flush();
  expect(queryHome).toHaveBeenCalledTimes(2);
  pending[1].resolve(homeSummary({ runs: 3 })); await flush();
  expect(states.at(-1)).toMatchObject({ phase: 'ready', summary: { runs: 3 } });
});
