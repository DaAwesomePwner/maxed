import { readFileSync } from 'node:fs';
import { cleanup, render } from '@testing-library/svelte';
import { flushSync } from 'svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { DropView } from '../../src/lib/bindings/DropView';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';
import { spriteUrl } from '../../src/lib/bridge/visuals';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import WhiteBanner from '../../src/lib/home/v4/WhiteBanner.svelte';
import { createWhiteQueue, newWhites, type WhiteBannerItem, type WhiteBannerState } from '../../src/lib/home/v4/white-queue';
import { routeHref } from '../../src/lib/navigation/routes';

beforeEach(() => vi.useFakeTimers());
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });
const item = (id = '1'): WhiteBannerItem => ({ dropId: id, itemType: 101, itemName: `Synthetic item ${id}`,
  bagType: 201, areaName: 'Synthetic dungeon', sourceName: 'Synthetic boss (estimated)',
  at: String(Date.parse('2026-10-12T12:00:00Z') * 1000), streak: { kind: 'ended', runs: 23 } });
const drop = (id: string, bag_rank: number | null = 255): DropView => ({ id, bag_rank, session_id: 'synthetic-session',
  run_id: 'synthetic-run', at: item().at, bag_type: 201, items: [101], contents: [], enchant: null,
  source_enemy: null, source_estimated: false, source_reason: null, contents_known: false, issues: [] });
const sprite: SpriteRef = { key: 'synthetic', sheet: 'mapObjects.png', x: 0, y: 0, width: 8, height: 8 };
const visuals = { ...emptyHomeVisuals(), client_fingerprint: 'a'.repeat(64), objects: [
  { object_type: 101, name: 'Synthetic item', sprite, bag: null },
  { object_type: 201, name: 'White bag', sprite: { ...sprite, key: 'synthetic-bag' }, bag: null },
] };
const props = { visuals, resolveSpriteUrl: (s: SpriteRef) => spriteUrl(visuals.client_fingerprint, s)!,
  itemHref: (type: number) => routeHref({ kind: 'item', type }) };
const empty: WhiteBannerState = { current: null, overflow: 0 };

function queuedBanner(options?: { durationMs?: number; maxQueued?: number }) {
  const page = render(WhiteBanner, { ...props, state: empty });
  const queue = createWhiteQueue(state => { flushSync(() => page.rerender({ state })); }, options);
  return { page, queue };
}

it('banner_lasts_six_seconds', () => {
  const { page, queue } = queuedBanner();
  const region = page.getByRole('status');
  expect(region.textContent).toBe('');
  queue.push(item());
  expect(page.getByRole('status').textContent).toContain('Synthetic item 1');
  vi.advanceTimersByTime(5_999);
  expect(page.getByRole('status')).toBeTruthy();
  vi.advanceTimersByTime(1);
  expect(page.getByRole('status')).toBe(region);
  expect(region.textContent).toBe('');
  expect(vi.getTimerCount()).toBe(0);
});

it('queue_waits_and_counts_overflow_past_three', () => {
  const { page, queue } = queuedBanner();
  for (let id = 1; id <= 5; id++) queue.push(item(String(id)));
  for (let id = 1; id <= 4; id++) {
    expect(page.getByRole('status').textContent).toContain(`Synthetic item ${id}`);
    expect(page.queryByText('+1 more whites') !== null).toBe(id === 4);
    vi.advanceTimersByTime(6_000);
  }
  expect(page.getByRole('status').textContent).toBe('');
  queue.push(item('6'));
  expect(page.getByRole('status').textContent).not.toContain('more whites');
});

it('push_does_not_extend_the_current_banner_and_options_control_the_queue', () => {
  const changed = vi.fn();
  const queue = createWhiteQueue(changed, { durationMs: 100, maxQueued: 1 });
  queue.push(item('1'));
  vi.advanceTimersByTime(99);
  queue.push(item('2')); queue.push(item('3')); queue.push(item('4'));
  vi.advanceTimersByTime(1);
  expect(changed).toHaveBeenLastCalledWith({ current: item('2'), overflow: 2 });
  vi.advanceTimersByTime(100);
  expect(changed).toHaveBeenLastCalledWith(empty);
});

it('overflow_is_not_repeated_when_another_white_arrives_during_the_final_banner', () => {
  const changed = vi.fn();
  const queue = createWhiteQueue(changed, { durationMs: 100, maxQueued: 1 });
  queue.push(item('1')); queue.push(item('2')); queue.push(item('3'));
  vi.advanceTimersByTime(100);
  expect(changed).toHaveBeenLastCalledWith({ current: item('2'), overflow: 1 });
  queue.push(item('4'));
  vi.advanceTimersByTime(100);
  expect(changed).toHaveBeenLastCalledWith({ current: item('4'), overflow: 0 });
});

it('zero_waiting_capacity_counts_overflow_on_the_current_banner', () => {
  const changed = vi.fn();
  const queue = createWhiteQueue(changed, { maxQueued: 0 });
  queue.push(item()); queue.push(item('2'));
  expect(changed).toHaveBeenLastCalledWith({ current: item(), overflow: 1 });
  vi.advanceTimersByTime(6_000);
  expect(changed).toHaveBeenLastCalledWith(empty);
});

it('dispose_cancels_waiting_banners_and_ignores_later_pushes', () => {
  const changed = vi.fn();
  const queue = createWhiteQueue(changed);
  queue.push(item()); queue.push(item('2'));
  queue.dispose(); queue.dispose(); changed.mockClear();
  queue.push(item('3'));
  expect(vi.getTimerCount()).toBe(0);
  vi.advanceTimersByTime(30_000);
  expect(changed).not.toHaveBeenCalled();
});

it('queued_streak_uses_the_push_time_value', () => {
  const changed = vi.fn();
  const queue = createWhiteQueue(changed);
  const waiting = item('2');
  queue.push(item()); queue.push(waiting);
  waiting.streak!.runs = 0;
  vi.advanceTimersByTime(6_000);
  expect(changed).toHaveBeenLastCalledWith({ current: item('2'), overflow: 0 });
});

it('drops_present_at_mount_never_banner', () => {
  const present = [drop('old'), drop('unknown', null), drop('ordinary', 1)];
  const seen = new Set(present.map(value => value.id));
  const changed = vi.fn();
  const queue = createWhiteQueue(changed);
  for (const value of newWhites(present, seen)) queue.push(item(value.id));
  expect(changed).not.toHaveBeenCalled();
  expect(newWhites([...present, drop('new'), drop('new'), drop('non-white', 1)], seen).map(value => value.id)).toEqual(['new']);
  expect(newWhites([drop('new')], seen)).toEqual([]);
  // The area/scope owner reseeds every present ID, even bags with unknown rank.
  const nextArea = [drop('next-area'), drop('unknown')];
  const nextSeen = new Set(nextArea.map(value => value.id));
  expect(newWhites(nextArea, nextSeen)).toEqual([]);
});

it('banner_is_a_polite_status', () => {
  const page = render(WhiteBanner, { ...props, state: { current: item(), overflow: 0 } });
  expect(page.getByRole('status').getAttribute('aria-live')).toBe('polite');
  expect(page.getByRole('status').getAttribute('aria-atomic')).toBe('true');
  expect(page.getByText('White bag')).toBeTruthy();
  expect(page.getByText('Ended a 23-run Synthetic dungeon dry streak')).toBeTruthy();
  expect(page.getByText('Synthetic boss (estimated) · 7:00 AM')).toBeTruthy();
});

it('banner_links_to_item_and_renders_both_sprites', () => {
  const page = render(WhiteBanner, { ...props, state: { current: item(), overflow: 0 } });
  expect(page.getByRole('link', { name: /Synthetic item 1/ }).getAttribute('href')).toBe('#/loot/item/101');
  const pictures = page.getAllByRole('img');
  expect(pictures).toHaveLength(2);
  expect(pictures.map(picture => picture.querySelector('img')?.getAttribute('src')))
    .toEqual(visuals.objects.slice().reverse().map(object => props.resolveSpriteUrl(object.sprite)));
});

it('first_white_copy_and_realm_without_streak', async () => {
  const page = render(WhiteBanner, { ...props, state: { current: { ...item(), streak: { kind: 'first', runs: 46 } }, overflow: 0 } });
  expect(page.getByText('No Synthetic dungeon white in 46 runs before this')).toBeTruthy();
  await page.rerender({ state: { current: { ...item(), streak: null }, overflow: 0 } });
  expect(page.container.textContent).not.toMatch(/dry streak|runs before this/);
});

it('missing_item_source_time_and_sprites_stay_unknown', () => {
  const page = render(WhiteBanner, { ...props, visuals: emptyHomeVisuals(), state: {
    current: { ...item(), itemType: null, itemName: '—', sourceName: null, at: 'invalid', streak: null }, overflow: 0 } });
  expect(page.queryByRole('link')).toBeNull();
  expect(page.getByText('— · —')).toBeTruthy();
  expect(page.getAllByRole('img').every(picture => picture.getAttribute('data-fallback') === 'true')).toBe(true);
  expect(page.container.textContent).not.toMatch(/NaN|undefined/);
});

it('reduced_motion_disables_the_banner_entrance_and_nothing_loops_or_glows', () => {
  // jsdom cannot emulate CSS media queries; guard the stylesheet contract.
  const source = readFileSync('src/lib/home/v4/WhiteBanner.svelte', 'utf8');
  expect(source).toMatch(/@media\s*\(prefers-reduced-motion:\s*reduce\)\s*\{\s*\.white-banner\s*\{\s*animation:\s*none;/);
  expect(source).not.toMatch(/infinite|--glow-white|box-shadow/);
});
