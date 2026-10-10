import { setDebugMode } from '../../src/lib/debug/debug';
import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { TimelineEntry } from '../../src/lib/bindings/TimelineEntry';
import { bossEncounters, storyTimeline, storyVisualTypes } from '../../src/lib/runs/story';
import { fightSummary, flush, follow, largeStory, main, mountRuns, ready, region, runPage, story } from './fixtures';

beforeEach(() => { setDebugMode(true); vi.useFakeTimers(); });
afterEach(() => { setDebugMode(false); cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('selects every boss tier in time order and keeps legacy boss markers', () => {
  const fights = [fightSummary(9, { boss_type: 9 }),
    ...(['quest', 'miniboss', 'event_boss', 'boss', 'ordinary', null] as const)
      .map((target_kind, index) => fightSummary(index + 1, { target_kind }))];
  expect(bossEncounters(fights).map(fight => fight.id)).toEqual(['fight-1', 'fight-2', 'fight-3', 'fight-4', 'fight-9']);
});

it('groups ordinary and unknown fights only between notable events without mutating saved rows', () => {
  const value = largeStory(6); value.run.evidence = [];
  value.fights[1].target_type = null;
  value.fights[1].target_kind = null;
  value.fights[3].target_kind = 'miniboss';
  const point = (id: string, at: string, kind: TimelineEntry['kind']): TimelineEntry => ({ id, at, kind, related_id: null, explanation: id });
  value.timeline.push(point('drop', '2002500', 'drop'), point('Ada left (cause unconfirmed)', '2003100', 'player_left'),
    point('Bo left (cause unconfirmed)', '2003200', 'player_left'), point('portal', '2005500', 'portal_opened'));
  const before = structuredClone(value);
  const rows = storyTimeline(value);
  expect(rows.map(row => row.group ?? row.kind)).toEqual(['enemies', 'drop', 'enemies', 'departures', 'fight', 'enemies', 'portal_opened', 'enemies']);
  expect(rows[0]).toMatchObject({ count: 2, damage: 300, related_id: null });
  expect(rows[3]).toMatchObject({ count: 2, explanation: '2 players left' });
  expect(rows[3].members?.map(row => row.explanation)).toEqual(['Ada left (cause unconfirmed)', 'Bo left (cause unconfirmed)']);
  expect(rows[4].related_id).toBe('fight-4');
  expect(value).toEqual(before);
});

it('retains every evidence boundary and synthesizes legacy fights before grouping', () => {
  const value = largeStory(2); value.timeline = [];
  value.run.evidence = [{ at: '2001500', evidence: 'death_packet' }];
  expect(storyTimeline(value).map(row => row.group ?? row.kind)).toEqual(['enemies', 'death', 'enemies']);
  for (const kind of ['entered', 'kill', 'drop', 'death', 'party_death', 'completion', 'exit', 'capture_issue', 'portal_opened'] as const) {
    value.run.evidence = [];
    value.timeline = [{ id: kind, kind, at: '2001500', related_id: null, explanation: kind }];
    expect(storyTimeline(value).map(row => row.group ?? row.kind)).toEqual(['enemies', kind, 'enemies']);
  }
});

it('renders every supplied boss tab, including legacy minibosses, without detail reads', async () => {
  const value = largeStory(64, true);
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  expect(main().getAllByRole('tab')).toHaveLength(65);
  expect(page.visuals.mock.calls.every(([ids]) => ids.length <= 128)).toBe(true);
  await fireEvent.click(main().getByRole('tab', { name: 'Invented target 64' })); await flush();
  expect(main().getByRole('link', { name: 'Open fight ›' }).getAttribute('href')).toBe('#/runs/run-1/fights/fight-64');
  expect(page.queries.getFight).not.toHaveBeenCalled();
});
it('does not call provisional kills defeated', async () => {
  const value = largeStory(1, true);
  value.boss_fights![0] = { ...value.boss_fights![0], end_reason: 'session_idle', kill: { at: '8000000', evidence: 'hp_zero_removed', provisional: true } };
  mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  await fireEvent.click(main().getAllByRole('tab')[1]); await flush();
  expect(region('Fight damage').queryByText(/Killed ·|Defeated/)).toBeNull();
});
