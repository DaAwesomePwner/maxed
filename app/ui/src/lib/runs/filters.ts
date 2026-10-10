import type { RunFilter } from '../bindings/RunFilter';
import type { RunCursor } from '../bindings/RunCursor';
import type { TimeRange } from '../bindings/TimeRange';
import { localDayRange } from '../home/scope';

function localDate(value: string): Date {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) throw new RangeError('Invalid Dates');
  const [year, month, day] = value.split('-').map(Number);
  const date = new Date(year, month - 1, day);
  if (!Number.isFinite(date.getTime()) || date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) {
    throw new RangeError('Invalid Dates');
  }
  return date;
}
export function dateRange(firstDay: string, lastDay: string): TimeRange | null {
  if (!firstDay && !lastDay) return null;
  const first = localDate(firstDay), last = localDate(lastDay);
  if (first > last) throw new RangeError('Invalid Date Range');
  return { start: localDayRange(first).start, end: localDayRange(last).end };
}
export function withRunFilter(current: RunFilter, patch: Partial<Pick<RunFilter, 'dungeon' | 'outcome' | 'range' | 'sort' | 'character_id'>>): RunFilter {
  return { ...current, ...patch, cursor: null };
}
const id = (value: unknown): value is string => typeof value === 'string' && value.length > 0
  && new TextEncoder().encode(value).length <= 256 && !/[\u0000-\u001f\u007f-\u009f]/.test(value);
const timestamp = (value: unknown): value is string => typeof value === 'string' && /^(0|[1-9]\d*)$/.test(value)
  && BigInt(value) <= 9223372036854775807n;
export function validRunCursor(value: unknown): value is RunCursor | null {
  if (value === null) return true;
  if (!value || typeof value !== 'object') return false;
  const cursor = value as RunCursor;
  return id(cursor.id) && (cursor.sort_value === '-1' || timestamp(cursor.sort_value));
}
export function validRunFilter(value: unknown): value is RunFilter {
  if (!value || typeof value !== 'object') return false;
  const filter = value as RunFilter;
  return (filter.dungeon === null || id(filter.dungeon))
    && (filter.character_id === undefined || filter.character_id === null || id(filter.character_id))
    && [null, 'cleared', 'died', 'left', 'unknown'].includes(filter.outcome)
    && ['newest', 'oldest', 'duration_ascending', 'duration_descending'].includes(filter.sort)
    && (filter.range === null || (typeof filter.range === 'object' && timestamp(filter.range.start)
      && timestamp(filter.range.end) && BigInt(filter.range.start) < BigInt(filter.range.end)))
    && validRunCursor(filter.cursor) && (filter.cursor?.sort_value !== '-1' || filter.sort === 'duration_descending')
    && Number.isInteger(filter.limit) && filter.limit >= 1 && filter.limit <= 100;
}
export function filterDays(range: TimeRange | null): [string, string] {
  if (!range) return ['', ''];
  const format = (micros: bigint) => {
    const date = new Date(Number(micros / 1000n));
    return `${date.getFullYear().toString().padStart(4, '0')}-${(date.getMonth() + 1).toString().padStart(2, '0')}-${date.getDate().toString().padStart(2, '0')}`;
  };
  return [format(BigInt(range.start)), format(BigInt(range.end) - 1000n)];
}
