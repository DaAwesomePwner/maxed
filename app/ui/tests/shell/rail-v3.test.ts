import { fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import { tick } from 'svelte';
import HeroCard from '../../src/lib/shell/HeroCard.svelte';
import { railView } from '../../src/lib/shell/rail';
import { setDebugMode } from '../../src/lib/debug/debug';
import { homeSnapshot, mountV2, flushHomeRender } from '../home-v2/fixtures';

afterEach(() => setDebugMode(false));
it('renders game order, tones, unsplit numbers and no production condition chips', async () => {
  const snapshot = homeSnapshot();
  snapshot.character!.name = 'TestLongHeroXX';
  snapshot.character!.fame = 125340;
  snapshot.character!.stats = [{ stat_id: 58, value: 150000 }];
  snapshot.character!.stat_panel = { breakdown_provisional: true, conditions: [
    { name: 'Armored', bit: 26, secondary: false, confirmed: true },
  ], lines: [
    { stat_id: 0, current: 1250, total: 1385, base: 1075, cap: 1210, boost: 310, at_cap: false },
    { stat_id: 3, current: 52, total: 1105, base: 1105, cap: 1120, boost: 0, at_cap: false },
    { stat_id: 20, total: 150, base: 45, cap: 70, boost: 105, at_cap: false },
    { stat_id: 21, total: 125, base: 25, cap: 25, boost: 100, at_cap: true },
    { stat_id: 22, total: 50, base: 50, cap: 75, boost: 0, at_cap: false },
    { stat_id: 26, total: 140, base: 40, cap: 50, boost: 100, at_cap: false },
    { stat_id: 27, total: 135, base: 60, cap: 60, boost: 75, at_cap: true },
    { stat_id: 28, total: 110, base: 75, cap: 75, boost: 35, at_cap: true },
  ] };
  const page = render(HeroCard, { view: railView(snapshot) });
  expect(screen.getByRole('heading', { name: 'TestLongHeroXX' })).toBeTruthy();
  expect(screen.getByRole('img', { name: 'Class' }).style.width).toBe('60px');
  expect(screen.getByRole('meter', { name: 'Fame' }).getAttribute('aria-valuetext')).toBe('125340/150000');
  expect(screen.getByRole('meter', { name: 'HP' }).getAttribute('aria-valuetext')).toBe('1250/1385|27 (+310)');
  expect([...page.container.querySelectorAll('.stat-grid dt')].map(n => n.textContent)).toEqual(['ATT', 'DEF', 'SPD', 'DEX', 'VIT', 'WIS']);
  expect(page.container.querySelector('[data-stat-id="20"] dd')?.textContent).toBe('150|25 (+105)');
  expect(page.container.querySelector('[data-stat-id="21"]')?.getAttribute('data-tone')).toBe('max');
  expect(page.container.querySelector('[data-stat-id="22"]')?.getAttribute('data-tone')).toBe('plain');
  expect(page.container.querySelector('[data-stat-id="20"]')?.getAttribute('data-tone')).toBe('boosted');
  const cells = [...page.container.querySelectorAll('[data-stat-id]')];
  const hp = screen.getByRole('meter', { name: 'HP' });
  for (let i = 0; i < 8; i++) {
    const next = structuredClone(snapshot);
    next.character!.stat_panel!.lines[0].current = i % 2 ? 1250 : 12;
    next.character!.stat_panel!.lines.find(line => line.stat_id === 20)!.total = i % 2 ? 150 : 12;
    await page.rerender({ view: railView(next) });
    expect(screen.getByRole('meter', { name: 'HP' })).toBe(hp);
    expect(hp.getAttribute('aria-valuetext')).toBe(`${i % 2 ? 1250 : 12}/1385|27 (+310)`);
    expect([...page.container.querySelectorAll('[data-stat-id]')].every((cell, index) => cell === cells[index])).toBe(true);
  }
  expect(page.container.querySelectorAll('.equipment .frame')).toHaveLength(4);
  for (const frame of page.container.querySelectorAll<HTMLElement>('.equipment .frame')) expect(frame.style.width).toBe('32px');
  expect(page.container.textContent).not.toMatch(/Armored|Observed boost|125,340/);
  setDebugMode(true); await tick();
  expect(screen.getByText('Armored')).toBeTruthy();
  expect(page.container.textContent).toContain('Observed boost · provisional split');
});
it('handles missing goal, fame, panel, boost and art without fabricated numbers', async () => {
  const snapshot = homeSnapshot(); snapshot.character!.stats = [];
  snapshot.character!.fame = 125340;
  snapshot.character!.stat_panel = null;
  const page = render(HeroCard, { view: railView(snapshot) });
  const fame = screen.getByRole('meter', { name: 'Fame' });
  expect(fame.getAttribute('aria-valuetext')).toBe('125340');
  expect(fame.getAttribute('aria-valuenow')).toBe('100');
  expect(screen.getByRole('meter', { name: 'HP' }).getAttribute('aria-valuetext')).toBe('—/— (—)');
  expect(screen.getByRole('meter', { name: 'HP' }).getAttribute('aria-valuenow')).toBe('0');
  expect(page.container.querySelector('[data-stat-id="20"] dd')?.textContent).toBe('— (—)');
  expect(page.container.querySelector('[data-pip]')).toBeNull();
  snapshot.character!.fame = null;
  await page.rerender({ view: railView(snapshot) });
  expect(fame.getAttribute('aria-valuetext')).toBe('—');
  expect(fame.getAttribute('aria-valuenow')).toBe('0');
  expect(page.container.textContent).not.toMatch(/NaN|undefined|Infinity|\+0/);
  expect(screen.getByRole('img', { name: 'Class' }).dataset.fallback).toBe('true');
});

it('keeps icon history in the rail and removes the old content navigation row', async () => {
  const page = mountV2(homeSnapshot()); await flushHomeRender();
  const rail = screen.getByRole('complementary', { name: 'Character' });
  const history = within(rail).getByRole('group', { name: 'History' });
  expect(history.closest('.rail-top')).toBeTruthy();
  expect(within(history).getByRole('button', { name: 'Back' }).textContent?.trim()).toBe('←');
  expect(within(history).getByRole('button', { name: 'Forward' }).textContent?.trim()).toBe('→');
  expect(screen.getByRole('main').querySelector('.route-toolbar')).toBeNull();
  expect(screen.queryByRole('navigation', { name: 'Breadcrumb' })).toBeNull();
  await fireEvent.click(within(rail).getByRole('link', { name: 'Runs' })); await flushHomeRender();
  await fireEvent.click(within(history).getByRole('button', { name: 'Back' })); await flushHomeRender();
  expect(page.browser.adapter.hash).toBe('#/');
  await fireEvent.click(within(history).getByRole('button', { name: 'Forward' })); await flushHomeRender();
  expect(screen.getByRole('heading', { name: 'Runs', level: 1 })).toBeTruthy();
  for (const listener of page.listeners) expect(listener).toHaveBeenCalledOnce();
});
