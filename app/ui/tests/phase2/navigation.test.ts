import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import { advance, click, deferred, flush, follow, history, main, microtasks, mountPhase2, primary, ready, region, run, runPage, saved, story } from './fixtures';

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('phase2_navigation_windows_and_bounds', async () => {
  const rows = Array.from({ length: 14 }, (_, i) => run({ id: `run-${i}`, dungeon: `Invented dungeon ${i}` }));
  const page = mountPhase2({ queries: { listRuns: async () => ready(runPage(rows)) } }); await flush();
  expect(main().getAllByRole('link', { name: /Invented dungeon/ })).toHaveLength(14);
  const seventh = main().getByRole('link', { name: /Invented dungeon 6\b/ }); seventh.focus();
  await follow('#/runs/run-6');
  await fireEvent.click(main().getAllByRole('tab')[1]); await flush(); await follow('#/runs/run-6/fights/fight-1', region('Fight damage'));
  // The fixture's fight relationship must agree with the selected parent.
  // Override before the player read, without fabricating a run label on Fight.
  expect(main().getByRole('heading', { name: 'Fight Unavailable' })).toBeTruthy();
  await click('Back', history()); await click('Back', history());
  expect(document.activeElement?.getAttribute('data-row-id')).toBe('run-6');
  expect(page.browser.adapter.state).toMatchObject({ runsWindow: 0 });
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledTimes(1);
  const reads = page.queries.listRuns.mock.calls.length;
  for (let i = 0; i < 20; i++) page.publish({ ...page.snapshot, at: String(11000000 + i), sequence: String(13 + i) });
  await advance(5000); expect(page.queries.listRuns).toHaveBeenCalledTimes(reads);
  page.unmount(); await microtasks(); expect(page.browser.listeners).toBe(0); expect(vi.getTimerCount()).toBe(0);
});

it('actual Shell traverses Runs, story, fight, player, Back and Forward', async () => {
  const page = mountPhase2(); await flush();
  await follow('#/runs/run-1'); await fireEvent.click(main().getAllByRole('tab')[1]); await flush(); await follow('#/runs/run-1/fights/fight-1', region('Fight damage'));
  await follow('#/runs/run-1/fights/fight-1/players/7');
  expect(main().getByRole('heading', { level: 1, name: 'Player' })).toBeTruthy();
  for (const expected of ['#/runs/run-1/fights/fight-1', '#/runs/run-1', '#/runs']) {
    await click('Back', history()); expect(page.browser.adapter.hash).toBe(expected);
  }
  for (const expected of ['#/runs/run-1', '#/runs/run-1/fights/fight-1', '#/runs/run-1/fights/fight-1/players/7']) {
    await click('Forward', history()); expect(page.browser.adapter.hash).toBe(expected);
  }
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledTimes(1);
  expect(page.permission).not.toHaveBeenCalled();
});

it('death anchor waits for saved rows and a removed focused row falls back once', async () => {
  const value = story({ run: run({ died_at: '10000000' }), timeline: [
    ...Array.from({ length: 15 }, (_, i) => ({ id: `event-${i}`, at: String(2000000 + i), kind: 'portal_opened' as const, related_id: null, explanation: `Invented portal ${i}` })),
    { id: 'death', at: '10000000', kind: 'death', related_id: 'run-1', explanation: 'Observed local death' },
  ] });
  const pending = deferred<HistoryReply<RunStory | null>>();
  const page = mountPhase2({ hash: '#/runs/run-1/death', queries: { getRun: () => pending.promise } }); await flush();
  await advance(1000);
  expect(document.activeElement?.getAttribute('data-anchor')).not.toBe('death');
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  pending.resolve(ready(value)); await flush();
  expect(document.activeElement?.getAttribute('data-anchor')).toBe('death');
  expect(region('Timeline').getByRole('group', { name: /Duration/ })).toBeTruthy();
  // Separate restored list entry whose row disappeared during a detail visit.
  await follow('#/runs', primary());
  main().getByRole('link', { name: /Invented Sanctum/ }).focus(); await follow('#/runs/run-1');
  page.queries.listRuns.mockResolvedValue(ready(runPage([])));
  page.event({ run_ended: run() }, '13'); page.setStore(saved('13', page.snapshot.stream_id)); await advance();
  await click('Back', history());
  expect(document.activeElement).toBe(main().getByRole('heading', { level: 1, name: 'Runs' }));
  expect(screen.getAllByText(/Page Heading/)).toHaveLength(1);
});

it('navigation cancels obsolete Alerts readiness and keeps one mounted rail', async () => {
  const page = mountPhase2({ hash: '#/settings' }); await flush();
  const rail = screen.getByRole('complementary');
  await fireEvent.click(main().getByRole('link', { name: 'Alert history' }));
  await fireEvent.click(primary().getByRole('link', { name: 'Settings' })); await microtasks();
  expect(document.activeElement).toBe(main().getByRole('heading', { level: 1, name: 'Settings' }));
  expect(screen.getByRole('complementary')).toBe(rail);
  await advance(6000); expect(main().getByRole('heading', { level: 1, name: 'Settings' })).toBe(document.activeElement);
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledTimes(1);
});
