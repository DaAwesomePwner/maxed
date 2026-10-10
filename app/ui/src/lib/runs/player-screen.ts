import type { FightView } from '../bindings/FightView';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import type { KpiCell } from '../home/v3/types';
import type { HomeVisuals } from '../home/visuals';
import { objectVisual } from '../bridge/visual-index';
import { formatDps, formatShare, formatStat } from '../styles/format';
import { damageRank, nonnegative } from './damage';

export function playerKpis(player: PlayerDamage, fight: FightView): KpiCell[] {
  const damage = nonnegative(player.damage), hits = nonnegative(player.hits);
  const rates = fight.players.map(row => nonnegative(row.dps)).filter((rate): rate is number => rate !== null);
  const next = player.rank !== null && player.rank > 0 ? fight.players.filter(row => row.rank === player.rank! + 1) : [];
  const lead = next.length === 1 && nonnegative(player.damage) !== null && nonnegative(next[0].damage) !== null
    ? nonnegative(player.damage - next[0].damage) : null;
  const best = player.sources.filter(row => nonnegative(row.biggest_hit) !== null)
    .sort((a, b) => b.biggest_hit! - a.biggest_hit!)[0];
  return [
    { key: 'damage', label: 'Damage', value: formatStat(nonnegative(player.damage)), sub: `${formatShare(player.share)} of the fight`, tone: 'fame' },
    { key: 'dps', label: 'DPS', value: formatDps(player.dps), sub: `party avg ${formatDps(rates.length ? rates.reduce((a, b) => a + b, 0) / rates.length : null)}` },
    { key: 'rank', label: 'Rank', value: `${damageRank(player.rank)} / ${fight.players.length}`, sub: lead === null ? '—' : `+${formatStat(lead)} over #${next[0].rank}` },
    { key: 'hits', label: 'Hits', value: formatStat(nonnegative(player.hits)), sub: `${formatStat(hits !== null && hits > 0 && damage !== null ? damage / hits : null)} avg per hit` },
    { key: 'best_hit', label: 'Best hit', value: formatStat(nonnegative(player.biggest_hit)), sub: best ? sourceName(best.kind, best.item_type) : '—' },
    { key: 'deaths', label: 'Deaths', value: formatStat(nonnegative(player.deaths)) },
  ];
}
function sourceName(kind: string, itemType: number | null): string {
  return kind === 'unknown' ? 'Unknown Damage' : itemType === null ? '—' : `Item ${itemType}`;
}
export function sourceRows(player: PlayerDamage, visuals: HomeVisuals) {
  const total = nonnegative(player.damage);
  return player.sources.map((source, index) => {
    const visual = objectVisual(visuals.objects, source.item_type);
    const damage = nonnegative(source.damage), hits = nonnegative(source.hits);
    const share = damage !== null && total !== null && total > 0 ? damage / total : null;
    return { key: String(index), itemType: source.item_type,
      name: source.kind === 'unknown' ? 'Unknown Damage' : visual?.name ?? sourceName(source.kind, source.item_type),
      tier: visual?.item_tier ?? null, kind: source.kind, damage,
      share: share !== null && share <= 1 ? share : null, hits,
      avgHit: damage !== null && hits !== null && hits > 0 ? damage / hits : null,
      bestHit: nonnegative(source.biggest_hit), estimated: source.estimated, enchant: source.enchant };
  });
}
export function playerRankKey(player: PlayerDamage): number {
  return player.rank !== null && player.rank > 0 ? player.rank : Infinity;
}
export function playerNeighbors(fight: FightView, objectId: number): { previous: PlayerDamage | null; next: PlayerDamage | null } {
  if (fight.players.filter(row => row.object_id === objectId).length !== 1) return { previous: null, next: null };
  const ordered = [...fight.players].sort((a, b) => playerRankKey(a) - playerRankKey(b));
  const index = ordered.findIndex(row => row.object_id === objectId);
  return { previous: ordered[index - 1] ?? null, next: ordered[index + 1] ?? null };
}
