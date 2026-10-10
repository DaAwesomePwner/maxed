import { expect, it } from 'vitest';
import { invalidatesLogsHistory } from '../../src/lib/logs/invalidation';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { ChatMessageView } from '../../src/lib/bindings/ChatMessageView';
import { invalidatesAlertHistory } from '../../src/lib/alerts/invalidation';
const message = (channel: ChatMessageView['channel'], run_id: string | null = null): ChatMessageView => ({
  id: 'live-1:chat:1', session_id: 's', run_id, map: null, at: '1', channel, direction: null, sender_kind: 'player',
  sender: 'SynthAlpha', counterpart: null, is_local: false, stars: null, supporter: false, text: 'x', truncated: false,
});
const envelope = (event: EventEnvelope['event']): EventEnvelope => ({ stream_id: 'live-1', sequence: '2', at: '1', event });

it('invalidates partner whispers and only the matching run for scoped guild chat', () => {
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'chat', channel: 'whisper', partner: 'Quenby' }, envelope({ chat: message('whisper') }))).toBe(true);
  const route = { kind: 'logs', view: 'chat', channel: 'guild', runId: 'r1' } as const;
  expect(invalidatesLogsHistory(route, envelope({ chat: message('guild', 'r1') }))).toBe(true);
  expect(invalidatesLogsHistory(route, envelope({ chat: message('guild', 'r2') }))).toBe(false);
});

it('chat events invalidate matching chat tabs only', () => {
  const guild = envelope({ chat: message('guild') });
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'chat', channel: 'all' }, guild)).toBe(true);
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'chat', channel: 'guild' }, guild)).toBe(true);
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'chat', channel: 'party' }, guild)).toBe(false);
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'chat', channel: 'all', runId: 'r1' }, guild)).toBe(false);
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'keys', channel: 'all' }, guild)).toBe(false);
});
it('instance events invalidate key pops only for portals', () => {
  const portal = envelope({ instance_event: { id: 'i', session_id: 's', run_id: null, map: null, at: '1', provisional: false,
    kind: { type: 'portal_opened', opener: 'SynthAlpha', portal_type: null, dungeon: 'Lost Halls' } } });
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'keys', channel: 'all' }, portal)).toBe(true);
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'events', channel: 'all' }, portal)).toBe(true);
  const left = envelope({ instance_event: { id: 'j', session_id: 's', run_id: null, map: null, at: '1', provisional: true,
    kind: { type: 'player_left', object_id: 1, player_name: 'SynthBravo', class_id: null } } });
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'keys', channel: 'all' }, left)).toBe(false);
});

it('refreshes Logs alert history for quiet chat occurrences without refreshing chat', () => {
  const chat = envelope({ chat: message('guild') });
  expect(invalidatesAlertHistory({ kind: 'logs', view: 'alerts', channel: 'all' }, chat)).toBe(true);
  expect(invalidatesAlertHistory({ kind: 'logs', view: 'chat', channel: 'all' }, chat)).toBe(false);
});
it('matches run-scoped chat and excludes non-Logs routes', () => {
  const chat = envelope({ chat: message('party', 'r1') });
  expect(invalidatesLogsHistory({ kind: 'logs', view: 'chat', channel: 'all', runId: 'r1' }, chat)).toBe(true);
  expect(invalidatesLogsHistory(undefined, chat)).toBe(false);
  expect(invalidatesLogsHistory({ kind: 'alerts' }, chat)).toBe(false);
});


it.each([
  [{ channel: 'public' }, false],
  [{ channel: 'unknown' }, false],
  [{ channel: 'guild' }, true],
  [{ channel: 'party' }, true],
  [{ channel: 'whisper', direction: 'incoming' }, true],
  [{ channel: 'whisper', direction: 'outgoing' }, false],
  [{ channel: 'whisper', direction: 'unknown' }, false],
  [{ channel: 'whisper', direction: null }, false],
  [{ channel: 'guild', sender_kind: 'npc' }, false],
  [{ channel: 'party', sender_kind: 'system' }, false],
  [{ channel: 'whisper', direction: 'incoming', is_local: true }, false],
  [{ channel: 'guild', is_local: true }, false],
] as [Partial<ChatMessageView>, boolean][])('only eligible chat invalidates either alert route (%j)', (fields, expected) => {
  const chat = envelope({ chat: { ...message('guild'), ...fields } });
  expect(invalidatesAlertHistory({ kind: 'alerts' }, chat)).toBe(expected);
  expect(invalidatesAlertHistory({ kind: 'logs', view: 'alerts', channel: 'all' }, chat)).toBe(expected);
});
