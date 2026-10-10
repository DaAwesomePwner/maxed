import type { AlertFilter } from '../bindings/AlertFilter';
import { ALERT_KINDS } from '../bindings/alert-kinds';
import type { AlertKind } from '../bindings/AlertKind';
import { dateRange, filterDays } from '../runs/filters';

export { dateRange, filterDays };
export const ALERT_PAGE_SIZE = 50;
export const alertKinds: AlertKind[] = [...ALERT_KINDS];
// Keep the established UI order independent of the generated validation set.
export const alertKindDisplayOrder: readonly AlertKind[] = [
  'white_bag', 'personal_best', 'key_popped', 'whisper', 'guild_message', 'party_message', 'death', 'flawless_run',
];
export const alertKindLabels: Record<AlertKind, string> = {
  whisper: 'Whisper', guild_message: 'Guild Message', party_message: 'Party Message',
  white_bag: 'White Bag', personal_best: 'Personal Best', key_popped: 'Key Popped', death: 'Death', flawless_run: 'Flawless Run',
};
export const defaultAlertFilter = (): AlertFilter => ({ kind: null, range: null, query: '', cursor: null, limit: ALERT_PAGE_SIZE });

/** Literal text only: SQL escaping belongs to the existing native query. */
export function withAlertFilter(current: AlertFilter,
  patch: Partial<Pick<AlertFilter, 'kind' | 'range' | 'query'>>): AlertFilter {
  const next = { ...current, ...patch, cursor: null, limit: ALERT_PAGE_SIZE };
  next.query = next.query.trim();
  if ([...next.query].length > 128 || new TextEncoder().encode(next.query).length > 512 || next.query.includes('\u0000')) {
    throw new RangeError('Invalid search.');
  }
  if (next.kind !== null && !alertKinds.includes(next.kind)) throw new RangeError('Choose an alert kind.');
  return next;
}

export function alertTime(at: string): string {
  try {
    const date = new Date(Number(BigInt(at) / 1000n));
    return Number.isFinite(date.getTime()) ? date.toLocaleString() : '—';
  } catch { return '—'; }
}
