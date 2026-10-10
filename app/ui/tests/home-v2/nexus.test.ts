import { routeHref } from '../../src/lib/navigation/routes';
import { setDebugMode } from '../../src/lib/debug/debug';
import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import NexusHome from '../../src/lib/home/NexusHome.svelte';
import RecentRuns from '../../src/lib/home/RecentRuns.svelte';
import type { HomeHistoryInput, HomeHistoryState } from '../../src/lib/home/summary';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';
import { createHomeHistory } from '../../src/lib/home/summary';
import { localDayRange } from '../../src/lib/home/scope';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { saved } from '../home/fixtures';
import { catalog, deferred, flush, homeSnapshot, homeSummary, section } from './fixtures';

afterEach(() => { setDebugMode(false); vi.unstubAllGlobals(); vi.restoreAllMocks(); });
const ready = (summary: HomeSummary): HomeHistoryState => ({ phase: 'ready', summary, message: '', retryAvailable: true });
const nexus = () => homeSnapshot({ area_kind: 'nexus', run: null });
const renderNexus = (summary: HomeSummary) => render(NexusHome, { snapshot: nexus(), history: ready(summary), choice: 'session', retry: vi.fn() });

it('nexus_scope_watermark_dst_and_preview_limits', async () => {
  const snapshot = nexus(), pending = deferred<HomeSummary>();
  const queryHome = vi.fn().mockImplementationOnce(() => pending.promise.then(value => ({ status: 'ready' as const, value })))
    .mockResolvedValue({ status: 'ready', value: homeSummary({ runs: 321 }) });
  const states: HomeHistoryState[] = [];
  const history = createHomeHistory({ queryHome }, state => states.push(state));
  const input: HomeHistoryInput = { mode: 'nexus', scope: { session: { id: snapshot.session!.id } },
    streamId: snapshot.stream_id, required: '20', store: saved('19', snapshot.stream_id) };
  history.update(input); await flush();
  expect(queryHome).not.toHaveBeenCalled();
  expect(states.at(-1)?.phase).toBe('stale');
  history.update({ ...input, store: saved('20', snapshot.stream_id) }); await flush();
  expect(queryHome).toHaveBeenCalledWith(input.scope);
  history.update({ ...input, streamId: 'demo-new-source', scope: { session: { id: 'new-session' } }, store: saved('20', 'demo-new-source') });
  pending.resolve(homeSummary({ runs: 999999 })); await flush();
  expect(states.some(state => state.summary?.runs === 999999)).toBe(false);
  expect(states.at(-1)?.summary?.runs).toBe(321);
  expect(queryHome).toHaveBeenLastCalledWith({ session: { id: 'new-session' } });
  history.dispose();

  // Independent Chicago midnight pairs: no worker/process timezone mutation.
  const NativeDate = Date;
  const cases = [
    { day: 8, month: 2, start: '2026-03-08T06:00:00Z', end: '2026-03-09T05:00:00Z', hours: 23 },
    { day: 1, month: 10, start: '2026-11-01T05:00:00Z', end: '2026-11-02T06:00:00Z', hours: 25 },
  ];
  for (const test of cases) {
    class CalendarDate extends NativeDate {
      constructor(year: number, month: number, day: number) {
        if (year !== 2026 || month !== test.month || ![test.day, test.day + 1].includes(day)) throw new Error('Unexpected calendar boundary');
        super(day === test.day ? test.start : test.end);
      }
    }
    const now = new NativeDate(test.start);
    now.getFullYear = () => 2026; now.getMonth = () => test.month; now.getDate = () => test.day;
    vi.stubGlobal('Date', CalendarDate);
    const range = localDayRange(now);
    expect(range).toEqual({ start: String(NativeDate.parse(test.start) * 1000), end: String(NativeDate.parse(test.end) * 1000) });
    expect(BigInt(range.end) - BigInt(range.start)).toBe(BigInt(test.hours) * 3600000000n);
    const dayQuery = vi.fn().mockResolvedValue({ status: 'ready', value: homeSummary() });
    const dayHistory = createHomeHistory({ queryHome: dayQuery }, () => {});
    dayHistory.update({ ...input, scope: { today: { range } }, store: saved('20', snapshot.stream_id) }); await flush();
    expect(dayQuery).toHaveBeenCalledWith({ today: { range } });
    dayHistory.dispose(); vi.unstubAllGlobals();
  }

  const summary = homeSummary({ runs: 321, clears: 234, deaths: 17, fame_delta: 4567, fame_missing_baselines: 2, fame_sampled: true });
  summary.recent_runs = Array.from({ length: 10 }, (_, index) => ({ ...summary.recent_runs[0], id: `run-${index}`, dungeon: `Preview dungeon ${index}` }));
  const page = renderNexus(summary);
  expect(page.container.textContent).toContain('321');
  expect(page.container.textContent).toContain('234');
  expect(page.container.textContent).toContain('4,567');
  expect(page.container.textContent).not.toMatch(/partial|baseline/i);
  expect(page.container.textContent).not.toMatch(/sampled/i);
  const recent = section('Recent runs');
  expect(recent.querySelectorAll('[data-row-id]')).toHaveLength(6);
  await fireEvent.click(within(recent).getByRole('button', { name: /next/i }));
  expect(recent.querySelectorAll('[data-row-id]')).toHaveLength(4);
  expect(recent.querySelector('[data-row-id="run-6"]')).toBeTruthy();
  expect(page.container.textContent).toContain('321');
});

it('dungeon_stats_observed_clear_null_and_cap', async () => {
  const summary = homeSummary({ dungeon_stats: Array.from({ length: 20 }, (_, index) => ({
    dungeon: `Aggregate dungeon ${index}`, runs: 70 + index, clears: 12, deaths: 1, observed_clears: index === 0 ? 0 : 3,
    mean_clear_micros: index === 0 ? null : 90000000, best_clear_micros: index === 0 ? null : 60000000, last_started: '5000000',
  })) });
  const page = renderNexus(summary);
  const dungeon = section('By dungeon');
  expect(within(dungeon).getAllByText(/^Aggregate dungeon/)).toHaveLength(6);
  expect(dungeon.textContent).not.toContain('Server cap: 20 dungeons');
  expect(dungeon.textContent).not.toContain('Observed-clear times only; missing times unavailable.');
  expect(within(dungeon).getByRole('columnheader', { name: 'Dungeon' })).toBeTruthy();
  expect(within(dungeon).getByRole('rowheader', { name: 'Aggregate dungeon 0' })).toBeTruthy();
  expect(within(dungeon).getByRole('rowheader', { name: 'Aggregate dungeon 1' })).toBeTruthy();
  expect(dungeon.textContent).toContain('\u2014');
  expect(dungeon.textContent).toContain('70');
  await fireEvent.click(within(dungeon).getByRole('button', { name: /next/i }));
  expect(within(dungeon).getAllByText(/^Aggregate dungeon/)).toHaveLength(6);
  expect(dungeon.textContent).toContain('Aggregate dungeon 6');
  await page.rerender({ snapshot: nexus(), history: ready({ ...summary, dungeon_stats: null }), choice: 'session', retry: vi.fn() });
  expect(section('By dungeon').textContent).toContain('\u2014');
  await page.rerender({ snapshot: nexus(), history: ready({ ...summary, dungeon_stats: [] }), choice: 'session', retry: vi.fn() });
  expect(section('By dungeon').textContent).toMatch(/no.*(dungeon|qualifying|runs)/i);
  const legacy = { ...summary }; delete legacy.dungeon_stats;
  await page.rerender({ snapshot: nexus(), history: ready(legacy), choice: 'session', retry: vi.fn() });
  expect(section('By dungeon').textContent).toContain('\u2014');
});

it('recent_details_share_not_rank_or_fame', () => {
  const summary = homeSummary();
  const runs = ['missing', 'unique', 'duplicate'].map(id => ({ ...summary.recent_runs[0], id, dungeon: `Synthetic ${id}`,
    completed_at: '4000000', died_at: '4500000' }));
  const details = [
    { ...summary.recent_details[0], run_id: 'unique', local_share: 0, share_partial: true, loot_preview_limited: true },
    { ...summary.recent_details[0], run_id: 'duplicate', local_share: 0.75 },
    { ...summary.recent_details[0], run_id: 'duplicate', local_share: 0.75 },
  ];
  const page = render(RecentRuns, { runs, details });
  const row = (id: string) => page.container.querySelector(`[data-row-id="${id}"]`)!;
  const shareIndex = 3;
  expect(shareIndex).toBeGreaterThanOrEqual(0);
  const share = (id: string) => row(id).children[shareIndex];
  expect(share('unique').textContent).toMatch(/0%/);
  expect(share('unique').textContent).not.toContain('Partial share');
  for (const id of ['missing', 'duplicate']) {
    expect(share(id).textContent?.trim()).toContain('\u2014');
    expect(share(id).textContent).not.toMatch(/75%|0%/);
  }
  for (const run of runs) {
    expect(row(run.id).textContent).toMatch(/clear|completion/i);
    expect(row(run.id).getAttribute('href')).toBe(routeHref({ kind: 'run', id: run.id }));
expect(row(run.id).querySelector('a, button, details')).toBeNull();
    expect(row(run.id).textContent).not.toMatch(/rank|fame/i);
  }
});

it('nexus_best_bag_not_best_item', () => {
  const summary = homeSummary({ unranked_drop_count: 9 });
  summary.best_drop = { ...summary.best_drop!, contents_known: false, contents: [], items: [], source_estimated: true, source_reason: 'Synthetic source uncertainty' };
  const visuals = { ...emptyHomeVisuals(), phase: 'ready' as const, objects: [catalog(summary.best_drop.bag_type, 'Synthetic ranked bag')] };
  const page = render(NexusHome, { snapshot: nexus(), history: ready(summary), choice: 'session', retry: vi.fn(), visuals });
  const best = section('Best bag');
  expect(best.textContent).toContain('Synthetic ranked bag');
  expect(best.textContent).not.toMatch(/contents.*(unavailable|unknown|unseen)|not observed/i);
  expect(best.textContent).not.toMatch(/source estimated/i);
  expect(best.textContent).not.toContain('9 unranked observed bags excluded from selection.');
  expect(page.container.textContent).not.toMatch(/best UT|best ST|fame.hour|realm.trip|world.boss count/i);
});

it.each(['loading', 'stale', 'superseded', 'busy', 'error'] as const)('nexus_retains_%s_history_state_and_retry', async phase => {
  const retry = vi.fn();
  const retryAvailable = !['loading', 'superseded'].includes(phase);
  const history: HomeHistoryState = { phase, summary: phase === 'loading' ? null : homeSummary(),
    message: `Synthetic ${phase} history reason`, retryAvailable };
  const page = render(NexusHome, { snapshot: nexus(), history, choice: 'today', retry });
  expect(page.container.textContent).not.toContain(history.message);
  setDebugMode(true); await fireEvent.click(page.container);
  expect(page.container.textContent).toContain(history.message);
const error = phase === 'error' || (phase === 'busy' && retryAvailable);
const button = screen.queryByRole('button', { name: 'Retry' });
expect(Boolean(button)).toBe(error);
if (button) { button.focus(); await fireEvent.click(button); }
expect(retry).toHaveBeenCalledTimes(error ? 1 : 0);

});
