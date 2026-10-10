import type { Collection } from '../bindings/Collection';
import type { CollectionCell } from '../bindings/CollectionCell';
import type { ChaseItem } from '../bindings/ChaseItem';
import type { ChaseRow } from '../bindings/ChaseRow';
import type { KpiCell } from '../home/v3/types';
import { clockWhen } from './loot-format';

export type CollectionSort = 'seen' | 'recent' | 'name';
const compareTime = (a: string, b: string) => BigInt(a) < BigInt(b) ? -1 : BigInt(a) > BigInt(b) ? 1 : 0;
export function chaseOrder(rows: ChaseRow[]): ChaseRow[] {
  return [...rows].sort((a, b) => b.dry_runs - a.dry_runs || compareTime(a.item.pinned_at, b.item.pinned_at));
}
export function dungeonList(catalog: string[], chosen: string | null): string[] {
  return [...new Set([...catalog, ...(chosen === null ? [] : [chosen])])];
}
export function sortCells(cells: CollectionCell[], sort: CollectionSort, nameOf: (type: number) => string): CollectionCell[] {
  return [...cells].sort((a, b) => (sort === 'seen' ? b.count - a.count : sort === 'recent' ? compareTime(b.last_at, a.last_at) : 0)
    || nameOf(a.item_type).localeCompare(nameOf(b.item_type)) || a.item_type - b.item_type);
}
export function collectionKpis(collection: Collection | null, chase: ChaseItem[], dungeon: string | null): { cells: KpiCell[]; lastType: number | null } {
  const rows = (collection?.chase ?? []).filter(row => chase.some(item => item.item_type === row.item.item_type && item.dungeon === row.item.dungeon));
  const dry = chaseOrder(rows)[0];
  const last = rows.filter(row => row.last_at !== null).sort((a, b) => compareTime(b.last_at!, a.last_at!))[0];
  const grid = dungeon !== null && collection?.grid_dungeon === dungeon ? collection.grid : null;
  const cells: KpiCell[] = [
    { key: 'chasing', label: 'Chasing', value: chase.length ? String(chase.length) : '—', sub: collection && chase.length ? `${rows.filter(row => row.seen > 0).length} seen at least once` : null },
    { key: 'dry', label: 'Longest dry streak', value: dry ? String(dry.dry_runs) : '—', sub: dry ? `runs · #${dry.item.item_type}${dry.item.dungeon === null ? '' : ` in ${dry.item.dungeon}`}` : null, tone: 'fame' },
    { key: 'last', label: 'Last chase item seen', value: '—', sub: last ? `${clockWhen(last.last_at!, new Date())}${last.item.dungeon === null ? '' : ` · ${last.item.dungeon}`}` : null, tooltip: null },
    { key: 'seen_here', label: dungeon ? `Seen in ${dungeon}` : 'Seen in dungeon', value: grid?.length ? String(grid.length) : '—', sub: grid?.length ? `UT and ST items · ${grid.filter(cell => cell.tier === 'UT').length} UT · ${grid.filter(cell => cell.tier === 'ST').length} ST` : null },
  ];
  return { cells, lastType: last?.item.item_type ?? null };
}
