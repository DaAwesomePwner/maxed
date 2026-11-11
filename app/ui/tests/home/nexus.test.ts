import { itemHoverParams } from '../../src/lib/components/item-hover';
import ScopeLoot from '../../src/lib/home/v3/ScopeLoot.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { render, cleanup, fireEvent, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import userEvent from '@testing-library/user-event';
import { characterFame, deferred, homeSnapshot, homeSummary, saved } from './fixtures';
import { flushHomeRender, mountHome } from './shell-fixture';
import { localDayRange } from '../../src/lib/home/scope';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';

describe('P4 cold schema recovery protection (NON-RED)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
  });
  afterEach(() => {
    cleanup();
    vi.clearAllTimers();
    vi.unstubAllGlobals();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it.each(['manual', 'navigation'] as const)('non_red_home_schema_error_recovers_populated_history (%s)', async action => {
    const start = Date.now();
    const message = 'History schema upgrade failed; close other Maxed instances or resolve local storage access, then Retry.';
    const page = mountHome(undefined, undefined, '#/', { queryHome: async () => {
      if (Date.now() - start < 5000) throw new Error(message);
      return { status: 'ready', value: homeSummary() };
    } }); await flushHomeRender();
    expect(within(screen.getByRole('main')).getByText("Couldn't load history")).toBeTruthy();
    expect(screen.queryByRole('link', { name: /Synthetic Sanctum/ })).toBeNull();
    expect(screen.getByRole('main').querySelector('[data-kpi="runs"] .value')?.textContent).toBe('—');
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flushHomeRender();
    expect(within(screen.getByRole('main')).getByText("Couldn't load history")).toBeTruthy();
    expect(page.query).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(5000); await flushHomeRender();
    expect(page.query).toHaveBeenCalledTimes(2);
    if (action === 'manual') await fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    else {
      const primary = within(screen.getByRole('navigation', { name: 'Navigation' }));
      await fireEvent.click(primary.getByRole('link', { name: 'Runs' })); await flushHomeRender();
      await fireEvent.click(primary.getByRole('link', { name: 'Home' }));
    }
    await flushHomeRender();
    expect(screen.getByRole('link', { name: /Synthetic Sanctum/ })).toBeTruthy();
    expect(screen.getByRole('main').querySelector('[data-kpi="runs"] .value')?.textContent).toBe('1');
    expect(page.query).toHaveBeenCalledTimes(3);
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('durable Nexus story in the real Shell', () => {
  it('native Demo uses the snapshot day without scheduling a wall-clock midnight refresh', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2030, 0, 1, 23, 59, 47));
    const clock = new Date('2026-03-08T18:00:00Z');
    const snapshot = homeSnapshot({ stream_id: 'opaque-synthetic-source', area_kind: 'nexus', run: null, at: (BigInt(clock.getTime()) * 1000n).toString() });
    const midnightDelay = Number(BigInt(localDayRange(new Date()).end) / 1000n) - Date.now();

    // Positive control: the same timer observer sees live Today's midnight job.
    const live = mountHome(snapshot);
    await flushHomeRender();
    const fakeSetTimeout = globalThis.setTimeout;
    const delays: (number | undefined)[] = [];
    globalThis.setTimeout = ((...args: Parameters<typeof globalThis.setTimeout>) => {
      delays.push(args[1]);
      return fakeSetTimeout(...args);
    }) as typeof globalThis.setTimeout;
    try {
      await fireEvent.click(screen.getByRole('button', { name: /^Today$/i }));
      await flushHomeRender();
      expect(live.query).toHaveBeenLastCalledWith({ today: { range: localDayRange(new Date()) } });
      expect(delays.includes(midnightDelay)).toBe(true);
      live.unmount(); await flushHomeRender(); delays.length = 0;

      // Native Demo has no preview prop: only SetupStatus.demo selects its clock.
      const demo = mountHome(snapshot, homeSummary(), '#/', { preview: false, demo: true });
      await flushHomeRender();
      expect(demo.client.querySetup).toHaveBeenCalled();
      await fireEvent.click(screen.getByRole('button', { name: /^Today$/i }));
      await flushHomeRender();
      expect(demo.query).toHaveBeenLastCalledWith({ today: { range: localDayRange(clock) } });
      expect(screen.getByRole('main').textContent).not.toMatch(/demo day/i);
      expect(delays.includes(midnightDelay)).toBe(false);
      const count = demo.query.mock.calls.length;
      await vi.advanceTimersByTimeAsync(midnightDelay + 1);
      window.dispatchEvent(new Event('focus'));
      document.dispatchEvent(new Event('visibilitychange'));
      await flushHomeRender();
      expect(demo.query).toHaveBeenCalledTimes(count);
      expect(demo.query).toHaveBeenLastCalledWith({ today: { range: localDayRange(clock) } });
      expect(screen.getByRole('main').textContent).not.toMatch(/demo day/i);
    } finally {
      globalThis.setTimeout = fakeSetTimeout;
    }
  });

  it('offers Retry only for an error and returns focus to the scope control during loading', async () => {
    const pending = deferred<HomeSummary>();
    const queryHome = vi.fn().mockRejectedValueOnce(new Error('Synthetic initial failure'))
      .mockImplementationOnce(() => pending.promise.then(value => ({ status: 'ready' as const, value })));
    const page = mountHome(undefined, undefined, '#/', { queryHome }); await flushHomeRender();
    const retry = screen.getByRole('button', { name: 'Retry' }); retry.focus();
    await userEvent.setup().keyboard('{Enter}'); await flushHomeRender();
    expect(page.query).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Session' }));
    pending.resolve(homeSummary()); await flushHomeRender();
    expect(screen.getByRole('link', { name: /Synthetic Sanctum/i })).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
  });
  it('superseded history retries unchanged inputs without a Retry control', async () => {
    vi.useFakeTimers();
    const page = mountHome(undefined, undefined, '#/', { queryHome: vi.fn().mockResolvedValueOnce({ status: 'superseded' })
      .mockResolvedValue({ status: 'ready', value: homeSummary() }) });
    await vi.advanceTimersByTimeAsync(2000); await flushHomeRender();
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
    expect(screen.getByRole('link', { name: /Synthetic Sanctum/i })).toBeTruthy();
    expect(page.query).toHaveBeenCalledTimes(2);
    await fireEvent.click(screen.getByRole('button', { name: 'Today' })); await vi.advanceTimersByTimeAsync(2000); await flushHomeRender();
    expect(page.query).toHaveBeenCalledTimes(3);
    expect(screen.getByRole('link', { name: /Synthetic Sanctum/i })).toBeTruthy();
  });

  it('idle Nexus preserves KPI nodes and makes no history reads on four Hz snapshots', async () => {
    vi.useFakeTimers();
    const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
    const page = mountHome(snapshot); await flushHomeRender();
    const main = screen.getByRole('main');
    const cell = main.querySelector('[data-kpi="runs"]');
    expect(cell?.textContent).toContain('1');
    const loot = vi.spyOn(page.client, 'queryScopeLoot');
    let sequence = 12;
    page.client.queryStore = vi.fn(async () => saved(String(sequence), snapshot.stream_id));
    for (sequence = 13; sequence <= 32; sequence++) {
      page.transport.publishSnapshot({ ...snapshot, sequence: String(sequence), at: String(5000000 + sequence * 250000), character: { ...snapshot.character!, fame: 600 + sequence } });
      await vi.advanceTimersByTimeAsync(250); await flushHomeRender();
    }
    expect(main.querySelector('[data-kpi="runs"]')).toBe(cell);
    expect(page.query).toHaveBeenCalledOnce(); expect(loot).not.toHaveBeenCalled();
    page.query.mockResolvedValueOnce({ status: 'ready', value: homeSummary({ runs: 9999 }) });
    await fireEvent.click(screen.getByRole('button', { name: 'Today' })); await flushHomeRender();
    expect(main.querySelector('[data-kpi="runs"] .value')?.textContent).toBe('9,999');
    expect(page.query).toHaveBeenCalledTimes(2);
  });

  it('Demo Today uses the snapshot day even when the owner wall clock differs', async () => {
    vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
    const clock = new Date('2026-03-08T18:00:00Z');
    const snapshot = homeSnapshot({ stream_id: 'demo-fixture', area_kind: 'nexus', run: null, at: (BigInt(clock.getTime()) * 1000n).toString() });
    const page = mountHome(snapshot, homeSummary(), '#/', { preview: true, demo: true });
    await vi.advanceTimersByTimeAsync(1);
    await fireEvent.click(screen.getByRole('button', { name: /^Today$/i }));
    await vi.advanceTimersByTimeAsync(1);
    expect(page.query).toHaveBeenLastCalledWith({ today: { range: localDayRange(clock) } });
    expect(screen.getByRole('main').textContent).not.toMatch(/demo day/i);
  });

  it('loads once, labels the default session scope and does not read on 250 ms updates', async () => {
    const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
    const page = mountHome(snapshot, homeSummary({ fame_delta: 25, ...characterFame(25), fame_missing_baselines: 1 }));
    await waitFor(() => expect(page.query).toHaveBeenCalledOnce());
    expect(page.query).toHaveBeenCalledWith({ session: { id: snapshot.session!.id } });
    const main = within(screen.getByRole('main'));
    await main.findByText('+25');
    expect(main.queryByText(/partial.*known.character|known.character.*partial/i)).toBeNull();
    expect(main.getAllByText(/Session/).length).toBeGreaterThan(0);
    const currentFame = () => screen.getByRole('main').querySelector('[data-kpi="fame"] .value')?.textContent;
    expect(currentFame()).toBe('+25');
    for (let i = 13; i <= 32; i++) {
      page.transport.publishSnapshot({ ...snapshot, sequence: String(i), at: String(5000000 + i * 250000), character: { ...snapshot.character!, fame: 600 + i } });
    }
    await flushHomeRender();
    expect(currentFame()).toBe('+25');
    expect(page.query).toHaveBeenCalledOnce();
  });

  it('joins recent details by run ID and keeps a missing share unavailable', async () => {
    const summary = homeSummary();
    summary.recent_runs.push({ ...summary.recent_runs[0], id: 'other', dungeon: 'Other synthetic dungeon' });
    summary.recent_details = [{ ...summary.recent_details[0], run_id: 'other', local_share: 0.25 }];
    mountHome(undefined, summary);
    const first = await screen.findByRole('link', { name: /Synthetic Sanctum/i });
    const row = first.closest('[data-row-id]')!;
    expect(row.textContent).toMatch(/unavailable|—/i);
    expect(row.textContent).not.toMatch(/25\.0%|0\.0%/);
    const second = (await screen.findByRole('link', { name: /Other synthetic dungeon/i })).closest('[data-row-id]')!;
    expect(second.textContent).toMatch(/25\.0%/);
  });

  it('null top fight and best bag are unavailable, not zero DPS or no loot ever', async () => {
    mountHome(undefined, homeSummary({ top_fight: null, best_drop: null, fame_delta: null }));
    const main = within(screen.getByRole('main'));
    await main.findByText(/Top Active DPS/i);
    expect(screen.getByRole('main').textContent).not.toMatch(/0 DPS|no loot ever|personal best/i);
    expect(main.getAllByText(/\u2014/).length).toBeGreaterThan(0);
  });

  it.each([[], [{ at: '9007199254740993', character_id: 'a', fame: 10 }], [
    { at: '9007199254740993', character_id: 'a', fame: 10 },
    { at: '9007199254740994', character_id: 'a', fame: 10 },
  ]].map(fame => ({ fame })))('renders empty, single or flat fame without invalid SVG geometry ($fame)', async ({ fame }) => {
    const page = mountHome(undefined, homeSummary({ fame, fame_sampled: true }));
    await waitFor(() => expect(page.query).toHaveBeenCalledOnce());
    await screen.findByRole('link', { name: /Synthetic Sanctum/i });
    const main = screen.getByRole('main');
    expect(main.innerHTML).not.toMatch(/NaN|Infinity/);
    if (fame.length) {
      const chart = within(main).getByRole('img', { name: /Fame over time/ });
      expect(chart.querySelector('title')?.textContent).toBeTruthy();
      expect(chart.querySelector('desc')?.textContent).toBeTruthy();
      expect(chart.closest('[aria-live]')).toBeNull();
      expect(main.textContent).not.toMatch(/sampled/i);
    }
  });

  it('shows all ten stable recent rows and keeps at most 120 fame points', async () => {
    const summary = homeSummary();
    summary.recent_runs = Array.from({ length: 10 }, (_, index) => ({ ...summary.recent_runs[0], id: `run-${index}`, dungeon: `Synthetic dungeon ${index}` }));
    summary.recent_details = summary.recent_runs.map(run => ({ ...summary.recent_details[0], run_id: run.id }));
    summary.fame = Array.from({ length: 120 }, (_, index) => ({ at: String(9007199254740993n + BigInt(index)), character_id: 'a', fame: 100 + index }));
    const page = mountHome(undefined, summary);
    await screen.findByRole('link', { name: /Synthetic dungeon 0/i });
    const main = screen.getByRole('main');
    const ids = () => [...main.querySelectorAll('[data-row-id]')].map(row => row.getAttribute('data-row-id'));
    expect(ids()).toEqual(summary.recent_runs.map(run => run.id));
    expect(screen.getByRole('link', { name: /Synthetic dungeon 9/i }).getAttribute('href')).toBe('#/runs/run-9');
    expect(screen.queryByRole('button', { name: 'Next Runs' })).toBeNull();
    expect(page.query).toHaveBeenCalledOnce();
    const chart = within(main).getByRole('img', { name: /Fame over time/ });
    expect(chart.querySelectorAll('circle').length).toBeLessThanOrEqual(120);
    expect(chart.innerHTML).not.toMatch(/NaN|Infinity|<animate/);
  });

  it('Today refreshes after a backgrounded day change and cancels its timer on unmount', async () => {
    vi.useFakeTimers(); vi.setSystemTime(new Date(2026, 9, 2, 12));
    const page = mountHome();
    await vi.advanceTimersByTimeAsync(1);
    const today = screen.getByRole('button', { name: /^Today$/i });
    await fireEvent.click(today); await vi.advanceTimersByTimeAsync(1);
    await flushHomeRender();
    const first = page.query.mock.lastCall![0];
    expect(first).toEqual({ today: { range: localDayRange(new Date(2026, 9, 2, 12)) } });
    const beforeRefresh = page.query.mock.calls.length;
    vi.setSystemTime(new Date(2026, 9, 3, 12));
    window.dispatchEvent(new Event('focus'));
    document.dispatchEvent(new Event('visibilitychange'));
    await vi.advanceTimersByTimeAsync(1);
    await flushHomeRender();
    expect(page.query.mock.lastCall![0]).not.toEqual(first);
    expect(page.query.mock.lastCall![0]).toEqual({ today: { range: localDayRange(new Date(2026, 9, 3, 12)) } });
    expect(page.query).toHaveBeenCalledTimes(beforeRefresh + 1);
    const count = page.query.mock.calls.length;
    page.unmount(); await vi.advanceTimersByTimeAsync(2 * 86400000);
    expect(page.query).toHaveBeenCalledTimes(count);
  });
});

it('passes the best copy and formatted count to the scope loot hover', () => {
  const enchant = { state: 'recorded' as const, slots: 2, applied: 2, ids: [7, 8] };
  const page = render(ScopeLoot, { page: { items: [{ item_type: 101, tier: 'UT', count: 3, last_at: '2000000', best_enchant: enchant }], next: null, total_bags: 3 },
    pageNumber: 1, visuals: emptyHomeVisuals(), resolveSpriteUrl: () => null, enabled: true, next: () => {}, first: () => {} });
  const tile = page.container.querySelector('.tile')!;
  expect(tile.getAttribute('data-item-hover')).toBe('101');
  expect(tile.hasAttribute('title')).toBe(false);
  expect(tile.hasAttribute('tabindex')).toBe(false);
  expect(itemHoverParams(tile)).toEqual({ type: 101, enchant, name: 'Item 101', tier: 'UT', suffix: '×3', label: 'Best copy' });
});
