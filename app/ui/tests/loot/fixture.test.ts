import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import type { DropFilter } from '../../src/lib/bindings/DropFilter';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import snapshot from '../fixtures/t0-live';
import FixtureApp from '../shell/fixtures/FixtureApp.svelte';
import { fakeHistory } from '../navigation/fake-history';
import { lootBags, lootDungeons, lootQueries, lootSettings, lootVisuals, lootRuns } from '../shell/fixtures/loot';

const filter: DropFilter = { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 };
afterEach(cleanup);
it('pages every bag once, including equal timestamps, and preserves observation states', () => {
  const rows = [];
  let page = lootQueries.listDrops(filter);
  for (;;) {
    rows.push(...page.rows);
    if (!page.next) break;
    page = lootQueries.listDrops({ ...filter, cursor: page.next });
  }
  expect(rows).toEqual(lootBags);
  expect(new Set(rows.map(row => row.drop.id)).size).toBe(rows.length);
  expect(rows.filter(row => row.dungeon !== null)).toHaveLength(120);
  expect(rows.filter(row => row.dungeon === null).length).toBeGreaterThan(0);
  expect(new Set(rows.flatMap(row => row.drop.contents.map(item => item.enchant.state)))).toEqual(new Set(['missing', 'invalid', 'recorded_empty', 'recorded']));
  expect(rows.some(row => !row.drop.contents_known)).toBe(true);
  expect(rows.some(row => row.drop.contents_known && !row.drop.contents.length)).toBe(true);
  expect(rows.some(row => row.drop.issues.includes('stream_gap'))).toBe(true);
  expect(rows.some(row => row.drop.source_enemy === null)).toBe(true);
  expect(rows.some(row => row.drop.source_estimated)).toBe(true);
  expect(lootVisuals.objects.some(row => row.object_type === 65000)).toBe(false);
});
it('applies combined filters and exclusive end dates before paging', () => {
  const range = { start: lootBags.at(-1)!.drop.at, end: lootBags[0].drop.at };
  for (const place of [{ kind: 'outside' } as const, { kind: 'dungeon', name: lootDungeons[0] } as const]) {
    const expected = lootBags.filter(({ drop, dungeon }) => (place.kind === 'outside' ? dungeon === null : dungeon === place.name)
      && drop.bag_rank === 255 && drop.contents.some(item => item.item_type === 100)
      && drop.contents.some(item => item.enchant.state === 'recorded' && (item.enchant.applied ?? 0) > 0)
      && BigInt(drop.at) >= BigInt(range.start) && BigInt(drop.at) < BigInt(range.end));
    const actual = lootQueries.listDrops({ ...filter, place, range, min_rank: 255, item_type: 100, enchanted_only: true });
    expect(actual.rows).toEqual(expected);
    expect(expected.length).toBeGreaterThan(0);
  }
});
it('derives item, stats and collection counts from retained appearances and ended runs', () => {
  const appearances = lootBags.flatMap(({ drop }) => drop.contents.filter(item => item.item_type === 100));
  const item = lootQueries.itemHistory({ item_type: 100, cursor: null, limit: 50 });
  expect(item.totals.appearances).toBe(appearances.length);
  expect(item.totals.bags).toBe(lootBags.filter(({ drop }) => drop.items.includes(100)).length);
  expect(item.enchant_variants.reduce((sum, row) => sum + row.count, 0)).toBe(appearances.length);
  expect(item.by_dungeon.reduce((sum, row) => sum + row.count, 0)).toBe(appearances.length);
  expect(item.by_source.reduce((sum, row) => sum + row.count, 0)).toBe(appearances.length);
  const stats = lootQueries.lootStats({ range: null });
  expect(stats.totals.reduce((sum, row) => sum + row.count, 0)).toBe(lootBags.length);
  expect(stats.whites).toBe(lootBags.filter(row => row.drop.bag_rank === 255).length);
  expect(stats.dungeons).toHaveLength(6);
  expect(stats.dungeons.reduce((sum, row) => sum + row.runs_ended, 0)).toBe(lootRuns.filter(run => run.ended !== null).length);
  expect(stats.sources.filter(row => row.dungeon === null).every(row => row.runs_ended === null)).toBe(true);
  const collection = lootQueries.collection({ dungeon: lootDungeons[0], chase: lootSettings.chase });
  expect(collection.chase).toHaveLength(20);
  expect(collection.chase.some(row => row.seen === 0)).toBe(true);
  for (const row of collection.chase) {
    const count = lootBags.filter(bag => row.item.dungeon === null || bag.dungeon === row.item.dungeon)
      .flatMap(bag => bag.drop.contents).filter(item => item.item_type === row.item.item_type).length;
    expect(row.seen).toBe(count);
  }
  expect(collection.grid.length).toBeGreaterThan(0);
  expect(collection.grid.every(cell => ['UT', 'ST'].includes(cell.tier))).toBe(true);
  expect(lootQueries.searchItems(' Invented ', 3)).toHaveLength(3);
  expect(lootQueries.searchItems('no match', 25)).toEqual([]);
});
it('seeds and round-trips pins without sharing settings between preview clients', async () => {
  const transport = () => createFixtureTransport(snapshot, lootVisuals, undefined, undefined, undefined, undefined, undefined,
    { lootSettings, catalogDungeons: lootDungeons }, lootQueries);
  const client = createClient(transport()), other = createClient(transport());
  expect((await client.querySettings()).loot).toEqual(lootSettings);
  const pins = lootSettings.chase.slice(1);
  await client.setChaseList(pins);
  expect((await client.querySettings()).loot.chase).toEqual(pins);
  expect((await other.querySettings()).loot).toEqual(lootSettings);
});
it.each([
  ['#/loot', 'Invented Item 100'], ['#/loot/stats', 'Invented Abbey'],
  ['#/loot/collection', 'Invented Item 100'], ['#/loot/item/100', 'Invented Item 100'],
  ['#/loot/item/65000', '#65000'],
])('renders populated default fixture route %s', async (hash, text) => {
  render(FixtureApp, { historyAdapter: fakeHistory(hash).adapter });
  await waitFor(() => expect(within(screen.getByRole('main')).getAllByText(text).length).toBeGreaterThan(0));
  expect(screen.queryByText('Loot unavailable')).toBeNull();
});
it('restores the paged feed after an item visit', async () => {
  render(FixtureApp, { historyAdapter: fakeHistory('#/loot').adapter });
  const next = await screen.findByRole('button', { name: 'Older bags' });
  await waitFor(() => expect(next.getAttribute('aria-disabled')).toBe('false'));
  await fireEvent.click(next);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Newer bags' }).getAttribute('aria-disabled')).toBe('false'));
  const ids = () => [...screen.getByRole('main').querySelectorAll('[data-row-id]')].map(row => row.getAttribute('data-row-id'));
  const itemLink = (await within(screen.getByRole('main')).findAllByRole('link', { name: /Invented Item 100/ }))[0];
  const before = ids();
  await fireEvent.click(itemLink);
  await screen.findByRole('heading', { name: 'Item', level: 1 });
  await fireEvent.click(screen.getByRole('button', { name: 'Back' }));
  await screen.findByRole('button', { name: 'Older bags' });
  await waitFor(() => expect(ids()).toEqual(before));
});
it('keeps a preview pin change across collection and item navigation', async () => {
  render(FixtureApp, { historyAdapter: fakeHistory('#/loot/item/100').adapter });
  await fireEvent.click(await screen.findByRole('button', { name: 'Chasing' }));
  await screen.findByRole('button', { name: 'Chase' });
  expect(screen.queryByRole('navigation', { name: 'Loot views' })).toBeNull();
  await fireEvent.click(within(screen.getByRole('navigation', { name: 'Navigation' })).getByRole('link', { name: 'Loot' }));
  await fireEvent.click(within(screen.getByRole('navigation', { name: 'Loot views' })).getByRole('link', { name: 'Collection' }));
  const chase = await screen.findByRole('region', { name: 'Chase items' });
  await waitFor(() => expect(within(chase).getAllByRole('button', { name: /^Stop chasing / })).toHaveLength(19));
  expect(within(chase).queryByText('Invented Item 100')).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'Back' }));
  await screen.findByRole('button', { name: 'Older bags' });
  await fireEvent.click(screen.getByRole('button', { name: 'Back' }));
  const pin = await screen.findByRole('button', { name: 'Chase' });
  await waitFor(() => expect((pin as HTMLButtonElement).disabled).toBe(false));
  await fireEvent.click(pin);
  await screen.findByRole('button', { name: 'Chasing' });
});
