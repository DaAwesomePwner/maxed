import { afterEach, expect, it, vi } from 'vitest';
import type { ChatRow } from '../../src/lib/bindings/ChatRow';
import { injectChicagoDate } from '../runs/fixtures';
import { excerpt, threadItems } from '../../src/lib/logs/whisper-screen';

afterEach(() => vi.unstubAllGlobals());
const row = (id: string, at: string, map = 'Nexus'): ChatRow => ({ hidden: false, message: {
  id, at: String(Date.parse(at) * 1000), session_id: 'synthetic', map, channel: 'whisper',
  sender: 'Vexlin', sender_kind: 'player', direction: 'incoming', is_local: false,
  supporter: false, text: 'Ready?', truncated: false,
} });
it('orders two pages oldest first and splits at Chicago midnight and place changes', () => {
  injectChicagoDate();
  const older = row('1', '2026-10-08T04:59:00Z');
  const middle = row('2', '2026-10-08T05:01:00Z');
  const newest = row('3', '2026-10-08T05:02:00Z', 'Invented vault');
  newest.message.direction = 'outgoing';
  expect(threadItems([[newest, middle], [older]], new Date('2026-10-08T18:00:00Z'))).toEqual([
    { type: 'day', label: 'Yesterday · Nexus' }, { type: 'line', row: older, outgoing: false },
    { type: 'day', label: 'Today · Nexus' }, { type: 'line', row: middle, outgoing: false },
    { type: 'day', label: 'Today · Invented vault' }, { type: 'line', row: newest, outgoing: true },
  ]);
});
it('does not invent a direction for an unknown excerpt', () => {
  const partner = { name: 'Vexlin', lines: 1, last_at: '0', last_text: 'see you tomorrow', stars: null };
  expect(excerpt({ ...partner, last_direction: 'unknown' })).toBe('see you tomorrow');
  expect(excerpt({ ...partner, last_direction: 'outgoing' })).toBe('you: see you tomorrow');
});
