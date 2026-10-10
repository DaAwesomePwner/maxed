import { afterEach, expect, it, vi } from 'vitest';
import { dayGroups, listKpis, presetFor, presetRange, vsBest } from '../../src/lib/runs/runs-list';
import type { RunRowDetail } from '../../src/lib/bindings/RunRowDetail';
import { injectChicagoDate, run } from './fixtures';

afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });
export const detail = (patch: Partial<RunRowDetail> = {}): RunRowDetail => ({ run_id: 'run-1', character_name: 'TestMage', class_id: 100,
  local_share: .25, share_partial: false, fame_delta: 412, loot_preview: [], observed_drop_count: 0,
  clear_micros: 552_000_000, best_clear_micros: 552_000_000, previous_best_clear_micros: 580_000_000, is_personal_best: true, ...patch });
it.each([
  ['2026-03-08T05:59:59Z', '1772863200000000', '1772949600000000'],
  ['2026-03-08T06:00:00Z', '1772949600000000', '1773032400000000'],
  ['2026-11-01T05:00:00Z', '1793509200000000', '1793599200000000'],
])('aligns presets to Chicago days at %s', (at, start, end) => {
  injectChicagoDate(); const now = new Date(at);
  expect(presetRange('today', now)).toEqual({ start, end });
  for (const preset of ['all', '30d', '7d', 'today'] as const) expect(presetFor(presetRange(preset, now), now)).toBe(preset);
  expect(presetFor({ start: '1', end: '2' }, now)).toBeNull();
  const seven = presetRange('7d', now)!;
  expect(new Date(Number(BigInt(seven.start) / 1000n)).getDate()).toBe(new Date(now.getFullYear(), now.getMonth(), now.getDate() - 6).getDate());
});
it('groups local days and leaves unobserved fame unknown', () => {
  const now = new Date(2026, 9, 2, 12);
  const at = (day: number) => String(BigInt(new Date(2026, 8, day, 12).getTime()) * 1000n);
  const rows = [run({ id: 'today', started: at(32) }), run({ id: 'yesterday', started: at(31) }), run({ id: 'older', started: at(30) })];
  const groups = dayGroups(rows, new Map([['today', detail()]]), now);
  expect(groups.map(({ label, fame, runs, cleared }) => ({ label, fame, runs, cleared }))).toEqual([
    { label: 'Today', fame: 412, runs: 1, cleared: 1 }, { label: 'Yesterday', fame: null, runs: 1, cleared: 1 },
    { label: 'Wed, Sep 30', fame: null, runs: 1, cleared: 1 },
  ]);
});
it('formats PB improvements and slower clears without fabricating unknowns', () => {
  expect(vsBest(detail())).toEqual({ text: '−0:28', tone: 'good' });
  expect(vsBest(detail({ clear_micros: 620_000_000, best_clear_micros: 580_000_000, is_personal_best: false }))).toEqual({ text: '+0:40', tone: 'plain' });
  expect(vsBest(undefined)).toBeNull();
  expect(vsBest(detail({ clear_micros: null }))).toBeNull();
  expect(vsBest(detail({ previous_best_clear_micros: null }))).toBeNull();
  expect(vsBest(detail({ previous_best_clear_micros: undefined }))).toBeNull();
  expect(vsBest(detail({ is_personal_best: false, best_clear_micros: null }))).toBeNull();
  expect(vsBest(detail({ is_personal_best: false, previous_best_clear_micros: null }))).toEqual({ text: '0:00', tone: 'plain' });
});
it('keeps missing summary KPIs unknown', () => {
  expect(listKpis(null).map(cell => [cell.key, cell.value])).toEqual(['runs', 'deaths', 'time', 'pbs', 'fame'].map(key => [key, '—']));
});

it.each(['duration_ascending', 'duration_descending'] as const)('keeps %s rows in page order without day headers', sort => {
  const now = new Date(2026, 9, 2, 12);
  const at = (day: number) => String(new Date(2026, 9, day, 12).getTime() * 1000);
  const rows = [run({ id: 'a', started: at(2) }), run({ id: 'b', started: at(1) }), run({ id: 'c', started: at(2) })];
  const groups = dayGroups(rows, new Map(), now, sort);
  expect(groups.flatMap(group => group.rows)).toEqual(rows);
  expect(groups.every(group => group.label === null)).toBe(true);
});
