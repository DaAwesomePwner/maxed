import { afterEach, expect, it, vi } from 'vitest';
import { dateRange, validRunFilter, withRunFilter } from '../../src/lib/runs/filters';
import { defaultFilters, normalizeListState } from '../../src/lib/navigation/state';
import { invalidatesRunHistory } from '../../src/lib/runs/invalidation';
import { homeSnapshot } from '../home/fixtures';

afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });
it('restores a safe character filter and normalizes legacy filters to null', () => {
  expect(normalizeListState({ filters: { ...defaultFilters(), character_id: 'test-mage' } }).filters).toMatchObject({ character_id: 'test-mage' });
  expect(normalizeListState({ filters: { ...defaultFilters(), character_id: undefined } }).filters).toMatchObject({ character_id: null });
  expect(normalizeListState({ filters: { ...defaultFilters(), character_id: null } }).filters).toMatchObject({ character_id: null });
});
it.each(['', 'bad\ncharacter', 'bad\u0085character', 'x'.repeat(257), 42])('rejects invalid character filter %s', character_id => {
  const filters = { ...defaultFilters(), character_id };
  expect(validRunFilter(filters)).toBe(false);
  expect(normalizeListState({ filters }).filters).toEqual(defaultFilters());
});
it('resets only the cursor when changing filters, preserving limit and null versus Unknown', () => {
  const current = { ...defaultFilters(), limit: 50, cursor: { id: 'last', sort_value: '12' } };
  expect(withRunFilter(current, { outcome: 'unknown' })).toEqual({ ...current, outcome: 'unknown', cursor: null });
  expect(withRunFilter(current, { outcome: null }).outcome).toBeNull();
  expect(withRunFilter(current, { character_id: 'test-mage' })).toEqual({ ...current, character_id: 'test-mage', cursor: null });
  expect(current.cursor?.id).toBe('last');
});
it.each([['', '2026-10-02'], ['2026-10-02', ''], ['2026-02-30', '2026-03-01'], ['2026-10-03', '2026-10-02'], ['invalid', 'invalid']])('rejects invalid local date pair %s / %s', (first, last) => {
  expect(() => dateRange(first, last)).toThrow(RangeError);
  expect(dateRange('', '')).toBeNull();
});
it('normalizes older and malformed browser state without passing invalid native filters', () => {
  expect(normalizeListState({})).toMatchObject({ filters: defaultFilters(), runsPosition: null, selectedFightId: null });
  expect(validRunFilter({ ...defaultFilters(), cursor: { id: 'last', sort_value: '1e9' } })).toBe(false);
  expect(validRunFilter({ ...defaultFilters(), range: { start: '12', end: '11' } })).toBe(false);
  const restored = normalizeListState({ filters: defaultFilters(), runsPosition: { previous: Array.from({ length: 110 }, () => null) } });
  expect(restored.runsPosition?.previous).toHaveLength(100);
  expect(restored.filters.limit).toBe(25);
});
it('matches fight ticks synchronously only on their current P4 detail route', () => {
  const snapshot = homeSnapshot();
  const fight = { ...snapshot.fights[0], id: 'fight-a', run_id: 'run-a' };
  const event = { stream_id: snapshot.stream_id, sequence: '13', at: snapshot.at, event: { fight_updated: fight } };
  expect(invalidatesRunHistory({ kind: 'home' }, event)).toBe(false);
  expect(invalidatesRunHistory({ kind: 'runs' }, event)).toBe(false);
  expect(invalidatesRunHistory({ kind: 'run', id: 'other' }, event)).toBe(false);
  expect(invalidatesRunHistory({ kind: 'run', id: 'run-a' }, event)).toBe(true);
  expect(invalidatesRunHistory({ kind: 'fight', id: 'fight-a' }, event)).toBe(true);
  expect(invalidatesRunHistory({ kind: 'player', fightId: 'fight-a', objectId: -1 }, event)).toBe(true);
});

it('retains the absolute page offset after clipping the cursor history', () => {
  const state = normalizeListState({ filters: defaultFilters(), runsPosition: { previous: Array.from({ length: 102 }, () => null), offset: 3 } });
  expect(state.runsPosition).toEqual({ previous: Array.from({ length: 100 }, () => null), offset: 5 });
  expect(normalizeListState(state)).toEqual(state);
  expect(normalizeListState({ filters: defaultFilters(), runsPosition: { previous: [], offset: -1 } }).runsPosition?.offset).toBe(0);
});
