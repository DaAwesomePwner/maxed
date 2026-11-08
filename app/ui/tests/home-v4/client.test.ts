import { expect, it, vi } from 'vitest';
import type { BossHistory } from '../../src/lib/bindings/BossHistory';
import type { BossHistoryRequest } from '../../src/lib/bindings/BossHistoryRequest';
import type { DungeonProfile } from '../../src/lib/bindings/DungeonProfile';
import type { DungeonProfileRequest } from '../../src/lib/bindings/DungeonProfileRequest';
import type { HomeHud } from '../../src/lib/bindings/HomeHud';
import type { HomeScope } from '../../src/lib/bindings/HomeScope';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import snapshot from '../fixtures/t0-live';

it('get_home_boss_baselines_invokes_a_cloned_fixture_request_and_result', async () => {
  const request: BossHistoryRequest = {
    range: { start: '10', end: '30' }, character_id: 'synthetic-character', target_types: [42],
  };
  const value = { rows: [{ target_type: 42, fights: 3, avg_dps: 2000, best_dps: 3000,
    kill_samples: 4, median_kill_micros: 75_000_000 }], truncated: false };
  const homeBossBaselines = vi.fn((_request: BossHistoryRequest) => value);
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, {}, undefined, undefined, { homeBossBaselines });
  const invoke = vi.spyOn(fixture, 'invoke');
  const reply = await createClient(fixture).getHomeBossBaselines(request);
  expect(invoke).toHaveBeenCalledWith('get_home_boss_baselines', { request });
  expect(homeBossBaselines).toHaveBeenCalledWith(request);
  expect(homeBossBaselines.mock.calls[0][0]).not.toBe(request);
  expect(reply).toEqual({ status: 'ready', value });
  if (reply.status === 'ready') expect(reply.value).not.toBe(value);
});

it('defaults Home boss baselines to an empty ready result', async () => {
  await expect(createClient(createFixtureTransport(snapshot)).getHomeBossBaselines({
    range: null, character_id: null, target_types: null,
  })).resolves.toEqual({ status: 'ready', value: { rows: [], truncated: false } });
});

it('reports missing Home boss baselines in strict previews', async () => {
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, { requireHistoryFixtures: true });
  await expect(createClient(fixture).getHomeBossBaselines({ range: null, character_id: null, target_types: null }))
    .rejects.toThrow('Home boss baselines was not supplied for this synthetic fixture preview');
});

it('get_home_dungeon_streak_invokes_a_cloned_fixture_request_and_result', async () => {
  const request = { dungeon: 'Synthetic Sanctum' };
  const value = { ...request, runs_ended: 46, runs_since_last_white: 23, avg_runs_per_white: 23, last_white_at: '100' };
  const homeDungeonStreak = vi.fn((_request: typeof request) => value);
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, {}, undefined, undefined, { homeDungeonStreak });
  const invoke = vi.spyOn(fixture, 'invoke');
  const reply = await createClient(fixture).getHomeDungeonStreak(request);
  expect(invoke).toHaveBeenCalledWith('get_home_dungeon_streak', { request });
  expect(homeDungeonStreak).toHaveBeenCalledWith(request);
  expect(homeDungeonStreak.mock.calls[0][0]).not.toBe(request);
  expect(reply).toEqual({ status: 'ready', value });
  if (reply.status === 'ready') expect(reply.value).not.toBe(value);
});

it('client_get_run_recap_invokes_command', async () => {
  const request = { run_id: 'synthetic-run' };
  const value = { run_id: request.run_id, clear_number: 38, in_a_row: 3, chain: null,
    pb_clear: { clear_micros: 90, previous_best_micros: 100 }, dry_streak_end: null,
    best_dps: [], milestones: [] };
  const runRecap = vi.fn((_request: typeof request) => value);
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, {}, undefined, undefined, { runRecap });
  const invoke = vi.spyOn(fixture, 'invoke');
  const reply = await createClient(fixture).getRunRecap(request);
  expect(invoke).toHaveBeenCalledWith('get_run_recap', { request });
  expect(runRecap).toHaveBeenCalledWith(request);
  expect(runRecap.mock.calls[0][0]).not.toBe(request);
  expect(reply).toEqual({ status: 'ready', value });
  if (reply.status === 'ready') expect(reply.value).not.toBe(value);
});

it('defaults a missing run recap to null', async () => {
  await expect(createClient(createFixtureTransport(snapshot)).getRunRecap({ run_id: 'missing' }))
    .resolves.toEqual({ status: 'ready', value: null });
});

it('client_get_visit_recap_invokes_command', async () => {
  const request = { visit_id: 'synthetic-stream:visit:42' };
  const value = { visit_id: request.visit_id, session_id: 'synthetic-session', character_id: null,
    title: 'Synthetic Realm', periods: [{ visit_id: request.visit_id, started: '10', ended: '20' }],
    fights: [], boss_fights: [], boss_fights_limited: false, announcements: [], announcements_limited: false,
    drops: [], drops_limited: false, fame_delta: 10, best_dps: [], chain: null };
  const visitRecap = vi.fn((_request: typeof request) => value);
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, {}, undefined, undefined, { visitRecap });
  const invoke = vi.spyOn(fixture, 'invoke');
  const reply = await createClient(fixture).getVisitRecap(request);
  expect(invoke).toHaveBeenCalledWith('get_visit_recap', { request });
  expect(visitRecap).toHaveBeenCalledWith(request);
  expect(visitRecap.mock.calls[0][0]).not.toBe(request);
  expect(reply).toEqual({ status: 'ready', value });
  if (reply.status === 'ready') {
    expect(reply.value).not.toBe(value);
    expect(reply.value?.periods).not.toBe(value.periods);
  }
});

it('defaults a missing visit_recap to null', async () => {
  await expect(createClient(createFixtureTransport(snapshot)).getVisitRecap({ visit_id: 'missing' }))
    .resolves.toEqual({ status: 'ready', value: null });
});

it('reports missing visit_recap fixtures in strict previews', async () => {
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, { requireHistoryFixtures: true });
  await expect(createClient(fixture).getVisitRecap({ visit_id: 'missing' }))
    .rejects.toThrow('Visit recap was not supplied for this synthetic fixture preview');
});

it('reports missing run recap fixtures in strict previews', async () => {
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, { requireHistoryFixtures: true });
  await expect(createClient(fixture).getRunRecap({ run_id: 'missing' }))
    .rejects.toThrow('Run recap was not supplied for this synthetic fixture preview');
});

it('defaults a dungeon streak to zero counts and null unknowns', async () => {
  await expect(createClient(createFixtureTransport(snapshot)).getHomeDungeonStreak({ dungeon: 'Synthetic' }))
    .resolves.toEqual({ status: 'ready', value: { dungeon: 'Synthetic', runs_ended: 0,
      runs_since_last_white: null, avg_runs_per_white: null, last_white_at: null } });
});

it('reports missing dungeon streak fixtures in strict previews', async () => {
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, { requireHistoryFixtures: true });
  await expect(createClient(fixture).getHomeDungeonStreak({ dungeon: 'Synthetic' }))
    .rejects.toThrow('Home dungeon streak was not supplied for this synthetic fixture preview');
});

it('get_home_hud_invokes_the_command', async () => {
  const scope: HomeScope = { today: { range: { start: '10', end: '30' } } };
  const value: HomeHud = {
    fame_per_hour: 600, combat: { fights: 2, weighted_dps: 2500, weighted_party_dps: 2000,
      vs_party: 0.25, avg_rank: 2 }, whites: 1, ut_st: [42], ut_st_count: 1,
    potions: [{ stat: 'life', object_type: null, count: 2 }],
  };
  const homeHud = vi.fn((_scope: HomeScope) => value);
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, {}, undefined, undefined, { homeHud });
  const invoke = vi.spyOn(fixture, 'invoke');
  const reply = await createClient(fixture).getHomeHud(scope);
  expect(invoke).toHaveBeenCalledWith('get_home_hud', { scope });
  expect(homeHud).toHaveBeenCalledWith(scope);
  expect(homeHud.mock.calls[0][0]).not.toBe(scope);
  expect(reply).toEqual({ status: 'ready', value });
  if (reply.status === 'ready') expect(reply.value).not.toBe(value);
});

it('defaults Home HUD to an empty ready result', async () => {
  const client = createClient(createFixtureTransport(snapshot));
  await expect(client.getHomeHud({ session: { id: 'synthetic-session' } })).resolves.toEqual({
    status: 'ready', value: { fame_per_hour: null, combat: null, whites: 0,
      ut_st: [], ut_st_count: 0, potions: [] },
  });
});

it('reports missing Home HUD when history fixtures are required', async () => {
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, { requireHistoryFixtures: true });
  await expect(createClient(fixture).getHomeHud({ session: { id: 'synthetic-session' } }))
    .rejects.toThrow('Home HUD was not supplied for this synthetic fixture preview');
});

it('get_boss_history_invokes_the_command', async () => {
  const request: BossHistoryRequest = {
    range: { start: '10', end: '30' }, character_id: 'synthetic-character', target_types: [42],
  };
  const value: BossHistory = { rows: [{
    target_type: 42, target_name: null, fights: 3, avg_dps: 2000, best_dps: 3000,
    best_fight_id: 'synthetic-best', avg_rank: 2, avg_party_dps: null,
    kills: 4, median_kill_micros: 75_000_000,
  }], truncated: false };
  const bossHistory = vi.fn((_request: BossHistoryRequest) => value);
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, {}, undefined, undefined, { bossHistory });
  const invoke = vi.spyOn(fixture, 'invoke');
  const reply = await createClient(fixture).getBossHistory(request);
  expect(invoke).toHaveBeenCalledWith('get_boss_history', { request });
  expect(bossHistory).toHaveBeenCalledWith(request);
  expect(bossHistory.mock.calls[0][0]).not.toBe(request);
  expect(reply).toEqual({ status: 'ready', value });
  if (reply.status === 'ready') expect(reply.value).not.toBe(value);
});

it('defaults boss history to an empty ready result', async () => {
  const client = createClient(createFixtureTransport(snapshot));
  await expect(client.getBossHistory({ range: null, character_id: null, target_types: null }))
    .resolves.toEqual({ status: 'ready', value: { rows: [], truncated: false } });
});

it('get_dungeon_profile_invokes_the_command', async () => {
  const request: DungeonProfileRequest = {
    dungeon: 'Synthetic Dungeon', character_id: 'synthetic-character', range: { start: '10', end: '30' },
  };
  const value: DungeonProfile = {
    dungeon: request.dungeon, clears: 4, bosses: [{
      target_type: 42, target_name: 'Synthetic Boss', seen_in: 3, median_offset_micros: 100_000_000,
      median_kill_micros: 60_000_000, kill_samples: 3, best_dps: 3000,
    }], usual_run_dps: 2000, run_dps_samples: 4, usual_clear_micros: 600_000_000, clear_samples: 4,
  };
  const dungeonProfile = vi.fn((_request: DungeonProfileRequest) => value);
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, {}, undefined, undefined, { dungeonProfile });
  const invoke = vi.spyOn(fixture, 'invoke');
  const reply = await createClient(fixture).getDungeonProfile(request);
  expect(invoke).toHaveBeenCalledWith('get_dungeon_profile', { request });
  expect(dungeonProfile).toHaveBeenCalledWith(request);
  expect(dungeonProfile.mock.calls[0][0]).not.toBe(request);
  expect(reply).toEqual({ status: 'ready', value });
  if (reply.status === 'ready') expect(reply.value).not.toBe(value);
});

it('defaults dungeon profile to an empty ready result', async () => {
  const client = createClient(createFixtureTransport(snapshot));
  await expect(client.getDungeonProfile({
    dungeon: 'Synthetic Dungeon', character_id: null, range: { start: '10', end: '30' },
  })).resolves.toEqual({ status: 'ready', value: {
    dungeon: 'Synthetic Dungeon', clears: 0, bosses: [], usual_run_dps: null, run_dps_samples: 0,
    usual_clear_micros: null, clear_samples: 0,
  } });
});

it('reports missing dungeon profile when history fixtures are required', async () => {
  const fixture = createFixtureTransport(snapshot, undefined, undefined, undefined, undefined,
    undefined, undefined, { requireHistoryFixtures: true });
  await expect(createClient(fixture).getDungeonProfile({
    dungeon: 'Synthetic Dungeon', character_id: null, range: { start: '10', end: '30' },
  })).rejects.toThrow('Dungeon profile was not supplied for this synthetic fixture preview');
});
