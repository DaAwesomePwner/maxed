import type { HomeLiveWindow } from './contracts';

type Periods = NonNullable<HomeLiveWindow['realmPeriods']>;
const bounds = new WeakMap<Periods, { started: bigint; ended: bigint | null }[]>();
function periodBounds(periods: Periods) {
  let parsed = bounds.get(periods);
  if (!parsed) {
    // Visit updates replace the array; all row checks share its parsed bounds.
    parsed = periods.map(period => ({ started: BigInt(period.started), ended: period.ended === null ? null : BigInt(period.ended) }));
    bounds.set(periods, parsed);
  }
  return parsed;
}

/** The same observed realm segments scope Home and the rail, excluding time away. */
export function realmVisitPeriod(window: HomeLiveWindow, at: string): number {
  const time = BigInt(at);
  return window.realmPeriods !== undefined
    ? periodBounds(window.realmPeriods).findIndex(period => time >= period.started && (period.ended === null || time < period.ended))
    : window.visitStarted !== null && time >= BigInt(window.visitStarted) ? 0 : -1;
}
export function inRealmVisit(window: HomeLiveWindow, at: string): boolean {
  return realmVisitPeriod(window, at) >= 0;
}
export function realmVisitElapsed(window: HomeLiveWindow, at: string): string | null {
  if (window.visitStarted === null) return null;
  const time = BigInt(at);
  return (window.realmPeriods === undefined ? [{ started: BigInt(window.visitStarted), ended: null }] : periodBounds(window.realmPeriods)).reduce((sum, period) => {
    const end = period.ended ?? time, start = period.started;
    return sum + (end > start ? end - start : 0n);
  }, 0n).toString();
}
export function realmVisitFame(window: HomeLiveWindow): number | null {
  const periods = window.realmPeriods;
  if (!periods?.length || periods.some(period => period.fame === null)) return null;
  return periods.reduce((sum, period) => sum + period.fame!, 0);
}
