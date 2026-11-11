import { readFileSync } from 'node:fs';
import { fireEvent, render, screen } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, expect, it, vi } from 'vitest';
import DamageTable from '../../src/lib/home/v3/DamageTable.svelte';
import BossHpBar from '../../src/lib/home/v3/BossHpBar.svelte';
import FightTabs from '../../src/lib/home/v3/FightTabs.svelte';
import { rankWindow } from '../../src/lib/home/v3/rank-window';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { fight } from '../home-v2/fixtures';

afterEach(() => vi.unstubAllGlobals());
const visuals = emptyHomeVisuals();
const resolveSpriteUrl = () => null;
const rows = Array.from({ length: 85 }, (_, i) => ({ player_id: `p${i}`, object_id: i, rank: i + 1, name: `FourteenChar${i}`,
  class_id: 999, is_local: i === 14, damage: (85 - i) * 100, dps: 99, share: 0.01 }));

it('links usable name cells with an affordance and keeps local highlight', () => {
  const page = render(DamageTable, { basis: 'player_active_window_v1', rows: rows.slice(14, 16), total: 2, visuals, resolveSpriteUrl,
    rowHref: row => row.is_local ? '#/runs/r/players/p14' : null });
  const link = screen.getByRole('link', { name: /FourteenChar14/ });
  expect(link.getAttribute('href')).toBe('#/runs/r/players/p14');
  expect(link.textContent).toContain('›');
  expect(link.closest('tr')?.classList.contains('self')).toBe(true);
  expect(screen.getByRole('rowheader', { name: 'FourteenChar15' }).querySelector('a')).toBeNull();
  expect(page.container.querySelectorAll('.damage-row a')).toHaveLength(1);
});

it('renders a ranked window, actual other damage and a real more link', () => {
  const page = render(DamageTable, { basis: 'player_active_window_v1', rows: rankWindow(rows), total: 85, otherDamage: 100,
    visuals, resolveSpriteUrl, moreHref: '#/fights/f' });
  expect(page.container.querySelectorAll('.damage-row')).toHaveLength(6);
  expect(page.container.querySelectorAll('.damage-row.self')).toHaveLength(1);
  expect(screen.getByText('··· 10 players')).toBeTruthy();
  expect(screen.getByRole('link', { name: '+79 more players ›' }).getAttribute('href')).toBe('#/fights/f');
  expect(page.container.querySelector('[data-unattributed]')?.textContent).toContain('100');
  expect(page.container.querySelector<HTMLElement>('.damage-fill')?.style.width).toBe('100%');
  expect(page.container.querySelector('[data-fallback="true"]')?.getAttribute('aria-label')).toBe('Class');
});
it('keeps column definitions and row slots stable through 2 to 4 digit churn', async () => {
  const page = render(DamageTable, { basis: 'player_active_window_v1', rows: rows.slice(0, 5), total: 85, visuals, resolveSpriteUrl });
  const columns = [...page.container.querySelectorAll('col')].map(col => col.getAttribute('style'));
  const table = page.container.querySelector('table');
  const slots = [...page.container.querySelectorAll('.damage-row')];
  await page.rerender({ rows: rows.slice(0, 5).map(row => ({ ...row, damage: 9999, dps: 9999 })),
    total: 85, visuals, resolveSpriteUrl });
  expect(page.container.querySelector('table')).toBe(table);
  expect([...page.container.querySelectorAll('col')].map(col => col.getAttribute('style'))).toEqual(columns);
  expect([...page.container.querySelectorAll('.damage-row')]).toEqual(slots);
  expect(page.container.textContent).toContain('9,999');
});
it('gives eight-digit damage totals their own slot beside the track', () => {
  const page = render(DamageTable, { basis: 'player_active_window_v1', rows: [{ ...rows[0], damage: 12_345_678 }], total: 1,
    otherDamage: 10_000_000, visuals, resolveSpriteUrl });
  const figures = [...page.container.querySelectorAll('.amount')].map(amount => amount.querySelector(':scope > b')?.textContent);
  expect(figures).toEqual(['12,345,678', '10,000,000']);
  for (const amount of page.container.querySelectorAll('.amount')) expect(amount.querySelector('.track .damage-fill')).toBeTruthy();
  // JSDOM cannot measure text; the slot is sized in ch from content rather than a fixed rem width.
  const css = readFileSync('src/lib/home/v3/DamageTable.svelte', 'utf8');
  expect(css).toMatch(/\.amount b \{ flex: none; min-width: 10ch;/);
  expect(css).not.toMatch(/\.amount b \{[^}]*flex-basis/);
});
it('uses singular gap copy at rank six and plural copy as rank changes', async () => {
  const atRank = (rank: number) => rankWindow(rows.map(row => ({ ...row, is_local: row.rank === rank })));
  const page = render(DamageTable, { basis: 'player_active_window_v1', rows: atRank(6), total: 85, visuals, resolveSpriteUrl });
  expect(page.container.querySelector('.gap')?.textContent).toBe('··· 1 player');
  await page.rerender({ rows: atRank(7) });
  expect(page.container.querySelector('.gap')?.textContent).toBe('··· 2 players');
});
it('unknown HP preserves its slot and confirmed kills show defeated HP', async () => {
  const page = render(BossHpBar, { hp: null, max: null });
  const slot = page.container.querySelector('.boss-hp');
  expect(slot?.getAttribute('data-state')).toBe('unknown');
  expect(screen.queryByRole('meter')).toBeNull();
  await page.rerender({ hp: 99, max: 9999 });
  expect(page.container.querySelector('.boss-hp')).toBe(slot);
  expect(screen.getByRole('meter').getAttribute('aria-valuenow')).toBe('99');
  await page.rerender({ hp: 9999, max: 9999 });
  expect(page.container.querySelector('.boss-hp')).toBe(slot);
  expect(slot?.textContent).toContain('9,999 / 9,999 · 100%');
  const status = page.container.querySelector('.boss-status');
  await page.rerender({ hp: 73, max: 9999, ended: true });
  expect(slot?.textContent).toContain('73 / 9,999');
  expect(status?.textContent).toBe('Ended');
  expect(slot?.getAttribute('data-state')).toBe('ended');
  expect(page.container.querySelector('.boss-status')).toBe(status);
  await page.rerender({ hp: 73, max: 9999, ended: true, killedAt: '1000000' });
  expect(slot?.textContent).toContain('Killed');
  expect(slot?.getAttribute('data-state')).toBe('killed');
  expect(screen.getByRole('meter').getAttribute('aria-valuenow')).toBe('0');
  expect(slot?.textContent).toContain('Defeated');
  expect(status?.textContent).toMatch(/^Killed · \d{1,2}:\d{2}\s?[AP]M$/);
  expect(page.container.querySelector('.boss-hp')).toBe(slot);
  expect(page.container.querySelector('.boss-status')).toBe(status);
});
it.each([false, true])('confirmed kills supply zero HP when HP was never observed (inside: %s)', async statusInside => {
  const page = render(BossHpBar, { hp: null, max: 336000, killedAt: '1000000', statusInside });
  const meter = screen.getByRole('meter');
  expect(meter.getAttribute('aria-valuenow')).toBe('0');
  expect(meter.getAttribute('aria-valuetext')).toContain('Defeated · 0 / 336,000 · 0%');
  expect(meter.classList.contains('unknown')).toBe(false);
  await page.rerender({ killedAt: null });
  expect(screen.queryByRole('meter')).toBeNull();
  await page.rerender({ killedAt: '1000000', max: null });
  expect(screen.queryByRole('meter')).toBeNull();
});
it('announces the kill with HP only when the status is inside the meter', async () => {
  const killedAt = String(new Date(2026, 9, 3, 19, 35, 30).getTime() * 1000);
  const page = render(BossHpBar, { hp: 0, max: 336000, killedAt });
  const meter = screen.getByRole('meter');
  expect(meter.getAttribute('aria-valuetext')).toBe('Defeated \u00b7 0 / 336,000 · 0%');
  expect(page.container.querySelector('.boss-status')?.textContent).toMatch(/^Killed · \d{1,2}:\d{2}\s?[AP]M$/);
  await page.rerender({ statusInside: true });
  expect(meter.getAttribute('aria-valuetext')).toBe('Defeated \u00b7 0 / 336,000 · 0%; Defeated · 7:35 PM');
  expect(page.container.querySelector('.boss-status')).toBeNull();
  await page.rerender({ killedAt: null });
  expect(meter.getAttribute('aria-valuetext')).toBe('0 / 336,000 · 0%');
});
it.each([NaN, Infinity, -1])('invalid values render dashes (%s)', value => {
  const page = render(DamageTable, { basis: 'player_active_window_v1', rows: [{ ...rows[0], rank: value, damage: value, dps: value, share: value }],
    total: 1, otherDamage: value, visuals, resolveSpriteUrl });
  expect(page.container.textContent).not.toMatch(/NaN|Infinity|undefined/);
  expect(page.container.querySelector('[data-unattributed]')).toBeNull();
  const hp = render(BossHpBar, { hp: value, max: 100 });
  expect(hp.container.querySelector('[role="meter"]')).toBeNull();
});
it('64 fights scroll without selecting; whole run is fixed and keyboard navigation works', async () => {
  const onselect = vi.fn();
  const fights = Array.from({ length: 64 }, (_, i) => fight(`f${i}`, {
    started: String(64_000_000 - i * 1_000_000), last_hit: '65000000',
    ended: i ? '65000000' : null,
  }));
  const page = render(FightTabs, { fights, selected: 'f0', visuals, resolveSpriteUrl, onselect });
  const strip = page.container.querySelector<HTMLElement>('.fight-scroll')!;
  Object.defineProperties(strip, { clientWidth: { configurable: true, value: 340 },
    scrollWidth: { configurable: true, value: 64 * 170 } });
  const scrollBy = vi.fn(); strip.scrollBy = scrollBy;
  await fireEvent.scroll(strip); await tick();
  expect(screen.getAllByRole('tab')[0].textContent).toBe('Whole run');
  expect(strip.contains(screen.getByRole('tab', { name: 'Whole run' }))).toBe(false);
  expect(screen.getAllByRole('tab')).toHaveLength(65);
  const more = screen.getByRole('button', { name: /62 more/ });
  expect(screen.getByRole('tablist').contains(more)).toBe(false);
  await fireEvent.click(more);
  expect(scrollBy).toHaveBeenCalledWith({ left: 340, behavior: 'auto' });
  expect(onselect).not.toHaveBeenCalled();
  await fireEvent.keyDown(screen.getByRole('tab', { name: 'Whole run' }), { key: 'ArrowRight' });
  expect(onselect).toHaveBeenCalledWith('f0');
  expect(page.container.querySelectorAll('.finished')).toHaveLength(63);
  expect(page.container.querySelectorAll('.live')).toHaveLength(1);
});
it('keeps Whole run tabbable when the selected fight is absent without changing selection', async () => {
  const onselect = vi.fn();
  const page = render(FightTabs, { fights: [fight('f0'), fight('f1')], selected: 'f0',
    visuals, resolveSpriteUrl, onselect });
  expect(page.container.querySelectorAll('[role="tab"][tabindex="0"]')).toHaveLength(1);
  expect(page.container.querySelector('[role="tab"][tabindex="0"]')?.getAttribute('aria-selected')).toBe('true');
  await page.rerender({ fights: [fight('f1')] });
  const whole = screen.getByRole('tab', { name: 'Whole run' });
  expect([...page.container.querySelectorAll('[role="tab"][tabindex="0"]')]).toEqual([whole]);
  expect(page.container.querySelector('[role="tab"][aria-selected="true"]')).toBeNull();
  expect(onselect).not.toHaveBeenCalled();
  await fireEvent.keyDown(whole, { key: 'ArrowRight' });
  expect(onselect).toHaveBeenCalledWith('f1');
  await page.rerender({ selected: 'non-boss' });
  expect([...page.container.querySelectorAll('[role="tab"][tabindex="0"]')]).toEqual([whole]);
});
it('updates overflow when fights arrive without a scroll or resize event', async () => {
  const onselect = vi.fn();
  const page = render(FightTabs, { fights: [fight('f0')], selected: 'f0',
    visuals, resolveSpriteUrl, onselect });
  const strip = page.container.querySelector<HTMLElement>('.fight-scroll')!;
  Object.defineProperties(strip, { clientWidth: { configurable: true, value: 340 },
    scrollWidth: { configurable: true, value: 340 } });
  const scrollBy = vi.fn(); strip.scrollBy = scrollBy;
  await fireEvent.scroll(strip);
  expect(page.container.querySelector('.more')).toBeNull();
  Object.defineProperty(strip, 'scrollWidth', { configurable: true, value: 680 });
  await page.rerender({ fights: Array.from({ length: 4 }, (_, i) => fight(`f${i}`)) });
  const more = screen.getByRole('button', { name: '2 more ›' });
  expect(screen.queryByRole('button', { name: '‹ Newest' })).toBeNull();
  await fireEvent.click(more);
  expect(scrollBy).toHaveBeenCalledWith({ left: 340, behavior: 'auto' });
  expect(onselect).not.toHaveBeenCalled();
});
it('recognizes the fractional scroll end and lets the overflow button return to newest', async () => {
  const onselect = vi.fn();
  const page = render(FightTabs, { fights: Array.from({ length: 4 }, (_, i) => fight(`f${i}`)),
    selected: 'f0', visuals, resolveSpriteUrl, onselect });
  const strip = page.container.querySelector<HTMLElement>('.fight-scroll')!;
  Object.defineProperties(strip, { clientWidth: { configurable: true, value: 340 },
    scrollWidth: { configurable: true, value: 680 } });
  const scrollBy = vi.fn(); strip.scrollBy = scrollBy;
  strip.scrollLeft = 338;
  await fireEvent.scroll(strip);
  expect(screen.getByRole('button', { name: '1 more ›' })).toBeTruthy();
  strip.scrollLeft = 339.6;
  await fireEvent.scroll(strip);
  const newest = screen.getByRole('button', { name: '‹ Newest' });
  expect(screen.queryByRole('button', { name: '1 more ›' })).toBeNull();
  await fireEvent.click(newest);
  expect(scrollBy).toHaveBeenCalledWith({ left: -680, behavior: 'auto' });
  expect(onselect).not.toHaveBeenCalled();
});
