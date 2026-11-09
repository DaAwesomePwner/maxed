import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { screen, within } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { homeSnapshot, saved } from './fixtures';
import { flushHomeRender, mountHome } from './shell-fixture';

afterEach(() => vi.unstubAllGlobals());
describe('automatic Home transitions', () => {
  it.each([false, true])('automatic_switch_keeps_focus_and_reduced_motion (reduce=%s)', async reduce => {
    vi.stubGlobal('matchMedia', vi.fn((media: string) => ({ media, matches: reduce, onchange: null,
      addListener: vi.fn(), removeListener: vi.fn(), addEventListener: vi.fn(), removeEventListener: vi.fn(), dispatchEvent: vi.fn() })));
    const snapshot = homeSnapshot(); const page = mountHome(snapshot);
    await screen.findByRole('rowheader', { name: /\bYou$/ });
    // Finish the one-shot startup paint boundary before observing automatic
    // transitions. It reports readiness and does not animate the page.
    await new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    const primary = screen.getByRole('navigation', { name: 'Navigation' });
    const focus = within(primary).getByRole('link', { name: 'Home' }); focus.focus();
    const push = vi.spyOn(page.browser.adapter, 'push');
    const raf = vi.spyOn(window, 'requestAnimationFrame');
    const rail = screen.getByRole('complementary', { name: 'Character' });
    expect(page.query).not.toHaveBeenCalled();
    const announcements = () => [...screen.getByRole('main').querySelectorAll('[aria-live]:not([aria-live="off"])')].map(node => node.textContent);
    const beforeAnnouncements = announcements();
    const personalDps = () => within(screen.getByRole('main')).getByRole('rowheader', { name: /\bYou$/ })
      .closest('tr')!.querySelector('td:nth-child(5)')!.textContent;
    expect(personalDps()).toBe('150');
    for (let sequence = 13; sequence < 25; sequence++) {
      const updated = structuredClone(snapshot);
      updated.fights[0].players[0].dps = 150 + sequence;
      page.transport.publishSnapshot({ ...updated, sequence: String(sequence), at: String(5000000 + sequence * 250000) });
    }
    await flushHomeRender();
    expect(personalDps()).toBe('174'); // Positive control: this publish path redraws numeric state.
    expect(announcements()).toEqual(beforeAnnouncements);
    page.client.queryStore = vi.fn(async () => saved('25'));
    page.transport.publishSnapshot({ ...snapshot, sequence: '25', area_kind: 'nexus', run: null });
    await flushHomeRender();
    await screen.findByRole('link', { name: /Synthetic Sanctum/i });
    expect(document.activeElement).toBe(focus);
    expect(page.browser.adapter.hash).toBe('#/'); expect(push).not.toHaveBeenCalled();
    expect(screen.getByRole('complementary', { name: 'Character' })).toBe(rail);
    for (const listener of page.listeners) expect(listener).toHaveBeenCalledOnce();
    const main = screen.getByRole('main');
    for (const chart of main.querySelectorAll('svg')) expect(chart.closest('[aria-live]:not([aria-live="off"])')).toBeNull();
    expect(main.querySelector('svg animate')).toBeNull();
    expect(raf).not.toHaveBeenCalled();
    const motion = readFileSync(resolve(process.cwd(), 'src/lib/styles/motion.css'), 'utf8');
    const reduced = motion.slice(motion.indexOf('@media (prefers-reduced-motion: reduce)'));
    expect(reduced).toContain('animation: none !important');
    expect(reduced).toContain('transition: none !important');
    expect(reduced).toContain('scroll-behavior: auto !important');
    page.unmount(); expect(page.browser.listeners).toBe(0);
  });

  it('retired and out-of-order snapshots cannot resurrect an old Dungeon', async () => {
    const initial = homeSnapshot(); const page = mountHome(initial);
    await screen.findByRole('rowheader', { name: /\bYou$/ });
    const next = homeSnapshot({ stream_id: 'demo-new', sequence: '1', area_kind: 'nexus', run: null });
    page.client.querySnapshot = vi.fn(async () => next);
    page.client.queryStore = vi.fn(async () => saved('1', next.stream_id));
    page.transport.publishSnapshot(next);
    await flushHomeRender();
    await screen.findByRole('link', { name: /Synthetic Sanctum/i });

    // Each rejection has its own accepted same-stream positive control. Flush
    // before assertions, rather than letting waitFor pass on the previous DOM.
    const showDungeonThenNexus = async (sequence: number) => {
      const dungeon = homeSnapshot({ stream_id: next.stream_id, sequence: String(sequence) });
      page.transport.publishSnapshot(dungeon);
      await flushHomeRender();
      expect(screen.getByRole('rowheader', { name: /\bYou$/ })).toBeTruthy();
      expect(within(screen.getByRole('main')).getByRole('heading', { name: 'Party damage' })).toBeTruthy();
      page.client.queryStore = vi.fn(async () => saved(String(sequence + 1), next.stream_id));
      page.transport.publishSnapshot({ ...next, sequence: String(sequence + 1) });
      await flushHomeRender();
      expect(within(screen.getByRole('main')).getByRole('heading', { name: next.map! })).toBeTruthy();
    };
    const expectNexus = () => {
      expect(screen.queryByRole('rowheader', { name: /\bYou$/ })).toBeNull();
      expect(within(screen.getByRole('main')).queryByRole('heading', { name: 'Party damage' })).toBeNull();
      expect(page.browser.adapter.hash).toBe('#/');
    };
    await showDungeonThenNexus(2);
    page.transport.publishSnapshot({ ...initial, sequence: '999' });
    await flushHomeRender();
    expectNexus();
    await showDungeonThenNexus(4);
    page.transport.publishSnapshot({ ...next, sequence: '0', area_kind: 'dungeon', run: initial.run });
    await flushHomeRender();
    expectNexus();
  });
});
