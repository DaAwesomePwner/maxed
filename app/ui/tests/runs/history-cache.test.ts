import { expect, it } from 'vitest';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { Route } from '../../src/lib/navigation/routes';
import { createHistoryCache, isFresh } from '../../src/lib/runs/history-cache';
import { historyKey, type HistoryInput, type HistoryQuery, type HistoryValue } from '../../src/lib/runs/history';
import { invalidatesSavedHistory } from '../../src/lib/shell/history-invalidation';
import { homeSnapshot } from '../home/fixtures';

const loot: Route = { kind: 'loot', view: 'feed' };
const runs: Route = { kind: 'runs' };
const value: HistoryValue = { kind: 'drops', page: { rows: [], next: null } };
const query: HistoryQuery = { kind: 'drops', filter: { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 20 } };
const entry = (route: Route = loot) => ({ value, route, query, required: '39' });
const bagEvent = (sequence: string): EventEnvelope => ({ stream_id: 'synthetic', sequence, at: '1',
  event: { bag_dropped: homeSnapshot().drops[0] } });
function harness(options: { capacity?: number; maxAgeMs?: number } = {}) {
  let clock = 100, route: Route | undefined = loot;
  const cache = createHistoryCache({ ...options, now: () => clock, route: () => route,
    invalidates: (route, event) => route.kind === 'loot' && 'bag_dropped' in event.event });
  return { cache, advance: (ms: number) => { clock += ms; }, now: () => clock,
    navigate: (next: Route | undefined) => { route = next; } };
}

it('put_then_get_returns_clean_entry', () => {
  const h = harness(); h.cache.put('loot', entry());
  const stored = h.cache.get('loot')!;
  expect(stored).toEqual({ ...entry(), storedAt: h.now(), dirtySince: null, dirtyAll: false });
  expect(stored.value).toBe(value);
  expect(h.cache.get('loot')).toBe(stored);
  expect(isFresh(stored, h.now(), 300_000)).toBe(true);
  expect(h.cache.fresh(stored)).toBe(true);
});

it('observe_marks_only_matching_routes_and_keeps_first_sequence', () => {
  const h = harness(); h.cache.put('loot', entry()); h.cache.put('runs', entry(runs));
  const stored = h.cache.get('loot')!;
  h.cache.observe(bagEvent('40')); h.cache.observe(bagEvent('41'));
  expect(stored.dirtySince).toBe('40');
  expect(h.cache.fresh(stored)).toBe(false);
  expect(h.cache.get('runs')!.dirtySince).toBeNull();
});

it('max_age_makes_entry_not_fresh', () => {
  const h = harness(); h.cache.put('loot', entry()); const stored = h.cache.get('loot')!;
  h.advance(299_999);
  expect(isFresh(stored, h.now(), 300_000)).toBe(true); expect(h.cache.fresh(stored)).toBe(true);
  h.advance(1);
  expect(isFresh(stored, h.now(), 300_000)).toBe(false); expect(h.cache.fresh(stored)).toBe(false);
});

it('invalidate_all_and_clear', () => {
  const h = harness(); h.cache.put('loot', entry()); h.cache.put('runs', entry(runs));
  h.cache.invalidateAll();
  for (const key of ['loot', 'runs']) {
    expect(h.cache.get(key)!.dirtyAll).toBe(true);
    expect(h.cache.fresh(h.cache.get(key)!)).toBe(false);
  }
  h.cache.clear();
  expect(h.cache.get('loot')).toBeUndefined(); expect(h.cache.get('runs')).toBeUndefined();
});

it('lru_evicts_least_recently_used_at_capacity', () => {
  const h = harness();
  for (let i = 0; i < 32; i++) h.cache.put(`k${i}`, entry());
  h.cache.get('k0'); h.cache.put('k32', entry());
  expect(h.cache.get('k1')).toBeUndefined();
  expect(h.cache.get('k0')).toBeDefined(); expect(h.cache.get('k32')).toBeDefined();
});

it('replacement_is_clean_and_most_recently_used', () => {
  const h = harness({ capacity: 2 }); h.cache.put('a', entry()); h.cache.put('b', entry());
  h.cache.observe(bagEvent('40')); h.cache.invalidateAll(); h.advance(10);
  h.cache.put('a', entry(runs)); h.cache.put('c', entry());
  expect(h.cache.get('b')).toBeUndefined();
  expect(h.cache.get('a')).toEqual({ ...entry(runs), storedAt: h.now(), dirtySince: null, dirtyAll: false });
});

it('uses_the_configured_clock_age_and_current_route', () => {
  const h = harness({ maxAgeMs: 10 }); h.cache.put('loot', entry());
  h.advance(10); expect(h.cache.fresh(h.cache.get('loot')!)).toBe(false);
  expect(h.cache.route()).toBe(loot);
  h.navigate(runs); expect(h.cache.route()).toBe(runs);
  h.navigate(undefined); expect(h.cache.route()).toBeUndefined();
});

it('historyKey_is_stable_across_property_order', () => {
  const input: HistoryInput = { scopeKey: 'live', streamId: 'synthetic', required: null, store: null,
    query: { kind: 'drops', filter: { min_rank: 4, place: { kind: 'dungeon', name: 'Synthetic Sanctum' },
      item_type: 7, enchanted_only: true, range: { start: '1', end: '2' }, cursor: { at: '2', id: 'a' }, limit: 20 } } };
  const reordered: HistoryInput = { ...input, query: { kind: 'drops', filter: {
    limit: 20, cursor: { id: 'a', at: '2' }, range: { end: '2', start: '1' }, enchanted_only: true,
    item_type: 7, place: { name: 'Synthetic Sanctum', kind: 'dungeon' }, min_rank: 4 } } };
  expect(historyKey(input)).toBe(historyKey(reordered));
  expect(historyKey(input)).toBe(JSON.stringify(['live', 'synthetic',
    ['drops', 4, 'dungeon', 'Synthetic Sanctum', 7, true, '1', '2', '2', 'a', 20]]));
  if (reordered.query.kind !== 'drops') throw new Error('Expected drops query');
  reordered.query.filter.cursor = { at: '2', id: 'b' };
  expect(historyKey(input)).not.toBe(historyKey(reordered));
  expect(historyKey(input)).not.toBe(historyKey({ ...input, scopeKey: 'other' }));
  expect(historyKey(input)).not.toBe(historyKey({ ...input, streamId: 'other' }));
  expect(historyKey(input)).toBe(historyKey({ ...input, required: '40' }));
});


it('reservations_keep_first_matching_event_and_global_invalidation', () => {
  const { cache } = harness();
  const lootToken = cache.begin('loot', loot, query), runsToken = cache.begin('runs', runs, query);
  expect(lootToken).not.toBe(runsToken);
  cache.observe(bagEvent('40')); cache.observe(bagEvent('41')); cache.invalidateAll();
  cache.put('loot', entry(), lootToken); cache.put('runs', entry(runs), runsToken);
  expect(cache.get('loot')).toMatchObject({ dirtySince: '40', dirtyAll: true });
  expect(cache.get('runs')).toMatchObject({ dirtySince: null, dirtyAll: true });
});

it.each(['end', 'put', 'clear'] as const)('%s_releases_reservations', release => {
  const { cache } = harness();
  const token = cache.begin('loot', loot, query);
  if (release === 'end') cache.end(token);
  else if (release === 'put') cache.put('loot', entry(), token);
  else cache.clear();
  cache.observe(bagEvent('40')); cache.invalidateAll();
  cache.put('loot', entry(), token);
  expect(cache.get('loot')).toMatchObject({ dirtySince: null, dirtyAll: false });
});

it.each(['stored', 'in-flight'])('party_chat_dirties_guild_overview_%s', mode => {
  const route: Route = { kind: 'logs', view: 'chat', channel: 'guild' };
  const overview: HistoryQuery = { kind: 'chat-overview', request: { run_id: null } };
  const cache = createHistoryCache({ route: () => route, invalidates: invalidatesSavedHistory });
  const cached = { value: { kind: 'chat-overview' as const, overview: { lines: 0,
    by_channel: { public: 0, guild: 0, party: 0, whisper: 0, unknown: 0 }, first_at: null, latest_at: null } },
    route, query: overview, required: null };
  const token = mode === 'in-flight' ? cache.begin('overview', route, overview) : undefined;
  if (mode === 'stored') cache.put('overview', cached);
  const event: EventEnvelope = { stream_id: 'synthetic', sequence: '13', at: '1', event: { chat: {
    id: 'synthetic-chat', session_id: 'synthetic', run_id: null, map: null, at: '1', channel: 'party',
    direction: null, sender_kind: 'player', sender: 'Synthetic', counterpart: null, is_local: false,
    stars: null, supporter: false, text: 'hello', truncated: false,
  } } };
  expect(invalidatesSavedHistory(route, event)).toBe(false);
  cache.observe(event); cache.observe({ ...event, sequence: '14' });
  if (mode === 'in-flight') cache.put('overview', cached, token);
  expect(cache.get('overview')!.dirtySince).toBe('13');
  expect(cache.fresh(cache.get('overview')!)).toBe(false);
});
