import { expect, it } from 'vitest';
import { createLogsPending, matchesPendingView } from '../../src/lib/logs/pending';
import { defaultLogsPosition, emptyListState } from '../../src/lib/navigation/state';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';

it('counts envelopes rather than sequence gaps and only clears the applied checkpoint', () => {
  const pending = createLogsPending();
  pending.accept('chat', '10'); pending.accept('chat', '100'); pending.accept('chat', '101');
  expect(pending.count('chat')).toBe(3);
  expect(pending.applied('chat', '100')).toBe(1);
  pending.applied('obsolete', '200'); expect(pending.count('chat')).toBe(1);
  expect(pending.count('keys')).toBe(0);
  pending.accept('keys', '105'); expect(pending.count('keys')).toBe(1);
  pending.reset(); expect(pending.count('keys')).toBe(0);
});

it('ignores unrelated checkpoints and NPC lines with NPC disabled', () => {
  const route = { kind: 'logs', view: 'chat', channel: 'all' } as const;
  const state = { ...emptyListState(), logsPosition: defaultLogsPosition() };
  const envelope = (event: EventEnvelope['event']): EventEnvelope => ({ stream_id: 'synthetic', sequence: '2', at: '2', event });
  expect(matchesPendingView(route, state, envelope({ run_updated: {} } as EventEnvelope['event']))).toBe(false);
  const npc = envelope({ chat: { sender_kind: 'npc', channel: 'public' } } as EventEnvelope['event']);
  expect(matchesPendingView(route, state, npc)).toBe(false);
  state.logsPosition.chat.filter.show_npc = true;
  expect(matchesPendingView(route, state, npc)).toBe(true);
  expect(matchesPendingView({ ...route, channel: 'whisper' }, state, npc)).toBe(false);
});

it('narrows event kinds, ranges and local dungeon openers before counting', () => {
  const state = { ...emptyListState(), logsPosition: defaultLogsPosition() };
  const envelope = { stream_id: 'synthetic', sequence: '2', at: '15', event: {
    instance_event: { id: 'portal', session_id: 'synthetic', at: '15', provisional: false,
      kind: { type: 'portal_opened', dungeon: 'Invented vault', opener: 'Vexlin', opener_is_local: true } },
  } } as EventEnvelope;
  const route = { kind: 'logs', view: 'keys', channel: 'all' } as const;
  expect(matchesPendingView(route, state, envelope)).toBe(true);
  state.logsPosition.range = { start: '20', end: '30' };
  expect(matchesPendingView(route, state, envelope)).toBe(false);
  state.logsPosition.range = { start: '10', end: '20' };
  state.logsPosition.keys.filter.opener_local = false;
  expect(matchesPendingView(route, state, envelope)).toBe(false);
  state.logsPosition.keys.filter.opener_local = true;
  expect(matchesPendingView(route, state, envelope)).toBe(true);
  state.logsPosition.events.filter.kinds = ['player_death'];
  expect(matchesPendingView({ ...route, view: 'events' }, state, envelope)).toBe(false);
});
