import { expect, it } from 'vitest';
import { emptyListState, normalizeListState } from '../../src/lib/navigation/state';

const position = () => ({
  chat: { filter: { channel: 'guild', sender: 'SynthAlpha', query: 'carry', run_id: 'r1', show_npc: false, show_hidden: true,
    cursor: { at: '1000', id: 'chat-1' }, limit: 50 }, previous: [null] },
  events: { filter: { kinds: ['portal_opened'], run_id: null, dungeon: null, cursor: null, limit: 50 }, previous: [] },
});
it('retains bounded query state without storing page payloads', () => {
  const original = position();
  const normalized = normalizeListState({ logsPosition: { ...original, rows: ['private text'] } });
  expect(normalized.logsPosition?.chat).toMatchObject(original.chat);
  expect(normalized.logsPosition?.keys).toEqual(original.events);
  expect(normalized.logsPosition?.events.filter.kinds).toEqual([]);
  original.chat.filter.cursor.id = 'changed';
  expect(normalized.logsPosition?.chat.filter.cursor?.id).toBe('chat-1');
  expect(emptyListState().logsPosition).toBeNull();
});
it('rejects malformed or unsafe saved Logs query arguments', () => {
  for (const patch of [{ channel: 'bad' }, { query: 'a'.repeat(129) }, { query: 'a\0b' }, { show_npc: 1 },
    { cursor: { at: '-1', id: 'chat-1' } }, { limit: 101 }, { run_id: '' }, { sender: 'a'.repeat(33) }]) {
    const value = position();
    const normalized = normalizeListState({ logsPosition: { ...value, chat: { ...value.chat, filter: { ...value.chat.filter, ...patch } } } }).logsPosition;
    expect(normalized?.chat.filter.sender).toBeNull();
    expect(normalized?.keys).toEqual(value.events);
  }
  expect(normalizeListState({ logsPosition: { chat: position().chat } }).logsPosition?.chat).toMatchObject(position().chat);
  const value = position(); value.events.filter.kinds = ['unknown'];
  expect(normalizeListState({ logsPosition: value }).logsPosition?.events.filter.kinds).toEqual([]);
});
it('retains a valid Events slot when Chat is invalid and round-trips the shared range', () => {
  const events = { ...position().events, filter: { ...position().events.filter, kinds: ['player_death'] } };
  const range = { start: '1000', end: '2000' };
  const normalized = normalizeListState({ logsPosition: { chat: {}, events, range } });
  expect(normalized.logsPosition?.events).toMatchObject(events);
  expect(normalizeListState(normalized).logsPosition?.range).toEqual(range);
});
