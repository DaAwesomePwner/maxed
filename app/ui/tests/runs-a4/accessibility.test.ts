import { itemHoverParams } from '../../src/lib/components/item-hover';
import { setDebugMode } from '../../src/lib/debug/debug';
// Boss-tier fixtures preserve the existing window/focus checks after trash grouping.
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunPage } from '../../src/lib/bindings/RunPage';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import { DENSE_STORY_ROWS } from '../../src/lib/phase2/contracts';
import SourceBreakdown from '../../src/lib/runs/SourceBreakdown.svelte';
import { advance, click, deferred, flush, main, mountRuns, ready, region, run, runPage, saved, source, story, player } from '../runs/fixtures';

beforeEach(() => { setDebugMode(true); vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { setDebugMode(false); cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('keeps Next mounted and focused while a run page is pending and ignores repeated activation', async () => {
  const cursor = { sort_value: '1000000', id: 'run-1' };
  const page = mountRuns({ queries: { listRuns: async () => ready(runPage([run()], cursor)) } }); await flush();
  const pending = deferred<HistoryReply<RunPage>>(); page.queries.listRuns.mockReturnValue(pending.promise);
  const next = main().getByRole('button', { name: 'Next' }); next.focus();
  await fireEvent.click(next); await fireEvent.click(next); await flush();
  expect(main().getByRole('button', { name: 'Next' })).toBe(next);
  expect(next.getAttribute('aria-disabled')).toBe('true'); expect(document.activeElement).toBe(next);
  expect(page.queries.listRuns).toHaveBeenCalledTimes(2);
  pending.resolve(ready(runPage([run({ id: 'page-two', dungeon: 'Accepted page two' })]))); await flush();
  expect(main().getByRole('link', { name: /Accepted page two/ })).toBeTruthy();
  expect(document.activeElement).toBe(next); expect(next.getAttribute('aria-disabled')).toBe('true');
  expect(page.queries.listRuns).toHaveBeenCalledTimes(2);
});

it('announces one error and recovery while retaining Retry through pending and later silent refresh', async () => {
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => { throw new Error('synthetic failure'); } } }); await flush();
  const error = main().getByText('History Error');
  const retry = main().getByRole('button', { name: 'Retry' }); retry.focus();
  const pending = deferred<HistoryReply<RunStory | null>>(); page.queries.getRun.mockReturnValue(pending.promise);
  await fireEvent.click(retry); await fireEvent.click(retry); await flush();
  expect(error.textContent).toContain('History Error'); expect(document.activeElement).toBe(retry);
  expect(retry.getAttribute('aria-disabled')).toBe('true');
  pending.resolve(ready(story())); await flush();
  expect(main().getByText('History Ready')).toBe(error);
  expect(main().getByRole('button', { name: 'Retry' })).toBe(retry);
  expect(document.activeElement).toBe(retry); expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  page.queries.getRun.mockResolvedValue(ready(story({ run: run({ dungeon: 'Accepted revision' }) })));
  page.event({ run_updated: run() }, '13'); page.setStore(saved('13')); await advance();
  expect(main().getByText('Accepted revision')).toBeTruthy(); expect(error.textContent).toBe('History Ready');
  expect(document.activeElement).toBe(retry); expect(page.queries.getRun).toHaveBeenCalledTimes(3);
});

it.each([false, true])('story revision preserves keyed focus, scroll, history and announcements (reduce=%s)', async reduce => {
  vi.stubGlobal('matchMedia', vi.fn((media: string) => ({ media, matches: reduce, onchange: null,
    addEventListener: vi.fn(), removeEventListener: vi.fn(), addListener: vi.fn(), removeListener: vi.fn(), dispatchEvent: vi.fn() })));
  const timers = vi.getTimerCount();
  const page = mountRuns({ hash: '#/runs/run-1' }); await flush();
  const link = main().getAllByRole('tab')[1]; await fireEvent.click(link); await flush(); link.focus();
  const pane = screen.getByRole('main'); pane.scrollTop = 97;
  const length = page.browser.length;
  const announcements = main().getAllByRole('status').map(node => node.textContent);
  const changed = story(); changed.boss_fights![0].total_damage = 8888;
  page.queries.getRun.mockResolvedValue(ready(changed));
  page.event({ run_updated: changed.run }, '13'); page.setStore(saved('13')); await advance();
  expect(main().getAllByRole('tab')[1]).toBe(link); expect(main().getByText(/8[,.]?888 damage/)).toBeTruthy();
  expect(document.activeElement).toBe(link); expect(pane.scrollTop).toBe(97); expect(page.browser.length).toBe(length);
  expect(main().getAllByRole('status').map(node => node.textContent)).toEqual(announcements);
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  page.unmount(); await flush(); expect(page.browser.listeners).toBe(0); expect(vi.getTimerCount()).toBe(timers);
  page.event({ run_updated: run() }, '14'); await advance(10000);
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
});

it('source tables retain all rows, a keyboard scroll target and shrink with saved data', async () => {
  const sources = Array.from({ length: 101 }, (_, i) => source({ item_type: 1000 + i }));
  const label = 'DemoPlayer sources';
  const page = render(SourceBreakdown, { player: player({ sources }), label }); await flush();
  const scope = within(screen.getByRole('region', { name: label }));
  expect(scope.getByRole('heading', { name: label })).toBeTruthy();
  expect(scope.getAllByRole('row').slice(1)).toHaveLength(101);
  expect(scope.getByText('Item 1000')).toBeTruthy(); expect(scope.getByText('Item 1100')).toBeTruthy();
  const scroll = scope.getByRole('region', { name: `${label} table scroll area` }); scroll.focus();
  expect(document.activeElement).toBe(scroll);
  await page.rerender({ player: player({ sources: sources.slice(0, 2) }), label }); await flush();
  expect(scope.getAllByRole('row').slice(1)).toHaveLength(2); expect(document.activeElement).toBe(scroll);
  await page.rerender({ player: player({ sources }), label }); await flush();
  expect(scope.getAllByRole('row').slice(1)).toHaveLength(101);
  expect(scope.getByText('Item 1100')).toBeTruthy();
});

it('death navigation selects a relevant boss and focuses the death anchor', async () => {
  const value = story(); value.run.died_at = '10500000';
  mountRuns({ hash: '#/runs/run-1/death', queries: { getRun: async () => ready(value) } }); await flush();
  expect(main().getAllByRole('tab').filter(tab => tab.getAttribute('aria-selected') === 'true')).toHaveLength(1);
  expect(main().getAllByRole('tab')[1].getAttribute('aria-selected')).toBe('true');
  expect(document.activeElement?.getAttribute('data-anchor')).toBe('death');
});

it('leaves sources without item types without an item hover or added tab stop', () => {
  const page = render(SourceBreakdown, { player: player({ sources: [source(), source({ kind: 'unknown', item_type: null }), source({ kind: 'ability', item_type: null })] }) });
  const cells = [...page.container.querySelectorAll('.identity')];
  expect(cells[0].getAttribute('data-item-hover')).toBe('1000');
  expect(itemHoverParams(cells[0])?.type).toBe(1000);
  for (const cell of cells) expect(cell.hasAttribute('tabindex')).toBe(false);
  for (const cell of cells.slice(1)) {
    expect(cell.hasAttribute('data-item-hover')).toBe(false);
    expect(itemHoverParams(cell)).toBeUndefined();
  }
});
