import { expect, it } from 'vitest';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { Route } from '../../src/lib/navigation/routes';
import { invalidatesRunHistory } from '../../src/lib/runs/invalidation';
import { invalidatesSavedHistory } from '../../src/lib/shell/history-invalidation';
import { fight, run } from './fixtures';

const route: Route = { kind: 'run-player', runId: 'run-1', playerId: 'player-1' };
const envelope = (event: EngineEvent): EventEnvelope => ({ stream_id: 'test', sequence: '1', at: '1', event });
function events(runId: string | null): EngineEvent[] {
  return [
    ...(['run_started', 'run_updated', 'run_ended'] as const).map(kind => ({ [kind]: run({ id: runId ?? 'other' }) }) as EngineEvent),
    ...(['fight_started', 'fight_updated', 'fight_ended'] as const).map(kind => ({ [kind]: fight({ run_id: runId }) }) as EngineEvent),
    { player_died: { at: '1', character_id: null, run_id: runId } },
    { instance_event: { id: 'event', session_id: 'session', at: '1', run_id: runId, provisional: false,
      kind: { type: 'player_left', object_id: 7, player_name: 'Synthetic' } } },
    ...notices(runId),
  ];
}
function notices(runId: string | null): EngineEvent[] {
  return [
    { player_observed: { scope_id: 'scope', run_id: runId, player_id: 'player-1', object_id: 7,
      is_local: false, maxed_stats: [], stats: [], equipment: [], first_seen: '1', last_seen: '2' } },
    { scope_timeline_updated: { scope_id: 'scope', run_id: runId, buckets: [], players: [], full: false,
      compacted: false, disabled: false } },
  ];
}

it.each(events('run-1'))('invalidates run player on a matching event: %j', event => {
  expect(invalidatesRunHistory(route, envelope(event))).toBe(true);
  expect(invalidatesSavedHistory(route, envelope(event))).toBe(true);
});
it.each([...events('other'), ...events(null)])('ignores unrelated run player events: %j', event => {
  expect(invalidatesRunHistory(route, envelope(event))).toBe(false);
});
it('refreshes run players on character merges', () => {
  expect(invalidatesRunHistory(route, envelope({ character_merged: { provisional_id: 'p', verified_id: 'v' } }))).toBe(true);
});
it.each(['run-1', 'other', null])('matches fight player notices to its run (%s)', runId => {
  for (const event of notices(runId)) {
    expect(invalidatesRunHistory({ kind: 'player', fightId: 'fight-1', objectId: 7, runId: 'run-1' }, envelope(event))).toBe(runId === 'run-1');
    expect(invalidatesRunHistory({ kind: 'player', fightId: 'fight-1', objectId: 7 }, envelope(event))).toBe(false);
    expect(invalidatesRunHistory({ kind: 'home' }, envelope(event))).toBe(false);
  }
});
