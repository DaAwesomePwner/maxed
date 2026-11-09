import type { DropView } from '../../bindings/DropView';
import type { HomeVisuals } from '../visuals';
import { freezeProjection } from '../drop-data';
import { lootItems, type LootItem } from '../v2/loot';
import type { BagOverflow } from './types';
import { rankedDrops } from '../../loot/ranking';
export { rankedDrops } from '../../loot/ranking';

export function bestItem(drop: DropView, visuals: HomeVisuals): LootItem | null {
  if (visuals.phase !== 'ready' && !visuals.objects.length) return null;
  return lootItems(drop, visuals)[0] ?? null;
}
type Best = { drop: DropView; item: LootItem | null } | null;
interface HomeLoot {
  ranked: DropView[];
  best: Best;
  bags: DropView[];
  counts: { bags: number; ut: number; st: number; items: number };
}
const lootCache = new WeakMap<DropView[], WeakMap<HomeVisuals, HomeLoot>>();
/** One shared projection per immutable input pair; bag order matches the store. */
export function homeLoot(drops: DropView[], visuals: HomeVisuals): HomeLoot {
  let cache = lootCache.get(drops);
  const previous = cache?.get(visuals);
  if (previous) return previous;
  const ranked = rankedDrops(drops);
  const counts = { bags: ranked.length, ut: 0, st: 0, items: 0 };
  for (const drop of ranked) {
    const items = lootItems(drop, visuals);
    for (const item of items) {
      counts.items++;
      if (item.tier === 'UT') counts.ut++;
      if (item.tier === 'ST') counts.st++;
    }
  }
  const best = ranked[0] ? { drop: ranked[0], item: bestItem(ranked[0], visuals) } : null;
  const bags = rankedDrops(ranked.filter(row => row.id !== best?.drop.id));
  if (best) freezeProjection(best);
  const result = freezeProjection({ ranked, best, bags: freezeProjection(bags), counts: freezeProjection(counts) });
  if (!cache) { cache = new WeakMap(); lootCache.set(drops, cache); }
  cache.set(visuals, result);
  return result;
}
export function bestDrop(drops: DropView[], visuals: HomeVisuals): Best {
  return homeLoot(drops, visuals).best;
}
const overflowCache = new WeakMap<DropView[], BagOverflow[]>();
export function overflowSummary(drops: DropView[]): BagOverflow[] {
  const previous = overflowCache.get(drops);
  if (previous) return previous;
  const counts = new Map<number, BagOverflow>();
  for (const drop of rankedDrops(drops)) {
    const row = counts.get(drop.bag_type);
    if (row) row.count++;
    else counts.set(drop.bag_type, { bagType: drop.bag_type, count: 1 });
  }
  const result = [...counts.values()];
  overflowCache.set(drops, result);
  return result;
}
export function rarityName(count: number | null): 'Uncommon' | 'Rare' | 'Legendary' | 'Divine' | null {
  return count === 1 ? 'Uncommon' : count === 2 ? 'Rare' : count === 3 ? 'Legendary' : count === 4 ? 'Divine' : null;
}
