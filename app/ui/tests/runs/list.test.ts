import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { RunListSummary } from '../../src/lib/bindings/RunListSummary';
import type { RunPage } from '../../src/lib/bindings/RunPage';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { advance, click, deferred, edit, flush, follow, main, mountRuns, ready, run, runPage } from './fixtures';

const summary: RunListSummary = { runs: 27, cleared: 20, died: 2, left: 3, unknown: 1, time_in_runs_micros: 500_000_000,
  personal_bests: 2, fame_delta: 412, fame_observed_runs: 20, first_started: '1000000', total_runs_all_time: 100,
  last_death: null, characters: [{ id: 'test-mage', name: 'TestMage', class_id: 100, runs: 27 }] };
const summarized = (rows = [run()], next: RunPage['next'] = null): RunPage => ({ ...runPage(rows, next), summary });
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('distinguishes loading, empty and errors, and only offers available retries', async () => {
  const pending = deferred<HistoryReply<RunPage>>();
  const page = mountRuns({ queries: { listRuns: () => pending.promise } }); await flush();
  expect(main().getByText(/Loading.*runs/i)).toBeTruthy();
  expect(main().queryByRole('button', { name: 'Retry' })).toBeNull();
  pending.resolve(ready(runPage([]))); await flush();
  expect(main().getByText('No runs match these filters')).toBeTruthy();
  page.queries.listRuns.mockRejectedValueOnce(new Error('disk read failed'));
  await click('Clear filters');
  expect(main().getByText(/Run error/i)).toBeTruthy();
  expect(main().queryByText('No runs match these filters')).toBeNull();
  page.queries.listRuns.mockResolvedValue(ready(runPage())); await click('Retry');
  expect(main().getByRole('link', { name: /Invented Sanctum/i }).getAttribute('data-row-id')).toBe('run-1');
  expect(main().queryByRole('button', { name: 'Retry' })).toBeNull();
});
it.each(['cleared', 'died', 'left', 'unknown'] as const)('applies counted outcome %s and character on change', async outcome => {
  const page = mountRuns({ queries: { listRuns: async () => ready(summarized()) } }); await flush();
  expect(main().getByRole('button', { name: 'All 27' }).getAttribute('aria-pressed')).toBe('true');
  await click(new RegExp(`^${outcome} ${summary[outcome]}$`, 'i'));
  expect(page.queries.listRuns.mock.lastCall![0].outcome).toBe(outcome);
  expect(main().getByRole('button', { name: new RegExp(`^${outcome} `, 'i') }).getAttribute('aria-pressed')).toBe('true');
  expect(main().getByRole('option', { name: 'TestMage' })).toBeTruthy();
  await edit('Character', 'test-mage');
  expect(page.queries.listRuns.mock.lastCall![0]).toMatchObject({ character_id: 'test-mage', outcome, cursor: null, limit: 25 });
});
it('debounces search for 300 ms; Enter flushes and Escape clears', async () => {
  const page = mountRuns(); await flush();
  await edit('Dungeon', 'San'); await advance(299);
  expect(page.queries.listRuns).toHaveBeenCalledTimes(1);
  await advance(1); expect(page.queries.listRuns.mock.lastCall![0].dungeon).toBe('San');
  await edit('Dungeon', '  Sanctum  '); await fireEvent.keyDown(main().getByLabelText('Dungeon'), { key: 'Enter' }); await flush();
  expect(page.queries.listRuns.mock.lastCall![0].dungeon).toBe('Sanctum');
  await advance(300); expect(page.queries.listRuns).toHaveBeenCalledTimes(3);
  await fireEvent.keyDown(main().getByLabelText('Dungeon'), { key: 'Escape' }); await flush();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(3);
  expect((main().getByLabelText('Dungeon') as HTMLInputElement).value).toBe('');
  expect(main().getByRole('link', { name: /Invented Sanctum/ })).toBeTruthy();
});
it.each(['newest', 'oldest', 'duration_ascending', 'duration_descending'] as const)('renders 25 rows and pages with cached counts (%s)', async sort => {
  const cursor = { sort_value: '11000000', id: 'run-025' };
  const rows = Array.from({ length: 25 }, (_, i) => run({ id: `run-${i}` }));
  const page = mountRuns({ queries: { listRuns: async filter => ready(filter.cursor ? runPage([run({ dungeon: 'Page two' })]) : summarized(rows, cursor)) } });
  await flush(); await edit('Sort', sort);
  expect(main().getAllByRole('link', { name: /Invented Sanctum/ })).toHaveLength(25);
  expect(main().queryByRole('button', { name: 'Next Rows' })).toBeNull();
  expect(main().getByText('1–25 of 27')).toBeTruthy();
  await click('Next');
  expect(page.queries.listRuns.mock.lastCall![0]).toMatchObject({ sort, cursor, limit: 25 });
  expect(main().getByText('26–26 of 27')).toBeTruthy();
  expect(main().getByRole('button', { name: 'All 27' })).toBeTruthy();
  const reads = page.queries.listRuns.mock.calls.length;
  await click('Previous');
  expect(page.queries.listRuns).toHaveBeenCalledTimes(reads);
  expect(main().getByText('1–25 of 27')).toBeTruthy();
  expect(main().getAllByRole('link', { name: /Invented Sanctum/ })).toHaveLength(25);
});
it('shows partial capture and coexisting clear/death without debug mode', async () => {
  mountRuns({ queries: { listRuns: async () => ready(runPage([run({ complete_capture: false, died_at: '10000000' })])) } }); await flush();
  const row = main().getByRole('link', { name: /Invented Sanctum/ });
  expect(row.textContent).toContain('partial'); expect(row.textContent).toContain('Clear and death');
});
it('clears all filters from an empty result', async () => {
  const page = mountRuns({ queries: { listRuns: async () => ready(summarized([])) } }); await flush();
  await click('Today'); await click('Died 2'); await edit('Character', 'test-mage'); await edit('Dungeon', 'Missing'); await advance(300);
  const reads = page.queries.listRuns.mock.calls.length;
  await click('Clear filters');
  expect(page.queries.listRuns).toHaveBeenCalledTimes(reads);
  expect((main().getByLabelText('Dungeon') as HTMLInputElement).value).toBe('');
  expect((main().getByLabelText('Character') as HTMLSelectElement).value).toBe('');
  expect(main().getByRole('button', { name: 'All 27' }).getAttribute('aria-pressed')).toBe('true');
  expect(main().getByRole('button', { name: 'All time' }).getAttribute('aria-pressed')).toBe('true');
});
it('caps previous cursors and explains the retained history limit', async () => {
  const page = mountRuns({ queries: { listRuns: async filter => {
    const index = filter.cursor ? Number(filter.cursor.id) : 0;
    return ready(runPage([run({ id: `page-${index}`, dungeon: `Window ${index}` })], { sort_value: '1', id: String(index + 1) }));
  } } }); await flush();
  for (let i = 0; i < 102; i++) await click('Next');
  expect(main().getAllByRole('link', { name: /Window / })).toHaveLength(1);
  expect(main().getByText(/100 previous/)).toBeTruthy(); await advance(250);
  expect((page.browser.adapter.state as { runsPosition: { previous: unknown[] } }).runsPosition.previous).toHaveLength(100);
});
it('replaces a mutable outcome revision and retains the selected filter', async () => {
  const page = mountRuns(); await flush(); await click(/^Cleared /);
  page.queries.listRuns.mockResolvedValue(ready(runPage([])));
  page.event({ run_updated: run({ outcome: 'left' }) }, '13'); await flush();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(2);
  page.setStore({ state: 'ready', stream_id: page.snapshot.stream_id, durable: '13', queued: 0, message: '' }); await advance();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(3);
  expect(main().queryByRole('link', { name: /Invented Sanctum/ })).toBeNull();
  expect(main().getByText('No runs match these filters')).toBeTruthy();
  expect(main().getByRole('button', { name: /^Cleared / }).getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByRole('heading', { name: 'Runs', level: 1 })).toBeTruthy();
});

it('renders enriched rows, bounds loot previews and leaves unavailable values unknown', async () => {
  const details: NonNullable<RunPage['details']> = [{ run_id: 'run-1', character_name: 'TestMage', class_id: 100,
    local_share: .25, share_partial: true, fame_delta: 412, clear_micros: 552_000_000, best_clear_micros: 552_000_000, previous_best_clear_micros: 580_000_000, is_personal_best: true,
    observed_drop_count: 6, loot_preview: Array.from({ length: 3 }, (_, i) => ({ drop_id: `bag-${i}`, bag_type: 400 + i, bag_rank: i })) }];
  mountRuns({ queries: { listRuns: async () => ready({ ...summarized([run(), run({ id: 'unknown', dungeon: 'Unknown details' })]), details }) } }); await flush();
  const row = main().getByRole('link', { name: /Invented Sanctum/ });
  for (const value of ['TestMage', '25%', 'partial', '+412', '9:12', 'PB', '−0:28', '+3']) expect(row.textContent).toContain(value);
  expect(row.querySelectorAll('[role="img"][aria-label="Observed bag"]')).toHaveLength(3);
  expect(row.querySelectorAll('a, button')).toHaveLength(0);
  expect(main().getByRole('link', { name: /Unknown details/ }).textContent).not.toMatch(/0%|0:00|PB/);
  expect(main().getByText(/100 saved runs/)).toBeTruthy();
});


it('leaves the first personal best comparison unknown', async () => {
  const details: NonNullable<RunPage['details']> = [{ run_id: 'run-1', character_name: null, class_id: null,
    local_share: null, share_partial: false, fame_delta: null, clear_micros: 552_000_000,
    best_clear_micros: 552_000_000, previous_best_clear_micros: null, is_personal_best: true,
    observed_drop_count: 0, loot_preview: [] }];
  mountRuns({ queries: { listRuns: async () => ready({ ...summarized([run()]), details }) } }); await flush();
  const row = main().getByRole('link', { name: /Invented Sanctum/ });
  expect(row.textContent).toContain('PB');
  expect(row.textContent).not.toContain('0:00');
  expect(row.querySelector('.comparison')?.textContent).toBe('—');
});

it('labels saved open runs as in progress', async () => {
  mountRuns({ queries: { listRuns: async () => ready(runPage([run({ outcome: null, ended: null, completed_at: null })])) } });
  await flush();
  expect(main().getByRole('link', { name: /Invented Sanctum/ }).textContent).toContain('In progress');
});

it.each(['duration_ascending', 'duration_descending'] as const)('preserves interleaved day order without headers for %s', async sort => {
  const at = (day: number) => String(new Date(2030, 0, day, 10).getTime() * 1000);
  const rows = [run({ id: 'a', started: at(1), ended: String(Number(at(1)) + 1_000_000) }),
    run({ id: 'b', started: at(2), ended: String(Number(at(2)) + 2_000_000) }),
    run({ id: 'c', started: at(1), ended: String(Number(at(1)) + 3_000_000) })];
  const ordered = sort === 'duration_ascending' ? rows : [...rows].reverse();
  mountRuns({ queries: { listRuns: async () => ready(runPage(ordered)) } }); await flush(); await edit('Sort', sort);
  expect(main().getAllByRole('link', { name: /Invented Sanctum/ }).map(row => row.getAttribute('data-row-id'))).toEqual(ordered.map(row => row.id));
  expect(screen.getByRole('main').querySelectorAll('.day')).toHaveLength(0);
});

it('restores page-two summaries, KPIs and Unknown selection on Back and Forward', async () => {
  const cursor = { sort_value: '11000000', id: 'page-one-last' };
  const cached = { ...summary, unknown: 27 };
  const page = mountRuns({ hash: '#/settings', queries: { listRuns: async filter => ready(filter.cursor
    ? runPage([run()]) : { ...summarized([run()], cursor), summary: cached }) } }); await flush();
  await follow('#/runs', within(screen.getByRole('navigation', { name: 'Navigation' })));
  await click('Unknown 27'); await click('Next');
  const assertSummary = () => {
    expect(main().getByText(/100 saved runs/)).toBeTruthy();
    expect(main().getByText('20 cleared · 74%')).toBeTruthy();
    expect(main().getByText('0h 8m')).toBeTruthy();
    expect(main().getByText('+412')).toBeTruthy();
    expect(main().getByText('observed in 20 runs')).toBeTruthy();
    expect(main().getByRole('button', { name: 'Unknown 27' }).getAttribute('aria-pressed')).toBe('true');
    expect(main().getByRole('option', { name: 'TestMage' })).toBeTruthy();
    expect(main().getByText('26–26 of 27')).toBeTruthy();
  };
  assertSummary(); await follow('#/runs/run-1');
  const history = () => within(screen.getByRole('group', { name: 'History' }));
  await click('Back', history()); assertSummary();
  await click('Back', history()); expect(page.browser.adapter.hash).toBe('#/settings');
  await click('Forward', history()); assertSummary();
  expect(page.queries.listRuns.mock.calls.filter(([filter]) => filter.cursor === null)).toHaveLength(2);
});

it('refreshes day labels and preset selection when a live page arrives after local midnight', async () => {
  vi.setSystemTime(new Date(2030, 0, 1, 23, 59, 59));
  const row = run({ started: String(new Date(2030, 0, 1, 12).getTime() * 1000) });
  const page = mountRuns({ queries: { listRuns: async () => ready(summarized([row])) } }); await flush();
  await click('Today');
  expect(main().getByRole('button', { name: 'Today' }).getAttribute('aria-pressed')).toBe('true');
  expect(screen.getByRole('main').querySelector('.day')?.textContent).toContain('Today');
  await advance(2000);
  page.event({ run_updated: row }, '13');
  page.setStore({ state: 'ready', stream_id: page.snapshot.stream_id, durable: '13', queued: 0, message: '' }); await advance();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(3);
  expect(screen.getByRole('main').querySelector('.day')?.textContent).toContain('Yesterday');
  expect(main().getByRole('button', { name: 'Today' }).getAttribute('aria-pressed')).toBe('false');
});
