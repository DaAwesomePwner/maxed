import { itemHoverParams } from '../../src/lib/components/item-hover';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Loot from '../../src/routes/Loot.svelte';
import RouteFrame from '../../src/routes/RouteFrame.svelte';
import { SHELL_CONNECTION } from '../../src/lib/shell/context';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import { createClient } from '../../src/lib/bridge/client';
import { setNativePresentationVisible } from '../../src/lib/bridge/visibility';
import { createFixtureTransport, type LootFixtureQueries } from '../../src/lib/bridge/fixture';
import type { LootDropRow } from '../../src/lib/bindings/LootDropRow';
import type { DropFilter } from '../../src/lib/bindings/DropFilter';
import type { VisualPage } from '../../src/lib/bindings/VisualPage';
import { emptyListState, type ListState } from '../../src/lib/navigation/state';
import { dateRange } from '../../src/lib/runs/filters';
import { setDebugMode } from '../../src/lib/debug/debug';
import { deferred, homeSnapshot } from '../home/fixtures';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { DropPage } from '../../src/lib/bindings/DropPage';
import { flush, story, run } from '../runs/fixtures';
import { createRouter } from '../../src/lib/navigation/history';
import { fakeHistory } from '../navigation/fake-history';
import type { Route } from '../../src/lib/navigation/routes';

const cursor = { at: '2000000', id: 'partial' };
const defaultFilter = (): DropFilter => ({ min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 });
function rows(): LootDropRow[] {
  const item = { slot: 0, item_type: 101, quantity: 1, taken_observed_at: null,
    enchant: { state: 'recorded' as const, slots: 2, applied: 2, ids: [10, 11] } };
  const base = { ...homeSnapshot().drops[0], at: '3000000', bag_type: 800, bag_rank: 255, run_id: 'synthetic-run',
    contents_known: true, source_enemy: null, source_estimated: false, source_reason: null, contents: [item], items: [101] };
  return [
    { dungeon: 'Invented Sanctum', drop: { ...base, id: 'white' } },
    { dungeon: null, drop: { ...base, id: 'partial', at: cursor.at, run_id: null, bag_rank: 3, bag_type: 801,
      contents_known: false, source_enemy: 900, source_estimated: true, source_reason: 'Nearby encounter',
      contents: [{ ...item, item_type: 102, taken_observed_at: '2500000', enchant: { state: 'missing', slots: null, applied: null, ids: [] } }], items: [102] } },
    { dungeon: null, drop: { ...base, id: 'unknown', at: '1000000', run_id: null, bag_rank: null,
      contents: [{ ...item, item_type: 65000 }], items: [65000] } },
  ];
}
const visuals: VisualPage = { client_fingerprint: 'a'.repeat(64), objects: [
  { object_type: 800, name: 'Catalog white', sprite: null, bag: { object_type: 800, name: 'Catalog white', base_type: 800, rank: 255, is_white: true, capture: true } },
  { object_type: 801, name: 'Catalog blue', sprite: null, bag: { object_type: 801, name: 'Catalog blue', base_type: 801, rank: 3, is_white: false, capture: true } },
  { object_type: 101, name: 'Invented sword', sprite: null, bag: null, item_tier: 'UT' },
  { object_type: 102, name: 'Invented ring', sprite: null, bag: null },
  { object_type: 900, name: 'Invented guardian', sprite: null, bag: null },
] };
function eventConnection() {
  const observers = new Set<(event: EventEnvelope) => void>();
  const context = new Map([[SHELL_CONNECTION, { onEvent(receive: (event: EventEnvelope) => void) {
    observers.add(receive);
    return () => { observers.delete(receive); };
  } }]]);
  return { context, emitBatch(events: EventEnvelope[]) {
    for (const event of events) for (const receive of observers) receive(event);
  } };
}
function mount(options: { listDrops?: LootFixtureQueries['listDrops']; savedState?: ListState; busy?: boolean; error?: boolean; route?: Extract<Route, { kind: 'loot' }>; summary?: LootFixtureQueries['getDropSummary'] } = {}) {
  const listDrops = vi.fn(options.listDrops ?? (() => ({ rows: rows(), next: cursor })));
  const searchItems = vi.fn(() => [{ object_type: 101, name: 'Invented sword', tier: 'UT' }]);
  const transport = createFixtureTransport(homeSnapshot(), visuals, undefined, undefined, undefined, undefined, undefined, {}, { listDrops, searchItems, getDropSummary: options.summary });
  const client = createClient(transport);
  vi.spyOn(client, 'listCatalogDungeons').mockResolvedValue(['Invented Sanctum']);
  if (options.busy) vi.spyOn(client, 'listDrops').mockResolvedValue({ status: 'busy' });
  if (options.error) vi.spyOn(client, 'listDrops').mockRejectedValue(new Error('synthetic read failure'));
  const onStateChange = vi.fn(), onRowsReady = vi.fn();
  const connection = eventConnection();
  const view = render(Loot, { context: connection.context, props: { route: options.route ?? { kind: 'loot', view: 'feed' }, client, savedState: options.savedState,
    onStateChange, onRowsReady, entryKey: 'feed-entry' } });
  return { ...view, emitBatch: connection.emitBatch, client, listDrops, searchItems, onStateChange, onRowsReady };
}
beforeEach(() => { setDebugMode(false); vi.useFakeTimers(); });
afterEach(() => { cleanup(); setNativePresentationVisible(true); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('renders the white bag and dungeon, and labels runless bags Outside a run', async () => {
  mount(); await flush();
  expect(screen.getAllByText('Catalog white').length).toBeGreaterThan(0);
  expect(within(screen.getByRole('table', { name: 'Bags' })).getByText('Invented Sanctum')).toBeTruthy();
  expect(within(screen.getByRole('table', { name: 'Bags' })).getAllByText('Outside a run')).toHaveLength(2);
});
it('keeps partial contents and estimated source visible with debug mode off', async () => {
  mount(); await flush();
  expect(screen.getByText('partial')).toBeTruthy();
  await fireEvent.click(document.querySelector('[data-row-id="drop:partial"]')!); await flush();
  expect(screen.getAllByText('est.').length).toBeGreaterThan(0);
  expect(screen.getByText(/Taken/)).toBeTruthy();
  expect(screen.getByText(/Enchant unknown/)).toBeTruthy();
});
it('renders missing catalog types as visible numeric item links', async () => {
  mount(); await flush();
  expect(screen.getByText('#65000')).toBeTruthy();
  expect(screen.getByRole('link', { name: /#65000/ }).getAttribute('href')).toBe('#/loot/item/65000');
});
it('links item tiles and includes a Run link only for a bag inside a run', async () => {
  mount(); await flush();
  expect(screen.getAllByRole('link', { name: /Invented sword/ })[0].getAttribute('href')).toBe('#/loot/item/101');
  expect(screen.getByRole('link', { name: /Invented ring/ }).getAttribute('href')).toBe('#/loot/item/102');
  expect(screen.getAllByRole('link', { name: 'Run ›' })).toHaveLength(1);
  expect(screen.getByRole('link', { name: 'Run ›' }).getAttribute('href')).toBe('#/runs/synthetic-run');
});
it('pages Next with the returned cursor and Previous restores the first page', async () => {
  const page = mount(); await flush();
  const next = screen.getByRole('button', { name: 'Older bags' });
  expect(next.getAttribute('aria-disabled')).toBe('false');
  next.focus(); await fireEvent.click(next); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), cursor });
  expect(document.activeElement).toBe(next);
  await fireEvent.click(screen.getByRole('button', { name: 'Newer bags' })); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith(defaultFilter());
});
it('applying Enchanted only resets the cursor and cursor history', async () => {
  const page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Older bags' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' }));
  await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), enchanted_only: true });
  expect(screen.getByRole('button', { name: 'Newer bags' }).getAttribute('aria-disabled')).toBe('true');
});
it('offers Retry after busy and shows an error instead of an empty feed after a read failure', async () => {
  const busy = mount({ busy: true }); await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
  expect(screen.queryByText('No bags yet')).toBeNull();
  busy.unmount();
  const failure = mount({ error: true }); await flush();
  expect(within(screen.getByRole('alert')).getByText(/^Couldn't load bags/)).toBeTruthy();
  expect(screen.getAllByText(/Couldn't load bags/)).toHaveLength(1);
  expect(screen.queryByRole('heading', { name: "Couldn't load bags" })).toBeNull();
  expect(screen.queryByText('No bags yet')).toBeNull();
  vi.mocked(failure.client.listDrops).mockResolvedValue({ status: 'ready', value: { rows: [], next: null } });
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
  expect(screen.getByText('No bags yet')).toBeTruthy();
  expect(screen.getByText('Capture a run to see its bags here.')).toBeTruthy();
});
it('saves the applied filter in lootPosition.feed and restores it on return', async () => {
  const page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' }));
  await flush();
  const patch = page.onStateChange.mock.lastCall![0];
  expect(patch.lootPosition?.feed).toEqual({ filter: { ...defaultFilter(), enchanted_only: true }, previous: [], selectedDropId: 'white' });
  page.unmount();
  const restored = mount({ savedState: { ...emptyListState(), ...patch } }); await flush();
  expect(restored.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), enchanted_only: true });
  expect(screen.getByRole('button', { name: 'Enchanted only' }).getAttribute('aria-pressed')).toBe('true');
});
it('selects search results by keyboard, applies dates and place, and clears filters', async () => {
  const page = mount(); await flush();
  const search = screen.getByRole('combobox', { name: 'Find an item' });
  await fireEvent.input(search, { target: { value: 'sword' } });
  await fireEvent.keyDown(search, { key: 'Enter' }); await flush();
  expect(page.searchItems).toHaveBeenCalledWith('sword', 10);
  expect(screen.getByRole('listbox', { name: 'Item results' })).toBeTruthy();
  await fireEvent.keyDown(search, { key: 'ArrowDown' });
  await fireEvent.keyDown(search, { key: 'Enter' }); await flush();
  expect(screen.getByRole('button', { name: 'Clear item' })).toBeTruthy();
  await fireEvent.change(screen.getByLabelText('Place'), { target: { value: 'outside' } });
  await fireEvent.change(screen.getByLabelText('Minimum bag tier'), { target: { value: '255' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Custom…' }));
  await fireEvent.input(screen.getByLabelText('From'), { target: { value: '2026-03-08' } });
  await fireEvent.input(screen.getByLabelText('Through'), { target: { value: '2026-03-08' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Use dates' })); await flush();
  await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), item_type: 101, place: { kind: 'outside' }, min_rank: 255, range: dateRange('2026-03-08', '2026-03-08') });
  await fireEvent.click(screen.getByRole('button', { name: 'Clear item' }));
  await fireEvent.change(screen.getByLabelText('Place'), { target: { value: '' } });
  await fireEvent.change(screen.getByLabelText('Minimum bag tier'), { target: { value: '' } }); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), range: dateRange('2026-03-08', '2026-03-08') });
  await fireEvent.click(screen.getByRole('button', { name: 'All time' })); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith(defaultFilter());
});
it('distinguishes unseen contents, empty bags and slot order', async () => {
  const sample = rows();
  sample[0].drop.contents = [sample[0].drop.contents[0], { ...sample[0].drop.contents[0], slot: 2, item_type: 102 }].reverse();
  sample[1].drop.contents = []; sample[2].drop.contents = [];
  mount({ listDrops: () => ({ rows: sample, next: null }) }); await flush();
  expect(screen.getByText('contents not seen')).toBeTruthy();
  expect(screen.getByText('Empty bag')).toBeTruthy();
  const links = within(screen.getByRole('table', { name: 'Bags' })).getAllByRole('link').filter(link => link.getAttribute('href')?.startsWith('#/loot/item/'));
  expect(links.map(link => link.getAttribute('href'))).toEqual(['#/loot/item/101', '#/loot/item/102']);
});
it('distinguishes filtered and unfiltered empty results and releases rows once per entry', async () => {
  const page = mount({ listDrops: () => ({ rows: [], next: null }) }); await flush();
  expect(screen.getByText('No bags yet')).toBeTruthy();
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('feed-entry');
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' }));
  await flush();
  expect(screen.getByText('No bags match these filters')).toBeTruthy();
  expect(screen.getByText('Try a longer range or another bag tier.')).toBeTruthy();
  expect(page.onRowsReady).toHaveBeenCalledTimes(1);
});

it('waits for loaded rows before releasing the entry and restores a saved cursor stack', async () => {
  const savedState: ListState = { ...emptyListState(), scrollTop: 87, focusedRowId: 'drop:white:item:0:101', lootPosition: {
    range: null,
    feed: { filter: { ...defaultFilter(), cursor }, previous: [null] },
    item: { type: 101, previous: [null] }, statsRange: { first: '2026-03-08', last: '2026-03-08' }, collectionDungeon: 'Invented Sanctum',
  } };
  const page = mount({ savedState });
  const pending = deferred<HistoryReply<DropPage>>();
  vi.spyOn(page.client, 'listDrops').mockReturnValueOnce(pending.promise);
  await flush();
  expect(screen.getByRole('heading', { name: 'Loading bags' })).toBeTruthy();
  expect(page.onRowsReady).not.toHaveBeenCalled();
  expect(page.onStateChange).not.toHaveBeenCalled();
  pending.resolve({ status: 'ready', value: { rows: rows(), next: null } }); await flush();
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('feed-entry');
  expect(page.client.listDrops).toHaveBeenCalledWith({ ...defaultFilter(), cursor });
  expect(screen.getByRole('button', { name: 'Newer bags' }).getAttribute('aria-disabled')).toBe('false');
  await fireEvent.click(screen.getByRole('button', { name: 'Latest' })); await flush();
  expect(page.client.listDrops).toHaveBeenLastCalledWith(defaultFilter());
  expect(page.onStateChange.mock.lastCall![0].lootPosition).toEqual({ ...savedState.lootPosition, feed: { filter: defaultFilter(), previous: [], selectedDropId: 'white' } });
  expect(page.onRowsReady).toHaveBeenCalledTimes(1);
});

it('rejects invalid search and dates without issuing history or catalog searches', async () => {
  const page = mount(); await flush();
  const search = screen.getByRole('combobox', { name: 'Find an item' });
  for (const value of ['   ', 'a'.repeat(65)]) {
    await fireEvent.input(search, { target: { value } });
    await fireEvent.keyDown(search, { key: 'Enter' }); await flush();
    expect(screen.getByText('Enter 1–64 characters to search')).toBeTruthy();
  }
  expect(page.searchItems).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: 'Custom…' }));
  await fireEvent.input(screen.getByLabelText('From'), { target: { value: '2026-03-09' } });
  await fireEvent.input(screen.getByLabelText('Through'), { target: { value: '2026-03-08' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Use dates' })); await flush();
  expect(screen.getByRole('alert').textContent).toBe('Invalid Date Range');
  expect(page.listDrops).toHaveBeenCalledTimes(1);
});

it('reports no client as unavailable and releases its entry once', async () => {
  const onRowsReady = vi.fn();
  render(Loot, { route: { kind: 'loot', view: 'feed' }, entryKey: 'unavailable', onRowsReady }); await flush();
  expect(screen.getByRole('heading', { name: 'Loot unavailable' })).toBeTruthy();
  expect(screen.queryByText('No bags yet')).toBeNull();
  expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('unavailable');
});

it('keeps discovered bag tiers available after a filter returns no bags', async () => {
  const page = mount(); await flush();
  page.listDrops.mockReturnValue({ rows: [], next: null });
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' }));
  await flush();
  expect(screen.getByText('No bags match these filters')).toBeTruthy();
  expect(within(screen.getByLabelText('Minimum bag tier')).getByRole('option', { name: 'Catalog white' }).getAttribute('value')).toBe('255');
});

it('shows an item search rejection visibly', async () => {
  const page = mount(); await flush();
  vi.spyOn(page.client, 'searchItems').mockRejectedValue(new Error('synthetic failure'));
  await fireEvent.input(screen.getByRole('combobox', { name: 'Find an item' }), { target: { value: 'sword' } });
  await fireEvent.keyDown(screen.getByRole('combobox', { name: 'Find an item' }), { key: 'Enter' }); await flush();
  expect(screen.getByText('Item search unavailable. Press Enter to retry.')).toBeTruthy();
});
it('shows No matching items for an empty search', async () => {
  const page = mount(); await flush();
  vi.spyOn(page.client, 'searchItems').mockResolvedValue([]);
  await fireEvent.input(screen.getByRole('combobox', { name: 'Find an item' }), { target: { value: 'absent' } });
  await fireEvent.keyDown(screen.getByRole('combobox', { name: 'Find an item' }), { key: 'Enter' }); await flush();
  expect(screen.getByText('No matching items')).toBeTruthy();
});
it('discards a stale search reply', async () => {
  const page = mount(); await flush();
  const pending = deferred<Awaited<ReturnType<typeof page.client.searchItems>>>();
  vi.spyOn(page.client, 'searchItems').mockReturnValueOnce(pending.promise).mockResolvedValue([]);
  const search = screen.getByRole('combobox', { name: 'Find an item' });
  await fireEvent.input(search, { target: { value: 'sword' } });
  await fireEvent.keyDown(search, { key: 'Enter' }); await flush();
  await fireEvent.input(search, { target: { value: 'absent' } });
  await fireEvent.keyDown(search, { key: 'Enter' }); await flush();
  pending.resolve([{ object_type: 101, name: 'Stale sword', tier: null }]); await flush();
  expect(screen.queryByText('Stale sword')).toBeNull();
  expect(screen.getByText('No matching items')).toBeTruthy();
  expect(search.hasAttribute('aria-controls')).toBe(false);
});
it('sends a dungeon place filter', async () => {
  const page = mount(); await flush();
  await fireEvent.change(screen.getByLabelText('Place'), { target: { value: 'dungeon:Invented Sanctum' } });
  await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), place: { kind: 'dungeon', name: 'Invented Sanctum' } });
});
it('clears the item before applying', async () => {
  const page = mount(); await flush();
  const search = screen.getByRole('combobox', { name: 'Find an item' });
  await fireEvent.input(search, { target: { value: 'sword' } });
  await fireEvent.keyDown(search, { key: 'Enter' }); await flush();
  expect(search.getAttribute('aria-controls')).toBe(screen.getByRole('listbox').id);
  await fireEvent.click(screen.getByRole('option', { name: /Invented sword/ }));
  await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Clear item' }));
  await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), item_type: null });
});
it('labels a card with no source Unknown source', async () => {
  mount(); await flush();
  expect(within(screen.getByRole('complementary', { name: 'Selected bag' })).getByText('Unknown source')).toBeTruthy();
});
it('offers White bag before any bags have loaded', async () => {
  mount({ listDrops: () => ({ rows: [], next: null }) }); await flush();
  expect(screen.getByRole('option', { name: 'White bag' }).getAttribute('value')).toBe('255');
});
it('keeps no-client controls unavailable even when disabled', async () => {
  const onRowsReady = vi.fn();
  render(Loot, { route: { kind: 'loot', view: 'feed' }, enabled: false, onRowsReady, entryKey: 'no-client' }); await flush();
  expect(screen.queryByRole('combobox', { name: 'Find an item' })).toBeNull();
  expect(screen.getByRole('heading', { name: 'Loot unavailable' })).toBeTruthy();
  expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('no-client');
});

it('keeps the retry prompt when item search has a client but is disabled', async () => {
  const page = mount(); await page.rerender({ enabled: false }); await flush();
  const search = screen.getByRole('combobox', { name: 'Find an item' });
  await fireEvent.input(search, { target: { value: 'sword' } });
  await fireEvent.keyDown(search, { key: 'Enter' }); await flush();
  expect(screen.getByText('Item search unavailable. Press Enter to retry.')).toBeTruthy();
  expect(page.searchItems).not.toHaveBeenCalled();
});

it('resets the cursor stack when the shared header range changes', async () => {
  const page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Older bags' })); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), cursor });
  await fireEvent.click(screen.getByRole('button', { name: 'Today' })); await flush();
  const patch = page.onStateChange.mock.lastCall![0];
  expect(patch.lootPosition?.range).not.toBeNull();
  expect(patch.lootPosition?.feed.previous).toEqual([]);
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), range: patch.lootPosition?.range });
  expect(screen.getByRole('button', { name: 'Newer bags' }).getAttribute('aria-disabled')).toBe('true');
});

it('discards a saved cursor when its feed range differs from the shared range', async () => {
  const range = dateRange('2026-03-08', '2026-03-08');
  const savedState: ListState = { ...emptyListState(), lootPosition: {
    range, feed: { filter: { ...defaultFilter(), cursor }, previous: [null] },
    item: null, statsRange: null, collectionDungeon: null,
  } };
  const page = mount({ savedState }); await flush();
  expect(page.listDrops).toHaveBeenCalledExactlyOnceWith({ ...defaultFilter(), range });
  expect(page.onStateChange.mock.lastCall![0].lootPosition?.feed.previous).toEqual([]);
});

it('renders 50 dense rows and backend day counts rather than page counts', async () => {
  const sample = Array.from({ length: 50 }, (_, i) => ({ ...rows()[0], drop: { ...rows()[0].drop, id: `bag-${i}` } }));
  const totals = { bags: 120, runs: 40, runs_ended: 41, whites: 3, ut_items: 22, st_items: 9, enchanted_items: 11, items_counted: true, best_enchants: 3, last_white: null, first_at: sample[0].drop.at };
  const page = mount({ listDrops: () => ({ rows: sample, next: null }), summary: request => ({ totals, days: request.days.map(() => totals) }) }); await flush();
  expect(within(screen.getByRole('table', { name: 'Bags' })).getAllByRole('row').filter(row => row.hasAttribute('data-row-id'))).toHaveLength(50);
  expect(screen.queryByRole('article')).toBeNull(); expect(screen.getByText(/120 bags · 3 whites · 22 UT/)).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  expect(document.querySelector('[data-kpi="rate"]')?.textContent).toContain('7.3%');
  const summary = vi.spyOn(page.client, 'getDropSummary');
  await fireEvent.change(screen.getByLabelText('Minimum bag tier'), { target: { value: '255' } }); await flush();
  expect(screen.queryByRole('button', { name: 'Apply' })).toBeNull();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), min_rank: 255 });
  expect(summary).toHaveBeenCalledWith(expect.objectContaining({ filter: expect.objectContaining({ min_rank: 255 }), include_items: true }));
});
it('selects rows by click and keyboard and shows every item in the panel', async () => {
  const sample = rows(); sample[1].drop.contents = Array.from({ length: 8 }, (_, slot) => ({ ...sample[1].drop.contents[0], slot, item_type: 100 + slot }));
  mount({ listDrops: () => ({ rows: sample, next: null }) }); await flush();
  expect(document.querySelector('[data-row-id="drop:white"]')?.getAttribute('aria-current')).toBe('true');
  await fireEvent.click(document.querySelector('[data-row-id="drop:partial"]')!); await flush();
  const panel = within(screen.getByRole('complementary', { name: 'Selected bag' }));
  expect(panel.getAllByRole('link')).toHaveLength(8);
  expect(panel.getAllByRole('link').map(link => link.getAttribute('href'))).toEqual(Array.from({ length: 8 }, (_, i) => `#/loot/item/${100 + i}`));
  const taken = panel.getAllByRole('listitem').filter(item => itemHoverParams(item)?.note === 'Removal observed');
  expect(taken).toHaveLength(8);
  expect(taken.every(item => item.classList.contains('taken'))).toBe(true);
  expect(taken.every(item => item.hasAttribute('data-item-hover') && !item.hasAttribute('title'))).toBe(true);
  await fireEvent.keyDown(document.querySelector('[data-row-id="drop:unknown"]')!, { key: ' ' }); await flush();
  expect(document.querySelector('[data-row-id="drop:unknown"]')?.getAttribute('aria-current')).toBe('true');
});
it('loads other bags from the selected run and hides them for a runless bag or failed story', async () => {
  const page = mount(); const getRun = vi.spyOn(page.client, 'getRun').mockResolvedValue({ status: 'ready', value: story({ run: run({ id: 'synthetic-run' }), drops: [rows()[0].drop, { ...rows()[0].drop, id: 'other-bag' }] }) });
  await flush(); expect(screen.getByRole('heading', { name: 'Other bags this run' })).toBeTruthy();
  expect(document.querySelector('[data-drop-id="other-bag"]')).toBeTruthy();
  expect(document.querySelector('[data-drop-id="white"]')).toBeNull();
  await fireEvent.click(document.querySelector('[data-row-id="drop:partial"]')!); await flush();
  expect(screen.queryByRole('heading', { name: 'Other bags this run' })).toBeNull();
  getRun.mockRejectedValue(new Error('Synthetic failure'));
  await fireEvent.click(document.querySelector('[data-row-id="drop:white"]')!); await flush();
  expect(screen.queryByRole('heading', { name: 'Other bags this run' })).toBeNull();
});
it('applies a seeded place entry and restores the unseeded filter, page and selection on Back/Forward', async () => {
  const adapter = fakeHistory('#/loot'), router = createRouter(adapter.adapter);
  let page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Older bags' })); await flush();
  await fireEvent.click(document.querySelector('[data-row-id="drop:partial"]')!); await flush();
  router.save(page.onStateChange.mock.lastCall![0]);
  router.navigate({ kind: 'loot', view: 'feed', seed: { place: 'Lost Halls' } });
  page.unmount(); page = mount({ route: router.state.entry.route as Extract<Route, { kind: 'loot' }>, savedState: router.state.entry }); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), place: { kind: 'dungeon', name: 'Lost Halls' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Older bags' })); await flush();
  await fireEvent.click(document.querySelector('[data-row-id="drop:unknown"]')!); await flush();
  router.save(page.onStateChange.mock.lastCall![0]); router.back(); page.unmount();
  page = mount({ savedState: router.state.entry }); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), enchanted_only: true, cursor });
  expect(document.querySelector('[data-row-id="drop:partial"]')?.getAttribute('aria-current')).toBe('true');
  router.forward(); page.unmount(); page = mount({ route: router.state.entry.route as Extract<Route, { kind: 'loot' }>, savedState: router.state.entry }); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), place: { kind: 'dungeon', name: 'Lost Halls' }, cursor });
  expect(document.querySelector('[data-row-id="drop:unknown"]')?.getAttribute('aria-current')).toBe('true');
  router.back(); page.unmount(); page = mount({ savedState: router.state.entry }); await flush();
  expect(document.querySelector('[data-row-id="drop:partial"]')?.getAttribute('aria-current')).toBe('true');
  router.dispose();
});
it('requests at most eight day summaries and leaves later headers without counts', async () => {
  const sample = Array.from({ length: 10 }, (_, i) => ({ ...rows()[0], drop: { ...rows()[0].drop, id: `day-${i}`, at: String(new Date(2026, 8, 30 - i, 12).getTime() * 1000) } }));
  const totals = { bags: 100, runs: 20, runs_ended: 20, whites: 1, items_counted: false, best_enchants: null, last_white: null, first_at: sample.at(-1)!.drop.at };
  const summary = vi.fn<NonNullable<LootFixtureQueries['getDropSummary']>>(request => ({ totals, days: request.days.map(() => totals) }));
  mount({ listDrops: () => ({ rows: sample, next: null }), summary }); await flush();
  expect(summary.mock.calls.filter(([request]) => request.include_items).at(-1)![0].days).toHaveLength(8);
  const headings = document.querySelectorAll('tr.group'); expect(headings).toHaveLength(10);
  expect(headings[7].textContent).toContain('100 bags'); expect(headings[8].textContent).not.toContain('bags');
});

it('clears the shared date range with empty-result filters', async () => {
  const page = mount({ listDrops: () => ({ rows: [], next: null }) }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Today' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Clear filters' })); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith(defaultFilter());
  expect(screen.getByRole('button', { name: 'All time' }).getAttribute('aria-pressed')).toBe('true');
});
it('retains the last good totals while the next page is loading', async () => {
  const totals = { bags: 120, runs: 40, runs_ended: 41, whites: 3, ut_items: 22, st_items: 9, enchanted_items: 11, items_counted: true, best_enchants: 3, last_white: null, first_at: null };
  const page = mount({ summary: request => ({ totals, days: request.days.map(() => totals) }) }); await flush();
  const pending = deferred<HistoryReply<DropPage>>(); vi.spyOn(page.client, 'listDrops').mockReturnValueOnce(pending.promise);
  await fireEvent.click(screen.getByRole('button', { name: 'Older bags' })); await flush();
  expect(document.querySelector('[data-kpi="bags"] .value')?.textContent).toBe('120');
  expect(screen.getByRole('heading', { name: 'Loading bags' })).toBeTruthy();
  pending.resolve({ status: 'ready', value: { rows: rows(), next: null } }); await flush();
});

const reviewTotals = { bags: 120, runs: 40, runs_ended: 41, whites: 3, ut_items: 22, st_items: 9,
  enchanted_items: 11, items_counted: true, best_enchants: 3, last_white: null, first_at: null };
it('keeps the selected run card and its scroll through another run watermark refresh', async () => {
  const page = mount();
  const getRun = vi.spyOn(page.client, 'getRun').mockResolvedValue({ status: 'ready', value: story({
    run: run({ id: 'synthetic-run' }), drops: [rows()[0].drop, { ...rows()[0].drop, id: 'other-bag' }],
  }) });
  const streamId = homeSnapshot().stream_id;
  const store = { state: 'ready' as const, queued: 0, durable: '2', stream_id: streamId, message: '' };
  await page.rerender({ streamId, required: '1', store }); await flush();
  expect(getRun).toHaveBeenCalledTimes(1);
  const card = screen.getByRole('heading', { name: 'Other bags this run' });
  const scroll = screen.getByRole('region', { name: 'All bags' }); scroll.scrollTop = 31;
  const pending = deferred<HistoryReply<DropPage>>();
  const drops = vi.spyOn(page.client, 'listDrops').mockReturnValueOnce(pending.promise);
  page.emitBatch([{ stream_id: streamId, sequence: '2', at: '3000000', event: { bag_dropped: { ...rows()[0].drop, run_id: 'another-run' } } }]);
  await page.rerender({ streamId, store, required: '2' }); await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(drops).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('heading', { name: 'Other bags this run' })).toBe(card);
  expect(getRun).toHaveBeenCalledTimes(1);
  pending.resolve({ status: 'ready', value: { rows: structuredClone(rows()), next: cursor } }); await flush();
  expect(getRun).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('heading', { name: 'Other bags this run' })).toBe(card);
  expect(screen.getByRole('region', { name: 'All bags' })).toBe(scroll);
  expect(scroll.scrollTop).toBe(31);
});
it.each(['error', 'superseded', 'busy'] as const)('clears old filter summary numbers after %s', async failure => {
  const page = mount({ summary: request => ({ totals: reviewTotals, days: request.days.map(() => reviewTotals) }) }); await flush();
  expect(document.querySelector('[data-kpi="bags"] .value')?.textContent).toBe('120');
  const summary = vi.spyOn(page.client, 'getDropSummary');
  if (failure === 'error') summary.mockRejectedValue(new Error('Synthetic summary failure'));
  else summary.mockResolvedValue({ status: failure });
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' })); await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect([...document.querySelectorAll('[data-kpi] .value')].map(value => value.textContent)).toEqual(Array(5).fill('—'));
  expect(document.querySelector('tr.group')?.textContent).not.toContain('120 bags');
  {
    summary.mockResolvedValue({ status: 'ready', value: { totals: reviewTotals, days: [reviewTotals] } });
    summary.mockClear();
    const drops = vi.spyOn(page.client, 'listDrops');
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
    expect(summary).toHaveBeenCalledTimes(1); expect(drops).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[data-kpi="bags"] .value')?.textContent).toBe('120');
  }
});
it('waits for a page and requests its summary exactly once, including an empty page', async () => {
  const page = mount({ summary: request => ({ totals: reviewTotals, days: request.days.map(() => reviewTotals) }) });
  const pending = deferred<HistoryReply<DropPage>>();
  const drops = vi.spyOn(page.client, 'listDrops').mockReturnValueOnce(pending.promise);
  const summary = vi.spyOn(page.client, 'getDropSummary');
  const feedCalls = () => summary.mock.calls.filter(([request]) => request.include_items);
  await flush(); expect(feedCalls()).toHaveLength(0);
  pending.resolve({ status: 'ready', value: { rows: rows(), next: cursor } }); await flush();
  expect(feedCalls()).toHaveLength(1); expect(feedCalls()[0][0].days).toHaveLength(1);
  summary.mockClear();
  const next = deferred<HistoryReply<DropPage>>(); drops.mockReturnValueOnce(next.promise);
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' })); await flush();
  expect(feedCalls()).toHaveLength(0);
  next.resolve({ status: 'ready', value: { rows: [], next: null } }); await flush();
  expect(feedCalls()).toHaveLength(1); expect(feedCalls()[0][0].days).toEqual([]);
});
it('separates the local-day label and keeps the footer outside the table scroller inside its card', async () => {
  const sample = rows().map(row => ({ ...row, drop: { ...row.drop, at: String(Date.now() * 1000) } }));
  mount({ listDrops: () => ({ rows: sample, next: null }), summary: request => ({ totals: reviewTotals, days: request.days.map(() => reviewTotals) }) }); await flush();
  expect(document.querySelector('tr.group')?.textContent).toBe('Today · 120 bags · 3 whites · 22 UT');
  const table = screen.getByRole('table', { name: 'Bags' });
  const card = table.closest('.table-card')!;
  const pager = within(card as HTMLElement).getByRole('navigation', { name: 'Loot pages' });
  expect(pager.closest('footer')?.parentElement).toBe(card);
  expect(table.closest('.table-scroll')?.contains(pager)).toBe(false);
  expect(within(card as HTMLElement).getByText('Bags observed while capturing; pickups are not confirmed.')).toBeTruthy();
});

it('retains summary numbers through one auto-retried Busy followed by ready', async () => {
  const page = mount({ summary: request => ({ totals: reviewTotals, days: request.days.map(() => reviewTotals) }) }); await flush();
  const before = [...document.querySelectorAll('[data-kpi] .value')].map(value => value.textContent);
  const pending = deferred<Awaited<ReturnType<typeof page.client.getDropSummary>>>();
  const summary = vi.spyOn(page.client, 'getDropSummary').mockResolvedValueOnce({ status: 'busy' }).mockReturnValueOnce(pending.promise);
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' })); await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(summary).toHaveBeenCalledTimes(2);
  expect([...document.querySelectorAll('[data-kpi] .value')].map(value => value.textContent)).toEqual(before);
  expect(document.querySelector('tr.group')?.textContent).toContain('120 bags');
  expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  pending.resolve({ status: 'ready', value: { totals: reviewTotals, days: [reviewTotals] } }); await flush();
  expect([...document.querySelectorAll('[data-kpi] .value')].map(value => value.textContent)).toEqual(before);
});
it.each(['bag_dropped', 'drop_updated', 'run_ended', 'batched bag_dropped', 'hidden bag_dropped'] as const)('refreshes the selected run through RouteFrame for %s without remounting its card', async kind => {
  const fixture = mount(); fixture.unmount();
  const client = fixture.client, snapshot = homeSnapshot(), streamId = snapshot.stream_id;
  const store = { state: 'ready' as const, queued: 0, durable: '2', stream_id: streamId, message: '' };
  const initial = story({ run: run({ id: 'synthetic-run' }), drops: [rows()[0].drop, { ...rows()[0].drop, id: 'other-bag' }] });
  const getRun = vi.spyOn(client, 'getRun').mockResolvedValue({ status: 'ready', value: initial });
  const connection = eventConnection();
  const page = render(RouteFrame, { context: connection.context, props: { client, snapshot, route: { kind: 'loot', view: 'feed' }, required: '1', store } }); await flush();
  expect(getRun).toHaveBeenCalledTimes(1);
  const card = screen.getByRole('heading', { name: 'Other bags this run' });
  const scroll = screen.getByRole('region', { name: 'All bags' }); scroll.scrollTop = 31;
  const pending = deferred<Awaited<ReturnType<typeof client.getRun>>>(); getRun.mockReturnValueOnce(pending.promise);
  const newBag = { ...rows()[0].drop, id: 'new-bag' };
  const event: EngineEvent = kind === 'run_ended' ? { run_ended: initial.run }
    : kind === 'drop_updated' ? { drop_updated: newBag } : { bag_dropped: newBag };
  const batch: EventEnvelope[] = [{ stream_id: streamId, sequence: '2', at: newBag.at, event }];
  if (kind === 'batched bag_dropped') batch.push({ stream_id: streamId, sequence: '3', at: newBag.at,
    event: { bag_dropped: { ...newBag, id: 'unrelated-bag', run_id: 'another-run' } } });
  if (kind === 'hidden bag_dropped') setNativePresentationVisible(false);
  connection.emitBatch(batch);
  const required = batch.at(-1)!.sequence;
  await page.rerender({ required, store: { ...store, durable: required } });
  if (kind === 'hidden bag_dropped') {
    await flush(); await vi.advanceTimersByTimeAsync(60_000); await flush();
    expect(getRun).toHaveBeenCalledTimes(1);
    setNativePresentationVisible(true);
  }
  await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(getRun).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('heading', { name: 'Other bags this run' })).toBe(card);
  expect(screen.getByRole('region', { name: 'All bags' })).toBe(scroll);
  expect(scroll.scrollTop).toBe(31);
  pending.resolve({ status: 'ready', value: { ...initial, drops: [...initial.drops, newBag] } }); await flush();
  expect(getRun).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('heading', { name: 'Other bags this run' })).toBe(card);
  expect(screen.getByRole('region', { name: 'All bags' })).toBe(scroll);
  expect(scroll.scrollTop).toBe(31);
  expect(scroll.querySelector('[data-drop-id="new-bag"]')).not.toBeNull();
});
it.each(['error', 'busy'] as const)('hides previous-filter totals when the new drops page ends in %s without a summary read', async failure => {
  const page = mount({ summary: request => ({ totals: reviewTotals, days: request.days.map(() => reviewTotals) }) }); await flush();
  expect(document.querySelector('[data-kpi="bags"] .value')?.textContent).toBe('120');
  const summary = vi.spyOn(page.client, 'getDropSummary');
  const drops = vi.spyOn(page.client, 'listDrops');
  if (failure === 'error') drops.mockRejectedValue(new Error('Synthetic drops failure'));
  else drops.mockResolvedValue({ status: 'busy' });
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' })); await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(summary).not.toHaveBeenCalled();
  expect([...document.querySelectorAll('[data-kpi] .value')].map(value => value.textContent)).toEqual(Array(5).fill('—'));
  expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
  expect(document.querySelector('tr.group')).toBeNull();
});

it('preserves the filter, page and selection when clicking the active range and traversing history', async () => {
  const router = createRouter(fakeHistory('#/loot').adapter);
  router.navigate({ kind: 'loot', view: 'feed' });
  let page = mount({ savedState: router.state.entry }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Enchanted only' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Older bags' })); await flush();
  await fireEvent.click(document.querySelector('[data-row-id="drop:partial"]')!); await flush();
  const before = page.onStateChange.mock.lastCall![0].lootPosition.feed;
  await fireEvent.click(screen.getByRole('button', { name: 'All time' })); await flush();
  expect(page.onStateChange.mock.lastCall![0].lootPosition.feed).toEqual(before);
  router.save(page.onStateChange.mock.lastCall![0]);
  router.back(); router.forward(); page.unmount();
  page = mount({ savedState: router.state.entry }); await flush();
  expect(page.listDrops).toHaveBeenLastCalledWith({ ...defaultFilter(), enchanted_only: true, cursor });
  expect(document.querySelector('[data-row-id="drop:partial"]')?.getAttribute('aria-current')).toBe('true');
  router.dispose();
});

it('shows estimated-source evidence only in debug mode in the table and selected bag', async () => {
  mount(); await flush();
  await fireEvent.click(document.querySelector('[data-row-id="drop:partial"]')!); await flush();
  expect(screen.getAllByText('est.').every(tag => !tag.hasAttribute('title'))).toBe(true);
  setDebugMode(true); await flush();
  expect(screen.getAllByText('est.').every(tag => tag.title === 'Attribution is uncertain based on the available captured evidence.')).toBe(true);
  setDebugMode(false); await flush();
  expect(screen.getAllByText('est.').every(tag => !tag.hasAttribute('title'))).toBe(true);
});

it('retains the same-filter saved summary after contention exhausts its retry', async () => {
  const fixture = mount({ summary: request => ({ totals: reviewTotals, days: request.days.map(() => reviewTotals) }) });
  fixture.unmount();
  const client = fixture.client, snapshot = homeSnapshot();
  const store = { state: 'ready' as const, queued: 0, durable: '2', stream_id: snapshot.stream_id, message: '' };
  const page = render(RouteFrame, { client, snapshot, route: { kind: 'loot', view: 'feed' }, required: '1', store });
  await flush();
  expect(document.querySelector('[data-kpi="bags"] .value')?.textContent).toBe('120');
  const summary = vi.spyOn(client, 'getDropSummary').mockResolvedValue({ status: 'busy' });
  await page.rerender({ required: '2' }); await flush();
  await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(summary).toHaveBeenCalled();
  expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
  expect(document.querySelector('[data-kpi="bags"] .value')?.textContent).toBe('120');
  summary.mockResolvedValue({ status: 'ready', value: { totals: reviewTotals, days: [reviewTotals] } });
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
  expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  expect(document.querySelector('[data-kpi="bags"] .value')?.textContent).toBe('120');
});
