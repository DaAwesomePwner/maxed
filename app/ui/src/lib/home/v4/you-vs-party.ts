import type { DamageBucket } from '../../bindings/DamageBucket';

export interface PartySeries { you: (number | null)[]; party: (number | null)[]; partyRecorded: boolean }

function interval(bucket: DamageBucket): { from: bigint; until: bigint } | null {
  if (!/^\d+$/.test(bucket.from) || !/^\d+$/.test(bucket.until)) return null;
  const from = BigInt(bucket.from), until = BigInt(bucket.until);
  return until > from ? { from, until } : null;
}

function rate(damage: number | null | undefined, seconds: number | null | undefined): number | null {
  if (damage == null || seconds == null || !Number.isFinite(damage) || !Number.isFinite(seconds)
    || damage < 0 || seconds <= 0) return null;
  const value = damage / seconds;
  return Number.isFinite(value) ? value : null;
}

export function partySeries(buckets: DamageBucket[]): PartySeries {
  return {
    you: buckets.map(bucket => {
      const time = interval(bucket);
      return rate(bucket.local, time ? Number(time.until - time.from) / 1_000_000 : null);
    }),
    party: buckets.map(bucket => rate(bucket.others_damage, bucket.others_presence)),
    partyRecorded: buckets.length > 0 && buckets.every(bucket => bucket.others_damage != null && bucket.others_presence != null),
  };
}

/** Use actual interval endpoints and break paths at absent observations or time gaps. */
export function partyChartView(buckets: DamageBucket[], width: number, height: number) {
  const series = partySeries(buckets);
  const times = buckets.map(interval);
  let start: bigint | null = null, end: bigint | null = null, peak = 0;
  for (let index = 0; index < times.length; index++) {
    const time = times[index];
    if (!time) continue;
    if (start === null || time.from < start) start = time.from;
    if (end === null || time.until > end) end = time.until;
    peak = Math.max(peak, series.you[index] ?? 0, series.partyRecorded ? series.party[index] ?? 0 : 0);
  }
  const rough = Math.max(peak, 1) / 4;
  const power = 10 ** Math.floor(Math.log10(rough));
  const step = ([1, 2, 5, 10].find(multiplier => multiplier * power >= rough) ?? 10) * power;
  const max = Math.ceil(Math.max(peak, 1) / step) * step;
  const left = 44, right = Math.max(left + 1, width - 10), top = 10, bottom = height - 24;
  const x = (at: bigint) => start === null || end === null ? left
    : left + Number((at - start) * 1_000_000n / (end - start)) / 1_000_000 * (right - left);
  const y = (value: number) => bottom - value / max * (bottom - top);
  const ticks = Array.from({ length: Math.round(max / step) + 1 }, (_, index) => {
    const value = Number((index * step).toPrecision(12));
    return { y: y(value), label: value >= 1000 ? `${value / 1000}k` : String(value) };
  });
  function line(values: (number | null)[]) {
    let path = '', previous: bigint | null = null;
    let marker: { x: number; y: number; value: number } | null = null;
    for (let index = 0; index < times.length; index++) {
      const time = times[index], value = values[index];
      if (!time || value === null) { previous = null; continue; }
      path += `${previous === time.from ? 'L' : 'M'}${x(time.from)},${y(value)} L${x(time.until)},${y(value)} `;
      marker = { x: x(time.until), y: y(value), value };
      previous = time.until;
    }
    return { path: path.trim(), marker };
  }
  const you = line(series.you), party = line(series.partyRecorded ? series.party : buckets.map(() => null));
  return { ...series, start, end, ticks, left, right, youLine: you, partyLine: party,
    hasData: you.marker !== null || party.marker !== null };
}
