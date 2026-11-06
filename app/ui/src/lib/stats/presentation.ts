const whole = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 });
const loot = new Intl.NumberFormat('en-US', { maximumFractionDigits: 1 });
export const NO_VALUE = '—';
const date = (at: string) => new Date(Number(BigInt(at) / 1000n));

/** "≥" marks a total that includes a character with no earlier fame sample, as on Home. */
export const fameText = (fame: number, missing: boolean) => `${missing ? '≥ ' : ''}${whole.format(fame)}`;
export const rateText = (rate: number | null) => rate === null ? NO_VALUE : whole.format(rate);
/** Keep sparse loot rates visible without adding trailing zeroes. */
export const lootRateText = (rate: number | null) => rate === null ? NO_VALUE : (rate < 10 ? loot : whole).format(rate);
export function durationText(micros: number): string {
  const minutes = Math.floor(micros / 60_000_000);
  return minutes >= 60 ? `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, '0')}m` : `${minutes}m`;
}
export function clearText(micros: number | null): string {
  if (micros === null) return NO_VALUE;
  const seconds = Math.round(micros / 1_000_000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}
export const percentText = (part: number, total: number) => total > 0 ? `${Math.round(part * 100 / total)}%` : NO_VALUE;
export const whenText = (at: string) => date(at).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
export const dayText = (at: string) => date(at).toLocaleDateString(undefined, { dateStyle: 'medium' });
