import { expect, it } from 'vitest';
import { homeSnapshot } from '../home/fixtures';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { rankedDrops, bestItem, bestDrop, overflowSummary, rarityName } from '../../src/lib/home/v3/loot-rank';
import type { DropView } from '../../src/lib/bindings/DropView';

const visuals = { ...emptyHomeVisuals(), objects: [
  { object_type: 1, item_tier: 'UT', name: 'Crown', sprite: null, bag: null },
  { object_type: 2, item_tier: 'ST', name: 'Ring', sprite: null, bag: null },
  { object_type: 3, item_tier: 'T14', name: 'Sword', sprite: null, bag: null },
  { object_type: 4, item_tier: 'T9', name: 'Wand', sprite: null, bag: null },
] };
function bag(id: string, rank: number | null, types: number[], at = '1000000'): DropView {
  return { ...homeSnapshot().drops[0], id, at, bag_type: rank ?? 999, bag_rank: rank,
    items: types, contents_known: true, contents: types.map((item_type, slot) => ({
      item_type, slot, quantity: 1, taken_observed_at: null,
      enchant: { state: 'missing', slots: null, applied: null, ids: [] },
    })) };
}
it('orders rank, exact timestamp descending, then ID descending without mutating input', () => {
  const drops = [bag('a', 90, []), bag('z', 90, []), bag('new', 90, [], '9007199254740993'), bag('unknown', null, [])];
  const before = structuredClone(drops);
  expect(rankedDrops(drops).map(d => d.id)).toEqual(['new', 'z', 'a', 'unknown']);
  expect(drops).toEqual(before);
  expect(rankedDrops([drops[0], { ...drops[0], bag_rank: 255 }])).toHaveLength(1);
});
it('selects UT then ST then numeric tier within the highest-ranked bag', () => {
  const mixed = bag('mixed', 10, [4, 3, 2, 1]);
  expect(bestItem(mixed, visuals)?.itemType).toBe(1);
  expect(bestItem(bag('tier', 10, [4, 3]), visuals)?.itemType).toBe(3);
  expect(bestDrop([bag('white', 255, [3]), mixed], visuals)?.drop.id).toBe('white');
  expect(bestDrop([bag('white', 255, [3]), bag('orange', 90, [2])], visuals)?.item?.tier).toBe('T14');
  expect(bestDrop([bag('empty', 255, []), bag('blue', 40, [3])], visuals)?.drop.id).toBe('empty');
});
it('keeps unknown contents and rarity unknown, but uses recorded legacy items', () => {
  const drop = bag('legacy', 10, [1]);
  expect(bestItem({ ...drop, contents: [] }, visuals)?.itemType).toBe(1);
  expect(bestItem({ ...drop, contents: [], contents_known: false }, visuals)).toBeNull();
  expect(bestDrop([], visuals)).toBeNull();
  expect(bestItem(drop, emptyHomeVisuals())).toBeNull();
  expect([null, 0, -1, 1.5, 5].map(rarityName)).toEqual([null, null, null, null, null]);
  expect([1, 2, 3, 4].map(rarityName)).toEqual(['Uncommon', 'Rare', 'Legendary', 'Divine']);
});
it('groups only hidden unique bags by type in rank order', () => {
  const brown = bag('brown', 0, []);
  expect(overflowSummary([brown, bag('pink', 10, []), bag('brown2', 0, []), brown])).toEqual([
    { bagType: 10, count: 1 }, { bagType: 0, count: 2 },
  ]);
});

// Mirrors drops in scrubbed/maxed-shaitan-run/expected.json at d78d0e7.
// ObjectVisual exposes catalog tiers, but no slot type or consumable flag.
const shaitanVisuals = { ...emptyHomeVisuals(), objects: [
  { object_type: 2806, name: 'Wand of Recompense', item_tier: '12' },
  { object_type: 2812, name: 'Acropolis Armor', item_tier: '12' },
  { object_type: 2793, name: 'Potion of Life', item_tier: '4' },
  { object_type: 2591, name: 'Potion of Attack', item_tier: '2' },
  { object_type: 2612, name: 'Potion of Vitality', item_tier: '2' },
  { object_type: 65075, name: 'Forgotten Relics x14', item_tier: null },
  { object_type: 2795, name: 'Greater Health Potion', item_tier: null },
  { object_type: 5466, name: 'Potion of Defense (SB)', item_tier: null },
  { object_type: 5469, name: 'Potion of Wisdom (SB)', item_tier: null },
  { object_type: 31551, name: 'Agents of Oryx Shard x1', item_tier: null },
].map(row => ({ ...row, sprite: null, bag: null })) };

it('picks the newer bag at equal rank regardless of its item tiers', () => {
  const first = { ...bag('first', 60, [2806, 2812, 2793, 2591, 2612, 65075], '1791143305741000'), bag_type: 1726 };
  const second = { ...bag('second', 60, [2795, 5466, 5469, 31551], '1791143321503000'), bag_type: 1726 };
  expect(bestDrop([first, second], shaitanVisuals)?.drop.id).toBe('second');
  expect(bestDrop([second, first], shaitanVisuals)?.item?.itemType).toBe(2795);
});

it('keeps the store rank and time ordering for gear and potion-only bags', () => {
  const gear = bag('gear', 60, [2806]);
  for (const types of [[2795], [2793]]) {
    const potion = bag('potion', 60, types, '2000000');
    expect(bestDrop([gear, potion], shaitanVisuals)?.drop.id).toBe('potion');
    expect(bestDrop([gear, { ...potion, bag_rank: 90 }], shaitanVisuals)?.drop.id).toBe('potion');
  }
  expect(bestDrop([gear, bag('later', 60, [2812], '2000000')], shaitanVisuals)?.drop.id).toBe('later');
});

it('selects the same store-ordered bag with or without visuals and regardless of input order', () => {
  const drops = [bag('a', 255, [1], '9007199254740992'), bag('b', 255, [2], '9007199254740993'),
    bag('z', 255, [3], '9007199254740993'), bag('lower', 90, [1], '9007199254740994')];
  for (const input of [drops, [...drops].reverse()]) {
    expect(bestDrop(input, visuals)?.drop.id).toBe('z');
    expect(bestDrop(input, emptyHomeVisuals())).toEqual({ drop: drops[2], item: null });
  }
});
