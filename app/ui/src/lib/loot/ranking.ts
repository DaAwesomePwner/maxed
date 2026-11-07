import type { DropView } from '../bindings/DropView';
import { freezeProjection, uniqueDrops } from '../home/drop-data';

export function tierOrder(tier: string | null): [number, number] {
  if (tier === 'UT') return [3, 0];
  if (tier === 'ST') return [2, 0];
  const numeric = tier?.match(/^(?:T)?(\d+)$/);
  return numeric && Number.isSafeInteger(Number(numeric[1])) ? [1, Number(numeric[1])] : [0, 0];
}
export function compareItems(a: { tier: string | null; itemType: number; slot: number },
  b: { tier: string | null; itemType: number; slot: number }): number {
  const left = tierOrder(a.tier), right = tierOrder(b.tier);
  return right[0] - left[0] || right[1] - left[1] || a.itemType - b.itemType || a.slot - b.slot;
}

const rank = (drop: DropView) => Number.isFinite(drop.bag_rank) ? drop.bag_rank! : -1;
const at = (drop: DropView) => /^\d+$/.test(drop.at) ? BigInt(drop.at) : -1n;
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
