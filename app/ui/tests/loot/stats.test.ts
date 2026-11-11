import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Loot from '../../src/routes/Loot.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { LootStats } from '../../src/lib/bindings/LootStats';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { emptyListState, type ListState } from '../../src/lib/navigation/state';
import { dateRange } from '../../src/lib/runs/filters';
import { presetRange } from '../../src/lib/runs/runs-list';
import { STATS_CAPTION } from '../../src/lib/loot/presentation';
import { whiteRate } from '../../src/lib/loot/loot-format';
import { createRouter } from '../../src/lib/navigation/history';
import { fakeHistory } from '../navigation/fake-history';
import { setDebugMode } from '../../src/lib/debug/debug';
import { deferred, homeSnapshot } from '../home/fixtures';
import { flush } from '../runs/fixtures';

function fixture(): LootStats {
  return {
    dungeons: [
      { dungeon: 'Invented Abbey', runs_ended: 24, clears: 12, bags: [{ bag_rank: 255, count: 3 }], whites: 3,
        runs_since_last_white: 4, white_runs: 3, avg_runs_per_white: 8, longest_dry: 9, best: { drop_id: 'bag-a', run_id: 'run-a', bag_rank: 255, at: '1000000' } },
      { dungeon: 'Invented Burrow', runs_ended: 10, clears: 2, bags: [{ bag_rank: 42, count: 1 }], whites: 0,
        runs_since_last_white: null, white_runs: 0, avg_runs_per_white: null, longest_dry: null, best: { drop_id: 'bag-b', run_id: null, bag_rank: 42, at: '2000000' } },
    ],
    sources: [
      { source_enemy: 900, dungeon: 'Invented Abbey', bags: [{ bag_rank: 255, count: 3 }], whites: 3, runs_ended: 24 },
      { source_enemy: 901, dungeon: null, bags: [{ bag_rank: null, count: 2 }], whites: 0, runs_ended: null },
    ],
    totals: [{ bag_rank: null, count: 2 }, { bag_rank: 42, count: 1 }, { bag_rank: 255, count: 3 }], whites: 3,
  };
}
function savedRange(): ListState {
  return { ...emptyListState(), lootPosition: {
    range: dateRange('2026-03-07', '2026-03-09'),
    feed: { filter: { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 }, previous: [] },
    item: { type: 101, previous: [null] }, collectionDungeon: 'Invented Abbey',
    statsRange: { first: '2026-03-07', last: '2026-03-09' },
  } };
}
function mount(options: { value?: LootStats; savedState?: ListState; error?: boolean; busy?: boolean; container?: HTMLElement } = {}) {
  const lootStats = vi.fn(() => options.value ?? fixture());
  const client = createClient(createFixtureTransport(homeSnapshot(), { client_fingerprint: 'a'.repeat(64), objects: [
    { object_type: 900, name: 'Invented guardian', sprite: null, bag: null },
  ] }, undefined, undefined, undefined, undefined, undefined, {}, { lootStats }));
  const read = vi.spyOn(client, 'getLootStats');
  if (options.error) read.mockRejectedValue(new Error('synthetic read failure'));
  if (options.busy) read.mockResolvedValue({ status: 'busy' });
  const onStateChange = vi.fn(), onRowsReady = vi.fn();
  const view = render(Loot, { target: options.container, props: { route: { kind: 'loot', view: 'stats' }, client,
    savedState: options.savedState, onStateChange, onRowsReady, entryKey: 'stats-entry' } });
  return { ...view, client, read, onStateChange, onRowsReady };
}
beforeEach(() => { setDebugMode(false); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('shows the verbatim footer with debug mode off', async () => {
  mount(); await flush();
  expect(screen.getByText(STATS_CAPTION).closest('footer')).toBeTruthy();
  expect(screen.getByText(STATS_CAPTION).closest('[hidden]')).toBeNull();
});
it('formats white percentages over ended runs and leaves zero denominators undefined', () => {
  expect(whiteRate(3, 24)).toBe('12.5%');
  expect(whiteRate(1, 0)).toBe('—');
});
it('shows No whites yet for a null all-time streak', async () => {
  mount(); await flush();
  expect(within(screen.getByRole('table', { name: 'By dungeon' })).getByText('No whites yet')).toBeTruthy();
});
it('shows counts only and no rate for a runless source', async () => {
  mount(); await flush();
  const table = within(screen.getByRole('table', { name: 'By source' }));
  const row = table.getByText('#901').closest('tr')!;
  expect(within(row).getByTitle('#901 · Outside a run')).toBeTruthy();
  expect(within(row).getByText('—')).toBeTruthy();
  expect(within(row).getByTitle('Outside a run: counts only')).toBeTruthy();
  expect(table.getByText('Invented guardian')).toBeTruthy();
  expect(table.getByText('12.5%')).toBeTruthy();
});
it('applies local calendar dates and saves them while preserving other loot state', async () => {
  const page = mount({ savedState: savedRange() }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: /Custom|Mar 7/ }));
  await fireEvent.input(screen.getByLabelText('From'), { target: { value: '2026-10-01' } });
  await fireEvent.input(screen.getByLabelText('Through'), { target: { value: '2026-10-05' } });
  expect(page.read).toHaveBeenCalledTimes(1);
  await fireEvent.click(screen.getByRole('button', { name: 'Use dates' })); await flush();
  expect(page.read).toHaveBeenLastCalledWith({ range: dateRange('2026-10-01', '2026-10-05') });
  expect(page.onStateChange.mock.lastCall![0]).toMatchObject({ lootPosition: {
    ...savedRange().lootPosition, range: dateRange('2026-10-01', '2026-10-05'),
  }, scrollTop: 0, focusedRowId: null });
  expect(screen.getByRole('button', { name: 'Since last white (all time)' })).toBeTruthy();
});
it('sorts dungeon rows by White rate in either direction without reading again', async () => {
  const page = mount(); await flush();
  const table = screen.getByRole('table', { name: 'By dungeon' });
  const names = () => [...table.querySelectorAll('tbody th .ellipsis')].map(row => row.textContent?.trim());
  expect(names()).toEqual(['Invented Abbey', 'Invented Burrow']);
  const button = within(table).getByRole('button', { name: 'White rate' });
  await fireEvent.click(button);
  expect(names()).toEqual(['Invented Burrow', 'Invented Abbey']);
  expect(button.closest('th')?.getAttribute('aria-sort')).toBe('ascending');
  await fireEvent.click(button);
  expect(names()).toEqual(['Invented Abbey', 'Invented Burrow']);
  expect(button.closest('th')?.getAttribute('aria-sort')).toBe('descending');
  expect(page.read).toHaveBeenCalledTimes(1);
});
it('renders an error with Retry and recovers when the read succeeds', async () => {
  const page = mount({ error: true }); await flush();
  expect(screen.getByRole('heading', { name: "Couldn't load bags" })).toBeTruthy();
  expect(screen.queryByText('No bags yet')).toBeNull();
  expect(screen.getByRole('button', { name: 'Retry' }).hasAttribute('disabled')).toBe(false);
  page.read.mockResolvedValue({ status: 'ready', value: fixture() });
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
  expect(screen.getByRole('table', { name: 'By dungeon' })).toBeTruthy();
});
it('uses restored calendar days on the first read', async () => {
  const page = mount({ savedState: savedRange() }); await flush();
  expect(page.read).toHaveBeenCalledExactlyOnceWith({ range: dateRange('2026-03-07', '2026-03-09') });
  await fireEvent.click(screen.getByRole('button', { name: 'Mar 7 – Mar 9' }));
  expect((screen.getByLabelText('From') as HTMLInputElement).value).toBe('2026-03-07');
  expect((screen.getByLabelText('Through') as HTMLInputElement).value).toBe('2026-03-09');
});
it('resets dates and persisted scope to All time', async () => {
  const page = mount({ savedState: savedRange() }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'All time' })); await flush();
  expect(page.read).toHaveBeenLastCalledWith({ range: null });
  expect(page.onStateChange.mock.lastCall![0].lootPosition?.range).toBeNull();
  expect(screen.getByRole('button', { name: 'All time' }).getAttribute('aria-pressed')).toBe('true');
  expect(screen.queryByRole('button', { name: 'Since last white (all time)' })).toBeNull();
});
it('rejects reversed dates without changing saved scope or issuing a read', async () => {
  const page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: /Custom|Mar 7/ }));
  await fireEvent.input(screen.getByLabelText('From'), { target: { value: '2026-10-05' } });
  await fireEvent.input(screen.getByLabelText('Through'), { target: { value: '2026-10-01' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Use dates' })); await flush();
  expect(screen.getByRole('alert').textContent).toBe('Invalid Date Range');
  expect(page.read).toHaveBeenCalledTimes(1);
  expect(page.onStateChange).not.toHaveBeenCalled();
});
it('shows tier totals, total whites, and links only best bags with a run', async () => {
  mount(); await flush();
  const tiers = within(screen.getByRole('table', { name: 'Bags by tier' }));
  expect(tiers.getByRole('row', { name: 'Unknown tier 2' })).toBeTruthy();
  expect(tiers.getByRole('row', { name: 'Rank 42 1' })).toBeTruthy();
  expect(tiers.getByRole('row', { name: 'White bag 3' })).toBeTruthy();
  expect(document.querySelector('[data-kpi=whites] .value')?.textContent).toBe('3');
  expect(screen.getByRole('link', { name: /White bag ·/ }).getAttribute('href')).toBe('#/runs/run-a');
  expect(within(screen.getByRole('table', { name: 'By dungeon' })).getByTitle('Rank 42').closest('a')).toBeNull();
});
it('waits for stats before releasing rows-ready once', async () => {
  const page = mount(), pending = deferred<HistoryReply<LootStats>>();
  page.read.mockReturnValueOnce(pending.promise); await flush();
  expect(screen.getByRole('heading', { name: 'Loading loot stats' })).toBeTruthy();
  expect(page.onRowsReady).not.toHaveBeenCalled();
  pending.resolve({ status: 'ready', value: fixture() }); await flush();
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('stats-entry');
});
it('keeps a busy read distinct from empty data and offers Retry', async () => {
  const page = mount({ busy: true }); await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(screen.getByRole('heading', { name: 'Loot history busy' })).toBeTruthy();
  expect(screen.queryByText('No bags yet')).toBeNull();
  expect(screen.getByRole('button', { name: 'Retry' }).hasAttribute('disabled')).toBe(false);
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('stats-entry');
});
it('shows an empty state only when there is no data at all', async () => {
  mount({ value: { dungeons: [], sources: [], totals: [], whites: 0 } }); await flush();
  expect(screen.getByRole('heading', { name: 'No bags yet' })).toBeTruthy();
  expect(screen.queryAllByRole('table')).toHaveLength(0);
});
it('distinguishes an empty date range from an empty all-time history', async () => {
  mount({ value: { dungeons: [], sources: [], totals: [], whites: 0 }, savedState: savedRange() }); await flush();
  expect(screen.getByRole('heading', { name: 'No bags in this range' })).toBeTruthy();
  expect(screen.queryByText('No bags yet')).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'All time' })); await flush();
  expect(screen.getByRole('heading', { name: 'No bags yet' })).toBeTruthy();
  expect(screen.queryByText('No bags in this range')).toBeNull();
});
it('retains ended-run rows with no bags and renders an undefined rate as a dash', async () => {
  const value = fixture(); value.dungeons[0].runs_ended = 0;
  value.dungeons[0].bags = []; value.dungeons[0].whites = 0; value.dungeons[0].best = null;
  value.dungeons = [value.dungeons[0]]; value.sources = []; value.totals = []; value.whites = 0;
  mount({ value }); await flush();
  const row = screen.getByRole('row', { name: /Invented Abbey/ });
  expect(within(row).getAllByText('—')).toHaveLength(2);
  expect(screen.queryByText('No bags yet')).toBeNull();
});
it('releases rows-ready without a client, even while disabled', async () => {
  const onRowsReady = vi.fn();
  render(Loot, { route: { kind: 'loot', view: 'stats' }, enabled: false, onRowsReady, entryKey: 'absent' }); await flush();
  expect(screen.getByRole('heading', { name: 'Loot unavailable' })).toBeTruthy();
  expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('absent');
});
it('does not read or release rows-ready with a client while disabled', async () => {
  const page = mount();
  await page.rerender({ enabled: false }); await flush();
  expect(page.read).not.toHaveBeenCalled(); expect(page.onRowsReady).not.toHaveBeenCalled();
  await page.rerender({ enabled: true }); await flush();
  expect(page.read).toHaveBeenCalledTimes(1);
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('stats-entry');
});
it.each(['error', 'busy'] as const)('does not show a retained empty state after a %s refresh', async phase => {
  const page = mount({ value: { dungeons: [], sources: [], totals: [], whites: 0 } }); await flush();
  expect(screen.getByRole('heading', { name: 'No bags yet' })).toBeTruthy();
  if (phase === 'error') page.read.mockRejectedValue(new Error('synthetic refresh failure'));
  else page.read.mockResolvedValue({ status: 'busy' });
  await fireEvent.click(screen.getByRole('button', { name: 'Today' })); await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(screen.getByRole('heading', { name: phase === 'error' ? "Couldn't load bags" : 'Loot history busy' })).toBeTruthy();
  expect(screen.queryByRole('heading', { name: /^No bags/ })).toBeNull();
});

it('renders all 18 dungeon rows without paging controls and links to their feeds', async () => {
  const value = fixture();
  value.dungeons = Array.from({ length: 18 }, (_, i) => ({ ...value.dungeons[0], dungeon: `Invented dungeon ${i}` }));
  mount({ value }); await flush();
  const table = screen.getByRole('table', { name: 'By dungeon' });
  expect(table.querySelectorAll('tbody tr')).toHaveLength(18);
  expect(screen.queryByRole('button', { name: /Older|Newer|Show more/ })).toBeNull();
  const link = within(table).getByRole('link', { name: 'Invented dungeon 0' });
  expect(link.getAttribute('href')).toBe('#/loot/in/Invented%20dungeon%200');
  const click = vi.fn((event: Event) => event.preventDefault()); link.addEventListener('click', click);
  await fireEvent.click(link.closest('tr')!.querySelector('td')!);
  expect(click).toHaveBeenCalledTimes(1);
});
it('shows percentage rates including zero whites and no ended runs', async () => {
  const value = fixture(); value.dungeons[0].runs_ended = 41;
  value.dungeons.push({ ...value.dungeons[1], dungeon: 'Invented Cave', runs_ended: 0 });
  mount({ value }); await flush();
  const table = within(screen.getByRole('table', { name: 'By dungeon' }));
  expect(table.getByText('7.3%')).toBeTruthy(); expect(table.getByText('0.0%')).toBeTruthy(); expect(table.getByText('—')).toBeTruthy();
});
it('persists aria-sort through Back and Forward', async () => {
  const browser = fakeHistory('#/loot/stats'), router = createRouter(browser.adapter);
  const page = mount({ savedState: router.state.entry }); await flush();
  expect(screen.getByRole('button', { name: 'Ended runs' }).closest('th')?.getAttribute('aria-sort')).toBe('descending');
  await fireEvent.click(screen.getByRole('button', { name: 'Ended runs' }));
  router.save(page.onStateChange.mock.lastCall![0]);
  router.navigate({ kind: 'loot', view: 'feed' }); router.back();
  await page.rerender({ savedState: router.state.entry }); await flush();
  expect(screen.getByRole('button', { name: 'Ended runs' }).closest('th')?.getAttribute('aria-sort')).toBe('ascending');
  router.forward(); router.navigate({ kind: 'loot', view: 'stats' });
  await page.rerender({ savedState: router.state.entry }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'White rate' }));
  await fireEvent.click(screen.getByRole('button', { name: 'White rate' }));
  router.save(page.onStateChange.mock.lastCall![0]);
  router.back(); router.back(); await page.rerender({ savedState: router.state.entry }); await flush();
  expect(screen.getByRole('button', { name: 'Ended runs' }).closest('th')?.getAttribute('aria-sort')).toBe('ascending');
  router.forward(); router.forward(); await page.rerender({ savedState: router.state.entry }); await flush();
  expect(screen.getByRole('button', { name: 'White rate' }).closest('th')?.getAttribute('aria-sort')).toBe('descending');
  router.dispose();
});
it('refetches a preset and shares it with the Feed', async () => {
  const router = createRouter(fakeHistory('#/loot/stats').adapter);
  const page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: '7 days' })); await flush();
  const range = presetRange('7d', new Date());
  expect(page.read).toHaveBeenLastCalledWith({ range });
  router.save(page.onStateChange.mock.lastCall![0]); router.navigate({ kind: 'loot', view: 'feed' });
  const drops = vi.spyOn(page.client, 'listDrops');
  await page.rerender({ route: { kind: 'loot', view: 'feed' }, savedState: router.state.entry }); await flush();
  expect(drops.mock.lastCall?.[0].range).toEqual(range);
  router.dispose();
});
it('resets the main pane on a range change but preserves its initial scroll', async () => {
  const pane = document.createElement('main');
  document.body.append(pane); pane.scrollTop = 350;
  mount({ container: pane });
  expect(pane.querySelector('.loot-stats')).not.toBeNull();
  expect(pane.scrollTop).toBe(350);
  await flush(); expect(pane.scrollTop).toBe(350);
  await fireEvent.click(screen.getByRole('button', { name: 'Today' })); await flush();
  expect(pane.scrollTop).toBe(0); pane.remove();
});
it('sorts source rows by bags and caps them at the top 12', async () => {
  const value = fixture(); value.sources = Array.from({ length: 15 }, (_, i) => ({ ...value.sources[0], source_enemy: 1000 + i, bags: [{ bag_rank: null, count: i + 1 }] }));
  mount({ value }); await flush();
  const rows = screen.getByRole('table', { name: 'By source' }).querySelectorAll('tbody tr');
  expect(rows).toHaveLength(12); expect(rows[0].textContent).toContain('#1014'); expect(rows[11].textContent).toContain('#1003');
  expect(screen.getByText('top 12 of 15')).toBeTruthy();
});
it.each([true, false])('uses resolved catalog names for tiers and best bags (bag metadata: %s)', async metadata => {
  const value = fixture(); value.totals[1].bag_type = 402; value.dungeons[1].best!.bag_type = 402;
  const page = mount({ value });
  const visuals = vi.spyOn(page.client, 'getObjectVisuals').mockResolvedValue({ client_fingerprint: 'a'.repeat(64), objects: [
    { object_type: 402, name: 'Synthetic blue bag', sprite: null, bag: metadata ? { object_type: 402, name: 'Synthetic blue bag', base_type: 402, is_white: false, rank: 42, capture: true } : null },
  ] });
  await flush();
  expect(visuals.mock.calls.some(([types]) => types.includes(402))).toBe(true);
  const tiers = within(screen.getByRole('table', { name: 'Bags by tier' }));
  expect(tiers.getByRole('row', { name: 'Synthetic blue bag 1' })).toBeTruthy();
  expect(tiers.queryByText('Rank 42')).toBeNull();
  expect(within(screen.getByRole('table', { name: 'By dungeon' })).getByTitle('Synthetic blue bag')).toBeTruthy();
});

it('keeps the Stats sort after changing range and traversing Back and Forward', async () => {
  const router = createRouter(fakeHistory('#/loot').adapter);
  router.navigate({ kind: 'loot', view: 'stats' });
  const page = mount({ savedState: router.state.entry }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Whites' }));
  await fireEvent.click(screen.getByRole('button', { name: '7 days' })); await flush();
  router.save(page.onStateChange.mock.lastCall![0]);
  router.back(); router.forward();
  await page.rerender({ savedState: router.state.entry }); await flush();
  expect(screen.getByRole('button', { name: 'Whites' }).closest('th')?.getAttribute('aria-sort')).toBe('ascending');
  expect(screen.getByRole('button', { name: '7 days' }).getAttribute('aria-pressed')).toBe('true');
  router.dispose();
});

it('keeps 7 days when drilling from a Stats dungeon row into Feed', async () => {
  const router = createRouter(fakeHistory('#/loot/stats').adapter);
  const page = mount({ savedState: router.state.entry }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: '7 days' })); await flush();
  router.save(page.onStateChange.mock.lastCall![0]);
  const range = router.state.entry.lootPosition!.range;
  const link = within(screen.getByRole('table', { name: 'By dungeon' })).getByRole('link', { name: 'Invented Abbey' });
  const { parseRoute } = await import('../../src/lib/navigation/routes');
  link.addEventListener('click', event => { event.preventDefault(); router.navigate(parseRoute(link.getAttribute('href')!)); });
  await fireEvent.click(link.closest('tr')!.querySelector('td')!);
  page.unmount();
  const read = vi.spyOn(page.client, 'listDrops');
  render(Loot, { route: router.state.entry.route as { kind: 'loot'; view: 'feed' }, client: page.client, savedState: router.state.entry }); await flush();
  expect(read.mock.calls[0][0]).toMatchObject({ range, place: { kind: 'dungeon', name: 'Invented Abbey' } });
  expect(screen.getByRole('button', { name: '7 days' }).getAttribute('aria-pressed')).toBe('true');
  router.dispose();
});
