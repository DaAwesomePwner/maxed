import { expect, it } from 'vitest';
import type { DropView } from '../../src/lib/bindings/DropView';
import type { ObjectVisual } from '../../src/lib/bindings/ObjectVisual';
import { placeLabel, bagLabel, sourceLabel, enchantLabel, STATS_CAPTION } from '../../src/lib/loot/presentation';
import { homeSnapshot } from '../home/fixtures';

it('labels places, including bags outside a run', () => {
  expect(placeLabel(null)).toBe('Outside a run');
  expect(placeLabel('Invented Sanctum')).toBe('Invented Sanctum');
});
it('uses catalog names before white, rank and unknown-tier fallbacks', () => {
  const visual: ObjectVisual = { object_type: 1, name: 'Catalog bag', sprite: null, bag: null };
  expect(bagLabel(255, visual)).toBe('Catalog bag');
  expect(bagLabel(255, null)).toBe('White bag');
  expect(bagLabel(3, visual)).toBe('Catalog bag');
  expect(bagLabel(null, null)).toBe('Unknown tier');
  expect(bagLabel(3, { ...visual, name: null })).toBe('Rank 3');
  expect(bagLabel(3, { ...visual, bag: { object_type: 1, name: 'Metadata bag', base_type: 1, is_white: false, rank: 3, capture: true } })).toBe('Metadata bag');
});
it('keeps unknown, named, uncatalogued and estimated sources distinct', () => {
  const drop: DropView = { ...homeSnapshot().drops[0], source_enemy: null, source_estimated: false, source_reason: null };
  expect(sourceLabel(drop, null)).toEqual({ text: 'Unknown source', estimated: false, reason: null });
  expect(sourceLabel({ ...drop, source_enemy: 12 }, 'Invented boss')).toEqual({ text: 'Invented boss', estimated: false, reason: null });
  expect(sourceLabel({ ...drop, source_enemy: 12 }, null)).toEqual({ text: '#12', estimated: false, reason: null });
  expect(sourceLabel({ ...drop, source_enemy: 12, source_estimated: true, source_reason: 'Nearby encounter' }, 'Invented boss'))
    .toEqual({ text: 'Invented boss', estimated: true, reason: 'Nearby encounter' });
});
it('preserves all four enchant states', () => {
  const base = { slots: null, applied: null, ids: [] };
  expect(enchantLabel({ ...base, state: 'missing' })).toBe('Enchant unknown');
  expect(enchantLabel({ ...base, state: 'invalid' })).toBe('Enchant unreadable');
  expect(enchantLabel({ ...base, state: 'recorded_empty', applied: 0 })).toBe('No enchants');
  expect(enchantLabel({ ...base, state: 'recorded', applied: 2, ids: [10, 11] })).toBe('2 enchants');
  expect(enchantLabel({ ...base, state: 'recorded', applied: 1, ids: [10] })).toBe('1 enchant');
  expect(enchantLabel({ ...base, state: 'recorded' })).toBe('Enchant unknown');
});
it('exports the exact stats honesty caption', () => {
  expect(STATS_CAPTION).toBe("Counts the bags Maxed saw while capturing. Not a drop rate: missed captures and bags out of view aren't counted.");
});
