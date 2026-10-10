import { expect, it } from 'vitest';
import type { ChatMessageView } from '../../src/lib/bindings/ChatMessageView';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { HistoryQuery } from '../../src/lib/runs/history';
import { queryInvalidates } from '../../src/lib/runs/history-dependencies';
import { homeSnapshot } from '../home/fixtures';
import { fight, run } from './fixtures';

const chat = (run_id: string | null = null, channel: ChatMessageView['channel'] = 'party'): ChatMessageView => ({
  id: 'synthetic-chat', session_id: 'synthetic', run_id, map: null, at: '1', channel,
  direction: null, sender_kind: 'player', sender: 'Synthetic', counterpart: null, is_local: false,
  stars: null, supporter: false, text: 'hello', truncated: false,
});
const instance = (run_id: string | null): EngineEvent => ({ instance_event: {
  id: 'synthetic-left', session_id: 'synthetic', run_id, map: null, at: '1', provisional: true,
  kind: { type: 'player_left', object_id: 1, player_name: 'Synthetic', class_id: null },
} });
const matches = (query: HistoryQuery, event: EngineEvent) => queryInvalidates(query, {
  stream_id: 'synthetic', sequence: '13', at: '1', event,
});
const unrelated: EngineEvent = { capture_state_changed: homeSnapshot().capture };

it.each([null, 'run-1'])('chat_overview_respects_run_scope_%s', run_id => {
  const query: HistoryQuery = { kind: 'chat-overview', request: { run_id } };
  expect(matches(query, { chat: chat('run-1') })).toBe(true);
  expect(matches(query, { chat: chat('run-2') })).toBe(run_id === null);
  expect(matches(query, { chat: chat() })).toBe(run_id === null);
  expect(matches(query, unrelated)).toBe(false);
});

it('sender_depends_on_any_chat_and_partners_on_whispers', () => {
  const sender: HistoryQuery = { kind: 'chat-sender', request: { name: 'Other Synthetic' } };
  const partners: HistoryQuery = { kind: 'whisper-partners', filter: { query: '', show_hidden: false, limit: 20 } };
  expect(matches(sender, { chat: chat() })).toBe(true);
  expect(matches(sender, unrelated)).toBe(false);
  expect(matches(partners, { chat: chat(null, 'whisper') })).toBe(true);
  expect(matches(partners, { chat: chat() })).toBe(false);
  expect(matches(partners, unrelated)).toBe(false);
});

it.each([null, 'run-1'])('event_summary_respects_run_scope_%s', run_id => {
  const query: HistoryQuery = { kind: 'event-summary', request: {
    filter: { kinds: [], run_id, dungeon: null, cursor: null, limit: 20 },
  } };
  expect(matches(query, instance('run-1'))).toBe(true);
  expect(matches(query, instance('run-2'))).toBe(run_id === null);
  expect(matches(query, instance(null))).toBe(run_id === null);
  expect(matches(query, unrelated)).toBe(false);
});

const runEvents: Array<[string, (id: string) => EngineEvent]> = [
  ...(['run_started', 'run_updated', 'run_ended'] as const).map(kind =>
    [kind, (id: string) => ({ [kind]: run({ id }) }) as EngineEvent] as [string, (id: string) => EngineEvent]),
  ...(['bag_dropped', 'drop_updated'] as const).map(kind =>
    [kind, (id: string) => ({ [kind]: { ...homeSnapshot().drops[0], run_id: id } }) as EngineEvent] as [string, (id: string) => EngineEvent]),
  ...(['fight_started', 'fight_updated', 'fight_ended'] as const).map(kind =>
    [kind, (id: string) => ({ [kind]: fight({ run_id: id }) }) as EngineEvent] as [string, (id: string) => EngineEvent]),
  ['player_died', run_id => ({ player_died: { at: '1', character_id: null, run_id } })],
  ['instance_event', instance],
];
it.each(runEvents)('run_story_matches_own_run_for_%s', (_kind, event) => {
  const query: HistoryQuery = { kind: 'run', id: 'run-1' };
  expect(matches(query, event('run-1'))).toBe(true);
  expect(matches(query, event('run-2'))).toBe(false);
});

it('run_story_depends_on_character_changes_but_not_unrelated_events', () => {
  const query: HistoryQuery = { kind: 'run', id: 'run-1' };
  expect(matches(query, { character_updated: homeSnapshot().character! })).toBe(true);
  expect(matches(query, { character_merged: { provisional_id: 'other', verified_id: 'verified' } })).toBe(true);
  expect(matches(query, { chat: chat('run-1') })).toBe(false);
  expect(matches(query, unrelated)).toBe(false);
});

it('primary_list_dependencies_stay_with_route_predicates', () => {
  expect(matches({ kind: 'runs', filter: { dungeon: null, outcome: null, range: null,
    sort: 'newest', cursor: null, limit: 20 } }, { run_updated: run() })).toBe(false);
});
