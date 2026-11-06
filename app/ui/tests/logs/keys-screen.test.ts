import { setupFixture } from '../setup/fixture';
import { expect, it } from 'vitest';
import { keyKpis, mutedDungeons } from '../../src/lib/logs/keys-screen';
import type { EventSummary } from '../../src/lib/bindings/EventSummary';
import type { AlertPreferences } from '../../src/lib/bindings/AlertPreferences';
const summary: EventSummary = { total: 120, by_kind: { portal_opened: 100, player_death: 10, player_left: 10, realm_event_killed: 0 },
  dungeons: [{ dungeon: 'Invented vault', pops: 60, last_at: '86400000000' }], local_pops: 4,
  last_local_pop: { dungeon: 'Invented vault', pops: 1, last_at: '86400000000' }, first_at: '0', flagged_since: '86400000000' };
const prefs = { key_pop_dungeons: null, key_pop_excluded_dungeons: [] } as unknown as AlertPreferences;
it('uses the custom range for per-day rates and portal counts for shares', () => {
  const cells = keyKpis(summary, { start: '0', end: '864000000000' }, new Set(['Muted']), new Date(864000000));
  expect(cells.map(cell => cell.key)).toEqual(['pops', 'mine', 'most', 'dungeons']);
  expect(cells[0]).toMatchObject({ value: '100', sub: '10.0 per day' });
  expect(cells[1]).toMatchObject({ value: '4', sub: expect.stringContaining('last: Invented vault,') });
  expect(cells[2]).toMatchObject({ value: 'Invented vault', sub: '60 pops · 60% of all' });
  expect(cells[3]).toMatchObject({ value: '1', sub: '1 muted for alerts' });
});
it('shows counted-since only when local flags started after the range or all-time start', () => {
  expect(keyKpis(summary, null, new Set(), new Date())[1].tooltip).toMatch(/^Counted since /);
  expect(keyKpis(summary, { start: '86400000000', end: '864000000000' }, new Set(), new Date())[1].tooltip).toBeNull();
  expect(keyKpis({ ...summary, first_at: '86400000000' }, null, new Set(), new Date())[1].tooltip).toBeNull();
});
it('keeps missing and empty history unknown', () => {
  expect(keyKpis(null, null, new Set(), new Date()).map(cell => cell.value)).toEqual(['—', '—', '—', '—']);
  expect(keyKpis({ ...summary, by_kind: { ...summary.by_kind, portal_opened: 0 }, dungeons: [], last_local_pop: null }, null, new Set(), new Date()).map(cell => cell.value)).toEqual(['—', '—', '—', '—']);
});
it('keeps the local count unknown before opener flags were observed', () => {
  expect(keyKpis({ ...summary, flagged_since: null, local_pops: 0 }, null, new Set(), new Date())[1].value).toBe('—');
  expect(keyKpis(summary, { start: '0', end: summary.flagged_since! }, new Set(), new Date())[1].value).toBe('—');
  expect(keyKpis({ ...summary, local_pops: 0 }, null, new Set(), new Date())[1].value).toBe('0');
});
it('mirrors all, all-except and selected alert dungeon modes', () => {
  expect([...mutedDungeons(prefs, ['A', 'B'])]).toEqual([]);
  expect([...mutedDungeons({ ...prefs, key_pop_excluded_dungeons: ['B'] }, ['A', 'B'])]).toEqual(['B']);
  expect([...mutedDungeons({ ...prefs, key_pop_dungeons: ['A'], key_pop_excluded_dungeons: ['A'] }, ['A', 'B'])]).toEqual(['B']);
  expect([...mutedDungeons({ ...prefs, key_pop_dungeons: [] }, ['A', 'B'])]).toEqual(['A', 'B']);
});

it.each(['off', 'toast'] as const)('describes %s alerts with no dungeons selected', async loudness => {
  const preferences = (await setupFixture().client.querySettings()).alerts;
  preferences.key_popped = loudness;
  expect(keyKpis(null, null, new Set(['Vault']), new Date(), preferences)[3].sub).toBe(loudness === 'off' ? 'Key-pop alerts off' : 'No dungeons selected');
});
