import type { ItemHistory } from '../bindings/ItemHistory';
import type { ChaseItem } from '../bindings/ChaseItem';
import type { KpiCell } from '../home/v3/types';
import { placeLabel, plural } from './presentation';

export function itemKpis(history: ItemHistory, chase: ChaseItem[]): KpiCell[] {
  const t = history.totals, most = history.by_dungeon[0];
  const chased = chase.find(item => item.item_type === history.item_type && item.dungeon !== null);
  const dry = chased ? history.by_dungeon.find(place => place.dungeon === chased.dungeon) : most;
  const variants = history.enchant_variants.filter(variant => variant.state === 'recorded' && (variant.applied ?? 0) > 0);
  const best = Math.max(0, ...variants.map(variant => variant.applied!));
  const count = (value: number) => value.toLocaleString('en-US');
  const dungeon = chased?.dungeon ?? dry?.dungeon;
  return [
    { key: 'seen', label: 'Seen', value: count(t.appearances), sub: `in ${count(t.bags)} ${plural(t.bags, 'bag')}` },
    { key: 'taken', label: 'Taken', value: count(t.taken), sub: 'removed from the bag' },
    { key: 'most', label: 'Most seen in', value: most ? placeLabel(most.dungeon) : '—', sub: most ? `${count(most.count)} of ${count(t.appearances)}` : null, tooltip: most ? placeLabel(most.dungeon) : null },
    { key: 'enchanted', label: 'Enchanted', value: count(variants.reduce((sum, variant) => sum + variant.count, 0)), sub: best > 0 ? `best: ${best} ${plural(best, 'enchant')}` : null },
    { key: 'dry', label: 'Dry streak', value: dry?.dungeon != null && dry.runs_since != null ? count(dry.runs_since) : '—', sub: dungeon != null ? `runs of ${dungeon} since last seen` : null },
  ];
}

export function shareRows<T extends { count: number }>(rows: T[]): Array<T & { share: number }> {
  const max = Math.max(0, ...rows.map(row => row.count));
  return rows.map(row => ({ ...row, share: max ? row.count / max : 0 }));
}
