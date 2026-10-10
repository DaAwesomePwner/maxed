import type { LootStats } from '../bindings/LootStats';
import type { DungeonLootStats } from '../bindings/DungeonLootStats';
import type { BagTierCount } from '../bindings/BagTierCount';
import type { KpiCell } from '../home/v3/types';
import { shortWhen, whiteRate } from './loot-format';

export const statsSortKeys = ['dungeon', 'runs_ended', 'clears', 'bags', 'whites', 'rate', 'runs_since_last_white', 'best'] as const;
export type StatsSort = typeof statsSortKeys[number];
export const bagTotal = (bags: BagTierCount[]) => bags.reduce((sum, tier) => sum + tier.count, 0);

/** An ordinal tier shade, not an inferred bag colour. A lone rank uses full violet. */
export function rankShade(rank: number | null, ranks: number[]): string {
  if (rank === 255) return 'var(--bag-white)';
  if (rank === null) return 'var(--text-muted)';
  const ordered = [...new Set([...ranks, rank])].filter(value => value !== 255).sort((a, b) => a - b);
  const percent = ordered.length === 1 ? 100 : 35 + 65 * ordered.indexOf(rank) / (ordered.length - 1);
  return `color-mix(in srgb, var(--violet) ${percent}%, transparent)`;
}

export function bestDungeon(rows: DungeonLootStats[]): DungeonLootStats | undefined {
  return rows.filter(row => row.best).sort((a, b) =>
    b.best!.bag_rank - a.best!.bag_rank || (BigInt(a.best!.at) > BigInt(b.best!.at) ? -1 : BigInt(a.best!.at) < BigInt(b.best!.at) ? 1 : a.dungeon.localeCompare(b.dungeon)))[0];
}

export function statsKpis(stats: LootStats): KpiCell[] {
  const runs = stats.dungeons.reduce((sum, row) => sum + row.runs_ended, 0);
  const clears = stats.dungeons.reduce((sum, row) => sum + row.clears, 0);
  const bags = bagTotal(stats.totals);
  const empty = !stats.dungeons.length && !stats.sources.length && !bags && !stats.whites;
  const dry = sortDungeons(stats.dungeons, 'runs_since_last_white', true).find(row => row.runs_since_last_white !== null);
  const best = bestDungeon(stats.dungeons);
  return [
    { key: 'runs', label: 'Runs', value: empty ? '—' : String(runs), sub: empty ? null : `${clears} cleared · ${runs ? `${Math.round(clears / runs * 100)}%` : '—'}` },
    { key: 'bags', label: 'Bags', value: empty ? '—' : String(bags), sub: empty ? null : `${runs ? (bags / runs).toFixed(1) : '—'} per run` },
    { key: 'whites', label: 'White bags', value: empty ? '—' : String(stats.whites), sub: empty ? null : `${whiteRate(stats.whites, runs)} white rate` },
    { key: 'dry', label: 'Longest dry streak', value: dry ? String(dry.runs_since_last_white) : '—', sub: dry ? `runs of ${dry.dungeon} since a white` : null, tone: 'fame' },
    { key: 'best', label: 'Best bag', value: best ? best.best!.bag_rank === 255 ? 'White bag' : 'Bag' : '—', sub: best ? `${best.dungeon} · ${shortWhen(best.best!.at, new Date())}` : null },
  ];
}

export function stackSegments(bags: BagTierCount[]): Array<{ rank: number | null; bagType: number | null; percent: number }> {
  const total = bagTotal(bags);
  return bags.filter(tier => tier.count > 0).map(tier => ({ rank: tier.bag_rank, bagType: tier.bag_type ?? null, percent: tier.count / total * 100 }));
}

export function sortDungeons(rows: DungeonLootStats[], key: StatsSort, descending: boolean): DungeonLootStats[] {
  const value = (row: DungeonLootStats) => key === 'bags' ? bagTotal(row.bags)
    : key === 'rate' ? row.runs_ended === 0 ? null : row.whites / row.runs_ended
    : key === 'best' ? row.best?.bag_rank ?? null : row[key];
  return [...rows].sort((a, b) => {
    const left = value(a), right = value(b);
    if (left === null || right === null) return left === right ? a.dungeon.localeCompare(b.dungeon) : left === null ? 1 : -1;
    const order = typeof left === 'string' && typeof right === 'string' ? left.localeCompare(right) : Number(left) - Number(right);
    return (descending ? -order : order) || a.dungeon.localeCompare(b.dungeon);
  });
}
