import { itemHoverParams } from '../../src/lib/components/item-hover';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import LootBagTable from '../../src/lib/loot/LootBagTable.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { homeSnapshot } from '../home/fixtures';
import type { EnchantView } from '../../src/lib/bindings/EnchantView';

afterEach(cleanup);
const enchants: EnchantView[] = [
  { state: 'recorded', applied: 2, slots: null, ids: [10, 11] },
  { state: 'missing', applied: null, slots: null, ids: [] },
  { state: 'invalid', applied: null, slots: null, ids: [] },
  { state: 'recorded_empty', applied: 0, slots: null, ids: [] },
];
const rows = () => enchants.map((enchant, i) => ({ dungeon: 'Invented Sanctum', drop: {
  ...homeSnapshot().drops[0], id: `bag-${i}`, run_id: 'run-a', contents_known: i !== 1,
  contents: [
    { quantity: 1, slot: 0, item_type: 101, enchant, taken_observed_at: i === 0 ? '2000000' : null },
    { quantity: 1, slot: 1, item_type: 102, enchant: enchants[3], taken_observed_at: null },
  ],
} }));
it('shows the selected item enchant and taken observations and only other item tiles', async () => {
  const page = render(LootBagTable, { rows: rows(), visuals: emptyHomeVisuals(), now: new Date(), columns: 'item', itemType: 101, resolveSpriteUrl: () => null, rowHref: () => '#/runs/run-a' });
  expect(screen.getAllByRole('columnheader').map(header => header.textContent)).toEqual(['When', 'Bag', 'Enchants', 'Also in the bag', 'Taken', 'Source', 'Place', 'Run']);
  for (const label of ['2 enchants', 'Enchant unknown', 'Enchant unreadable', 'No enchants', 'Taken', 'partial']) expect(screen.getAllByText(label).length).toBeGreaterThan(0);
  expect(screen.queryByRole('link', { name: '#101' })).toBeNull();
  expect(screen.getAllByRole('link', { name: '#102' })).toHaveLength(4);
  const row = page.container.querySelector<HTMLElement>('[data-row-id="drop:bag-0"]')!;
  const runClick = vi.fn((event: Event) => event.preventDefault());
  within(row).getByRole('link', { name: 'Open run' }).addEventListener('click', runClick);
  await fireEvent.click(within(row).getByRole('link', { name: '#102' }));
  expect(runClick).not.toHaveBeenCalled();
});
it('keeps Feed columns, item tiles and row selection unchanged', async () => {
  const onSelect = vi.fn(); const data = rows();
  const page = render(LootBagTable, { rows: data, visuals: { ...emptyHomeVisuals(), phase: 'ready' }, now: new Date(), columns: 'feed', resolveSpriteUrl: () => null, onSelect });
  expect(screen.getAllByRole('columnheader').map(header => header.textContent)).toEqual(['Time', 'Bag', 'Contents', 'Source', 'Place', 'Run']);
  expect(screen.getAllByRole('link', { name: '#101' })).toHaveLength(4);
  const tile = screen.getAllByRole('link', { name: '#101' })[0];
  expect(tile.getAttribute('data-item-hover')).toBe('101');
  expect(tile.hasAttribute('title')).toBe(false);
  expect(itemHoverParams(tile)).toEqual({ type: 101, enchant: enchants[0], name: '#101', tier: undefined, note: 'Removal observed' });
  const best = page.container.querySelector('.contents > span')!;
  expect(best.getAttribute('data-item-hover')).toBe('101');
  expect(best.hasAttribute('title')).toBe(false);
  expect(itemHoverParams(best)).toEqual({ type: 101, enchant: enchants[0], name: '#101', tier: undefined });
  await fireEvent.click(page.container.querySelector('[data-row-id]')!);
  expect(onSelect).toHaveBeenCalledExactlyOnceWith(data[0]);
});

it('uses the Home pick for tier, item-type and slot ties, missing tiers and catalog-only classification', async () => {
  const { bestItem } = await import('../../src/lib/home/v3/loot-rank');
  const catalog = { ...emptyHomeVisuals(), phase: 'ready' as const, objects: [
    { object_type: 101, item_tier: null, name: 'Synthetic consumable' },
    { object_type: 102, item_tier: '12', name: 'Synthetic numeric' },
    { object_type: 103, item_tier: 'T12', name: 'Synthetic prefixed' },
    { object_type: 104, item_tier: 'ST', name: 'Synthetic set' },
    { object_type: 105, item_tier: 'UT', name: 'Synthetic untiered' },
    { object_type: 106, item_tier: 'UT', name: 'Synthetic other' },
  ].map(row => ({ ...row, sprite: null, bag: null })) };
  const cases = [
    { types: [101, 102, 104, 105], expected: 105 },
    { types: [105, 106, 105], expected: 105 },
    { types: [101, 102, 104], expected: 104 },
    { types: [103, 102], expected: 102 },
    { types: [101, 102], expected: 102 },
    { types: [999, 101], expected: 101 },
    { types: [999, 998], expected: 998 },
  ];
  for (const { types, expected } of cases) {
    const drop = { ...rows()[0].drop, contents: types.map((item_type, slot) => ({
      ...rows()[0].drop.contents[0], item_type, slot,
    })) };
    const pick = bestItem(drop, catalog);
    expect(pick?.itemType).toBe(expected);
    expect(pick?.slot).toBe(types.indexOf(expected));
    const page = render(LootBagTable, { rows: [{ drop, dungeon: null }], visuals: catalog,
      now: new Date(), columns: 'feed', resolveSpriteUrl: () => null });
    expect(page.container.querySelector('.contents > span')?.getAttribute('data-item-hover')).toBe(String(expected));
    page.unmount();
  }
  // Changing only the catalog tier changes classification; the name is irrelevant.
  const drop = rows()[0].drop;
  expect(bestItem(drop, catalog)?.itemType).toBe(102);
  expect(bestItem(drop, { ...catalog, objects: catalog.objects.map(row => row.object_type === 101
    ? { ...row, item_tier: 'UT' } : row) })?.itemType).toBe(101);
});
