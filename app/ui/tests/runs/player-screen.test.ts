import { expect, it } from 'vitest';
import { playerKpis, playerNeighbors, playerRankKey, sourceRows } from '../../src/lib/runs/player-screen';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { fight, player, source } from './fixtures';

it('keeps saved metrics and compares DPS only with players with known DPS', () => {
  const self = player({ rank: 1, damage: 84000, hits: 168, dps: null, share: .25 });
  const next = player({ object_id: 8, rank: 2, damage: 72000, dps: 400 });
  const cells = playerKpis(self, fight({ players: [next, self] }));
  expect(cells.map(cell => cell.key)).toEqual(['damage', 'dps', 'rank', 'hits', 'best_hit', 'deaths']);
  expect(cells[0]).toMatchObject({ value: '84,000', sub: '25% of the fight' });
  expect(cells[1]).toMatchObject({ value: '—', sub: 'party avg 400' });
  expect(cells[2]).toMatchObject({ value: '1 / 2', sub: '+12,000 over #2' });
  expect(cells[3]).toMatchObject({ value: '168', sub: '500 avg per hit' });
});

it('does not invent a next rank, missing averages or a best source', () => {
  const self = player({ rank: 1, dps: null, hits: 0, biggest_hit: null, sources: [] });
  const cells = playerKpis(self, fight({ players: [self] }));
  expect(cells[1].sub).toBe('party avg —');
  expect(cells[2].sub).toBe('—');
  expect(cells[3].sub).toBe('— avg per hit');
  expect(cells[4]).toMatchObject({ value: '—', sub: '—' });
});

it('preserves separate source evidence, tier, share and unavailable hits', () => {
  const unknownHits = { ...source(), hits: null } as unknown as ReturnType<typeof source>;
  const self = player({ damage: 100, sources: [source({ damage: 75 }), unknownHits,
    source({ kind: 'unknown', item_type: null, damage: 25 })] });
  const rows = sourceRows(self, { ...emptyHomeVisuals(), objects: [
    { object_type: 1000, name: 'Synthetic crown', item_tier: 'UT', sprite: null, bag: null },
  ] });
  expect(rows[0]).toMatchObject({ name: 'Synthetic crown', tier: 'UT', share: .75, avgHit: 25 });
  expect(rows[1]).toMatchObject({ hits: null, avgHit: null });
  expect(rows[2]).toMatchObject({ name: 'Unknown Damage', share: .25 });
  expect(new Set(rows.map(row => row.key)).size).toBe(3);
  expect(sourceRows(player({ damage: 0 }), emptyHomeVisuals())[0].share).toBeNull();
});

it('finds rank neighbors at either end and in the middle without changing input order', () => {
  const rows = [3, 1, 2].map(rank => player({ rank, object_id: rank, player_id: `p${rank}` }));
  const value = fight({ players: rows });
  expect(playerNeighbors(value, 1)).toEqual({ previous: null, next: rows[2] });
  expect(playerNeighbors(value, 2)).toEqual({ previous: rows[1], next: rows[0] });
  expect(playerNeighbors(value, 3)).toEqual({ previous: rows[2], next: null });
  expect(playerNeighbors(value, 99)).toEqual({ previous: null, next: null });
  expect(rows.map(row => row.rank)).toEqual([3, 1, 2]);
  expect(playerNeighbors(fight({ players: [rows[0], rows[0]] }), 3)).toEqual({ previous: null, next: null });
});

it('does not turn invalid saved metrics into known zero averages or shares', () => {
  const self = player({ damage: Infinity, hits: Infinity });
  expect(playerKpis(self, fight({ players: [self] }))[3].sub).toBe('— avg per hit');
  expect(sourceRows(self, emptyHomeVisuals())[0].share).toBeNull();
  const invalidHits = player({ hits: Infinity });
  expect(playerKpis(invalidHits, fight())[3].sub).toBe('— avg per hit');
});

it('places nonpositive and missing ranks after ranked players in stable order', () => {
  const rows = [null, 0, -1, 2, 1].map((rank, index) => player({ rank, object_id: index + 1 }));
  expect(rows.map(playerRankKey)).toEqual([Infinity, Infinity, Infinity, 2, 1]);
  const ordered = [...rows].sort((a, b) => playerRankKey(a) - playerRankKey(b));
  expect(ordered.map(row => row.object_id)).toEqual([5, 4, 1, 2, 3]);
  ordered.forEach((row, index) => {
    expect(playerNeighbors(fight({ players: rows }), row.object_id)).toEqual({
      previous: ordered[index - 1] ?? null, next: ordered[index + 1] ?? null,
    });
  });
});
