import type { FightView } from '../../bindings/FightView';
import type { DropView } from '../../bindings/DropView';
import type { PlayerDamage } from '../../bindings/PlayerDamage';
import type { RunPlayerTotal } from '../../bindings/RunPlayerTotal';
import type { LiveSnapshot } from '../../bindings/LiveSnapshot';
import type { DungeonBest } from '../../bindings/DungeonBest';
import type { InstanceEventView } from '../../bindings/InstanceEventView';
import type { HomeVisuals } from '../visuals';
import type { DamageRow } from './types';
import { formatElapsed, formatTimeDelta } from '../../styles/format';
import { knownNumber } from '../evidence';
import { homeLoot } from './loot-rank';

export const bossOnly = (fight: FightView): boolean =>
  fight.target_type != null && (fight.target_kind === 'boss' || fight.target_kind === 'event_boss');

export function damageRows(players: Array<PlayerDamage | RunPlayerTotal>, self: PlayerDamage | RunPlayerTotal | null): DamageRow[] {
  return [...players].sort((a, b) => (knownNumber(b.damage) ?? -1) - (knownNumber(a.damage) ?? -1)
    || a.player_id.localeCompare(b.player_id)).map(row => ({
    rank: row.rank ?? null, name: row.name ?? null, class_id: row.class_id ?? null,
    is_local: self !== null && row.player_id === self.player_id,
    damage: row.damage, dps: row.dps ?? null, share: row.share ?? null,
  }));
}
export function runRows(players: RunPlayerTotal[], self: RunPlayerTotal | null): DamageRow[] {
  const rows = damageRows(players, self), top = rows.slice(0, 9);
  const local = rows.find(row => row.is_local);
  if (local && !top.includes(local)) top.splice(8, 1, local);
  return top;
}
export function personalBest(snapshot: LiveSnapshot | null, dungeonBests: DungeonBest[] | null) {
  const run = snapshot?.run;
  const best = dungeonBests?.find(row => row.dungeon === run?.dungeon)?.best_clear_micros;
  const end = run?.completed_at ?? run?.ended ?? snapshot?.at;
  if (!run || !end || !/^\d+$/.test(run.started) || !/^\d+$/.test(end)
    || best == null || !Number.isSafeInteger(best) || best <= 0) return null;
  const elapsed = BigInt(end) - BigInt(run.started);
  if (elapsed < 0n) return null;
  const delta = elapsed - BigInt(best);
  return { best: formatElapsed('0', String(best)),
    delta: formatTimeDelta(delta),
    behind: delta > 0n };
}
export function lootCounts(drops: DropView[], visuals: HomeVisuals) {
  return homeLoot(drops, visuals).counts;
}
export function notificationTip(events: InstanceEventView[], type: 'player_left' | 'player_death'): string | null {
  const lines = events.flatMap(event => {
    const kind = event.kind;
    if (kind.type !== type) return [];
    if (kind.type === 'player_death') return [`${kind.player_name} · Level ${kind.level ?? '—'} · Killed by ${kind.killed_by ?? '—'}`];
    return [kind.player_name ?? 'Player'];
  });
  return lines.length ? lines.join('\n') : null;
}
export function fightDps(fight: FightView): number | null {
  const from = fight.first_hit, until = fight.last_hit;
  if (!from || !until || !/^\d+$/.test(from) || !/^\d+$/.test(until)) return null;
  const seconds = Number(BigInt(until) - BigInt(from)) / 1_000_000;
  const damage = knownNumber(fight.total_damage);
  const rate = damage !== null && seconds >= 3 ? damage / seconds : NaN;
  return Number.isFinite(rate) ? rate : null;
}
