import { createAlertHistory, type AlertHistoryInput } from '../../src/lib/alerts/history';
import type { EventFilter } from '../../src/lib/bindings/EventFilter';
import type { EventSummary } from '../../src/lib/bindings/EventSummary';
import { describe, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport, type LogsFixtureQueries } from '../../src/lib/bridge/fixture';
import { createRunHistory, type HistoryQuery, type HistoryState } from '../../src/lib/runs/history';
import type { ChatOverview } from '../../src/lib/bindings/ChatOverview';
import type { ChatSenderSummary } from '../../src/lib/bindings/ChatSenderSummary';
import type { WhisperPartners } from '../../src/lib/bindings/WhisperPartners';
import demoSnapshot from '../fixtures/t0-live';

const overview: ChatOverview = { lines: 2, by_channel: { public: 0, guild: 1, party: 0, whisper: 1, unknown: 0 },
  first_at: '1', latest_at: '2' };
const sender: ChatSenderSummary = { name: 'Vexlin', lines: 2, first_at: '1', last_at: '2', channels: ['guild', 'whisper'],
  stars: 72, supporter: true, whispers: 1 };
const partners: WhisperPartners = { rows: [{ name: 'Vexlin', lines: 1, last_at: '2', last_text: 'Synthetic whisper',
  last_direction: 'unknown', stars: null }], people: 1, lines: 1 };
const request = { run_id: 'synthetic-run' };
const senderRequest = { name: 'Vexlin' };
const filter = { query: 'vex', show_hidden: true, limit: 200 };
function fixture(logs?: LogsFixtureQueries, strict = false) {
  return createFixtureTransport(demoSnapshot, undefined, undefined, undefined, undefined, undefined, undefined,
    { requireHistoryFixtures: strict }, undefined, logs);
}

describe('Logs summary bridge', () => {
  it('routes native names and argument keys and isolates fixture arguments and results', async () => {
    const queries: LogsFixtureQueries = {
      getChatOverview: input => { input.run_id = 'changed'; return overview; },
      getChatSender: input => { input.name = 'changed'; return sender; },
      listWhisperPartners: input => { input.query = 'changed'; return partners; },
    };
    const transport = fixture(queries);
    const invoke = vi.spyOn(transport, 'invoke');
    const client = createClient(transport);
    const result = await client.getChatOverview(request);
    const person = await client.getChatSender(senderRequest);
    const people = await client.listWhisperPartners(filter);
    expect(invoke.mock.calls).toEqual([
      ['get_chat_overview', { request }], ['get_chat_sender', { request: senderRequest }],
      ['list_whisper_partners', { filter }],
    ]);
    expect(request.run_id).toBe('synthetic-run');
    expect(senderRequest.name).toBe('Vexlin');
    expect(filter.query).toBe('vex');
    expect(result).toEqual({ status: 'ready', value: overview });
    expect(person).toEqual({ status: 'ready', value: sender });
    expect(people).toEqual({ status: 'ready', value: partners });
    if (result.status !== 'ready' || person.status !== 'ready' || people.status !== 'ready') throw new Error('Expected ready');
    result.value.by_channel.guild = 99;
    person.value!.channels.length = 0;
    people.value.rows[0].name = 'changed';
    expect(overview.by_channel.guild).toBe(1);
    expect(sender.channels).toEqual(['guild', 'whisper']);
    expect(partners.rows[0].name).toBe('Vexlin');
  });

  it('returns empty defaults and rejects missing strict fixtures and query failures', async () => {
    const client = createClient(fixture());
    await expect(client.getChatOverview({})).resolves.toEqual({ status: 'ready', value: {
      lines: 0, by_channel: { public: 0, guild: 0, party: 0, whisper: 0, unknown: 0 }, first_at: null, latest_at: null,
    } });
    await expect(client.getChatSender(senderRequest)).resolves.toEqual({ status: 'ready', value: null });
    await expect(client.listWhisperPartners(filter)).resolves.toEqual({ status: 'ready', value: { rows: [], people: 0, lines: 0 } });
    const fail = () => { throw new Error('Synthetic failure'); };
    for (const transport of [fixture(undefined, true), fixture({ getChatOverview: fail, getChatSender: fail, listWhisperPartners: fail })]) {
      const failing = createClient(transport);
      await expect(failing.getChatOverview(request)).rejects.toThrow();
      await expect(failing.getChatSender(senderRequest)).rejects.toThrow();
      await expect(failing.listWhisperPartners(filter)).rejects.toThrow();
    }
  });

  it('dispatches all history kinds and refreshes when their arguments change', async () => {
    vi.useFakeTimers();
    const client = createClient(fixture({ getChatOverview: () => overview, getChatSender: () => sender, listWhisperPartners: () => partners }));
    const reads = [vi.spyOn(client, 'getChatOverview'), vi.spyOn(client, 'getChatSender'), vi.spyOn(client, 'listWhisperPartners')];
    const states: HistoryState[] = [];
    const controller = createRunHistory(client, state => states.push(state));
    const queries: HistoryQuery[] = [
      { kind: 'chat-overview', request }, { kind: 'chat-overview', request: {} },
      { kind: 'chat-sender', request: senderRequest }, { kind: 'chat-sender', request: { name: 'Quenby' } },
      { kind: 'whisper-partners', filter }, { kind: 'whisper-partners', filter: { ...filter, show_hidden: false } },
      { kind: 'whisper-partners', filter: { ...filter, query: 'quen' } },
      { kind: 'whisper-partners', filter: { ...filter, limit: 1 } },
    ];
    try {
      for (const query of queries) {
        controller.update({ query, scopeKey: 'replay', streamId: null, required: null, store: null });
        await vi.advanceTimersByTimeAsync(1000);
        expect(states.at(-1)).toMatchObject({ phase: 'ready', value: { kind: query.kind } });
      }
      expect(reads.map(read => read.mock.calls.length)).toEqual([2, 2, 4]);
    } finally { controller.dispose(); vi.useRealTimers(); }
  });
});


const eventFilter: EventFilter = { kinds: [], run_id: null, dungeon: null, cursor: null, limit: 100,
  range: { start: '1', end: '10' }, query: 'Vexlin', opener_local: true };
const eventSummary: EventSummary = { total: 2, by_kind: { player_death: 0, player_left: 0, portal_opened: 2, realm_event_killed: 0 },
  dungeons: [{ dungeon: 'Synthetic vault', pops: 2, last_at: '3' }], local_pops: 1,
  last_local_pop: { dungeon: 'Synthetic vault', pops: 1, last_at: '3' }, first_at: '2', flagged_since: '1' };

describe('Event summary bridge', () => {
  it('routes the command, isolates fixtures, and supplies empty defaults', async () => {
    const transport = fixture({ getEventSummary: input => { input.filter.query = 'changed'; return eventSummary; } });
    const invoke = vi.spyOn(transport, 'invoke');
    const client = createClient(transport);
    const request = { filter: eventFilter };
    const reply = await client.getEventSummary(request);
    expect(invoke).toHaveBeenCalledWith('get_event_summary', { request });
    expect(eventFilter.query).toBe('Vexlin');
    expect(reply).toEqual({ status: 'ready', value: eventSummary });
    if (reply.status !== 'ready') throw new Error('Expected ready');
    reply.value.dungeons[0].pops = 99;
    expect(eventSummary.dungeons[0].pops).toBe(2);
    await expect(createClient(fixture()).getEventSummary(request)).resolves.toEqual({ status: 'ready', value: {
      total: 0, by_kind: { player_death: 0, player_left: 0, portal_opened: 0, realm_event_killed: 0 },
      dungeons: [], local_pops: 0, last_local_pop: null, first_at: null, flagged_since: null,
    } });
    await expect(createClient(fixture(undefined, true)).getEventSummary(request)).rejects.toThrow();
    await expect(createClient(fixture({ getEventSummary: () => { throw new Error('Synthetic failure'); } })).getEventSummary(request)).rejects.toThrow('Synthetic failure');
  });

  it('ignores query-only summary changes while refreshing event search and scoped reads', async () => {
    vi.useFakeTimers();
    const client = createClient(fixture({ getEventSummary: () => eventSummary, listInstanceEvents: () => ({ rows: [], next: null, entered_runs: [] }) }));
    const summaries = vi.spyOn(client, 'getEventSummary');
    const pages = vi.spyOn(client, 'listInstanceEvents');
    const states: HistoryState[] = [];
    const controller = createRunHistory(client, state => states.push(state));
    const filters: EventFilter[] = [eventFilter,
      { ...eventFilter, query: 'Quenby' },
      { ...eventFilter, range: { start: '2', end: '10' } },
      { ...eventFilter, range: { start: '2', end: '11' } },
      { ...eventFilter, run_id: 'synthetic-run' }];
    try {
      for (const filter of filters) {
        controller.update({ query: { kind: 'event-summary', request: { filter } }, scopeKey: 'replay', streamId: null, required: null, store: null });
        await vi.advanceTimersByTimeAsync(1000);
        expect(states.at(-1)).toMatchObject({ phase: 'ready', value: { kind: 'event-summary', summary: eventSummary } });
      }
      for (const filter of [...filters, { ...eventFilter, opener_local: false }, { ...eventFilter, opener_local: null }]) {
        controller.update({ query: { kind: 'events', filter }, scopeKey: 'replay', streamId: null, required: null, store: null });
        await vi.advanceTimersByTimeAsync(1000);
        expect(states.at(-1)).toMatchObject({ phase: 'ready', value: { kind: 'events', page: { entered_runs: [] } } });
      }
      expect(summaries).toHaveBeenCalledTimes(filters.length - 1);
      expect(pages).toHaveBeenCalledTimes(filters.length + 2);
    } finally { controller.dispose(); vi.useRealTimers(); }
  });
});


describe('Alert summary bridge', () => {
  it('routes the native command and isolates fixture arguments and replies', async () => {
    const summary = { total: 1, by_kind: [{ kind: 'white_bag' as const, count: 1 }],
      first_at: '1', last_white: { at: '1', run_id: null, dungeon: null } };
    const request = { range: { start: '1', end: '2' } };
    const transport = createFixtureTransport(demoSnapshot, undefined, undefined, undefined, {
      listAlerts: async () => ({ status: 'ready', value: { rows: [], next: null } }),
      getAlertSummary: async input => { if (input.range) input.range.start = 'changed'; return { status: 'ready', value: summary }; },
    });
    const invoke = vi.spyOn(transport, 'invoke');
    const result = await createClient(transport).getAlertSummary(request);
    expect(invoke).toHaveBeenCalledWith('get_alert_summary', { request });
    expect(request.range.start).toBe('1');
    expect(result).toEqual({ status: 'ready', value: summary });
    if (result.status !== 'ready') throw new Error('Expected ready');
    result.value.by_kind[0].count = 99;
    expect(summary.by_kind[0].count).toBe(1);
    await expect(createClient(createFixtureTransport(demoSnapshot)).getAlertSummary(request)).rejects.toThrow('Alert summary');
  });
});


it('refreshes alert history when included or excluded kinds change', async () => {
  const listAlerts = vi.fn(async () => ({ status: 'ready' as const, value: { rows: [], next: null } }));
  const history = createAlertHistory({ listAlerts }, () => {});
  const input: AlertHistoryInput = { scopeKey: 'synthetic', streamId: null, required: null, store: null,
    filter: { kind: null, range: null, query: '', cursor: null, limit: 50 } };
  history.update(input);
  await vi.waitFor(() => expect(listAlerts).toHaveBeenCalledTimes(1));
  history.update({ ...input, filter: { ...input.filter, kinds: ['whisper', 'guild_message'] } });
  await vi.waitFor(() => expect(listAlerts).toHaveBeenCalledTimes(2));
  history.update({ ...input, filter: { ...input.filter, kinds: ['whisper', 'guild_message'], exclude_kinds: ['key_popped'] } });
  await vi.waitFor(() => expect(listAlerts).toHaveBeenCalledTimes(3));
  expect(listAlerts.mock.calls.at(-1)).toEqual([{ ...input.filter, kinds: ['whisper', 'guild_message'], exclude_kinds: ['key_popped'] }]);
  history.dispose();
});
