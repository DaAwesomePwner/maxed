import { readFileSync } from 'node:fs';
import { cleanup, render, within } from '@testing-library/svelte';
import { writable } from 'svelte/store';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { HomeHud } from '../../src/lib/bindings/HomeHud';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { spriteUrl } from '../../src/lib/bridge/visuals';
import { itemHoverParams } from '../../src/lib/components/item-hover';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import SessionHud from '../../src/lib/home/v4/SessionHud.svelte';
import { overflow, rankText, sessionClock, signedPercent } from '../../src/lib/home/v4/hud';
import { productionFeatures, routeHref } from '../../src/lib/navigation/routes';
import { SHELL_CONNECTION, type ShellConnection } from '../../src/lib/shell/context';
import Shell from '../../src/lib/shell/Shell.svelte';
import Home from '../../src/routes/Home.svelte';
import HomePreview from '../preview/HomePreview.svelte';
import { homeSnapshot } from '../home/fixtures';
import { fakeHistory } from '../navigation/fake-history';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2026-10-12T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const settle = () => vi.advanceTimersByTimeAsync(0);
const hud: HomeHud = { fame_per_hour: 1234, combat: { fights: 5, weighted_dps: 138,
  weighted_party_dps: 100, vs_party: 0.38, avg_rank: 2.6 }, whites: 2,
  ut_st: [101, 102, 103, 104, 105], ut_st_count: 5,
  potions: [{ stat: 'life', object_type: 201, count: 7 }, { stat: 'mana', object_type: null, count: 5 }, { stat: 'attack', object_type: null, count: 3 },
    { stat: 'speed', object_type: null, count: 2 }, { stat: 'wisdom', object_type: null, count: 1 }] };
const sprite: SpriteRef = { key: 'synthetic', sheet: 'mapObjects.png', x: 0, y: 0, width: 8, height: 8 };
const visuals = { ...emptyHomeVisuals(), client_fingerprint: 'a'.repeat(64), objects: [
  ...hud.ut_st.map(object_type => ({ object_type, name: `Synthetic item ${object_type}`, sprite, bag: null })),
  { object_type: 201, name: 'Synthetic catalog label', sprite, bag: null },
] };
const props = { hud, sessionStarted: '0', nowMs: 5_400_000, visuals,
  resolveSpriteUrl: (s: SpriteRef) => spriteUrl(visuals.client_fingerprint, s)!,
  lootHref: routeHref({ kind: 'loot', view: 'feed' }) };

it('hero_and_hud_values_read_once', async () => {
  window.history.replaceState(null, '', '/?preview=home-v4-dungeon#/');
  const page = render(HomePreview);
  await vi.advanceTimersByTimeAsync(500);
  const main = page.getByRole('main');
  for (const node of main.querySelectorAll('[aria-labelledby]')) {
    for (const id of node.getAttribute('aria-labelledby')!.split(/\s+/)) {
      expect(document.getElementById(id)?.classList.contains('sr-only')).not.toBe(true);
    }
  }
  for (const [selector, label] of [['.dps', 'Your DPS'], ['.rank', 'Average rank'], ['.v4-clock strong', 'Run time']]) {
    const value = page.container.querySelector(selector)!;
    expect(value).toBeTruthy();
    const sibling = value.previousElementSibling!;
    expect(sibling.classList.contains('sr-only')).toBe(true);
    expect(sibling.textContent).toBe(label);
    const readable = document.createElement('div');
    readable.append(sibling.cloneNode(true), ' ', value.cloneNode(true));
    readable.querySelectorAll('[aria-hidden="true"]').forEach(node => node.remove());
    expect(readable.textContent!.split(label)).toHaveLength(2);
    expect(readable.textContent!.replace(/\s+/g, ' ')).toContain(`${label} ${value.textContent!.trim()}`);
    expect(value.hasAttribute('role')).toBe(false);
    expect(value.hasAttribute('aria-labelledby')).toBe(false);
  }
});

it('hud_shows_dashes_without_data', () => {
  const page = render(SessionHud, { ...props, hud: null, sessionStarted: null });
  const items = page.container.querySelectorAll('[data-hud]');
  expect(Array.from(items, item => item.getAttribute('data-hud')))
    .toEqual(['fame', 'combat', 'whites', 'ut-st', 'pots', 'session']);
  for (const item of items) expect(item.textContent).toContain('—');
  expect(page.container.textContent).not.toMatch(/NaN|undefined|error|\b0\b/i);
});

it('hud_formats_vs_party_and_rank', () => {
  const page = render(SessionHud, props);
  const combat = page.container.querySelector('[data-hud="combat"]')!;
  expect(combat.textContent).toContain('You vs party avg');
  expect(combat.textContent).toContain('+38%');
  expect(combat.textContent).toContain('#2.6');
  expect(page.container.querySelector('.rank')?.textContent).toBe('#2.6');
  expect(combat.querySelector('a, button')).toBeNull();
  expect(page.container.querySelector('[data-hud="session"]')?.textContent).toContain('1:30');
});

it('hud_comparison_uses_delta_colours_and_standard_focus', async () => {
  const page = render(SessionHud, props);
  expect(page.getByText('+38%').classList.contains('positive')).toBe(true);
  await page.rerender({ hud: { ...hud, combat: { ...hud.combat!, vs_party: -0.05 } } });
  expect(page.getByText('−5%').classList.contains('negative')).toBe(true);
  await page.rerender({ hud: null });
  expect(page.container.querySelector('strong.positive, strong.negative')).toBeNull();
  const source = readFileSync('src/lib/home/v4/SessionHud.svelte', 'utf8');
  expect(source).toMatch(/\.positive\s*\{[^}]*color:\s*var\(--success\)/);
  expect(source).toMatch(/\.negative\s*\{[^}]*color:\s*var\(--danger\)/);
  expect(source).toMatch(/a:focus-visible\s*\{[^}]*outline: 2px solid var\(--focus-ring\)/);
  expect(source).not.toContain('var(--party-avg)');
});

it.each([0, .004, -.004])('hud_rounded_zero_is_neutral: %s', vs_party => {
  const page = render(SessionHud, { ...props, hud: { ...hud, combat: { ...hud.combat!, vs_party } } });
  const value = page.getByText('0%');
  expect(value.classList.contains('positive')).toBe(false);
  expect(value.classList.contains('negative')).toBe(false);
});

it('hud_caps_ut_st_at_three_with_overflow', async () => {
  const page = render(SessionHud, props);
  const items = page.container.querySelector('[data-hud="ut-st"]')!;
  expect(items.querySelectorAll('[role="img"]')).toHaveLength(3);
  expect(items.textContent).toContain('+2');
  const hover = items.querySelector('[data-item-hover="101"]')!;
  expect(itemHoverParams(hover)?.type).toBe(101);
  expect(hover.closest('a')?.href).toContain('#/loot');
  expect(items.querySelector('img')?.getAttribute('src')).toBe(props.resolveSpriteUrl(sprite));
  await page.rerender({ hud: { ...hud, ut_st_count: 20 } });
  expect(items.textContent).toContain('+17');
});

it('hud_pots_show_top_three_and_sum_rest', () => {
  const page = render(SessionHud, { ...props, hud: { ...hud, potions: [...hud.potions].reverse() } });
  const pots = page.container.querySelector('[data-hud="pots"]')!;
  for (const value of ['7', '5', '3', '+3']) expect(within(pots as HTMLElement).getByText(value)).toBeTruthy();
  expect(within(pots as HTMLElement).getByRole('img', { name: 'Life' })).toBeTruthy();
  expect(pots.textContent).toContain('Mana');
  expect(pots.textContent).toContain('Attack');
  expect(pots.textContent).not.toMatch(/Speed|Wisdom/);
});

it('hud_loot_links_to_loot', () => {
  const page = render(SessionHud, props);
  for (const key of ['whites', 'ut-st', 'pots']) {
    const item = page.container.querySelector(`[data-hud="${key}"]`)!;
    const links = item.matches('a') ? [item] : Array.from(item.querySelectorAll('a'));
    expect(links.length).toBeGreaterThan(0);
    for (const link of links) expect(link.getAttribute('href')).toBe('#/loot');
  }
});

it('hud_distinguishes_known_zero_counts_from_missing_values', () => {
  const page = render(SessionHud, { ...props, hud: { fame_per_hour: null, combat: null,
    whites: 0, ut_st: [], ut_st_count: 0, potions: [] } });
  for (const key of ['whites', 'ut-st', 'pots']) {
    expect(page.container.querySelector(`[data-hud="${key}"]`)?.textContent).toContain('0');
  }
});

it('formats_signed_values_ranks_overflow_and_session_clock', () => {
  expect([signedPercent(0.38), signedPercent(-0.05), signedPercent(null), signedPercent(NaN), signedPercent(0)])
    .toEqual(['+38%', '−5%', '—', '—', '0%']);
  expect([rankText(2.6), rankText(null), rankText(Infinity)]).toEqual(['#2.6', '—', '—']);
  expect(overflow([1, 2, 3, 4, 5], 3)).toEqual({ shown: [1, 2, 3], more: 2 });
  expect(overflow([1], 3)).toEqual({ shown: [1], more: 0 });
  expect(sessionClock('1000000', 5_401_000)).toBe('1:30');
  expect(sessionClock(null, 1000)).toBe('—');
  expect(sessionClock('bad', 1000)).toBe('—');
  expect(sessionClock('2000000', 1000)).toBe('—');
});

it('home_renders_hud_only_with_flag', async () => {
  for (const enabled of [false, true]) {
    const client = createClient(createFixtureTransport(homeSnapshot()));
    const read = vi.spyOn(client, 'getHomeHud');
    const page = render(Shell, { client, features: { ...productionFeatures, homeRedesign: enabled },
      preview: true, historyAdapter: fakeHistory('#/').adapter });
    await settle();
    expect(page.container.querySelector('[data-hud="fame"]') !== null).toBe(enabled);
    expect(read.mock.calls.length > 0).toBe(enabled);
    if (enabled) expect(page.container.querySelector('.home')?.firstElementChild?.getAttribute('aria-label')).toBe('Session HUD');
    page.unmount();
  }
});

function homeHarness() {
  const snapshot = homeSnapshot();
  const client = createClient(createFixtureTransport(snapshot));
  const read = vi.spyOn(client, 'getHomeHud').mockResolvedValue({ status: 'ready', value: hud });
  const profile = vi.spyOn(client, 'getDungeonProfile');
  const usual = vi.spyOn(client, 'getHomeBossBaselines');
  const catalog = vi.spyOn(client, 'searchItems').mockImplementation(async query => query === 'Potion of Life'
    ? [{ object_type: 201, name: 'Potion of Life', tier: null }] : []);
  const visualRead = vi.spyOn(client, 'getObjectVisuals');
  const listeners = new Set<(event: EventEnvelope) => void>();
  const homeWindow = writable({ ...emptyHomeLiveWindow(), streamId: snapshot.stream_id,
    runId: snapshot.run!.id, fights: snapshot.fights });
  const connection: ShellConnection = { capture: writable(snapshot.capture), snapshot: writable(snapshot), homeWindow,
    selectHomeFight() {}, reconnect() {}, onEvent(receive) { listeners.add(receive); return () => { listeners.delete(receive); }; } };
  const page = render(Home, { props: { snapshot, client, redesign: true, savedScope: 'today' },
    context: new Map([[SHELL_CONNECTION, connection]]) });
  return { page, snapshot, client, read, profile, usual, catalog, visualRead, listeners, homeWindow };
}

it('home_reads_session_scope_verified_character_and_window_bosses_without_snapshot_polling', async () => {
  const h = homeHarness(); await settle();
  expect(h.read).toHaveBeenCalledExactlyOnceWith({ session: { id: h.snapshot.session!.id } });
  expect(h.profile).toHaveBeenCalledWith(expect.objectContaining({ dungeon: h.snapshot.run!.dungeon,
    character_id: h.snapshot.character!.id }));
  expect(h.usual).toHaveBeenCalledWith(expect.objectContaining({ character_id: h.snapshot.character!.id,
    target_types: [h.snapshot.fights[0].target_type ?? h.snapshot.fights[0].boss_type] }));
  for (let i = 0; i < 5; i++) await h.page.rerender({ snapshot: { ...h.snapshot, at: String(Number(h.snapshot.at) + i) } });
  await vi.advanceTimersByTimeAsync(30_000);
  expect(h.read).toHaveBeenCalledOnce();
  expect(h.catalog).not.toHaveBeenCalled();
  const types = h.visualRead.mock.calls.flatMap(([types]) => types);
  for (const type of [...hud.ut_st, 201, 1292]) expect(types).toContain(type);
  await h.page.rerender({ snapshot: { ...h.snapshot, character: { ...h.snapshot.character!, identity_verified: false } } });
  await settle();
  expect(h.profile).toHaveBeenLastCalledWith(expect.objectContaining({ character_id: null }));
  expect(h.usual).toHaveBeenCalledOnce();
});

it('home_forwards_events_hides_values_on_read_error_and_disposes_on_flag_change', async () => {
  const h = homeHarness(); await settle();
  h.read.mockRejectedValue(new Error('Synthetic read failure'));
  const event: EventEnvelope = { stream_id: h.snapshot.stream_id, sequence: '20', at: h.snapshot.at,
    event: { bag_dropped: h.snapshot.drops[0] } };
  h.listeners.forEach(receive => receive(event));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.read).toHaveBeenCalledTimes(2);
  expect(h.page.container.querySelector('[data-hud="combat"]')?.textContent).not.toContain('+38%');
  expect(h.page.container.querySelector('[data-hud="combat"]')?.textContent).toContain('—');
  expect(h.page.container.querySelector('[data-hud="combat"]')?.textContent).not.toContain('error');
  await h.page.rerender({ redesign: false });
  h.listeners.forEach(receive => receive(event));
  await vi.advanceTimersByTimeAsync(30_000);
  expect(h.read).toHaveBeenCalledTimes(2);
  expect(h.page.container.querySelector('[data-hud]')).toBeNull();
  h.page.unmount();
  expect(h.listeners.size).toBe(0);
});

it('home_includes_event_bosses_but_ignores_ordinary_targets_and_retired_windows', async () => {
  const h = homeHarness(); await settle();
  const fight = h.snapshot.fights[0];
  h.homeWindow.update(window => ({ ...window, fights: [fight,
    { ...fight, id: 'synthetic-event-boss', target_type: 301, target_kind: 'event_boss' },
    { ...fight, id: 'synthetic-ordinary', target_type: 302, target_kind: 'ordinary' }] }));
  await settle();
  expect(h.usual).toHaveBeenLastCalledWith(expect.objectContaining({ target_types: [301] }));
  h.homeWindow.update(window => ({ ...window, streamId: 'retired-synthetic-stream',
    fights: [{ ...fight, target_type: 303 }] }));
  await settle();
  expect(h.usual).toHaveBeenCalledTimes(2);
});

it('home_clock_ticks_every_thirty_seconds_and_uses_the_synthetic_clock', async () => {
  const started = String((Date.now() - 59_000) * 1000);
  const snapshot = homeSnapshot(); snapshot.session!.started = started;
  const page = render(Home, { snapshot, redesign: true }); await settle();
  const clock = () => page.container.querySelector('[data-hud="session"]')?.textContent;
  expect(clock()).toContain('0:00');
  await vi.advanceTimersByTimeAsync(29_999); expect(clock()).toContain('0:00');
  await vi.advanceTimersByTimeAsync(1); expect(clock()).toContain('0:01');
  await page.rerender({ syntheticClock: true, snapshot: { ...snapshot, at: String(Number(started) + 7_200_000_000) } });
  expect(clock()).toContain('2:00');
  await vi.advanceTimersByTimeAsync(60_000); expect(clock()).toContain('2:00');
  page.unmount();
});

it('home_clock_freezes_at_session_end', async () => {
  const snapshot = homeSnapshot();
  snapshot.session!.started = String((Date.now() - 90_000) * 1000);
  const page = render(Home, { snapshot, redesign: true }); await settle();
  const clock = () => page.container.querySelector('[data-hud="session"]')?.textContent;
  expect(clock()).toContain('0:01');
  await page.rerender({ snapshot: { ...snapshot, session: { ...snapshot.session!, ended: String(Date.now() * 1000) } } });
  await vi.advanceTimersByTimeAsync(120_000);
  expect(clock()).toContain('0:01');
});
