import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { expect, it, vi } from 'vitest';
import HomePreview from './HomePreview.svelte';
import { createHomePreview } from './scenario';
import RecapPlaceholder from '../../src/lib/home/v4/RecapPlaceholder.svelte';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { homeSnapshot } from '../home/fixtures';
import { hpTraceView } from '../../src/lib/home/v4/death';
import * as clipboard from '../../src/lib/home/v4/clipboard';

it.each([
  ['home-v4-recap', '.recap-screen'],
  ['home-v4-recap-died', '.recap-screen .death-banner'],
  ['home-v4-recap-realm', '.visit-recap-screen'],
] as const)('recap_scenes_mount_with_the_flag: %s', async (mode, root) => {
  window.history.replaceState(null, '', `/?preview=${mode}#/`);
  const copy = vi.spyOn(clipboard, 'copyPng').mockResolvedValue('copied');
  try {
    render(HomePreview);
    const ready = await screen.findByRole('region', { name: 'Recap ready' }, { timeout: 3000 });
    expect(screen.getByRole('main').querySelector('.nexus-home')).toBeTruthy();
    expect(screen.queryByRole('region', { name: 'Recap' })).toBeNull();
    await fireEvent.click(within(ready).getByRole('button', { name: 'View recap' }));
    await waitFor(() => expect(screen.getByRole('main').querySelector(`[data-home="v4"] ${root}`)).toBeTruthy(),
      { timeout: 3000 });
    expect(screen.getByRole('button', { name: 'Copy recap image' })).toBeTruthy();
    expect(screen.getByRole('main').querySelector('.recap-placeholder')).toBeNull();
    expect(copy).not.toHaveBeenCalled();
  } finally { copy.mockRestore(); }
});

it('saved_recap_fixture_records_match_the_scene', async () => {
  const runId = homeSnapshot().run!.id;
  const cleared = createHomePreview('home-v4-recap'), died = createHomePreview('home-v4-recap-died');
  const realm = createHomePreview('home-v4-recap-realm');
  try {
    const story = await cleared.client.getRun(runId), recap = await cleared.client.getRunRecap({ run_id: runId });
    expect(story.status).toBe('ready'); expect(recap.status).toBe('ready');
    if (story.status !== 'ready' || recap.status !== 'ready') throw new Error('Missing saved fixture');
    expect(story.value!.boss_fights).toHaveLength(3);
    expect(story.value!.totals!.players).toHaveLength(8);
    expect(story.value!.drops.filter(drop => drop.bag_rank === 255)).toHaveLength(1);
    expect(recap.value).toMatchObject({ in_a_row: 3, clear_number: 38,
      pb_clear: { clear_micros: 249_000_000, previous_best_micros: 260_000_000 },
      dry_streak_end: { runs: 23, had_previous_white: true } });
    expect(recap.value!.best_dps).toHaveLength(1);
    expect(recap.value!.chain!.steps.map(step => step.kind)).toEqual(['realm', 'castle', 'sanctuary']);
    expect(recap.value!.chain!.steps.at(-1)!.item_types).toHaveLength(4);
    const death = await died.client.getRun(runId);
    if (death.status !== 'ready') throw new Error('Missing death fixture');
    expect(death.value!.run.death!.hp_trace).toHaveLength(12);
    expect(hpTraceView(death.value!.run.death!.hp_trace, 320, 64, death.value!.run.died_at!).gaps).toBe(1);
    const visit = await realm.client.getVisitRecap({ visit_id: 'synthetic-visit' });
    if (visit.status !== 'ready') throw new Error('Missing visit fixture');
    expect(visit.value!.boss_fights.filter(fight => fight.kill)).toHaveLength(5);
    expect(visit.value!.announcements).toHaveLength(2);
    expect(visit.value!.chain!.steps.map(step => step.kind)).toEqual(['realm', 'castle']);
  } finally { cleared.dispose(); died.dispose(); realm.dispose(); }
});

it.each([
  ['home-v4-dungeon', '.dungeon-screen'],
  ['home-v4-realm', '.realm-screen'],
  ['home-v4-nexus', '.nexus-home'],
  ['home-v4-recap', '[aria-label="Recap ready"]'],
] as const)('v4_scenes_mount_with_the_flag: %s', async (mode, root) => {
  window.history.replaceState(null, '', `/?preview=${mode}#/`);
  render(HomePreview);
  await waitFor(() => expect(screen.getByRole('main').querySelector(`[data-home="v4"] ${root}`)).toBeTruthy());
  expect(screen.getByLabelText('Session HUD').querySelectorAll('[data-hud]')).toHaveLength(6);
});

it.each([
  ['home-v4-dungeon', '2', 6, '+3 more'],
  ['home-v4-realm', '9', 7, '+30 more'],
] as const)('v4 live fixtures include party buckets, real ranks and usuals: %s', async (mode, rank, rows, more) => {
  window.history.replaceState(null, '', `/?preview=${mode}#/`);
  render(HomePreview);
  const main = screen.getByRole('main');
  await waitFor(() => {
    expect(main.querySelector('.damage-row.self td')?.textContent).toBe(rank);
    expect(main.querySelectorAll('.damage-row')).toHaveLength(rows);
    expect(main.querySelector('.you-vs-party-chart path.party')).toBeTruthy();
    expect(main.textContent).toContain('vs your usual');
    expect(main.textContent).toContain(more);
    expect(main.querySelector('[data-hud="combat"]')?.textContent).toContain('+38%');
  });
  if (mode === 'home-v4-dungeon') {
    await waitFor(() => expect(main.textContent).toContain('Boss 3 of 3'));
    for (const name of ['Moss Sentinel', 'Crystal Herald', 'Moonlit Warden']) expect(main.textContent).toContain(name);
    expect(main.querySelector('.dry-streak-row')?.textContent).toContain('dry streak 23');
  } else expect(main.querySelector('.gap')?.textContent).toContain('2 players');
});

it('v4 legacy realm fixture shows the missing party line note', async () => {
  window.history.replaceState(null, '', '/?preview=home-v4-realm&variant=legacy#/');
  render(HomePreview);
  const main = screen.getByRole('main');
  await within(main).findByText('Party average line not recorded for this fight');
  expect(main.querySelector('.you-vs-party-chart path.party')).toBeNull();
  expect(within(main).getByText('Ended', { selector: '.ended-tag' })).toBeTruthy();
});

it('v4 recap comes from a saved run and dismisses to the session', async () => {
  window.history.replaceState(null, '', '/?preview=home-v4-recap#/');
  render(HomePreview);
  await screen.findByText('PB');
  await fireEvent.click(screen.getByRole('button', { name: 'View recap' }));
  const recap = screen.getByRole('region', { name: 'Recap' });
  expect(within(recap).getByRole('heading', { name: /CLEARED/ })).toBeTruthy();
  expect(within(recap).getByText('4:09')).toBeTruthy();
  expect(within(recap.querySelector('.recap-tiles') as HTMLElement).getByText('#2 of 8')).toBeTruthy();
  expect(recap.querySelectorAll('.recap-fights-card tbody tr')).toHaveLength(3);
  expect(recap.textContent).toContain('BEST DPS');
  expect(recap.textContent).toContain('3rd Synthetic Moonlit Sanctum in a row tonight');
  expect(recap.textContent).toContain('38');
  expect(recap.textContent).toContain('23');
  await fireEvent.click(within(recap).getByRole('button', { name: 'Back' }));
  await waitFor(() => expect(screen.getByRole('main').querySelector('[data-home="v4"] .nexus-home')).toBeTruthy());
  expect(screen.queryByRole('region', { name: 'Recap' })).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: /^Last recap:/ }));
  expect(screen.getByRole('region', { name: 'Recap' })).toBeTruthy();
});

it('recap_placeholder_still_mounts_while_saving', () => {
  const page = render(RecapPlaceholder, { recap: { watermark: '12',
    source: { kind: 'run', streamId: 'synthetic-stream', runId: 'synthetic-run' },
    summary: { kind: 'run', title: 'Synthetic Sanctum', outcome: 'cleared', startedAt: '1000000',
      endedAt: '65000000', localDps: null, rank: null, players: null, fights: [], best: null, runId: 'synthetic-run' } },
    status: 'saving', visuals: emptyHomeVisuals(), resolveSpriteUrl: () => '', onDismiss: vi.fn(), onRetry: vi.fn() });
  expect(page.container.querySelector('.recap-placeholder')).toBeTruthy();
  expect(page.getByText('Saving run…')).toBeTruthy();
  expect(page.container.querySelector('.recap-screen')).toBeNull();
});

it.each(['dungeon', 'realm', 'nexus'] as const)('resolves synthetic enchant descriptions in the %s preview', async mode => {
  const preview = createHomePreview(mode);
  try {
    const details = await preview.client.getItemDetails(200, [1, 2, 3, 4]);
    expect(details.enchants.map(enchant => enchant.name)).toEqual([
      'Synthetic Edge I', 'Synthetic Guard II', 'Synthetic Spark III', 'Synthetic Echo IV',
    ]);
    expect(details.enchants[0].description!.length).toBeGreaterThanOrEqual(230);
  } finally { preview.dispose(); }
});

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
    expect(main.querySelector('[data-home="v3"]')).toBeTruthy();
    expect(main.querySelector('[data-home="v4"]')).toBeNull();
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
  const pager = within(main).getByRole('navigation', { name: 'Loot Pages' });
  await fireEvent.click(within(pager).getByRole('button', { name: 'Next page' }));
  await waitFor(() => expect(main.querySelector('[data-loot-type]')!.getAttribute('data-loot-type')).not.toBe(first));
  expect(main.querySelectorAll('[data-loot-type]')).toHaveLength(32);
  await fireEvent.click(within(pager).getByRole('button', { name: 'First page' }));
  await waitFor(() => expect(main.querySelector('[data-loot-type]')!.getAttribute('data-loot-type')).toBe(first));
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
