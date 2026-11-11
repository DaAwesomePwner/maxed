import { cleanup, render } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import type { HomeDungeonStreak } from '../../src/lib/bindings/HomeDungeonStreak';
import DryStreakRow from '../../src/lib/home/v4/DryStreakRow.svelte';
import { createEndedStreaks } from '../../src/lib/home/v4/streaks';

afterEach(cleanup);
const dungeon = 'Synthetic dungeon';
const stats: HomeDungeonStreak = { dungeon, runs_ended: 46,
  runs_since_last_white: 22, avg_runs_per_white: 23, last_white_at: '100' };

it('ended_pill_wins', async () => {
  const page = render(DryStreakRow, { dungeon, stats, ended: 22 });
  expect(page.getByText('Ended at 22 runs')).toBeTruthy();
  expect(page.queryByRole('meter')).toBeNull();
  expect(page.container.textContent).not.toContain('dry streak');
  await page.rerender({ stats: null, ended: 0 });
  expect(page.getByText('Ended at 0 runs')).toBeTruthy();
});

it('missing_stats_show_dash', () => {
  const page = render(DryStreakRow, { dungeon, stats: null, ended: null });
  expect(page.getByText('Dry streak —')).toBeTruthy();
  expect(page.queryByRole('meter')).toBeNull();
});

it('no_white_seen_text', () => {
  const page = render(DryStreakRow, { dungeon, ended: null,
    stats: { ...stats, runs_since_last_white: null, avg_runs_per_white: null, last_white_at: null } });
  expect(page.getByText('No white seen in 46 runs')).toBeTruthy();
  expect(page.queryByRole('meter')).toBeNull();
});

it('bar_turns_amber_past_average', async () => {
  const page = render(DryStreakRow, { dungeon, stats, ended: null });
  const meter = () => page.getByRole('meter', { name: `${dungeon} dry streak` });
  expect(page.getByText(`${dungeon} dry streak 22`)).toBeTruthy();
  expect(meter().getAttribute('aria-valuenow')).toBe('22');
  expect(meter().getAttribute('aria-valuetext')).toBe('22 runs; average gap 23 runs');
  expect(meter().classList.contains('past-average')).toBe(false);
  expect(page.getByTitle('Average gap: 23 runs').style.left).toBe('50%');
  expect(Number.parseFloat((meter().querySelector('.fill') as HTMLElement).style.width)).toBeCloseTo(22 / 46 * 100);
  await page.rerender({ stats: { ...stats, runs_since_last_white: 23 } });
  expect(meter().classList.contains('past-average')).toBe(false);
  await page.rerender({ stats: { ...stats, runs_since_last_white: 24 } });
  expect(meter().classList.contains('past-average')).toBe(true);
  await page.rerender({ stats: { ...stats, runs_since_last_white: 100 } });
  expect(Number(meter().getAttribute('aria-valuemax'))).toBeGreaterThanOrEqual(100);
});

it('caption_says_not_a_drop_rate', () => {
  const page = render(DryStreakRow, { dungeon, stats, ended: null });
  expect(page.getByText(`Your average gap between ${dungeon} whites: 23 runs`)
    .getAttribute('title')).toBe('Observed, not a drop rate');
});

it('unknown_average_has_no_bar_and_zero_streak_remains_zero', () => {
  const page = render(DryStreakRow, { dungeon, ended: null,
    stats: { ...stats, runs_since_last_white: 0, avg_runs_per_white: null } });
  expect(page.getByText(`${dungeon} dry streak 0`)).toBeTruthy();
  expect(page.getByText(`Your average gap between ${dungeon} whites: — runs`)).toBeTruthy();
  expect(page.queryByRole('meter')).toBeNull();
  expect(page.container.textContent).not.toMatch(/NaN|undefined/);
});

it('ended_streaks_are_per_dungeon_per_instance_and_resettable', () => {
  const ended = createEndedStreaks();
  expect(ended.get(dungeon)).toBeNull();
  ended.record(dungeon, 22);
  ended.record('Synthetic other dungeon', 0);
  expect(ended.get(dungeon)).toBe(22);
  expect(ended.get('Synthetic other dungeon')).toBe(0);
  expect(createEndedStreaks().get(dungeon)).toBeNull();
  ended.reset();
  expect(ended.get(dungeon)).toBeNull();
  expect(ended.get('Synthetic other dungeon')).toBeNull();
});
