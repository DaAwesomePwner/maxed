import { cleanup, render, within } from '@testing-library/svelte';
import { afterEach, expect, it } from 'vitest';
import type { HomeBossBaseline } from '../../src/lib/bindings/HomeBossBaseline';
import type { DungeonProfile } from '../../src/lib/bindings/DungeonProfile';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { SpriteRef } from '../../src/lib/bindings/SpriteRef';
import { spriteUrl } from '../../src/lib/bridge/visuals';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import BossesCard from '../../src/lib/home/v4/BossesCard.svelte';
import { bossRows, killDelta, type BossRow } from '../../src/lib/home/v4/bosses';
import { homeSnapshot } from '../home/fixtures';

afterEach(cleanup);
const sprite: SpriteRef = { key: 'synthetic-boss', sheet: 'mapObjects.png', x: 0, y: 0, width: 8, height: 8 };
const visuals = { ...emptyHomeVisuals(), client_fingerprint: 'a'.repeat(64), objects: [
  { object_type: 1, name: 'Synthetic first', sprite, bag: null },
  { object_type: 2, name: 'Synthetic second', sprite: null, bag: null },
  { object_type: 3, name: 'Synthetic upcoming', sprite: null, bag: null },
] };
const profile: DungeonProfile = { dungeon: 'Synthetic dungeon', clears: 5, usual_run_dps: null,
  run_dps_samples: 0, usual_clear_micros: null, clear_samples: 0,
  bosses: [1, 2, 3].map(target_type => ({ target_type, target_name: null, seen_in: 4,
    median_offset_micros: target_type * 120_000_000, median_kill_micros: 118_000_000,
    kill_samples: 4, best_dps: 2400 })),
};
const history = (overrides: Partial<HomeBossBaseline> = {}): HomeBossBaseline => ({ target_type: 1,
  fights: 4, avg_dps: 1400, best_dps: 2500,
  kill_samples: 4, median_kill_micros: 120_000_000, ...overrides });
function fight(target_type: number, started: string, overrides: Partial<FightView> = {}): FightView {
  const base = homeSnapshot().fights[0];
  return { ...base, id: `synthetic-${target_type}-${started}`, target_type, boss_type: target_type,
    started, ended: null, kill: null, players: [
      { ...base.players[0], is_local: false, dps: 9000, rank: 1 },
      { ...base.players[0], is_local: true, dps: 1400, rank: 2 },
    ], ...overrides };
}
const killed = (at: string): Pick<FightView, 'kill' | 'ended'> => ({ ended: at,
  kill: { at, evidence: 'hp_zero_removed', provisional: false } });
const row = (overrides: Partial<BossRow> = {}): BossRow => ({ targetType: 1, name: 'Synthetic first', sprite,
  state: 'killed', killMicros: 106_000_000, elapsedMicros: null, usualKillMicros: 118_000_000,
  dps: 1400, rank: 2, bestDps: 2400, ...overrides });

it('bosses_follow_fight_order_then_profile_upcoming', () => {
  const fights = [fight(1, '10000000', killed('116000000')), fight(2, '2000000', { ended: '8000000' }),
    fight(99, '0', { target_kind: 'ordinary' })];
  const rows = bossRows(fights, profile, new Map(), 120_000_000, visuals);
  expect(rows.map(value => [value.targetType, value.state])).toEqual([[2, 'ended'], [1, 'killed'], [3, 'upcoming']]);
  expect(rows[1]).toMatchObject({ name: 'Synthetic first', sprite, killMicros: 106_000_000,
    usualKillMicros: 118_000_000, dps: 1400, rank: 2 });
  expect(rows[2]).toMatchObject({ name: 'Synthetic upcoming', dps: null, rank: null, bestDps: 2400 });
  expect(fights[0].target_type).toBe(1);
  const reordered = { ...profile, bosses: [profile.bosses[2], profile.bosses[1], profile.bosses[0]] };
  expect(bossRows([fight(1, '0'), fight(1, '10')], reordered, new Map(), 20, visuals)
    .map(value => [value.targetType, value.state])).toEqual([[1, 'live'], [1, 'live'], [3, 'upcoming'], [2, 'upcoming']]);
});

it('faster_than_usual_text', () => {
  expect(killDelta(row())).toBe('▲ 12s faster than usual');
  expect(killDelta(row({ killMicros: 117_000_000 }))).toBe('▲ 1s faster than usual');
});

it('slower_or_equal_shows_usually', () => {
  for (const killMicros of [118_000_000, 130_000_000, 117_500_000, null]) {
    expect(killDelta(row({ killMicros }))).toBe('usually 1:58');
  }
  expect(killDelta(row({ usualKillMicros: null }))).toBe('');
});

it('no_profile_lists_only_fought_bosses', () => {
  const rows = bossRows([fight(2, '0', { target_kind: 'event_boss' }), fight(1, '2')], null,
    new Map([[1, history()]]), 10_000_000, visuals);
  expect(rows.map(value => value.targetType)).toEqual([2, 1]);
  expect(rows[1].usualKillMicros).toBe(120_000_000);
  expect(bossRows([], null, new Map(), 0, visuals)).toEqual([]);
});

it('live_boss_has_elapsed_and_no_kill_time', () => {
  const rows = bossRows([fight(1, '2000000')], null, new Map(), 15_000_000, visuals);
  expect(rows[0]).toMatchObject({ state: 'live', elapsedMicros: 13_000_000, killMicros: null });
});

it('provisional_and_missing_kills_never_become_confirmed_kill_times', () => {
  const provisional = { at: '10000000', evidence: 'hp_zero_removed' as const, provisional: true };
  const rows = bossRows([fight(1, '0', { kill: provisional }),
    fight(1, '100', { kill: provisional, ended: '12000000' }),
    fight(2, '200', { ended: '12000000' }), fight(3, '300', { ...killed('10000000'), ended: null })],
  null, new Map(), 15_000_000, visuals);
  expect(rows.map(value => [value.state, value.killMicros])).toEqual([
    ['live', null], ['ended', null], ['ended', null], ['killed', 9_999_700],
  ]);
  expect(rows[1].elapsedMicros).toBeNull();
});

it('usual_prefers_profile_and_requires_three_confirmed_kills', () => {
  const usual = new Map([[1, history()]]);
  expect(bossRows([fight(1, '0')], profile, usual, 0, visuals)[0].usualKillMicros).toBe(118_000_000);
  const sparse = { ...profile, bosses: [{ ...profile.bosses[0], kill_samples: 2 }] };
  expect(bossRows([fight(1, '0')], sparse, usual, 0, visuals)[0].usualKillMicros).toBe(120_000_000);
  expect(bossRows([fight(1, '0')], sparse, new Map([[1, history({ kill_samples: 2 })]]), 0, visuals)[0].usualKillMicros).toBeNull();
});

it('catalog_resolves_names_and_sprites_with_fight_type_fallback', () => {
  const rows = bossRows([fight(1, '0'), fight(98, '1', { boss_type: 1 }), fight(99, '2')],
    null, new Map([[1, { ...history(), target_name: 'Stale history name' } as HomeBossBaseline]]), 10, visuals);
  expect(rows.map(value => [value.name, value.sprite])).toEqual([
    ['Synthetic first', sprite], ['Synthetic first', sprite], ['Unknown boss', null],
  ]);
});

it('unknown_local_stats_and_invalid_times_stay_unknown', () => {
  const rows = bossRows([fight(1, 'invalid', { players: [] }), fight(2, '20', { ...killed('10') }),
    fight(3, '9007199254740993', killed('9007199255740993'))], null, new Map(), 0, visuals);
  expect(rows.find(value => value.targetType === 1)).toMatchObject({ dps: null, rank: null, elapsedMicros: null });
  expect(rows.find(value => value.targetType === 2)?.killMicros).toBeNull();
  expect(rows.find(value => value.targetType === 3)?.killMicros).toBe(1_000_000);
  expect(bossRows([fight(1, '20')], null, new Map(), 10, visuals)[0].elapsedMicros).toBeNull();
});

it('card_renders_killed_live_ended_and_faded_upcoming_rows', () => {
  const rows = [row(), row({ name: 'Synthetic live', state: 'live', killMicros: null, elapsedMicros: 13_000_000 }),
    row({ name: 'Synthetic ended', state: 'ended', killMicros: null, dps: null, rank: null }),
    row({ name: 'Synthetic upcoming', state: 'upcoming', killMicros: null, dps: null, rank: null })];
  const page = render(BossesCard, { rows, visuals, resolveSpriteUrl: s => spriteUrl(visuals.client_fingerprint, s)! });
  expect(page.getByRole('heading', { name: 'Bosses' })).toBeTruthy();
  const items = page.getAllByRole('listitem');
  expect(within(items[0]).getByText('Kill time 1:46')).toBeTruthy();
  expect(within(items[0]).getByText('▲ 12s faster than usual')).toBeTruthy();
  expect(within(items[0]).getByText('Your DPS 1,400 · #2')).toBeTruthy();
  expect(items[0].querySelector('img')?.getAttribute('src')).toBe(spriteUrl(visuals.client_fingerprint, sprite));
  expect(within(items[1]).getByText('Live')).toBeTruthy();
  expect(within(items[1]).getByText('Elapsed 0:13')).toBeTruthy();
  expect(within(items[2]).getByText('Ended')).toBeTruthy();
  expect(within(items[2]).getByText('Kill time —')).toBeTruthy();
  expect(within(items[2]).getByText('Your DPS — · —')).toBeTruthy();
  expect(items[3].classList.contains('upcoming')).toBe(true);
  expect(within(items[3]).getByText('usually 1:58')).toBeTruthy();
  expect(within(items[3]).getByText('best DPS 2,400')).toBeTruthy();
  expect(items[3].textContent).not.toContain('in this dungeon');
});
