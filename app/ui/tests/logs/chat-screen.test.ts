import { expect, it } from 'vitest';
import type { ChatRow } from '../../src/lib/bindings/ChatRow';
import type { ChatOverview } from '../../src/lib/bindings/ChatOverview';
import { placeGroups, chatSubline, senderSuggestions, copyText } from '../../src/lib/logs/chat-screen';
const now = new Date(2026, 9, 8, 21);
const row = (id: number, run: string | null, map = 'Nexus', sender = 'Vexlin'): ChatRow => ({ hidden: false, message: {
  id: String(id), session_id: 'synthetic', run_id: run, map, at: String(new Date(2026, 9, 8, 20, id).getTime() * 1000), channel: 'public', sender_kind: 'player',
  sender, direction: null, counterpart: null, is_local: false, stars: null, supporter: false, text: 'have 2 keys', truncated: false,
} });
it('groups contiguous run and map changes using the oldest visible line', () => {
  const groups = placeGroups([row(6, 'a', 'The Shatters'), row(5, 'a', 'The Shatters'), row(4, 'b', 'The Shatters'), row(3, null), row(2, null, 'Realm'), row(1, null)], now).filter(item => item.type === 'place');
  expect(groups.map(group => [group.count, group.runId])).toEqual([[2, 'a'], [1, 'b'], [1, null], [1, null], [1, null]]);
  expect(groups[0].label).toBe('Today · 8:05 PM · The Shatters');
  expect(new Set(groups.map(group => group.key)).size).toBe(5);
});
it('starts a group for a page beginning mid-place and counts only that page', () => {
  expect(placeGroups([row(4, 'a'), row(3, 'a')], now)[0]).toMatchObject({ type: 'place', count: 2, label: 'Today · 8:03 PM · Nexus' });
  expect(placeGroups([], now)).toEqual([]);
});
it('hides an unknown or empty overview and omits unknown retention', () => {
  const overview: ChatOverview = { lines: 3412, by_channel: { public: 3412, guild: 0, party: 0, whisper: 0, unknown: 0 }, first_at: null, latest_at: null };
  expect(chatSubline(null, 30)).toBeNull(); expect(chatSubline({ ...overview, lines: 0 }, 30)).toBeNull();
  expect(chatSubline(overview, 30)).toBe('3,412 chat lines · kept 30 days');
  expect(chatSubline(overview, null)).toBe('3,412 chat lines');
});
it('deduplicates suggestions without case, filters prefixes and caps at eight', () => {
  const partner = { name: 'vEXLIN', lines: 2, last_at: '0', last_text: '', last_direction: 'unknown' as const, stars: null };
  expect(senderSuggestions([row(1, null)], [partner, { ...partner, name: 'Quenby' }], 'v')).toEqual([{ name: 'Vexlin', channel: 'public' }]);
  expect(senderSuggestions(Array.from({ length: 20 }, (_, i) => row(i, null, 'Nexus', `Synth${i}`)), [], '')).toHaveLength(8);
});
it('copies plain text with the observed time and sender', () => { expect(copyText(row(54, null))).toBe('[8:54 PM] Vexlin: have 2 keys'); });
