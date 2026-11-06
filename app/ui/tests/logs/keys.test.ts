import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import KeyPopsView from '../../src/lib/logs/KeyPopsView.svelte';
import EventsView from '../../src/lib/logs/EventsView.svelte';
import EventPortal from '../../src/lib/logs/EventPortal.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import fixture from '../fixtures/t0-live';
import type { EventSummary } from '../../src/lib/bindings/EventSummary';
import { defaultLogsPosition, emptyListState, type ListState } from '../../src/lib/navigation/state';
const base = { id: 'pop', session_id: 's', run_id: null, map: 'Nexus', at: '1000000', provisional: false };
const cursor = { at: '1000000', id: 'pop' };
const summary: EventSummary = { total: 20, by_kind: { player_death: 1, player_left: 4, realm_event_killed: 3, portal_opened: 12 },
  dungeons: [{ dungeon: 'Invented vault', pops: 8, last_at: '1000000' }, { dungeon: 'Muted vault', pops: 4, last_at: '1' }], local_pops: 2, last_local_pop: null, first_at: '1', flagged_since: '2' };
function setup() {
  const client = createClient(createFixtureTransport(fixture));
  client.listInstanceEvents = vi.fn(async filter => ({ status: 'ready' as const, value: { rows: [{ ...base, id: filter.cursor ? 'older' : 'pop',
    kind: { type: 'portal_opened' as const, opener: filter.cursor ? 'Quenby' : 'Morrow', dungeon: 'Invented vault', opener_is_local: true } }], next: filter.cursor ? null : cursor,
    entered_runs: [] } }));
  client.getEventSummary = vi.fn(async () => ({ status: 'ready' as const, value: summary }));
  client.listCatalogDungeons = vi.fn(async () => ['Invented vault', 'Muted vault']);
  client.getDungeonPortalType = vi.fn(async () => null);
  const settings = client.querySettings;
  client.querySettings = vi.fn(async () => { const value = await settings(); return { ...value, alerts: { ...value.alerts, key_popped: 'toast' as const, key_pop_dungeons: null, key_pop_excluded_dungeons: ['Muted vault'] } }; });
  return client;
}
afterEach(() => { cleanup(); vi.useRealTimers(); window.location.hash = ''; });
async function flush() { for (let i = 0; i < 12; i++) { await Promise.resolve(); await tick(); } }
it('defers unrelated checkpoints without a pill and omits range counts', async () => {
  const client = setup(), position = defaultLogsPosition();
  position.range = { start: '0', end: '2000000' }; position.keys.filter.cursor = cursor;
  const view = render(KeyPopsView, { client, savedState: { ...emptyListState(), logsPosition: position }, required: '1', streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } });
  await screen.findByRole('table');
  await view.rerender({ required: '2', pendingCount: 0 }); await flush();
  expect(screen.queryByRole('button', { name: /new key pops/i })).toBeNull();
  expect(client.listInstanceEvents).toHaveBeenCalledTimes(1);
  await view.rerender({ required: '3', pendingCount: 2 }); await flush();
  expect(screen.getByRole('button', { name: '↑ New key pops' })).toBeTruthy();
});
it('reads only portals, filters from the ranking and removable chip, and strikes muted dungeons', async () => {
  const client = setup(); render(KeyPopsView, { client });
  const rank = await screen.findByRole('button', { name: 'Invented vault 8' });
  await waitFor(() => expect(screen.getByRole('button', { name: 'Muted vault 4' }).querySelector('.muted')).toBeTruthy());
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ kinds: ['portal_opened'], dungeon: null }));
  await fireEvent.click(rank);
  await waitFor(() => expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ dungeon: 'Invented vault', cursor: null })));
  expect(screen.getByRole('button', { name: 'Remove dungeon filter' }).textContent).toContain('Invented vault');
  await fireEvent.click(screen.getByRole('button', { name: 'Remove dungeon filter' }));
  await waitFor(() => expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ dungeon: null })));
  await fireEvent.click(rank); await fireEvent.click(rank);
  await waitFor(() => expect(rank.getAttribute('aria-pressed')).toBe('false'));
  expect(client.getEventSummary).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('link', { name: 'Alert settings ›' }).getAttribute('href')).toBe('#/settings');
});
it('filters local openers on the server and debounces opener search without refetching summary', async () => {
  vi.useFakeTimers(); const client = setup(); render(KeyPopsView, { client }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Opened by me' })); await flush();
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ opener_local: true }));
  await fireEvent.input(screen.getByRole('textbox', { name: 'Find who opened it' }), { target: { value: 'Morrow' } });
  await vi.advanceTimersByTimeAsync(249); expect(client.listInstanceEvents).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1); await flush();
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'Morrow', opener_local: true }));
  expect(client.getEventSummary).toHaveBeenCalledTimes(1);
});
it.each([false, true])('strikes uncatalogued ranked dungeons in selected-only mode while keeping the KPI catalog-based (catalog failure: %s)', async failed => {
  const client = setup(), settings = client.querySettings;
  client.querySettings = vi.fn(async () => {
    const value = await settings();
    return { ...value, alerts: { ...value.alerts, key_pop_dungeons: ['Invented vault'], key_pop_excluded_dungeons: [] } };
  });
  client.listCatalogDungeons = vi.fn(async () => {
    if (failed) throw new Error('Synthetic catalog failure');
    return ['Invented vault', 'Catalog-only vault'];
  });
  const view = render(KeyPopsView, { client });
  await waitFor(() => expect(screen.getByRole('button', { name: 'Muted vault 4' }).querySelector('.muted')).toBeTruthy());
  expect(screen.getByRole('button', { name: 'Invented vault 8' }).querySelector('.muted')).toBeNull();
  expect(view.container.querySelector('[data-kpi="dungeons"]')?.textContent).toContain(`${failed ? 0 : 1} muted for alerts`);
  if (failed) expect(screen.getByText('Dungeon catalog unavailable. Check assets in Settings.')).toBeTruthy();
});
it('rounds fractional match gaps and persists selection when the run link itself is clicked', async () => {
  const client = setup(), onStateChange = vi.fn();
  client.listInstanceEvents = vi.fn(async () => ({ status: 'ready' as const, value: {
    rows: ['first', 'matched'].map(id => ({ ...base, id, kind: { type: 'portal_opened' as const, dungeon: 'Invented vault', opener: id === 'first' ? 'Morrow' : 'Quenby' } })),
    next: null,
    entered_runs: [{ event_id: 'matched', run_id: 'run-2', dungeon: 'Invented vault', started_at: '43318912', outcome: 'left' }],
  } }));
  render(KeyPopsView, { client, onStateChange });
  const row = await screen.findByRole('row', { name: /Quenby/ });
  expect(row.getAttribute('aria-selected')).toBe('false');
  const link = within(row).getByRole('link');
  expect(link.getAttribute('title')).toBe('A Invented vault run started 42 s after this portal opened');
  await fireEvent.click(link);
  expect(onStateChange.mock.lastCall![0].logsPosition.keys.selectedId).toBe('matched');
  await waitFor(() => expect(window.location.hash).toBe('#/runs/run-2'));
});
it('serializes page, range summary, all-time summary and settings and ignores row filters for summaries', async () => {
  const client = setup(), order: string[] = [];
  const page = client.listInstanceEvents, prefs = client.querySettings;
  client.listInstanceEvents = vi.fn(async filter => { order.push('page'); return page(filter); });
  client.getEventSummary = vi.fn(async request => { order.push(request.filter.range ? 'range' : 'all'); return { status: 'ready' as const, value: summary }; });
  client.querySettings = vi.fn(async () => { order.push('settings'); return prefs(); });
  render(KeyPopsView, { client, savedState: { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), range: { start: '0', end: '2000000' } } } });
  await waitFor(() => expect(order).toEqual(['page', 'range', 'all', 'settings']));
  expect(client.getEventSummary).toHaveBeenCalledWith({ filter: expect.objectContaining({ query: '', dungeon: null, opener_local: null }) });
  await fireEvent.click(screen.getByRole('button', { name: '7 days' }));
  await waitFor(() => expect(order).toEqual(['page', 'range', 'all', 'settings', 'page', 'range']));
});
it('opens matched runs by row or keyboard, labels the match, and only tags known local openers', async () => {
  const client = setup();
  client.listInstanceEvents = vi.fn(async () => ({ status: 'ready' as const, value: { rows: [true, false, null].map((local, i) => ({ ...base, id: `pop${i}`, kind: { type: 'portal_opened' as const, dungeon: 'Invented vault', opener: `Vexlin ${i}`, opener_is_local: local } })), next: null,
    entered_runs: [{ event_id: 'pop0', run_id: 'run-1', dungeon: 'Invented vault', started_at: '31000000', outcome: 'left' }] } }));
  const onStateChange = vi.fn(); render(KeyPopsView, { client, onStateChange });
  const row = await screen.findByRole('row', { name: /Vexlin 0/ });
  expect(screen.getAllByText('you')).toHaveLength(1);
  const link = within(row).getByRole('link');
  expect(link.getAttribute('title')).toBe('A Invented vault run started 30 s after this portal opened');
  expect(link.getAttribute('href')).toBe('#/runs/run-1');
  await fireEvent.click(row); await waitFor(() => expect(window.location.hash).toBe('#/runs/run-1'));
  window.location.hash = '';
  await fireEvent.keyDown(row, { key: 'Enter' }); await waitFor(() => expect(window.location.hash).toBe('#/runs/run-1'));
  await fireEvent.click(screen.getByRole('row', { name: /Vexlin 1/ }));
  expect(onStateChange.mock.lastCall![0].logsPosition.keys.selectedId).toBe('pop1');
});
it('keeps the catalog failure message and reads nothing while disabled', async () => {
  const client = setup(), onRowsReady = vi.fn();
  client.listCatalogDungeons = vi.fn(async () => { throw new Error('synthetic'); });
  const view = render(KeyPopsView, { client, enabled: false, onRowsReady, entryKey: 'keys' }); await flush();
  for (const read of [client.listInstanceEvents, client.getEventSummary, client.querySettings, client.listCatalogDungeons]) expect(read).not.toHaveBeenCalled();
  expect(onRowsReady).not.toHaveBeenCalled();
  await view.rerender({ enabled: true });
  await screen.findByText('Dungeon catalog unavailable. Check assets in Settings.');
  await screen.findByRole('row', { name: /Morrow/ });
  expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('keys');
});
it('restores keys independently of Events including selection and cursor', async () => {
  const client = setup();
  const savedState: ListState = { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), keys: { filter: { kinds: ['portal_opened'], run_id: null, dungeon: 'Invented vault', cursor, limit: 50, query: 'Quenby', opener_local: true }, previous: [null], selectedId: 'older' } } };
  const view = render(KeyPopsView, { client, savedState });
  expect((await screen.findByRole('row', { name: /Quenby/ })).getAttribute('aria-selected')).toBe('true');
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith({ ...savedState.logsPosition!.keys.filter, range: null });
  view.unmount(); render(EventsView, { client, savedState });
  await waitFor(() => expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ kinds: [], dungeon: null, cursor: null, query: '' })));
});
it.each([false, true])('defers new pops and only counts them without row filters (filtered: %s)', async filtered => {
  const client = setup();
  const savedState: ListState = { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), keys: { filter: { kinds: ['portal_opened'], run_id: null, dungeon: filtered ? 'Invented vault' : null, cursor, limit: 50 }, previous: [null] } } };
  const view = render(KeyPopsView, { client, savedState, required: '1', pendingCount: 12, streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } });
  await screen.findByRole('row', { name: /Quenby/ }); await view.rerender({ required: '2' });
  const pill = await screen.findByRole('button', { name: filtered ? '↑ New key pops' : '↑ 12 new key pops' });
  expect(client.listInstanceEvents).toHaveBeenCalledTimes(1); await fireEvent.click(pill);
  await screen.findByRole('row', { name: /Morrow/ });
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: null }));
});
it('renders portal art with latest-wins lookup and retires its listener', async () => {
  const client = setup(); let finishOld!: (value: number | null) => void;
  const release = vi.fn(); client.onVisualsChanged = vi.fn(() => release);
  client.getDungeonPortalType = vi.fn().mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve; })).mockResolvedValue(2);
  client.getObjectVisuals = vi.fn(async () => ({ client_fingerprint: 'a'.repeat(64), objects: [{ object_type: 2, name: 'Synthetic portal', sprite: { key: 'synthetic', sheet: 'mapObjects.png', x: 0, y: 0, width: 8, height: 8 }, bag: null }] }));
  const view = render(EventPortal, { client, dungeon: 'Old dungeon' }); await flush();
  await view.rerender({ dungeon: 'Invented vault' }); await screen.findByRole('img', { name: 'Synthetic portal' });
  finishOld(1); await flush(); expect(client.getObjectVisuals).toHaveBeenCalledExactlyOnceWith([2]);
  expect(release).not.toHaveBeenCalled(); view.unmount(); expect(release).toHaveBeenCalledOnce();
});
it('refreshes at the head and restores focus when the focused portal disappears', async () => {
  vi.useFakeTimers(); const client = setup(), onStateChange = vi.fn();
  const main = document.createElement('main'), heading = document.createElement('h1');
  heading.textContent = 'Logs'; heading.tabIndex = -1; main.append(heading); document.body.append(main);
  const target = document.createElement('div'); main.append(target);
  const view = render(KeyPopsView, { target, props: { client, onStateChange, required: '1', streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } } });
  await flush(); screen.getByRole('row', { name: /Morrow/ }).focus();
  client.listInstanceEvents = vi.fn(async () => ({ status: 'ready' as const, value: { rows: [], next: null } }));
  await view.rerender({ required: '2' }); await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(document.activeElement).toBe(heading); expect(screen.getByText('Row Removed')).toBeTruthy();
  expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ focusedRowId: null }));
  expect(screen.queryByRole('button', { name: /new key pops/i })).toBeNull();
  view.unmount(); main.remove();
});
it('offers empty-state actions and retries errors without discarding filters', async () => {
  const client = setup();
  client.listInstanceEvents = vi.fn().mockRejectedValueOnce(new Error('synthetic failure')).mockResolvedValue({ status: 'ready', value: { rows: [], next: null } });
  client.getEventSummary = vi.fn(async () => ({ status: 'ready' as const, value: { ...summary, total: 0, by_kind: { player_death: 0, player_left: 0, realm_event_killed: 0, portal_opened: 0 }, dungeons: [], local_pops: 0, first_at: null } }));
  render(KeyPopsView, { client, savedState: { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), range: { start: '0', end: '10' } } } });
  await screen.findByText("Couldn't load key pops. Your filters are kept.");
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
  await screen.findByRole('heading', { name: 'No key pops in this range' });
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ range: { start: '0', end: '10' } }));
  await fireEvent.click(screen.getByRole('button', { name: 'Show all time' }));
  await screen.findByRole('heading', { name: 'No key pops' });
  await waitFor(() => expect(screen.queryByText(/key pops seen/)).toBeNull());
  expect(screen.getByText('Portals will appear when Maxed sees them open in-game.')).toBeTruthy();
});

it('keeps trailing spaces without repeated persistence, selection or paging resets', async () => {
  vi.useFakeTimers(); const client = setup(), listInstanceEvents = vi.mocked(client.listInstanceEvents);
  const onStateChange = vi.fn();
  const view = render(KeyPopsView, { client, onStateChange }); await flush();
  const input = screen.getByRole('textbox', { name: 'Find who opened it' }) as HTMLInputElement;
  await fireEvent.input(input, { target: { value: 'Vexlin' } });
  await vi.advanceTimersByTimeAsync(250); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Older key pops' })); await flush();
  const row = view.container.querySelector<HTMLElement>('[data-row-id]')!;
  await fireEvent.click(row); await flush();
  const saved = onStateChange.mock.lastCall![0].logsPosition.keys;
  expect(saved.previous).toHaveLength(1);
  expect(saved.selectedId).toBe(row.dataset.rowId);
  const writes = onStateChange.mock.calls.length, reads = listInstanceEvents.mock.calls.length;
  await fireEvent.input(input, { target: { value: 'Vexlin ' } });
  await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(input.value).toBe('Vexlin ');
  expect(onStateChange).toHaveBeenCalledTimes(writes);
  expect(listInstanceEvents).toHaveBeenCalledTimes(reads);
  expect(row.getAttribute('aria-selected')).toBe('true');
  expect(onStateChange.mock.lastCall![0].logsPosition.keys).toEqual(saved);
  await fireEvent.input(input, { target: { value: 'Invented ' } });
  await vi.advanceTimersByTimeAsync(500); await flush();
  expect(input.value).toBe('Invented ');
  expect(listInstanceEvents.mock.lastCall![0].query).toBe('Invented');
  await fireEvent.input(input, { target: { value: input.value + 'vault' } });
  await vi.advanceTimersByTimeAsync(500); await flush();
  expect(listInstanceEvents.mock.lastCall![0].query).toBe('Invented vault');
});
