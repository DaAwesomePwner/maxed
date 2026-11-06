import { expect, it } from 'vitest';
import type { DamageRow } from '../../src/lib/home/v3/types';
import { topWithSelf } from '../../src/lib/home/v4/meter';

const rows = (self: number, count = 12): DamageRow[] => Array.from({ length: count }, (_, i) => ({
  player_id: `synthetic-${i}`, object_id: i, rank: i + 1, name: `Synthetic ${i}`, class_id: null,
  is_local: i + 1 === self, damage: (count - i) * 100, dps: null, share: null,
}));

it('top_with_self_keeps_self_in_top_six_once', () => {
  const input = rows(3);
  expect(topWithSelf(input)).toEqual(input.slice(0, 6));
  expect(input).toHaveLength(12);
});

it('top_with_self_adds_gap_and_self', () => {
  const input = rows(9);
  expect(topWithSelf(input)).toEqual([...input.slice(0, 6), { gap: 2 }, input[8]]);
  expect(topWithSelf(input, 3)).toEqual([...input.slice(0, 3), { gap: 5 }, input[8]]);
});

it('top_with_self_handles_no_self_short_lists_and_adjacent_self', () => {
  expect(topWithSelf([])).toEqual([]);
  expect(topWithSelf(rows(0))).toEqual(rows(0).slice(0, 6));
  expect(topWithSelf(rows(2, 3))).toEqual(rows(2, 3));
  const input = rows(7);
  expect(topWithSelf(input)).toEqual([...input.slice(0, 6), { gap: 0 }, input[6]]);
});
