import type { DpsBasis } from '../bindings/DpsBasis';

export type DpsScope = DpsBasis | 'whole_run' | null | undefined;

/** Describe the persisted denominator without changing the saved value. */
export function dpsMetric(basis: DpsScope): { label: string; tooltip: string } {
  if (basis === 'player_active_window_v1') return {
    label: 'Active DPS',
    tooltip: "Damage ÷ that player's time from first to last hit. Unavailable below 3 seconds.",
  };
  return {
    label: 'DPS',
    tooltip: basis === 'whole_run'
      ? "Damage ÷ the whole run's time from first to last hit, shared by all players."
      : "Damage ÷ the whole fight's time from first to last hit, shared by all players.",
  };
}
