import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Loot from '../../src/routes/Loot.svelte';
import CollectionChaseList from '../../src/lib/loot/CollectionChaseList.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { Collection } from '../../src/lib/bindings/Collection';
import type { CollectionRequest } from '../../src/lib/bindings/CollectionRequest';
import { emptyListState, type ListState } from '../../src/lib/navigation/state';
import { setDebugMode } from '../../src/lib/debug/debug';
import { deferred, homeSnapshot } from '../home/fixtures';
import { flush } from '../runs/fixtures';
import { shortWhen } from '../../src/lib/loot/loot-format';
import { createRouter } from '../../src/lib/navigation/history';
import { fakeHistory } from '../navigation/fake-history';

function fixture(): Collection {
  return { chase: [
    { item: { item_type: 101, dungeon: 'Invented Abbey', pinned_at: '1000000' }, seen: 2, last_at: '2000000', dry_runs: 1 },
    { item: { item_type: 102, dungeon: null, pinned_at: '1000000' }, seen: 3, last_at: '3000000', dry_runs: 5 },
    { item: { item_type: 103, dungeon: 'Invented Abbey', pinned_at: '1000000' }, seen: 0, last_at: null, dry_runs: 9 },
  ], grid_dungeon: null, grid: [] };
}
function mount(options: { value?: Collection; dungeons?: string[]; savedState?: ListState; error?: boolean; settingsError?: boolean; catalogError?: boolean; busy?: boolean; container?: HTMLElement } = {}) {
  const value = options.value ?? fixture();
  const collection = vi.fn((request: CollectionRequest): Collection => ({ ...value, grid_dungeon: request.dungeon,
    chase: value.chase.filter(row => request.chase.some(pin => pin.item_type === row.item.item_type && pin.dungeon === row.item.dungeon)) }));
  const client = createClient(createFixtureTransport(homeSnapshot(), { client_fingerprint: 'a'.repeat(64), objects: [
    { object_type: 101, name: 'Invented sword', sprite: null, bag: null, item_tier: 'UT' },
  ] }, undefined, undefined, undefined, undefined, undefined, {}, { collection, lootStats: () => ({ totals: [], whites: 0, sources: [], dungeons: [
    { dungeon: 'Invented Abbey', runs_ended: 3, clears: 2, bags: [{ bag_rank: 255, count: 4 }], whites: 4, white_runs: 2, avg_runs_per_white: 1.5, longest_dry: 1, runs_since_last_white: 0, best: null },
    { dungeon: 'Invented Burrow', runs_ended: 2, clears: 1, bags: [], whites: 0, white_runs: 0, avg_runs_per_white: null, longest_dry: 2, runs_since_last_white: null, best: null },
  ] }) }));
  const originalSettings = client.querySettings;
  const settings = vi.spyOn(client, 'querySettings').mockImplementation(async () => ({ ...await originalSettings(), loot: { chase: value.chase.map(row => row.item) } }));
  if (options.settingsError) settings.mockRejectedValueOnce(new Error('synthetic settings failure'));
  const catalog = vi.spyOn(client, 'listCatalogDungeons').mockResolvedValue(options.dungeons ?? ['Invented Abbey', 'Invented Burrow']);
  if (options.catalogError) catalog.mockRejectedValueOnce(new Error('synthetic catalog failure'));
  const read = vi.spyOn(client, 'getCollection');
  if (options.error) read.mockRejectedValueOnce(new Error('synthetic read failure'));
  if (options.busy) read.mockResolvedValue({ status: 'busy' });
  const setChaseList = vi.spyOn(client, 'setChaseList');
  const search = vi.spyOn(client, 'searchItems').mockResolvedValue([{ object_type: 104, name: 'Invented crown', tier: 'ST' }]);
  const onStateChange = vi.fn(), onRowsReady = vi.fn();
  const view = render(Loot, { target: options.container, props: { route: { kind: 'loot', view: 'collection' }, client, savedState: options.savedState,
    onStateChange, onRowsReady, entryKey: 'collection-entry' } });
  return { ...view, client, read, settings, catalog, setChaseList, search, onStateChange, onRowsReady };
}
beforeEach(() => { setDebugMode(false); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('shows the dungeon dry streak with singular run and the last observation', async () => {
  mount(); await flush();
  expect(screen.getByText('1 run since last seen')).toBeTruthy();
  expect(screen.getByRole('cell', { name: '2' })).toBeTruthy();
  expect(screen.getByRole('columnheader', { name: 'Last seen' })).toBeTruthy();
  expect(screen.getByRole('link', { name: 'Invented sword' }).getAttribute('href')).toBe('#/loot/item/101');
});
it('shows the any-dungeon dry streak', async () => {
  mount(); await flush();
  expect(screen.getByText('5 runs since last seen')).toBeTruthy();
});
it('shows the never-seen dry streak since chasing started', async () => {
  mount(); await flush();
  expect(screen.getByText('9 runs since you started chasing')).toBeTruthy();
  expect(screen.getByText('Not seen yet')).toBeTruthy();
});
it('preserves every chase row fact and unpin control', async () => {
  mount(); await flush();
  const list = within(screen.getByRole('table', { name: 'Chase list' }));
  for (const row of fixture().chase) {
    const item = list.getAllByRole('row').slice(1).find(element =>
      within(element).getByRole('link').getAttribute('href') === `#/loot/item/${row.item.item_type}`)!;
    const content = within(item);
    expect(content.getByText(row.item.dungeon ?? 'Any dungeon')).toBeTruthy();
    expect(content.getByRole('cell', { name: String(row.seen) })).toBeTruthy();
    expect(content.getByText(row.last_at === null ? 'Not seen yet' : shortWhen(row.last_at, new Date()))).toBeTruthy();
    expect(content.getByRole('button', { name: /^Stop chasing / }).getAttribute('aria-pressed')).toBe('true');
  }
});
it('sorts chase rows by dry runs descending', async () => {
  mount(); await flush();
  const rows = within(screen.getByRole('table', { name: 'Chase list' })).getAllByRole('row').slice(1);
  expect(rows.map(row => within(row).getByRole('link').getAttribute('href'))).toEqual(['#/loot/item/103', '#/loot/item/102', '#/loot/item/101']);
});
it('unpins an item then requests the collection with the remaining chase list', async () => {
  const page = mount(); await flush();
  const row = screen.getByRole('link', { name: 'Invented sword' }).closest('tr')!;
  await fireEvent.click(within(row).getByRole('button', { name: /^Stop chasing / })); await flush();
  const chase = fixture().chase.slice(1).map(row => row.item);
  expect(page.setChaseList).toHaveBeenCalledExactlyOnceWith(chase);
  expect(page.read).toHaveBeenLastCalledWith({ dungeon: 'Invented Abbey', chase });
  expect(page.read).toHaveBeenCalledTimes(2);
});
it('pins a search result with its chosen dungeon', async () => {
  const page = mount(); await flush();
  const input = screen.getByRole('combobox', { name: 'Add an item to chase' });
  await fireEvent.input(input, { target: { value: ' crown ' } });
  await fireEvent.keyDown(input, { key: 'Enter' }); await flush();
  expect(page.search).toHaveBeenCalledExactlyOnceWith('crown', 10);
  const option = screen.getByRole('option', { name: /Invented crown/ });
  const result = within(option);
  expect(option.hasAttribute('data-item-hover')).toBe(false);
  expect(option.querySelector('.tile')?.getAttribute('data-item-hover')).toBe('104');
  expect(result.getByText('Invented crown').getAttribute('data-item-hover')).toBe('104');
  const dungeon = result.getByRole('combobox', { name: 'Dungeon for Invented crown' });
  expect(dungeon.closest('[data-item-hover]')).toBeNull();
  dungeon.focus(); await flush();
  expect(option.hasAttribute('aria-describedby')).toBe(false);
  await fireEvent.change(result.getByRole('combobox'), { target: { value: 'Invented Abbey' } });
  await fireEvent.click(result.getByRole('button', { name: /^Chase / })); await flush();
  expect(page.setChaseList).toHaveBeenLastCalledWith([...fixture().chase.map(row => row.item),
    { item_type: 104, dungeon: 'Invented Abbey', pinned_at: (BigInt(Date.now()) * 1000n).toString() }]);
  expect(page.read.mock.lastCall![0].chase.at(-1)?.dungeon).toBe('Invented Abbey');
});
it('saves dungeon selection and restores it on the first read', async () => {
  const page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Invented Abbey' })); await flush();
  expect(page.read.mock.lastCall![0].dungeon).toBe('Invented Abbey');
  const patch = page.onStateChange.mock.lastCall![0];
  expect(patch.lootPosition.collectionDungeon).toBe('Invented Abbey');
  page.unmount();
  const restored = mount({ savedState: { ...emptyListState(), ...patch } }); await flush();
  expect(restored.read.mock.calls[0][0].dungeon).toBe('Invented Abbey');
});
it('keeps the observed-only grid note visible with debug disabled', async () => {
  mount(); await flush();
  expect(screen.getByText('Only items Maxed has seen drop').closest('details')).toBeNull();
});
it('shows the empty chase and automatically selects the dungeon with most observed bags', async () => {
  mount({ value: { chase: [], grid_dungeon: null, grid: [] } }); await flush();
  expect(screen.getByText('Chase an item to track it')).toBeTruthy();
  expect(screen.getByText('Search above, pick a dungeon and click the star.')).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Invented Abbey' }).getAttribute('aria-pressed')).toBe('true');
});
it('shows a dungeon-specific empty grid', async () => {
  mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Invented Burrow' })); await flush();
  expect(screen.getByText('No UT or ST items seen from Invented Burrow yet')).toBeTruthy();
});
it('shows linked grid cells with catalog tiers and observed counts', async () => {
  mount({ value: { chase: [], grid_dungeon: null, grid: [{ item_type: 101, tier: 'UT', count: 4, last_at: '2000000' }] } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Invented Abbey' })); await flush();
  const grid = within(screen.getByRole('list', { name: 'Observed items' }));
  expect(grid.getByText('Invented sword').classList.contains('ut')).toBe(true);
  expect(grid.getByText('×4')).toBeTruthy();
  expect(grid.getByRole('link').getAttribute('href')).toBe('#/loot/item/101');
});
it('shows a retryable collection error instead of empty results', async () => {
  const page = mount({ error: true }); await flush();
  expect(screen.getByText("Couldn't load collection.")).toBeTruthy();
  expect(screen.queryByText('Chase an item to track it')).toBeNull();
  expect(screen.queryByRole('heading', { name: 'Choose a dungeon' })).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
  expect(page.read).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('cell', { name: '2' })).toBeTruthy();
});
it('waits for settings and lets a settings failure be retried without reading an empty chase', async () => {
  const page = mount({ settingsError: true }); await flush();
  expect(screen.getByText("Couldn't load the chase list")).toBeTruthy();
  expect(page.read).not.toHaveBeenCalled();
  expect(screen.queryByText('Chase an item to track it')).toBeNull();
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('collection-entry');
  await fireEvent.click(screen.getByRole('button', { name: 'Retry chase list' })); await flush();
  expect(page.read.mock.calls[0][0].chase).toEqual(fixture().chase.map(row => row.item));
});
it('shows a retryable dungeon catalog failure', async () => {
  const page = mount({ catalogError: true }); await flush();
  expect(screen.getByText('Dungeon choices unavailable.')).toBeTruthy();
  expect(screen.queryByRole('heading', { name: 'Choose a dungeon' })).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry dungeon choices' })); await flush();
  expect(page.catalog).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('navigation', { name: 'Collection dungeon' })).toBeTruthy();
});
it('rejects invalid searches and ignores replies after the query changes', async () => {
  const page = mount(); await flush();
  const input = screen.getByRole('combobox', { name: 'Add an item to chase' });
  for (const value of [' ', 'a'.repeat(65)]) {
    await fireEvent.input(input, { target: { value } }); await fireEvent.keyDown(input, { key: 'Enter' }); await flush();
    expect(screen.getByText('Enter 1–64 characters to search.')).toBeTruthy();
  }
  expect(page.search).not.toHaveBeenCalled();
  const pending = deferred<Awaited<ReturnType<typeof page.client.searchItems>>>();
  page.search.mockReturnValueOnce(pending.promise);
  await fireEvent.input(input, { target: { value: 'old' } }); await fireEvent.keyDown(input, { key: 'Enter' }); await flush();
  await fireEvent.input(input, { target: { value: 'new' } });
  pending.resolve([{ object_type: 104, name: 'Stale crown', tier: 'ST' }]); await flush();
  expect(screen.queryByText('Stale crown')).toBeNull();
});
it('releases rows-ready for exhausted busy retries and offers Retry', async () => {
  const page = mount({ busy: true }); await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(screen.getByText('Loot history busy')).toBeTruthy();
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('collection-entry');
  expect(screen.queryByText('Chase an item to track it')).toBeNull();
});
it('releases rows-ready without a client even when disabled', async () => {
  const onRowsReady = vi.fn();
  render(Loot, { route: { kind: 'loot', view: 'collection' }, enabled: false, entryKey: 'offline', onRowsReady }); await flush();
  expect(screen.getByRole('heading', { name: 'Loot unavailable' })).toBeTruthy();
  expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('offline');
});
it('does not read or release rows-ready while disabled with a client', async () => {
  const page = mount();
  await page.rerender({ enabled: false }); await flush();
  expect(page.read).not.toHaveBeenCalled();
  expect(page.onRowsReady).not.toHaveBeenCalled();
  await page.rerender({ enabled: true }); await flush();
  expect(page.read).toHaveBeenCalledTimes(1);
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('collection-entry');
});
it('waits for the collection read before releasing rows-ready', async () => {
  const page = mount();
  const pending = deferred<Awaited<ReturnType<typeof page.client.getCollection>>>();
  page.read.mockReturnValueOnce(pending.promise); await flush();
  expect(screen.getByRole('heading', { name: 'Loading collection' })).toBeTruthy();
  expect(page.onRowsReady).not.toHaveBeenCalled();
  pending.resolve({ status: 'ready', value: fixture() }); await flush();
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('collection-entry');
});
it('does not read again when numeric store progress changes without a loot invalidation', async () => {
  const page = mount(); await flush();
  for (const queued of [1, 2, 3]) {
    await page.rerender({ store: { state: 'ready', queued, durable: String(queued), stream_id: null, message: '' } }); await flush();
  }
  expect(page.read).toHaveBeenCalledTimes(1);
});
it('keeps the storage failure warning visible above retained observations', async () => {
  const page = mount(); await flush();
  await page.rerender({ store: { state: 'failed', queued: 1, durable: '0', stream_id: null, message: '' } }); await flush();
  expect(screen.getByText(/^Saving failed\. Saved history may be stale/).tagName).toBe('P');
  expect(screen.getByRole('table', { name: 'Chase list' })).toBeTruthy();
});
it('hides retained empty states after a failed refresh', async () => {
  const page = mount({ value: { chase: [], grid_dungeon: null, grid: [] } }); await flush();
  expect(screen.getByText('Chase an item to track it')).toBeTruthy();
  expect(screen.getByText('Search above, pick a dungeon and click the star.')).toBeTruthy();
  page.read.mockRejectedValueOnce(new Error('synthetic refresh failure'));
  await fireEvent.click(screen.getByRole('button', { name: 'Invented Burrow' })); await flush();
  expect(screen.getByText("Couldn't load collection.")).toBeTruthy();
  expect(screen.queryByText('Chase an item to track it')).toBeNull();
  expect(screen.queryByRole('heading', { name: 'Choose a dungeon' })).toBeNull();
});
it('serializes pin saves across chase rows and search results using the updated list', async () => {
  const page = mount(); await flush();
  const input = screen.getByRole('combobox', { name: 'Add an item to chase' });
  await fireEvent.input(input, { target: { value: 'crown' } });
  await fireEvent.keyDown(input, { key: 'Enter' }); await flush();
  const pending = deferred<void>();
  page.setChaseList.mockReturnValueOnce(pending.promise);
  const row = screen.getByRole('link', { name: 'Invented sword' }).closest('tr')!;
  await fireEvent.click(within(row).getByRole('button', { name: /^Stop chasing / })); await flush();
  const buttons = screen.getAllByRole<HTMLButtonElement>('button', { name: /^(Stop chasing|Chase) / });
  expect(buttons).toHaveLength(4);
  expect(buttons.every(button => button.disabled)).toBe(true);
  expect(page.setChaseList).toHaveBeenCalledTimes(1);
  pending.resolve(); await flush();
  expect(screen.getAllByRole<HTMLButtonElement>('button', { name: /^(Stop chasing|Chase) / }).every(button => !button.disabled)).toBe(true);
  await fireEvent.click(screen.getByRole('button', { name: /^Chase / })); await flush();
  expect(page.setChaseList).toHaveBeenLastCalledWith([...fixture().chase.slice(1).map(row => row.item),
    { item_type: 104, dungeon: null, pinned_at: (BigInt(Date.now()) * 1000n).toString() }]);
});
it('removes the unpinned row while the collection reread is pending', async () => {
  const page = mount(); await flush();
  const pending = deferred<Awaited<ReturnType<typeof page.client.getCollection>>>();
  page.read.mockReturnValueOnce(pending.promise);
  const row = screen.getByRole('link', { name: 'Invented sword' }).closest('tr')!;
  await fireEvent.click(within(row).getByRole('button', { name: /^Stop chasing / })); await flush();
  expect(page.read).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole('link', { name: 'Invented sword' })).toBeNull();
  expect(screen.queryByRole('button', { name: /^Chase / })).toBeNull();
  pending.resolve({ status: 'ready', value: { ...fixture(), chase: fixture().chase.slice(1) } }); await flush();
});
it('re-enables other pin buttons after a failed save without changing the chase list', async () => {
  const page = mount(); await flush();
  const pending = deferred<void>();
  page.setChaseList.mockReturnValueOnce(pending.promise);
  await fireEvent.click(screen.getAllByRole('button', { name: /^Stop chasing / })[0]); await flush();
  expect(screen.getAllByRole<HTMLButtonElement>('button', { name: /^Stop chasing / }).every(button => button.disabled)).toBe(true);
  pending.reject(new Error('synthetic save failure')); await flush();
  expect(screen.getByText("Couldn't save the chase list")).toBeTruthy();
  expect(screen.getAllByRole<HTMLButtonElement>('button', { name: /^Stop chasing / }).every(button => !button.disabled)).toBe(true);
  expect(page.read).toHaveBeenCalledTimes(1);
});
it('filters retained chase rows against settings by both item type and dungeon', async () => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const settings = await client.querySettings();
  const value = fixture();
  value.chase.push({ ...value.chase[0], item: { ...value.chase[0].item, dungeon: null } });
  settings.loot.chase = value.chase.map(row => row.item);
  const view = render(CollectionChaseList, { client, settings, rows: value.chase, visuals: emptyHomeVisuals(), onSaved: vi.fn() }); await flush();
  expect(screen.getAllByRole('link', { name: '#101' })).toHaveLength(2);
  await view.rerender({ settings: { ...settings, loot: { chase: settings.loot.chase.slice(1) } } }); await flush();
  const remaining = screen.getAllByRole('link', { name: '#101' });
  expect(remaining).toHaveLength(1);
  expect(within(remaining[0].closest('tr')!).getByText('Any dungeon')).toBeTruthy();
  expect(screen.queryByRole('button', { name: /^Chase / })).toBeNull();
});
it('keeps a completed pin after its search result is cleared during the save', async () => {
  const page = mount(); await flush();
  const input = screen.getByRole('combobox', { name: 'Add an item to chase' });
  await fireEvent.input(input, { target: { value: 'crown' } });
  await fireEvent.keyDown(input, { key: 'Enter' }); await flush();
  const pending = deferred<void>();
  page.setChaseList.mockReturnValueOnce(pending.promise);
  await fireEvent.click(screen.getByRole('button', { name: /^Chase / })); await flush();
  const saved = page.setChaseList.mock.calls[0][0];
  expect(saved.at(-1)?.item_type).toBe(104);
  await fireEvent.input(input, { target: { value: 'sword' } }); await flush();
  expect(screen.queryByRole('listbox', { name: 'Item results' })).toBeNull();
  expect(screen.getAllByRole<HTMLButtonElement>('button', { name: /^Stop chasing / }).every(button => button.disabled)).toBe(true);
  pending.resolve(); await flush();
  expect(page.read).toHaveBeenLastCalledWith({ dungeon: 'Invented Abbey', chase: saved });
  expect(page.read).toHaveBeenCalledTimes(2);
  const row = screen.getByRole('link', { name: 'Invented sword' }).closest('tr')!;
  const unpin = within(row).getByRole<HTMLButtonElement>('button', { name: /^Stop chasing / });
  expect(unpin.disabled).toBe(false);
  await fireEvent.click(unpin); await flush();
  expect(page.setChaseList).toHaveBeenLastCalledWith(saved.filter(item => item.item_type !== 101));
});
it('disables every search-result dungeon select while saving and restores them afterward', async () => {
  const page = mount(); await flush();
  page.search.mockResolvedValue([
    { object_type: 104, name: 'Invented crown', tier: 'ST' },
    { object_type: 105, name: 'Invented ring', tier: 'UT' },
  ]);
  const input = screen.getByRole('combobox', { name: 'Add an item to chase' });
  await fireEvent.input(input, { target: { value: 'Invented' } });
  await fireEvent.keyDown(input, { key: 'Enter' }); await flush();
  const selects = screen.getAllByRole<HTMLSelectElement>('combobox', { name: /^Dungeon for / });
  expect(selects).toHaveLength(2);
  const pending = deferred<void>();
  page.setChaseList.mockReturnValueOnce(pending.promise);
  await fireEvent.click(screen.getAllByRole('button', { name: /^Chase / })[0]); await flush();
  expect(selects.every(select => select.disabled)).toBe(true);
  pending.resolve(); await flush();
  expect(selects.every(select => !select.disabled)).toBe(true);
});

it('marks chased grid items with a gold star and saves the chosen sort', async () => {
  const value = fixture();
  value.grid = [{ item_type: 101, tier: 'UT', count: 4, last_at: '2000000' }];
  const page = mount({ value }); await flush();
  const grid = within(screen.getByRole('list', { name: 'Observed items' }));
  expect(grid.getByLabelText('Chased item').classList.contains('star')).toBe(true);
  await fireEvent.change(screen.getByRole('combobox', { name: 'Sort' }), { target: { value: 'name' } });
  expect(page.onStateChange.mock.lastCall![0].lootPosition.collectionSort).toBe('name');
  expect(page.read).toHaveBeenCalledTimes(1);
});
it('restores dungeon and sort through Back and Forward and refetches the chosen grid', async () => {
  const browser = fakeHistory('#/loot/collection'), router = createRouter(browser.adapter);
  const page = mount({ savedState: router.state.entry }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Invented Burrow' })); await flush();
  await fireEvent.change(screen.getByRole('combobox', { name: 'Sort' }), { target: { value: 'recent' } });
  router.save(page.onStateChange.mock.lastCall![0]);
  router.navigate({ kind: 'loot', view: 'feed' }); router.navigate({ kind: 'loot', view: 'collection' });
  await page.rerender({ savedState: router.state.entry }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Invented Abbey' }));
  await fireEvent.change(screen.getByRole('combobox', { name: 'Sort' }), { target: { value: 'name' } });
  router.save(page.onStateChange.mock.lastCall![0]);
  router.back(); router.back(); await page.rerender({ savedState: router.state.entry }); await flush();
  expect(page.read.mock.lastCall![0].dungeon).toBe('Invented Burrow');
  expect(screen.getByRole<HTMLSelectElement>('combobox', { name: 'Sort' }).value).toBe('recent');
  router.forward(); router.forward(); await page.rerender({ savedState: router.state.entry }); await flush();
  expect(page.read.mock.lastCall![0].dungeon).toBe('Invented Abbey');
  expect(screen.getByRole<HTMLSelectElement>('combobox', { name: 'Sort' }).value).toBe('name');
  router.dispose();
});

it('shows the backpressured-store message with retained observations', async () => {
  const page = mount(); await flush();
  await page.rerender({ store: { state: 'backpressured', queued: 1, durable: '0', stream_id: null, message: '' } }); await flush();
  expect(screen.getByText('Saving is backlogged. Saved history may be stale while observations wait to be saved.').tagName).toBe('P');
  expect(screen.getByRole('table', { name: 'Chase list' })).toBeTruthy();
});
it('shows the schema-error recovery message beside the retryable error bar', async () => {
  const page = mount();
  const message = 'History schema is newer than this Maxed build; update Maxed, then Retry.';
  page.read.mockRejectedValueOnce(new Error(message)); await flush();
  expect(screen.getByText(message).tagName).toBe('P');
  expect(screen.getByRole('alert').textContent).toContain("Couldn't load collection.");
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
  expect(screen.queryByText(message)).toBeNull();
  expect(screen.getByRole('table', { name: 'Chase list' })).toBeTruthy();
});
it('keeps the settings chase count but hides unavailable observation KPIs after an error', async () => {
  const page = mount(); await flush();
  expect(screen.getByText('2 seen at least once')).toBeTruthy();
  page.read.mockRejectedValueOnce(new Error('synthetic refresh failure'));
  await fireEvent.click(screen.getByRole('button', { name: 'Invented Burrow' })); await flush();
  expect(screen.getByText("Couldn't load collection.")).toBeTruthy();
  const kpis = page.container.querySelector('.kpis')!;
  expect(kpis.querySelector('[data-kpi="chasing"] .value')?.textContent).toBe('3');
  expect(kpis.querySelector('[data-kpi="chasing"] .sub')?.textContent?.trim()).toBe('');
  for (const key of ['dry', 'last', 'seen_here']) {
    expect(kpis.querySelector(`[data-kpi="${key}"] .value`)?.textContent).toBe('—');
    expect(kpis.querySelector(`[data-kpi="${key}"] .sub`)?.textContent?.trim()).toBe('');
  }
});
it('colours the last-seen KPI without turning its label into a tier tooltip', async () => {
  const value = fixture(); value.chase = [value.chase[0]];
  const page = mount({ value }); await flush();
  expect(page.container.querySelector('.kpis')?.getAttribute('data-last-tier')).toBe('UT');
  expect(page.container.querySelector('[data-kpi="last"] button')).toBeNull();
});
it('omits an unknown dungeon from the named dry-streak KPI', async () => {
  const value = fixture(); value.chase = [{ ...value.chase[0], item: { ...value.chase[0].item, dungeon: null } }];
  const page = mount({ value }); await flush();
  expect(page.container.querySelector('[data-kpi="dry"] .sub')?.textContent).toBe('runs · Invented sword');
  expect(page.container.querySelector('[data-kpi="last"] .sub')?.textContent).not.toContain(' · ');
});
it('preserves and saves page scroll for auto-selection, dungeon clicks and sort changes', async () => {
  const main = document.createElement('main'); document.body.append(main); main.scrollTop = 120;
  const page = mount({ container: main }); await flush();
  expect(main.scrollTop).toBe(120);
  expect(page.onStateChange.mock.lastCall![0].scrollTop).toBe(120);
  main.scrollTop = 340;
  await fireEvent.click(screen.getByRole('button', { name: 'Invented Burrow' })); await flush();
  expect(main.scrollTop).toBe(340);
  expect(page.onStateChange.mock.lastCall![0].scrollTop).toBe(340);
  main.scrollTop = 260;
  await fireEvent.change(screen.getByRole('combobox', { name: 'Sort' }), { target: { value: 'name' } }); await flush();
  expect(main.scrollTop).toBe(260);
  expect(page.onStateChange.mock.lastCall![0].scrollTop).toBe(260);
});

it.each(['Enter', 'click', 'row click', 'ArrowDown then Enter'])('keeps the hash when picking a chase search result by %s', async method => {
  const page = mount(); await flush();
  const hash = window.location.hash;
  const input = screen.getByRole<HTMLInputElement>('combobox', { name: 'Add an item to chase' });
  await fireEvent.input(input, { target: { value: 'crown' } });
  await fireEvent.keyDown(input, { key: 'Enter' }); await flush();
  const result = screen.getByRole('option', { name: /Invented crown/ });
  input.focus();
  await fireEvent.keyDown(input, { key: 'ArrowDown' });
  if (method === 'Enter') { result.focus(); await fireEvent.keyDown(result, { key: 'Enter' }); }
  else if (method === 'ArrowDown then Enter') await fireEvent.keyDown(input, { key: 'Enter' });
  else if (method === 'row click') await fireEvent.click(result);
  else await fireEvent.click(within(result).getByText('Invented crown'));
  await flush();
  expect(window.location.hash).toBe(hash);
  expect(screen.getByRole('heading', { name: 'Chase list' })).toBeTruthy();
  expect(screen.getByRole('option', { name: /Invented crown/ })).toBe(result);
  expect(within(result).getByRole('combobox', { name: 'Dungeon for Invented crown' })).toBeTruthy();
  expect(within(result).getByRole('button', { name: /^Chase / })).toBeTruthy();
  expect(input.value).toBe('crown');
  expect(input.getAttribute('aria-activedescendant')).toBe(result.id);
  expect(result.getAttribute('aria-selected')).toBe('true');
  expect(document.activeElement).toBe(method === 'Enter' ? result : input);
  expect(page.setChaseList).not.toHaveBeenCalled();
  expect(page.search).toHaveBeenCalledTimes(1);
});

it('chases a search result with its chosen dungeon after Enter on the row', async () => {
  const page = mount(); await flush();
  const input = screen.getByRole<HTMLInputElement>('combobox', { name: 'Add an item to chase' });
  await fireEvent.input(input, { target: { value: 'crown' } });
  await fireEvent.keyDown(input, { key: 'Enter' }); await flush();
  const result = screen.getByRole('option', { name: /Invented crown/ });
  await fireEvent.keyDown(result, { key: 'Enter' });
  const dungeon = within(result).getByRole('combobox', { name: 'Dungeon for Invented crown' });
  await fireEvent.click(dungeon);
  await fireEvent.change(dungeon, { target: { value: 'Invented Burrow' } });
  await fireEvent.click(within(result).getByRole('button', { name: /^Chase / })); await flush();
  expect(page.setChaseList).toHaveBeenCalledExactlyOnceWith([...fixture().chase.map(row => row.item),
    { item_type: 104, dungeon: 'Invented Burrow', pinned_at: (BigInt(Date.now()) * 1000n).toString() }]);
  expect(screen.getByRole('option', { name: /Invented crown/ })).toBe(result);
  expect(input.value).toBe('crown');
});

it('defaults to the most observed dungeon and collapses catalog-only choices', async () => {
  const page = mount({ dungeons: ['Invented Abbey', 'Invented Burrow', 'Invented Courtyard'] });
  vi.spyOn(page.client, 'getLootStats').mockResolvedValue({ status: 'ready', value: { totals: [], whites: 0, sources: [], dungeons: [
    { dungeon: 'Invented Abbey', runs_ended: 10, clears: 0, bags: [{ bag_rank: 1, count: 1 }], whites: 0, white_runs: 0, avg_runs_per_white: null, longest_dry: 10, runs_since_last_white: null, best: null },
    { dungeon: 'Invented Burrow', runs_ended: 2, clears: 0, bags: [{ bag_rank: 1, count: 9 }], whites: 0, white_runs: 0, avg_runs_per_white: null, longest_dry: 2, runs_since_last_white: null, best: null },
  ] } });
  await flush();
  expect(page.read.mock.calls[0][0].dungeon).toBe('Invented Burrow');
  const nav = screen.getByRole('navigation', { name: 'Collection dungeon' });
  expect([...nav.querySelectorAll(':scope > button')].map(button => button.getAttribute('title'))).toEqual(['Invented Burrow', 'Invented Abbey']);
  const group = nav.querySelector('details')!;
  expect(group.open).toBe(false); expect(group.querySelector('summary')?.textContent).toBe('Not visited (1)');
  group.open = true;
  await fireEvent.click(within(nav).getByRole('button', { name: 'Invented Courtyard' })); await flush();
  expect(page.read.mock.lastCall![0].dungeon).toBe('Invented Courtyard');
});
it.each(['scopeKey', 'streamId'] as const)('clears old visit evidence when %s changes and the replacement fails', async identity => {
  const page = mount({ dungeons: ['Invented Courtyard', 'Invented Burrow', 'Invented Abbey'] });
  const visits = vi.spyOn(page.client, 'getLootStats');
  await flush();
  const nav = screen.getByRole('navigation', { name: 'Collection dungeon' });
  expect([...nav.querySelectorAll(':scope > button')].map(button => button.getAttribute('title')))
    .toEqual(['Invented Abbey', 'Invented Burrow']);
  expect(within(nav).getByText('Not visited (1)')).toBeTruthy();

  const replacement = deferred<Awaited<ReturnType<typeof page.client.getLootStats>>>();
  visits.mockReturnValueOnce(replacement.promise);
  await page.rerender({ [identity]: 'synthetic-replacement' }); await flush();
  expect(visits).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('navigation', { name: 'Collection dungeon' })).toBe(nav);
  replacement.reject(new Error('synthetic replacement history failure')); await flush();
  expect.soft(screen.queryByText('Visit history unavailable.')).not.toBeNull();
  expect.soft(screen.queryByRole('button', { name: 'Retry visit history' })).not.toBeNull();
  expect.soft(within(nav).queryByText(/^Not visited/)).toBeNull();
  expect.soft(within(nav).queryByText('All dungeons (3)')).not.toBeNull();
  expect.soft(nav.querySelectorAll(':scope > button')).toHaveLength(0);
  expect.soft([...nav.querySelectorAll('button')].map(button => button.getAttribute('title')))
    .toEqual(['Invented Courtyard', 'Invented Burrow', 'Invented Abbey']);

  visits.mockResolvedValueOnce({ status: 'ready', value: { totals: [], whites: 0, sources: [], dungeons: [
    { dungeon: 'Invented Courtyard', runs_ended: 1, clears: 0, bags: [], whites: 0, white_runs: 0, avg_runs_per_white: null, longest_dry: 1, runs_since_last_white: null, best: null },
  ] } });
  await fireEvent.click(screen.getByRole('button', { name: 'Retry visit history' })); await flush();
  expect(visits).toHaveBeenCalledTimes(3);
  expect(screen.queryByText('Visit history unavailable.')).toBeNull();
  expect([...nav.querySelectorAll(':scope > button')].map(button => button.getAttribute('title')))
    .toEqual(['Invented Courtyard']);
  expect(within(nav).getByText('Not visited (2)')).toBeTruthy();
});
it('keeps unavailable visit history distinct from not visited and offers retry', async () => {
  const page = mount();
  const visits = vi.spyOn(page.client, 'getLootStats').mockRejectedValueOnce(new Error('synthetic unavailable history'));
  await flush();
  expect(screen.getByText('Visit history unavailable.')).toBeTruthy();
  expect(screen.queryByText(/^Not visited/)).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry visit history' })); await flush();
  expect(visits).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('button', { name: 'Invented Abbey' }).getAttribute('aria-pressed')).toBe('true');
});
