import type { DropView } from '../../bindings/DropView';
import type { HomeVisuals } from '../visuals';
import { freezeProjection, uniqueDrops } from '../drop-data';
import { lootItems, type LootItem } from '../v2/loot';
import type { BagOverflow } from './types';

const rank = (drop: DropView) => Number.isFinite(drop.bag_rank) ? drop.bag_rank! : -1;
const at = (drop: DropView) => /^\d+$/.test(drop.at) ? BigInt(drop.at) : -1n;
function numericTier(tier: string | null): number {
  const numeric = tier?.match(/^(?:T)?(\d+)$/);
  return numeric && Number.isSafeInteger(Number(numeric[1])) ? Number(numeric[1]) : -1;
}
const keys = new WeakMap<DropView, { rank: number; at: bigint }>();
function sortKey(drop: DropView) {
  let key = keys.get(drop);
  if (!key) { key = { rank: rank(drop), at: at(drop) }; keys.set(drop, key); }
  return key;
}
const rankedCache = new WeakMap<DropView[], DropView[]>();
export function rankedDrops(drops: DropView[]): DropView[] {
  let ranked = rankedCache.get(drops);
  if (!ranked) {
    ranked = [...uniqueDrops(drops)];
    for (const drop of ranked) sortKey(drop);
    ranked.sort((a, b) => {
      const left = sortKey(a), right = sortKey(b);
      return right.rank - left.rank || (left.at < right.at ? 1 : left.at > right.at ? -1 : 0)
        || (a.id < b.id ? 1 : a.id > b.id ? -1 : 0);
    });
    freezeProjection(ranked);
    rankedCache.set(drops, ranked);
    rankedCache.set(ranked, ranked);
  }
  return ranked;
}
export function bestItem(drop: DropView, visuals: HomeVisuals): LootItem | null {
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
/** One shared projection per immutable input pair. Stable traversal preserves
 * the old sort's recency/ID tie breaks; slot counts stay distinct from evidence quantities. */
export function homeLoot(drops: DropView[], visuals: HomeVisuals): HomeLoot {
  let cache = lootCache.get(drops);
  const previous = cache?.get(visuals);
  if (previous) return previous;
  const ranked = rankedDrops(drops);
  const counts = { bags: ranked.length, ut: 0, st: 0, items: 0 };
  let ut: Best = null, st: Best = null, fallback: Best = null, fallbackTier = -1;
  for (const drop of ranked) {
    const items = lootItems(drop, visuals), item = items[0];
    for (const item of items) {
      counts.items++;
      if (item.tier === 'UT') counts.ut++;
      if (item.tier === 'ST') counts.st++;
    }
    if (!item) continue;
    const candidate = { drop, item }, tier = numericTier(item.tier);
    if (item.tier === 'UT') ut ??= candidate;
    if (item.tier === 'ST') st ??= candidate;
    if (!fallback || rank(drop) > rank(fallback.drop)
      || (rank(drop) === rank(fallback.drop) && tier > fallbackTier)) {
      fallback = candidate; fallbackTier = tier;
    }
  }
  const best = ut ?? st ?? fallback ?? (ranked[0] ? { drop: ranked[0], item: null } : null);
  const bags = ranked.filter(row => row.id !== best?.drop.id);
  rankedCache.set(bags, bags);
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
