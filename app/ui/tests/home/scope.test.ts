import { afterEach, describe, expect, it, vi } from 'vitest';
import { homeScope, localDayRange } from '../../src/lib/home/scope';
import { fameSegments } from '../../src/lib/home/summary';
import { homeSnapshot } from './fixtures';

// Inject independently tabulated Chicago midnights rather than depending on the
// host Windows timezone or changing process.env.TZ inside a Vitest worker.
const NativeDate = Date;
const midnights: Record<string, string> = {
  '2026-2-8': '2026-03-08T06:00:00Z', '2026-2-9': '2026-03-09T05:00:00Z',
  '2026-10-1': '2026-11-01T05:00:00Z', '2026-10-2': '2026-11-02T06:00:00Z',
};
function chicagoDay(iso: string, year: number, month: number, day: number) {
  const now = new NativeDate(iso);
  now.getFullYear = () => year; now.getMonth = () => month; now.getDate = () => day;
  class ChicagoDate extends NativeDate {
    constructor(y: number, m: number, d: number) {
      const midnight = midnights[`${y}-${m}-${d}`];
      if (!midnight) throw new Error('Unexpected local midnight in deterministic Chicago fixture');
      super(midnight);
    }
  }
  vi.stubGlobal('Date', ChicagoDate);
  return now;
}
afterEach(() => vi.unstubAllGlobals());

describe('local calendar and fame identity boundaries', () => {
  it('midnight_dst_and_character_segments', () => {
    let now = chicagoDay('2026-03-08T18:00:00Z', 2026, 2, 8);
    const spring = localDayRange(now);
    expect(spring).toEqual({ start: '1772949600000000', end: '1773032400000000' });
    expect(BigInt(spring.end) - BigInt(spring.start)).toBe(23n * 3600n * 1000000n);
    now = chicagoDay('2026-11-01T18:00:00Z', 2026, 10, 1);
    const fall = localDayRange(now);
    expect(fall).toEqual({ start: '1793509200000000', end: '1793599200000000' });
    expect(BigInt(fall.end) - BigInt(fall.start)).toBe(25n * 3600n * 1000000n);
    const points = [
      { at: '9007199254740993', character_id: 'a', fame: 10 },
      { at: '9007199254740994', character_id: 'a', fame: 11 },
      { at: '9007199254740995', character_id: 'b', fame: 900 },
      { at: '9007199254740996', character_id: 'a', fame: 12 },
    ];
    expect(fameSegments(points)).toEqual([points.slice(0, 2), [points[2]], [points[3]]]);
    expect(fameSegments([])).toEqual([]);
  });

  it('session identity survives midnight and no captured session never falls back', () => {
    const snapshot = homeSnapshot();
    for (const now of [new NativeDate('2026-03-08T05:59:59Z'), new NativeDate('2026-03-08T06:00:00Z')]) {
      expect(homeScope('session', snapshot, now)).toEqual({ session: { id: snapshot.session!.id } });
      expect(homeScope('session', homeSnapshot({ session: null }), now)).toBeNull();
    }
  });

  it.each(['demo-fixture', 'replay-fixture', 'opaque-synthetic-stream'])('%s Today accepts the caller-selected snapshot clock', stream_id => {
    const at = new NativeDate('2026-03-08T18:00:00Z');
    const snapshot = homeSnapshot({ stream_id, at: (BigInt(at.getTime()) * 1000n).toString() });
    const expected = { today: { range: localDayRange(at) } };
    // The UI knows the namespace and passes its clock; this pure helper must
    // not infer personal/demo provenance from opaque stream-ID spelling.
    expect(homeScope('today', snapshot, at)).toEqual(expected);
    const ownerClock = new NativeDate('2030-01-01T12:00:00Z');
    expect(homeScope('today', snapshot, ownerClock)).toEqual({ today: { range: localDayRange(ownerClock) } });
  });

  it('invalid or pre-epoch dates cannot escape as negative or NaN timestamps', () => {
    for (const now of [new NativeDate(NaN), new NativeDate('1900-01-01T12:00:00Z')]) {
      expect(homeScope('today', homeSnapshot(), now)).toBeNull();
    }
  });
});
