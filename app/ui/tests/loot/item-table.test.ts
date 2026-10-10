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
  const page = render(LootBagTable, { rows: data, visuals: emptyHomeVisuals(), now: new Date(), columns: 'feed', resolveSpriteUrl: () => null, onSelect });
  expect(screen.getAllByRole('columnheader').map(header => header.textContent)).toEqual(['Time', 'Bag', 'Contents', 'Source', 'Place', 'Run']);
  expect(screen.getAllByRole('link', { name: '#101' })).toHaveLength(4);
  await fireEvent.click(page.container.querySelector('[data-row-id]')!);
  expect(onSelect).toHaveBeenCalledExactlyOnceWith(data[0]);
});
