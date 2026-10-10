import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Logs from '../../src/routes/Logs.svelte';
import type { ChatRow } from '../../src/lib/bindings/ChatRow';
import type { ChatFilter } from '../../src/lib/bindings/ChatFilter';
import type { WhisperPartnerFilter } from '../../src/lib/bindings/WhisperPartnerFilter';
import type { WhisperPartner } from '../../src/lib/bindings/WhisperPartner';
import { defaultLogsPosition, emptyListState } from '../../src/lib/navigation/state';
import { createLogsPreview } from '../preview/logs-scenario';
import { advance, flush, deferred } from '../runs/fixtures';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); document.querySelectorAll('main').forEach(node => node.remove()); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const row = (n: number, name = 'Vexlin'): ChatRow => ({ hidden: false, message: {
  id: `synthetic-${n}`, at: String(n * 1_000_000), session_id: 'synthetic', map: 'Nexus', channel: 'whisper',
  sender: n % 2 ? name : 'Lethra', counterpart: n % 2 ? 'Lethra' : name, direction: n % 2 ? 'incoming' : 'outgoing',
  sender_kind: 'player', is_local: n % 2 === 0, stars: 72, supporter: false, text: `message ${n}`, truncated: false,
} });
const people: WhisperPartner[] = ['Vexlin', 'Quenby'].map((name, i) => ({ name, lines: 650, last_at: String((650 - i) * 1_000_000), last_text: `latest from ${name}`, last_direction: 'incoming', stars: 72 }));
function mount(options: { partner?: string; savedPartner?: string; count?: number; stored?: boolean; people?: WhisperPartner[]; runId?: string } = {}) {
  const preview = createLogsPreview('default'), client = preview.client;
  let hidden: string[] = [], count = options.count ?? 650;
  const order: string[] = [];
  const listWhisperPartners = vi.fn(async (filter: WhisperPartnerFilter) => {
    order.push('people');
    const rows = (options.people ?? people).filter(person => (filter.show_hidden || !hidden.includes(person.name)) && (!filter.query || person.name === 'Quenby'));
    return { status: 'ready' as const, value: { rows, people: rows.length, lines: rows.length * count } };
  });
  const listChat = vi.fn(async (filter: ChatFilter) => {
    order.push('thread');
    const last = filter.cursor ? Number(BigInt(filter.cursor.at) / 1_000_000n) - 1 : count;
    const rows = Array.from({ length: Math.min(100, last) }, (_, i) => row(last - i, filter.sender ?? 'Vexlin'));
    return { status: 'ready' as const, value: { rows, next: last > 100 ? { at: rows.at(-1)!.message.at, id: rows.at(-1)!.message.id } : null } };
  });
  client.listWhisperPartners = listWhisperPartners; client.listChat = listChat;
  const querySettings = client.querySettings;
  client.querySettings = async () => { const settings = await querySettings(); settings.chat.store.whisper = options.stored ?? true; return settings; };
  client.updateChatSettings = vi.fn(async settings => { hidden = settings.hidden_senders; });
  const savedState = { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), whisper: { query: '', showHidden: false, partner: options.savedPartner ?? null } } };
  const pane = document.createElement('main'); pane.innerHTML = '<h1 tabindex="-1">Logs</h1>'; document.body.append(pane);
  const onStateChange = vi.fn(), onRowsReady = vi.fn(), onHistoryApplied = vi.fn();
  const route = { kind: 'logs' as const, view: 'chat' as const, channel: 'whisper' as const, partner: options.partner, runId: options.runId };
  const view = render(Logs, { target: pane, props: { client, route, savedState, onStateChange, onRowsReady, onHistoryApplied,
    entryKey: 'entry', streamId: 'synthetic', required: '1', pendingCount: 7,
    store: { state: 'ready', stream_id: 'synthetic', durable: '100', queued: 0, message: '' } } });
  return { ...view, client, listChat, listWhisperPartners, order, onStateChange, onRowsReady, onHistoryApplied, route, savedState, pane, append: (amount = 1) => count += amount };
}
const lines = () => [...document.querySelectorAll<HTMLElement>('[data-row-id]')];
const thread = () => screen.getByRole('region', { name: 'Whisper thread' });
it('never rereads People for non-whisper checkpoints at the bottom or while scrolled', async () => {
  const view = mount(); await flush();
  await view.rerender({ pendingCount: 0, required: '2' }); await advance(2000);
  expect(view.listWhisperPartners).toHaveBeenCalledTimes(1);
  expect(view.listChat).toHaveBeenCalledTimes(1);
  const element = geometry(); element.scrollTop = 0; await fireEvent.scroll(element);
  await view.rerender({ required: '3' }); await advance(2000);
  expect(view.listWhisperPartners).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole('button', { name: /New whispers/ })).toBeNull();
});
it('keeps both live-read identities throttled during a burst of whispers', async () => {
  const view = mount(); await flush();
  await view.rerender({ required: '2', pendingCount: 8 }); await flush();
  expect(view.listWhisperPartners).toHaveBeenCalledTimes(1);
  expect(view.listChat).toHaveBeenCalledTimes(1);
  await advance(999);
  expect(view.listWhisperPartners).toHaveBeenCalledTimes(1);
  await advance(1);
  expect(view.listWhisperPartners).toHaveBeenCalledTimes(2);
  expect(view.listChat).toHaveBeenCalledTimes(2);
  // An unrelated checkpoint must not reuse the still-unacknowledged count.
  await view.rerender({ required: '3' }); await advance(2000);
  expect(view.listWhisperPartners).toHaveBeenCalledTimes(2);
  expect(view.listChat).toHaveBeenCalledTimes(2);
});
it.each([['eNbY', ''], ['message', 'message']])('searches the full exchange for partner substrings: %s', async (query, threadQuery) => {
  const view = mount({ partner: 'Quenby' }); await flush();
  expect(view.listChat).toHaveBeenCalledTimes(1);
  view.onHistoryApplied.mockClear();
  await fireEvent.input(screen.getByRole('searchbox'), { target: { value: query } }); await advance(1250);
  expect(view.listWhisperPartners.mock.lastCall![0].query).toBe(query);
  expect(view.listChat).toHaveBeenCalledTimes(2);
  expect(view.listChat.mock.lastCall![0]).toMatchObject({ sender: 'Quenby', query: threadQuery });
  expect(lines()).toHaveLength(100);
  expect(lines().at(-1)?.textContent).toContain('message 650');
  expect(screen.getByRole('button', { name: 'Load earlier' }).hasAttribute('disabled')).toBe(false);
  expect(view.onHistoryApplied).toHaveBeenCalledWith('entry', '1');
});
it('completes each search reset when successive partner substrings share a thread query', async () => {
  const view = mount({ partner: 'Quenby' }); await flush();
  for (const query of ['message', 'Que', 'Quen', 'eNbY', '']) {
    const reads = view.listChat.mock.calls.length;
    view.onHistoryApplied.mockClear();
    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: query } }); await advance(1250);
    expect(view.listChat).toHaveBeenCalledTimes(reads + 1);
    expect(view.listChat.mock.lastCall![0].query).toBe(query === 'message' ? 'message' : '');
    expect(lines()).toHaveLength(100);
    expect(screen.getByRole('button', { name: 'Load earlier' }).hasAttribute('disabled')).toBe(false);
    expect(view.onHistoryApplied).toHaveBeenCalledWith('entry', '1');
  }
});
it('keeps run-scoped whispers in Chat with the removable run chip', async () => {
  const view = mount({ runId: 'synthetic-run' }); await flush();
  expect(screen.getByRole('table', { name: 'Chat lines' })).toBeTruthy();
  expect(screen.getByRole('link', { name: /^Remove run filter:/ }).getAttribute('href')).toBe('#/logs/chat/whisper');
  expect(screen.getByRole('link', { name: 'Guild' }).getAttribute('href')).toBe('#/logs/chat/guild/run/synthetic-run');
  expect(view.listChat.mock.calls[0][0]).toMatchObject({ channel: 'whisper', run_id: 'synthetic-run' });
  expect(view.listWhisperPartners).not.toHaveBeenCalled();
  expect(screen.queryByRole('region', { name: 'Whisper thread' })).toBeNull();
});
it.each(['Quenby', 'Morrow'])('hiding %s chooses the next person, or the previous at the end', async name => {
  const rows = [...people, { ...people[0], name: 'Morrow' }];
  const view = mount({ partner: name, people: rows }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Hide' })); await flush();
  expect(view.listChat.mock.lastCall![0].sender).toBe(name === 'Quenby' ? 'Morrow' : 'Quenby');
});
it('shows Pick a person after hiding the only person while Hidden is on', async () => {
  mount({ people: [people[0]] }); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Hidden' })); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Hide' })); await flush();
  expect(screen.getByText('Pick a person')).toBeTruthy();
  expect(screen.queryByText('Loading whispers')).toBeNull();
  expect(screen.getByRole('link', { name: /Vexlin.*latest from Vexlin/ })).toBeTruthy();
});
it('offers Clear search when no whispers match the query', async () => {
  const view = mount(); await flush();
  view.listWhisperPartners.mockResolvedValueOnce({ status: 'ready', value: { rows: [], people: 0, lines: 0 } });
  view.listChat.mockResolvedValueOnce({ status: 'ready', value: { rows: [], next: null } });
  await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'absent' } }); await advance(250);
  expect(screen.getByText('No whispers match "absent"')).toBeTruthy();
  expect(screen.queryByText('No whispers yet')).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: 'Clear search' })); await flush();
  expect(view.listWhisperPartners.mock.lastCall![0].query).toBe('');
  expect(view.listChat.mock.lastCall![0].query).toBe('');
});
it('does not expose rejected routes for overlong names, including Chat links at the cap', async () => {
  const name = 'Vexlin'.padEnd(64, 'x');
  mount({ people: [{ ...people[0], name }] }); await flush();
  expect(screen.getByRole('navigation', { name: 'Whisper conversations' }).querySelector('a')).toBeNull();
  expect(screen.getByRole('button', { name: `All chat from ${name}` }).hasAttribute('disabled')).toBe(true);
  for (let i = 0; i < 4; i++) { await fireEvent.click(screen.getByRole('button', { name: 'Load earlier' })); await flush(); }
  expect(screen.queryByRole('link', { name: 'Older lines in Chat ›' })).toBeNull();
  expect(screen.getByRole('button', { name: 'Older lines in Chat ›' }).hasAttribute('disabled')).toBe(true);
});
it.each(['pill', 'bottom'])('resets a deferred thread with over 100 new lines via %s and pages into the gap', async action => {
  const view = mount(); await flush();
  const element = geometry(); element.scrollTop = 0; await fireEvent.scroll(element);
  view.append(150); await view.rerender({ required: '2', pendingCount: 8 }); await flush();
  expect(view.listChat).toHaveBeenCalledTimes(1);
  if (action === 'pill') await fireEvent.click(screen.getByRole('button', { name: '↑ New whispers' }));
  else { element.scrollTop = element.scrollHeight - element.clientHeight; await fireEvent.scroll(element); }
  await advance();
  expect(lines()).toHaveLength(100);
  expect(lines()[0].dataset.rowId).toBe('synthetic-701');
  expect(lines().at(-1)!.dataset.rowId).toBe('synthetic-800');
  await fireEvent.click(screen.getByRole('button', { name: 'Load earlier' })); await flush();
  expect(view.listChat.mock.lastCall![0].cursor).toEqual({ at: '701000000', id: 'synthetic-701' });
  expect(lines().map(line => line.dataset.rowId)).toEqual(Array.from({ length: 200 }, (_, i) => `synthetic-${601 + i}`));
});
function geometry() {
  const element = thread();
  Object.defineProperties(element, { clientHeight: { configurable: true, value: 260 }, scrollHeight: { configurable: true, get: () => lines().length * 26 } });
  vi.spyOn(element, 'getBoundingClientRect').mockImplementation(() => ({ top: 100, bottom: 360 } as DOMRect));
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function(this: HTMLElement) {
    const index = lines().indexOf(this), top = 100 + index * 26 - element.scrollTop;
    return { top, bottom: top + 26 } as DOMRect;
  });
  return element;
}

it('selects the newest person after the serialized people read and persists only filters and partner', async () => {
  const view = mount(); await flush();
  expect(view.order).toEqual(['people', 'thread']);
  expect(view.listWhisperPartners.mock.calls[0][0]).toEqual({ query: '', show_hidden: false, limit: 200 });
  expect(view.listChat.mock.calls[0][0]).toMatchObject({ sender: 'Vexlin', channel: 'whisper', limit: 100 });
  expect(view.onStateChange.mock.lastCall![0].logsPosition.whisper).toEqual({ query: '', showHidden: false, partner: 'Vexlin' });
  expect(view.onRowsReady).toHaveBeenCalledExactlyOnceWith('entry');
  expect(screen.getByRole('link', { name: 'All chat from Vexlin' }).getAttribute('href')).toBe('#/logs/from/Vexlin');
  expect(screen.queryByRole('button', { name: 'NPC lines' })).toBeNull();
});
it('prefers a route partner over saved selection and restores newest pages on Back/Forward', async () => {
  const view = mount({ partner: 'Quenby', savedPartner: 'Vexlin' }); await flush();
  expect(view.listChat.mock.lastCall![0].sender).toBe('Quenby');
  await fireEvent.click(screen.getByRole('button', { name: 'Load earlier' })); await flush();
  expect(lines()).toHaveLength(200);
  await view.rerender({ route: { ...view.route, partner: 'Vexlin' }, entryKey: 'back' }); await flush();
  expect(view.listChat.mock.lastCall![0]).toMatchObject({ sender: 'Vexlin', cursor: null });
  await view.rerender({ route: view.route, entryKey: 'forward' }); await flush();
  expect(view.listChat.mock.lastCall![0]).toMatchObject({ sender: 'Quenby', cursor: null });
  expect(lines()).toHaveLength(100);
});
it('uses the saved partner when the route has none', async () => {
  const view = mount({ savedPartner: 'Quenby' }); await flush();
  expect(view.listChat.mock.lastCall![0].sender).toBe('Quenby');
});
it('prepends earlier lines while preserving the first visible row offset, then caps at 500', async () => {
  mount(); await flush(); const element = geometry(); element.scrollTop = 80;
  await fireEvent.scroll(element);
  const first = lines().find(row => row.getBoundingClientRect().bottom > 100)!;
  const offset = first.getBoundingClientRect().top - 100;
  await fireEvent.click(screen.getByRole('button', { name: 'Load earlier' })); await flush();
  expect(lines()[0].dataset.rowId).toBe('synthetic-451');
  expect(first.getBoundingClientRect().top - 100).toBe(offset);
  for (let i = 0; i < 3; i++) { await fireEvent.click(screen.getByRole('button', { name: 'Load earlier' })); await flush(); }
  expect(lines()).toHaveLength(500);
  expect(screen.getByText('Showing the latest 500 lines')).toBeTruthy();
  expect(screen.getByRole('link', { name: 'Older lines in Chat ›' }).getAttribute('href')).toBe('#/logs/from/Vexlin');
  expect(screen.queryByRole('button', { name: 'Load earlier' })).toBeNull();
});
it('debounces search for both panes and sends Hidden to both reads', async () => {
  const view = mount(); await flush();
  await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'Quenby' } });
  await advance(249); expect(view.listWhisperPartners).toHaveBeenCalledTimes(1);
  await advance(1);
  expect(view.listWhisperPartners.mock.lastCall![0].query).toBe('Quenby');
  expect(view.listChat.mock.lastCall![0].query).toBe('Quenby');
  expect(screen.getAllByRole('link').filter(link => link.getAttribute('href') === '#/logs/chat/whisper/Vexlin')).toHaveLength(0);
  await fireEvent.click(screen.getByRole('button', { name: 'Hidden' })); await flush();
  expect(view.listWhisperPartners.mock.lastCall![0].show_hidden).toBe(true);
  expect(view.listChat.mock.lastCall![0].show_hidden).toBe(true);
});
it('hides through chat settings and moves to the next person', async () => {
  const view = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Hide' })); await flush();
  expect(view.client.updateChatSettings).toHaveBeenCalledWith(expect.objectContaining({ hidden_senders: expect.arrayContaining(['Vexlin']) }));
  expect(view.listChat.mock.lastCall![0].sender).toBe('Quenby');
  expect(screen.getByRole('link', { name: 'All chat from Quenby' })).toBeTruthy();
});
it('refreshes people and appends at bottom while retaining earlier pages and deduplicating', async () => {
  const view = mount(); await flush();
  await fireEvent.click(screen.getByRole('button', { name: 'Load earlier' })); await flush();
  const element = geometry(); element.scrollTop = element.scrollHeight - element.clientHeight; await fireEvent.scroll(element);
  view.append(); await view.rerender({ required: '2', pendingCount: 8 }); await advance();
  expect(view.order.slice(-2)).toEqual(['people', 'thread']);
  expect(lines()).toHaveLength(201); expect(lines().at(-1)!.dataset.rowId).toBe('synthetic-651');
  expect(new Set(lines().map(row => row.dataset.rowId)).size).toBe(201);
  expect(screen.queryByRole('button', { name: /New whispers/ })).toBeNull();
  expect(element.scrollTop).toBe(element.scrollHeight);
  for (let i = 0; i < 3; i++) { await fireEvent.click(screen.getByRole('button', { name: 'Load earlier' })); await flush(); }
  expect(lines()).toHaveLength(500);
  expect(lines()[0].dataset.rowId).toBe('synthetic-152');
});
it('defers while scrolled up and shows an unnumbered New whispers pill', async () => {
  const view = mount(); await flush(); const element = geometry(); element.scrollTop = 0; await fireEvent.scroll(element);
  view.append(); await view.rerender({ required: '2', pendingCount: 8 }); await flush();
  expect(view.listChat).toHaveBeenCalledTimes(1);
  const pill = screen.getByRole('button', { name: '↑ New whispers' });
  await fireEvent.click(pill); await advance();
  expect(view.listChat).toHaveBeenCalledTimes(2);
  expect(lines().at(-1)!.dataset.rowId).toBe('synthetic-651');
});
it('waits for the durable checkpoint, and reports readiness once', async () => {
  const view = mount(); await flush();
  view.append(); await view.rerender({ required: '101', pendingCount: 8 }); await flush();
  expect(view.listWhisperPartners).toHaveBeenCalledTimes(1);
  await view.rerender({ store: { state: 'ready', stream_id: 'synthetic', durable: '101', queued: 0, message: '' } }); await advance();
  expect(view.listWhisperPartners).toHaveBeenCalledTimes(2);
  expect(view.onHistoryApplied).toHaveBeenLastCalledWith('entry', '101');
  expect(view.onRowsReady).toHaveBeenCalledTimes(1);
});
it('does not overlap the thread read with a pending people read', async () => {
  const view = mount(); const pending = deferred<Awaited<ReturnType<typeof view.listWhisperPartners>>>();
  view.listWhisperPartners.mockImplementationOnce(() => pending.promise); await flush();
  expect(view.listChat).not.toHaveBeenCalled();
  pending.resolve({ status: 'ready', value: { rows: people, people: 2, lines: 1300 } }); await flush();
  expect(view.listChat).toHaveBeenCalledTimes(1);
});
it('uses the shared storage-off action', async () => {
  const view = mount({ stored: false }); await flush();
  expect(screen.getByText("Whisper chat isn't being saved")).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Turn on whisper chat' })); await flush();
  expect(view.client.updateChatSettings).toHaveBeenCalledWith(expect.objectContaining({ store: expect.objectContaining({ whisper: true }) }));
});
it('shows the empty state when nothing is stored', async () => {
  const view = mount(); view.listWhisperPartners.mockResolvedValueOnce({ status: 'ready', value: { rows: [], people: 0, lines: 0 } }); await flush();
  expect(screen.getByText('No whispers yet')).toBeTruthy();
  expect(view.listChat).not.toHaveBeenCalled();
  expect(screen.queryByText(/whispers with .*people/)).toBeNull();
});
it('focuses the heading and clears saved focus when a refreshed row disappears', async () => {
  const view = mount(); await flush();
  lines().at(-1)!.focus();
  view.listChat.mockResolvedValueOnce({ status: 'ready', value: { rows: [row(651), row(649)], next: null } });
  await view.rerender({ required: '2', pendingCount: 8 }); await advance();
  expect(document.activeElement).toBe(view.pane.querySelector('h1'));
  expect(screen.getByText('Row Removed')).toBeTruthy();
  expect(view.onStateChange).toHaveBeenCalledWith({ focusedRowId: null });
});
it('keeps filters on a failed people read and retries before reading the thread', async () => {
  const view = mount(); view.listWhisperPartners.mockRejectedValueOnce(new Error('synthetic failure')); await flush();
  expect(screen.getByText("Couldn't load whispers. Your filters are kept.")).toBeTruthy();
  expect(view.listChat).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
  expect(view.listChat).toHaveBeenCalledTimes(1);
});
