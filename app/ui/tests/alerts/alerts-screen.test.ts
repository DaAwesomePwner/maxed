import { expect, it } from 'vitest';
import { alertAction, alertKpis, shownAs, tileFilter, whyItFired } from '../../src/lib/alerts/alerts-screen';
import { alertKinds, alertKindDisplayOrder } from '../../src/lib/alerts/filters';
import { ALERT_KINDS } from '../../src/lib/bindings/alert-kinds';
import { alert } from '../phase2/fixtures';
it('keeps the established display order as a permutation of the generated alert kinds', () => {
  expect(alertKindDisplayOrder).toEqual(['white_bag', 'personal_best', 'key_popped', 'whisper', 'guild_message', 'party_message', 'death', 'flawless_run']);
  expect([...alertKindDisplayOrder].sort()).toEqual([...ALERT_KINDS].sort());
  expect(alertKinds).toEqual(ALERT_KINDS);
});
it('maps tiles to server filters and clears incompatible kinds', () => {
  expect(tileFilter('messages')).toEqual({ kind: null, kinds: ['whisper', 'guild_message', 'party_message'] });
  expect(tileFilter('white')).toEqual({ kind: 'white_bag', kinds: [] });
  expect(tileFilter('pb').kind).toBe('personal_best'); expect(tileFilter('keys').kind).toBe('key_popped'); expect(tileFilter('all').kind).toBeNull();
});
it('keeps unknown and empty KPI values unknown and never invents a PB split', () => {
  const now = new Date('2030-01-01T12:00:00Z');
  expect(alertKpis(null, null, 0, now).every(cell => cell.value === '—')).toBe(true);
  expect(alertKpis({ total: 0, by_kind: [], last_white: null, first_at: null }, null, 0, now).every(cell => cell.value === '—')).toBe(true);
  const cells = alertKpis({ total: 10, by_kind: [{ kind: 'personal_best', count: 2 }, { kind: 'whisper', count: 3 }], last_white: null, first_at: '1' }, null, 4, now);
  expect(cells[2]).toMatchObject({ value: '2', sub: 'best DPS and clear times' });
  expect(cells[3].sub).toBe('4 dungeons muted'); expect(cells[4].sub).toBe('3 whisper · 0 guild · 0 party'); expect(cells[1].sub).toBe('—');
});
it('labels saved presentation, including missing legacy values', () => {
  expect([undefined, null, 'banner', 'toast', 'quiet_log_only', 'off'].map(value => shownAs(value as Parameters<typeof shownAs>[0]))).toEqual(['—', '—', 'Banner', 'Toast', 'Log only', 'Off']);
});
it.each(alertKinds)('composes %s reasons without stored evidence and handles missing details', kind => {
  const value = alert({ kind, details: null, presentation: null, explanation: 'PRIVATE evidence' });
  expect(whyItFired(value)).not.toContain('PRIVATE'); expect(whyItFired(value)).toContain('presentation is unknown');
});
it('uses saved run/fight destinations, channel/partner routes, and dungeon actions', () => {
  expect(alertAction(alert())).toEqual({ label: 'View fight', href: '#/runs/run-1/fights/fight-1' });
  expect(alertAction(alert({ kind: 'death', details: null }))).toEqual({ label: 'View run', href: '#/runs/run-1/death' });
  expect(alertAction(alert({ kind: 'white_bag', details: null }))?.href).toBe('#/runs/run-1');
  expect(alertAction(alert({ kind: 'personal_best', details: { type: 'run_time_personal_best', dungeon: 'Invented vault', duration_micros: 1, previous_micros: 2 } }))?.label).toBe('View run');
  for (const channel of ['whisper', 'guild', 'party'] as const) {
    const kind = channel === 'whisper' ? 'whisper' : channel === 'guild' ? 'guild_message' : 'party_message';
    expect(alertAction(alert({ kind, details: { type: 'chat', channel, sender: 'Quenby', excerpt: 'Hi' } }))?.href).toBe(channel === 'whisper' ? '#/logs/chat/whisper/Quenby' : `#/logs/chat/${channel}`);
  }
  expect(alertAction(alert({ kind: 'key_popped', details: { type: 'key_popped', dungeon: 'Invented vault' } }))).toEqual({ label: 'Mute', mute: 'Invented vault' });
  expect(alertAction(alert({ kind: 'flawless_run', details: null }))?.href).toBe('#/runs/run-1');
  for (const kind of alertKinds) expect(alertAction(alert({ kind, details: null, run_id: null }))).toBeNull();
});

it('avoids invalid partner routes for overlong synthetic senders', () => {
  expect(alertAction(alert({ kind: 'whisper', details: { type: 'chat', channel: 'whisper', sender: 'Q'.repeat(64), excerpt: '' } }))?.href).toBe('#/logs/chat/whisper');
});

it('omits an unknown dungeon from the last white bag sub-line', () => {
  const now = new Date('2030-01-01T12:00:00Z'), at = String(now.getTime() * 1000);
  const cells = alertKpis({ total: 1, by_kind: [{ kind: 'white_bag', count: 1 }], last_white: { at, run_id: null, dungeon: null }, first_at: at }, null, 0, now);
  expect(cells[1].sub).toBe('last: today');
});
