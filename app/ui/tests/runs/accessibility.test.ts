import { setDebugMode } from '../../src/lib/debug/debug';
import { cleanup, fireEvent, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import { DENSE_STORY_ROWS } from '../../src/lib/phase2/contracts';
import { advance, click, deferred, fight, flush, follow, largeStory, main, mountRuns, player, ready, region, run, saved, source, story } from './fixtures';

beforeEach(() => { setDebugMode(true);
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
});
afterEach(() => { setDebugMode(false);
  cleanup();
  vi.clearAllTimers();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it.each([false, true])('refresh_keeps_focus_and_does_not_announce_damage_ticks (reduce=%s)', async reduce => {
  vi.stubGlobal('matchMedia', vi.fn((media: string) => ({ media, matches: reduce, onchange: null,
    addListener: vi.fn(), removeListener: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn(), dispatchEvent: vi.fn() })));
  const page = mountRuns({ hash: '#/runs/run-1/fights/fight-1' }); await flush();
  const row = main().getByRole('link', { name: /Invented Archer/ }); row.focus();
  const pane = screen.getByRole('main'); pane.scrollTop = 123;
  const initialLength = page.browser.length;
  // Accessible status/alert roles, not CSS or internal component nodes.
  const announcements = () => [...main().queryAllByRole('status'), ...main().queryAllByRole('alert')].map(node => node.textContent);
  const before = announcements();
  for (let seq = 13; seq <= 20; seq++) {
    page.event({ fight_updated: fight({ players: [player({ damage: seq * 100 })] }) }, String(seq));
    page.publish({ ...page.snapshot, sequence: String(seq), fights: [fight({ players: [player({ damage: seq * 100 })] })] });
    await advance(250);
  }
  page.queries.getFight.mockResolvedValue(ready(fight({ players: [player({ damage: 2000 })] })));
  page.setStore(saved('20')); await advance();
  expect(region('Players').getByRole('row', { name: /Invented Archer/ }).textContent).toMatch(/2[,.]?000/);
  expect(document.activeElement).toBe(row); expect(pane.scrollTop).toBe(123);
  expect(announcements()).toEqual(before); expect(page.browser.length).toBe(initialLength);
  expect(page.queries.getFight).toHaveBeenCalledTimes(2);
});

it('keyboard_timeline_and_full_source_table_are_reachable', async () => {
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTimeAsync });
  const selected = player({ sources: Array.from({ length: 101 }, (_, i) => source({ item_type: 1000 + i, damage: 3000 + i })) });
  const page = mountRuns({ hash: '#/runs/run-1', queries: {
    getRun: async () => ready(largeStory(101, true)), getFight: async id => ready(fight({ id, players: [selected] })),
    getPlayerBreakdown: async () => ready(selected),
  } }); await flush();
  const whole = main().getByRole('tab', { name: 'Whole run' }); whole.focus();
  await user.keyboard('{End}'); await flush();
  expect(main().getByRole('tab', { name: 'Invented target 64' }).getAttribute('aria-selected')).toBe('true');
  await fireEvent.click(main().getByRole('tab', { name: 'Invented target 51' })); await flush();
  await follow('#/runs/run-1/fights/fight-51');
  await follow('#/runs/run-1/fights/fight-51/players/7');
  expect(region('Damage sources').getAllByRole('row').slice(1)).toHaveLength(101);
  expect(region('Damage sources').queryByRole('button', { name: 'Next Window' })).toBeNull();
  const scrollArea = region('Damage sources').getByRole('region', { name: 'Damage sources table scroll area' });
  scrollArea.focus(); await user.keyboard('{ArrowDown}'); await flush();
  expect(document.activeElement).toBe(scrollArea);
  expect(region('Damage sources').getByText('3,100', { exact: true })).toBeTruthy();
  expect(page.queries.getPlayerBreakdown).toHaveBeenCalledTimes(1);
});

it('list_rows_never_nest_interactive_controls', async () => {
  const page = mountRuns(); await flush();
  const assertRows = () => {
    const links = main().getAllByRole('link');
    expect(links.length).toBeGreaterThan(0);
    for (const link of links) {
      expect(within(link).queryByRole('button')).toBeNull();
      expect(within(link).queryByRole('checkbox')).toBeNull();
      expect(within(link).queryByRole('link')).toBeNull();
    }
  };
  assertRows(); await follow('#/runs/run-1'); assertRows();
  await fireEvent.click(main().getAllByRole('tab')[1]); await flush(); await follow('#/runs/run-1/fights/fight-1', region('Fight damage')); assertRows();
  expect(page.queries.getFight).toHaveBeenCalledTimes(1);
});

it('retry_node_stays_focused_and_rapid_activation_does_not_duplicate_queries', async () => {
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => { throw new Error('read failed'); } } }); await flush();
  const retry = main().getByRole('button', { name: /Retry/i }); retry.focus();
  const pending = deferred<HistoryReply<RunStory | null>>(); page.queries.getRun.mockImplementation(() => pending.promise);
  await fireEvent.click(retry); await fireEvent.click(retry); await fireEvent.keyDown(retry, { key: 'Enter', repeat: true }); await flush();
  expect(main().getByRole('button', { name: /Retry/i })).toBe(retry);
  expect(retry.getAttribute('aria-disabled')).toBe('true'); expect(document.activeElement).toBe(retry);
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  pending.resolve(ready(story())); await flush();
  expect(main().getByText('Invented Sanctum')).toBeTruthy();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
});

it('debug evidence uses a native focusable disclosure', async () => {
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTimeAsync });
  mountRuns({ hash: '#/runs/run-1' }); await flush();
  const trigger = main().getByText('Run evidence'); trigger.focus();
  expect(document.activeElement).toBe(trigger); expect(trigger.tagName).toBe('SUMMARY');
  await fireEvent.click(trigger); await flush();
  expect(trigger.closest('details')?.open).toBe(true);
  expect(main().getByText(/Victory observed/)).toBeTruthy();
  await fireEvent.click(trigger); await flush();
  expect(trigger.closest('details')?.open).toBe(false);
});

it('route_and_shell_disposal_release_listeners_and_timers', async () => {
  const baselineTimers = vi.getTimerCount();
  const page = mountRuns({ hash: '#/runs/run-1' }); await flush();
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledTimes(1);
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  page.event({ run_updated: run() }, '13'); page.setStore(saved('13')); await advance();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  page.unmount(); await flush();
  expect(page.browser.listeners).toBe(0); expect(vi.getTimerCount()).toBe(baselineTimers);
  const reads = vi.mocked(page.client.queryStore).mock.calls.length;
  page.event({ run_updated: run() }, '14'); page.publish({ ...page.snapshot, sequence: '14' }); await advance(10000);
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  expect(page.client.queryStore).toHaveBeenCalledTimes(reads);
});
