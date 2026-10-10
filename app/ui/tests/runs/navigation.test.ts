// Boss-tier fixtures preserve the existing window/focus checks after trash grouping.
import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { setDebugMode } from '../../src/lib/debug/debug';
import { DENSE_STORY_ROWS } from '../../src/lib/phase2/contracts';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { homeSummary } from '../home/fixtures';
import { createHistoryCache } from '../../src/lib/runs/history-cache';
import { invalidatesSavedHistory } from '../../src/lib/shell/history-invalidation';
import { mountPhase2 } from '../phase2/fixtures';
import { advance, click, deferred, edit, fight, flush, follow, injectChicagoDate, largeStory, main, mountRuns, nativeQueue, ready, region, run, runPage, saved, story } from './fixtures';

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
});
afterEach(() => {
  setDebugMode(false);
  cleanup();
  vi.clearAllTimers();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});
const history = () => within(screen.getByRole('group', { name: 'History' }));
const primary = () => within(screen.getByRole('navigation', { name: 'Navigation' }));

it('runs_fight_player_back_forward_restores_page_focus', async () => {
  injectChicagoDate();
  vi.setSystemTime(new Date('2026-11-01T12:00:00Z'));
  const cursor = { sort_value: '1000000', id: 'page-one-last' };
  const page = mountRuns({ queries: { listRuns: async filter => ready(filter.cursor
    ? runPage([run()]) : runPage([run({ id: 'page-one', dungeon: 'Page one' })], cursor)),
    getRun: async () => ready(largeStory(101, true)),
  } }); await flush(); await edit('Dungeon', 'Invented Sanctum'); await advance(300); await click(/^Cleared /);
  await click('Today'); await edit('Sort', 'oldest'); await click('Next');
  const pane = screen.getByRole('main');
  Object.defineProperties(pane, { scrollHeight: { configurable: true, value: 1600 }, clientHeight: { configurable: true, value: 500 } });
  const row = main().getByRole('link', { name: /Invented Sanctum/ }); row.focus(); pane.scrollTop = 330; await fireEvent.scroll(pane);
  await follow('#/runs/run-1');
  const selected = main().getByRole('tab', { name: 'Invented target 51' });
  await fireEvent.click(selected); await flush(); selected.focus();
  pane.scrollTop = 220; await fireEvent.scroll(pane);
  await follow('#/runs/run-1/fights/fight-51', region('Fight damage'));
  await follow('#/runs/run-1/fights/fight-51/players/7');
  expect(main().getByRole('heading', { name: 'Player', level: 1 })).toBeTruthy();
  await click('Back', history()); expect(page.browser.adapter.hash).toBe('#/runs/run-1/fights/fight-51');
  await click('Back', history());
  expect(page.browser.adapter.state).toMatchObject({ selectedFightId: 'fight-51', storyPosition: { selectedTab: 'fight-51' } });
  expect(main().getByRole('tab', { name: 'Invented target 51' }).getAttribute('aria-selected')).toBe('true');
  expect(pane.scrollTop).toBe(220);
  await click('Back', history());
  expect(page.queries.listRuns.mock.lastCall![0]).toEqual({ dungeon: 'Invented Sanctum', outcome: 'cleared', character_id: null, range: { start: '1793509200000000', end: '1793599200000000' }, sort: 'oldest', cursor, limit: 25 });
  expect(main().getByRole('button', { name: /^Cleared / }).getAttribute('aria-pressed')).toBe('true');
  expect(main().getByRole('button', { name: 'Today' }).getAttribute('aria-pressed')).toBe('true');
  expect(page.browser.adapter.state).toMatchObject({ focusedRowId: 'run-1', runsPosition: { previous: [null] } });
  expect(document.activeElement?.getAttribute('data-row-id')).toBe('run-1'); expect(pane.scrollTop).toBe(330);
  await click('Forward', history()); await click('Forward', history()); await click('Forward', history());
  expect(page.browser.adapter.hash).toBe('#/runs/run-1/fights/fight-51/players/7');
  expect(main().getByRole('heading', { name: 'Player', level: 1 })).toBeTruthy();
});

it.each(['initial', 'explicit'] as const)('death_anchor_waits_for_story_rows (%s)', async entry => {
  setDebugMode(true);
  const pending = deferred<HistoryReply<RunStory | null>>();
  const value = largeStory(120); value.run.died_at = '10000000';
  value.timeline.push({ id: 'death', at: '10000000', kind: 'death', related_id: 'run-1', explanation: 'Observed death' });
  const summary = homeSummary({ recent_runs: [run({ died_at: '10000000' })] });
  const page = mountRuns({ hash: entry === 'initial' ? '#/runs/run-1/death' : '#/',
    home: async () => ready(summary), queries: { getRun: () => pending.promise } }); await flush();
  if (entry === 'explicit') await follow('#/runs/run-1/death');
  await advance(1000);
  expect(document.activeElement?.getAttribute('data-anchor')).not.toBe('death');
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  pending.resolve(ready(value)); await flush();
  const anchor = screen.getByRole('main').querySelector('[data-anchor="death"]');
  expect(anchor).toBeTruthy(); expect(document.activeElement).toBe(anchor);
  expect(anchor!.getAttribute('tabindex')).toBe('-1');
  expect(anchor!.textContent).toMatch(/Death/);
  expect(region('Timeline').queryAllByRole('link').length).toBeLessThanOrEqual(DENSE_STORY_ROWS);
});

it('clicking_fight_during_story_refresh_shows_fight_without_error', async () => {
  const queue = nativeQueue();
  const page = mountRuns({ hash: '#/runs/run-1', queries: {
    getRun: id => queue.submit(`run:${id}`), getFight: id => queue.submit(`fight:${id}`),
  } }); await flush(); queue.finish(story()); await flush();
  page.event({ run_updated: run() }, '13'); page.setStore(saved('13')); await advance();
  expect(queue.activeKey).toBe('run:run-1');
  await fireEvent.click(main().getAllByRole('tab')[1]); await flush(); await follow('#/runs/run-1/fights/fight-1', region('Fight damage'));
  expect(queue.pendingKey).toBe('fight:fight-1');
  queue.finish(story({ run: run({ dungeon: 'Retired refresh' }) })); await flush();
  expect(main().queryByText('Retired refresh')).toBeNull();
  expect(main().queryByRole('button', { name: /Retry/i })).toBeNull();
  queue.finish(fight()); await flush();
  expect(main().getByRole('heading', { name: 'Fight', level: 1 })).toBeTruthy();
  expect(main().getByRole('link', { name: /Invented Archer/ })).toBeTruthy();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2); expect(page.queries.getFight).toHaveBeenCalledTimes(1);
});

it('leaving_home_mid_read_does_not_block_next_route', async () => {
  const queue = nativeQueue();
  const page = mountRuns({ hash: '#/', home: () => queue.submit<HomeSummary>('home'), queries: {
    listRuns: () => queue.submit('runs'), getRun: id => queue.submit(`run:${id}`), getFight: id => queue.submit(`fight:${id}`),
  } }); await flush();
  expect(queue.activeKey).toBe('home'); await follow('#/runs', primary());
  expect(queue.pendingKey).toBe('runs');
  queue.finish(homeSummary()); await flush(); queue.finish(runPage()); await flush();
  expect(main().getByRole('link', { name: /Invented Sanctum/ })).toBeTruthy();
  await follow('#/runs/run-1'); queue.finish(story()); await flush();
  await fireEvent.click(main().getAllByRole('tab')[1]); await flush(); await follow('#/runs/run-1/fights/fight-1', region('Fight damage')); queue.finish(fight()); await flush();
  expect(main().getByRole('heading', { name: 'Fight', level: 1 })).toBeTruthy();
  expect(main().getByRole('link', { name: /Invented Archer/ })).toBeTruthy();
  expect(main().queryByRole('button', { name: /Retry/i })).toBeNull();
  expect(page.client.queryHome).toHaveBeenCalledTimes(1); expect(page.queries.getFight).toHaveBeenCalledTimes(1);
});

it('home_today_death_returns_saved_scope', async () => {
  setDebugMode(true);
  const summary = homeSummary({ recent_runs: [run({ died_at: '10000000' })] });
  const page = mountRuns({ hash: '#/', home: async () => ready(summary), queries: {
    getRun: async () => ready(story({ run: run({ died_at: '10000000' }) })),
  } }); await flush(); await click('Today'); await follow('#/runs/run-1/death');
  expect(document.activeElement?.getAttribute('data-anchor')).toBe('death');
  await click('Back', history());
  expect(page.browser.adapter.state).toMatchObject({ homeScope: 'today' });
  expect(vi.mocked(page.client.queryHome).mock.lastCall![0]).toHaveProperty('today');
  expect(main().getByRole('link', { name: /Invented Sanctum/ })).toBeTruthy();
  await click('Forward', history()); expect(document.activeElement?.getAttribute('data-anchor')).toBe('death');
});

it.each(['removed', 'timeout'] as const)('restoration_missing_row_and_five_second_fallback (%s)', async mode => {
  const historyCache = createHistoryCache({ invalidates: invalidatesSavedHistory, route: () => ({ kind: 'runs' }) });
  const page = mountPhase2({ hash: '#/settings', historyCache }); await flush(); await follow('#/runs', primary());
  const pane = screen.getByRole('main');
  const row = main().getByRole('link', { name: /Invented Sanctum/ }); row.focus(); await follow('#/runs/run-1');
  const pending = deferred<HistoryReply<ReturnType<typeof runPage>>>();
  page.queries.listRuns.mockImplementation(() => pending.promise);
  page.event({ run_ended: run() }, '13'); page.setStore(saved('13', page.snapshot.stream_id)); await advance();
  // The timeout fallback applies when no cached rows are available.
  if (mode === 'timeout') historyCache.clear();
  await click('Back', history());
  if (mode === 'removed') expect(document.activeElement?.getAttribute('data-row-id')).toBe('run-1');
  await advance(4999);
  expect(document.activeElement).not.toBe(main().getByRole('heading', { name: 'Runs', level: 1 }));
  if (mode === 'removed') pending.resolve(ready(runPage([]))); else await advance(1);
  await flush();
  expect(document.activeElement).toBe(main().getByRole('heading', { name: 'Runs', level: 1 }));
  expect(screen.getByText(mode === 'removed' ? 'Row Removed' : 'Page Heading')).toBeTruthy();
  expect(pane.scrollTop).toBe(0);
  if (mode === 'timeout') {
    const focused = document.activeElement; pending.resolve(ready(runPage())); await flush();
    expect(document.activeElement).toBe(focused);
    expect(main().getByRole('link', { name: /Invented Sanctum/ })).toBeTruthy();
  } else expect(main().getByText(/No.*runs/i)).toBeTruthy();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(2);
});

it('rapid_traversal_ignores_old_readiness_and_retired_namespace_history', async () => {
  const queue = nativeQueue();
  const page = mountRuns({ queries: { getRun: id => queue.submit(`run:${id}`) } }); await flush();
  await follow('#/runs/run-1'); await click('Back', history()); await click('Forward', history());
  queue.finish(story({ run: run({ dungeon: 'Old traversal' }) })); await flush();
  expect(main().queryByText('Old traversal')).toBeNull();
  queue.finish(story({ run: run({ dungeon: 'Current traversal' }) })); await flush();
  expect(main().getByText('Current traversal')).toBeTruthy();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  const next = { ...page.snapshot, stream_id: 'replacement-live', sequence: '1' };
  page.setStore(saved('1', next.stream_id)); page.publish(next); await flush();
  page.browser.adapter.go(-1); await flush();
  expect(page.browser.adapter.hash).toBe('#/');
  expect(main().getByRole('heading', { name: 'Home', level: 1 })).toBeTruthy();
  expect(main().queryByText('Current traversal')).toBeNull();
});
