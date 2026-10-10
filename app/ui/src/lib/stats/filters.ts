import type { TimeRange } from '../bindings/TimeRange';
import type { StatsFilter } from '../bindings/StatsFilter';
import type { StatsMapRow } from '../bindings/StatsMapRow';
import type { StatsMapSort, StatsPosition, StatsRange } from '../navigation/state';

const micros = (date: Date) => (BigInt(Math.max(0, date.getTime())) * 1000n).toString();

/** Local-midnight boundaries, as Home's Today: from N−1 days ago through the end of today. */
export function statsRange(choice: StatsRange, now: Date): TimeRange | null {
  if (choice === 'all' || !Number.isFinite(now.getTime())) return null;
  const days = choice === '7d' ? 7 : 30;
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate() - (days - 1));
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  return { start: micros(start), end: micros(end) };
}

export function statsFilter(position: StatsPosition, now: Date): StatsFilter {
  return { range: statsRange(position.range, now), character_id: position.characterId };
}

function sortValue(row: StatsMapRow, key: StatsMapSort): number | string | null {
  switch (key) {
    case 'map': return row.map;
    case 'visits': return row.visits;
    case 'time': return row.time_micros;
    case 'fame': return row.fame;
    case 'rate': return row.fame_per_hour;
    case 'clears': return row.clears;
    case 'deaths': return row.deaths;
  }
}

/** Rows without a value ("—") sort last in both directions; ties fall back to the map name. */
export function sortMaps(rows: StatsMapRow[], key: StatsMapSort, descending: boolean): StatsMapRow[] {
  return [...rows].sort((a, b) => {
    const x = sortValue(a, key), y = sortValue(b, key);
    if (x === null || y === null) return x === y ? a.map.localeCompare(b.map) : x === null ? 1 : -1;
    const order = typeof x === 'string' ? x.localeCompare(String(y)) : x - Number(y);
    return (descending ? -order : order) || a.map.localeCompare(b.map);
  });
}
