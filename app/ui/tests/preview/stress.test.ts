import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { tick } from 'svelte';
import { setDebugMode } from '../../src/lib/debug/debug';
import HomePreview from './HomePreview.svelte';
import { createHomePreview } from './scenario';
import { busyRail, caseDebugText, stressCases, stressRail } from './acceptance-cases';

beforeEach(() => {
  // Node may expose no global localStorage; exercise the debug store's persistence API explicitly.
  const values = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
    removeItem: (key: string) => { values.delete(key); },
  });
});

afterEach(() => {
  setDebugMode(false);
  vi.unstubAllGlobals();
  window.history.replaceState(null, '', '/#/');
});

it.each(stressCases)('$label supplies coherent stress DTOs', async scenario => {
  window.history.replaceState(null, '', '/' + scenario.query + '#/');
  const preview = createHomePreview(scenario.mode);
  try {
    await preview.client.querySnapshot(); // Establish baseline and start publication.
    let snapshot = await preview.client.querySnapshot();
    await waitFor(async () => {
      snapshot = await preview.client.querySnapshot();
      expect(BigInt(snapshot.sequence)).toBeGreaterThan(0n);
    });
    expect(snapshot.character!.name).toBe(scenario.stress ? stressRail.name : busyRail.name);
    if (scenario.stress) expect(snapshot.character!.name).toHaveLength(14);
    expect(snapshot.character!.fame).toBe(scenario.stress ? 125340 : 1680);
    expect(snapshot.character!.stats.some(row => row.stat_id === 58)).toBe(false);
    const scope = { session: { id: snapshot.session!.id } };
    if (scenario.mode === 'nexus') {
      const page = await preview.client.queryScopeLoot(scope, 32);
      expect(page.status).toBe('ready');
      if (page.status !== 'ready') throw new Error('Synthetic scope loot was not ready');
      expect(page.value.total_bags).toBe(128);
      expect(page.value.items).toHaveLength(32);
      expect(page.value.next).not.toBeNull();
    } else {
      expect(snapshot.instance!.population).toBe(85);
      expect(snapshot.instance!.population_peak).toBe(85);
      expect(snapshot.instance!.context.instance_cap).toBe(85);
      expect(snapshot.drops).toHaveLength(128);
      expect(new Set(snapshot.drops.map(row => row.id)).size).toBe(128);
      const fight = snapshot.fights.at(-1)!;
      expect(fight.players).toHaveLength(scenario.participants);
      expect(fight.players.filter(row => row.is_local)).toHaveLength(1);
      expect(fight.players[13]).toMatchObject({
        rank: 14, is_local: true, player_id: snapshot.character!.id,
      });
      expect(fight.ended !== null).toBe(scenario.ended);
      const total = fight.players.reduce((sum, row) => sum + row.damage, fight.unattributed_damage);
      if (scenario.stress) expect(fight.total_damage).toBe(total);
      else {
        // The shipped busy builder rounds each proportional player damage independently.
        expect(Math.abs(fight.total_damage - total)).toBeLessThanOrEqual(fight.players.length / 2);
        expect(snapshot.fights.filter(row => row.kill)).toHaveLength(6);
      }
      if (scenario.mode === 'dungeon') {
        expect(snapshot.fights).toHaveLength(64);
        expect(snapshot.run_totals!.fights).toBe(64);
        expect(snapshot.run_totals!.local!.rank).toBe(14);
      } else {
        expect(snapshot.fights.filter(row => row.kill).length).toBeGreaterThanOrEqual(6);
        expect(snapshot.instance!.visit_fame_delta).toBe(1140);
      }
    }
  } finally {
    preview.dispose();
  }
});

it.each(stressCases)('$label mounts stress data and restores debug copy', async scenario => {
  setDebugMode(false);
  window.history.replaceState(null, '', '/' + scenario.query + '#/');
  render(HomePreview);
  const rail = screen.getByRole('complementary', { name: 'Character' });
  const main = screen.getByRole('main');
  const expectedRail = scenario.stress ? stressRail : busyRail;
  await waitFor(() => {
    expect(rail.textContent).toContain(expectedRail.name);
    expect(within(rail).getByRole('meter', { name: 'HP' })
      .getAttribute('aria-valuetext')).toBe(expectedRail.hp);
    expect(within(rail).getByRole('meter', { name: 'MP' })
      .getAttribute('aria-valuetext')).toBe(expectedRail.mp);
    expect(within(rail).getByRole('meter', { name: 'Fame' })
      .getAttribute('aria-valuetext')).toBe(expectedRail.fame);
    expect(within(rail).getByRole('meter', { name: 'Fame' })
      .getAttribute('aria-valuenow')).toBe('100');
    expect(rail.querySelector('[data-stat-id="20"] dd')?.textContent).toBe(expectedRail.att);
    expect(rail.querySelector('[data-stat-id="26"] dd')?.textContent).toBe(expectedRail.vit);
    expect(rail.querySelector('[data-stat-id="22"] dd')?.textContent).toBe(expectedRail.spd);
    if (scenario.stress) {
      expect(rail.querySelector('[data-stat-id="0"] [data-tone="boosted"]')).toBeTruthy();
      expect(rail.querySelector('[data-stat-id="3"] [data-tone="plain"]')).toBeTruthy();
      expect(rail.querySelectorAll('[data-pip="4"]')).toHaveLength(4);
    }
    expect(main.textContent).toContain(scenario.target);
    if (scenario.mode === 'nexus') {
      expect(main.querySelectorAll('[data-loot-type]')).toHaveLength(32);
    } else {
      const table = within(main).getByRole('table', { name: 'Player Damage' });
      if (scenario.stress) {
        expect(table.querySelector('.self td')?.textContent?.trim()).toBe('14');
        expect(table.querySelector('.gap')?.textContent).toContain('9 players');
        expect(main.textContent).toContain('+41 more players');
      } else {
        // Ended-only busy replay keeps its first focused encounter; preserve that contract.
        expect(main.querySelector('[data-fight-id]')?.getAttribute('data-fight-id')).toBe('realm-retained-0');
        expect(table.querySelector('.self td')?.textContent?.trim()).toBe('1');
        expect(table.querySelectorAll('.damage-row')).toHaveLength(5);
        expect(table.querySelector('.gap')).toBeNull();
        expect(main.textContent).toContain('+2 more players');
      }
      if (scenario.mode === 'realm') expect(main.querySelector('.kill-count')?.textContent).toBe(scenario.stress ? '7' : '6');
      if (scenario.ended) expect(main.textContent).toContain('Ended');
    }
  }, { timeout: 5000 });
  expect(main.textContent).not.toMatch(/NaN|undefined|Infinity/);
  for (const text of caseDebugText(scenario)) expect(main.textContent).not.toContain(text);
  expect(rail.textContent).not.toContain('Observed boost');
  setDebugMode(true);
  expect(localStorage.getItem('maxed.debug')).toBe('1');
  await tick();
  for (const details of main.querySelectorAll('details')) details.open = true;
  for (const text of caseDebugText(scenario)) expect(main.textContent).toContain(text);
  expect(rail.textContent).toContain('Observed boost');
  setDebugMode(false);
  await tick();
  for (const text of caseDebugText(scenario)) expect(main.textContent).not.toContain(text);

  if (scenario.mode === 'nexus') {
    await fireEvent.click(within(main).getByRole('button', { name: 'Today' }));
    await waitFor(() => expect(main.querySelectorAll('[data-loot-type]')).toHaveLength(32),
      { timeout: 5000 });
  }
});
