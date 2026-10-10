import type { HomeScope } from '../bindings/HomeScope';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { TimeRange } from '../bindings/TimeRange';

export type ScopeChoice = 'session' | 'today';

export function localDayRange(now: Date): TimeRange {
  if (!Number.isFinite(now.getTime()) || now.getTime() < 0) throw new RangeError('A valid nonnegative date is required.');
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  if (!Number.isFinite(start.getTime()) || !Number.isFinite(end.getTime()) || start.getTime() < 0) {
    throw new RangeError('The local day is outside the supported timestamp range.');
  }
  return { start: (BigInt(start.getTime()) * 1000n).toString(), end: (BigInt(end.getTime()) * 1000n).toString() };
}

/** For Demo/Replay the caller supplies the snapshot clock as now. */
export function homeScope(choice: ScopeChoice, snapshot: LiveSnapshot | null, now: Date): HomeScope | null {
  if (choice === 'session') return snapshot?.session?.id ? { session: { id: snapshot.session.id } } : null;
  try { return { today: { range: localDayRange(now) } }; }
  catch { return null; }
}
