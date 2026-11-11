import { expect, it } from 'vitest';
import { sessionPageKpis } from '../../src/lib/stats/session-kpis';
import type { StatsSessionRow } from '../../src/lib/bindings/StatsSessionRow';
const row = (duration: number, fame: number, missing = false): StatsSessionRow => ({
  id: 'synthetic-session', started: '0', ended: '1', duration_micros: duration, fame,
  missing_baseline: missing, fame_per_hour: null, bags: 0, notable_bags: 0, bags_per_hour: null, notable_bags_per_hour: null, runs: 0, clears: 0, deaths: 0,
});
it('weights rate by observed time and labels the bounded page scope', () => {
  const cells = sessionPageKpis([row(3_600_000_000, 100), row(7_200_000_000, 500, true)]);
  expect(cells.find(c => c.key === 'rate')?.value).toBe('≥ 200');
  expect(cells.find(c => c.key === 'fame')?.value).toBe('≥ 600');
  expect(cells.every(c => c.sub === 'this page')).toBe(true);
});
it('preserves the five-minute threshold and real zero', () => {
  expect(sessionPageKpis([row(299_999_999, 10)]).find(c => c.key === 'rate')?.value).toBe('—');
  expect(sessionPageKpis([row(300_000_000, 0)]).find(c => c.key === 'rate')?.value).toBe('0');
});

it('weights bag rates by total page time rather than averaging row rates', () => {
  const cells = sessionPageKpis([
    { ...row(3_600_000_000, 0), bags: 12, notable_bags: 3, bags_per_hour: 12, notable_bags_per_hour: 3 },
    { ...row(7_200_000_000, 0), bags: 6, notable_bags: 1, bags_per_hour: 3, notable_bags_per_hour: 0.5 },
  ]);
  expect(cells.find(c => c.key === 'bags-rate')?.value).toBe('6');
  expect(cells.find(c => c.key === 'notable-rate')?.value).toBe('1.3');
  expect(cells.every(c => c.sub === 'this page')).toBe(true);
});
it('applies the five-minute minimum to page bag rates, including empty and zero totals', () => {
  for (const key of ['bags-rate', 'notable-rate']) {
    expect(sessionPageKpis([]).find(c => c.key === key)?.value).toBe('—');
    expect(sessionPageKpis([{ ...row(299_999_999, 0), bags: 1, notable_bags: 1 }]).find(c => c.key === key)?.value).toBe('—');
    expect(sessionPageKpis([row(300_000_000, 0)]).find(c => c.key === key)?.value).toBe('0');
    expect(sessionPageKpis([
      { ...row(150_000_000, 0), bags: 1, notable_bags: 1 }, row(150_000_000, 0),
    ]).find(c => c.key === key)?.value).toBe('12');
  }
});
