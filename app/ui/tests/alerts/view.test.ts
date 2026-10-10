import { afterEach, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import AlertPanel from '../../src/lib/alerts/AlertPanel.svelte';
import AlertsView from '../../src/lib/alerts/AlertsView.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import fixture from '../fixtures/t0-live';
import { alert, deferred } from '../phase2/fixtures';
import { defaultLogsPosition, emptyListState, normalizeListState } from '../../src/lib/navigation/state';
import { createRouter } from '../../src/lib/navigation/history';
import { fakeHistory } from '../navigation/fake-history';
import type { AlertSummary } from '../../src/lib/bindings/AlertSummary';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { createAlertSession } from '../../src/lib/alerts/session';
import { createDungeonMute, DUNGEON_MUTE } from '../../src/lib/alerts/dungeon-mute';
import { setDebugMode } from '../../src/lib/debug/debug';
const summary: AlertSummary = { total: 110, by_kind: [{ kind: 'white_bag', count: 10 }], last_white: null, first_at: '1' };
const cursor = { at: '1', id: 'older' };
function setup() {
  const client = createClient(createFixtureTransport(fixture));
  client.listAlerts = vi.fn(async filter => ({ status: 'ready' as const, value: { rows: Array.from({ length: 50 }, (_, i) => alert({ id: `${filter.cursor ? 'older' : 'row'}-${i}` })), next: filter.cursor ? null : cursor } }));
  client.getAlertSummary = vi.fn(async () => ({ status: 'ready' as const, value: summary }));
  return client;
}
afterEach(() => { cleanup(); vi.useRealTimers(); setDebugMode(false); });
async function flush() { for (let i = 0; i < 20; i++) { await Promise.resolve(); await tick(); } }
it('filters full server pages with tiles, mirrored kind and Hide key pops', async () => {
  const client = setup(); render(AlertsView, { client });
  await screen.findByRole('table'); expect(document.querySelectorAll('[data-row-id]')).toHaveLength(50);
  await fireEvent.click(screen.getByRole('button', { name: /^Messages/ }));
  await waitFor(() => expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ kind: null, kinds: ['whisper', 'guild_message', 'party_message'], limit: 50 })));
  expect((screen.getByRole('combobox') as HTMLSelectElement).value).toBe('messages');
  await fireEvent.click(screen.getByRole('button', { name: 'Hide key pops' })); await flush();
  expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ exclude_kinds: ['key_popped'] }));
  await fireEvent.click(screen.getByRole('button', { name: 'Older alerts' })); await flush();
  expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ cursor })); expect(document.querySelectorAll('[data-row-id]')).toHaveLength(50);
  await fireEvent.change(screen.getByRole('combobox'), { target: { value: 'death' } }); await flush();
  expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ kind: 'death', kinds: [], cursor: null }));
  expect(screen.getByRole('button', { name: /^All alerts/ }).getAttribute('aria-pressed')).toBe('true');
  expect(client.getAlertSummary).toHaveBeenCalledTimes(1);
});
it.each(['tile', 'kind'])('returns to All when Hide key pops is enabled from the keys %s', async source => {
  const client = setup(); render(AlertsView, { client }); await screen.findByRole('table');
  if (source === 'tile') await fireEvent.click(screen.getByRole('button', { name: /^Keys popped/ }));
  else await fireEvent.change(screen.getByRole('combobox'), { target: { value: 'key_popped' } });
  await flush(); await fireEvent.click(screen.getByRole('button', { name: 'Hide key pops' })); await flush();
  expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ kind: null, kinds: [], exclude_kinds: ['key_popped'] }));
  expect(screen.getByRole('button', { name: /^All alerts/ }).getAttribute('aria-pressed')).toBe('true');
});
it('debounces search by 250ms without rereading summaries and releases rows once', async () => {
  vi.useFakeTimers(); const client = setup(), onRowsReady = vi.fn(); render(AlertsView, { client, onRowsReady, entryKey: 'entry' }); await flush();
  await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'Quenby' } });
  await vi.advanceTimersByTimeAsync(249); expect(client.listAlerts).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1); await flush(); expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'Quenby' }));
  expect(client.getAlertSummary).toHaveBeenCalledTimes(1); expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('entry');
});
it('orders page, range summary, all-time summary, then settings', async () => {
  const client = setup(), order: string[] = [], list = client.listAlerts, settings = client.querySettings;
  client.listAlerts = vi.fn(async filter => { order.push('page'); return list(filter); });
  const pending = deferred<HistoryReply<AlertSummary>>();
  client.getAlertSummary = vi.fn(async request => { order.push(request.range ? 'range' : 'all'); return request.range ? pending.promise : { status: 'ready' as const, value: summary }; });
  client.querySettings = vi.fn(async () => { order.push('settings'); return settings(); });
  render(AlertsView, { client, savedState: { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), range: { start: '0', end: '100' } } } });
  await waitFor(() => expect(order).toEqual(['page', 'range'])); pending.resolve({ status: 'ready' as const, value: summary });
  await waitFor(() => expect(order).toEqual(['page', 'range', 'all', 'settings']));
  await fireEvent.click(screen.getByRole('button', { name: '7 days' })); await flush();
  expect(order).toEqual(['page', 'range', 'all', 'settings', 'page', 'range']);
});
it('shows typed details and estimated sources, keeping stored explanation debug-only', async () => {
  const client = setup(); client.listAlerts = vi.fn(async () => ({ status: 'ready' as const, value: { rows: [alert({ kind: 'white_bag', explanation: 'Synthetic debug evidence', details: { type: 'white_bag', drop_id: 'drop-1', bag_type: 200, ordinal: null, window: null, source_enemy: null, source_estimated: true, contents_known: false } })], next: null } }));
  render(AlertsView, { client }); const panel = await screen.findByRole('region', { name: 'Alert details' });
  expect(within(panel).getByText('est.')).toBeTruthy(); expect(within(panel).getByText('Unseen')).toBeTruthy();
  expect(within(panel).getByRole('link', { name: /View run/ }).getAttribute('href')).toBe('#/runs/run-1');
  expect(panel.querySelector('a[href^="#/loot"]')).toBeNull();
  expect(screen.queryByText('Synthetic debug evidence')).toBeNull(); setDebugMode(true); await tick(); expect(screen.getByText('Synthetic debug evidence')).toBeTruthy();
});
it('normalizes legacy windows and persists only new alert selection/filter state', async () => {
  const client = setup(), onStateChange = vi.fn(), savedState = { ...emptyListState(), alertsPosition: { filter: { kind: null, range: null, query: '', cursor: null, limit: 50 }, previous: [], window: 3 } };
  expect(normalizeListState(savedState).alertsPosition?.window).toBe(3);
  render(AlertsView, { client, savedState, onStateChange }); await screen.findByRole('table');
  await fireEvent.click(document.querySelector('[data-row-id="row-8"]')!);
  expect(onStateChange.mock.lastCall![0].alertsPosition).toMatchObject({ tile: 'all', selectedId: 'row-8' });
  expect(onStateChange.mock.lastCall![0].alertsPosition).not.toHaveProperty('window');
});
it.each(['events', 'keys'] as const)('shares range with %s through Back and Forward', async view => {
  const client = setup(), router = createRouter(fakeHistory(`#/logs/${view}`).adapter, undefined, 'synthetic');
  const range = { start: '1', end: '100' }; router.save({ logsPosition: { ...defaultLogsPosition(), range } });
  router.navigate({ kind: 'logs', view: 'alerts', channel: 'all' });
  render(AlertsView, { client, savedState: router.state.entry, onStateChange: patch => router.save(patch) }); await screen.findByRole('table');
  expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ range }));
  await fireEvent.click(screen.getByRole('button', { name: 'All time' })); await flush();
  expect(router.state.entry.logsPosition?.range).toBeNull(); router.back(); expect(router.state.entry.logsPosition?.range).toEqual(range); router.forward(); expect(router.state.entry.logsPosition?.range).toBeNull(); router.dispose();
});
it('defers older-page invalidations and labels the new-alert pill without a misleading count', async () => {
  const client = setup(); const view = render(AlertsView, { client, required: '1', streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } });
  await screen.findByRole('table'); await fireEvent.click(screen.getByRole('button', { name: 'Older alerts' })); await flush();
  await view.rerender({ required: '2', pendingCount: 23 }); await flush(); expect(client.listAlerts).toHaveBeenCalledTimes(2);
  const pill = screen.getByRole('button', { name: /New alerts/i }); expect(pill.textContent).not.toContain('23'); await fireEvent.click(pill); await flush();
  expect(client.listAlerts).toHaveBeenCalledTimes(3); expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ cursor: null }));
});

it('mutes and unmutes a dungeon through the shared context in both preference modes', async () => {
  const client = setup(); const original = await client.querySettings();
  let settings = { ...original, alerts: { ...original.alerts, key_pop_dungeons: null as string[] | null, key_pop_excluded_dungeons: [] as string[] } };
  client.querySettings = vi.fn(async () => settings);
  client.updateAlertPreferences = vi.fn(async alerts => { settings = { ...settings, alerts }; });
  client.listAlerts = vi.fn(async () => ({ status: 'ready' as const, value: { rows: [alert({ kind: 'key_popped', details: { type: 'key_popped', dungeon: 'Invented vault' } })], next: null } }));
  const session = createAlertSession(client), mute = createDungeonMute(session, () => true); await session.refresh();
  const view = render(AlertsView, { props: { client }, context: new Map([[DUNGEON_MUTE, mute]]) });
  await fireEvent.click(await screen.findByRole('button', { name: 'Mute Invented vault' }));
  await waitFor(() => expect(settings.alerts.key_pop_excluded_dungeons).toEqual(['Invented vault']));
  await fireEvent.click(await screen.findByRole('button', { name: 'Unmute Invented vault' }));
  await waitFor(() => expect(settings.alerts.key_pop_excluded_dungeons).toEqual([]));
  settings = { ...settings, alerts: { ...settings.alerts, key_pop_dungeons: [] } }; await session.refresh();
  await fireEvent.click(await screen.findByRole('button', { name: 'Unmute Invented vault' }));
  await waitFor(() => expect(settings.alerts.key_pop_dungeons).toEqual(['Invented vault']));
  view.unmount(); mute.dispose(); session.dispose();
});
it('empty history offers settings and unknown KPIs', async () => {
  const client = setup(); client.listAlerts = vi.fn(async () => ({ status: 'ready' as const, value: { rows: [], next: null } }));
  client.getAlertSummary = vi.fn(async () => ({ status: 'ready' as const, value: { total: 0, by_kind: [], last_white: null, first_at: null } }));
  render(AlertsView, { client }); await screen.findByRole('heading', { name: 'No alerts' });
  expect(screen.queryByText(/alerts · since/)).toBeNull(); expect(screen.getAllByRole('link', { name: 'Alert settings' })).toHaveLength(2);
  expect([...document.querySelectorAll('.kpis strong')].every(node => node.textContent === '—')).toBe(true);
});
it('restores focus to the heading when a refreshed alert is removed', async () => {
  const client = setup(), onStateChange = vi.fn();
  const main = document.createElement('main'); main.innerHTML = '<h1 tabindex="-1">Logs</h1>'; document.body.append(main);
  const view = render(AlertsView, { props: { client, onStateChange, required: '1', streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } }, target: main });
  await screen.findByRole('table'); const row = document.querySelector<HTMLElement>('[data-row-id="row-0"]')!; row.focus();
  client.listAlerts = vi.fn(async () => ({ status: 'ready' as const, value: { rows: [alert({ id: 'replacement' })], next: null } }));
  await view.rerender({ required: '2' }); await flush();
  expect(document.activeElement).toBe(main.querySelector('h1')); expect(screen.getByText('Alert removed. Heading focused.')).toBeTruthy();
  expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ focusedRowId: null })); view.unmount(); main.remove();
});

it('retains an in-flight summary when the kind changes', async () => {
  const client = setup(), pending = deferred<HistoryReply<AlertSummary>>();
  client.getAlertSummary = vi.fn(() => pending.promise); render(AlertsView, { client });
  await screen.findByRole('table'); await waitFor(() => expect(client.getAlertSummary).toHaveBeenCalledTimes(1));
  await fireEvent.click(screen.getByRole('button', { name: /^White bags/ })); await flush();
  pending.resolve({ status: 'ready', value: summary }); await flush();
  expect(client.getAlertSummary).toHaveBeenCalledTimes(1); expect(client.listAlerts).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('button', { name: /^White bags/ }).textContent).toContain('10');
});

it('defers refresh while the table is scrolled even when the main pane is at the top', async () => {
  const client = setup(), main = document.createElement('main'); document.body.append(main);
  const view = render(AlertsView, { props: { client, required: '1', streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } }, target: main });
  await screen.findByRole('table'); const table = main.querySelector<HTMLElement>('.scroll')!; table.scrollTop = 80; await fireEvent.scroll(table);
  await view.rerender({ required: '2', pendingCount: 1 }); await flush(); expect(client.listAlerts).toHaveBeenCalledTimes(1);
  await fireEvent.click(screen.getByRole('button', { name: /New alerts/i })); await flush(); expect(client.listAlerts).toHaveBeenCalledTimes(2); expect(table.scrollTop).toBe(0);
  view.unmount(); main.remove();
});

it('retires rendered alerts while disabled and does not read until re-enabled', async () => {
  const client = setup(), view = render(AlertsView, { client }); await screen.findByRole('table');
  await view.rerender({ enabled: false }); await flush(); expect(screen.queryByRole('table')).toBeNull(); expect(client.listAlerts).toHaveBeenCalledTimes(1);
  await view.rerender({ enabled: true }); await screen.findByRole('table'); expect(client.listAlerts).toHaveBeenCalledTimes(2);
});
it('retains saved rows and Retry during storage failure without bypassing its checkpoint', async () => {
  const client = setup(), store = { state: 'ready' as const, stream_id: 'live-1', durable: '100', queued: 0, message: '' };
  const view = render(AlertsView, { client, required: '1', streamId: 'live-1', store }); await screen.findByRole('table');
  await view.rerender({ store: { ...store, state: 'failed' } }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush(); expect(client.listAlerts).toHaveBeenCalledTimes(1); expect(screen.getByRole('table')).toBeTruthy();
  await view.rerender({ store }); await flush(); expect(client.listAlerts).toHaveBeenCalledTimes(2);
});

it.each(['foo ', ' foo '])('preserves typed spaces during debounced search: %j', async text => {
  vi.useFakeTimers(); const client = setup(); render(AlertsView, { client }); await flush();
  const input = screen.getByRole('searchbox') as HTMLInputElement;
  await fireEvent.input(input, { target: { value: text } }); await vi.advanceTimersByTimeAsync(250); await flush();
  expect(input.value).toBe(text); expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'foo' }));
  await fireEvent.input(input, { target: { value: `${input.value}vault` } }); await vi.advanceTimersByTimeAsync(250); await flush();
  expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'foo vault' }));
});
it.each(['tile', 'kind'])('clears Hide key pops when key pops are selected by %s', async control => {
  const client = setup(); render(AlertsView, { client }); await screen.findByRole('table');
  const hide = screen.getByRole('button', { name: 'Hide key pops' }); await fireEvent.click(hide); await flush();
  expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ exclude_kinds: ['key_popped'] }));
  if (control === 'tile') await fireEvent.click(screen.getByRole('button', { name: /^Keys popped/ }));
  else await fireEvent.change(screen.getByRole('combobox'), { target: { value: 'key_popped' } });
  await flush(); expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ kind: 'key_popped', exclude_kinds: [] }));
  expect(hide.getAttribute('aria-pressed')).toBe('false');
});
it.each(['white_bag', 'death', 'flawless_run'] as const)('formats %s panel time and avoids a duplicate heading', kind => {
  vi.useFakeTimers(); const now = new Date(2030, 0, 1, 20, 59); vi.setSystemTime(now);
  render(AlertPanel, { alert: alert({ kind, at: String(now.getTime() * 1000), details: kind === 'white_bag' ? {
    type: 'white_bag', drop_id: 'drop-1', bag_type: 200, ordinal: 1, source_enemy: null, source_estimated: false, contents_known: false,
    window: { start: String(new Date(2030, 0, 1).getTime() * 1000), end: String(new Date(2030, 0, 8).getTime() * 1000) },
  } : null }) });
  const panel = screen.getByRole('region', { name: 'Alert details' });
  expect(within(panel).getByText('Today · 8:59 PM')).toBeTruthy(); expect(panel.querySelector('strong')).toBeNull();
  if (kind === 'white_bag') expect(within(panel).getByText('Jan 1 – Jan 7')).toBeTruthy();
});
it('applies the white-bag and key-pop kind colors to table glyphs', async () => {
  const client = setup(); client.listAlerts = vi.fn(async () => ({ status: 'ready' as const, value: {
    rows: [alert({ id: 'white', kind: 'white_bag', details: null }), alert({ id: 'key', kind: 'key_popped', details: null })], next: null,
  } }));
  render(AlertsView, { client }); await screen.findByRole('table');
  expect(document.querySelector('[data-row-id="white"] .k.white .g.bag')).toBeTruthy();
  expect(document.querySelector('[data-row-id="key"] .k.key .g.portal')).toBeTruthy();
});

it.each(['foo bar', 'x'.repeat(129)])('preserves a pending or rejected search when a tile changes: %j', async text => {
  vi.useFakeTimers(); const client = setup(); render(AlertsView, { client }); await flush();
  const input = screen.getByRole('searchbox') as HTMLInputElement;
  await fireEvent.input(input, { target: { value: 'foo' } }); await vi.advanceTimersByTimeAsync(250); await flush();
  expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'foo' }));
  await fireEvent.input(input, { target: { value: text } });
  await vi.advanceTimersByTimeAsync(text.length > 128 ? 250 : 100); await flush();
  if (text.length > 128) expect(screen.getByText('Invalid search.')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: /^White bags/ })); await flush();
  expect(input.value).toBe(text);
  expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ kind: 'white_bag', query: 'foo' }));
  await vi.advanceTimersByTimeAsync(250); await flush();
  expect(input.value).toBe(text);
  if (text.length <= 128) expect(client.listAlerts).toHaveBeenLastCalledWith(expect.objectContaining({ kind: 'white_bag', query: 'foo bar' }));
  else expect(screen.getByText('Invalid search.')).toBeTruthy();
});
