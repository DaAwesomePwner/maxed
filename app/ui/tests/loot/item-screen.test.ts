import { expect, it } from 'vitest';
import type { ItemHistory } from '../../src/lib/bindings/ItemHistory';
import { itemKpis, shareRows } from '../../src/lib/loot/item-screen';

const fixture = (): ItemHistory => ({ item_type: 101,
  totals: { appearances: 9, bags: 8, taken: 3, first_at: '1', last_at: '2' },
  by_dungeon: [{ dungeon: 'Invented Sanctum', count: 6, last_at: '2', runs_since: 4 }, { dungeon: 'Invented Hall', count: 3, last_at: '1', runs_since: 12 }],
  by_source: [], bags: { rows: [], next: null }, enchant_variants: [
    { state: 'recorded', applied: 2, ids: [1, 2], count: 3 },
    { state: 'recorded', applied: 3, ids: [1, 2, 3], count: 1 },
    { state: 'missing', applied: 4, ids: [], count: 2 },
    { state: 'invalid', applied: 4, ids: [], count: 2 },
    { state: 'recorded_empty', applied: 0, ids: [], count: 1 },
  ] });
it('adapts all five KPIs and counts only recorded positive enchants', () => {
  expect(itemKpis(fixture(), [])).toEqual([
    { key: 'seen', label: 'Seen', value: '9', sub: 'in 8 bags' },
    { key: 'taken', label: 'Taken', value: '3', sub: 'removed from the bag' },
    { key: 'most', label: 'Most seen in', value: 'Invented Sanctum', sub: '6 of 9', tooltip: 'Invented Sanctum' },
    { key: 'enchanted', label: 'Enchanted', value: '4', sub: 'best: 3 enchants' },
    { key: 'dry', label: 'Dry streak', value: '4', sub: 'runs of Invented Sanctum since last seen' },
  ]);
});
it('uses singular wording for one enchant and hides the best subline without positive recorded variants', () => {
  const h = fixture();
  h.enchant_variants = [{ state: 'recorded', applied: 1, ids: [1], count: 3 }];
  expect(itemKpis(h, []).find(cell => cell.key === 'enchanted')).toMatchObject({ value: '3', sub: 'best: 1 enchant' });
  h.enchant_variants = fixture().enchant_variants.filter(variant => variant.state !== 'recorded');
  h.enchant_variants.push({ state: 'recorded', applied: 0, ids: [], count: 1 });
  expect(itemKpis(h, []).find(cell => cell.key === 'enchanted')).toMatchObject({ value: '0', sub: null });
  h.enchant_variants = [];
  expect(itemKpis(h, []).find(cell => cell.key === 'enchanted')).toMatchObject({ value: '0', sub: null });
});
it('limits the place tooltip to Most seen in, including outside a run', () => {
  const h = fixture(); h.by_dungeon[0].dungeon = null;
  expect(itemKpis(h, []).filter(cell => cell.tooltip)).toEqual([
    { key: 'most', label: 'Most seen in', value: 'Outside a run', sub: '6 of 9', tooltip: 'Outside a run' },
  ]);
});
it('uses the chased dungeon, ignoring other items and unscoped chase entries', () => {
  const chase = [{ item_type: 102, dungeon: 'Invented Sanctum', pinned_at: '1' }, { item_type: 101, dungeon: null, pinned_at: '1' }, { item_type: 101, dungeon: 'Invented Hall', pinned_at: '1' }];
  expect(itemKpis(fixture(), chase).at(-1)).toMatchObject({ value: '12', sub: 'runs of Invented Hall since last seen' });
  expect(itemKpis(fixture(), chase.slice(0, 2)).at(-1)?.value).toBe('4');
});
it('keeps missing, legacy and outside-run dry streaks unknown', () => {
  const h = fixture();
  for (const place of [{ dungeon: null, runs_since: 4 }, { dungeon: 'Invented Sanctum', runs_since: null }, { dungeon: 'Invented Sanctum', runs_since: undefined }]) {
    h.by_dungeon = [{ ...place, count: 9, last_at: '2' }];
    expect(itemKpis(h, []).at(-1)?.value).toBe('—');
  }
  expect(itemKpis(h, [{ item_type: 101, dungeon: 'Unseen dungeon', pinned_at: '1' }]).at(-1)).toMatchObject({ value: '—', sub: 'runs of Unseen dungeon since last seen' });
});
it('preserves a known zero dry streak', () => {
  const h = fixture(); h.by_dungeon[0].runs_since = 0;
  expect(itemKpis(h, []).at(-1)?.value).toBe('0');
});
it('scales shares to the maximum without mutating the input', () => {
  const rows = [{ count: 2, name: 'a' }, { count: 8, name: 'b' }, { count: 0, name: 'c' }];
  expect(shareRows(rows)).toEqual([{ count: 2, name: 'a', share: .25 }, { count: 8, name: 'b', share: 1 }, { count: 0, name: 'c', share: 0 }]);
  expect(rows[0]).not.toHaveProperty('share');
  expect(shareRows([])).toEqual([]);
  expect(shareRows([{ count: 0 }])).toEqual([{ count: 0, share: 0 }]);
});
