import { render, screen, within } from '@testing-library/svelte';
import { expect, it } from 'vitest';
import LootPreview from './LootPreview.svelte';
import { createLootPreview } from './loot-scenario';
import type { LootDropRow } from '../../src/lib/bindings/LootDropRow';
import { dayGroups } from '../../src/lib/loot/feed-screen';
import type { DropCursor } from '../../src/lib/bindings/DropCursor';

it('mounts synthetic bags and defaults to the Loot feed', async () => {
  window.history.replaceState(null, '', '/?preview=loot'); render(LootPreview);
  expect(await within(screen.getByRole('main')).findAllByText(/Synthetic relic/)).not.toHaveLength(0);
  expect(location.hash).toBe('#/loot');
  expect(screen.getByRole('heading', { name: 'Loot', level: 1 }).classList.contains('sr-only')).toBe(true);
});
it.each(['default', 'stress'] as const)('provides the %s scenario sizes and mutable chase settings', async variant => {
  const preview = createLootPreview(variant);
  try {
    const stats = await preview.client.getLootStats({ range: null });
    if (stats.status !== 'ready') throw new Error('Expected ready history');
    expect(stats.value.dungeons).toHaveLength(variant === 'stress' ? 128 : 18);
    const settings = await preview.client.querySettings(); expect(settings.loot.chase).toHaveLength(variant === 'stress' ? 100 : 6);
    const bags: LootDropRow[] = [];
    let cursor: DropCursor | null = null, total = 0, pages = 0;
    do {
      const reply = await preview.client.listDrops({ range: null, min_rank: null, place: null, item_type: null, enchanted_only: false, cursor, limit: 50 });
      if (reply.status !== 'ready') throw new Error('Expected ready bags');
      bags.push(...reply.value.rows);
      total += reply.value.rows.length; pages++; cursor = reply.value.next;
      if (variant === 'stress') expect(reply.value.rows).toHaveLength(50);
    } while (cursor);
    expect(total).toBe(variant === 'stress' ? 1000 : 66); expect(pages).toBe(variant === 'stress' ? 20 : 2);
    expect(new Set(bags.map(row => row.drop.id)).size).toBe(total);
    expect(new Set(bags.map(row => new Date(Number(BigInt(row.drop.at) / 1000n)).toDateString())).size).toBe(3);
    expect(bags.some(row => row.drop.bag_rank === 255)).toBe(true);
    expect(bags.some(row => !row.drop.contents_known)).toBe(true);
    expect(bags.some(row => row.drop.issues.length > 0)).toBe(true);
    expect(bags.some(row => row.drop.source_estimated)).toBe(true);
    expect(bags.some(row => row.drop.source_enemy === null)).toBe(true);
    expect(bags.some(row => row.drop.run_id === null && row.dungeon === null)).toBe(true);
    const contents = bags.flatMap(row => row.drop.contents);
    const filter = { range: null, min_rank: null, place: null, item_type: null, enchanted_only: false, cursor: null, limit: 50 };
    const summary = await preview.client.getDropSummary({ filter, days: dayGroups(bags, new Date()).map(group => group.range), include_items: true });
    if (summary.status !== 'ready') throw new Error('Expected ready drop summary');
    expect(summary.value.totals.bags).toBe(total);
    expect(summary.value.days.reduce((sum, day) => sum + day.bags, 0)).toBe(total);
    expect(summary.value.totals.whites).toBe(bags.filter(row => row.drop.bag_rank === 255).length);
    expect(summary.value.totals.enchanted_items).toBe(contents.filter(item => (item.enchant.applied ?? 0) > 0).length);
    const outside = await preview.client.getDropSummary({ filter: { ...filter, place: { kind: 'outside' } }, days: [], include_items: false });
    if (outside.status !== 'ready') throw new Error('Expected ready outside summary');
    expect(outside.value.totals.bags).toBe(bags.filter(row => row.dungeon === null).length);
    expect(outside.value.totals).toMatchObject({ runs_ended: null, items_counted: false, ut_items: null, st_items: null, enchanted_items: null });

    expect(contents.some(item => item.taken_observed_at !== null)).toBe(true);
    expect([...new Set(contents.map(item => item.enchant.applied))].sort()).toEqual([0, 1, 2, 3]);
    const collection = await preview.client.getCollection({ dungeon: null, chase: settings.loot.chase });
    if (collection.status !== 'ready') throw new Error('Expected ready collection');
    expect(collection.value.chase.filter(row => row.seen === 0).length).toBeGreaterThanOrEqual(2);
    const chosen = stats.value.dungeons[0].dungeon;
    const grid = await preview.client.getCollection({ dungeon: chosen, chase: settings.loot.chase });
    if (grid.status !== 'ready') throw new Error('Expected ready dungeon grid');
    expect(grid.value.grid_dungeon).toBe(chosen);
    expect(grid.value.grid.length).toBeGreaterThan(0);
    if (variant === 'default') {
      expect(grid.value.grid).toHaveLength(14);
      expect(grid.value.chase.filter(row => row.seen === 0)).toHaveLength(2);
    }
    expect(grid.value.grid.every(cell => bags.some(row => row.dungeon === chosen && row.drop.items.includes(cell.item_type)))).toBe(true);
    expect(new Set(grid.value.chase.map(row => row.dry_runs)).size).toBeGreaterThan(1);
    const catalog = await preview.client.searchItems('Synthetic', 100);
    expect(catalog.some(item => item.tier === 'UT')).toBe(true);
    expect(catalog.some(item => item.tier === 'ST')).toBe(true);
    if (variant === 'stress') {
      expect(catalog.every(item => item.name.length === 64)).toBe(true);
      expect(stats.value.dungeons.every(row => row.dungeon.length === 64)).toBe(true);
    }
    const item = await preview.client.getItemHistory({ item_type: 2000, cursor: null, limit: 50 });
    if (item.status !== 'ready') throw new Error('Expected ready item history');
    expect(item.value.by_source.reduce((sum, row) => sum + row.count, 0)).toBe(item.value.totals.appearances);
    expect(item.value.enchant_variants.reduce((sum, row) => sum + row.count, 0)).toBe(item.value.totals.appearances);
    await preview.client.setChaseList([]); expect((await preview.client.querySettings()).loot.chase).toEqual([]);
  } finally { preview.dispose(); }
});
