import type { DropView } from '../bindings/DropView';
import type { EnchantView } from '../bindings/EnchantView';
import type { ObjectVisual } from '../bindings/ObjectVisual';

export function plural(count: number, singular: string, multiple = `${singular}s`): string {
  return count === 1 ? singular : multiple;
}
export function placeLabel(dungeon: string | null): string {
  return dungeon ?? 'Outside a run';
}
export function bagLabel(rank: number | null, visual: ObjectVisual | null): string {
  const name = visual?.bag?.name ?? visual?.name;
  return name?.replace(/^B\.\s*(.+)$/i, '$1 bag') ?? (rank === 255 ? 'White bag' : rank === null ? 'Unknown tier' : `Rank ${rank}`);
}
export function itemTierLabel(tier: string | null | undefined): string {
  return tier == null ? '—' : /^\d+$/.test(tier) ? `T${tier}` : tier;
}
export function sourceLabel(drop: DropView, name: string | null): { text: string; estimated: boolean; reason: string | null } {
  return { text: drop.source_enemy === null ? 'Unknown source' : name ?? `#${drop.source_enemy}`,
    estimated: drop.source_estimated, reason: drop.source_reason };
}
export function enchantLabel(enchant: EnchantView): string {
  switch (enchant.state) {
    case 'missing': return 'Enchant unknown';
    case 'invalid': return 'Enchant unreadable';
    case 'recorded_empty': return 'No enchants';
    case 'recorded': return enchant.applied === null ? 'Enchant unknown' : `${enchant.applied} ${enchant.applied === 1 ? 'enchant' : 'enchants'}`;
  }
}
export const STATS_CAPTION: string = "Counts the bags Maxed saw while capturing. Not a drop rate: missed captures and bags out of view aren't counted.";
