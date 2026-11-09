// Boss-tier fixtures preserve the existing window/focus checks after trash grouping.
import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { emptyListState, normalizeListState } from '../../src/lib/navigation/state';
import { advance, click, flush, follow, largeStory, main, mountRuns, ready, region, run, saved } from '../runs/fixtures';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('normalizes legacy and malformed story presentation values without storing rows', () => {
  expect(emptyListState().storyPosition).toBeNull(); expect(normalizeListState({}).storyPosition).toBeNull();
  const value = normalizeListState({ storyPosition: { allFights: true, otherTargets: true,
    timelineWindow: -1, fightWindow: 2, lootWindow: 1.2, otherFightWindow: 0, timelinePointId: '\u0000' } });
  expect(value.storyPosition).toEqual({ selectedTab: null, allFights: true, otherTargets: true, timelinePointId: null });
  expect(normalizeListState(JSON.parse(JSON.stringify(value)))).toEqual(value);
});

it('normalizes new selected tabs and rejects malformed tab IDs', () => {
  expect(normalizeListState({ storyPosition: { selectedTab: 'fight-64' } }).storyPosition?.selectedTab).toBe('fight-64');
  expect(normalizeListState({ storyPosition: { selectedTab: '\u0000' } }).storyPosition?.selectedTab).toBeNull();
});
it('renders all bounded bosses and bags without window controls', async () => {
  const value = largeStory(64, true);
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  expect(main().getAllByRole('tab')).toHaveLength(65);
  expect(screen.getByRole('region', { name: 'Loot this run' }).querySelectorAll('[data-drop-id]')).toHaveLength(101);
  expect(main().queryByRole('button', { name: /Next Window|Previous Window|Next Bosses/ })).toBeNull();
  expect(page.queries.getFight).not.toHaveBeenCalled();
});
it('removed selected boss falls back to Whole run and does not return after growth', async () => {
  const value = largeStory(4, true);
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  const tab = main().getByRole('tab', { name: 'Invented target 4' });
  await fireEvent.click(tab); await flush(); tab.focus();
  page.queries.getRun.mockResolvedValue(ready(largeStory(1, true)));
  page.event({ run_updated: run() }, '13'); page.setStore(saved('13')); await advance();
  expect(main().getByRole('tab', { name: 'Whole run' }).getAttribute('aria-selected')).toBe('true');
  expect(main().getByText('Row Removed')).toBeTruthy();
  expect(document.activeElement?.hasAttribute('data-story-header')).toBe(true);
  page.queries.getRun.mockResolvedValue(ready(value));
  page.event({ run_updated: run() }, '14'); page.setStore(saved('14')); await advance();
  expect(main().getByRole('tab', { name: 'Whole run' }).getAttribute('aria-selected')).toBe('true');
});
