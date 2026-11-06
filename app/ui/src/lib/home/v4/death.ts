import type { DeathDetail } from '../../bindings/DeathDetail';
import type { DropView } from '../../bindings/DropView';
import type { HpSample } from '../../bindings/HpSample';
import type { ObjectVisual } from '../../bindings/ObjectVisual';
import { objectVisual } from '../../bridge/visual-index';
import { knownNumber } from '../evidence';
import { timestamp } from './recap-view';

export function bagsBeforeDeath(drops: DropView[], diedAt: string): DropView[] {
  const end = timestamp(diedAt);
  return end === null ? [] : drops.filter(drop => {
    const at = timestamp(drop.at);
    // Death time is floored to milliseconds; include its remaining 999 microseconds.
    return at !== null && at <= end + 999n;
  });
}

function normalizedReadings(samples: HpSample[], diedAt?: string) {
  const end = timestamp(diedAt);
  const readings = samples.map(sample => {
    const at = timestamp(sample.at), hp = knownNumber(sample.hp);
    return at === null || hp === null ? null : { at, hp, max: knownNumber(sample.max_hp),
      time: end !== null && at > end ? end : at };
  });
  const valid = readings.filter(reading => reading !== null);
  const peak = valid.reduce((max, reading) => Math.max(max, reading.hp), 1);
  return readings.map(reading => reading === null ? null : { ...reading,
    ratio: Math.min(1, reading.hp / (reading.max !== null && reading.max > 0 ? reading.max : peak)) });
}

export function hpTraceSummary(samples: HpSample[], gaps: number): string | null {
  const valid = normalizedReadings(samples).filter(reading => reading !== null);
  if (!valid.length) return null;
  const first = valid[0], last = valid.at(-1)!;
  const usesPeak = (reading: typeof first) => reading.max === null || reading.max <= 0;
  const percent = (reading: typeof first) => `${Math.round(reading.ratio * 100)}%`;
  const range = usesPeak(first) === usesPeak(last)
    ? `${percent(first)} to ${percent(last)}${usesPeak(first) ? ' of observed peak' : ''}`
    : `${percent(first)}${usesPeak(first) ? ' of observed peak' : ''} to ${percent(last)}${usesPeak(last) ? ' of observed peak' : ''}`;
  return `Observed HP over the final 5 seconds: ${range}`
    + (gaps > 0 ? `, ${gaps} ${gaps === 1 ? 'gap' : 'gaps'} in readings` : '');
}

/** Samples arrive in time order. Clamp floored death offsets before scaling their own time range. */
export function hpTraceView(samples: HpSample[], width: number, height: number, diedAt?: string): { paths: string[]; gaps: number } {
  const readings = normalizedReadings(samples, diedAt);
  const valid = readings.filter(reading => reading !== null);
  if (!valid.length) return { paths: [], gaps: 0 };
  const start = valid[0].time, span = valid.at(-1)!.time - start;
  const paths: string[] = [];
  let points: string[] = [], previous: bigint | null = null;
  const flush = () => {
    // A zero-length segment with a round cap keeps a lone observation visible.
    if (points.length) paths.push(`M ${points[0]} L ${points.length === 1 ? points[0] : points.slice(1).join(' L ')}`);
    points = [];
  };
  for (const reading of readings) {
    if (reading === null) { flush(); previous = null; continue; }
    if (previous !== null && reading.at - previous > 1_000_000n) flush();
    const x = span > 0n ? Number(reading.time - start) / Number(span) * width : width;
    const y = height * (1 - reading.ratio);
    points.push(`${Number(x.toFixed(3))} ${Number(y.toFixed(3))}`);
    previous = reading.at;
  }
  flush();
  return { paths, gaps: paths.length - 1 };
}

export function killerLabel(death: DeathDetail | null, objects: ObjectVisual[]): { text: string; spriteType: number | null; verified: boolean } {
  const catalog = objectVisual(objects, death?.killed_by_type);
  if (catalog?.name?.trim()) return { text: catalog.name.trim(), spriteType: catalog.object_type, verified: true };
  return { text: death?.killed_by?.trim() ? death.killed_by : '—', spriteType: catalog?.object_type ?? null, verified: false };
}
