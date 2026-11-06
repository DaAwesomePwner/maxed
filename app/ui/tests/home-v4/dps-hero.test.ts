import { readFileSync } from 'node:fs';
import { cleanup, render } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import type { ComponentProps } from 'svelte';
import type { HomeBossBaseline } from '../../src/lib/bindings/HomeBossBaseline';
import type { PlayerDamage } from '../../src/lib/bindings/PlayerDamage';
import DpsHero from '../../src/lib/home/v4/DpsHero.svelte';
import { partyAverage, relative, usualFor } from '../../src/lib/home/v4/hero';

afterEach(cleanup);
it('hero_number_uses_primary_text_colour', () => {
  const source = readFileSync('src/lib/home/v4/DpsHero.svelte', 'utf8');
  expect(source).toMatch(/\.dps\s*\{[^}]*color:\s*var\(--text-primary\)/);
});
const props: ComponentProps<typeof DpsHero> = { label: 'Your DPS · Synthetic boss', live: true,
  dps: 1380, rank: 2, players: 8, share: 0.25, bestHit: 12345,
  party: { avg: 1000, players: 7 }, usual: { kind: 'none' }, buckets: [] };

it.each([1004, 996])('hero_zero_percent_is_neutral: %s', dps => {
  const page = render(DpsHero, { ...props, dps, usual: { kind: 'value', dps: 1000, fights: 5 } });
  for (const text of ['0% vs party average', '0% vs your usual · avg 1,000 · 5 fights']) {
    const line = page.getByText(text);
    if (text === '0% vs party average') {
      expect(line.classList.contains('positive')).toBe(false);
      expect(line.classList.contains('negative')).toBe(false);
    }
    expect(line.textContent).not.toMatch(/[▲▼]/);
  }
});
const history = (fights: number, avg_dps: number | null = 1000): HomeBossBaseline => ({
  target_type: 1, fights, avg_dps, best_dps: null,
  kill_samples: 0, median_kill_micros: null,
});
const player = (dps: number | null, is_local = false): PlayerDamage => ({ object_id: 1,
  player_id: 'synthetic-player', name: null, class_id: null, is_local, damage: 0, hits: 0,
  biggest_hit: null, deaths: 0, dps, share: null, rank: null, sources: [] });

it('hero_shows_dash_without_dps', () => {
  const page = render(DpsHero, { ...props, dps: null, rank: null, players: null, share: null, bestHit: null });
  expect(page.container.querySelector('.dps')?.textContent).toBe('—');
  expect(page.getByText('vs party —')).toBeTruthy();
  expect(page.container.textContent).not.toMatch(/NaN|undefined/);
  expect(page.container.querySelector('dl')?.textContent).toContain('—');
});

it('hero_vs_party_above_and_below', async () => {
  const page = render(DpsHero, props);
  expect(page.container.querySelector('.dps')?.textContent).toBe('1,380');
  expect(page.getByText('▲ 38% above party average').getAttribute('title')).toBe('vs party (7 players)');
  expect(page.getByText('▲ 38% above party average').classList.contains('positive')).toBe(true);
  await page.rerender({ dps: 750 });
  expect(page.getByText('▼ 25% below party average')).toBeTruthy();
  expect(page.getByText('▼ 25% below party average').classList.contains('negative')).toBe(true);
  await page.rerender({ dps: 0 });
  expect(page.getByText('▼ 100% below party average')).toBeTruthy();
});

it('hero_vs_party_dash_without_other_players', async () => {
  const page = render(DpsHero, { ...props, party: null });
  expect(page.getByText('vs party —').getAttribute('title')).toBe('vs party (0 players)');
  await page.rerender({ party: { avg: 0, players: 2 } });
  expect(page.getByText('vs party —')).toBeTruthy();
});

it('hero_usual_learning_until_three_fights', async () => {
  expect(usualFor(undefined)).toEqual({ kind: 'none' });
  for (const fights of [0, 1, 2]) expect(usualFor(history(fights))).toEqual({ kind: 'learning', fights });
  expect(usualFor(history(3))).toEqual({ kind: 'value', dps: 1000, fights: 3 });
  expect(usualFor(history(3, null))).toEqual({ kind: 'learning', fights: 3 });
  const page = render(DpsHero, { ...props, usual: usualFor(history(2)) });
  expect(page.getByText('learning (2 fights)')).toBeTruthy();
  await page.rerender({ usual: usualFor(history(3)) });
  expect(page.getByText('▲ 38% vs your usual · avg 1,000 · 3 fights')).toBeTruthy();
  await page.rerender({ dps: 750 });
  expect(page.getByText('▼ 25% vs your usual · avg 1,000 · 3 fights')).toBeTruthy();
  await page.rerender({ dps: null });
  expect(page.getByText('vs your usual — · avg 1,000 · 3 fights')).toBeTruthy();
  await page.rerender({ usual: { kind: 'none' } });
  expect(page.container.textContent).not.toMatch(/usual|learning/);
});

it('party_average_excludes_local_and_unknown_but_includes_known_zero', () => {
  expect(partyAverage([player(10000, true), player(400), player(0), player(null)]))
    .toEqual({ avg: 200, players: 2 });
  expect(partyAverage([player(100, true), player(null)])).toBeNull();
  expect(partyAverage([])).toBeNull();
});

it('relative_preserves_unknown_and_zero_baselines', () => {
  expect(relative(1380, 1000)).toBeCloseTo(0.38);
  expect(relative(750, 1000)).toBe(-0.25);
  expect(relative(0, 1000)).toBe(-1);
  for (const [value, base] of [[null, 100], [100, null], [100, 0], [NaN, 100], [100, Infinity], [-1, 100]]) {
    expect(relative(value, base)).toBeNull();
  }
});

it('hero_renders_label_live_stats_and_optional_target_without_a_sprite_resolver', async () => {
  const page = render(DpsHero, { props: { ...props, target: { name: 'Synthetic target', kind: 'Boss', elapsed: '1:23', sprite: null } } });
  for (const text of ['Your DPS · Synthetic boss', 'Live', '#2 of 8', '25%', '12,345', 'Synthetic target', 'Boss', '1:23']) {
    expect(page.getByText(text)).toBeTruthy();
  }
  expect(page.container.querySelector('.dps')?.textContent).toBe('1,380');
  await page.rerender({ live: false, target: null });
  expect(page.queryByText('Live')).toBeNull();
  expect(page.queryByText('Synthetic target')).toBeNull();
});
