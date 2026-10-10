import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { expect, it, vi } from 'vitest';
import HomePreview from './HomePreview.svelte';

it.each([
  ['dungeon', 'Synthetic Moonlit Sanctum'],
  ['realm', 'Synthetic Willow Realm'],
  ['nexus', 'Nexus'],
] as const)('mounts the real Home route for the %s preview', async (mode, heading) => {
  window.history.replaceState(null, '', `/?preview=${mode}#/`);
  render(HomePreview);
  expect(await within(screen.getByRole('main')).findByRole('heading', { name: heading })).toBeTruthy();
  expect(screen.getByRole('complementary', { name: 'Character' })).toBeTruthy();
  // Check evidence that depends on ordered events/history, not just the snapshot title.
  await waitFor(() => {
    const main = screen.getByRole('main');
    expect(main.querySelectorAll('.area-home')).toHaveLength(mode === 'nexus' ? 0 : 1);
    if (mode !== 'nexus') expect(main.querySelector(`.home > .area-home.${mode}-home`)).toBeTruthy();
    if (mode === 'dungeon') {
      expect(main.querySelectorAll('[role="tab"]')).toHaveLength(4);
      expect(main.querySelectorAll('[data-drop-id]').length).toBeLessThanOrEqual(5);
      expect(main.querySelector('.area-loot')?.textContent).toContain('5');
    } else if (mode === 'realm') {
      expect(main.querySelectorAll('[data-kill-id]')).toHaveLength(2);
      expect(main.querySelectorAll('[data-drop-id]').length).toBeLessThanOrEqual(5);
      expect(main.querySelector('.area-loot')?.textContent).toContain('5');
    } else {
      expect(main.querySelectorAll('[data-row-id]')).toHaveLength(6);
expect(main.textContent).toContain('The Shatters');
expect(main.querySelectorAll('[data-loot-type]')).toHaveLength(12);
expect(main.querySelector('[data-pip="4"]')).toBeTruthy();
    }
  }, { timeout: 3000 });
});

it('renders the 14-character and four-digit rail stress case through the real Shell', async () => {
  window.history.replaceState(null, '', '/?preview=nexus&variant=stress#/');
  render(HomePreview);
  const rail = screen.getByRole('complementary', { name: 'Character' });
  await within(rail).findByRole('heading', { name: 'TestLongHeroXX' });
  expect(within(rail).getByRole('meter', { name: 'HP' }).getAttribute('aria-valuetext')).toBe('1250/1385|27 (+310)');
  expect(rail.querySelector('[data-stat-id="20"] dd')?.textContent).toBe('150|25 (+105)');
  expect(within(rail).getByRole('meter', { name: 'Fame' }).getAttribute('aria-valuetext')).toBe('125340');
  expect(rail.querySelectorAll('[data-pip="4"]')).toHaveLength(4);
});

it('retains Dungeon preview departure and death events through the final snapshot', async () => {
  vi.useFakeTimers();
  window.history.replaceState(null, '', '/?preview=dungeon#/');
  render(HomePreview);
  const main = screen.getByRole('main');
  // Include deferred replay, storage polling and the route-ready deadline.
  await vi.advanceTimersByTimeAsync(20_000);
  expect(within(main).getByRole('heading', { name: 'Synthetic Moonlit Sanctum' })).toBeTruthy();
  expect(main.querySelector('[data-kpi="left"] .value')?.textContent).toBe('1');
  expect(main.querySelector('[data-kpi="deaths"] .value')?.textContent).toBe('1');
  expect(main.querySelectorAll('.timeline svg.glyph')).toHaveLength(2);
  expect(main.querySelectorAll('.timeline svg.glyph.death')).toHaveLength(1);
});

it('keeps a stress Today preview bounded while paging aggregate types', async () => {
  window.history.replaceState(null, '', '/?preview=nexus&variant=stress#/');
  render(HomePreview);
  const main = screen.getByRole('main');
  await within(main).findByRole('heading', { name: 'Nexus' });
  await fireEvent.click(screen.getByRole('button', { name: 'Today' }));
  await waitFor(() => expect(main.querySelectorAll('[data-loot-type]')).toHaveLength(32));
  expect(main.textContent).toContain('9,999');
  expect(main.innerHTML).not.toMatch(/NaN|undefined|Infinity/);
  const first = main.querySelector('[data-loot-type]')!.getAttribute('data-loot-type');
  await fireEvent.click(screen.getByRole('button', { name: 'Next Loot' }));
  await waitFor(() => expect(main.querySelector('[data-loot-type]')!.getAttribute('data-loot-type')).not.toBe(first));
  expect(main.querySelectorAll('[data-loot-type]')).toHaveLength(32);
});
it('Dungeon busy preview retains 64 fights, 85 players and all 128 bags', async () => {
  window.history.replaceState(null, '', '/?preview=dungeon&variant=busy#/');
  render(HomePreview);
  const main = screen.getByRole('main');
  await waitFor(() => expect(main.querySelectorAll('[role="tab"]')).toHaveLength(65));
  expect(main.querySelectorAll('.damage-row')).toHaveLength(6);
  expect(within(main).getByRole('link', { name: /\+79 more players/ })).toBeTruthy();
  expect(main.querySelector('.area-loot')?.textContent).toContain('128');
  expect(main.querySelectorAll('.area-loot [data-drop-id]').length).toBeLessThan(128);
  expect(main.querySelector('[data-kpi="time"]')?.textContent).toContain('PB 3:50');
  await fireEvent.click(within(main).getByRole('tab', { name: 'Whole run' }));
  expect(main.querySelectorAll('.damage-row')).toHaveLength(9);
  expect(main.querySelector('.fight-chart')).toBeNull();
});
it('Dungeon preview churn updates values without replacing KPI nodes or the selected tab', async () => {
  window.history.replaceState(null, '', '/?preview=dungeon&variant=busy&churn=1#/');
  const page = render(HomePreview), main = screen.getByRole('main');
  await waitFor(() => expect(main.querySelectorAll('[role="tab"]')).toHaveLength(65));
  const cell = main.querySelector('[data-kpi="players"]')!;
  await fireEvent.click(within(main).getByRole('tab', { name: 'Whole run' }));
  await waitFor(() => expect(cell.querySelector('.value')?.textContent).toMatch(/^9\s*\/ 85/));
  await waitFor(() => expect(cell.querySelector('.value')?.textContent).toMatch(/^85\s*\/ 85/));
  expect(main.querySelector('[data-kpi="players"]')).toBe(cell);
  expect(within(main).getByRole('tab', { selected: true }).textContent).toBe('Whole run');
  page.unmount();
});

it('Realm busy preview has six merged boss kills and capped loot with 85 players', async () => {
  window.history.replaceState(null, '', '/?preview=realm&variant=busy#/');
  render(HomePreview);
  await waitFor(() => {
    const main = screen.getByRole('main');
    expect(main.querySelector('[data-kpi="players"] dd')?.textContent).toMatch(/85\s*\/\s*85/);
    expect(main.querySelector('[data-kpi="fame"] .value')?.textContent).toBe('+1,140');
    expect(main.querySelector('.kill-count')?.textContent).toBe('6');
    expect(main.querySelectorAll('[data-kill-id]').length).toBeLessThan(6);
    expect(main.querySelector('.area-loot')?.textContent).toContain('128');
    expect(main.querySelectorAll('.area-loot [data-drop-id]').length).toBeLessThan(128);
    expect(main.textContent).not.toMatch(/NaN|undefined/);
  }, { timeout: 3000 });
});
