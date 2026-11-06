import type { ItemDetailLookup } from '../bridge/item-details';
import { tierClass, type TierClass } from '../loot/loot-format';
import { enchantCount, rarityName } from '../styles/enchant';
import type { ItemHoverParams } from './item-hover';

export type CardSection =
  | { kind: 'none' }
  | { kind: 'message'; text: string }
  | { kind: 'rows'; label?: string; rows: { id: number; name: string; description: string | null; unknown: boolean }[] };
export interface ItemCardModel {
  name: string; tierClass: TierClass; base: string; meta: string; rarity: { name: string; level: 1 | 2 | 3 | 4 } | null;
  section: CardSection; note?: string;
}
export function itemCardModel(params: ItemHoverParams, lookup: ItemDetailLookup | null, failed: boolean): ItemCardModel {
  const item = failed ? null : lookup?.item;
  const tier = item?.tier ?? params.tier;
  const slot = item?.slot_label ?? (item?.slot_type != null ? `Slot ${item.slot_type}` : null);
  const base = [tier, slot].filter(Boolean).join(' ');
  const count = enchantCount(params.enchant);
  const rarity = count !== null && count >= 1 && count <= 4 ? { name: rarityName(count)!, level: count as 1 | 2 | 3 | 4 } : null;
  let section: CardSection = { kind: 'none' };
  if (failed) section = { kind: 'message', text: 'Details unavailable' };
  else if (params.enchant === null || params.enchant?.state === 'missing') section = { kind: 'message', text: 'Enchants not observed' };
  else if (params.enchant?.state === 'invalid') section = { kind: 'message', text: 'Enchant data unreadable' };
  else if (params.enchant?.state === 'recorded' && params.enchant.ids.length) {
    section = {
      kind: 'rows', label: params.label,
      rows: params.enchant.ids.map(id => {
        const enchant = lookup?.enchants.find(row => row.id === id);
        const name = enchant?.name?.trim();
        return { id, name: name || `Enchant #${id}`, description: name ? enchant?.description?.trim() || null : null, unknown: lookup !== null && !name };
      }),
    };
  }
  return { name: item?.name ?? params.name ?? `Item ${params.type}`, tierClass: tierClass(tier), base, meta: [base, params.suffix].filter(Boolean).join(' · '), rarity, section, note: params.note };
}
