import { cleanup, screen } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { flush, run, story, mountRuns, ready, saved, deferred } from './fixtures';

afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

it('RunStory caches PBs across run refreshes and retains them when a baseline refresh is superseded', async () => {
  vi.useFakeTimers();
  const value = story({ run: run({ started: '1000000', completed_at: '581000000' }), comparison: null });
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } });
  const client = page.client, getRun = page.queries.getRun;
  const bests = vi.spyOn(client, 'queryDungeonBests').mockResolvedValue({
    status: 'ready', value: [{ dungeon: value.run.dungeon, best_clear_micros: 552000000 }],
  });
  await flush();
  expect(screen.getByText(/PB 9:12/)).toBeTruthy();
  bests.mockResolvedValue({ status: 'superseded' });
  page.event({ run_updated: value.run }, '13'); page.setStore(saved('13'));
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(getRun).toHaveBeenCalledTimes(2); expect(bests).toHaveBeenCalledOnce();
  expect(screen.getByText(/PB 9:12/)).toBeTruthy();
  page.transport.publishAccountHistoryChanged({ revision: 1, reason: 'legacy_recovered' });
  await flush();
  expect(bests).toHaveBeenCalledTimes(2);
  expect(screen.getByText(/PB 9:12/)).toBeTruthy();
});

it('coalesces account-history changes during an in-flight PB read into one fresh read', async () => {
  vi.useFakeTimers();
  const value = story({ run: run({ started: '1000000', completed_at: '581000000' }), comparison: null });
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } });
  const pending = deferred<Awaited<ReturnType<typeof page.client.queryDungeonBests>>>();
  const bests = vi.spyOn(page.client, 'queryDungeonBests').mockReturnValueOnce(pending.promise)
    .mockResolvedValue(ready([{ dungeon: value.run.dungeon, best_clear_micros: 540000000 }]));
  await flush();
  expect(bests).toHaveBeenCalledOnce();
  page.transport.publishAccountHistoryChanged({ revision: 1, reason: 'account_switched' });
  page.transport.publishAccountHistoryChanged({ revision: 2, reason: 'legacy_recovered' });
  await flush();
  expect(page.queries.getRun).toHaveBeenCalledOnce();
  pending.resolve(ready([{ dungeon: value.run.dungeon, best_clear_micros: 552000000 }]));
  await flush();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  expect(bests).toHaveBeenCalledTimes(2);
  expect(screen.queryByText(/PB 9:12/)).toBeNull();
  expect(screen.getByText(/PB 9:00/)).toBeTruthy();
  await vi.advanceTimersByTimeAsync(5000);
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
});
