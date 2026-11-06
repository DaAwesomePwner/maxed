import type { StatsFixtureQueries } from '../../../src/lib/bridge/fixture';
import type { StatsCharacter } from '../../../src/lib/bindings/StatsCharacter';
import type { StatsMapRow } from '../../../src/lib/bindings/StatsMapRow';
import type { StatsSessionRow } from '../../../src/lib/bindings/StatsSessionRow';
import type { StatsDungeonRow } from '../../../src/lib/bindings/StatsDungeonRow';
import fixture from '../../fixtures/t0-live';

const MIN = 60_000_000;
export const statsCharacters: StatsCharacter[] = [
  { id: 'preview-alpha', name: 'SynthAlpha', class_id: 782, last_seen: '7200000000' },
  { id: 'preview-bravo', name: 'SynthBravo', class_id: 768, last_seen: '3600000000' },
];
export const statsMapRows: StatsMapRow[] = [
  { group: 'realm', map: 'Realm', visits: 4, time_micros: 95 * MIN, fame: 1840, missing_baseline: false, fame_per_hour: 1162.1, clears: null, deaths: null },
  { group: 'dungeon', map: 'Lost Halls', visits: 2, time_micros: 38 * MIN, fame: 900, missing_baseline: true, fame_per_hour: 1421.1, clears: 1, deaths: 1 },
  { group: 'hubs', map: 'Hubs', visits: 6, time_micros: 22 * MIN, fame: 0, missing_baseline: false, fame_per_hour: 0, clears: null, deaths: null },
  { group: 'dungeon', map: 'Pirate Cave', visits: 1, time_micros: 3 * MIN, fame: 40, missing_baseline: false, fame_per_hour: null, clears: 1, deaths: 0 },
];

const HOUR = 60 * MIN;
export const statsSessionRows: StatsSessionRow[] = [
  { id: 'preview-session-2', started: String(30 * HOUR), ended: null, duration_micros: 40 * MIN, fame: 620,
    missing_baseline: false, fame_per_hour: 930, bags: 8, notable_bags: 2, bags_per_hour: 12, notable_bags_per_hour: 3, runs: 2, clears: 1, deaths: 0 },
  { id: 'preview-session-1', started: String(5 * HOUR), ended: String(7 * HOUR), duration_micros: 2 * HOUR, fame: 1200,
    missing_baseline: true, fame_per_hour: 600, bags: 40, notable_bags: 4, bags_per_hour: 20, notable_bags_per_hour: 2, runs: 3, clears: 3, deaths: 1 },
];
const previewRun = { ...fixture.run!, id: 'preview-run', session_id: 'preview-session-1' };

export const statsDungeonRows: StatsDungeonRow[] = [
  { dungeon: 'Lost Halls', runs: 12, clears: 9, deaths: 2, observed_clears: 8, mean_clear_micros: 14.5 * MIN,
    best_clear_micros: 11 * MIN, total_micros: 190 * MIN, last_started: String(30 * HOUR) },
  { dungeon: 'Pirate Cave', runs: 3, clears: 3, deaths: 0, observed_clears: 0, mean_clear_micros: null,
    best_clear_micros: null, total_micros: 9 * MIN, last_started: String(6 * HOUR) },
];

export const statsQueries: StatsFixtureQueries = {
  sessions: filter => ({
    rows: filter.cursor ? [] : statsSessionRows.filter(r => filter.include_empty || r.fame !== 0 || r.runs > 0),
    next: null, characters: statsCharacters,
  }),
  session: request => {
    const session = statsSessionRows.find(r => r.id === request.id);
    return session ? { session, maps: statsMapRows.slice(0, 3), maps_truncated: false, unrecorded_fame: 120,
      runs: [previewRun], runs_truncated: false, characters: statsCharacters } : null;
  },
  dungeons: () => ({ rows: statsDungeonRows, truncated: false, characters: statsCharacters }),
  maps: () => ({ rows: statsMapRows, truncated: false, tracking_since: '1700000000000000', characters: statsCharacters }),
};
