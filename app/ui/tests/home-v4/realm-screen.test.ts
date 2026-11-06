import { readFileSync } from 'node:fs';
import { cleanup, fireEvent, render, within } from '@testing-library/svelte';
import type { ComponentProps } from 'svelte';
import { writable } from 'svelte/store';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { HomeBossBaseline } from '../../src/lib/bindings/HomeBossBaseline';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { setDebugMode } from '../../src/lib/debug/debug';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import RealmScreen from '../../src/lib/home/v4/RealmScreen.svelte';
import type { HomeV4ReadState } from '../../src/lib/home/v4/reads';
import * as whiteQueue from '../../src/lib/home/v4/white-queue';
import { SHELL_CONNECTION, type ShellConnection } from '../../src/lib/shell/context';
import Home from '../../src/routes/Home.svelte';
import { catalog, fight, homeSnapshot, realmSnapshot } from '../home-v2/fixtures';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2026-10-12T12:00:00Z')); setDebugMode(false); });
afterEach(() => { cleanup(); setDebugMode(false); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const settle = () => vi.advanceTimersByTimeAsync(0);
function props(): ComponentProps<typeof RealmScreen> {
  const snapshot = realmSnapshot(); snapshot.at = '100000000';
  snapshot.instance!.realm_score = { current: 186000, max: 300000 };
  snapshot.instance!.quest_target = { object_id: 42, object_type: 302, since: '1000000' };
  const live = fight('synthetic-live', { run_id: null, started: '46000000', last_hit: snapshot.at,
    local_last_hit: snapshot.at, total_damage: 100000, unattributed_damage: 10000 });
  live.players = Array.from({ length: 37 }, (_, i) => ({ ...live.players[0],
    object_id: i + 1, player_id: i === 8 ? snapshot.character!.id : `synthetic-player-${i + 1}`, name: `Synthetic ${i + 1}`,
    is_local: i === 8, rank: i + 1, damage: 100000 - i * 1000, dps: i === 8 ? 1500 : 1000, share: .03 }));
  const killed = fight('synthetic-killed', { run_id: null, target_type: 301, target_kind: 'boss', started: '10000000',
    ended: '20000000', kill: { at: '20000000', evidence: 'hp_zero_removed', provisional: false } });
  killed.players[0] = { ...killed.players[0], dps: 1500, rank: 1, share: .5 };
  snapshot.fights = [live, killed];
  const drop = homeSnapshot().drops[0];
  const drops = [
    { ...drop, id: 'linked-best', run_id: null, at: '21000000', bag_rank: 255, source_enemy: 301, source_estimated: true },
    { ...drop, id: 'unlinked-older', run_id: null, at: '5000000', bag_rank: 6, source_enemy: 999 },
    { ...drop, id: 'unlinked-newer', run_id: null, at: '29000000', bag_rank: 1, source_enemy: null },
  ];
  const reads: HomeV4ReadState = { hud: null, streak: null, profile: null,
    usual: new Map([[300, { target_type: 300, target_name: 'Wrong history name', fights: 5, avg_dps: 1000,
      best_dps: 2000, kill_samples: 5, median_kill_micros: 118000000 } as HomeBossBaseline]]),
    failedUsualTypes: new Set(), errors: { hud: false, streak: false, profile: false, usual: false } };
  return { snapshot, stale: false, reads, white: { current: null, overflow: 0 },
    visuals: { ...emptyHomeVisuals(), objects: [catalog(300, 'Synthetic live target'), catalog(301, 'Synthetic killed target'),
      catalog(302, 'Synthetic quest'), catalog(303, 'Synthetic announcement'), catalog(1796, 'Realm Portal')] },
    window: { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, visitKey: 'synthetic-visit', visitStarted: '1000000',
      realmPeriods: [{ started: '1000000', ended: '30000000', fame: 12 }, { started: '40000000', ended: null, fame: 20 }],
      focusedFightId: live.id, fights: snapshot.fights, drops, partial: false, reasons: [],
      realmAnnouncements: [{ id: 'synthetic-notice', session_id: snapshot.session!.id, run_id: null, at: '45000000',
        provisional: true, kind: { type: 'realm_event_killed', object_type: 303, boss_key: 'internal-key', killer: null } }] } };
}

it('hero_live_then_ended_then_quest', async () => {
  const input = props(), page = render(RealmScreen, input);
  const hero = page.getByRole('region', { name: 'Your DPS · Synthetic live target' });
  expect(hero.querySelector('.dps')?.textContent).toBe('1,500');
  expect(within(hero).getByText('▲ 50% above party average')).toBeTruthy();
  expect(within(hero).getByText('▲ 50% vs your usual · avg 1,000 · 5 fights')).toBeTruthy();
  expect(hero.querySelector<HTMLElement>('.plot')?.style.height).toBe('112px');
  expect(page.getByRole('meter', { name: 'Target HP' }).getAttribute('aria-valuenow')).toBe('240');
  const ended = { ...input.snapshot!.fights[0], ended: '99000000', kill: null };
  await page.rerender({ snapshot: { ...input.snapshot!, fights: [ended] }, window: { ...input.window, fights: [ended] } });
  expect(page.getByText('Ended').closest('.ended')).toBeTruthy();
  expect(page.getByRole('region', { name: 'Your DPS · Synthetic live target' })).toBeTruthy();
  await page.rerender({ snapshot: { ...input.snapshot!, fights: [] }, window: { ...input.window, fights: [], focusedFightId: null } });
  expect(page.getByRole('heading', { name: 'Quest: Synthetic quest' })).toBeTruthy();
  expect(page.queryByLabelText('Your DPS')).toBeNull();
});

it('realm_meter_appends_self_outside_top_six', () => {
  const page = render(RealmScreen, props()), table = page.getByRole('table', { name: 'Player Damage' });
  expect(page.getByRole('heading', { name: 'Damage on Synthetic live target' })).toBeTruthy();
  expect(table.querySelectorAll('.damage-row')).toHaveLength(7);
  expect(within(table).getByText('··· 2 players')).toBeTruthy();
  expect(table.querySelector('.self td')?.textContent).toBe('9');
  expect(within(table).queryByText('Synthetic 7')).toBeNull();
  expect(page.getByText(/\+30 more/)).toBeTruthy();
  expect(page.getByText(/10% of damage not attributed to a player/)).toBeTruthy();
  expect(within(table).getByRole('link', { name: /Synthetic 9/ }).getAttribute('href')).toContain('/players/');
  expect(page.getByRole('link', { name: 'Open fight ›' }).getAttribute('href')).toBe('#/fights/synthetic-live');
});

it('realm_progress_is_labelled_score', async () => {
  const input = props(), page = render(RealmScreen, input);
  const bar = page.getByRole('progressbar', { name: 'Realm closing progress' });
  expect(bar.getAttribute('title')).toBe('Realm score, not a closure prediction');
  expect(bar.getAttribute('aria-valuenow')).toBe('62');
  expect(page.getByText('Realm score 186,000 / 300,000 · 62%')).toBeTruthy();
  await page.rerender({ snapshot: { ...input.snapshot!, instance: { ...input.snapshot!.instance!, realm_score: null } } });
  expect(bar.hasAttribute('aria-valuenow')).toBe(false);
  expect(page.getByText('Realm score — / — · —')).toBeTruthy();
});

it('elapsed_times_use_snapshot_capture_clock_and_exclude_time_away', () => {
  const page = render(RealmScreen, props());
  expect(page.getByLabelText('Time here').textContent).toBe('1:29');
  expect(page.getByText('excludes time away')).toBeTruthy();
  expect(within(page.getByRole('region', { name: 'Your DPS · Synthetic live target' })).getByText('0:54')).toBeTruthy();
  expect(page.getByText('Fame this visit +32')).toBeTruthy();
  expect(within(page.getByRole('region', { name: 'Visit timeline' })).getByText('Now: 1:39')).toBeTruthy();
});

it('names_and_sprites_use_catalog_then_fight_type_then_unknown_never_history', async () => {
  const input = props();
  const sprite = { key: 'synthetic', sheet: 'characters.png', x: 0, y: 0, width: 8, height: 8 };
  input.visuals.client_fingerprint = 'a'.repeat(64);
  input.visuals.objects[0].sprite = sprite;
  input.visuals.objects[1].sprite = { ...sprite, x: 8 };
  input.visuals.objects[2].sprite = { ...sprite, x: 16 };
  input.visuals.objects.push({ ...catalog(999, 'Unused'), name: null });
  const page = render(RealmScreen, input);
  const hero = page.getByRole('region', { name: 'Your DPS · Synthetic live target' });
  expect(within(hero).getByRole('img', { name: 'Synthetic live target' }).getAttribute('data-fallback')).toBe('false');
  expect(hero.querySelector('img')?.getAttribute('src')).toContain('/characters.png/0-0-8-8.png');
  expect(within(page.getByRole('region', { name: 'Events this visit' }))
    .getByRole('img', { name: 'Synthetic killed target' }).querySelector('img')?.getAttribute('src'))
    .toContain('/characters.png/8-0-8-8.png');
  const fallback = { ...input.snapshot!.fights[0], target_type: 999, boss_type: 300 };
  await page.rerender({ snapshot: { ...input.snapshot!, fights: [fallback] }, window: { ...input.window, fights: [fallback] } });
  expect(page.getByRole('region', { name: 'Your DPS · Synthetic live target' })).toBeTruthy();
  await page.rerender({ snapshot: { ...input.snapshot!, fights: [] }, window: { ...input.window, fights: [], focusedFightId: null } });
  expect(within(page.getByRole('region', { name: 'Current quest' })).getByRole('img', { name: 'Synthetic quest' })
    .querySelector('img')?.getAttribute('src')).toContain('/characters.png/16-0-8-8.png');
  await page.rerender({ snapshot: { ...input.snapshot!, fights: [fallback] }, window: { ...input.window, fights: [fallback] } });
  await page.rerender({ visuals: emptyHomeVisuals() });
  expect(page.getByRole('region', { name: 'Your DPS · Unknown target' })).toBeTruthy();
  expect(page.container.textContent).not.toContain('Wrong history name');
  await page.rerender({ snapshot: { ...input.snapshot!, fights: [] }, window: { ...input.window, fights: [], focusedFightId: null } });
  expect(page.getByRole('heading', { name: 'Quest: Unknown target' })).toBeTruthy();
});

it('read_errors_hide_usual_values_without_persistent_error_copy', async () => {
  const input = props(), page = render(RealmScreen, input);
  await page.rerender({ reads: { ...input.reads, failedUsualTypes: new Set([300]), errors: { hud: true, streak: true, profile: true, usual: true } } });
  expect(page.queryByText(/vs your usual|learning|error/i)).toBeNull();
  expect(page.container.querySelector('.dps')?.textContent).toBe('1,500');
});

it('failed_usual_types_do_not_hide_a_successful_realm_target_usual', async () => {
  const input = props(), page = render(RealmScreen, input);
  await page.rerender({ reads: { ...input.reads, failedUsualTypes: new Set([301]),
    errors: { ...input.reads.errors, usual: true } } });
  expect(page.getByText('▲ 50% vs your usual · avg 1,000 · 5 fights')).toBeTruthy();
  await page.rerender({ reads: { ...input.reads, failedUsualTypes: new Set([300]),
    errors: { ...input.reads.errors, usual: true } } });
  expect(page.queryByText(/vs your usual/)).toBeNull();
});

it('live_status_remains_mounted_through_recovery', async () => {
  const page = render(RealmScreen, props());
  const status = page.getAllByRole('status').find(node => node.textContent === 'Live')!;
  expect(status?.getAttribute('aria-live')).toBe('polite');
  await page.rerender({ stale: true });
  expect(page.getByText('Recovering')).toBe(status);
  await page.rerender({ stale: false });
  expect(status.textContent).toBe('Live');
  expect(page.getAllByRole('status')).toContain(status);
});

it('events_show_kills_newest_first_linked_bags_and_faded_announcements', async () => {
  const page = render(RealmScreen, props()), events = page.getByRole('region', { name: 'Events this visit' });
  expect(events.querySelector('.event-count')?.textContent).toBe('2');
  expect(Array.from(events.querySelectorAll('[data-event-id]'), row => row.getAttribute('data-event-id')))
    .toEqual(['announcement:synthetic-notice', 'fight:synthetic-killed']);
  const announced = events.querySelector<HTMLElement>('.announced')!;
  expect(within(announced).getByText("Announced · you weren't there")).toBeTruthy();
  expect(announced.textContent).not.toContain('DPS');
  const killed = events.querySelector<HTMLElement>('[data-event-id="fight:synthetic-killed"]')!;
  expect(within(killed).getByText('1,500 DPS · #1 of 1 · 50%')).toBeTruthy();
  const bag = within(killed).getByRole('link', { name: /Bag.*estimated/i });
  await fireEvent.focusIn(bag);
  expect(within(killed).getByRole('tooltip').textContent).toContain('Source estimated');
  expect(events.querySelectorAll('[data-drop-id]')).toHaveLength(1);
  const loot = page.getByRole('region', { name: 'Loot this visit' });
  expect(Array.from(loot.querySelectorAll('[data-drop-id]'), row => row.getAttribute('data-drop-id')))
    .toEqual(['linked-best', 'unlinked-newer', 'unlinked-older']);
});

it('retained_kills_keep_dps_but_unknown_player_counts', () => {
  const input = props(), killed = input.snapshot!.fights[1];
  input.snapshot = { ...input.snapshot!, fights: [input.snapshot!.fights[0]] };
  input.window = { ...input.window, fights: input.snapshot.fights,
    realmKills: [{ ...killed, self: killed.players[0], partial: false }] };
  const page = render(RealmScreen, input);
  expect(within(page.getByRole('region', { name: 'Events this visit' })).getByText('1,500 DPS · #1 of — · 50%')).toBeTruthy();
});

it('dense_linked_bags_group_by_type_without_mixing_estimated_sources', async () => {
  const input = props();
  input.window.drops = Array.from({ length: 80 }, (_, index) => ({ ...input.window.drops[0],
    id: `synthetic-linked-${index}`, source_estimated: index < 40 }));
  const page = render(RealmScreen, input), events = page.getByRole('region', { name: 'Events this visit' });
  expect(within(events).getAllByRole('link')).toHaveLength(2);
  const estimated = within(events).getByRole('link', { name: '40 × Bag (estimated)' });
  expect(within(events).getByRole('link', { name: '40 × Bag' })).toBeTruthy();
  await fireEvent.focusIn(estimated);
  expect(within(events).getByRole('tooltip').textContent).toContain('40 × Bag');
  expect(within(events).getByRole('tooltip').textContent).toContain('Source estimated');
  expect(page.getByRole('region', { name: 'Loot this visit' }).querySelectorAll('[data-drop-id]')).toHaveLength(80);
});

it('visit_timeline_has_event_spans_linked_bag_pips_hollow_announcements_and_now', () => {
  const page = render(RealmScreen, props()), timeline = page.getByRole('region', { name: 'Visit timeline' });
  expect(timeline.querySelectorAll('.event-span')).toHaveLength(2);
  expect(timeline.querySelectorAll('.bag-pip')).toHaveLength(1);
  expect(timeline.querySelectorAll('.announced-diamond')).toHaveLength(1);
  expect(timeline.querySelector('[data-now]')).toBeTruthy();
  expect(within(timeline).getByRole('link', { name: /Synthetic killed target/ }).getAttribute('href')).toBe('#/fights/synthetic-killed');
});

it('sequential_timeline_events_share_a_lane_while_overlapping_events_do_not', () => {
  const input = props(), killed = input.snapshot!.fights[1];
  const overlap = { ...killed, id: 'synthetic-overlap', started: '15000000', ended: '25000000',
    kill: { ...killed.kill!, at: '25000000' } };
  input.window.fights = [...input.window.fights, overlap];
  const page = render(RealmScreen, input), timeline = page.getByRole('region', { name: 'Visit timeline' });
  const spans = Array.from(timeline.querySelectorAll<HTMLElement>('.event-span'));
  const top = (id: string) => spans.find(span => span.getAttribute('href') === `#/fights/${id}`)!.style.top;
  expect(top('synthetic-live')).toBe(top('synthetic-killed'));
  expect(top('synthetic-overlap')).not.toBe(top('synthetic-killed'));
});

it('debug_evidence_and_290px_responsive_grid_follow_shared_screen_patterns', async () => {
  const page = render(RealmScreen, props());
  expect(page.queryByText('Visit evidence')).toBeNull();
  setDebugMode(true); await settle();
  expect(page.getByText('Visit evidence')).toBeTruthy();
  const css = readFileSync('src/lib/home/v4/screens.css', 'utf8');
  expect(css).toMatch(/\.v4-realm-grid\s*\{[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\) 290px/);
  expect(css).toMatch(/@media\s*\(max-width:\s*950px\)[\s\S]*\.v4-realm-grid[^}]*grid-template-columns:\s*minmax\(0,\s*1fr\)/);
});

function homeHarness(redesign = true, input = props()) {
  const snapshot = input.snapshot!;
  const client = createClient(createFixtureTransport(snapshot));
  const history = vi.spyOn(client, 'getHomeBossBaselines');
  const visuals = vi.spyOn(client, 'getObjectVisuals').mockImplementation(async types => ({ ...input.visuals,
    objects: input.visuals.objects.filter(object => types.includes(object.object_type)) }));
  const homeWindow = writable(input.window);
  const factory = vi.spyOn(whiteQueue, 'createWhiteQueue');
  const connection: ShellConnection = { capture: writable(snapshot.capture), snapshot: writable(snapshot), homeWindow,
    selectHomeFight() {}, reconnect() {} };
  const page = render(Home, { props: { snapshot, client, redesign, syntheticClock: true },
    context: new Map([[SHELL_CONNECTION, connection]]) });
  return { page, homeWindow, factory, input, history, visuals };
}

it('home_requests_the_fights_fallback_catalog_type', async () => {
  const input = props(); input.snapshot!.fights[0].target_type = 999;
  const h = homeHarness(true, input); await settle();
  expect(h.visuals.mock.calls.flatMap(([types]) => types)).toContain(300);
  expect(h.page.getByRole('region', { name: 'Your DPS · Synthetic live target' })).toBeTruthy();
});

it.each(['quest', 'miniboss'] as const)('home_reads_usual_for_realm_%s_without_polling_on_metrics', async kind => {
  const input = props();
  input.snapshot!.fights[0].target_kind = kind;
  const h = homeHarness(true, input); await settle();
  expect(h.history.mock.calls.some(([request]) => request.target_types?.includes(300))).toBe(true);
  h.history.mockClear();
  h.homeWindow.update(window => ({ ...window, fights: window.fights.map(fight => ({ ...fight, total_damage: fight.total_damage + 1 })) }));
  await settle(); expect(h.history).not.toHaveBeenCalled();
});

it('away_drops_are_excluded_and_idle_focus_is_dimmed_without_claiming_a_kill', async () => {
  const input = props();
  input.window = { ...input.window, drops: [...input.window.drops, { ...input.window.drops[0], id: 'away', at: '35000000' }] };
  const page = render(RealmScreen, input);
  expect(page.container.querySelector('[data-drop-id="away"]')).toBeNull();
  await page.rerender({ snapshot: { ...input.snapshot!, at: '120000000' } });
  expect(page.getByText('Ended').closest('.ended')).toBeTruthy();
  expect(page.queryByText(/Defeated/)).toBeNull();
  expect(page.getByRole('region', { name: 'Events this visit' }).querySelector('.event-count')?.textContent).toBe('2');
});

it('new_realm_white_banners_with_no_streak_line_in_persistent_live_region', async () => {
  const h = homeHarness(); await settle();
  const region = h.page.container.querySelector('.white-banner-region') as HTMLElement;
  expect(region).toBeTruthy(); expect(region.textContent).toBe('');
  h.homeWindow.update(window => ({ ...window, drops: [...window.drops, { ...window.drops[0], id: 'synthetic-new-white' }] }));
  await settle();
  expect(within(region).getByText('WHITE BAG')).toBeTruthy();
  expect(region.getAttribute('aria-live')).toBe('polite');
  expect(region.textContent).not.toMatch(/dry streak|runs before this/);
  expect(h.page.container.querySelector('.v4-hero-content')?.getAttribute('aria-hidden')).toBe('true');
  await vi.advanceTimersByTimeAsync(6000);
  expect(h.page.container.querySelector('.white-banner-region')).toBe(region);
  expect(region.textContent).toBe('');
});

it('home_flag_off_keeps_existing_realm_without_white_queue', async () => {
  const h = homeHarness(false); await settle();
  expect(h.page.container.querySelector('.realm-home')).toBeTruthy();
  expect(h.page.container.querySelector('.realm-screen')).toBeNull();
  expect(h.factory).not.toHaveBeenCalled();
});
