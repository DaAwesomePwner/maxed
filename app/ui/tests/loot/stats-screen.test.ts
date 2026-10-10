import { expect, it } from 'vitest';
import { rankShade, sortDungeons, stackSegments, statsKpis } from '../../src/lib/loot/stats-screen';
import { whiteRate } from '../../src/lib/loot/loot-format';
import type { DungeonLootStats } from '../../src/lib/bindings/DungeonLootStats';
const row = (dungeon: string, runs = 10): DungeonLootStats => ({ dungeon, runs_ended: runs, clears: 5, bags: [], whites: 0, runs_since_last_white: null, best: null });
it.each([
  [255, 'var(--bag-white)'],
  [null, 'var(--text-muted)'],
  [0, 'color-mix(in srgb, var(--violet) 35%, transparent)'],
  [2, 'color-mix(in srgb, var(--violet) 67.5%, transparent)'],
  [42, 'color-mix(in srgb, var(--violet) 100%, transparent)'],
] as const)('shades rank %s by ordinal position without inferring a bag colour', (rank, shade) => {
  const ranks = [42, 255, 0, 2, 42];
  expect(rankShade(rank, ranks)).toBe(shade);
  expect(ranks).toEqual([42, 255, 0, 2, 42]);
});
it('uses full violet for a single non-white rank', () => {
  expect(rankShade(42, [255, 42, 42])).toBe('color-mix(in srgb, var(--violet) 100%, transparent)');
  expect(rankShade(42, [])).toBe('color-mix(in srgb, var(--violet) 100%, transparent)');
});
it('uses ended-run denominators, including zero whites and uncapped rates', () => {
  expect(whiteRate(0, 10)).toBe('0.0%'); expect(whiteRate(1, 0)).toBe('—'); expect(whiteRate(3, 41)).toBe('7.3%'); expect(whiteRate(2, 1)).toBe('200.0%');
});
it('builds a 100 percent stack without zero counts and preserves unknown metadata', () => {
  const segments = stackSegments([{ bag_rank: 255, bag_type: 10, count: 3 }, { bag_rank: 2, count: 0 }, { bag_rank: null, count: 1 }]);
  expect(segments).toEqual([{ rank: 255, bagType: 10, percent: 75 }, { rank: null, bagType: null, percent: 25 }]);
  expect(segments.reduce((n, s) => n + s.percent, 0)).toBe(100); expect(stackSegments([])).toEqual([]);
});
it('sorts nulls last in both directions, breaks ties by name, and does not mutate input', () => {
  const rows = [row('Zed'), row('Absent', 0), row('Abbey')];
  for (const descending of [true, false]) expect(sortDungeons(rows, 'rate', descending).map(r => r.dungeon)).toEqual(['Abbey', 'Zed', 'Absent']);
  expect(rows[0].dungeon).toBe('Zed');
  expect(sortDungeons([row('Small', 1), row('Large', 20)], 'runs_ended', true)[0].dungeon).toBe('Large');
});
it('renders all empty KPI values as unknown', () => {
  const cells = statsKpis({ dungeons: [], sources: [], totals: [], whites: 0 });
  expect(cells.map(c => c.key)).toEqual(['runs', 'bags', 'whites', 'dry', 'best']);
  expect(cells.every(c => c.value === '—' && c.sub == null)).toBe(true);
});
it('sums run outcomes, takes the longest streak and chooses the newest highest rank', () => {
  const a = { ...row('Abbey', 20), runs_since_last_white: 22, best: { drop_id: 'a', run_id: null, bag_rank: 255, at: '1000000' } };
  const b = { ...row('Burrow'), runs_since_last_white: 2, best: { drop_id: 'b', run_id: null, bag_rank: 255, at: '2000000' } };
  const cells = statsKpis({ dungeons: [a, b], sources: [], totals: [{ bag_rank: 255, count: 3 }], whites: 3 });
  expect(cells[0]).toMatchObject({ value: '30', sub: '10 cleared · 33%' });
  expect(cells[1]).toMatchObject({ value: '3', sub: '0.1 per run' });
  expect(cells[2].sub).toBe('10.0% white rate');
  expect(cells[3]).toMatchObject({ value: '22', sub: 'runs of Abbey since a white', tone: 'fame' });
  expect(cells[4].sub).toMatch(/^Burrow · /);
});
it('normalizes malformed saved sorts without changing legacy entry shapes', async () => {
  const { normalizeListState } = await import('../../src/lib/navigation/state');
  expect(normalizeListState({ lootPosition: {} }).lootPosition).not.toHaveProperty('statsSort');
  expect(normalizeListState({ lootPosition: { statsSort: { key: 'invalid', descending: true } } }).lootPosition?.statsSort).toBeNull();
  expect(normalizeListState({ lootPosition: { statsSort: { key: 'runs_ended', descending: true } } }).lootPosition?.statsSort).toEqual({ key: 'runs_ended', descending: true });
});
