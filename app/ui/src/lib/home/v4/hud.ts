import type { PotionStat } from '../../bindings/PotionStat';

export function signedPercent(ratio: number | null): string {
  if (ratio === null || !Number.isFinite(ratio)) return '—';
  const value = Math.round(Math.abs(ratio) * 100);
  return `${value === 0 ? '' : ratio < 0 ? '−' : '+'}${value}%`;
}

export function rankText(avg: number | null): string {
  return avg === null || !Number.isFinite(avg) ? '—' : `#${avg.toFixed(1)}`;
}

export function overflow<T>(items: T[], visible: number): { shown: T[]; more: number } {
  const shown = items.slice(0, Math.max(0, visible));
  return { shown, more: items.length - shown.length };
}

export function sessionClock(startedMicros: string | null, nowMs: number, endedMicros: string | null = null): string {
  if (endedMicros !== null) nowMs = /^\d+$/.test(endedMicros) ? Number(endedMicros) / 1000 : NaN;
  if (startedMicros === null || !/^\d+$/.test(startedMicros) || !Number.isFinite(nowMs)) return '—';
  const started = Number(startedMicros) / 1000;
  if (!Number.isFinite(started) || nowMs < started) return '—';
  const minutes = Math.floor((nowMs - started) / 60_000);
  return `${Math.floor(minutes / 60)}:${String(minutes % 60).padStart(2, '0')}`;
}

export const potionNames: Record<PotionStat, string> = {
  life: 'Life', mana: 'Mana', attack: 'Attack', defense: 'Defense',
  speed: 'Speed', dexterity: 'Dexterity', vitality: 'Vitality', wisdom: 'Wisdom',
};

// The catalog's base white bag (crates/maxed-assets/src/rules.rs).
export const WHITE_BAG_TYPE = 1292;
