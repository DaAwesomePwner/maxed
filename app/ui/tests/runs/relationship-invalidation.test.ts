import { cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import { advance, fight, flush, main, mountRuns, player, ready, saved } from './fixtures';

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => {
  cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks();
});

function notice(kind: 'player_observed' | 'scope_timeline_updated', runId: string): EngineEvent {
  return kind === 'player_observed'
    ? { player_observed: { scope_id: 'scope', run_id: runId, player_id: 'player-1', object_id: 7,
      is_local: false, maxed_stats: [], stats: [], equipment: [], first_seen: '1', last_seen: '2' } }
    : { scope_timeline_updated: { scope_id: 'scope', run_id: runId, buckets: [], players: [], full: false,
      compacted: false, disabled: false } };
}

it.each(['player_observed', 'scope_timeline_updated'] as const)(
  'refreshes standalone fight player on durable related %s only', async kind => {
    const page = mountRuns({ hash: '#/fights/fight-1/players/7' }); await flush();
    expect(main().getByTitle('Invented Archer')).toBeTruthy();
    const revised = player({ name: 'Updated Archer' });
    page.queries.getFight.mockResolvedValue(ready(fight({ players: [revised] })));
    page.queries.getPlayerBreakdown.mockResolvedValue(ready(revised));
    page.event(notice(kind, 'other-run'), '13');
    page.setStore(saved('13')); await advance();
    expect(main().queryByTitle('Updated Archer')).toBeNull();
    page.event(notice(kind, 'run-1'), '14'); await flush();
    expect(main().queryByTitle('Updated Archer')).toBeNull();
    page.setStore(saved('14')); await advance();
    expect(main().getByTitle('Updated Archer')).toBeTruthy();
  });

it.each(['player_observed', 'scope_timeline_updated'] as const)(
  'ignores %s when the reported relationship belongs to a different fight', async kind => {
    const page = mountRuns({ hash: '#/fights/fight-1/players/7',
      queries: { getFight: async () => ready(fight({ id: 'other-fight' })) } });
    await flush();
    expect(main().getByText('Player Unavailable')).toBeTruthy();
    page.queries.getFight.mockResolvedValue(ready(fight()));
    page.event(notice(kind, 'run-1'), '13');
    page.setStore(saved('13')); await advance();
    expect(main().getByText('Player Unavailable')).toBeTruthy();
    expect(main().queryByTitle('Invented Archer')).toBeNull();
    // A fight-scoped event still recovers the route.
    page.event({ fight_updated: fight() }, '14');
    page.setStore(saved('14')); await advance();
    expect(main().getByTitle('Invented Archer')).toBeTruthy();
  });
