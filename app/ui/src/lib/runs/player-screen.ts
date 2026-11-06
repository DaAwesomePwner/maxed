import type { FightView } from '../bindings/FightView';
import type { PlayerDamage } from '../bindings/PlayerDamage';
import type { HomeVisuals } from '../home/visuals';
import { objectVisual } from '../bridge/visual-index';
import { nonnegative } from './damage';

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
  const ordered = [...fight.players].sort((a, b) => playerRankKey(a) - playerRankKey(b)
    || (a.name ?? '').localeCompare(b.name ?? ''));
  const index = ordered.findIndex(row => row.object_id === objectId);
  return { previous: ordered[index - 1] ?? null, next: ordered[index + 1] ?? null };
}
