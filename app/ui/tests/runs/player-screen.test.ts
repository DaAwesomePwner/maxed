import { expect, it } from 'vitest';
import { playerNeighbors, playerRankKey, sourceRows } from '../../src/lib/runs/player-screen';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { fight, player, source } from './fixtures';

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

it('does not turn invalid saved source metrics into known values', () => {
  expect(sourceRows(player({ damage: Infinity }), emptyHomeVisuals())[0].share).toBeNull();
  const invalidHits = player({ sources: [source({ hits: Infinity })] });
  expect(sourceRows(invalidHits, emptyHomeVisuals())[0].avgHit).toBeNull();
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

it('orders tied ranks by player name for previous and next navigation', () => {
  const rows = ['Demo Zeta', 'Demo Alpha', 'Demo Beta'].map((name, i) => player({ name, rank: 1, object_id: i + 1 }));
  expect(playerNeighbors(fight({ players: rows }), 3)).toEqual({ previous: rows[1], next: rows[0] });
});
