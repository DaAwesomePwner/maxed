import type { FightView } from '../bindings/FightView';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import type { RunFightStripRow } from '../bindings/RunFightStripRow';
import type { KpiCell } from '../home/v3/types';
import { formatDps, formatElapsed, formatShare, formatStat } from '../styles/format';
import { damageRank, nonnegative } from './damage';

export function fightKpis(fight: FightView, self: PlayerDamage | null): KpiCell[] {
  const duration = fight.first_hit !== null && fight.last_hit !== null
    ? formatElapsed(fight.first_hit, fight.last_hit) : '—';
  const seconds = duration !== '—' ? Number(BigInt(fight.last_hit!) - BigInt(fight.first_hit!)) / 1_000_000 : 0;
  const rates = fight.buckets.map(bucket => bucketRate(bucket.total, bucket.from, bucket.until)).filter(Number.isFinite);
  return [
    { key: 'duration', label: 'Duration', value: duration, sub: 'first to last hit' },
    { key: 'damage', label: 'Damage', value: formatStat(nonnegative(fight.total_damage)), sub: `${fight.players.length} players · ${formatStat(nonnegative(fight.unattributed_damage))} unattributed` },
    { key: 'party_dps', label: 'Party DPS', value: formatDps(seconds > 0 ? fight.total_damage / seconds : null), sub: `peak ${formatDps(rates.length ? Math.max(...rates) : null)}` },
    { key: 'your_dps', label: 'Your DPS', value: formatDps(self?.dps), tone: 'fame', sub: self ? `rank ${damageRank(self.rank)} of ${fight.players.length} · ${formatShare(self.share)} share` : '—' },
    { key: 'best_hit', label: 'Your best hit', value: formatStat(nonnegative(self?.biggest_hit ?? null)), sub: self ? `${formatStat(nonnegative(self.hits))} hits · ${formatStat(nonnegative(self.deaths))} deaths` : '—' },
  ];
}
export function fightNeighbors(strip: RunFightStripRow[], fightId: string) {
  const index = strip.findIndex(row => row.fight_id === fightId);
  return { index, count: strip.length, previous: index > 0 ? strip[index - 1] : null,
    next: index >= 0 ? strip[index + 1] ?? null : null };
}
export function bucketRate(amount: number, from: string, until: string): number {
  if (!Number.isFinite(amount) || amount < 0 || !/^\d+$/.test(from) || !/^\d+$/.test(until)) return NaN;
  const seconds = Number(BigInt(until) - BigInt(from)) / 1_000_000;
  return seconds > 0 ? amount / seconds : NaN;
}
/** NaN marks unavailable bucket values so the renderer leaves a gap, never a zero. */
export function seriesForChart(fight: FightView, plotted: string[]): Array<{ id: string; label: string; color: string; local: boolean; points: number[] }> {
  const rows = [{ id: 'party', label: 'Party', color: 'var(--violet)', local: false, points: fight.buckets.map(bucket => bucket.total) }];
  const colors = ['#8de4e5', '#75dfa1', '#f2a6cf', '#ffc076', '#83bbff', '#c99a70'];
  const locals = fight.players.filter(player => player.is_local);
  const rank = (player: PlayerDamage) => player.rank !== null && player.rank > 0 ? player.rank : Infinity;
  const players = [...fight.players].sort((a, b) => rank(a) - rank(b));
  let colorIndex = 0;
  for (const player of players) {
    if (fight.players.filter(row => row.player_id === player.player_id).length !== 1) continue;
    const matches = fight.player_series?.filter(row => row.player_id === player.player_id && row.object_id === player.object_id) ?? [];
    const points = matches.length === 1 && matches[0].points.length === fight.buckets.length ? matches[0].points
      : player.is_local && locals.length === 1 ? fight.buckets.map(bucket => bucket.local ?? NaN) : null;
    if (points) rows.push({ id: player.player_id, label: player.name ?? (player.is_local ? 'You' : 'Unknown player'),
      color: player.is_local ? 'var(--gold)' : colors[colorIndex++ % colors.length], local: player.is_local, points });
  }
  const order = (row: (typeof rows)[number]) => row.local ? 0 : row.id === 'party' ? 1 : 2;
  return rows.filter(row => plotted.includes(row.id)).sort((a, b) => order(a) - order(b));
}

/** Saved toggle state takes precedence at the route; defaults only include available series. */
export function defaultFightPlotted(fight: FightView): string[] {
  const available = seriesForChart(fight, ['party', ...fight.players.map(row => row.player_id)]);
  const locals = available.filter(row => row.local);
  const otherIds = new Set(available.filter(row => !row.local && row.id !== 'party').map(row => row.id));
  const others = [...fight.players].filter(row => otherIds.has(row.player_id))
    .sort((a, b) => (nonnegative(b.damage) ?? -1) - (nonnegative(a.damage) ?? -1)).slice(0, 2);
  return [...(locals.length === 1 ? [locals[0].id] : []), 'party', ...others.map(row => row.player_id)];
}
