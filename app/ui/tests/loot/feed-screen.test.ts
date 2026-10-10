import { afterEach, expect, it, vi } from 'vitest';
import { feedKpis, dayGroups, dayLine, headerLine } from '../../src/lib/loot/feed-screen';
import type { DropTotals } from '../../src/lib/bindings/DropTotals';
import { homeSnapshot } from '../home/fixtures';
import { injectChicagoDate } from '../runs/fixtures';
const totals = (patch: Partial<DropTotals> = {}): DropTotals => ({ bags: 18, runs: 12, runs_ended: 41, whites: 3,
  ut_items: 4, st_items: 2, enchanted_items: 6, items_counted: true, best_enchants: 3, last_white: null, first_at: null, ...patch });
afterEach(() => vi.unstubAllGlobals());
it('renders unknown and empty summary KPIs as dashes', () => {
  expect(feedKpis(null).map(cell => cell.value)).toEqual(Array(5).fill('—'));
  expect(feedKpis({ totals: totals({ bags: 0 }), days: [] }).map(cell => cell.value)).toEqual(Array(5).fill('—'));
});
it('uses ended runs for an uncapped white rate and preserves unknown denominators', () => {
  const rate = (runs_ended: number | null) => feedKpis({ totals: totals({ runs_ended }), days: [] }).find(cell => cell.key === 'rate');
  expect(rate(41)).toMatchObject({ label: 'White rate', value: '7.3%', sub: 'over 41 ended runs' });
  expect(rate(2)?.value).toBe('150.0%'); expect(rate(0)?.value).toBe('—'); expect(rate(null)?.value).toBe('—');
});
it('explains uncounted item KPIs and omits uncounted UT day totals', () => {
  const t = totals({ items_counted: false, ut_items: null, st_items: null, enchanted_items: null });
  for (const cell of feedKpis({ totals: t, days: [] }).slice(3)) expect(cell).toMatchObject({ value: '—', tooltip: 'Narrow the date range to count items' });
  expect(dayLine(t)).toBe('18 bags · 3 whites');
});
it('groups by Chicago days across midnight and DST with local day boundaries', () => {
  injectChicagoDate();
  const at = (value: string) => String(new Date(value).getTime() * 1000);
  const rows = ['2026-03-09T00:01:00-05:00', '2026-03-08T23:59:00-05:00', '2026-03-08T00:01:00-06:00', '2026-03-07T23:59:00-06:00']
    .map((value, index) => ({ dungeon: null, drop: { ...homeSnapshot().drops[0], id: String(index), at: at(value) } }));
  const groups = dayGroups(rows, new Date('2026-03-09T01:00:00-05:00'));
  expect(groups.map(g => [g.label, g.rows.length])).toEqual([['Today', 1], ['Yesterday', 2], ['Sat, Mar 7', 1]]);
  expect(BigInt(groups[1].range.end) - BigInt(groups[1].range.start)).toBe(23n * 60n * 60n * 1000000n);
});
it('omits zero day parts except bags and handles absent totals', () => {
  expect(dayLine(null)).toBe(''); expect(dayLine(totals({ whites: 1 }))).toBe('18 bags · 1 white · 4 UT');
  expect(dayLine(totals({ bags: 0, whites: 0, ut_items: 0 }))).toBe('0 bags');
  expect(headerLine(null)).toBeNull(); expect(headerLine(totals({ bags: 0 }))).toBeNull();
  expect(headerLine(totals({ bags: 1284, first_at: String(new Date(2026, 8, 14).getTime() * 1000) }))).toBe('1,284 bags seen · since Sep 14');
});

it('uses singular bag wording in the header for one observed bag', () => {
  expect(headerLine(totals({ bags: 1 }))).toBe('1 bag seen');
  expect(headerLine(totals({ bags: 1, first_at: String(new Date(2026, 8, 14).getTime() * 1000) }))).toBe('1 bag seen · since Sep 14');
});

it('uses singular enchant wording and omits an unknown best enchant count', () => {
  const enchanted = (best_enchants: number | null) => feedKpis({ totals: totals({ best_enchants }), days: [] })[4];
  expect(enchanted(1).sub).toBe('best: 1 enchant');
  expect(enchanted(2).sub).toBe('best: 2 enchants');
  expect(enchanted(null).sub).toBeNull();
});
it('explains the missing white-rate denominator outside a run', () => {
  expect(feedKpis({ totals: totals({ runs_ended: null }), days: [] })[2].sub).toBe('Outside a run: counts only');
});
