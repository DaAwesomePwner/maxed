import type { HomeBossBaseline } from '../../bindings/HomeBossBaseline';
import type { DungeonProfile } from '../../bindings/DungeonProfile';
import type { FightView } from '../../bindings/FightView';
import type { SpriteRef } from '../../bindings/SpriteRef';
import { formatElapsed } from '../../styles/format';
import type { HomeVisuals } from '../visuals';
import { bossOnly } from '../v3/screen-data';

export interface BossRow {
  targetType: number; name: string; sprite: SpriteRef | null;
  state: 'live' | 'killed' | 'ended' | 'upcoming'; killMicros: number | null; elapsedMicros: number | null;
  usualKillMicros: number | null; dps: number | null; rank: number | null; bestDps: number | null;
}

function timestamp(value: string): bigint | null {
  return /^\d+$/.test(value) ? BigInt(value) : null;
}

function duration(started: string, ended: string): number | null {
  const start = timestamp(started), end = timestamp(ended);
  if (start === null || end === null || end < start) return null;
  const elapsed = Number(end - start);
  return Number.isSafeInteger(elapsed) ? elapsed : null;
}

function known(value: number | null | undefined): number | null {
  return value != null && Number.isFinite(value) && value >= 0 ? value : null;
}

/** History/profile reads are scoped to the preceding 90 days by the caller. */
export function bossRows(fights: FightView[], profile: DungeonProfile | null,
  usual: ReadonlyMap<number, HomeBossBaseline>, nowMicros: number, visuals: HomeVisuals): BossRow[] {
  const objects = new Map(visuals.objects.map(object => [object.object_type, object]));
  const learned = new Map(profile?.bosses.map(boss => [boss.target_type, boss]) ?? []);
  function base(targetType: number, fight?: FightView): BossRow {
    const boss = learned.get(targetType), history = usual.get(targetType);
    const visual = objects.get(targetType), fallback = fight?.boss_type == null ? undefined : objects.get(fight.boss_type);
    const profileMedian = boss && boss.kill_samples >= 3 ? known(boss.median_kill_micros) : null;
    const historyMedian = history && history.kill_samples >= 3 ? known(history.median_kill_micros) : null;
    return { targetType, name: visual?.name ?? fallback?.name ?? boss?.target_name ?? 'Unknown boss',
      sprite: visual?.sprite ?? fallback?.sprite ?? null, state: 'upcoming', killMicros: null, elapsedMicros: null,
      usualKillMicros: profileMedian ?? historyMedian, dps: null, rank: null,
      bestDps: known(boss?.best_dps) ?? known(history?.best_dps) };
  }
  const fought = fights.filter(bossOnly).sort((a, b) => {
    const start = timestamp(a.started), next = timestamp(b.started);
    if (start === null || next === null) return start === next ? 0 : start === null ? 1 : -1;
    return start < next ? -1 : start > next ? 1 : 0;
  });
  const rows = fought.map(fight => {
    const row = base(fight.target_type!, fight), local = fight.players.find(player => player.is_local);
    const confirmed = fight.kill != null && !fight.kill.provisional;
    row.state = confirmed ? 'killed' : fight.ended !== null ? 'ended' : 'live';
    row.killMicros = confirmed ? duration(fight.started, fight.kill!.at) : null;
    row.elapsedMicros = row.state === 'live' && Number.isSafeInteger(nowMicros) && nowMicros >= 0
      ? duration(fight.started, String(nowMicros)) : null;
    row.dps = known(local?.dps);
    row.rank = local?.rank != null && Number.isInteger(local.rank) && local.rank > 0 ? local.rank : null;
    return row;
  });
  const seen = new Set(rows.map(row => row.targetType));
  for (const boss of profile?.bosses ?? []) if (!seen.has(boss.target_type)) rows.push(base(boss.target_type));
  return rows;
}

export function killDelta(row: BossRow): string {
  const usual = known(row.usualKillMicros), kill = known(row.killMicros);
  if (usual === null) return '';
  if (row.state === 'killed' && kill !== null && usual - kill >= 1_000_000) {
    return `▲ ${Math.floor((usual - kill) / 1_000_000)}s faster than usual`;
  }
  return `usually ${formatElapsed('0', String(usual))}`;
}
