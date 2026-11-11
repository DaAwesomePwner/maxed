import { setDebugMode } from '../../src/lib/debug/debug';
import { fireEvent, screen, waitFor } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRouter } from '../../src/lib/navigation/history';
import { parseRoute, routeHref } from '../../src/lib/navigation/routes';
import { emptyListState } from '../../src/lib/navigation/state';
import { fakeHistory } from '../navigation/fake-history';
import { deferred, homeSnapshot, homeSummary, saved } from './fixtures';
import { flushHomeRender, mountHome } from './shell-fixture';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';

afterEach(() => {
  setDebugMode(false);
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('Home death destination and restoration', () => {
  it.each(['rows arrive', 'bounded timeout'] as const)('Back waits for stale Home rows before restoration (%s)', async completion => {
    vi.useFakeTimers();
    const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
    const summary = homeSummary();
    const page = mountHome(snapshot, summary);
    await flushHomeRender();
    const row = screen.getByRole('link', { name: /Synthetic Sanctum/i });
    const pane = screen.getByRole('main');
    Object.defineProperties(pane, { scrollHeight: { value: 1200, configurable: true }, clientHeight: { value: 500, configurable: true } });
    row.focus(); pane.scrollTop = 300; await fireEvent.scroll(pane);
    const focus = vi.spyOn(HTMLElement.prototype, 'focus');
    await fireEvent.click(row); await flushHomeRender();
    const runHeading = screen.getByRole('heading', { name: 'Run', level: 1 });
    expect(document.activeElement).toBe(runHeading);
    expect(focus.mock.instances).toContain(runHeading); // Positive control for the focus observer.
    expect(pane.scrollTop).toBe(0);

    const pending = deferred<HomeSummary>();
    page.query.mockImplementation(() => pending.promise.then(value => ({ status: 'ready' as const, value })));
    page.client.queryStore = vi.fn(async () => saved('12', snapshot.stream_id));
    page.transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: '13', at: snapshot.at,
      event: { run_updated: { ...summary.recent_runs[0], ended: snapshot.at } } });
    await flushHomeRender();
    const back = screen.getByRole('button', { name: /^Back$/i });
    back.focus(); focus.mockClear();
    await fireEvent.click(back); await flushHomeRender();
    const homeHeading = screen.getByRole('heading', { name: 'Home', level: 1 });
    expect(page.browser.adapter.hash).toBe('#/');
    expect(page.browser.adapter.state).toMatchObject({ focusedRowId: 'demo-run', scrollTop: 300 });
    expect(pane.querySelector('.history-status')).toBeNull();
    expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toContain('Refreshing history');
    const expectWaiting = () => {
      expect(screen.queryByRole('link', { name: /Synthetic Sanctum/i })).toBeNull();
      expect(pane.querySelector('.restoration-status')?.textContent).toBe('');
      expect(focus.mock.instances).not.toContain(homeHeading);
      expect(pane.scrollTop).toBe(0);
      expect(page.query).toHaveBeenCalledOnce();
    };
    expectWaiting();
    if (completion === 'rows arrive') {
      await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
      expectWaiting();
      page.client.queryStore = vi.fn(async () => saved('13', snapshot.stream_id));
      await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
      expect(page.query).toHaveBeenCalledTimes(2);
      expect(pane.querySelector('.restoration-status')?.textContent).toBe('');
      expect(focus.mock.instances).not.toContain(homeHeading);
      pending.resolve(summary); await flushHomeRender();
      const restored = screen.getByRole('link', { name: /Synthetic Sanctum/i });
      expect(document.activeElement).toBe(restored);
      expect(pane.scrollTop).toBe(300);
      expect(pane.querySelector('.restoration-status')?.textContent).toBe('');
    } else {
      // A's current bounded rows-ready contract is 5 seconds, not an endless
      // wait or a deadline restarted by repeated stale updates.
      for (let index = 0; index < 19; index++) {
        page.transport.publishSnapshot({ ...snapshot, sequence: String(14 + index) });
        await vi.advanceTimersByTimeAsync(250); await flushHomeRender();
      }
      await vi.advanceTimersByTimeAsync(249); await flushHomeRender();
      expectWaiting();
      await vi.advanceTimersByTimeAsync(1); await flushHomeRender();
      expect(pane.querySelector('.restoration-status')?.textContent).toBe('Page heading');
      expect(document.activeElement).toBe(homeHeading);
      expect(focus.mock.instances).toContain(homeHeading);
    }
  });

  it('round-trips encoded run IDs with a durable death suffix and rejects other points', () => {
    const route = { kind: 'run' as const, id: 'invented / run?#', point: 'death' as const };
    expect(parseRoute(routeHref(route))).toEqual(route);
    expect(routeHref(route)).toBe('#/runs/invented%20%2F%20run%3F%23/death');
    expect(parseRoute('#/runs/a/invented').kind).toBe('not-found');
    const router = createRouter(fakeHistory(routeHref(route)).adapter);
    expect(router.state.entry.anchor).toBe('death'); router.dispose();
  });
  it('restores the separate Home scope, scroll and row without changing RunFilter', () => {
    const browser = fakeHistory(); const router = createRouter(browser.adapter);
    expect(emptyListState().homeScope).toBeNull();
    const filters = structuredClone(router.state.entry.filters);
    router.save({ homeScope: 'today', scrollTop: 300, focusedRowId: 'demo-run' });
    router.navigate({ kind: 'run', id: 'demo-run', point: 'death' });
    expect(router.state.entry.anchor).toBe('death');
    router.back();
    expect(router.state.entry).toMatchObject({ homeScope: 'today', scrollTop: 300, focusedRowId: 'demo-run', filters });
    router.forward(); expect(router.state.entry.anchor).toBe('death');
    router.navigate({ kind: 'run', id: 'demo-run', point: 'death' }, { anchor: 'explicit' });
    expect(router.state.entry.anchor).toBe('explicit'); router.dispose();
  });
  it.each([undefined, 'invalid'])('normalizes legacy saved homeScope %s after cache eviction', homeScope => {
    const browser = fakeHistory(); const router = createRouter(browser.adapter);
    const legacy = { ...browser.adapter.state as object, homeScope };
    browser.adapter.replace(legacy, '#/');
    for (let i = 0; i < 55; i++) router.navigate({ kind: 'settings' });
    for (let i = 0; i < 55; i++) router.back();
    expect(router.state.entry.homeScope).toBeNull(); router.dispose();
  });
  it('real Shell Home death story Back Forward uses sibling links and restores focus', async () => {
    setDebugMode(true);
    const summary = homeSummary();
    summary.recent_runs[0].died_at = '5000000'; summary.deaths = 1;
    const page = mountHome(undefined, summary);
    // The story route reads saved history. Supply the synthetic death record
    // while checking Home navigation.
    page.client.getRun = vi.fn<typeof page.client.getRun>(async () => ({ status: 'ready', value: {
      run: summary.recent_runs[0], character: homeSnapshot().character, fights: [], drops: [], comparison: null,
      timeline: [{ id: 'saved-death', kind: 'death', at: '5000000', related_id: 'demo-run', explanation: 'Observed death' }],
    } }));
    const death = await screen.findByRole('link', { name: /death/i });
    expect(death.getAttribute('href')).toBe('#/runs/demo-run/death');
    expect(death.parentElement?.closest('a, button')).toBeNull();
    const today = screen.getByRole('button', { name: /^Today$/i });
    await fireEvent.click(today);
    await waitFor(() => expect(page.query.mock.lastCall![0]).toHaveProperty('today'));
    await page.query.mock.results.at(-1)!.value;
    const todayDeath = await screen.findByRole('link', { name: /death/i });
    const pane = screen.getByRole('main');
    Object.defineProperties(pane, { scrollHeight: { value: 1200, configurable: true }, clientHeight: { value: 500, configurable: true } });
    pane.scrollTop = 300; await fireEvent.scroll(pane);
    todayDeath.focus(); await fireEvent.click(todayDeath);
    await waitFor(() => expect(document.activeElement?.getAttribute('data-anchor')).toBe('death'));
    const destination = document.activeElement!;
    expect(destination.getAttribute('tabindex')).toBe('-1');
    expect(destination.textContent).not.toMatch(/killed by|killer/i);
    await fireEvent.click(screen.getByRole('button', { name: /^Back$/i }));
    await waitFor(() => expect(page.browser.adapter.hash).toBe('#/'));
    await waitFor(() => expect(document.activeElement?.closest('[data-row-id]')?.getAttribute('data-row-id')).toBe('demo-run'));
    expect(screen.getByRole('main').scrollTop).toBe(300);
    expect(page.browser.adapter.state).toMatchObject({ homeScope: 'today' });
    await fireEvent.click(screen.getByRole('button', { name: /^Forward$/i }));
    await waitFor(() => expect(document.activeElement?.getAttribute('data-anchor')).toBe('death'));
    expect(page.client.getRun).toHaveBeenCalledTimes(2);
    expect(page.client.getRun).toHaveBeenLastCalledWith('demo-run');
  });
});
