import { expect, it } from 'vitest';
import { homeSnapshot } from '../home/fixtures';
import { emptyHomeVisuals } from '../../src/lib/home/visuals';
import { bossOnly, damageRows, runRows, personalBest, lootCounts, notificationTip, fightDps } from '../../src/lib/home/v3/screen-data';
import { rankWindow } from '../../src/lib/home/v3/rank-window';

it('carries player and object ids through fight and run row windows', () => {
  const seed = homeSnapshot().fights[0].players[0];
  const rows = damageRows([seed], seed);
  expect(rows[0]).toMatchObject({ player_id: seed.player_id, object_id: seed.object_id });
  expect(rankWindow(rows)[0]).toMatchObject({ player_id: seed.player_id, object_id: seed.object_id });
  const { object_id: _objectId, sources: _sources, ...total } = seed;
  expect(damageRows([total], total)[0]).toMatchObject({ player_id: seed.player_id, object_id: null });
  expect(runRows([total], total)[0]).toMatchObject({ player_id: seed.player_id, object_id: null });
});

it('keeps a short glimpse DPS unavailable and orders nullable player DPS by damage', () => {
  const fight = homeSnapshot().fights[0];
  expect(fightDps({ ...fight, first_hit: '1000000', last_hit: '1400000', total_damage: 6780 })).toBeNull();
  const seed = fight.players[0];
  const rows = damageRows([
    { ...seed, player_id: 'steady', damage: 1000, dps: 100 },
    { ...seed, player_id: 'glimpse', damage: 6780, dps: null },
  ], null);
  expect(rows.map(row => row.damage)).toEqual([6780, 1000]);
  expect(rows[0].dps).toBeNull();
});

it('uses saved best clear and signed deltas, with an explicit missing baseline', () => {
  const snapshot = homeSnapshot();
  snapshot.run!.started = '1000000'; snapshot.at = '769000000';
  const dungeonBests = [{ dungeon: snapshot.run!.dungeon, best_clear_micros: 719_000_000 }];
  expect(personalBest(snapshot, dungeonBests)).toEqual({ best: '11:59', delta: '+0:49', behind: true });
  snapshot.at = '701000000';
  expect(personalBest(snapshot, dungeonBests)).toEqual({ best: '11:59', delta: '−0:19', behind: false });
  expect(personalBest(snapshot, null)).toBeNull();
});
it('filters display bosses without changing Realm focus policy', () => {
  const seed = homeSnapshot().fights[0];
  expect(['boss', 'event_boss', 'miniboss', 'quest', 'ordinary'].filter(target_kind =>
    bossOnly({ ...seed, target_kind: target_kind as typeof seed.target_kind }))).toEqual(['boss', 'event_boss']);
});
it('sorts damage deterministically and only highlights verified identity', () => {
  const snapshot = homeSnapshot(), seed = snapshot.fights[0].players[0];
  const players = [{ ...seed, player_id: 'z', is_local: false, damage: 99 },
    { ...seed, player_id: 'a', is_local: false, damage: 99 }, seed];
  expect(damageRows(players, null).every(row => !row.is_local)).toBe(true);
  expect(damageRows(players, seed).filter(row => row.is_local)).toHaveLength(1);
  const totals = Array.from({ length: 85 }, (_, i) => ({ ...seed, player_id: `p${i}`, damage: 1000 - i, rank: i + 1 }));
  expect(runRows(totals, totals[84])).toHaveLength(9);
  expect(runRows(totals, totals[84]).at(-1)).toMatchObject({ rank: 85, is_local: true });
});
it('counts appearances, all tiers and all 128 bags, not stack quantities', () => {
  const seed = homeSnapshot().drops[0];
  const drops = Array.from({ length: 128 }, (_, i) => ({ ...seed, id: `bag${i}`, contents_known: true,
    contents: [{ ...seed.contents[0], item_type: 200, quantity: 99 }], items: [200] }));
  const visuals = { ...emptyHomeVisuals(), objects: [{ object_type: 200, name: 'ST', sprite: null, bag: null, item_tier: 'ST' }] };
  expect(lootCounts(drops, visuals)).toEqual({ bags: 128, ut: 0, st: 128, items: 128 });
});
it('keeps death detail in tooltip text and invalid duration DPS unknown', () => {
  const snapshot = homeSnapshot();
  expect(notificationTip([{ id: 'd', session_id: 's', run_id: 'r', at: '1', provisional: false,
    kind: { type: 'player_death', player_name: 'TestHero', level: 20, killed_by: 'Guardian', is_local: false } }], 'player_death'))
    .toBe('TestHero · Level 20 · Killed by Guardian');
  expect(fightDps({ ...snapshot.fights[0], first_hit: '1', last_hit: '1' })).toBeNull();
});

it('shows a nonzero sub-second PB delta', () => {
  const snapshot = homeSnapshot();
  snapshot.run!.started = '1000000'; snapshot.run!.completed_at = '87500000';
  expect(personalBest(snapshot, [{ dungeon: snapshot.run!.dungeon, best_clear_micros: 87_000_000 }])?.delta).toBe('under 1 s faster');
});
