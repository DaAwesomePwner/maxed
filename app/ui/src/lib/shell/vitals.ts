import type { CharacterView } from '../bindings/CharacterView';
import type { StatLine } from '../bindings/StatLine';
const known = (n: number | null): n is number => n !== null && Number.isFinite(n);
export function potsLeft(statId: number, base: number | null, cap: number | null): number | null {
  if (!known(base) || !known(cap)) return null;
  // Home v3 approved assumption: HP/MP potions give 5. The scrubbed catalog
  // fixtures do not include consumable IncrementStat evidence to verify this.
  const step = statId === 0 || statId === 3 ? 5 : 1;
  return Math.ceil(Math.max(0, cap - base) / step);
}
// Consume the engine's shared gate; legacy DTO cap flags alone are insufficient.
export function statPotsLeft(line: StatLine | null | undefined): number | null {
  if (!line?.split_evidence?.cap_eligible || typeof line.at_cap !== 'boolean') return null;
  const { base, cap, exalt } = line;
  if (!Number.isSafeInteger(base) || !Number.isSafeInteger(cap) || !Number.isSafeInteger(exalt)
    || base! < 0 || cap! < 0 || exalt! < 0) return null;
  const placement = line.split_evidence.placement;
  if (exalt !== 0 && placement !== 'verified_inside_base' && placement !== 'verified_inside_boost') return null;
  return potsLeft(line.stat_id, base!, cap!);
}

// A missing or partial legacy panel cannot substantiate an aggregate cap claim.
export function characterMaxed(character: CharacterView | null | undefined): number | null {
  const lines = character?.stat_panel?.lines;
  if (!lines || lines.length !== 8
    || ![0, 3, 20, 21, 22, 26, 27, 28].every(id =>
      lines.some(line => line.stat_id === id && statPotsLeft(line) !== null))) return null;
  return character?.maxed ?? null;
}

export function statCapUnavailableReason(line: StatLine): string | null {
  if (statPotsLeft(line) !== null) return null;
  const evidence = line.split_evidence;
  if (!evidence) return 'split evidence missing';
  if (evidence.reasons.includes('negative_other')) return 'inconsistent stat split';
  if (evidence.reasons.includes('invalid_exalt')) return 'invalid exalt level';
  if (evidence.reasons.includes('missing_exalt')) return 'exalt level unknown';
  if (evidence.reasons.includes('overflow') || evidence.reasons.includes('negative_base')) return 'invalid stat arithmetic';
  if (evidence.reasons.includes('unverified_placement')) return 'exalt placement unverified';
  return 'base or cap unavailable';
}
export function statTone(atCap: boolean, boost: number | null): 'max' | 'boosted' | 'plain' {
  return atCap ? 'max' : known(boost) && boost !== 0 ? 'boosted' : 'plain';
}
export function vitalText(cur: number | null, max: number | null, pots: number | null, boost: number | null): string {
  const amount = (n: number | null) => known(n) ? String(n) : '—';
  const value = amount(cur) + (max === null ? '' : `/${amount(max)}`);
  const remaining = known(pots) && pots > 0 ? `|${pots}` : '';
  const mod = !known(boost) ? ' (—)' : boost === 0 ? '' : boost > 0 ? ` (+${boost})` : ` (−${Math.abs(boost)})`;
  return value + remaining + mod;
}
