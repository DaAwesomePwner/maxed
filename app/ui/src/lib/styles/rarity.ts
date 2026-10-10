import type { BagDef } from '../bindings/BagDef';
export const bagTones = ['brown', 'pink', 'purple', 'blue', 'cyan', 'red', 'orange', 'white', 'unknown'] as const;
export type BagTone = typeof bagTones[number];
/** Supply reviewed asset categories at the assets join. Never infer from IDs, names or rank. */
export function bagTone(bag: BagDef | null, reviewedBaseTypes: ReadonlyMap<number, BagTone> = new Map()): BagTone {
  if (!bag) return 'unknown';
  if (bag.is_white) return 'white';
  return reviewedBaseTypes.get(bag.base_type) ?? 'unknown';
}
