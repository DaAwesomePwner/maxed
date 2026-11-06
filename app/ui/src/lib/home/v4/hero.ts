import type { HomeBossBaseline } from '../../bindings/HomeBossBaseline';
import type { PlayerDamage } from '../../bindings/PlayerDamage';

export type Usual = { kind: 'value'; dps: number; fights: number }
  | { kind: 'learning'; fights: number } | { kind: 'none' };

/** The caller supplies history scoped to this character and the preceding 90 days. */
export function usualFor(row: HomeBossBaseline | undefined): Usual {
  if (!row) return { kind: 'none' };
  return row.fights >= 3 && row.avg_dps !== null && Number.isFinite(row.avg_dps) && row.avg_dps >= 0
    ? { kind: 'value', dps: row.avg_dps, fights: row.fights }
    : { kind: 'learning', fights: row.fights };
}

export function partyAverage(players: PlayerDamage[]): { avg: number; players: number } | null {
  const others = players.filter(player => !player.is_local && player.dps !== null
    && Number.isFinite(player.dps) && player.dps >= 0);
  if (!others.length) return null;
  return { avg: others.reduce((sum, player) => sum + player.dps! / others.length, 0), players: others.length };
}

export function relative(value: number | null, base: number | null): number | null {
  if (value === null || base === null || !Number.isFinite(value) || !Number.isFinite(base) || value < 0 || base <= 0) return null;
  const result = value / base - 1;
  return Number.isFinite(result) ? result : null;
}
