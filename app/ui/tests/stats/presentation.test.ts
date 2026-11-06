import { expect, it } from 'vitest';
import { lootRateText } from '../../src/lib/stats/presentation';

it.each([
  [0.33, '0.3'],
  [4.5, '4.5'],
  [9.96, '10'],
  [12.4, '12'],
  [0, '0'],
  [null, '—'],
] as const)('formats loot rate %s as %s', (rate, expected) => {
  expect(lootRateText(rate)).toBe(expected);
});
