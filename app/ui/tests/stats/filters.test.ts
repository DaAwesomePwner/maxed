import { expect, it } from 'vitest';
import { sortMaps, statsRange } from '../../src/lib/stats/filters';
import { durationText, fameText, rateText } from '../../src/lib/stats/presentation';
import { statsQueries } from '../shell/fixtures/stats';

const at = (date: Date) => (BigInt(date.getTime()) * 1000n).toString();

it('ranges start at local midnight and end tomorrow', () => {
  const now = new Date(2026, 9, 6, 15, 30);
  expect(statsRange('all', now)).toBeNull();
  expect(statsRange('7d', now)).toEqual({ start: at(new Date(2026, 8, 30)), end: at(new Date(2026, 9, 7)) });
  expect(statsRange('30d', now)).toEqual({ start: at(new Date(2026, 8, 7)), end: at(new Date(2026, 9, 7)) });
});
it('sorts dashes last in both directions', () => {
  const rows = statsQueries.maps!({ range: null, character_id: null }).rows;
  expect(sortMaps(rows, 'rate', true).map(r => r.map)).toEqual(['Lost Halls', 'Realm', 'Hubs', 'Pirate Cave']);
  expect(sortMaps(rows, 'rate', false).map(r => r.map)).toEqual(['Hubs', 'Realm', 'Lost Halls', 'Pirate Cave']);
  expect(sortMaps(rows, 'map', false).map(r => r.map)).toEqual(['Hubs', 'Lost Halls', 'Pirate Cave', 'Realm']);
});
it('formats fame, rates and durations', () => {
  expect(fameText(1840, false)).toBe('1,840');
  expect(fameText(900, true)).toBe('≥ 900');
  expect(rateText(null)).toBe('—');
  expect(rateText(1162.13)).toBe('1,162.1');
  expect(durationText(95 * 60_000_000)).toBe('1h 35m');
  expect(durationText(3 * 60_000_000)).toBe('3m');
});
