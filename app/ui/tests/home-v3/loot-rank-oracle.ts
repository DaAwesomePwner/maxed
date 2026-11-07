// Uncached reference for the documented bag order (updated for issue #15).
import type { DropView } from '../../src/lib/bindings/DropView';
import type { HomeVisuals } from '../../src/lib/home/visuals';
import { lootItems, type LootItem } from './loot-oracle';
import type { BagOverflow } from '../../src/lib/home/v3/types';

const rank = (drop: DropView) => Number.isFinite(drop.bag_rank) ? drop.bag_rank! : -1;
const at = (drop: DropView) => /^\d+$/.test(drop.at) ? BigInt(drop.at) : -1n;
export function rankedDrops(drops: DropView[]): DropView[] {
  return [...new Map(drops.map(drop => [drop.id, drop])).values()].sort((a, b) =>
    rank(b) - rank(a) || (at(a) < at(b) ? 1 : at(a) > at(b) ? -1 : 0)
    || (a.id < b.id ? 1 : a.id > b.id ? -1 : 0));
}
export function bestItem(drop: DropView, visuals: HomeVisuals): LootItem | null {
  return lootItems(drop, visuals)[0] ?? null;
}
export function bestDrop(drops: DropView[], visuals: HomeVisuals): { drop: DropView; item: LootItem | null } | null {
  const ranked = rankedDrops(drops);
  return ranked[0] ? { drop: ranked[0], item: bestItem(ranked[0], visuals) } : null;
}
export function overflowSummary(drops: DropView[]): BagOverflow[] {
  const counts = new Map<number, BagOverflow>();
  for (const drop of rankedDrops(drops)) {
    const row = counts.get(drop.bag_type);
    if (row) row.count++;
    else counts.set(drop.bag_type, { bagType: drop.bag_type, count: 1 });
  }
  return [...counts.values()];
}
export function rarityName(count: number | null): 'Uncommon' | 'Rare' | 'Legendary' | 'Divine' | null {
  return count === 1 ? 'Uncommon' : count === 2 ? 'Rare' : count === 3 ? 'Legendary' : count === 4 ? 'Divine' : null;
}
