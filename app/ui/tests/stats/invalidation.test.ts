import { expect, it } from 'vitest';
import { invalidatesStatsHistory } from '../../src/lib/stats/invalidation';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import fixture from '../fixtures/t0-live';

const envelope = (event: EventEnvelope['event']): EventEnvelope => ({ stream_id: 'live-1', sequence: '2', at: '1', event });
const stats = { kind: 'stats', view: 'maps' } as const;

it('re-reads on map changes, run ends and session updates', () => {
  expect(invalidatesStatsHistory(stats, envelope({ map_entered: { realm_instance_key: null, at: '1', name: 'Nexus', kind: 'nexus' } }))).toBe(true);
  expect(invalidatesStatsHistory(stats, envelope({ run_ended: fixture.run! }))).toBe(true);
  expect(invalidatesStatsHistory(stats, envelope({ session_updated: fixture.session! }))).toBe(true);
});
it('ignores fame updates and other routes', () => {
  const fame = envelope({ fame_changed: { at: '1', character_id: 'c', fame: 1 } });
  expect(invalidatesStatsHistory(stats, fame)).toBe(false);
  expect(invalidatesStatsHistory(stats, envelope({ run_started: fixture.run! }))).toBe(false);
  expect(invalidatesStatsHistory({ kind: 'runs' }, envelope({ run_ended: fixture.run! }))).toBe(false);
  expect(invalidatesStatsHistory(undefined, envelope({ run_ended: fixture.run! }))).toBe(false);
});
