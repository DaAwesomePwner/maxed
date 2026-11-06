import { expect, it } from 'vitest';
import { fightKpis, fightNeighbors, fightSourceRows, seriesForChart } from '../../src/lib/runs/fight-screen';
import { dpsMetric } from '../../src/lib/runs/dps-metric';
import { fight, player, source } from './fixtures';
import type { RunFightStripRow } from '../../src/lib/bindings/RunFightStripRow';

it('uses saved personal metrics and preserves missing local identity', () => {
  const self = player();
  const cells = fightKpis(fight(), self);
  expect(cells.map(cell => cell.key)).toEqual(['duration', 'damage', 'party_dps', 'your_dps', 'best_hit']);
  expect(cells.find(cell => cell.key === 'your_dps')).toMatchObject({ label: 'Active DPS', tooltip: dpsMetric('player_active_window_v1').tooltip, value: '17', sub: 'rank 4 of 1 · 23% share' });
  expect(cells.find(cell => cell.key === 'best_hit')?.value).toBe('123');
  expect(fightKpis(fight(), null).slice(3).map(cell => cell.value)).toEqual(['—', '—']);
  expect(fightKpis(fight({ first_hit: null }), null)[2].value).toBe('—');
});
const strip: RunFightStripRow[] = ['a', 'b', 'c'].map(fight_id => ({ fight_id, target_type: null,
  target_name: null, boss_type: null, started: '0', ended: null, killed: false, player_count: 0,
  damage: null, rank: null, share: null }));
it('finds neighbors at either end, in the middle, and outside the strip', () => {
  expect(fightNeighbors(strip, 'a')).toEqual({ index: 0, count: 3, previous: null, next: strip[1] });
  expect(fightNeighbors(strip, 'b')).toEqual({ index: 1, count: 3, previous: strip[0], next: strip[2] });
  expect(fightNeighbors(strip, 'c')).toEqual({ index: 2, count: 3, previous: strip[1], next: null });
  expect(fightNeighbors([], 'a')).toEqual({ index: -1, count: 0, previous: null, next: null });
});
it('falls back to bucket local damage without fabricating other player series', () => {
  const value = fight({ player_series: undefined, buckets: [
    { from: '0', until: '1000000', total: 40, local: 10 },
    { from: '1000000', until: '3000000', total: 80, local: null },
  ] });
  const rows = seriesForChart(value, ['party', value.players[0].player_id, 'missing']);
  expect(rows.map(row => row.label)).toEqual([value.players[0].name, 'Party']);
  expect(rows[1].points).toEqual([40, 80]);
  expect(rows[0].points[0]).toBe(10);
  expect(Number.isNaN(rows[0].points[1])).toBe(true);
  expect(seriesForChart(value, [])).toEqual([]);
});
it('rejects ambiguous or misaligned player series', () => {
  const self = player();
  const row = { player_id: self.player_id, object_id: self.object_id, is_local: true, points: [5] };
  const value = fight({ buckets: [{ from: '0', until: '1000000', total: 40, local: 10 }], player_series: [row, row] });
  expect(seriesForChart(value, [self.player_id])[0].points).toEqual([10]);
});

it('merges item damage before ranking while retaining the estimated fraction', () => {
  const sources = [source({ damage: 68 }), source({ damage: 32, estimated: true }),
    source({ item_type: 1001, damage: 80 }), source({ item_type: null, damage: 4, estimated: true })];
  const before = structuredClone(sources);
  expect(fightSourceRows(sources).map(({ item_type, damage, estimatedDamage }) => ({ item_type, damage, estimatedDamage })))
    .toEqual([{ item_type: 1000, damage: 100, estimatedDamage: 32 },
      { item_type: 1001, damage: 80, estimatedDamage: 0 }, { item_type: null, damage: 4, estimatedDamage: 4 }]);
  expect(sources).toEqual(before);
});
it('does not assign one enchant snapshot to multiple different item copies', () => {
  expect(fightSourceRows([source(), source({ enchant: { state: 'recorded', slots: 1, applied: 1, ids: [1] } })])[0].enchant).toBeNull();
});

it('keeps legacy and missing-basis fight values labelled as whole-fight DPS', () => {
  for (const basis of ['whole_target_v0', undefined] as const) {
    const value = fight({ dps_basis: basis as ReturnType<typeof fight>['dps_basis'] });
    expect(fightKpis(value, player()).find(cell => cell.key === 'your_dps'))
      .toMatchObject({ label: 'DPS', tooltip: expect.stringContaining('whole fight'), value: '17' });
  }
});
