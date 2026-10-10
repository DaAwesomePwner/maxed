import type { EnchantView } from '../bindings/EnchantView';
import type { SpriteRef } from '../bindings/SpriteRef';

/** In-game rarity by enchant count: 0 Common through 4 Divine. */
export const rarityNames = ['Common', 'Uncommon', 'Rare', 'Legendary', 'Divine'] as const;
export type RarityName = typeof rarityNames[number];
export const MAX_ENCHANTS = 4;
/** The extractor's own strip of the client's RarityIcon_1..4: pip n at x = 16(n - 1). */
export const PIP_SHEET = 'enchantPips.png';
const PIP_ART = 16;

/** Enchants always fill every slot an item supports, so the applied count is the
 * pip count. Null when the client sent no usable enchant evidence for the item. */
export function enchantCount(enchant: EnchantView | null | undefined): number | null {
  if (!enchant || (enchant.state !== 'recorded' && enchant.state !== 'recorded_empty')) return null;
  const count = enchant.applied;
  return count !== null && Number.isSafeInteger(count) && count >= 0 && count <= MAX_ENCHANTS ? count : null;
}

export function rarityName(count: number | null): RarityName | null {
  return count === null ? null : rarityNames[count] ?? null;
}

/** RarityIcon_n for 1..4 enchants; no pip without enchants. */
export function pipSprite(count: number | null): SpriteRef | null {
  if (count === null || !Number.isInteger(count) || count < 1 || count > MAX_ENCHANTS) return null;
  return { key: `enchantPip:${count}`, sheet: PIP_SHEET, x: (count - 1) * PIP_ART, y: 0, width: PIP_ART, height: PIP_ART };
}

/** Pip edge in px: the art's 16px on 40px+ tiles, 12px on 24–39px tiles, 8px below. */
export function pipSize(tile: number): number {
  return tile >= 40 ? PIP_ART : tile >= 24 ? 12 : 8;
}

export function pipLabel(count: number): string {
  return `${rarityName(count)}, ${count} ${count === 1 ? 'enchant' : 'enchants'}`;
}
