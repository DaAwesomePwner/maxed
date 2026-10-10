import { expect, it } from 'vitest';
import { eventLabel } from '../../src/lib/logs/presentation';
import type { InstanceEventView } from '../../src/lib/bindings/InstanceEventView';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, vi } from 'vitest';
import EventsView from '../../src/lib/logs/EventsView.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import fixture from '../fixtures/t0-live';
import type { EventFilter } from '../../src/lib/bindings/EventFilter';
import EventPortal from '../../src/lib/logs/EventPortal.svelte';
import RunCard from '../../src/lib/logs/RunCard.svelte';
import { defaultLogsPosition, emptyListState, type ListState } from '../../src/lib/navigation/state';
import { tick } from 'svelte';
import { story } from '../runs/fixtures';
import { createRouter } from '../../src/lib/navigation/history';
import { fakeHistory } from '../navigation/fake-history';
import type { EventSummary } from '../../src/lib/bindings/EventSummary';
const base = { id: 'i', session_id: 's', run_id: null, map: 'Realm', at: '1', provisional: false };
it('labels each event kind with names and fallbacks', () => {
  expect(eventLabel({ ...base, kind: { type: 'player_death', player_name: 'SynthAlpha', level: 20, killed_by: 'Synthetic Boss', grave_type: null, object_id: null, class_id: null, is_local: false } } as InstanceEventView))
    .toBe('SynthAlpha died to Synthetic Boss');
  expect(eventLabel({ ...base, provisional: true, kind: { type: 'player_left', object_id: 1, player_name: null, class_id: null } } as InstanceEventView))
    .toBe('Unknown player left (unconfirmed)');
  expect(eventLabel({ ...base, kind: { type: 'portal_opened', opener: 'SynthBravo', portal_type: null, dungeon: 'Lost Halls' } } as InstanceEventView))
    .toBe('SynthBravo opened Lost Halls');
  expect(eventLabel({ ...base, kind: { type: 'realm_event_killed', boss_key: 'Synthetic Boss', object_type: null, killer: null } } as InstanceEventView))
    .toBe('Synthetic Boss killed');
});

afterEach(() => { cleanup(); vi.useRealTimers(); });
const cursor = { at: '1', id: 'i' };
function setup() {
  const listInstanceEvents = vi.fn((filter: EventFilter) => ({ rows: [{ ...base, id: filter.cursor ? 'older' : 'i',
    kind: { type: 'player_left' as const, object_id: 1, player_name: filter.cursor ? 'SynthBravo' : 'SynthAlpha' } }], next: filter.cursor ? null : cursor }));
  const client = createClient(createFixtureTransport(fixture, undefined, undefined, undefined, undefined, undefined, undefined, undefined, undefined, { listInstanceEvents }));
  client.listCatalogDungeons = vi.fn(async () => ['Lost Halls']);
  return { client, listInstanceEvents };
}
it('filters events within a run, pages and persists the cursor stack', async () => {
  const { client, listInstanceEvents } = setup();
  const onStateChange = vi.fn(), onRowsReady = vi.fn();
  render(EventsView, { client, runId: 'run-1', onStateChange, onRowsReady, entryKey: 'events' });
  await screen.findByRole('row', { name: /SynthAlpha left/ });
  expect(listInstanceEvents).toHaveBeenLastCalledWith({ kinds: [], run_id: 'run-1', dungeon: null, cursor: null, limit: 50, range: null, query: '' });
  await fireEvent.click(screen.getByRole('button', { name: 'Older events' }));
  await screen.findByRole('row', { name: /SynthBravo left/ });
  expect(onStateChange.mock.lastCall![0].logsPosition.events.previous).toEqual([null]);
  await fireEvent.click(screen.getByRole('button', { name: /^Deaths / }));
  await waitFor(() => expect(listInstanceEvents).toHaveBeenLastCalledWith({ kinds: ['player_death'], run_id: 'run-1', dungeon: null, cursor: null, limit: 50, range: null, query: '' }));
  expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('events');
});
async function flush() { for (let i = 0; i < 8; i++) { await Promise.resolve(); await tick(); } }

it.each([EventsView])('shows an empty feed and does no reads while disabled', async Component => {
  const { client, listInstanceEvents } = setup();
  listInstanceEvents.mockReturnValue({ rows: [], next: null });
  const onRowsReady = vi.fn();
  const view = render(Component, { client, enabled: false, onRowsReady, entryKey: 'empty' });
  await flush(); expect(listInstanceEvents).not.toHaveBeenCalled(); expect(onRowsReady).not.toHaveBeenCalled();
  await view.rerender({ enabled: true });
  await screen.findByRole('heading', { name: 'No events yet' });
  expect(onRowsReady).toHaveBeenCalledExactlyOnceWith('empty');
});
it('restores event pages and keeps saved chat position when changing kinds', async () => {
  const { client, listInstanceEvents } = setup();
  const chat = { filter: { channel: null, sender: null, query: 'synthetic', run_id: null, show_npc: false, show_hidden: false, cursor: null, limit: 50 }, previous: [] };
  const savedState: ListState = { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), chat,
    events: { filter: { kinds: ['player_left'], run_id: 'run-1', dungeon: null, cursor, limit: 50 }, previous: [null] } } };
  const onStateChange = vi.fn();
  const view = render(EventsView, { client, runId: 'run-1', savedState, onStateChange });
  await screen.findByRole('row', { name: /SynthBravo left/ });
  expect(listInstanceEvents.mock.lastCall![0].cursor).toEqual(cursor);
  await fireEvent.click(screen.getByRole('button', { name: /^Realm kills / }));
  await waitFor(() => expect(listInstanceEvents.mock.lastCall![0].kinds).toEqual(['realm_event_killed']));
  expect(onStateChange.mock.lastCall![0].logsPosition.chat).toEqual(chat);
  view.unmount();
  render(EventsView, { client, runId: 'run-2', savedState });
  await waitFor(() => expect(listInstanceEvents).toHaveBeenLastCalledWith({ kinds: [], run_id: 'run-2', dungeon: null, cursor: null, limit: 50, range: null, query: '' }));
});
it('restores a legacy Events entry without range, search, selection or separate key-pop state', async () => {
  const { client, listInstanceEvents } = setup();
  const savedState = { ...emptyListState(), logsPosition: {
    chat: defaultLogsPosition().chat,
    events: { filter: { kinds: ['player_left'], run_id: null, dungeon: null, cursor, limit: 50 }, previous: [null] },
  } } as ListState;
  render(EventsView, { client, savedState });
  const row = await screen.findByRole('row', { name: /SynthBravo left/ });
  expect(row.getAttribute('aria-selected')).toBe('true');
  expect(listInstanceEvents.mock.lastCall![0]).toEqual({ kinds: ['player_left'], run_id: null, dungeon: null, cursor, limit: 50, range: null, query: '' });
});
it('defers new events on older pages until requested', async () => {
  const { client, listInstanceEvents } = setup();
  const view = render(EventsView, { client, required: '1', streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } });
  await screen.findByRole('row', { name: /SynthAlpha left/ });
  await fireEvent.click(screen.getByRole('button', { name: 'Older events' }));
  await screen.findByRole('row', { name: /SynthBravo left/ });
  await view.rerender({ required: '2', pendingCount: 2 });
  await screen.findByRole('button', { name: /new events/i });
  expect(listInstanceEvents).toHaveBeenCalledTimes(2);
  await fireEvent.click(screen.getByRole('button', { name: /new events/i }));
  await screen.findByRole('row', { name: /SynthAlpha left/ });
  expect(listInstanceEvents.mock.lastCall![0].cursor).toBeNull();
});
it('trims event searches before reading history', async () => {
  const { client, listInstanceEvents } = setup();
  const position = defaultLogsPosition(); position.events.filter.query = ' Quenby ';
  render(EventsView, { client, savedState: { ...emptyListState(), logsPosition: position } });
  await screen.findByRole('table');
  expect(listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'Quenby' }));
  await fireEvent.input(screen.getByRole('textbox', { name: 'Find a player or boss' }), { target: { value: ' Vexlin ' } });
  await waitFor(() => expect(listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'Vexlin' })));
});
it('defers unrelated checkpoints without a pill and omits range counts', async () => {
  const { client, listInstanceEvents } = setup();
  const position = defaultLogsPosition(); position.range = { start: '0', end: '10000' };
  position.events.filter.cursor = cursor;
  const view = render(EventsView, { client, savedState: { ...emptyListState(), logsPosition: position }, required: '1', streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } });
  await screen.findByRole('table');
  await view.rerender({ required: '2', pendingCount: 0 });
  expect(screen.queryByRole('button', { name: /new events/i })).toBeNull();
  expect(listInstanceEvents).toHaveBeenCalledTimes(1);
  await view.rerender({ required: '3', pendingCount: 2 });
  expect(screen.getByRole('button', { name: '↑ New events' })).toBeTruthy();
});
it('keeps the production EventPortal lookup latest-wins and releases its listener', async () => {
  const { client } = setup();
  let finishOld!: (value: number | null) => void;
  const release = vi.fn();
  client.onVisualsChanged = vi.fn(() => release);
  client.getDungeonPortalType = vi.fn().mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve; })).mockResolvedValue(2);
  client.getObjectVisuals = vi.fn(async () => ({ client_fingerprint: 'a'.repeat(64), objects: [{ object_type: 2,
    name: 'Synthetic portal', sprite: { key: 'synthetic', sheet: 'mapObjects.png', x: 0, y: 0, width: 8, height: 8 }, bag: null }] }));
  const view = render(EventPortal, { client, dungeon: 'Old dungeon' });
  await flush();
  await view.rerender({ dungeon: 'Invented vault' });
  await screen.findByRole('img', { name: 'Synthetic portal' });
  finishOld(1); await flush();
  expect(client.getObjectVisuals).toHaveBeenCalledExactlyOnceWith([2]);
  expect(screen.getByRole('img', { name: 'Synthetic portal' })).toBeTruthy();
  expect(client.onVisualsChanged).toHaveBeenCalledOnce();
  expect(release).not.toHaveBeenCalled();
  view.unmount(); expect(release).toHaveBeenCalledOnce();
});

it.each([
  ['1450000000', '24:10'], ['86536000000', '24:02:16'], [null, '—'],
] as const)('formats run duration %s with the shared elapsed formatter', (ended, expected) => {
  const value = story(); value.run = { ...value.run, started: '0', ended, outcome: 'left' };
  render(RunCard, { runId: value.run.id, story: value, chat: null });
  expect(screen.getByRole('region', { name: 'Run' }).querySelector('p')?.textContent).toContain(` · ${expected} · Left`);
});

const summary: EventSummary = { total: 42, by_kind: { player_death: 2, player_left: 30, realm_event_killed: 7, portal_opened: 3 }, dungeons: [], local_pops: 0, last_local_pop: null, flagged_since: null, first_at: '1' };
it.each(['kind', 'range', 'page'] as const)('preserves pending typed search when the %s control changes', async control => {
  vi.useFakeTimers();
  const { client, listInstanceEvents } = setup();
  render(EventsView, { client }); await flush();
  const input = screen.getByRole('textbox', { name: 'Find a player or boss' });
  await fireEvent.input(input, { target: { value: 'Kestrel' } });
  await vi.advanceTimersByTimeAsync(100);
  if (control === 'kind') await fireEvent.click(screen.getByRole('button', { name: /^Deaths / }));
  else if (control === 'range') await fireEvent.click(screen.getByRole('button', { name: '7 days' }));
  else await fireEvent.click(screen.getByRole('button', { name: 'Older events' }));
  await flush();
  expect((input as HTMLInputElement).value).toBe('Kestrel');
  expect(listInstanceEvents.mock.lastCall![0].query).toBe('');
  await vi.advanceTimersByTimeAsync(250); await flush();
  expect(listInstanceEvents.mock.lastCall![0]).toMatchObject({ query: 'Kestrel', cursor: null });
  if (control === 'kind') expect(listInstanceEvents.mock.lastCall![0].kinds).toEqual(['player_death']);
  if (control === 'range') expect(listInstanceEvents.mock.lastCall![0].range).not.toBeNull();
});

it.each([
  ['range', 'error'], ['range', 'busy'], ['all', 'error'], ['all', 'busy'], ['reuse', 'error'], ['reuse', 'busy'],
] as const)('loads run and chat after the %s summary settles with %s', async (scope, failure) => {
  vi.useFakeTimers();
  const { client } = setup(), order: string[] = [];
  client.listInstanceEvents = vi.fn(async () => ({ status: 'ready' as const, value: {
    rows: [{ ...base, run_id: 'run-1', kind: { type: 'player_left' as const, object_id: 1, player_name: 'Kestrel' } }], next: null,
  } }));
  client.getEventSummary = vi.fn(async request => {
    const key = request.filter.range ? 'range' : 'all'; order.push(key);
    if (scope === 'reuse' || key === scope) {
      if (failure === 'error') throw new Error('Synthetic summary failure');
      return { status: 'busy' as const };
    }
    return { status: 'ready' as const, value: summary };
  });
  client.getRun = vi.fn(async () => { order.push('run'); return { status: 'ready' as const, value: story() }; });
  client.getChatOverview = vi.fn(async () => { order.push('chat'); return { status: 'ready' as const, value: {
    lines: 38, by_channel: { public: 38, guild: 0, party: 0, whisper: 0, unknown: 0 }, first_at: '1', latest_at: '2',
  } }; });
  render(EventsView, { client, runId: 'run-1', savedState: { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), range: scope === 'reuse' ? null : { start: '0', end: '10' } } } });
  await flush(); await vi.advanceTimersByTimeAsync(0); await flush();
  expect(client.getRun).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(150); await flush();
  const summaries = scope === 'reuse' ? ['all'] : ['range', 'all'];
  const expected = summaries.flatMap(key => failure === 'busy' && (scope === 'reuse' || key === scope) ? [key, key] : [key]);
  expect(order).toEqual([...expected, 'run', 'chat']);
  expect(within(screen.getByRole('region', { name: 'Run' })).getByText('38')).toBeTruthy();
  expect(screen.getByRole('link', { name: /^Remove run filter:/ }).textContent).toContain('Invented Sanctum');
});
it('shows segment counts, filters portals on the server, and debounces search without refetching counts', async () => {
  vi.useFakeTimers();
  const { client, listInstanceEvents } = setup();
  client.getEventSummary = vi.fn(async () => ({ status: 'ready' as const, value: summary }));
  client.listInstanceEvents = vi.fn(async filter => ({ status: 'ready' as const, value: { rows: [{ ...base, kind: { type: 'portal_opened' as const, opener: 'Morrow', dungeon: 'Invented vault' } }], next: null } }));
  client.getDungeonPortalType = vi.fn(async () => null);
  render(EventsView, { client });
  await flush();
  expect(screen.getByRole('button', { name: 'All 42' })).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Key pops 3' }));
  await flush();
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ kinds: ['portal_opened'] }));
  expect(client.getDungeonPortalType).toHaveBeenCalledWith('Invented vault');
  expect(screen.getByRole('row', { name: /Morrow opened Invented vault/ })).toBeTruthy();
  await fireEvent.input(screen.getByRole('textbox', { name: 'Find a player or boss' }), { target: { value: 'Morrow' } });
  await vi.advanceTimersByTimeAsync(249); await flush();
  expect(client.listInstanceEvents).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1); await flush();
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'Morrow' }));
  expect(client.getEventSummary).toHaveBeenCalledTimes(1);
  expect(client.getEventSummary).toHaveBeenCalledWith({ filter: expect.objectContaining({ query: '' }) });
  expect(listInstanceEvents).not.toHaveBeenCalled();
});

it('serializes page, range summary, all-time summary, run and chat reads and reuses the selected run', async () => {
  vi.useFakeTimers();
  const { client } = setup(), order: string[] = [];
  const death: InstanceEventView = { ...base, run_id: 'run-1', map: 'Invented vault', provisional: true,
    kind: { type: 'player_death', player_name: 'Kestrel', level: 20, class_id: 1, killed_by: 'Lantern Colossus', grave_type: 1, is_local: false } };
  const left: InstanceEventView = { ...base, id: 'left', run_id: 'run-1', kind: { type: 'player_left', object_id: 1, player_name: 'Quenby' } };
  client.listInstanceEvents = vi.fn(async () => { order.push('page'); return { status: 'ready' as const, value: { rows: [death, left], next: null } }; });
  client.getEventSummary = vi.fn(async request => { order.push(request.filter.range ? 'range' : 'all'); return { status: 'ready' as const, value: summary }; });
  client.getRun = vi.fn(async () => { order.push('run'); return { status: 'ready' as const, value: story({ events: [death, left] }) }; });
  client.getChatOverview = vi.fn(async () => { order.push('chat'); return { status: 'ready' as const, value: { lines: 38, by_channel: { public: 38, guild: 0, party: 0, whisper: 0, unknown: 0 }, first_at: '1', latest_at: '2' } }; });
  client.getObjectVisuals = vi.fn(async () => ({ client_fingerprint: 'a'.repeat(64), objects: [{ object_type: 1, name: 'Synthetic Rogue', sprite: null, bag: null }] }));
  render(EventsView, { client, runId: 'run-1', savedState: { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), range: { start: '0', end: '10' } } } });
  await flush();
  expect(order).toEqual(['page', 'range', 'all']);
  await vi.advanceTimersByTimeAsync(150); await flush();
  expect(order).toEqual(['page', 'range', 'all', 'run', 'chat']);
  const panel = within(screen.getByRole('region', { name: 'Event details' }));
  for (const value of ['Kestrel', '20', 'Synthetic Rogue', 'Lantern Colossus', 'Invented vault', 'Seen', 'No', 'unconfirmed']) expect(panel.getByText(value)).toBeTruthy();
  const card = within(screen.getByRole('region', { name: 'Run' }));
  expect(card.getByText('38')).toBeTruthy();
  expect(card.getByRole('link', { name: 'Events in this run' }).getAttribute('href')).toBe('#/logs/events/run/run-1');
  expect(card.getByRole('link', { name: 'Chat in this run' }).getAttribute('href')).toBe('#/logs/run/run-1');
  expect(card.getByRole('link', { name: 'Open run ›' }).getAttribute('href')).toBe('#/runs/run-1');
  expect(screen.getByRole('link', { name: /^Remove run filter:/ }).getAttribute('href')).toBe('#/logs/events');
  expect(screen.getByRole('link', { name: 'Chat in this run ›' })).toBeTruthy();
  await fireEvent.click(screen.getByRole('row', { name: /Quenby left/ }));
  await vi.advanceTimersByTimeAsync(500); await flush();
  expect(client.getRun).toHaveBeenCalledTimes(1); expect(client.getChatOverview).toHaveBeenCalledTimes(1);
});

it('persists the shared range between Events and Key pops and restores it through Back and Forward', async () => {
  const { client, listInstanceEvents } = setup();
  const router = createRouter(fakeHistory('#/logs/events').adapter, undefined, 'synthetic');
  const view = render(EventsView, { client, savedState: router.state.entry, onStateChange: patch => router.save(patch) });
  await screen.findByRole('row', { name: /SynthAlpha left/ });
  await fireEvent.click(screen.getByRole('button', { name: '7 days' }));
  await waitFor(() => expect(listInstanceEvents.mock.lastCall![0].range).toBeTruthy());
  const range = router.state.entry.logsPosition!.range;
  router.navigate({ kind: 'logs', view: 'keys', channel: 'all' });
  expect(router.state.entry.logsPosition!.range).toEqual(range);
  router.save({ logsPosition: { ...router.state.entry.logsPosition!, range: null } });
  router.back(); expect(router.state.entry.logsPosition!.range).toEqual(range);
  router.forward(); expect(router.state.entry.logsPosition!.range).toBeNull();
  view.unmount(); router.dispose();
});

it('keeps query and selection in saved state and shows a filtered new-data pill without a count', async () => {
  const { client } = setup();
  const onStateChange = vi.fn();
  const savedState = { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), events: {
    filter: { kinds: ['player_left'] as EventFilter['kinds'], run_id: null, dungeon: null, cursor, limit: 50, query: 'Synth' }, previous: [null], selectedId: 'older',
  } } };
  const view = render(EventsView, { client, savedState, onStateChange, required: '1', pendingCount: 12, streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } });
  await screen.findByRole('row', { name: /SynthBravo left/ });
  expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe('Synth');
  await fireEvent.click(screen.getByRole('row', { name: /SynthBravo left/ }));
  expect(onStateChange.mock.lastCall![0].logsPosition.events.selectedId).toBe('older');
  await view.rerender({ required: '2' });
  expect(await screen.findByRole('button', { name: '↑ New events' })).toBeTruthy();
});

it('offers Show all time for an empty range and retries errors without losing filters', async () => {
  const { client } = setup();
  client.listInstanceEvents = vi.fn().mockRejectedValueOnce(new Error('synthetic failure')).mockResolvedValue({ status: 'ready', value: { rows: [], next: null } });
  render(EventsView, { client, savedState: { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), range: { start: '0', end: '10' } } } });
  await screen.findByText("Couldn't load events. Your filters are kept.");
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
  await screen.findByRole('heading', { name: 'No events in this range' });
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ range: { start: '0', end: '10' } }));
  await fireEvent.click(screen.getByRole('button', { name: 'Show all time' }));
  await screen.findByRole('heading', { name: 'No events yet' });
  expect(client.listInstanceEvents).toHaveBeenLastCalledWith(expect.objectContaining({ range: null }));
  expect(screen.queryByText(/events seen/)).toBeNull();
});

it('refreshes at the head and restores focus to the heading when the focused event disappears', async () => {
  vi.useFakeTimers();
  const { client, listInstanceEvents } = setup(), onStateChange = vi.fn();
  const main = document.createElement('main'), heading = document.createElement('h1');
  heading.textContent = 'Logs'; heading.tabIndex = -1; main.append(heading); document.body.append(main);
  const target = document.createElement('div'); main.append(target);
  const view = render(EventsView, { target, props: { client, onStateChange, required: '1', streamId: 'live-1', store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } } });
  await flush();
  screen.getByRole('row', { name: /SynthAlpha left/ }).focus();
  listInstanceEvents.mockReturnValue({ rows: [], next: null });
  await view.rerender({ required: '2' });
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(document.activeElement).toBe(heading);
  expect(screen.getByText('Row Removed')).toBeTruthy();
  expect(onStateChange).toHaveBeenCalledWith(expect.objectContaining({ focusedRowId: null }));
  expect(screen.queryByRole('button', { name: /new events/i })).toBeNull();
  view.unmount(); main.remove();
});

it('keeps trailing spaces without repeated persistence, selection or paging resets', async () => {
  vi.useFakeTimers(); const { client, listInstanceEvents } = setup();
  const onStateChange = vi.fn();
  const view = render(EventsView, { client, onStateChange }); await flush();
  const input = screen.getByRole('textbox', { name: 'Find a player or boss' }) as HTMLInputElement;
  await fireEvent.input(input, { target: { value: 'Vexlin' } });
  await vi.advanceTimersByTimeAsync(250); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Older events' })); await flush();
  const row = view.container.querySelector<HTMLElement>('[data-row-id]')!;
  await fireEvent.click(row); await flush();
  const saved = onStateChange.mock.lastCall![0].logsPosition.events;
  expect(saved.previous).toHaveLength(1);
  expect(saved.selectedId).toBe(row.dataset.rowId);
  const writes = onStateChange.mock.calls.length, reads = listInstanceEvents.mock.calls.length;
  await fireEvent.input(input, { target: { value: 'Vexlin ' } });
  await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(input.value).toBe('Vexlin ');
  expect(onStateChange).toHaveBeenCalledTimes(writes);
  expect(listInstanceEvents).toHaveBeenCalledTimes(reads);
  expect(row.getAttribute('aria-selected')).toBe('true');
  expect(onStateChange.mock.lastCall![0].logsPosition.events).toEqual(saved);
  await fireEvent.input(input, { target: { value: 'Invented ' } });
  await vi.advanceTimersByTimeAsync(500); await flush();
  expect(input.value).toBe('Invented ');
  expect(listInstanceEvents.mock.lastCall![0].query).toBe('Invented');
  await fireEvent.input(input, { target: { value: input.value + 'vault' } });
  await vi.advanceTimersByTimeAsync(500); await flush();
  expect(listInstanceEvents.mock.lastCall![0].query).toBe('Invented vault');
});
