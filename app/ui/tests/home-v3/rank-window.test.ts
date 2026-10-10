import { expect, it } from 'vitest';
import { rankWindow } from '../../src/lib/home/v3/rank-window';

const players = (local: number, count = 85) => Array.from({ length: count }, (_, i) => ({
  rank: i + 1, is_local: i + 1 === local, name: `Player ${i + 1}`,
}));
const ranks = (rows: ReturnType<typeof rankWindow>) => rows.map(row => 'gap' in row ? `gap:${row.gap}` : row.rank);

it('shows five leaders when local is in them, or no unique local exists', () => {
  expect(ranks(rankWindow(players(2)))).toEqual([1, 2, 3, 4, 5]);
  expect(ranks(rankWindow(players(-1)))).toEqual([1, 2, 3, 4, 5]);
  const ambiguous = players(2); ambiguous[14].is_local = true;
  expect(ranks(rankWindow(ambiguous))).toEqual([1, 2, 3, 4, 5]);
});
it('follows rank churn through 85 players without duplicate leaders', () => {
  expect(ranks(rankWindow(players(15)))).toEqual([1, 2, 3, 'gap:10', 14, 15, 16]);
  expect(ranks(rankWindow(players(6)))).toEqual([1, 2, 3, 'gap:1', 5, 6, 7]);
  expect(ranks(rankWindow(players(85)))).toEqual([1, 2, 3, 'gap:80', 84, 85]);
});
it('does not invent neighbours for the five retained ended rows', () => {
  const rows = [...players(-1, 4), players(15)[14]];
  expect(ranks(rankWindow(rows))).toEqual([1, 2, 3, 4, 15]);
  expect(rows[4].rank).toBe(15);
});
it('keeps supplied ties, accepts unknown ranks and never mutates input', () => {
  const rows = players(15).reverse();
  const before = structuredClone(rows);
  expect(ranks(rankWindow(rows))).toEqual([1, 2, 3, 'gap:10', 14, 15, 16]);
  expect(rows).toEqual(before);
  expect(rankWindow([{ rank: null, is_local: true }])).toEqual([{ rank: null, is_local: true }]);
  expect(rankWindow([])).toEqual([]);
  expect(rankWindow(players(1), 9)).toHaveLength(9);
});
