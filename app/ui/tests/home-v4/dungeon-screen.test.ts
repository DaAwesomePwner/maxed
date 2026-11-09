import { readFileSync } from 'node:fs';
import { cleanup, fireEvent, render, within } from '@testing-library/svelte';
import type { ComponentProps } from 'svelte';
import { writable } from 'svelte/store';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { DropView } from '../../src/lib/bindings/DropView';
import type { HomeDungeonStreak } from '../../src/lib/bindings/HomeDungeonStreak';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { setDebugMode } from '../../src/lib/debug/debug';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import DungeonScreen from '../../src/lib/home/v4/DungeonScreen.svelte';
import type { HomeV4ReadState } from '../../src/lib/home/v4/reads';
import * as streaks from '../../src/lib/home/v4/streaks';
import * as whiteQueue from '../../src/lib/home/v4/white-queue';
import { SHELL_CONNECTION, type ShellConnection } from '../../src/lib/shell/context';
import Home from '../../src/routes/Home.svelte';
import { deferred, homeSnapshot } from '../home/fixtures';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2026-10-12T12:00:00Z')); setDebugMode(false); });
afterEach(() => { cleanup(); setDebugMode(false); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const settle = () => vi.advanceTimersByTimeAsync(0);
const visuals = { ...emptyHomeVisuals(), phase: 'ready' as const, objects: [
  { object_type: 300, name: 'Synthetic Starwarden', sprite: null, bag: null },
  { object_type: 301, name: 'Synthetic upcoming', sprite: null, bag: null },
  { object_type: 200, name: 'Synthetic treasure', sprite: null, bag: null },
  { object_type: 777, name: 'Synthetic new treasure', sprite: null, bag: null },
  { object_type: 888, name: 'Synthetic new source', sprite: null, bag: null },
] };
const dungeonStreak = (runs_since_last_white: number | null = 23): HomeDungeonStreak => ({
  // These successful banner replies include the fixture white; lag is tested in reads.test.ts.
  dungeon: 'Synthetic Sanctum', runs_ended: 46, runs_since_last_white, avg_runs_per_white: 23, last_white_at: homeSnapshot().drops[0].at,
});
function reads(): HomeV4ReadState {
  return { hud: null, streak: { generation: streaks.dungeonGeneration(snapshot())!, value: dungeonStreak() },
    profile: { dungeon: 'Synthetic Sanctum', clears: 10, usual_run_dps: 1000, run_dps_samples: 5,
      usual_clear_micros: 605_000_000, clear_samples: 5,
      bosses: [300, 301].map(target_type => ({ target_type, target_name: null, seen_in: 8,
        median_offset_micros: 100_000_000, median_kill_micros: 118_000_000, kill_samples: 5, best_dps: 2000 })) },
    usual: new Map([[300, { target_type: 300, fights: 5, avg_dps: 1000,
      best_dps: 2000, kill_samples: 5, median_kill_micros: 118_000_000 }]]),
    failedUsualTypes: new Set(), errors: { hud: false, streak: false, profile: false, usual: false } };
}
function snapshot(selfRank = 9, count = 12): LiveSnapshot {
  const value = homeSnapshot({ at: '15000000' });
  const original = value.fights[0].players[0];
  value.fights[0].players = Array.from({ length: count }, (_, i) => ({ ...original,
    object_id: i + 1, player_id: i + 1 === selfRank ? value.character!.id : `synthetic-player-${i + 1}`,
    name: `Synthetic ${i + 1}`, is_local: i + 1 === selfRank, rank: i + 1,
    damage: (count - i) * 1000, dps: i + 1 === selfRank ? 1500 : 1000, share: 0.1,
  }));
  value.fights[0].target_hp = 5000; value.fights[0].max_hp = 10000;
  value.fights[0].total_damage = 100000; value.fights[0].unattributed_damage = 10000;
  value.fights[0].buckets = [{ from: '2000000', until: '15000000', local: 19500,
    total: 33800, others_damage: 14300, others_presence: 14.3 }];
  value.run_totals = { fights: 2, kills: 1, total_damage: 200000, unattributed_damage: 20000,
    players: value.fights[0].players.map(row => ({ ...row, dps: row.is_local ? 1250 : 900 })), fame_delta: 25, partial: false };
  return value;
}
function props(): ComponentProps<typeof DungeonScreen> {
  return { snapshot: snapshot(), stale: false, visuals, portalType: null, liveWindow: null,
    dungeonBests: [{ dungeon: 'Synthetic Sanctum', best_clear_micros: 560_000_000 }], reads: reads(),
    endedStreaks: streaks.createEndedStreaks(), white: { current: null, overflow: 0 } };
}
const newDrop = (value: LiveSnapshot, id: string, bag_rank: number | null = 255): DropView => ({
  ...value.drops[0], id, run_id: value.run?.id ?? null, session_id: value.session!.id,
  bag_rank, source_enemy: 300, source_estimated: true,
});

it('hero_follows_live_boss_fight', () => {
  const input = props(), ended = { ...input.snapshot!.fights[0], id: 'synthetic-ended', ended: '10000000' };
  input.liveWindow = { ...emptyHomeLiveWindow(), streamId: input.snapshot!.stream_id,
    runId: input.snapshot!.run!.id, focusedFightId: ended.id, fights: [ended, ...input.snapshot!.fights] };
  const page = render(DungeonScreen, input);
  const hero = page.getByRole('region', { name: 'Your DPS · Synthetic Starwarden' });
  expect(hero.querySelector('.dps')?.textContent).toBe('1,500');
  expect(within(hero).getByText('▲ 50% above party average')).toBeTruthy();
  expect(within(hero).getByText('▲ 50% vs your usual · avg 1,000 · 5 fights')).toBeTruthy();
  expect(hero.querySelector<HTMLElement>('.plot')?.style.height).toBe('112px');
  expect(page.getByText('Boss 1 of 2')).toBeTruthy();
  expect(page.getByRole('meter', { name: 'Target HP' }).getAttribute('aria-valuenow')).toBe('5000');
  expect(page.getByText('Synthetic upcoming')).toBeTruthy();
});

it('hero_shows_whole_run_between_bosses', () => {
  const input = props(); input.snapshot!.fights[0].ended = '14000000';
  input.snapshot!.fights.push({ ...input.snapshot!.fights[0], id: 'synthetic-ordinary', target_kind: 'ordinary', ended: null });
  const page = render(DungeonScreen, input);
  const hero = page.getByRole('region', { name: 'Your DPS · Whole run' });
  expect(hero.querySelector('.dps')?.textContent).toBe('1,250');
  expect(within(hero).getByText('No live fight')).toBeTruthy();
  expect(within(hero).queryByText('Party average')).toBeNull();
  expect(within(hero).getByText('▲ 25% vs your usual · avg 1,000 · 5 fights')).toBeTruthy();
  expect(page.queryByRole('meter', { name: 'Target HP' })).toBeNull();
});

it('failed_usual_types_do_not_hide_a_successful_boss_usual', async () => {
  const input = props(), page = render(DungeonScreen, input);
  await page.rerender({ reads: { ...input.reads, failedUsualTypes: new Set([301]),
    errors: { ...input.reads.errors, usual: true } } });
  expect(page.getByText('▲ 50% vs your usual · avg 1,000 · 5 fights')).toBeTruthy();
  await page.rerender({ reads: { ...input.reads, failedUsualTypes: new Set([300]),
    errors: { ...input.reads.errors, usual: true } } });
  expect(page.queryByText(/vs your usual/)).toBeNull();
});

it('live_status_remains_mounted_through_recovery_and_end', async () => {
  const input = props(), page = render(DungeonScreen, input);
  const status = page.getAllByRole('status').find(node => node.textContent === 'Live')!;
  expect(status?.getAttribute('aria-live')).toBe('polite');
  await page.rerender({ stale: true });
  expect(page.getByText('Recovering')).toBe(status);
  await page.rerender({ stale: false, snapshot: { ...input.snapshot!, run: { ...input.snapshot!.run!, ended: '15000000' } } });
  expect(page.getByText('Ended')).toBe(status);
});

it('meter_appends_self_outside_top_six', () => {
  const page = render(DungeonScreen, props());
  const table = page.getByRole('table', { name: 'Player Damage' });
  expect(table.querySelectorAll('.damage-row')).toHaveLength(7);
  expect(within(table).getByText('··· 2 players')).toBeTruthy();
  expect(table.querySelector('.self td')?.textContent).toBe('9');
  expect(within(table).queryByText('Synthetic 7')).toBeNull();
  expect(page.getByText(/\+5 more/)).toBeTruthy();
  expect(page.getByText(/10% of damage not attributed to a player/)).toBeTruthy();
  expect(within(table).getByRole('link', { name: /Synthetic 9/ }).getAttribute('href')).toContain('/players/');
});

it('meter_switches_to_whole_run_without_changing_the_hero_or_losing_distant_self', async () => {
  const input = props(); input.snapshot = snapshot(20, 24); input.onSelectFight = vi.fn();
  const page = render(DungeonScreen, input);
  await fireEvent.click(page.getByRole('button', { name: 'Whole run' }));
  expect(page.getByRole('region', { name: 'Your DPS · Synthetic Starwarden' })).toBeTruthy();
  const table = page.getByRole('table', { name: 'Player Damage' });
  expect(table.querySelector('.self td')?.textContent).toBe('20');
  expect(within(table).getByText('··· 13 players')).toBeTruthy();
  expect(within(table).getByRole('link', { name: /Synthetic 20/ }).getAttribute('href')).toBe('#/runs/demo-run/players/demo-character');
  await fireEvent.click(page.getByRole('button', { name: 'This fight' }));
  expect(input.onSelectFight).toHaveBeenCalledWith('demo-fight');
  expect(page.getByRole('link', { name: 'Open fight ›' }).getAttribute('href')).toBe('#/runs/demo-run/fights/demo-fight');
});

it('header_shows_usual_clear_and_pb_only_when_known', async () => {
  const input = props(), page = render(DungeonScreen, input);
  expect(page.getByText('usual clear 10:05 · PB 9:20')).toBeTruthy();
  expect(page.container.querySelector('.v4-clock strong')?.textContent).toBe('0:14');
  await page.rerender({ reads: { ...input.reads, profile: null } });
  expect(page.getByText('PB 9:20')).toBeTruthy();
  await page.rerender({ reads: input.reads, dungeonBests: null });
  expect(page.getByText('usual clear 10:05')).toBeTruthy();
  await page.rerender({ reads: { ...input.reads, profile: null } });
  expect(page.container.textContent).not.toMatch(/usual clear|PB/);
});

it('boss_elapsed_uses_snapshot_capture_clock_and_catalog_names', () => {
  const page = render(DungeonScreen, props());
  const bosses = page.getByRole('region', { name: 'Bosses' });
  expect(within(bosses).getByText('Elapsed 0:13')).toBeTruthy();
  expect(within(bosses).getByText('Synthetic Starwarden')).toBeTruthy();
  expect(within(bosses).getByText('Synthetic upcoming')).toBeTruthy();
});

it('read_errors_hide_stale_values_without_error_copy', async () => {
  const input = props(), page = render(DungeonScreen, input);
  await page.rerender({ reads: { ...input.reads, failedUsualTypes: new Set([300]), errors: { hud: true, streak: true, profile: true, usual: true } } });
  expect(page.queryByText(/usual clear|vs your usual|usually|average gap|dry streak 23|error/i)).toBeNull();
  expect(page.getByText('Dry streak —')).toBeTruthy();
  expect(page.queryByText('Synthetic upcoming')).toBeNull();
  expect(page.getByText('PB 9:20')).toBeTruthy();
});

it('white_banner_covers_hero_while_active', async () => {
  const page = render(DungeonScreen, props());
  const region = page.container.querySelector('.white-banner-region')!;
  expect(region.textContent).toBe('');
  await page.rerender({ white: { current: { dropId: 'synthetic-white', itemType: 200, itemName: 'Synthetic treasure',
    bagType: 400, areaName: 'Synthetic Sanctum', sourceName: 'Synthetic Starwarden (estimated)', at: '15000000',
    streak: { kind: 'ended', runs: 23 } }, overflow: 0 } });
  expect(page.queryByRole('region', { name: 'Your DPS · Synthetic Starwarden' })).toBeNull();
  expect(page.getByText('WHITE BAG')).toBeTruthy();
  expect(region.closest('.v4-hero')).toBeTruthy();
  expect(page.getByRole('link', { name: 'Synthetic treasure' }).getAttribute('href')).toBe('#/loot/item/200');
  await page.rerender({ white: { current: null, overflow: 0 } });
  expect(page.container.querySelector('.white-banner-region')).toBe(region);
  expect(region.textContent).toBe('');
  expect(page.getByRole('region', { name: 'Your DPS · Synthetic Starwarden' })).toBeTruthy();
});

it('debug_evidence_present_in_debug_mode', async () => {
  const page = render(DungeonScreen, props());
  expect(page.queryByText('Run evidence')).toBeNull();
  setDebugMode(true); await settle();
  expect(page.getByText('Run evidence')).toBeTruthy();
  expect(page.getByText(/Ranks and shares are supplied/)).toBeTruthy();
});

it('loot_starts_with_dry_streak_then_best_and_newest_other_bags_and_timeline_has_no_pb', () => {
  const input = props(), value = input.snapshot!;
  value.drops = [newDrop(value, 'best'), { ...newDrop(value, 'older', 6), at: '6000000' },
    { ...newDrop(value, 'newer', 1), at: '9000000' }];
  const page = render(DungeonScreen, input), loot = page.getByRole('region', { name: 'Loot' });
  expect(loot.children[1].classList.contains('dry-streak-row')).toBe(true);
  expect(Array.from(loot.querySelectorAll('[data-drop-id]'), node => node.getAttribute('data-drop-id')))
    .toEqual(['best', 'newer', 'older']);
  expect(page.getByRole('region', { name: 'Timeline' }).textContent).not.toContain('PB');
  const css = readFileSync('src/lib/home/v4/screens.css', 'utf8');
  expect(css).toMatch(/grid-template-columns:\s*minmax\(0,\s*1fr\) 262px/);
  expect(css).toMatch(/@media\s*\(max-width:\s*950px\)/);
});

function homeHarness(initial = snapshot(), stats = dungeonStreak(), redesign = true) {
  const client = createClient(createFixtureTransport(initial));
  vi.spyOn(client, 'getLootStats');
  vi.spyOn(client, 'getHomeHud');
  vi.spyOn(client, 'getBossHistory');
  vi.spyOn(client, 'getHomeBossBaselines');
  vi.spyOn(client, 'getHomeDungeonStreak').mockResolvedValue({ status: 'ready', value: stats });
  vi.spyOn(client, 'getObjectVisuals').mockImplementation(async types => ({ ...visuals,
    objects: visuals.objects.filter(object => types.includes(object.object_type)) }));
  vi.spyOn(client, 'getDungeonProfile').mockResolvedValue({ status: 'ready', value: reads().profile! });
  const factory = vi.spyOn(whiteQueue, 'createWhiteQueue');
  const ended = streaks.createEndedStreaks();
  const record = vi.spyOn(ended, 'record'), reset = vi.spyOn(ended, 'reset');
  vi.spyOn(streaks, 'createEndedStreaks').mockReturnValue(ended);
  const homeWindow = writable({ ...emptyHomeLiveWindow(), streamId: initial.stream_id,
    visitKey: 'synthetic-entry', runId: initial.run?.id ?? null, fights: initial.fights, drops: initial.drops });
  const connection: ShellConnection = { capture: writable(initial.capture), snapshot: writable(initial), homeWindow,
    selectHomeFight() {}, reconnect() {} };
  const page = render(Home, { props: { snapshot: initial, client, redesign, syntheticClock: true },
    context: new Map([[SHELL_CONNECTION, connection]]) });
  const drop = (id: string, bag_rank: number | null = 255) => homeWindow.update(window => ({ ...window,
    drops: [...window.drops, newDrop(initial, id, bag_rank)] }));
  return { page, client, homeWindow, factory, drop, connection, record, reset };
}

it.each([[23, 0], [23, 23], [null, 0], [null, null]])(
  'home_only_first_white_gets_streak_when_read_changes_from_%s_to_%s', async (before, after) => {
    const value = snapshot(), h = homeHarness(value, dungeonStreak(before)); await settle();
    h.drop('first'); await settle();
    expect(h.page.getByText(before === null ? 'No Synthetic Sanctum white in 46 runs before this'
      : 'Ended a 23-run Synthetic Sanctum dry streak')).toBeTruthy();
    if (before !== null) expect(h.record).toHaveBeenCalledWith('Synthetic Sanctum', 23);
    vi.mocked(h.client.getHomeDungeonStreak).mockClear().mockResolvedValue({ status: 'ready', value: dungeonStreak(after) });
    await h.page.rerender({ event: { stream_id: value.stream_id, sequence: '20', at: value.at,
      event: { bag_dropped: newDrop(value, 'first') } } });
    await vi.advanceTimersByTimeAsync(500);
    expect(h.client.getHomeDungeonStreak).toHaveBeenCalledOnce();
    h.drop('second'); await settle();
    await vi.advanceTimersByTimeAsync(5500);
    const banner = h.page.container.querySelector('.white-banner-region') as HTMLElement;
    expect(within(banner).getByText('WHITE BAG')).toBeTruthy();
    expect(banner.textContent).not.toMatch(/Ended a|white in .* runs before this/);
    expect(h.record).toHaveBeenCalledTimes(before === null ? 0 : 1);
    if (before === null) expect(h.page.queryByText(/Ended at/)).toBeNull();
    else expect(h.page.getByText('Ended at 23 runs')).toBeTruthy();
  });

it.each([0, null])('home_zero_runs_never_show_or_record_a_streak_with_previous_white_%s', async previous => {
  const h = homeHarness(snapshot(), { ...dungeonStreak(previous), runs_ended: previous === null ? 0 : 46 }); await settle();
  h.drop('zero'); await settle();
  expect(h.page.getByText('WHITE BAG')).toBeTruthy();
  const banner = h.page.container.querySelector('.white-banner-region')!;
  expect(banner.textContent).not.toMatch(/Ended a|white in .* runs before this/);
  expect(h.page.queryByText(/Ended at/)).toBeNull();
  expect(h.record).not.toHaveBeenCalled();
});

it.each(['snapshot', 'session'])('home_null_%s_then_same_session_preserves_ended_streak', async missing => {
  const value = snapshot(), h = homeHarness(value); await settle(); h.drop('first'); await settle();
  expect(h.page.getByText('Ended at 23 runs')).toBeTruthy();
  h.reset.mockClear();
  await h.page.rerender({ snapshot: missing === 'snapshot' ? null : { ...value, session: null } });
  await settle();
  await h.page.rerender({ snapshot: value }); await settle();
  expect(h.page.getByText('Ended at 23 runs')).toBeTruthy();
  expect(h.reset).not.toHaveBeenCalled();
  await h.page.rerender({ snapshot: { ...value, session: { ...value.session!, id: 'synthetic-new-session' } } });
  await settle();
  expect(h.page.queryByText('Ended at 23 runs')).toBeNull();
  expect(h.reset).toHaveBeenCalledOnce();
});

it.each([0, 7])('home_first_white_in_next_run_uses_positive_streak_only_%s', async runs => {
  const value = snapshot(), h = homeHarness(value); await settle(); h.drop('first'); await settle();
  const next = { ...value, run: { ...value.run!, id: 'synthetic-next-run' } };
  h.homeWindow.update(window => ({ ...window, runId: next.run.id, drops: [], fights: [] }));
  await h.page.rerender({ snapshot: next }); await settle();
  vi.mocked(h.client.getHomeDungeonStreak).mockClear().mockResolvedValue({ status: 'ready', value: dungeonStreak(runs) });
  await h.page.rerender({ event: { stream_id: value.stream_id, sequence: '20', at: value.at,
    event: { bag_dropped: newDrop(value, 'first') } } });
  await vi.advanceTimersByTimeAsync(500);
  expect(h.client.getHomeDungeonStreak).toHaveBeenCalledOnce();
  h.homeWindow.update(window => ({ ...window, drops: [newDrop(next, 'next-white')] })); await settle();
  expect(h.page.getByText('WHITE BAG')).toBeTruthy();
  expect(h.page.getByText(`Ended at ${runs || 23} runs`)).toBeTruthy();
  if (runs > 0) {
    expect(h.page.getByText('Ended a 7-run Synthetic Sanctum dry streak')).toBeTruthy();
    expect(h.record).toHaveBeenLastCalledWith('Synthetic Sanctum', 7);
  } else {
    expect(h.page.queryByText(/Ended a /)).toBeNull();
    expect(h.record).toHaveBeenCalledTimes(1);
  }
});

it.each(['pending', 'failed', 'obsolete'] as const)('home_never_banners_run_A_streak_in_run_B_when_read_is_%s', async result => {
  const value = snapshot(), h = homeHarness(value); await settle();
  const pending = deferred<HistoryReply<HomeDungeonStreak>>();
  vi.mocked(h.client.getHomeDungeonStreak).mockReturnValueOnce(pending.promise);
  if (result === 'obsolete') {
    await h.page.rerender({ event: { stream_id: value.stream_id, sequence: '20', at: value.at,
      event: { run_ended: { ...value.run!, ended: value.at } } } });
    await vi.advanceTimersByTimeAsync(500);
    vi.mocked(h.client.getHomeDungeonStreak).mockResolvedValueOnce({ status: 'ready', value: dungeonStreak(0) });
  }
  const next = { ...value, run: { ...value.run!, id: 'synthetic-run-B' } };
  h.homeWindow.update(window => ({ ...window, runId: next.run.id, drops: [], fights: [] }));
  await h.page.rerender({ snapshot: next }); await settle();
  if (result === 'failed') pending.reject(new Error('synthetic run B failure'));
  else if (result === 'obsolete') pending.resolve({ status: 'ready', value: dungeonStreak(99) });
  await settle();
  h.homeWindow.update(window => ({ ...window, drops: [newDrop(next, 'run-B-white')] })); await settle();
  expect(h.page.getByText('WHITE BAG')).toBeTruthy();
  expect(h.page.container.querySelector('.white-banner-region')!.textContent).not.toMatch(/Ended a|white in .* runs before this/);
  expect(h.record).not.toHaveBeenCalled();
});

it('home_death_in_a_dungeon_refreshes_streak_without_profile_read', async () => {
  const value = snapshot(), h = homeHarness(value); await settle();
  vi.mocked(h.client.getDungeonProfile).mockClear();
  vi.mocked(h.client.getHomeDungeonStreak).mockClear();
  const run = { ...value.run!, ended: value.at, outcome: 'died' as const };
  await h.page.rerender({ snapshot: { ...value, run }, event: {
    stream_id: value.stream_id, sequence: '20', at: value.at, event: { run_ended: run } } });
  await vi.advanceTimersByTimeAsync(500);
  expect(h.client.getHomeDungeonStreak).toHaveBeenCalledOnce();
  expect(h.client.getDungeonProfile).not.toHaveBeenCalled();
});

it('home_ended_streak_survives_leaving_home_and_returning_in_the_same_session', async () => {
  const value = snapshot(), h = homeHarness(value); await settle(); h.drop('new'); await settle();
  expect(h.page.getByText('Ended at 23 runs')).toBeTruthy();
  h.page.unmount();
  const page = render(Home, { props: { snapshot: value, client: h.client, redesign: true, syntheticClock: true },
    context: new Map([[SHELL_CONNECTION, h.connection]]) });
  await settle();
  expect(page.queryByText('WHITE BAG')).toBeNull();
  expect(page.getByText('Ended at 23 runs')).toBeTruthy();
});

it('home_requests_catalog_visuals_for_learned_upcoming_bosses', async () => {
  const h = homeHarness(); await settle();
  expect(vi.mocked(h.client.getObjectVisuals).mock.calls.flatMap(([types]) => types)).toContain(301);
  expect(within(h.page.getByRole('region', { name: 'Bosses' })).getByText('Synthetic upcoming')).toBeTruthy();
});

it('home_banner_resolves_new_catalog_names_without_changing_the_push_time_streak', async () => {
  const value = snapshot(), h = homeHarness(value); await settle();
  const fresh = { ...newDrop(value, 'new-types'), source_enemy: 888, items: [777],
    contents: [{ ...value.drops[0].contents[0], item_type: 777 }] };
  h.homeWindow.update(window => ({ ...window, drops: [...window.drops, fresh] }));
  await settle();
  const banner = h.page.container.querySelector('.white-banner-region') as HTMLElement;
  expect(within(banner).getByRole('link', { name: 'Synthetic new treasure' }).getAttribute('href')).toBe('#/loot/item/777');
  expect(within(banner).getByText('Synthetic new source (estimated) · 6:00 PM')).toBeTruthy();
  vi.mocked(h.client.getHomeDungeonStreak).mockResolvedValue({ status: 'ready', value: dungeonStreak(0) });
  await h.page.rerender({ event: { stream_id: value.stream_id, sequence: '20', at: value.at, event: { bag_dropped: fresh } } });
  await vi.advanceTimersByTimeAsync(500);
  expect(within(banner).getByText('Ended a 23-run Synthetic Sanctum dry streak')).toBeTruthy();
  await vi.advanceTimersByTimeAsync(5500);
  expect(banner.textContent).toBe('');
});

it('home_whites_present_at_mount_and_late_classifications_never_banner', async () => {
  const value = snapshot(); value.drops = [newDrop(value, 'old'), newDrop(value, 'unknown', null)];
  const h = homeHarness(value); await settle();
  expect(h.page.queryByText('WHITE BAG')).toBeNull();
  h.homeWindow.update(window => ({ ...window, drops: window.drops.map(drop => ({ ...drop, bag_rank: 255 })) }));
  await settle(); expect(h.page.queryByText('WHITE BAG')).toBeNull();
  h.drop('new'); await settle();
  expect(h.page.getByText('WHITE BAG')).toBeTruthy();
  expect(h.page.getByText('Synthetic Starwarden (estimated) · 6:00 PM')).toBeTruthy();
});

it('home_ended_streak_records_and_persists_after_banner_and_across_runs_until_session_changes', async () => {
  const value = snapshot(), h = homeHarness(value); await settle(); h.drop('new'); await settle();
  expect(h.page.getByText('Ended a 23-run Synthetic Sanctum dry streak')).toBeTruthy();
  expect(h.page.getByText('Ended at 23 runs')).toBeTruthy();
  await vi.advanceTimersByTimeAsync(6000);
  expect(h.page.queryByText('WHITE BAG')).toBeNull();
  expect(h.page.getByText('Ended at 23 runs')).toBeTruthy();
  const next = { ...value, run: { ...value.run!, id: 'synthetic-next-run' } };
  h.homeWindow.update(window => ({ ...window, runId: next.run.id, drops: [], fights: [] }));
  await h.page.rerender({ snapshot: next }); await settle();
  expect(h.page.getByText('Ended at 23 runs')).toBeTruthy();
  await h.page.rerender({ snapshot: { ...next, session: { ...next.session!, id: 'synthetic-next-session' } } });
  await settle(); expect(h.page.queryByText('Ended at 23 runs')).toBeNull();
});

it('home_first_white_uses_runs_ended', async () => {
  const h = homeHarness(snapshot(), dungeonStreak(null)); await settle(); h.drop('first'); await settle();
  expect(h.page.getByText('No Synthetic Sanctum white in 46 runs before this')).toBeTruthy();
  expect(h.page.queryByText(/Ended at/)).toBeNull();
});

it('home_area_and_run_scope_changes_reseed_clear_display_and_dispose_old_queue', async () => {
  const value = snapshot(), h = homeHarness(value); await settle(); h.drop('first'); h.drop('waiting'); await settle();
  const oldQueue = h.factory.mock.results.at(-1)!.value;
  const dispose = vi.spyOn(oldQueue, 'dispose');
  const next = { ...value, run: { ...value.run!, id: 'synthetic-new-run' } };
  const present = newDrop(next, 'present-on-entry');
  h.homeWindow.update(window => ({ ...window, runId: next.run.id, drops: [present], fights: [] }));
  await h.page.rerender({ snapshot: next }); await settle();
  expect(dispose).toHaveBeenCalledOnce();
  expect(h.page.queryByText('WHITE BAG')).toBeNull();
  await vi.advanceTimersByTimeAsync(12_000);
  expect(h.page.queryByText('WHITE BAG')).toBeNull();
  h.homeWindow.update(window => ({ ...window, drops: [...window.drops, newDrop(next, 'new-in-next')] }));
  await settle(); expect(h.page.getByText('WHITE BAG')).toBeTruthy();
  await h.page.rerender({ snapshot: { ...next, area_kind: 'nexus', run: null } }); await settle();
  expect(h.page.queryByText('WHITE BAG')).toBeNull();
});

it('home_realm_white_has_null_streak_and_visit_change_reseeds', async () => {
  const value = snapshot(); value.run = null; value.area_kind = 'realm'; value.fights = [];
  value.drops = [newDrop(value, 'realm-present')];
  const h = homeHarness(value); await settle();
  const queue = h.factory.mock.results.at(-1)!.value, push = vi.spyOn(queue, 'push');
  h.drop('realm-new'); await settle();
  expect(push).toHaveBeenCalledWith(expect.objectContaining({ dropId: 'realm-new', streak: null }));
  const dispose = vi.spyOn(queue, 'dispose');
  h.homeWindow.update(window => ({ ...window, visitKey: 'synthetic-next-visit' })); await settle();
  expect(dispose).toHaveBeenCalledOnce();
  const nextPush = vi.spyOn(h.factory.mock.results.at(-1)!.value, 'push');
  h.homeWindow.update(window => ({ ...window, drops: [...window.drops] })); await settle();
  expect(nextPush).not.toHaveBeenCalled();
});

it('home_flag_off_keeps_existing_dungeon_and_does_not_create_white_queue', async () => {
  const h = homeHarness(snapshot(), dungeonStreak(), false); await settle();
  expect(h.page.container.querySelector('.dungeon-home')).toBeTruthy();
  expect(h.page.container.querySelector('.dungeon-screen')).toBeNull();
  expect(h.factory).not.toHaveBeenCalled();
  for (const read of [h.client.getHomeHud, h.client.getHomeDungeonStreak, h.client.getDungeonProfile,
    h.client.getBossHistory, h.client.getHomeBossBaselines, h.client.getLootStats]) expect(read).not.toHaveBeenCalled();
});

it('home_banner_reselects_best_item_when_new_tiers_arrive_after_a_white_bag', async () => {
  const value = snapshot(), h = homeHarness(value); await settle();
  const pending = deferred<Awaited<ReturnType<typeof h.client.getObjectVisuals>>>();
  vi.mocked(h.client.getObjectVisuals).mockReturnValue(pending.promise);
  const fresh = { ...newDrop(value, 'late-tiers'), items: [2591, 3000],
    contents: [2591, 3000].map((item_type, slot) => ({ ...value.drops[0].contents[0], item_type, slot })) };
  h.homeWindow.update(window => ({ ...window, drops: [...window.drops, fresh] })); await settle();
  expect(h.client.getObjectVisuals).toHaveBeenCalledWith(expect.arrayContaining([2591, 3000]));
  const banner = h.page.container.querySelector('.white-banner-region') as HTMLElement;
  expect(within(banner).getByRole('link').getAttribute('href')).toBe('#/loot/item/2591');
  pending.resolve({ ...visuals, objects: [
    { object_type: 2591, name: 'Synthetic potion', item_tier: null, sprite: null, bag: null },
    { object_type: 3000, name: 'Synthetic UT winner', item_tier: 'UT', sprite: null, bag: null },
  ] }); await settle();
  expect(within(banner).getByRole('link', { name: 'Synthetic UT winner' }).getAttribute('href')).toBe('#/loot/item/3000');
  expect(h.page.container.querySelector('.best-card')?.textContent).toContain('Synthetic UT winner');
  expect(within(banner).getByText('Ended a 23-run Synthetic Sanctum dry streak')).toBeTruthy();
});
