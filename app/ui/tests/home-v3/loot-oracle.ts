// Pre-Phase-2a implementation retained as a behavior oracle.
import type { DropView } from '../../src/lib/bindings/DropView';
import type { HomeVisuals } from '../../src/lib/home/visuals';
import { coverageLabels, reasonLabel } from '../../src/lib/home/evidence';
import type { LootViewProjector, ObservedLootItem } from '../../src/lib/home/v2/contracts';

export interface LootItem extends ObservedLootItem {
  slot: number;
  quantity: number | null;
  takenAt: string | null;
}

const compareId = (a: string, b: string) => a < b ? -1 : a > b ? 1 : 0;
const rank = (drop: DropView) => drop.bag_rank ?? -1;
function tierOrder(tier: string | null): [number, number] {
  if (tier === 'UT') return [3, 0];
  if (tier === 'ST') return [2, 0];
  const numeric = tier?.match(/^(?:T)?(\d+)$/);
  return numeric && Number.isSafeInteger(Number(numeric[1])) ? [1, Number(numeric[1])] : [0, 0];
}
function compareTier(a: string | null, b: string | null): number {
  const left = tierOrder(a), right = tierOrder(b);
  return right[0] - left[0] || right[1] - left[1];
}

/** Contents are authoritative when present; legacy items are repeated observed slots. */
export function lootItems(drop: DropView, visuals: HomeVisuals): LootItem[] {
  const tiers = new Map(visuals.objects.map(row => [row.object_type, row.item_tier ?? null]));
  const rows = drop.contents.length ? drop.contents.map(item => ({
    drop, itemType: item.item_type, tier: tiers.get(item.item_type) ?? null,
    slot: item.slot, quantity: item.quantity, takenAt: item.taken_observed_at, enchant: item.enchant,
  })) : drop.contents_known ? drop.items.map((itemType, slot) => ({
    drop, itemType, tier: tiers.get(itemType) ?? null, slot, quantity: null, takenAt: null, enchant: null,
  })) : [];
  return rows.sort((a, b) => compareTier(a.tier, b.tier) || a.itemType - b.itemType || a.slot - b.slot);
}

/** Input order is accepted revision order; the last projection of a bag ID wins. */
export const lootView: LootViewProjector = input => {
  const unique = [...new Map(input.drops.map(drop => [drop.id, drop])).values()];
  const items = new Map(unique.map(drop => [drop.id, lootItems(drop, input.visuals)]));
  const drops = unique.sort((a, b) => rank(b) - rank(a)
    || compareTier(items.get(a.id)![0]?.tier ?? null, items.get(b.id)![0]?.tier ?? null)
    || compareId(a.id, b.id));
  const reasons = new Set(input.reasons);
  let partial = input.partial;
  const gap = (reason: string) => { partial = true; reasons.add(reason); };
  if (input.partial && !reasons.size) reasons.add('Observation coverage is partial.');
  let bestUt: ObservedLootItem | null = null, bestSt: ObservedLootItem | null = null;
  let utCount = 0, stCount = 0, unclassifiedCount = 0;
  for (const drop of drops) {
    if (!drop.contents_known) gap('Some bag contents were not fully observed.');
    if (drop.bag_rank === null) reasons.add('Unranked bags follow ranked bags.');
    if (drop.source_estimated || (drop.source_enemy === null && drop.source_reason)) reasons.add(reasonLabel(drop.source_reason));
    for (const issue of drop.issues) gap(coverageLabels[issue]);
    for (const item of items.get(drop.id)!) {
      const quantity = item.quantity;
      const count = quantity === null ? 1 : Number.isSafeInteger(quantity) && quantity > 0 ? quantity : 0;
      if (quantity === null) gap('Legacy quantities are unavailable; counts include one observed entry per item slot.');
      else if (!count) gap('Invalid item quantities are excluded from counts.');
      if (item.tier === 'UT') { utCount += count; bestUt ??= item; }
      else if (item.tier === 'ST') { stCount += count; bestSt ??= item; }
      else if (tierOrder(item.tier)[0] === 0) {
        unclassifiedCount += count;
        gap('Some item tiers are unavailable or unclassified.');
      }
    }
  }
  return { drops, bestUt, bestSt, bagCount: drops.length, utCount, stCount, unclassifiedCount,
    partial, reasons: [...reasons] };
};
