import type { StatsSessionRow } from '../bindings/StatsSessionRow';
import type { KpiCell } from '../home/v3/types';
import { durationText, fameText, lootRateText, rateText } from './presentation';

/** Page totals only: do not imply that the bounded page covers all saved sessions. */
export function sessionPageKpis(rows: StatsSessionRow[]): KpiCell[] {
  const duration = rows.reduce((sum, row) => sum + row.duration_micros, 0);
  const fame = rows.reduce((sum, row) => sum + row.fame, 0);
  const missing = rows.some(row => row.missing_baseline);
  const perHour = (total: number) => duration >= 300_000_000 ? total * 3_600_000_000 / duration : null;
  const rate = perHour(fame);
  const bags = rows.reduce((sum, row) => sum + row.bags, 0);
  const notable = rows.reduce((sum, row) => sum + row.notable_bags, 0);
  return [
    { key: 'sessions', label: 'Sessions', value: String(rows.length), sub: 'On this page' },
    { key: 'time', label: 'Time', value: durationText(duration), sub: 'On this page' },
    { key: 'fame', label: 'Fame', value: fameText(fame, missing), tone: 'fame', sub: 'On this page' },
    { key: 'rate', label: 'Fame/hour', value: `${missing && rate !== null ? '≥ ' : ''}${rateText(rate)}`,
      sub: 'On this page', tooltip: 'Total observed fame divided by time on this page. Requires five minutes.' },
    { key: 'bags-rate', label: 'Bags/hour', value: lootRateText(perHour(bags)), sub: 'On this page',
      tooltip: 'Total observed bags divided by time on this page. Includes every bag in Loot Feed (Any bag). Requires five minutes.' },
    { key: 'notable-rate', label: 'Notable bags/hour', value: lootRateText(perHour(notable)), sub: 'On this page',
      tooltip: 'Observed orange/ST, red and white bags (including boosted variants) divided by time on this page. Requires five minutes.' },
  ];
}
