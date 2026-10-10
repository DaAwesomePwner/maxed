import { expect, it } from 'vitest';
import { chaseOrder, collectionKpis, dungeonList, sortCells } from '../../src/lib/loot/collection-screen';
import type { ChaseRow } from '../../src/lib/bindings/ChaseRow';
import type { CollectionCell } from '../../src/lib/bindings/CollectionCell';
import { clockWhen } from '../../src/lib/loot/loot-format';
import { normalizeListState } from '../../src/lib/navigation/state';

const row = (type: number, dry: number, pinned: string): ChaseRow => ({ item: { item_type: type, dungeon: null, pinned_at: pinned }, dry_runs: dry, seen: 0, last_at: null });
it('orders dry streaks descending and tied pins oldest first without mutating the input', () => {
  const rows = [row(1, 2, '30'), row(2, 8, '20'), row(3, 8, '10')];
  expect(chaseOrder(rows).map(row => row.item.item_type)).toEqual([3, 2, 1]);
  expect(rows.map(row => row.item.item_type)).toEqual([1, 2, 3]);
});
it('keeps catalog order, removes duplicates, and appends a missing restored dungeon', () => {
  expect(dungeonList(['B', 'A', 'B'], null)).toEqual(['B', 'A']);
  expect(dungeonList(['B', 'A', 'B'], 'C')).toEqual(['B', 'A', 'C']);
  expect(dungeonList(['B', 'A'], 'A')).toEqual(['B', 'A']);
});
it('shows unknown KPIs with no chase and no chosen dungeon', () => {
  const { cells } = collectionKpis({ chase: [], grid: [], grid_dungeon: null }, [], null);
  expect(cells.map(cell => cell.key)).toEqual(['chasing', 'dry', 'last', 'seen_here']);
  expect(cells.every(cell => cell.value === '—' && cell.sub === null)).toBe(true);
});
it('derives chase and distinct observed item KPIs without counting removed pins', () => {
  const rows = [row(1, 22, '10'), { ...row(2, 3, '20'), seen: 4, last_at: '3000000' }];
  const { cells, lastType } = collectionKpis({ chase: [...rows, row(3, 99, '1')], grid_dungeon: 'A', grid: [
    { item_type: 2, tier: 'ST', count: 4, last_at: '3000000' },
  ] }, rows.map(row => row.item), 'A');
  expect(lastType).toBe(2);
  expect(cells.map(cell => cell.value)).toEqual(['2', '22', '—', '1']);
  expect(cells[0].sub).toBe('1 seen at least once');
  expect(cells[3].sub).toBe('UT and ST items · 0 UT · 1 ST');
});
it('sorts cells by name, seen count, or timestamp without mutating the input', () => {
  const cells: CollectionCell[] = [
    { item_type: 1, tier: 'UT', count: 2, last_at: '9007199254740993' },
    { item_type: 2, tier: 'ST', count: 4, last_at: '9007199254740992' },
  ];
  const nameOf = (type: number) => type === 1 ? 'Zebra' : 'Apple';
  expect(sortCells(cells, 'name', nameOf).map(cell => cell.item_type)).toEqual([2, 1]);
  expect(sortCells(cells, 'seen', nameOf).map(cell => cell.item_type)).toEqual([2, 1]);
  expect(sortCells(cells, 'recent', nameOf).map(cell => cell.item_type)).toEqual([1, 2]);
  expect(cells[0].item_type).toBe(1);
});
it('normalizes saved sorts and rejects unknown sort values', () => {
  for (const collectionSort of ['seen', 'recent', 'name']) {
    expect(normalizeListState({ lootPosition: { collectionSort } }).lootPosition?.collectionSort).toBe(collectionSort);
  }
  expect(normalizeListState({ lootPosition: { collectionSort: 'invalid' } }).lootPosition?.collectionSort).toBeNull();
});

it('omits unknown dungeon suffixes from any-dungeon chase KPIs', () => {
  const chaseRow = { ...row(1, 12, '10'), seen: 2, last_at: '3000000' };
  const { cells } = collectionKpis({ chase: [chaseRow], grid: [], grid_dungeon: null }, [chaseRow.item], null);
  expect(cells[1].sub).toBe('runs · #1');
  expect(cells[2].sub).toBe(clockWhen('3000000', new Date()));
  expect(cells[2].tooltip).toBeNull();
});
it('uses only settings for KPIs when collection observations are unavailable', () => {
  const { cells } = collectionKpis(null, [row(1, 12, '10').item], 'A');
  expect(cells.map(cell => cell.value)).toEqual(['1', '—', '—', '—']);
  expect(cells.every(cell => cell.sub === null)).toBe(true);
});
