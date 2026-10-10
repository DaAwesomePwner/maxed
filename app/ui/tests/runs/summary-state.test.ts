import { expect, it } from 'vitest';
import type { RunListSummary } from '../../src/lib/bindings/RunListSummary';
import { defaultFilters, normalizeListState } from '../../src/lib/navigation/state';

const summary = (): RunListSummary => ({ runs: 27, cleared: 20, died: 2, left: 3, unknown: 1,
  time_in_runs_micros: 500_000_000, personal_bests: 2, fame_delta: -412, fame_observed_runs: 20,
  first_started: '1000000', total_runs_all_time: 100,
  last_death: { run_id: 'run-1', dungeon: 'Invented Sanctum', at: '1000000' },
  characters: [{ id: 'test-mage', name: 'TestMage', class_id: 100, runs: 27 }] });
const saved = (summary: unknown) => ({ filters: { ...defaultFilters(), cursor: { id: 'page-1', sort_value: '1' } },
  runsPosition: { previous: [null], offset: 0, summary } });

it('round-trips a detached, bounded summary and strips extra history fields', () => {
  const original = summary();
  const input = { ...original, rows: ['untrusted'], last_death: { ...original.last_death!, extra: 'untrusted' },
    characters: [{ ...original.characters[0], extra: 'untrusted' }] };
  const normalized = normalizeListState(saved(input));
  expect(normalized.runsPosition?.summary).toEqual(original);
  expect(normalizeListState(normalized)).toEqual(normalized);
  input.characters[0].name = 'Mutated'; input.last_death.dungeon = 'Mutated';
  expect(normalized.runsPosition?.summary).toEqual(original);
});

it.each([
  { runs: -1 }, { cleared: Infinity }, { died: 1.5 }, { left: '3' }, { unknown: NaN },
  { time_in_runs_micros: Number.MAX_SAFE_INTEGER + 1 }, { personal_bests: -1 }, { fame_delta: Infinity },
  { fame_observed_runs: -1 }, { total_runs_all_time: null }, { first_started: 'bad' },
  { last_death: { run_id: 'bad\n', dungeon: 'Test', at: '1' } },
  { characters: Array.from({ length: 51 }, () => summary().characters[0]) },
  { characters: [{ ...summary().characters[0], name: 'x'.repeat(257) }] },
  { characters: [{ ...summary().characters[0], class_id: 65536 }] },
  { characters: [{ ...summary().characters[0], runs: -1 }] },
])('discards invalid cached summaries without losing page position (%j)', patch => {
  const normalized = normalizeListState(saved({ ...summary(), ...patch }));
  expect(normalized.runsPosition).toEqual({ previous: [null], offset: 0 });
  expect(normalized.filters.cursor).toEqual({ id: 'page-1', sort_value: '1' });
});

it('loads legacy positions with no summary and preserves unknown summary values', () => {
  expect(normalizeListState(saved(undefined)).runsPosition).toEqual({ previous: [null], offset: 0 });
  const unknown = { ...summary(), fame_delta: null, first_started: null, last_death: null,
    characters: [{ id: 'unknown', name: null, class_id: null, runs: 1 }] };
  expect(normalizeListState(saved(unknown)).runsPosition?.summary).toEqual(unknown);
});
