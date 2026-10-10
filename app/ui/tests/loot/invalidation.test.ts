import { expect, it } from 'vitest';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { DropView } from '../../src/lib/bindings/DropView';
import type { Route } from '../../src/lib/navigation/routes';
import { invalidatesLootHistory } from '../../src/lib/loot/invalidation';
import { homeSnapshot } from '../home/fixtures';

const snapshot = homeSnapshot();
// Same synthetic stream/sequence envelope shape as tests/runs/fixtures.event.
const envelope = (event: EngineEvent): EventEnvelope => ({ stream_id: snapshot.stream_id, sequence: '13', at: snapshot.at, event });
const dropEvent = (kind: 'bag_dropped' | 'drop_updated', drop: DropView) => envelope(kind === 'bag_dropped' ? { bag_dropped: drop } : { drop_updated: drop });
const drop = { ...snapshot.drops[0], items: [7], contents: [] };
const routes: Route[] = [{ kind: 'loot', view: 'feed' }, { kind: 'loot', view: 'stats' }, { kind: 'loot', view: 'collection' }, { kind: 'item', type: 7 }];

it.each(['bag_dropped', 'drop_updated'] as const)('%s invalidates feed, stats and collection', kind => {
  for (const route of routes.slice(0, 3)) expect(invalidatesLootHistory(route, dropEvent(kind, drop))).toBe(true);
});
it.each(['bag_dropped', 'drop_updated'] as const)('%s invalidates only matching item pages via contents or items', kind => {
  const content = { slot: 0, item_type: 7, quantity: 1, enchant: { state: 'missing' as const, slots: null, applied: null, ids: [] }, taken_observed_at: null };
  for (const bag of [drop, { ...drop, items: [], contents: [content] }]) {
    expect(invalidatesLootHistory({ kind: 'item', type: 7 }, dropEvent(kind, bag))).toBe(true);
    expect(invalidatesLootHistory({ kind: 'item', type: 8 }, dropEvent(kind, bag))).toBe(false);
  }
  expect(invalidatesLootHistory({ kind: 'item', type: 7 }, dropEvent(kind, { ...drop, items: [], contents: [] }))).toBe(false);
});
it('run_ended invalidates feed white-rate summaries, stats, collection and item dry streaks', () => {
  expect(routes.map(route => invalidatesLootHistory(route, envelope({ run_ended: snapshot.run! })))).toEqual([true, true, true, true]);
});
it('drop_updated invalidates the matching item page when its content was taken', () => {
  const content = { slot: 0, item_type: 7, quantity: 1, enchant: { state: 'missing' as const, slots: null, applied: null, ids: [] }, taken_observed_at: null };
  const event = dropEvent('drop_updated', { ...drop, items: [], contents: [{ ...content, taken_observed_at: '20' }] });
  expect(invalidatesLootHistory({ kind: 'item', type: content.item_type }, event)).toBe(true);
});
it('numeric live updates and session_updated never invalidate loot', () => {
  const events: EngineEvent[] = [{ session_updated: snapshot.session! }, { fight_updated: snapshot.fights[0] },
    { fame_changed: { at: snapshot.at, character_id: 'synthetic-character', fame: 99 } },
    { character_updated: snapshot.character! }, { run_updated: snapshot.run! }];
  for (const route of routes) for (const event of events) expect(invalidatesLootHistory(route, envelope(event))).toBe(false);
});
it('unrelated or absent routes never invalidate', () => {
  const unrelated: Array<Route | undefined> = [undefined, { kind: 'home' }, { kind: 'runs' }, { kind: 'run', id: 'run-1' }, { kind: 'alerts' }, { kind: 'settings' }, { kind: 'setup' }, { kind: 'not-found' }];
  for (const route of unrelated) for (const event of [{ bag_dropped: drop }, { drop_updated: drop }, { run_ended: snapshot.run! }]) {
    expect(invalidatesLootHistory(route, envelope(event))).toBe(false);
  }
});
