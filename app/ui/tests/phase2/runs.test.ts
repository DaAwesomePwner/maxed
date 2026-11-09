import { setDebugMode } from '../../src/lib/debug/debug';
// Boss-tier fixtures preserve the existing window/focus checks after trash grouping.
import { cleanup, fireEvent, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { WINDOW_SIZE, FIGHT_WINDOW_SIZE, windowRows } from '../../src/lib/runs/story';
import { retainedBag } from '../runs-a4/loot-fixtures';
import { advance, click, edit, flush, largeStory, main, mountPhase2, ready, region, run, runPage, saved, story, totals, traverse } from './fixtures';

beforeEach(() => { setDebugMode(true); vi.useFakeTimers(); });
afterEach(() => { setDebugMode(false); cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('phase2_runs_filters_page_no_n_plus_one', async () => {
  const rows = Array.from({ length: 25 }, (_, i) => run({ id: `run-${i}`, dungeon: `Invented dungeon ${i}` }));
  const page = mountPhase2({ queries: { listRuns: async () => ready(runPage(rows)) } }); await flush();
  const links = main().getAllByRole('link').filter(link => /^#\/runs\/run-\d+$/.test(link.getAttribute('href') ?? ''));
  expect(links).toHaveLength(25); expect(links.at(-1)?.getAttribute('href')).toBe('#/runs/run-24');
  expect(main().queryByRole('button', { name: 'Next Rows' })).toBeNull();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(1); expect(page.queries.listRuns.mock.calls[0][0].limit).toBe(25);
  expect(page.queries.getRun).not.toHaveBeenCalled(); expect(page.queries.getFight).not.toHaveBeenCalled();
  expect(page.queries.getPlayerBreakdown).not.toHaveBeenCalled();
  await edit('Dungeon', '  Invented dungeon 6  '); await advance(300); await click(/^Cleared /); await edit('Sort', 'oldest');
  expect(page.queries.listRuns.mock.lastCall![0]).toMatchObject({ dungeon: 'Invented dungeon 6', outcome: 'cleared', sort: 'oldest', cursor: null, limit: 25 });
  await advance(250);
  expect(page.browser.adapter.state).toMatchObject({ filters: { dungeon: 'Invented dungeon 6', outcome: 'cleared', sort: 'oldest', range: null, cursor: null, limit: 25 } });
});

it('saved run loot uses the Home ranking and timeline bag selection', async () => {
  const drops = [retainedBag({ id: 'best', bag_rank: 6 }), retainedBag({ id: 'other', at: '7000000', bag_rank: 1, contents_known: false, contents: [], items: [] })];
  const page = mountPhase2({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ drops })) } }); await flush();
  const loot = main().getByRole('region', { name: 'Loot this run' });
  expect([...loot.querySelectorAll<HTMLElement>('[data-drop-id]')].map(row => row.dataset.dropId)).toEqual(['best', 'other']);
  const marker = screen.getByRole('region', { name: 'Timeline' }).querySelector<HTMLElement>('[data-marker-id="drop:other"]')!;
  await fireEvent.click(marker); await flush();
  expect(document.activeElement?.getAttribute('data-drop-id')).toBe('other');
  expect(loot.querySelector('[data-drop-id="other"]')?.classList.contains('selected')).toBe(true);
  expect(page.queries.getFight).not.toHaveBeenCalled();
});
it.each([2, 3])('saved clear keeps death warning and qualifies comparison (%s earlier clears)', async previous => {
  const value = story({ run: run({ died_at: '10000000', evidence: [
    { at: '9000000', evidence: 'victory' }, { at: '10000000', evidence: 'death_packet' },
  ] }), comparison: { previous_clears: previous, mean_micros: 10000000, change_fraction: -.2 } });
  mountPhase2({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  expect(main().getByText('Clear and death')).toBeTruthy();
  expect(main().getByText('Clear time').parentElement?.textContent).toContain('0:08');
  expect(main().queryByText(/20% faster/)).toBe(previous === 3 ? main().getByText(/20% faster/) : null);
});
it.each(['omitted', 'null', 'known'] as const)('saved run keeps totals distinct from summaries (%s)', async mode => {
  const value = story({ totals: mode === 'known' ? totals() : null });
  if (mode === 'omitted') delete value.totals;
  const page = mountPhase2({ hash: '#/runs/run-1', queries: { getRun: async () => ready(value) } }); await flush();
  const kpi = (key: string) => screen.getByRole('main').querySelector(`[data-kpi="${key}"] .value`)?.textContent;
  expect(kpi('damage')).toBe(mode === 'known' ? '987,654' : '—');
  expect(kpi('fame')).toBe(mode === 'known' ? '-9' : '—');
  expect(kpi('share')).toBe(mode === 'known' ? '12.5%' : '—');
  expect(page.queries.getFight).not.toHaveBeenCalled();
});
it('preserves public helper defaults and known zero totals', async () => {
  expect(WINDOW_SIZE).toBe(50); expect(FIGHT_WINDOW_SIZE).toBe(25);
  expect(windowRows(Array.from({ length: 60 }, (_, i) => i), 0)).toHaveLength(50);
  const value = totals({ fights: 0, kills: 0, total_damage: 0, fame_delta: 0, local: null, first_hit: null, last_hit: null });
  mountPhase2({ hash: '#/runs/run-1', queries: { getRun: async () => ready(story({ totals: value })) } }); await flush();
  const pane = screen.getByRole('main');
  expect(pane.querySelector('[data-kpi="damage"] .value')?.textContent).toBe('0');
  expect(pane.querySelector('[data-kpi="share"] .value')?.textContent).toBe('—');
  expect(pane.querySelector('[data-kpi="fame"] .value')?.textContent).toBe('0');
  expect(screen.queryByText(/NaN|Infinity/)).toBeNull();
});
