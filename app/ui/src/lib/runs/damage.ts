import type { CoverageIssue } from '../bindings/CoverageIssue';
import type { FightEnd } from '../bindings/FightEnd';
import type { SourceDamage } from '../bindings/SourceDamage';
import type { SourceKind } from '../bindings/SourceKind';
import { formatElapsed, formatStat } from '../styles/format';

export function nonnegative(value: number | null): number | null {
  return value !== null && Number.isFinite(value) && value >= 0 ? value : null;
}
export function damageStat(value: number | null): string {
  const known = nonnegative(value);
  return known === null ? '—' : formatStat(known);
}
const shareFormat = new Intl.NumberFormat('en-US', { style: 'percent', maximumFractionDigits: 2 });
export function damageShare(value: number | null): string {
  return nonnegative(value) !== null && value! <= 1 ? shareFormat.format(value!) : '—';
}
export function damageRank(value: number | null): string {
  return value !== null && Number.isInteger(value) && value > 0 ? formatStat(value) : '—';
}
export function hitInterval(first: string | null, last: string | null): string {
  if (first === null || last === null) return '—';
  const interval = formatElapsed(first, last);
  return interval === '\u2014' ? '—' : interval;
}
export const sourceKinds: SourceKind[] = ['weapon', 'ability', 'summon', 'item_effect', 'other', 'unknown'];
export const sourceLabels: Record<SourceKind, string> = {
  weapon: 'Weapon', ability: 'Ability', summon: 'Summon', item_effect: 'Item Effect', other: 'Other', unknown: 'Unknown',
};
/** Keep each saved entry, including identical items with distinct enchant evidence. */
export function orderedSources(sources: SourceDamage[]) {
  return sourceKinds.flatMap(kind => sources.map((source, index) => ({ source, index }))
    .filter(row => row.source.kind === kind));
}

/** Render only as text: saved reasons may contain arbitrary markup characters. */
export function sourceExplanation(source: SourceDamage): string {
  if (source.estimated && source.reasons.length === 0) return 'Estimated damage; reconstruction reasons unavailable.';
  return `${source.estimated ? 'Estimated damage. ' : ''}${source.reasons.join('; ')}`.trim();
}
export const coverageExplanation: Record<CoverageIssue, string> = {
  missing_stats: 'Missing combat stats: reconstruction may use documented assumptions or omit unsupported hits.',
  missing_identity: 'Player identity is incomplete or unverified; unsupported attribution is unavailable.',
  missing_catalog: 'Catalog metadata is missing; item attribution or damage reconstruction may be unavailable.',
  missing_projectile: 'Projectile evidence is missing; some hits cannot be reconstructed.',
  ambiguous_hit: 'Hit evidence is ambiguous; attribution cannot be resolved reliably.',
  resource_limit: 'A resource limit interrupted capture or reconstruction; observations are incomplete.',
  midstream: 'Capture started midstream, after traffic was already in progress.',
  decode_failure: 'Some traffic could not be decoded; observations are incomplete.',
  stream_gap: 'A gap in captured traffic may leave missing observations.',
  clock_regression: 'The capture clock moved backwards; timing coverage is uncertain.',
};
export const fightEndLabels: Record<FightEnd, string> = {
  map_changed: 'Map Changed', capture_stopped: 'Capture Stopped', session_idle: 'Session Idle',
  app_closed: 'App Closed', resource_limit: 'Resource Limit', killed: 'Target Killed',
};
export const reconstructionDefaults = [
  'Crucible/Blood Ritual: neutral 1.0 when their evidence is missing or unknown.',
  'Missing target stats: default zero for defense and the two condition stats; a missing target entity can omit reconstruction.',
  'Matching-ability enchant: neutral 1.0 when evidence is missing or malformed.',
  'Missing scaling stat: zero bonus.',
  'Unknown positive enchant IDs or missing combat metadata can omit reconstruction; omitted damage is unavailable, not zero contribution.',
  'Startup trust (D-trust): unverified startup packets do not become gameplay facts.',
];
