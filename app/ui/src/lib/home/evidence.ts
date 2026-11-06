import type { SourceDamage } from '../bindings/SourceDamage';
import type { CoverageIssue } from '../bindings/CoverageIssue';
import type { Evidence } from '../bindings/Evidence';
import { formatStat } from '../styles/format';

// Diagnostic copy: render coverage and attribution reasons only in debug mode.
export const coverageLabels: Record<CoverageIssue, string> = {
  midstream: 'Capture began partway through play', decode_failure: 'Some traffic could not be decoded',
  stream_gap: 'Some traffic was missed', missing_identity: 'Player identity is incomplete',
  missing_catalog: 'Some game metadata is unavailable', missing_stats: 'Some stats are unavailable',
  missing_projectile: 'Some projectile evidence is missing', ambiguous_hit: 'Some hit attribution is uncertain',
  resource_limit: 'Some observations were evicted at the capture limit', clock_regression: 'The capture clock moved backwards',
};
const inferredEquippedWeapon = 'inferred-equipped-weapon';
export function damageUncertain(source: SourceDamage): boolean {
  return source.reasons.some(reason => reason !== inferredEquippedWeapon)
    || (source.estimated && source.reasons.length === 0);
}
const reasonLabels = new Map([
  ['client-projectile-reconstruction', 'Damage attributed from client-side projectile reconstruction'],
  [inferredEquippedWeapon, 'Inferred from the equipped weapon'],
  ['resource-limit-source-summary', 'Some damage sources were combined at the capture limit'],
  ['source-not-observed', 'The source was not observed'],
  ['killed-target-same-tick', 'Bag attributed to the target killed in the same or next update (estimated)'],
  ['nearest-removed-hit-target', 'Source inferred from the nearest recently killed enemy that was hit'],
  ['ambiguous-nearby-removed-targets', 'Several nearby enemies could have dropped this bag'],
  ['ambiguous-nearby-killed-targets', 'Several nearby killed targets could have dropped this bag'],
  ...['dialogue-kitsune', 'dialogue-miko', 'dialogue-void', 'dialogue-sentinel', 'dialogue-archmage', 'dialogue-king']
    .map((code): [string, string] => [code, "Source inferred from the boss's defeat dialogue"]),
  ...['dialogue-revelry-tr-variant-unverified', 'dialogue-valen-hm-variant-unverified', 'dialogue-nox-hm-variant-unverified', 'dialogue-azamoth-hm-variant-unverified']
    .map((code): [string, string] => [code, "Source inferred from the boss's defeat dialogue; the encounter variant is unverified"]),
]);
export function reasonLabel(code: string | null | undefined): string {
  return reasonLabels.get(code?.trim() ?? '') ?? 'Attribution is uncertain based on the available captured evidence.';
}
export function reasonNotes(codes: string[]): string[] {
  return [...new Set(codes.map(reasonLabel))];
}
export function knownNumber(value: number | null | undefined): number | null {
  return value != null && Number.isFinite(value) && value >= 0 ? value : null;
}
export function statText(value: number | null | undefined): string | null {
  const known = knownNumber(value);
  return known === null ? null : formatStat(known);
}
export function evidenceLabel(evidence: Evidence): string {
  if (typeof evidence !== 'string') {
    return 'completion_counter' in evidence ? 'Completion counter observed' : 'Completion dialogue observed';
  }
  return ({ victory: 'Victory observed', death_packet: 'Your death observed', observed_exit: 'Exit observed',
    capture_gap: 'Capture gap', app_closed: 'Application closed' })[evidence];
}
