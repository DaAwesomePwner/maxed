import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import PinButton from '../../src/lib/loot/PinButton.svelte';
import { createRouter } from '../../src/lib/navigation/history';
import { parseRoute } from '../../src/lib/navigation/routes';
import { fakeHistory } from '../navigation/fake-history';
import Loot from '../../src/routes/Loot.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { ChaseItem } from '../../src/lib/bindings/ChaseItem';
import type { ItemHistory } from '../../src/lib/bindings/ItemHistory';
import type { VisualPage } from '../../src/lib/bindings/VisualPage';
import { emptyListState, type ListState } from '../../src/lib/navigation/state';
import { setDebugMode } from '../../src/lib/debug/debug';
import { deferred, homeSnapshot } from '../home/fixtures';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { flush } from '../runs/fixtures';

const cursor = { at: '2000000', id: 'synthetic-bag' };
function fixture(): ItemHistory {
  return { item_type: 101, totals: { appearances: 5, bags: 3, taken: 2, first_at: '1000000', last_at: '3000000' },
    enchant_variants: [
      { state: 'missing', applied: null, ids: [], count: 2 },
      { state: 'recorded', applied: 1, ids: [10], count: 1 },
      { state: 'invalid', applied: null, ids: [], count: 1 },
      { state: 'recorded_empty', applied: 0, ids: [], count: 1 },
    ],
    by_dungeon: [{ dungeon: null, count: 2, last_at: '1000000' }, { dungeon: 'Invented Sanctum', count: 3, last_at: '3000000' }],
    by_source: [{ source_enemy: 900, estimated: true, count: 3 }, { source_enemy: null, estimated: false, count: 2 }],
    bags: { rows: [{ dungeon: null, drop: { ...homeSnapshot().drops[0], id: 'synthetic-bag', run_id: null } }], next: cursor },
  };
}
const visuals: VisualPage = { client_fingerprint: 'a'.repeat(64), objects: [
  { object_type: 101, name: 'Invented sword', sprite: null, bag: null, item_tier: 'UT' },
  { object_type: 900, name: 'Invented guardian', sprite: null, bag: null },
] };
function mount(options: { value?: ItemHistory; type?: number; savedState?: ListState; busy?: boolean; error?: boolean; settingsError?: boolean; chase?: ChaseItem[] } = {}) {
  const itemHistory = vi.fn(() => options.value ?? fixture());
  const client = createClient(createFixtureTransport(homeSnapshot(), visuals, undefined, undefined, undefined, undefined, undefined, {}, { itemHistory }));
  const setChaseList = vi.spyOn(client, 'setChaseList');
  if (options.settingsError) vi.spyOn(client, 'querySettings').mockRejectedValueOnce(new Error('synthetic settings failure'));
  if (options.chase) {
    const querySettings = client.querySettings;
    vi.spyOn(client, 'querySettings').mockImplementation(async () => ({ ...await querySettings(), loot: { chase: options.chase! } }));
  }
  if (options.busy) vi.spyOn(client, 'getItemHistory').mockResolvedValue({ status: 'busy' });
  if (options.error) vi.spyOn(client, 'getItemHistory').mockRejectedValue(new Error('synthetic read failure'));
  const onStateChange = vi.fn(), onRowsReady = vi.fn();
  const view = render(Loot, { route: { kind: 'item', type: options.type ?? 101 }, client,
    streamId: 'synthetic-stream', savedState: options.savedState, onStateChange, onRowsReady, entryKey: 'item-entry' });
  return { ...view, client, itemHistory, setChaseList, onStateChange, onRowsReady };
}
beforeEach(() => { setDebugMode(false); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('renders totals from the fixture with catalog tier and visible honesty copy', async () => {
  mount(); await flush();
  expect(screen.getByText('in 3 bags')).toBeTruthy();
  expect(screen.getByText(/first seen/)).toBeTruthy();
  expect(screen.getByText(/last seen/)).toBeTruthy();
  expect(document.querySelector('[data-kpi=seen] .value')?.textContent).toBe('5');
  expect(document.querySelector('[data-kpi=taken] .value')?.textContent).toBe('2');
  expect(document.querySelector('[data-kpi=enchanted] .value')?.textContent).toBe('1');
  expect(screen.getByRole('heading', { name: 'Invented sword' })).toBeTruthy();
  expect(screen.getByText('UT')).toBeTruthy();
  expect(screen.getByText(/who took the item is unknown/)).toBeTruthy();
});
it('keeps missing and recorded enchant variants in separate rows', async () => {
  mount(); await flush();
  const variants = within(screen.getByRole('table', { name: 'Enchants' }));
  expect(variants.getByRole('row', { name: 'Enchant unknown 2' })).toBeTruthy();
  expect(variants.getByRole('row', { name: '1 enchant (#10) 1' })).toBeTruthy();
  expect(variants.getByRole('row', { name: 'Enchant unreadable 1' })).toBeTruthy();
  expect(variants.getByRole('row', { name: 'No enchants 1' })).toBeTruthy();
});
it('labels a runless place Outside a run', async () => {
  mount(); await flush();
  expect(within(screen.getByRole('table', { name: 'Where' })).getByText('Outside a run')).toBeTruthy();
});
it('shows Not seen yet and the numeric fallback for an unknown item', async () => {
  const value = fixture(); value.item_type = 65000;
  value.totals = { appearances: 0, bags: 0, taken: 0, first_at: null, last_at: null };
  value.enchant_variants = []; value.by_dungeon = []; value.by_source = []; value.bags = { rows: [], next: null };
  const page = mount({ type: 65000, value }); await flush();
  expect(screen.getByRole('heading', { name: 'Not seen yet' })).toBeTruthy();
  expect(screen.getByText("Maxed hasn't seen this item in any bag yet.")).toBeTruthy();
  expect(screen.getByRole('heading', { name: '#65000' })).toBeTruthy();
  expect(page.container.querySelector('[data-fallback="true"]')).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Chase' })).toBeTruthy();
});
it('adds a pin with a microsecond timestamp and removes it on the second click', async () => {
  const page = mount(); await flush();
  const now = (BigInt(Date.now()) * 1000n).toString();
  await fireEvent.click(screen.getByRole('button', { name: 'Chase' })); await flush();
  expect(page.setChaseList).toHaveBeenCalledExactlyOnceWith([{ item_type: 101, dungeon: null, pinned_at: now }]);
  expect(screen.getByRole('button', { name: 'Chasing' }).getAttribute('aria-pressed')).toBe('true');
  await fireEvent.click(screen.getByRole('button', { name: 'Chasing' })); await flush();
  expect(page.setChaseList).toHaveBeenLastCalledWith([]);
  expect(screen.getByRole('button', { name: 'Chase' }).getAttribute('aria-pressed')).toBe('false');
});
it('keeps the previous pin state and shows an inline save error on rejection', async () => {
  const page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Chase' })); await flush();
  page.setChaseList.mockRejectedValue(new Error('synthetic save failure'));
  await fireEvent.click(screen.getByRole('button', { name: 'Chasing' })); await flush();
  expect(screen.getByText("Couldn't save the chase list")).toBeTruthy();
  expect(screen.getByRole('button', { name: 'Chasing' }).getAttribute('aria-pressed')).toBe('true');
});
it('pages bags with the cursor, saves it, and restores it on return', async () => {
  const page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Older ›' })); await flush();
  expect(page.itemHistory).toHaveBeenLastCalledWith({ item_type: 101, cursor, limit: 50 });
  const patch = page.onStateChange.mock.lastCall![0];
  page.unmount();
  const restored = mount({ savedState: { ...emptyListState(), ...patch } }); await flush();
  expect(restored.itemHistory).toHaveBeenLastCalledWith({ item_type: 101, cursor, limit: 50 });
  await fireEvent.click(screen.getByRole('button', { name: '‹ Newer' })); await flush();
  expect(restored.itemHistory).toHaveBeenLastCalledWith({ item_type: 101, cursor: null, limit: 50 });
  await fireEvent.click(screen.getByRole('button', { name: 'Older ›' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Latest' })); await flush();
  expect(restored.itemHistory).toHaveBeenLastCalledWith({ item_type: 101, cursor: null, limit: 50 });
});
it('visibly marks estimated sources and unknown sources', async () => {
  mount(); await flush();
  const sources = within(screen.getByRole('table', { name: 'From' }));
  expect(sources.getByRole('row', { name: 'Invented guardian estimated 3' })).toBeTruthy();
  expect(sources.getByText('Unknown source')).toBeTruthy();
});
it('waits for item rows before releasing rows-ready', async () => {
  const page = mount();
  const pending = deferred<HistoryReply<ItemHistory>>();
  vi.spyOn(page.client, 'getItemHistory').mockReturnValueOnce(pending.promise); await flush();
  expect(screen.getByRole('heading', { name: 'Loading item history' })).toBeTruthy();
  expect(page.onRowsReady).not.toHaveBeenCalled();
  pending.resolve({ status: 'ready', value: fixture() }); await flush();
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('item-entry');
});
it('renders busy retry and errors without reporting a zero state', async () => {
  const busy = mount({ busy: true }); await flush(); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(screen.getByRole('heading', { name: 'Loot history busy' })).toBeTruthy();
  const retry = within(screen.getByRole('alert')).getByRole('button', { name: 'Retry' });
  expect(screen.getByRole('alert').textContent).toContain("Couldn't load bags");
  expect(retry.hasAttribute('disabled')).toBe(false);
  expect(retry.hasAttribute('aria-disabled')).toBe(false);
  expect(screen.queryByText('Not seen yet')).toBeNull();
  busy.unmount();
  const page = mount({ error: true }); await flush();
  expect(screen.getByRole('heading', { name: "Couldn't load item history" })).toBeTruthy();
  expect(within(screen.getByRole('alert')).getByText("Couldn't load bags")).toBeTruthy();
  expect(screen.queryByText('Not seen yet')).toBeNull();
  vi.mocked(page.client.getItemHistory).mockResolvedValue({ status: 'ready', value: fixture() });
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
  expect(screen.getByText('in 3 bags')).toBeTruthy();
});
it('releases rows-ready with no client even while disabled', async () => {
  const onRowsReady = vi.fn();
  render(Loot, { route: { kind: 'item', type: 101 }, enabled: false, onRowsReady, entryKey: 'absent' }); await flush();
  expect(screen.getByRole('heading', { name: 'Loot unavailable' })).toBeTruthy();
  expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('absent');
});
it('disables pinning until settings load and offers retry if loading fails', async () => {
  mount({ settingsError: true }); await flush();
  expect(screen.getByRole('button', { name: 'Chase' }).hasAttribute('disabled')).toBe(true);
  expect(screen.getByText("Couldn't load the chase list")).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry chase list' })); await flush();
  expect(screen.getByRole('button', { name: 'Chase' }).hasAttribute('disabled')).toBe(false);
});
it('disables the pin button during a save and preserves unrelated dungeon pins', async () => {
  const scoped = { item_type: 101, dungeon: 'Invented Sanctum', pinned_at: '1000000' };
  const page = mount({ chase: [scoped] }); await flush();
  const pending = deferred<void>();
  page.setChaseList.mockReturnValueOnce(pending.promise);
  const button = screen.getByRole('button', { name: 'Chase' });
  await fireEvent.click(button); await flush();
  expect(button.hasAttribute('disabled')).toBe(true);
  expect(button.getAttribute('aria-pressed')).toBe('false');
  expect(page.setChaseList.mock.lastCall![0]).toEqual([scoped, { item_type: 101, dungeon: null, pinned_at: (BigInt(Date.now()) * 1000n).toString() }]);
  pending.resolve(); await flush();
  expect(screen.getByRole('button', { name: 'Chasing' }).hasAttribute('disabled')).toBe(false);
  await fireEvent.click(screen.getByRole('button', { name: 'Chasing' })); await flush();
  expect(page.setChaseList).toHaveBeenLastCalledWith([scoped]);
});
it('resets paging when navigating to a different item and ignores its old saved state', async () => {
  const page = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Older ›' })); await flush();
  const patch = page.onStateChange.mock.lastCall![0];
  await page.rerender({ route: { kind: 'item', type: 102 }, savedState: { ...emptyListState(), ...patch }, entryKey: 'other-item' }); await flush();
  expect(page.itemHistory).toHaveBeenLastCalledWith({ item_type: 102, cursor: null, limit: 50 });
  expect(screen.getByRole('button', { name: '‹ Newer' }).getAttribute('aria-disabled')).toBe('true');
  expect(page.onRowsReady).toHaveBeenLastCalledWith('other-item');
});
it('does not load history or settings while disabled with a client', async () => {
  const page = mount();
  const getItemHistory = vi.spyOn(page.client, 'getItemHistory'), querySettings = vi.spyOn(page.client, 'querySettings');
  await page.rerender({ enabled: false }); await flush();
  expect(getItemHistory).not.toHaveBeenCalled();
  expect(querySettings).not.toHaveBeenCalled();
  expect(page.onRowsReady).not.toHaveBeenCalled();
  await page.rerender({ enabled: true }); await flush();
  expect(getItemHistory).toHaveBeenCalledTimes(1);
  expect(querySettings).toHaveBeenCalledTimes(1);
  expect(page.onRowsReady).toHaveBeenCalledExactlyOnceWith('item-entry');
});

it('uses singular totals for one appearance in one bag', async () => {
  const value = fixture();
  value.totals = { ...value.totals, appearances: 1, bags: 1, taken: 0 };
  mount({ value }); await flush();
  expect(screen.getByText('in 1 bag')).toBeTruthy();
});
it('distinguishes recorded enchant variants with the same applied count by their ids', async () => {
  const value = fixture();
  value.enchant_variants = [
    { state: 'recorded', applied: 2, ids: [10, 11], count: 3 },
    { state: 'recorded', applied: 2, ids: [12, 13], count: 2 },
  ];
  mount({ value }); await flush();
  const variants = within(screen.getByRole('table', { name: 'Enchants' }));
  expect(variants.getByRole('row', { name: '2 enchants (#10, #11) 3' })).toBeTruthy();
  expect(variants.getByRole('row', { name: '2 enchants (#12, #13) 2' })).toBeTruthy();
});
it.each(['error', 'busy'] as const)('hides a retained zero summary when a refresh returns %s', async phase => {
  const value = fixture();
  value.totals = { appearances: 0, bags: 0, taken: 0, first_at: null, last_at: null };
  value.enchant_variants = []; value.by_dungeon = []; value.by_source = []; value.bags = { rows: [], next: null };
  const page = mount({ value }); await flush();
  expect(screen.getByRole('heading', { name: 'Not seen yet' })).toBeTruthy();
  const read = vi.spyOn(page.client, 'getItemHistory');
  if (phase === 'error') read.mockRejectedValue(new Error('synthetic refresh failure'));
  else read.mockResolvedValue({ status: 'busy' });
  await page.rerender({ required: '2', store: { state: 'ready', durable: '2', stream_id: 'synthetic-stream', queued: 0, message: '' } }); await flush();
  await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(screen.getByRole('heading', { name: phase === 'error' ? "Couldn't load item history" : 'Loot history busy' })).toBeTruthy();
  expect(screen.queryByText(/^Seen 0/)).toBeNull();
  expect(screen.queryByText('Not seen yet')).toBeNull();
  expect(screen.queryByText("Maxed hasn't seen this item in any bag yet.")).toBeNull();
  expect(screen.queryAllByRole('table')).toHaveLength(0);
});

it('links the breadcrumb and feed action and keeps the shared range through both routes', async () => {
  const browser = fakeHistory('#/loot');
  const router = createRouter(browser.adapter);
  const range = { start: '1000000', end: '9000000' };
  const position = (await import('../../src/lib/navigation/state')).normalizeListState({ lootPosition: { range } }).lootPosition;
  router.save({ lootPosition: position });
  router.navigate({ kind: 'item', type: 101 });
  expect(router.state.entry.lootPosition?.range).toEqual(range);
  mount({ savedState: router.state.entry }); await flush();
  const breadcrumb = within(screen.getByRole('navigation', { name: 'Breadcrumb' })).getByRole('link', { name: 'Loot' });
  expect(breadcrumb.getAttribute('href')).toBe('#/loot');
  expect(screen.getByRole('link', { name: 'Show in feed' }).getAttribute('href')).toBe('#/loot/with/101');
  await fireEvent.click(breadcrumb);
  router.navigate(parseRoute(breadcrumb.getAttribute('href')!));
  expect(router.state.entry.lootPosition?.range).toEqual(range);
  router.back(); expect(router.state.entry.lootPosition?.range).toEqual(range);
  router.forward(); expect(router.state.entry.lootPosition?.range).toEqual(range);
  router.navigate({ kind: 'loot', view: 'feed', seed: { item: 101 } });
  expect(router.state.entry.lootPosition?.range).toEqual(range);
  router.dispose();
});
it('opens the run from an item table row by mouse and keyboard', async () => {
  const value = fixture(); value.bags.rows[0].drop.run_id = 'synthetic-run';
  const page = mount({ value }); await flush();
  const table = screen.getByRole('table', { name: 'Bags' });
  expect(within(table).getByRole('columnheader', { name: 'Enchants' })).toBeTruthy();
  expect(within(table).getByRole('columnheader', { name: 'Taken' })).toBeTruthy();
  const link = within(table).getByRole('link', { name: 'Open run' });
  expect(link.getAttribute('href')).toBe('#/runs/synthetic-run');
  const click = vi.fn((event: Event) => event.preventDefault()); link.addEventListener('click', click);
  const row = page.container.querySelector<HTMLElement>('[data-row-id]')!;
  await fireEvent.click(row); await fireEvent.keyDown(row, { key: 'Enter' }); await fireEvent.keyDown(row, { key: ' ' });
  expect(click).toHaveBeenCalledTimes(3);
});
it('renders and saves both states of the icon Chase control', async () => {
  const client = createClient(createFixtureTransport(homeSnapshot()));
  let settings = await client.querySettings();
  const save = vi.spyOn(client, 'setChaseList');
  const onChange = vi.fn();
  const page = render(PinButton, { client, itemType: 101, name: 'Invented sword', settings, variant: 'icon', onChange });
  const button = screen.getByRole('button', { name: 'Chase Invented sword' });
  expect(button.classList.contains('sq')).toBe(true);
  await fireEvent.click(button); await flush();
  expect(save).toHaveBeenCalledOnce();
  settings = { ...settings, loot: { ...settings.loot, chase: onChange.mock.lastCall![0] } };
  await page.rerender({ settings });
  const chasing = screen.getByRole('button', { name: 'Stop chasing Invented sword' });
  expect(chasing.getAttribute('aria-pressed')).toBe('true');
  await fireEvent.click(chasing); await flush();
  expect(save).toHaveBeenLastCalledWith([]);
});

it('does not request the unused header drop summary on Item routes', async () => {
  const page = mount();
  const summary = vi.spyOn(page.client, 'getDropSummary');
  await flush();
  expect(summary).not.toHaveBeenCalled();
});
