import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { PlayerDamage } from '../../src/lib/bindings/PlayerDamage';
import type { RunFightStripRow } from '../../src/lib/bindings/RunFightStripRow';
import { deferred, fight, flush, follow, main, mountRuns, player, ready, region, source } from './fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });
const players = [1, 2, 3].map(rank => player({ rank, object_id: rank, player_id: `p${rank}`, name: `DemoPlayer ${rank}`,
  damage: 1000 - rank * 100, hits: 10, dps: rank === 3 ? null : 100, sources: [source({ damage: 600 }), source({ damage: 200, kind: 'ability' })] }));
const strip: RunFightStripRow[] = ['fight-1', 'fight-2'].map((fight_id, index) => ({ fight_id, target_type: null, boss_type: null,
  target_name: `DemoBoss ${index + 1}`, started: '0', ended: '1000000', killed: true, player_count: 3,
  damage: index ? null : 900, rank: index ? null : 1, share: index ? null : .5 }));
it('shows KPIs, one source table, rank navigation and run context without a run fetch', async () => {
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/1', queries: {
    getFight: async () => ready(fight({ players })),
    getPlayerBreakdown: async (_, id) => ready(players.find(row => row.object_id === id)!), getRunFightStrip: async () => ready(strip),
  } }); await flush();
  expect(within(main().getByRole('navigation', { name: 'Breadcrumb' })).getAllByRole('listitem').map(row => row.textContent?.replace('/', '').trim())).toEqual(['Runs', 'Run', 'DemoBoss 1', 'DemoPlayer 1']);
  expect(main().getByText('party avg 100')).toBeTruthy();
  expect(main().getByText('+100 over #2')).toBeTruthy();
  expect(main().getByText('Rank', { selector: 'dt' }).nextElementSibling?.textContent).toMatch(/1\s*\/\s*3/);
  expect(region('Damage sources').getAllByRole('row')).toHaveLength(3);
  expect(region('Damage sources').queryByText('Enchantments')).toBeNull();
  expect(main().getByLabelText('‹ Previous player').getAttribute('aria-disabled')).toBe('true');
  expect(main().getByRole('link', { name: '› Next player: DemoPlayer 2' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/2');
  expect(page.queries.getRunFightStrip).toHaveBeenCalledWith('run-1', 'p1');
  expect(region('DemoPlayer 1 this run').getByRole('row', { name: /DemoBoss 2/ }).textContent).toContain('———');
  expect(region('DemoPlayer 1 this run').getByRole('link', { name: 'DemoBoss 1' }).getAttribute('aria-current')).toBe('page');
  expect(main().queryByText('Whole run')).toBeNull(); expect(page.queries.getRun).not.toHaveBeenCalled();
  const menu = main().getByRole('combobox', { name: 'Player' });
  expect(within(menu).getAllByRole('option')).toHaveLength(3);
  await fireEvent.change(menu, { target: { value: '3' } }); await flush();
  expect(page.browser.adapter.hash).toBe('#/runs/run-1/fights/fight-1/players/3');
  expect(main().getByLabelText('› Next player').getAttribute('aria-disabled')).toBe('true');
  await follow('#/runs/run-1/fights/fight-1/players/2');
  expect(main().getByRole('link', { name: '‹ Previous player: DemoPlayer 1' })).toBeTruthy();
  expect(main().getByRole('link', { name: '› Next player: DemoPlayer 3' })).toBeTruthy();
});
it.each(['busy', 'error'] as const)('keeps the player visible when the strip is %s', async status => {
  mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: { getRunFightStrip: async () => {
    if (status === 'error') throw new Error('Synthetic failure'); return { status };
  } } }); await flush();
  expect(region('Damage sources').getAllByRole('row')).toHaveLength(2);
  expect(region('Invented Archer this run').getByText('—')).toBeTruthy();
  expect(main().queryByText('Player Error')).toBeNull();
});
it('waits for player identity before fetching the strip and retires a pending strip on navigation', async () => {
  const detail = deferred<HistoryReply<PlayerDamage | null>>();
  const pending = deferred<HistoryReply<RunFightStripRow[]>>();
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7', queries: {
    getPlayerBreakdown: () => detail.promise, getRunFightStrip: () => pending.promise,
  } }); await flush();
  expect(page.queries.getRunFightStrip).not.toHaveBeenCalled();
  detail.resolve(ready(player())); await flush();
  expect(page.queries.getRunFightStrip).toHaveBeenCalledWith('run-1', 'player-1');
  await follow('#/runs', within(screen.getByRole('complementary', { name: 'Character' }))); pending.resolve(ready(strip)); await flush();
  expect(page.browser.adapter.hash).toBe('#/runs');
  expect(main().queryByRole('region', { name: 'Invented Archer this run' })).toBeNull();
});
it('shows selected-player DPS with time-weighted buckets and a text alternative', async () => {
  mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/1', queries: {
    getFight: async () => ready(fight({ players, buckets: [
      { from: '0', until: '1000000', total: 600, local: null }, { from: '1000000', until: '3000000', total: 900, local: null },
    ], player_series: [{ player_id: 'p1', object_id: 1, is_local: true, points: [100, 400] }] })),
    getPlayerBreakdown: async () => ready(players[0]),
  } }); await flush();
  const chart = region('Your DPS over the fight');
  expect(chart.getByRole('img', { name: 'Player DPS' })).toBeTruthy();
  const rows = within(chart.getByRole('table', { name: 'Player DPS by interval' })).getAllByRole('row');
  expect(rows[1].textContent).toContain('100'); expect(rows[2].textContent).toContain('200');
  expect(chart.getByText('party average')).toBeTruthy();
});
it('hides the chart when no player series is available', async () => {
  mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/7' }); await flush();
  expect(main().queryByRole('region', { name: 'Your DPS over the fight' })).toBeNull();
});

it('orders the player menu like neighbor navigation when ranks are missing or nonpositive', async () => {
  const rows = [0, 2, null, -1, 1].map((rank, index) => player({ rank, object_id: index + 1,
    player_id: `p${index + 1}`, name: `DemoPlayer ${index + 1}` }));
  mountRuns({ hash: '#/runs/run-1/fights/fight-1/players/2', queries: {
    getFight: async () => ready(fight({ players: rows })),
    getPlayerBreakdown: async (_, id) => ready(rows.find(row => row.object_id === id)!),
  } }); await flush();
  const menu = main().getByRole('combobox', { name: 'Player' });
  expect(within(menu).getAllByRole('option').map(option => option.getAttribute('value'))).toEqual(['5', '2', '1', '3', '4']);
  expect(main().getByRole('link', { name: '‹ Previous player: DemoPlayer 5' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/5');
  expect(main().getByRole('link', { name: '› Next player: DemoPlayer 1' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-1/players/1');
});
