import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ChatView from '../../src/lib/logs/ChatView.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import fixture from '../fixtures/t0-live';
import type { ChatFilter } from '../../src/lib/bindings/ChatFilter';
import type { ChatMessageView } from '../../src/lib/bindings/ChatMessageView';
import { defaultLogsPosition, emptyListState, type ListState } from '../../src/lib/navigation/state';
import type { LogsFixtureQueries } from '../../src/lib/bridge/fixture';
import { advance, flush } from '../runs/fixtures';
import { chatLineLabel, chatTime } from '../../src/lib/logs/presentation';
afterEach(cleanup);
const row = (n: number, text: string, extra: Partial<ChatMessageView> = {}) => ({ hidden: false, message: {
  id: `live-1:chat:${n}`, session_id: 's', run_id: null, map: 'Nexus', at: String(n * 1000), channel: 'whisper' as const,
  direction: 'incoming' as const, sender_kind: 'player' as const, sender: 'SynthAlpha', counterpart: 'SynthAlpha',
  is_local: false, stars: 70, supporter: false, text, truncated: false, ...extra } });
const seen: ChatFilter[] = [];
const client = () => createClient(createFixtureTransport(fixture, undefined, undefined, undefined, undefined, undefined, undefined,
  undefined, undefined, { listChat: filter => { seen.push(filter); return { rows: [row(1, '<script>alert(1)</script>'), row(2, 'need a carry?')], next: null }; } }));

it('renders whisper lines as text with direction labels', async () => {
  render(ChatView, { client: client(), channel: 'all' });
  await waitFor(() => expect(screen.getByText('need a carry?')).toBeTruthy());
  expect(screen.getAllByText('<script>alert(1)</script>').length).toBeGreaterThan(0);
  expect(document.querySelector('script')).toBeNull();
  expect(screen.getAllByText('From SynthAlpha').length).toBe(2);
});
it('reads the selected channel with NPC lines hidden by default', async () => {
  seen.length = 0;
  render(ChatView, { client: client(), channel: 'guild' });
  await waitFor(() => expect(seen.length).toBeGreaterThan(0));
  expect(seen[0]).toMatchObject({ channel: 'guild', show_npc: false, show_hidden: false, limit: 100, query: '' });
});


const cursor = { at: '1000', id: 'live-1:chat:1' };
const defaultFilter = (): ChatFilter => ({ channel: null, sender: null, query: '', run_id: null, show_npc: false, show_hidden: false, cursor: null, limit: 100 });
function mount(options: { savedState?: ListState; channel?: 'all' | 'guild' | 'party'; runId?: string; seedSender?: string; hiddenKeywords?: string[]; settingsFailures?: number; listChat?: LogsFixtureQueries['listChat'] } = {}) {
  const listChat = vi.fn(options.listChat ?? (filter => ({ rows: [row(1, filter.cursor ? 'older' : 'latest')], next: filter.cursor ? null : cursor })));
  const client = createClient(createFixtureTransport(fixture, undefined, undefined, undefined, undefined, undefined, undefined, undefined, undefined, { listChat }));
  if (options.hiddenKeywords) {
    const settings = client.querySettings;
    client.querySettings = async () => { const value = await settings(); return { ...value, chat: { ...value.chat, hidden_keywords: options.hiddenKeywords! } }; };
  }
  if (options.settingsFailures) {
    const settings = client.querySettings; let failures = options.settingsFailures;
    client.querySettings = vi.fn(async () => { if (failures-- > 0) throw new Error('synthetic settings failure'); return settings(); });
  }
  const pane = document.createElement('main'); document.body.append(pane);
  const onStateChange = vi.fn(), onRowsReady = vi.fn();
  const view = render(ChatView, { target: pane, props: { client, channel: options.channel ?? 'all', runId: options.runId,
    savedState: options.savedState, onStateChange, onRowsReady, entryKey: 'chat-entry', streamId: 'live-1', required: '1', pendingCount: 12,
    store: { state: 'ready', stream_id: 'live-1', durable: '100', queued: 0, message: '' } } });
  return { ...view, client, pane, listChat, onStateChange, onRowsReady };
}
describe('chat history interactions', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => { cleanup(); document.querySelectorAll('main').forEach(pane => pane.remove()); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
  it('keeps a non-chat checkpoint deferred without a pill and refreshes silently at the head', async () => {
    const view = mount(); await flush(); view.pane.scrollTop = 100; await fireEvent.scroll(view.pane);
    await view.rerender({ required: '2', pendingCount: 0 }); await advance();
    expect(screen.queryByRole('button', { name: /new lines/i })).toBeNull();
    expect(view.listChat).toHaveBeenCalledTimes(1);
    view.pane.scrollTop = 0; await fireEvent.scroll(view.pane); await flush();
    expect(view.listChat).toHaveBeenCalledTimes(2);
  });
  it('shows an unknown hidden-page count while Hidden is off', async () => {
    mount(); await flush();
    expect(screen.getByText('Lines hidden on this page').nextElementSibling?.textContent).toBe('—');
    await fireEvent.click(screen.getByRole('button', { name: 'Hidden' })); await flush();
    expect(screen.getByText('Lines hidden on this page').nextElementSibling?.textContent).toBe('0');
  });
  it('preserves a pending search when another toolbar filter changes', async () => {
    const view = mount(); await flush();
    await fireEvent.input(screen.getByRole('searchbox'), { target: { value: 'carry' } });
    await advance(100); await fireEvent.click(screen.getByRole('button', { name: 'NPC lines' }));
    expect((screen.getByRole('searchbox') as HTMLInputElement).value).toBe('carry');
    await advance(250);
    expect(view.listChat).toHaveBeenLastCalledWith(expect.objectContaining({ query: 'carry', show_npc: true }));
  });
  it('hides the pending number when hide lists are populated', async () => {
    const view = mount({ hiddenKeywords: ['synthetic'] });
    await flush(); view.pane.scrollTop = 100; await fireEvent.scroll(view.pane);
    await view.rerender({ required: '2', pendingCount: 12 }); await advance();
    expect(screen.getByRole('button', { name: '↑ New lines' })).toBeTruthy();
  });
  it('renders 100 selectable table rows and Public tags with place groups', async () => {
    const view = mount({ listChat: () => ({ rows: Array.from({ length: 100 }, (_, i) => row(i, `line ${i}`, { channel: 'public', run_id: i > 49 ? 'r2' : 'r1' })), next: null }) });
    await flush();
    expect(view.container.querySelectorAll('tr[data-row-id]')).toHaveLength(100);
    expect(screen.queryByRole('list', { name: 'Messages' })).toBeNull();
    expect(screen.getAllByText('Public').length).toBeGreaterThanOrEqual(100);
    expect(screen.getAllByRole('link', { name: 'Open run ›' })).toHaveLength(2);
    const second = view.container.querySelectorAll<HTMLElement>('[data-row-id]')[1];
    await fireEvent.keyDown(second, { key: ' ' });
    expect(second.getAttribute('aria-selected')).toBe('true');
    expect(view.onStateChange.mock.lastCall![0].logsPosition.chat.selectedId).toBe('live-1:chat:1');
    await fireEvent.keyDown(view.container.querySelectorAll('[data-row-id]')[2], { key: 'Enter' });
    expect(view.onStateChange.mock.lastCall![0].logsPosition.chat.selectedId).toBe('live-1:chat:2');
  });
  it('keeps the run on channel links and removes it through the run chip', async () => {
    mount({ runId: 'synthetic-run' }); await flush();
    expect(screen.getByRole('link', { name: 'Guild' }).getAttribute('href')).toBe('#/logs/chat/guild/run/synthetic-run');
    expect(screen.getByRole('link', { name: /^Remove run filter:/ }).getAttribute('href')).toBe('#/logs');
    expect(screen.getByRole('link', { name: 'Events in this run ›' }).getAttribute('href')).toBe('#/logs/events/run/synthetic-run');
  });
  it('debounces search and focuses it with slash outside fields', async () => {
    const view = mount(); await flush();
    const search = screen.getByLabelText('Search');
    await fireEvent.keyDown(window, { key: '/' }); expect(document.activeElement).toBe(search);
    await fireEvent.input(search, { target: { value: 'carry' } }); await advance(249);
    expect(view.listChat).toHaveBeenCalledTimes(1); await advance(1);
    expect(view.listChat).toHaveBeenLastCalledWith({ ...defaultFilter(), query: 'carry' });
    await fireEvent.click(screen.getByRole('button', { name: /Any sender/ }));
    const sender = screen.getByRole('combobox'); sender.focus(); await fireEvent.keyDown(sender, { key: '/' });
    expect(document.activeElement).toBe(sender);
  });
  it('loads overview after the page, sender once per name after 150ms, and partners lazily', async () => {
    const view = mount({ listChat: () => ({ rows: [row(1, 'first'), row(2, 'second'), row(3, 'third', { sender: 'Quenby' })], next: null }) });
    const overview = vi.spyOn(view.client, 'getChatOverview'), sender = vi.spyOn(view.client, 'getChatSender'), partners = vi.spyOn(view.client, 'listWhisperPartners');
    sender.mockResolvedValue({ status: 'ready', value: { name: 'SynthAlpha', lines: 148, first_at: '1000000', last_at: '2000000', channels: ['guild', 'whisper'], stars: 72, supporter: true, whispers: 41 } });
    await flush(); expect(overview).toHaveBeenCalledTimes(1); expect(sender).not.toHaveBeenCalled(); expect(partners).not.toHaveBeenCalled();
    await fireEvent.click(view.container.querySelector('[data-row-id="live-1:chat:1"]')!);
    await advance(150); expect(sender).toHaveBeenCalledTimes(1);
    expect(screen.getByText('148')).toBeTruthy(); expect(screen.getByRole('link', { name: 'Whispers with SynthAlpha' }).getAttribute('href')).toBe('#/logs/chat/whisper/SynthAlpha');
    await fireEvent.click(view.container.querySelector('[data-row-id="live-1:chat:2"]')!); await advance(150); expect(sender).toHaveBeenCalledTimes(1);
    await fireEvent.click(view.container.querySelector('[data-row-id="live-1:chat:3"]')!); await advance(149); expect(sender).toHaveBeenCalledTimes(1);
    await advance(1); expect(sender).toHaveBeenCalledTimes(2);
    await fireEvent.click(screen.getByRole('button', { name: /Any sender/ })); await flush(); expect(partners).toHaveBeenCalledTimes(1);
    await fireEvent.keyDown(screen.getByRole('combobox'), { key: 'Escape' }); await fireEvent.click(screen.getByRole('button', { name: /Any sender/ })); await flush(); expect(partners).toHaveBeenCalledTimes(1);
    await fireEvent.input(screen.getByLabelText('Search'), { target: { value: 'changed' } }); await advance(250); expect(overview).toHaveBeenCalledTimes(1);
    await advance(150); const senderReads = sender.mock.calls.length;
    await view.rerender({ required: '2' }); await advance(); expect(overview).toHaveBeenCalledTimes(1); expect(sender).toHaveBeenCalledTimes(senderReads);
  });
  it('copies the selected row and Only filters its sender; unavailable clipboard is harmless', async () => {
    const view = mount(); await flush();
    const writeText = vi.fn().mockResolvedValue(undefined); Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    await fireEvent.click(screen.getByRole('button', { name: 'Copy' })); await flush(); expect(writeText).toHaveBeenCalledWith(expect.stringContaining('SynthAlpha: latest'));
    expect(screen.getByText('Copied')).toBeTruthy();
    const announcements: string[] = [];
    const status = screen.getByText('Copied');
    const observer = new MutationObserver(() => announcements.push(status.textContent ?? ''));
    observer.observe(status, { childList: true, characterData: true, subtree: true });
    await fireEvent.click(screen.getByRole('button', { name: 'Copy' })); await flush(); observer.disconnect();
    expect(announcements).toContain(''); expect(announcements.at(-1)).toBe('Copied');
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined });
    await fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    await fireEvent.click(screen.getByRole('button', { name: 'Only' })); await flush();
    expect(view.listChat).toHaveBeenLastCalledWith({ ...defaultFilter(), sender: 'SynthAlpha' });
  });
  it('never attributes the previous sender summary to a new sender while loading or after failure', async () => {
    const view = mount({ listChat: () => ({ rows: [row(1, 'first'), row(2, 'second', { sender: 'Quenby' })], next: null }) });
    const sender = vi.spyOn(view.client, 'getChatSender').mockResolvedValue({ status: 'ready', value: { name: 'SynthAlpha', lines: 148, first_at: '1000000', last_at: '2000000', channels: ['guild'], stars: 72, supporter: false, whispers: 0 } });
    await flush(); await fireEvent.click(view.container.querySelector('[data-row-id="live-1:chat:1"]')!); await advance(150); expect(screen.getByText('148')).toBeTruthy();
    sender.mockRejectedValue(new Error('private sender payload'));
    await fireEvent.click(view.container.querySelector('[data-row-id="live-1:chat:2"]')!); await advance(149);
    expect(screen.queryByText('148')).toBeNull();
    expect(screen.getByText('Lines stored').nextElementSibling?.textContent).toBe('—');
    expect(screen.queryByText(/guildmate/)).toBeNull(); await advance(1);
    expect(screen.queryByText('148')).toBeNull(); expect(screen.queryByText('private sender payload')).toBeNull();
    expect(screen.getByText('Lines stored').nextElementSibling?.textContent).toBe('—');
  });
  it('distinguishes an empty history from a search without matches', async () => {
    mount({ listChat: () => ({ rows: [], next: null }) }); await flush();
    expect(screen.getByText('No chat yet')).toBeTruthy();
    await fireEvent.input(screen.getByLabelText('Search'), { target: { value: 'trade' } }); await advance(250);
    expect(screen.getByText('No chat lines match "trade"')).toBeTruthy();
    expect(screen.queryByRole('link', { name: 'Search all channels' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Clear search' })); await flush(); expect(screen.getByText('No chat yet')).toBeTruthy();
  });
  it('omits Hide for local and NPC senders', async () => {
    const view = mount({ listChat: () => ({ rows: [row(1, 'local', { is_local: true }), row(2, 'npc', { sender_kind: 'npc' })], next: null }) }); await flush();
    expect(screen.queryByRole('button', { name: /^Hide / })).toBeNull();
    expect(screen.queryByRole('link', { name: /^Whispers with/ })).toBeNull();
    await fireEvent.click(view.container.querySelector('[data-row-id="live-1:chat:2"]')!);
    expect(screen.queryByRole('link', { name: /^Whispers with/ })).toBeNull();
  });
  it('shows a genuinely empty guild channel without an empty search term', async () => {
    mount({ channel: 'guild', listChat: () => ({ rows: [], next: null }) }); await flush();
    expect(screen.getByText('No chat yet')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Clear search' })).toBeNull();
    expect(screen.queryByText(/match ""/)).toBeNull();
    await fireEvent.input(screen.getByLabelText('Search'), { target: { value: 'trade' } }); await advance(250);
    expect(screen.getByText('No guild lines match "trade"')).toBeTruthy();
    expect(screen.getByRole('link', { name: 'Search all channels' }).getAttribute('href')).toBe('#/logs');
  });
  it('omits row stars for local and unknown-star senders and titles place labels', async () => {
    const view = mount({ listChat: () => ({ rows: [row(1, 'local', { is_local: true }), row(2, 'unknown stars', { stars: null }), row(3, 'known stars')], next: null }) }); await flush();
    const rows = view.container.querySelectorAll('tr[data-row-id]');
    expect(rows[0].textContent).not.toContain('★'); expect(rows[0].textContent).toContain('you');
    expect(rows[1].textContent).not.toContain('★'); expect(rows[2].textContent).toContain('★ 70');
    const label = view.container.querySelector('.group .label');
    expect(label?.getAttribute('title')).toContain('Nexus');
    expect(view.container.querySelector('.group .rule')).toBeTruthy();
  });
  it('shows Retry only on errors and enables channel storage', async () => {
    const view = mount({ channel: 'party' });
    const settings = await view.client.querySettings(); await view.client.updateChatSettings({ ...settings.chat, store: { ...settings.chat.store, party: false } });
    await flush();
    // Refresh the entry so it reads the changed settings.
    await view.rerender({ enabled: false }); await flush(); await view.rerender({ enabled: true }); await flush();
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
    await fireEvent.click(screen.getByRole('button', { name: 'Turn on party chat' })); await flush();
    expect((await view.client.querySettings()).chat.store.party).toBe(true);
    view.listChat.mockImplementation(() => { throw new Error('private payload'); });
    await view.rerender({ required: '2' }); await advance();
    expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
    view.listChat.mockReturnValue({ rows: [row(1, 'recovered')], next: null });
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush(); expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  });
  it('restores selection and announces when a focused row disappears', async () => {
    const savedState = { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), chat: { filter: defaultFilter(), previous: [], selectedId: 'live-1:chat:2' } } };
    const view = mount({ savedState, listChat: () => ({ rows: [row(1, 'first'), row(2, 'second')], next: null }) }); await flush();
    const heading = document.createElement('h1'); heading.tabIndex = -1; view.pane.prepend(heading);
    const selected = view.container.querySelector<HTMLElement>('[data-row-id="live-1:chat:2"]')!; expect(selected.getAttribute('aria-selected')).toBe('true'); selected.focus();
    view.listChat.mockReturnValue({ rows: [row(1, 'first')], next: null }); await view.rerender({ required: '2' }); await advance();
    expect(document.activeElement).toBe(heading); expect(screen.getByText('Row Removed')).toBeTruthy();
    expect(view.onStateChange).toHaveBeenCalledWith(expect.objectContaining({ focusedRowId: null }));
    expect(view.onRowsReady).toHaveBeenCalledExactlyOnceWith('chat-entry');
  });
  it('uses the router seed and suppresses inaccurate pending counts with sender filters', async () => {
    const savedState = { ...emptyListState(), logsPosition: { ...defaultLogsPosition(), chat: { filter: { ...defaultFilter(), query: 'restored', cursor }, previous: [null] } } };
    const seededState = { ...savedState, logsPosition: { ...defaultLogsPosition(), chat: { filter: { ...defaultFilter(), sender: 'Vexlin' }, previous: [] } } };
    const view = mount({ seedSender: 'Vexlin', savedState: seededState }); await flush();
    expect(view.listChat).toHaveBeenCalledWith({ ...defaultFilter(), sender: 'Vexlin' });
    view.pane.scrollTop = 100; await fireEvent.scroll(view.pane); await view.rerender({ required: '2' }); await advance();
    expect(screen.getByRole('button', { name: '↑ New lines' })).toBeTruthy();
    cleanup(); const unseeded = mount({ savedState }); await flush(); expect(unseeded.listChat).toHaveBeenCalledWith({ ...defaultFilter(), query: 'restored', cursor });
  });
  it('rejects oversized or control-containing exact sender filters', async () => {
    const view = mount(); await flush();
    for (const value of ['é'.repeat(17), 'a\u0000b']) {
      await fireEvent.click(screen.getByRole('button', { name: /Any sender/ }));
      await fireEvent.input(screen.getByRole('combobox'), { target: { value } }); await fireEvent.keyDown(screen.getByRole('combobox'), { key: 'Enter' }); await flush();
      expect(screen.getByRole('alert').textContent).toContain('32 UTF-8 bytes'); expect(view.listChat).toHaveBeenCalledTimes(1);
    }
  });
  it('pages without losing button focus and persists the cursor stack', async () => {
    const view = mount(); await flush();
    const next = screen.getByRole('button', { name: 'Older lines' }); next.focus();
    await fireEvent.click(next); await flush();
    expect(screen.getAllByText('older')[0]).toBeTruthy();
    expect(document.activeElement).toBe(next);
    expect(view.onStateChange.mock.lastCall![0].logsPosition.chat).toEqual({ filter: { ...defaultFilter(), cursor }, previous: [null], selectedId: null });
    await fireEvent.click(screen.getByRole('button', { name: 'Newer lines' })); await flush();
    expect(screen.getAllByText('latest')[0]).toBeTruthy();
    expect(view.onRowsReady).toHaveBeenCalledExactlyOnceWith('chat-entry');
  });
  it('defers incoming messages while scrolled down, then returns to the head on request', async () => {
    const view = mount(); await flush(); view.pane.scrollTop = 100;
    await fireEvent.scroll(view.pane);
    view.listChat.mockReturnValue({ rows: [row(2, 'new arrival')], next: null });
    await view.rerender({ required: '2' }); await advance();
    expect(view.listChat).toHaveBeenCalledTimes(1);
    expect(screen.getAllByText('latest')[0]).toBeTruthy();
    expect(view.pane.scrollTop).toBe(100);
    expect(screen.getByRole('button', { name: '↑ New lines' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: /new lines/i })); await flush();
    expect(screen.getAllByText('new arrival')[0]).toBeTruthy();
    expect(view.pane.scrollTop).toBe(0);
    expect(screen.queryByRole('button', { name: /new lines/i })).toBeNull();
  });
  it('checks the actual pane position before a delayed scroll notification', async () => {
    const view = mount(); await flush(); view.pane.scrollTop = 100;
    await view.rerender({ required: '2' }); await advance();
    expect(view.listChat).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: /new lines/i })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: /new lines/i })); await flush();
    expect(view.pane.scrollTop).toBe(0);
    expect(view.listChat).toHaveBeenCalledTimes(2);
  });
  it('defers updates on older pages and refreshes automatically at the head', async () => {
    const view = mount(); await flush();
    await fireEvent.click(screen.getByRole('button', { name: 'Older lines' })); await flush();
    await view.rerender({ required: '2' }); await advance();
    expect(view.listChat).toHaveBeenCalledTimes(2);
    expect(screen.getAllByText('older')[0]).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: /new lines/i })); await flush();
    expect(view.listChat).toHaveBeenLastCalledWith(defaultFilter());
    view.listChat.mockReturnValue({ rows: [row(3, 'live arrival')], next: null });
    await view.rerender({ required: '3' }); await advance();
    expect(screen.getAllByText('live arrival')[0]).toBeTruthy();
  });
  it('restores matching saved chat state and rejects a different channel or run', async () => {
    const savedState: ListState = { ...emptyListState(), logsPosition: { ...defaultLogsPosition(),
      chat: { filter: { ...defaultFilter(), channel: 'guild', run_id: 'r1', cursor, query: 'carry' }, previous: [null] },
      events: { filter: { kinds: [], run_id: null, dungeon: null, cursor: null, limit: 100 }, previous: [] },
    } };
    mount({ savedState, channel: 'guild', runId: 'r1' }); await flush();
    expect(screen.getAllByText('older')[0]).toBeTruthy();
    expect((screen.getByLabelText('Search') as HTMLInputElement).value).toBe('carry');
    cleanup();
    const view = mount({ savedState, channel: 'all' }); await flush();
    expect(view.listChat).toHaveBeenCalledWith(defaultFilter());
  });
  it('applies search and visibility filters from the first page', async () => {
    const view = mount(); await flush();
    await fireEvent.click(screen.getByRole('button', { name: 'Older lines' })); await flush();
    await fireEvent.input(screen.getByLabelText('Search'), { target: { value: 'carry' } });
    await advance(250);
    await fireEvent.click(screen.getByRole('button', { name: /Any sender/ }));
    await fireEvent.input(screen.getByLabelText('Sender'), { target: { value: 'SynthBravo' } });
    await fireEvent.keyDown(screen.getByLabelText('Sender'), { key: 'Enter' }); await flush();
    expect(view.listChat).toHaveBeenLastCalledWith({ ...defaultFilter(), query: 'carry', sender: 'SynthBravo' });
    await fireEvent.click(screen.getByRole('button', { name: 'NPC lines' })); await flush();
    await fireEvent.click(screen.getByRole('button', { name: /^Hidden/ })); await flush();
    expect(view.listChat).toHaveBeenLastCalledWith({ ...defaultFilter(), query: 'carry', sender: 'SynthBravo', show_npc: true, show_hidden: true });
  });
  it('does not send invalid search text to history', async () => {
    const view = mount(); await flush();
    for (const value of ['a'.repeat(129), 'a\0b']) {
      await fireEvent.input(screen.getByLabelText('Search'), { target: { value } });
      await advance(250); await flush();
      expect(screen.getByRole('alert')).toBeTruthy();
      expect(view.listChat).toHaveBeenCalledTimes(1);
    }
  });
  it('keeps loading rows gated and releases error states without exposing backend errors', async () => {
    const view = mount();
    vi.spyOn(view.client, 'listChat').mockRejectedValue(new Error('private payload'));
    await flush();
    expect(screen.getByRole('alert').textContent).toContain("Couldn't load chat. Your filters are kept.");
    expect(screen.queryByText('private payload')).toBeNull();
    expect(view.onRowsReady).toHaveBeenCalledExactlyOnceWith('chat-entry');
  });
  it.each(['SynthAlpha' + '😀'.repeat(6), 'SynthAlpha' + 'a'.repeat(23), 'SynthAlpha\u0000', 'SynthAlpha\u007f', 'SynthAlpha\u0085'])('refuses invalid hidden sender bytes or controls without exposing the name in the error', async sender => {
    const view = mount({ listChat: () => ({ rows: [row(1, 'latest', { sender })], next: null }) });
    const update = vi.spyOn(view.client, 'updateChatSettings');
    await flush();
    await fireEvent.click(screen.getByRole('button', { name: `Hide ${sender}` })); await flush();
    expect(update).not.toHaveBeenCalled();
    expect(screen.getByRole('alert').textContent).toBe("Couldn't hide sender. Use 1–32 UTF-8 bytes with no control characters.");
  });
  it('accepts a hidden sender at the UTF-8 byte boundary', async () => {
    const sender = 'SynthAlpha' + 'é'.repeat(11);
    const view = mount({ listChat: () => ({ rows: [row(1, 'latest', { sender })], next: null }) });
    await flush();
    await fireEvent.click(screen.getByRole('button', { name: `Hide ${sender}` })); await flush();
    expect((await view.client.querySettings()).chat.hidden_senders).toEqual([sender]);
  });
  it('hides a sender through stored settings and visibly handles failures', async () => {
    const view = mount(); await flush();
    await fireEvent.click(screen.getByRole('button', { name: 'Hide SynthAlpha' })); await flush();
    expect((await view.client.querySettings()).chat.hidden_senders).toEqual(['SynthAlpha']);
    expect(view.listChat).toHaveBeenCalledTimes(2);
    vi.spyOn(view.client, 'updateChatSettings').mockRejectedValue(new Error('private payload'));
    await fireEvent.click(screen.getByRole('button', { name: 'Hide SynthAlpha' })); await flush();
    expect(screen.getByRole('alert').textContent).toBe("Couldn't hide sender. Retry.");
    expect(screen.queryByText('private payload')).toBeNull();
  });
});
it('labels outgoing and unknown-direction whispers and invalid timestamps', () => {
  expect(chatLineLabel(row(1, 'x', { direction: 'outgoing', counterpart: 'SynthBravo' }).message)).toBe('To SynthBravo');
  expect(chatLineLabel(row(1, 'x', { direction: null, counterpart: null }).message)).toBe('SynthAlpha → Unknown player');
  expect(chatTime('invalid')).toBe('—'); expect(chatTime('999999999999999999999999')).toBe('—');
});

describe('stable selection and reversible spam presentation', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => { cleanup(); document.querySelectorAll('main').forEach(pane => pane.remove()); vi.clearAllTimers(); vi.useRealTimers(); });
  it('starts unselected and keeps the chosen message after live replacement and pagination', async () => {
    const view = mount(); await flush();
    expect(screen.getByText('Select a line to see its sender.')).toBeTruthy();
    await fireEvent.click(view.container.querySelector('[data-row-id]')!);
    view.listChat.mockReturnValue({ rows: [row(9, 'synthetic new arrival', { sender: 'SynthBravo' })], next: cursor });
    await view.rerender({ required: '2' }); await advance();
    expect(view.container.querySelector('blockquote')?.textContent).toContain('latest');
    await fireEvent.click(screen.getByRole('button', { name: 'Older lines' })); await flush();
    expect(view.container.querySelector('blockquote')?.textContent).toContain('latest');
    expect(view.onStateChange.mock.lastCall![0].logsPosition.chat.selectedId).toBe('live-1:chat:1');
  });
  describe('chat settings read failure', () => {
    const ad = () => ({ rows: [row(1, 'synthgold-shop.com delivery', { channel: 'public' })], next: null });
    it('says the settings are unavailable instead of presenting defaults as the stored choice', async () => {
      const view = mount({ settingsFailures: 1, listChat: ad }); await flush();
      const error = screen.getByText("Couldn't load chat settings");
      expect(error.closest('[role="alert"]')?.querySelector('button')?.textContent).toBe('Retry');
      const toggle = screen.getByRole('checkbox', { name: 'Filter spam' }) as HTMLInputElement;
      expect(toggle.checked).toBe(false); expect(toggle.indeterminate).toBe(true); expect(toggle.disabled).toBe(true);
      expect(screen.getByText('synthgold-shop.com delivery')).toBeTruthy();
      expect(screen.queryByText('1 hidden')).toBeNull();
      expect(view.client.querySettings).toHaveBeenCalledTimes(1);
    });
    it('restores the real settings when Retry succeeds', async () => {
      const view = mount({ settingsFailures: 1, listChat: ad }); await flush();
      await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
      expect(view.client.querySettings).toHaveBeenCalledTimes(2);
      expect(screen.queryByText("Couldn't load chat settings")).toBeNull();
      const toggle = screen.getByRole('checkbox', { name: 'Filter spam' }) as HTMLInputElement;
      expect(toggle.checked).toBe(true); expect(toggle.indeterminate).toBe(false); expect(toggle.disabled).toBe(false);
      expect(screen.queryByText('synthgold-shop.com delivery')).toBeNull();
      expect(screen.getByText('1 hidden')).toBeTruthy();
    });
    it('keeps the error when Retry fails again', async () => {
      const view = mount({ settingsFailures: 2, listChat: ad }); await flush();
      await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
      expect(view.client.querySettings).toHaveBeenCalledTimes(2);
      expect(screen.getByText("Couldn't load chat settings")).toBeTruthy();
      expect((screen.getByRole('checkbox', { name: 'Filter spam' }) as HTMLInputElement).disabled).toBe(true);
    });
    it('behaves as before when the first read succeeds', async () => {
      mount({ listChat: ad }); await flush();
      expect(screen.queryByText("Couldn't load chat settings")).toBeNull();
      expect((screen.getByRole('checkbox', { name: 'Filter spam' }) as HTMLInputElement).checked).toBe(true);
      expect(screen.queryByText('synthgold-shop.com delivery')).toBeNull();
    });
  });
  it('keeps the toggle and filtered rows consistent when saving fails', async () => {
    const view = mount({ listChat: () => ({ rows: [row(1, 'synthgold-shop.com delivery', { channel: 'public' })], next: null }) });
    vi.spyOn(view.client, 'updateChatSettings').mockRejectedValue(new Error('synthetic save failure'));
    await flush();
    const toggle = screen.getByRole('checkbox', { name: 'Filter spam' }) as HTMLInputElement;
    await fireEvent.click(toggle); await flush();
    expect(toggle.checked).toBe(true);
    expect(screen.queryByText('synthgold-shop.com delivery')).toBeNull();
    expect(screen.getByRole('alert').textContent).toBe("Couldn't update spam filter. Retry.");
  });
  it('persists filtering, shows the hidden count and restores ads when disabled', async () => {
    const view = mount({ listChat: () => ({ rows: [row(1, 'synthgold-shop.com delivery', { channel: 'public' }), row(2, 'ready'), row(3, 'ready')], next: null }) });
    await flush();
    expect(screen.queryByText('synthgold-shop.com delivery')).toBeNull();
    expect(screen.getByText('1 hidden')).toBeTruthy(); expect(screen.getByText('×2')).toBeTruthy();
    await fireEvent.click(screen.getByRole('checkbox', { name: 'Filter spam' })); await flush();
    expect((await view.client.querySettings()).chat.filter_spam).toBe(false);
    expect(screen.getByText('synthgold-shop.com delivery')).toBeTruthy();
    await view.rerender({ enabled: false }); await flush(); await view.rerender({ enabled: true }); await flush();
    expect((screen.getByRole('checkbox', { name: 'Filter spam' }) as HTMLInputElement).checked).toBe(false);
  });
  it('keeps the group containing the selected message highlighted after a duplicate arrives', async () => {
    const text = 'WTS exalted dex pot for 3 life pm me';
    const original = row(1, text, { channel: 'public' });
    const view = mount({ listChat: () => ({ rows: [original], next: null }) }); await flush();
    await fireEvent.click(view.container.querySelector('[data-row-id]')!);
    view.listChat.mockReturnValue({ rows: [row(2, text, { channel: 'public' }), original], next: null });
    await view.rerender({ required: '2' }); await advance();
    expect(view.container.querySelector('[data-row-id="live-1:chat:2"]')?.getAttribute('aria-selected')).toBe('true');
    expect(screen.getByText('×2')).toBeTruthy();
    expect(view.container.querySelector('blockquote')?.textContent).toContain(text);
    expect(view.onStateChange.mock.lastCall![0].logsPosition.chat.selectedId).toBe(original.message.id);
    // A later separated occurrence must not steal the selected group's highlight.
    view.listChat.mockReturnValue({ rows: [row(4, text, { channel: 'public' }), row(3, 'ready'), row(2, text, { channel: 'public' }), original], next: null });
    await view.rerender({ required: '3' }); await advance();
    expect(view.container.querySelector('[data-row-id="live-1:chat:4"]')?.getAttribute('aria-selected')).toBe('false');
    expect(view.container.querySelector('[data-row-id="live-1:chat:2"]')?.getAttribute('aria-selected')).toBe('true');
  });
});
