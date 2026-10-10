import { render, screen, fireEvent, within } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';
import NexusHome from '../../src/lib/home/NexusHome.svelte';
import FameSparkline from '../../src/lib/home/FameSparkline.svelte';
import ScopeLoot from '../../src/lib/home/v3/ScopeLoot.svelte';
import { setDebugMode } from '../../src/lib/debug/debug';
import { homeSnapshot, homeSummary } from '../home/fixtures';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { routeHref } from '../../src/lib/navigation/routes';
import { lootCount } from '../../src/lib/home/nexus';
import type { HomeHistoryState } from '../../src/lib/home/summary';
import type { ScopeLootPage } from '../../src/lib/bindings/ScopeLootPage';

afterEach(() => { setDebugMode(false); vi.unstubAllGlobals(); });
const summary = () => homeSummary({ runs: 14, clears: 12, deaths: 1, fame_delta: 3420,
  fame: [{ at: '1000000', character_id: 'a', fame: 1250 }, { at: '2000000', character_id: 'a', fame: 4670 }],
  dungeon_stats: [{ dungeon: 'The Shatters', runs: 5, clears: 4, deaths: 1, observed_clears: 4,
    mean_clear_micros: 768000000, best_clear_micros: 719000000, last_started: '1000000', all_time_best_clear_micros: 719000000 }] });
const ready = (): HomeHistoryState => ({ phase: 'ready', summary: summary(), retryAvailable: true, message: '' });
const props = () => ({ snapshot: homeSnapshot({ area_kind: 'nexus', map: 'Nexus', run: null }), choice: 'session' as const, history: ready(), retry: vi.fn() });
it('shows the divided KPI copy, target link, PB and a single whole-row run destination', () => {
  const input = props();
  const fight = input.history.summary!.top_fight!;
  const page = render(NexusHome, input);
  expect(screen.getByRole('heading', { name: 'Nexus' })).toBeTruthy();
  expect(screen.getByText(/Session since/)).toBeTruthy();
  expect(screen.getByText('12 cleared')).toBeTruthy();
  expect(screen.getByText('in 14 runs')).toBeTruthy();
  expect(screen.getByText('1,250 → 4,670')).toBeTruthy();
  expect(screen.getByRole('link', { name: /Open fight/ }).getAttribute('href')).toBe(routeHref({ kind: 'fight', id: fight.id, runId: fight.run_id ?? undefined }));
  expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  expect(screen.getByText('PB')).toBeTruthy();
  const row = page.container.querySelector('[data-row-id]')!;
  expect(row.tagName).toBe('A');
  expect(row.getAttribute('href')).toBe('#/runs/demo-run');
  expect(row.querySelector('details')).toBeNull();
  expect(row.querySelectorAll('a')).toHaveLength(0);
});
it('Nexus uses the shared 600ms best-bag glow exactly once and disables it for reduced motion', async () => {
  const animate = vi.fn(() => ({ cancel: vi.fn() }));
  vi.stubGlobal('matchMedia', () => ({ matches: false }));
  vi.stubGlobal('getComputedStyle', () => ({ color: '#a77bff' }));
  const input = props();
  const drop = input.history.summary!.best_drop!;
  const type = drop.contents[0]?.item_type ?? drop.items[0];
  const visuals = { ...emptyHomeVisuals(), objects: [
    { object_type: type, name: 'Featured UT', item_tier: 'UT', sprite: null, bag: null },
  ] };
  const page = render(NexusHome, { ...input, visuals });
  Object.defineProperty(page.container.querySelector('.best-card')!, 'animate', { value: animate, configurable: true });
  const next = { ...input.history, summary: { ...input.history.summary!, best_drop: { ...drop, id: 'new-best' } } };
  await page.rerender({ history: next });
  expect(animate).toHaveBeenCalledExactlyOnceWith([
    { boxShadow: '0 0 22px 4px #a77bff' }, { boxShadow: '0 0 0 0 transparent' },
  ], { duration: 600, iterations: 1 });
  await page.rerender({ history: { ...next, summary: { ...next.summary, runs: 9999 } } });
  expect(animate).toHaveBeenCalledOnce();
  vi.stubGlobal('matchMedia', () => ({ matches: true }));
  await page.rerender({ history: { ...next, summary: { ...next.summary, best_drop: { ...drop, id: 'another-best' } } } });
  expect(animate).toHaveBeenCalledOnce();
});
it('does not glow the best bag when saved history first arrives or the period changes', async () => {
  const animate = vi.fn(() => ({ cancel: vi.fn() }));
  vi.stubGlobal('matchMedia', () => ({ matches: false }));
  vi.stubGlobal('getComputedStyle', () => ({ color: '#a77bff' }));
  Object.defineProperty(HTMLElement.prototype, 'animate', { value: animate, configurable: true });
  try {
    const input = props();
    const loaded = input.history;
    const drop = loaded.summary!.best_drop!;
    // A UT headline item: only UT, ST and white bags glow at all.
    const visuals = { ...emptyHomeVisuals(), objects: [
      { object_type: drop.contents[0]?.item_type ?? drop.items[0], name: 'Featured UT', item_tier: 'UT', sprite: null, bag: null },
    ] };
    const page = render(NexusHome, { ...input, visuals, history: { phase: 'loading', summary: null, retryAvailable: false, message: '' } });
    await page.rerender({ history: loaded });
    expect(page.container.querySelector('.best-card')).toBeTruthy();
    await page.rerender({ choice: 'today', history: { ...loaded, summary: { ...loaded.summary!, best_drop: { ...drop, id: 'today-best' } } } });
    expect(animate).not.toHaveBeenCalled();
    await page.rerender({ history: { ...loaded, summary: { ...loaded.summary!, best_drop: { ...drop, id: 'new-today-best' } } } });
    expect(animate).toHaveBeenCalledOnce();
  } finally { Reflect.deleteProperty(HTMLElement.prototype, 'animate'); }
});
it.each([719000000, 700000000, null, undefined])('shows PB only for matching known all-time best %s', allTime => {
  const input = props();
  input.history.summary!.dungeon_stats![0].all_time_best_clear_micros = allTime;
  render(NexusHome, input);
  expect(Boolean(screen.queryByText('PB'))).toBe(allTime === 719000000);
});
it.each(['loading', 'busy', 'superseded', 'error'] as const)('retains ready summary and best card while loot is %s', phase => {
  const retryLoot = vi.fn();
  const input = props();
  const page = render(NexusHome, { ...input, onRetryLoot: retryLoot,
    lootStatus: { phase, value: null, message: 'Synthetic loot status', retryAvailable: phase === 'error' || phase === 'busy' } });
  expect(page.container.querySelector('[data-kpi="runs"] .value')?.textContent).toBe('14');
  expect(page.container.querySelector('.best-card')).toBeTruthy();
  expect(screen.getByText('PB')).toBeTruthy();
  expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  expect(Boolean(screen.queryByRole('button', { name: 'Retry Loot' }))).toBe(phase === 'error' || phase === 'busy');
});
it('keeps unknown scope, fame, art and fight values explicit and never invents a PB', async () => {
  const input = props();
  const data = homeSummary({ top_fight: null, best_drop: null, fame_delta: null, fame: [], dungeon_stats: null, recent_runs: [], recent_details: [] });
  const page = render(NexusHome, { ...input, snapshot: null, history: { ...ready(), summary: data }, loot: { items: [], next: null, total_bags: 0 } });
  expect(screen.queryByRole('link', { name: /Open fight/ })).toBeNull();
  expect(screen.getByText('No Bags')).toBeTruthy();
  expect(screen.getByText('No Loot')).toBeTruthy();
  expect(screen.queryByText('PB')).toBeNull();
  expect(page.container.innerHTML).not.toMatch(/NaN|undefined|Infinity/);
  const error = { ...ready(), phase: 'error' as const, summary: null, message: 'History details' };
  await page.rerender({ ...input, history: error });
  await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
  expect(input.retry).toHaveBeenCalledOnce();
});
it('debug toggle restores session, fame, source, coverage, table and loot caveats', async () => {
  const input = props();
  const data = input.history.summary!;
  data.issues = ['stream_gap']; data.fame_sampled = true; data.fame_missing_baselines = 2;
  data.best_drop = { ...data.best_drop!, source_estimated: true, source_reason: 'source-not-observed', issues: ['missing_stats'] };
  input.history.message = 'Synthetic history reason';
  const page = render(NexusHome, input);
  expect(page.container.textContent).not.toMatch(/Partial|sampled|baseline|Source estimated|not pickup|30 minutes|Server cap/);
  setDebugMode(true); await tick();
  for (const text of ['30 minutes', 'Synthetic history reason', 'missing baselines', 'sampled fame', 'Source estimated',
    'Some traffic was missed', 'Observed contents, not pickup or ownership.', 'Server cap: 20 dungeons', 'Up to 10 recent runs']) {
    expect(page.container.textContent).toContain(text);
  }
  setDebugMode(false); await tick();
  expect(page.container.querySelector('[data-debug-note]')).toBeNull();
});
it('renders one area per known character segment, with no line across uncertain gaps', async () => {
  const points = [{ at: '1000000', character_id: 'a', fame: 1 }, { at: '2000000', character_id: 'a', fame: 2 },
    { at: '3000000', character_id: 'b', fame: 3 }, { at: '4000000', character_id: 'b', fame: 4 }];
  const page = render(FameSparkline, { points, period: 'Session' });
  expect(page.container.querySelectorAll('polygon')).toHaveLength(2);
  expect(page.container.querySelectorAll('polyline')).toHaveLength(2);
  expect(page.container.querySelectorAll('.axis span')).toHaveLength(2);
  await page.rerender({ points, period: 'Session', gaps: true });
  expect(page.container.querySelectorAll('polyline, polygon')).toHaveLength(0);
  expect(page.container.querySelectorAll('circle')).toHaveLength(4);
  expect(page.container.textContent).not.toContain('Gap locations are unavailable');
  setDebugMode(true); await tick();
  expect(screen.getAllByText('Gap locations are unavailable; all fame lines and fills are hidden for this scope.')).toHaveLength(1);
  expect(page.container.innerHTML).not.toMatch(/NaN|Infinity/);
});
it('pages a large aggregate scope with a bounded tile count, pips and fallback labels', async () => {
  const page: ScopeLootPage = { total_bags: 5000, next: 'more', items: Array.from({ length: 32 }, (_, i) => ({
    item_type: i, tier: i < 2 ? (i === 0 ? 'UT' : 'ST') : null, count: 1234,
    last_at: '100', best_enchant: i === 0 ? { state: 'recorded', applied: 4, slots: 4, ids: [1, 2, 3, 4] } : null,
  })) };
  const next = vi.fn();
  const view = render(ScopeLoot, { page, pageNumber: 1, visuals: emptyHomeVisuals(), resolveSpriteUrl: () => null, enabled: true, next, first: vi.fn() });
  expect(view.container.querySelectorAll('[data-loot-type]')).toHaveLength(32);
  expect(view.container.querySelectorAll('.big')).toHaveLength(2);
  expect(screen.getByRole('img', { name: 'Item 0' })).toBeTruthy();
  expect(screen.getAllByText('×1.2k')).toHaveLength(32);
  expect(view.container.querySelector('[data-loot-type="0"]')?.getAttribute('title')).toBe('Item 0 · 1,234');
  expect(view.container.querySelector('[data-pip="4"]')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'Next Loot' }));
  expect(next).toHaveBeenCalledOnce();
  await view.rerender({ page: { ...page, items: page.items.slice(0, 1), next: null }, pageNumber: 2,
    visuals: emptyHomeVisuals(), resolveSpriteUrl: () => null, enabled: true, next, first: vi.fn() });
  expect(view.container.querySelectorAll('[data-loot-type]')).toHaveLength(1);
});

it('shows Demo Day only in debug without changing the selected Today scope', async () => {
  const page = render(NexusHome, { ...props(), choice: 'today', syntheticClock: true,
    scope: { today: { range: { start: '1000000', end: '86401000000' } } } });
  expect(page.container.textContent).not.toContain('Demo Day');
  setDebugMode(true); await tick();
  expect(page.container.textContent).toContain('Demo Day');
});

it.each([
  [999, '×999'], [1000, '×1k'], [1234, '×1.2k'], [9999, '×10k'], [12345, '×12k'],
] as const)('formats loot count %s as %s', (count, expected) => {
  expect(lootCount(count)).toBe(expected);
});

it.each([
  ['realm', 'Realm'], ['other_hub', 'Realm'], ['unknown', 'Live'], ['nexus', 'Nexus'],
] as const)('uses the %s title fallback without a map name', (area_kind, title) => {
  render(NexusHome, { ...props(), snapshot: homeSnapshot({ area_kind, map: null, run: null }) });
  expect(screen.getByRole('heading', { name: title })).toBeTruthy();
});
