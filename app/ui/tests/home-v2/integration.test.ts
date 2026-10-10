import { fireEvent, render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import BannerHost from '../../src/lib/components/BannerHost.svelte';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { createHomeVisuals, type HomeVisuals } from '../../src/lib/home/visuals';
import { createVisualCache } from '../../src/lib/bridge/visuals';
import type { VisualPage } from '../../src/lib/bindings/VisualPage';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';
import { installNavigationInput } from '../../src/lib/navigation/history';
import { saved } from '../home/fixtures';
import { catalog, deferred, fight, flush, flushHomeRender, homeSnapshot, homeSummary, mountV2, realmSnapshot } from './fixtures';

afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });

it('home_v2_navigation_focus_and_bounds', async () => {
  const snapshot = homeSnapshot();
  snapshot.fights = Array.from({ length: 7 }, (_, index) => fight(`fight-${index}`, {
    target_type: 300 + index, ended: index === 0 ? null : '4000000',
  }));
  const page = mountV2(snapshot, undefined, { objects: snapshot.fights.map((row, index) => catalog(row.target_type!, `Synthetic encounter ${index}`)) });
  await flushHomeRender();
  const rail = screen.getByRole('complementary', { name: 'Character' });
  const main = screen.getByRole('main'), nav = within(screen.getByRole('navigation', { name: 'Navigation' }));
  const user = userEvent.setup();
  expect(within(main).getByRole('tablist', { name: 'Encounters' })).toBeTruthy();
  const tabs = within(main).getAllByRole('tab');
  const initialTabNames = tabs.map(tab => tab.textContent);
  expect(tabs).toHaveLength(8); // Seven scrollable fights and fixed Whole run.
  const whole = within(main).getByRole('tab', { name: /Whole run/i });
  whole.focus(); await user.keyboard('{Enter}');
  expect(whole.getAttribute('aria-selected')).toBe('true');
  const strip = main.querySelector<HTMLElement>('.fight-scroll')!;
  strip.scrollBy = vi.fn();
  const next = within(main).getByRole('button', { name: /more/ });
  next.focus(); await user.keyboard('{Enter}');
  expect(strip.scrollBy).toHaveBeenCalledOnce();
  expect(within(main).getAllByRole('tab').map(tab => tab.textContent)).toEqual(initialTabNames);
  expect(whole.getAttribute('aria-selected')).toBe('true');
  expect(document.activeElement).toBe(next);
  const disappearing = within(main).getAllByRole('tab')[0];
  disappearing.focus();
  page.publish(realmSnapshot()); await flushHomeRender();
  expect(disappearing.isConnected).toBe(false);
  expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Home', level: 1 }));
  expect(within(main).queryByRole('button', { name: /^Today$/ })).toBeNull();
  expect(main.querySelector('.realm-home')).toBeTruthy();
  expect(main.textContent).not.toMatch(/this observed visit/i);
  const homeLink = nav.getByRole('link', { name: 'Home' }); homeLink.focus();
  page.publish(homeSnapshot({ area_kind: 'nexus', run: null })); await flushHomeRender();
  expect(document.activeElement).toBe(homeLink);
  expect(within(main).getByRole('button', { name: /^Today$/ })).toBeTruthy();
  await fireEvent.click(screen.getByRole('link', { name: 'Settings' })); await flushHomeRender();
  expect(screen.getByRole('complementary', { name: 'Character' })).toBe(rail);
  await user.keyboard('{Alt>}{ArrowLeft}{/Alt}'); await flushHomeRender();
  expect(page.browser.adapter.hash).toBe('#/');
  await fireEvent.click(screen.getByRole('button', { name: 'Forward' })); await flushHomeRender();
  expect(page.browser.adapter.hash).toBe('#/settings');
  await fireEvent.click(screen.getByRole('button', { name: 'Back' })); await flushHomeRender();
  expect(page.browser.adapter.hash).toBe('#/');
  // Native side-button traversal has one owner, and uses the same popstate path.
  page.browser.adapter.go(1); await flushHomeRender();
  expect(page.browser.adapter.hash).toBe('#/settings');
  page.browser.adapter.go(-1); await flushHomeRender();
  expect(page.browser.adapter.hash).toBe('#/');
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledOnce();
  expect(main.querySelector('svg animate')).toBeNull();
  for (const chart of main.querySelectorAll('svg')) expect(chart.closest('[aria-live]:not([aria-live="off"])')).toBeNull();
  page.unmount(); await flushHomeRender();
  expect(page.browser.listeners).toBe(0);
});

it('home_v2_mouse_navigation_retains_one_owner', () => {
  const back = vi.fn(), forward = vi.fn();
  const native = installNavigationInput(window, { back, forward });
  window.dispatchEvent(new MouseEvent('mouseup', { button: 3, cancelable: true }));
  window.dispatchEvent(new MouseEvent('mouseup', { button: 4, cancelable: true }));
  expect(back).not.toHaveBeenCalled(); expect(forward).not.toHaveBeenCalled();
  native();
  const fallback = installNavigationInput(window, { back, forward }, 'fallback');
  expect(window.dispatchEvent(new MouseEvent('mouseup', { button: 3, cancelable: true }))).toBe(false);
  expect(window.dispatchEvent(new MouseEvent('mouseup', { button: 4, cancelable: true }))).toBe(false);
  expect(back).toHaveBeenCalledOnce(); expect(forward).toHaveBeenCalledOnce();
  fallback();
});

it('home_v2_banner_dedup_preserves_focus', async () => {
  const moment = { id: 'synthetic-banner', kind: 'white_bag' as const, title: 'Synthetic observed bag', context: 'Observed, not picked up' };
  const page = render(BannerHost, { moment: null, onDismiss: vi.fn() });
  const focus = document.createElement('button');
  focus.textContent = 'Synthetic focus target'; page.container.append(focus); focus.focus();
  await page.rerender({ moment, onDismiss: vi.fn() });
  await flushHomeRender();
  const status = screen.getByRole('status');
  expect(status.textContent).toContain(moment.title);
  expect(document.activeElement).toBe(focus);
  await page.rerender({ moment: null, onDismiss: vi.fn() });
  await page.rerender({ moment, onDismiss: vi.fn() }); await flushHomeRender();
  expect(screen.getByRole('status')).toBe(status);
  expect(status.textContent).toBe('');
  expect(document.activeElement).toBe(focus);
});

it('home_v2_2048_fights_event_overflow_and_listener_disposal', async () => {
  const snapshot = homeSnapshot();
  snapshot.fights = Array.from({ length: 2048 }, (_, index) => fight(`f-${String(index).padStart(4, '0')}`, {
    started: String(2000000 + index), last_hit: String(3000000 + index),
  }));
  let value = emptyHomeLiveWindow();
  const window = createHomeLiveWindow(next => { value = next; });
  window.snapshot(snapshot);
  window.state({ phase: 'live', historyStale: false, historyFailed: false, historyLagging: false, message: '' });
  expect(value.fights).toHaveLength(64);
  expect(value.fights.some(row => row.id === 'f-2047')).toBe(true);
  for (let index = 0; index < 300; index++) {
    window.event({ stream_id: snapshot.stream_id, sequence: String(index + 13), at: String(6000000 + index), event: {
      instance_event: { id: `e-${String(index).padStart(3, '0')}`, session_id: snapshot.session!.id, run_id: snapshot.run!.id,
        at: String(6000000 + index), kind: { type: 'player_left', object_id: index }, provisional: true },
    } });
  }
  expect(value.events).toHaveLength(256);
  expect(value.events[0].id).toBe('e-044');
  expect(value.partial).toBe(true);
  expect(value.reasons.join(' ')).toMatch(/limit|evict|overflow|bounded|truncated/i);
  window.dispose();
  expect(value.fights).toEqual([]); expect(value.events).toEqual([]);

  const releases = [vi.fn(), vi.fn(), vi.fn()];
  const page = mountV2(homeSnapshot(), undefined, { configure(client) {
    const registrations = [client.onSnapshot, client.onCapture, client.onEngineEvent] as const;
    client.onSnapshot = async receive => { const release = await registrations[0](receive); return () => { release(); releases[0](); }; };
    client.onCapture = async receive => { const release = await registrations[1](receive); return () => { release(); releases[1](); }; };
    client.onEngineEvent = async (receive, batch) => { const release = await registrations[2](receive, batch); return () => { release(); releases[2](); }; };
  } });
  await flushHomeRender();
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledOnce();
  page.unmount(); await flushHomeRender();
  for (const release of releases) expect(release).toHaveBeenCalledOnce();
});

it('home_v2_single_visual_queue_rejects_retired_fingerprint', async () => {
  const retired = deferred<VisualPage>();
  let active = 0, peak = 0, first = true;
  const fetch = vi.fn(async (types: number[]): Promise<VisualPage> => {
    active++; peak = Math.max(peak, active);
    try {
      if (first) { first = false; return await retired.promise; }
      return { client_fingerprint: 'b'.repeat(64), objects: types.map(type => catalog(type, `Fresh ${type}`)) };
    } finally { active--; }
  });
  const cache = createVisualCache(fetch);
  const stale = cache.get([1]);
  const rejection = expect(stale).rejects.toThrow(/changed/i);
  await flush(); cache.clear();
  const states: HomeVisuals[] = [];
  const release = vi.fn();
  const loader = createHomeVisuals({ getObjectVisuals: types => cache.get(types), onVisualsChanged: () => release }, state => states.push(state));
  loader.update(Array.from({ length: 260 }, (_, index) => index + 100));
  retired.resolve({ client_fingerprint: 'a'.repeat(64), objects: [catalog(1, 'Retired name')] });
  await rejection;
  for (let index = 0; index < 8; index++) await flush();
  expect(peak).toBe(1);
  expect(fetch.mock.calls.every(([types]) => types.length <= 128)).toBe(true);
  expect(states.at(-1)?.objects).toHaveLength(260);
  expect(states.at(-1)?.client_fingerprint).toBe('b'.repeat(64));
  expect(states.some(state => state.objects.some(row => row.name === 'Retired name'))).toBe(false);
  const calls = fetch.mock.calls.length;
  loader.update(Array.from({ length: 260 }, (_, index) => 359 - index)); await flush();
  expect(fetch).toHaveBeenCalledTimes(calls);
  loader.dispose(); expect(release).toHaveBeenCalledOnce();
});

it('home_v2_preview_live_namespace_discards_delayed_summary', async () => {
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
  const pending = deferred<HomeSummary>();
  const page = mountV2(snapshot, undefined, { preview: true, demo: true, configure(client) {
    client.queryHome = vi.fn().mockImplementationOnce(() => pending.promise.then(value => ({ status: 'ready' as const, value })))
      .mockResolvedValue({ status: 'ready', value: homeSummary({ runs: 222 }) });
  } });
  await flushHomeRender();
  expect(page.history[0]).toHaveBeenCalledOnce();
  const setup = await page.client.querySetup();
  page.client.querySetup = vi.fn(async () => ({ ...setup, demo: false }));
  page.client.queryStore = vi.fn(async () => saved('13', 'new-live-namespace'));
  await page.rerender({ client: page.client, historyAdapter: page.browser.adapter, preview: false });
  page.publish({ ...snapshot, stream_id: 'new-live-namespace', session: { ...snapshot.session!, id: 'new-live-session' } });
  pending.resolve(homeSummary({ runs: 999999 })); await flushHomeRender();
  expect(screen.getByRole('main').textContent).not.toContain('999,999');
  expect(page.history[0]).toHaveBeenLastCalledWith({ session: { id: 'new-live-session' } });
  expect(screen.getByRole('main').textContent).toContain('222');
  page.unmount();
});
