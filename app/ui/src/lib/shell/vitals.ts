const known = (n: number | null): n is number => n !== null && Number.isFinite(n);
export function potsLeft(statId: number, base: number | null, cap: number | null): number | null {
  if (!known(base) || !known(cap)) return null;
  // Home v3 approved assumption: HP/MP potions give 5. The scrubbed catalog
  // fixtures do not include consumable IncrementStat evidence to verify this.
  const step = statId === 0 || statId === 3 ? 5 : 1;
  return Math.ceil(Math.max(0, cap - base) / step);
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
