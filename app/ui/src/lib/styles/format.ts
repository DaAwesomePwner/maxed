const numberFormat = new Intl.NumberFormat('en-US', { maximumFractionDigits: 1 });
const dpsFormat = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 });
const percentFormat = new Intl.NumberFormat('en-US', { style: 'percent', maximumFractionDigits: 1 });
export function formatStat(value: number | null): string {
  return value !== null && Number.isFinite(value) ? numberFormat.format(value) : '—';
}
/** Preserve unavailable short-window DPS; never coerce it to zero. */
export function formatDps(value: number | null | undefined): string {
  return value != null && Number.isFinite(value) && value >= 0 ? dpsFormat.format(value === 0 ? 0 : value) : '—';
}
/** Display form of a backend enum value, e.g. `up_to_date` -> `Up To Date`. */
export function titleCase(value: string): string {
  return value.replaceAll('_', ' ').replace(/\b\w/g,letter => letter.toUpperCase());
}
export function formatShare(value: number | null): string {
  return value !== null && Number.isFinite(value) && value >= 0 && value <= 1
    ? percentFormat.format(value) : '—';
}
/** Engine timestamps are decimal microseconds, potentially larger than JS safe integers. */
export function formatElapsed(start: string, end: string, includeHours = false): string {
  if (!/^\d+$/.test(start) || !/^\d+$/.test(end)) return '—';
  const delta = BigInt(end) - BigInt(start);
  if (delta < 0n) return '—';
  const seconds = delta / 1_000_000n;
  if (includeHours && seconds >= 3600n) return `${seconds / 3600n}:${((seconds / 60n) % 60n).toString().padStart(2, '0')}:${(seconds % 60n).toString().padStart(2, '0')}`;
  return `${seconds / 60n}:${(seconds % 60n).toString().padStart(2, '0')}`;
}

/** Signed clear-time change, preserving real improvements below one second. */
export function formatTimeDelta(delta: bigint): string {
  const magnitude = delta < 0n ? -delta : delta;
  if (magnitude > 0n && magnitude < 1_000_000n) return `under 1 s ${delta < 0n ? 'faster' : 'slower'}`;
  return `${delta > 0n ? '+' : delta < 0n ? '−' : ''}${formatElapsed('0', String(magnitude))}`;
}

const clockOptions: Intl.DateTimeFormatOptions = { hour: 'numeric', minute: '2-digit', second: '2-digit' };
function clockDate(at: string | null | undefined): Date | null {
  if (at == null || !/^\d+$/.test(at)) return null;
  const date = new Date(Number(BigInt(at) / 1000n));
  return Number.isFinite(date.getTime()) ? date : null;
}
export function formatClock(at: string | null | undefined, locales?: Intl.LocalesArgument, options: Intl.DateTimeFormatOptions = {}): string | null {
  return clockDate(at)?.toLocaleTimeString(locales, { ...clockOptions, ...options }) ?? null;
}
/** Share a matching day period in its locale's prefix/suffix position. Unknown endpoints stay explicit. */
export function formatClockRange(start: string | null | undefined, end: string | null | undefined,
  locales?: Intl.LocalesArgument, options: Intl.DateTimeFormatOptions = {}): string {
  const formatter = new Intl.DateTimeFormat(locales, { ...clockOptions, ...options });
  const parts = (at: string | null | undefined) => {
    const date = clockDate(at);
    return date ? formatter.formatToParts(date) : [{ type: 'literal' as const, value: '—' }];
  };
  let first = parts(start), last = parts(end);
  const period = first.find(part => part.type === 'dayPeriod');
  if (period && period.value === last.find(part => part.type === 'dayPeriod')?.value) {
    const meaningful = first.filter(part => part.type !== 'literal');
    if (meaningful.at(-1)?.type === 'dayPeriod') first = first.filter(part => part.type !== 'dayPeriod');
    else if (meaningful[0]?.type === 'dayPeriod') last = last.filter(part => part.type !== 'dayPeriod');
  }
  const text = (values: Intl.DateTimeFormatPart[]) => values.map(part => part.value).join('').trim();
  return `${text(first)} – ${text(last)}`;
}
