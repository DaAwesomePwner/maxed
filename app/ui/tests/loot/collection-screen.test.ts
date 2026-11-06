import CollectionChaseList from '../../src/lib/loot/CollectionChaseList.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { homeSnapshot } from '../home/fixtures';
import { render } from '@testing-library/svelte';
import CollectionGrid from '../../src/lib/loot/CollectionGrid.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { itemHoverParams } from '../../src/lib/components/item-hover';
import { expect, it } from 'vitest';
import { collectionDungeons, chaseOrder, collectionKpis, dungeonList, sortCells } from '../../src/lib/loot/collection-screen';
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

it('opens catalog-level hover cards from collection item links', () => {
  const page = render(CollectionGrid, { dungeon: 'Invented Hall', dungeons: ['Invented Hall'], cells: [{ item_type: 101, tier: 'UT', count: 3, last_at: '2000000' }], visuals: emptyHomeVisuals(), ready: true, onChange: () => {}, chase: [], sort: 'seen', onSort: () => {}, portals: new Map() });
  const tile = page.container.querySelector('[data-row-id="collection-101"]')!;
  expect(tile.getAttribute('data-item-hover')).toBe('101');
  expect(tile.hasAttribute('title')).toBe(false);
  expect(tile.querySelector('.name')?.hasAttribute('title')).toBe(false);
  expect(itemHoverParams(tile)).toEqual({ type: 101, name: '#101', tier: 'UT' });
  expect(page.container.querySelector('nav button')?.getAttribute('title')).toBe('Invented Hall');
});

it('opens a catalog-level hover on a chase item while keeping observation titles', async () => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const settings = await client.querySettings();
  const chaseRow = { ...row(101, 12, '1000000'), last_at: '2000000' };
  chaseRow.item.dungeon = 'Invented Hall';
  settings.loot.chase = [chaseRow.item];
  const visuals = { ...emptyHomeVisuals(), objects: [
    { object_type: 101, name: 'Invented sword', item_tier: 'UT', sprite: null, bag: null },
  ] };
  const page = render(CollectionChaseList, { client, settings, rows: [chaseRow], visuals, onSaved: () => {} });
  const link = page.container.querySelector('a[href="#/loot/item/101"]')!;
  expect(link.getAttribute('data-item-hover')).toBe('101');
  expect(link.hasAttribute('title')).toBe(false);
  expect(itemHoverParams(link)).toEqual({ type: 101, name: 'Invented sword', tier: 'UT' });
  const cells = link.closest('tr')!.querySelectorAll('td');
  expect(cells[1].getAttribute('title')).toBe('Invented Hall');
  expect(cells[3].getAttribute('title')).toBe(clockWhen(chaseRow.last_at, new Date()));
  expect(cells[4].querySelector('.streak')?.getAttribute('title')).toBe('12 runs since last seen');
});

it('ranks visited dungeons by observed bags then ended visits, retaining missing catalog choices', () => {
  const dungeon = (name: string, drops: number, visits: number) => ({ dungeon: name, runs_ended: visits, clears: 0,
    bags: [{ bag_rank: 1, count: drops }], whites: 0, white_runs: 0, avg_runs_per_white: null, longest_dry: visits, runs_since_last_white: null, best: null });
  expect(collectionDungeons(['Invented A', 'Invented B', 'Invented C'], [
    dungeon('Invented A', 1, 10), dungeon('Invented B', 9, 1), dungeon('Invented D', 1, 20),
  ], 'Invented E')).toEqual({ visited: ['Invented B', 'Invented D', 'Invented A'], unvisited: ['Invented C', 'Invented E'] });
});
