import { expect, it } from 'vitest';
import { eventParts, kindSegments, runCounts } from '../../src/lib/logs/events-screen';
import type { InstanceEventView } from '../../src/lib/bindings/InstanceEventView';
import type { EventSummary } from '../../src/lib/bindings/EventSummary';
import type { RunStory } from '../../src/lib/bindings/RunStory';

const base = { id: 'synthetic-event', session_id: 'synthetic', at: '1', provisional: false, map: null };
const className = (id: number) => id === 1 ? 'Rogue' : null;
it('keeps unavailable segment counts unknown and formats known counts', () => {
  expect(kindSegments(null).map(segment => segment.count)).toEqual(['—', '—', '—', '—', '—']);
  const summary: EventSummary = { total: 1906, by_kind: { player_death: 23, player_left: 0, realm_event_killed: 38, portal_opened: 54 }, dungeons: [], local_pops: 0, last_local_pop: null, flagged_since: null, first_at: null };
  expect(kindSegments(summary).map(segment => [segment.key, segment.label, segment.count])).toEqual([
    ['all', 'All', '1,906'], ['death', 'Deaths', '23'], ['left', 'Departures', '0'], ['realm', 'Realm kills', '38'], ['key', 'Key pops', '54'],
  ]);
});
it('formats death observations without inventing missing level, class or killer', () => {
  expect(eventParts({ ...base, map: 'Invented vault', kind: { type: 'player_death', player_name: 'Kestrel', level: 20, class_id: 1, killed_by: 'Lantern Colossus', is_local: false } }, className))
    .toEqual({ kind: 'death', who: 'Kestrel', detail: 'Level 20 · Rogue · killed by Lantern Colossus', where: 'Invented vault' });
  expect(eventParts({ ...base, provisional: true, kind: { type: 'player_death', player_name: 'Kestrel', level: null, class_id: 2, killed_by: null, is_local: false } }, className))
    .toEqual({ kind: 'death', who: 'Kestrel', detail: '', where: '—' });
});
it('formats departures, realm kills and portals with nullable observations', () => {
  const cases: Array<[InstanceEventView['kind'], ReturnType<typeof eventParts>]> = [
    [{ type: 'player_left', object_id: 1, player_name: null, class_id: null }, { kind: 'left', who: null, detail: 'left', where: '—' }],
    [{ type: 'player_left', object_id: 1, player_name: 'Quenby', class_id: 1 }, { kind: 'left', who: 'Quenby', detail: 'left · Rogue', where: '—' }],
    [{ type: 'realm_event_killed', boss_key: 'Lantern Colossus', killer: null }, { kind: 'realm', who: 'Lantern Colossus', detail: 'killed', where: '—' }],
    [{ type: 'realm_event_killed', boss_key: 'Lantern Colossus', killer: 'Morrow' }, { kind: 'realm', who: 'Lantern Colossus', detail: 'killed by Morrow', where: '—' }],
    [{ type: 'portal_opened', opener: null, dungeon: null }, { kind: 'key', who: null, detail: 'opened a portal', where: '—' }],
    [{ type: 'portal_opened', opener: 'Vexlin', dungeon: 'Invented vault' }, { kind: 'key', who: 'Vexlin', detail: 'opened Invented vault', where: '—' }],
  ];
  for (const [kind, expected] of cases) expect(eventParts({ ...base, kind, provisional: true }, className)).toEqual(expected);
});
it('counts observed run deaths and departures including provisional observations', () => {
  const story = { events: [{ ...base, provisional: true, kind: { type: 'player_left', object_id: 1 } },
    { ...base, kind: { type: 'player_death', player_name: 'Kestrel', is_local: false } },
    { ...base, kind: { type: 'portal_opened' } }] } as RunStory;
  expect(runCounts(story)).toEqual({ deaths: 1, departures: 1 });
  expect(runCounts({ events: null } as RunStory)).toEqual({ deaths: 0, departures: 0 });
});
