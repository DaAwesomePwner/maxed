import { expect, it } from 'vitest';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { InstanceEventView } from '../../src/lib/bindings/InstanceEventView';
import { realmView } from '../../src/lib/home/realm';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { fight, realmSnapshot } from './fixtures';

const snapshot = () => realmSnapshot();
const killed = (id: string, at = '7000000', overrides: Partial<FightView> = {}) => fight(id, {
  run_id: null, started: '6000001', ended: at, target_kind: 'event_boss',
  kill: { at, evidence: 'hp_zero_removed', provisional: false }, ...overrides,
});
const announcement = (id: string, at = '7000100', type: number | null = 300): InstanceEventView => ({
  id, session_id: snapshot().session!.id, run_id: null, at, provisional: true,
  kind: { type: 'realm_event_killed', boss_key: 'Synthetic_Boss', object_type: type, killer: 'LastHit' },
});
const windowFor = (fights: FightView[] = [], events: InstanceEventView[] = []) => ({
  ...emptyHomeLiveWindow(), streamId: snapshot().stream_id, visitKey: 'visit:1', visitStarted: '6000000', fights, events,
});

it('merges one fought kill and its announcement without changing contribution, evidence or row identity', () => {
  const s = snapshot(), boss = killed('boss'), event = announcement('notice');
  const result = realmView(s, windowFor([boss], [event]));
  expect(result.kills).toHaveLength(1);
  expect(result.kills[0]).toMatchObject({ id: 'fight:boss', targetType: 300, bossKey: 'Synthetic_Boss',
    killer: 'LastHit', at: '7000000', evidence: 'hp_zero_removed', provisional: false,
    fightId: 'boss', started: '6000001', ended: '7000000' });
  expect(result.kills[0].self).toBe(boss.players[0]);
  expect(result.kills[0].self?.damage).toBe(boss.players[0].damage);
  expect(realmView(s, windowFor([], [event])).kills[0]).toMatchObject({ self: null, killer: 'LastHit', fightId: null });
  expect(realmView(s, windowFor([boss], [])).kills[0]).toMatchObject({ id: 'fight:boss', killer: null });
  expect(realmView(s, windowFor([boss], [event, event])).kills).toHaveLength(1);
});

it('counts boss, event, miniboss and quest fought kills and retains explicit event announcements', () => {
  const fights = (['boss', 'event_boss', 'miniboss', 'quest', 'ordinary'] as const)
    .map((target_kind, index) => killed(target_kind, String(7000000 + index), { target_kind }));
  const rows = realmView(snapshot(), windowFor(fights, [announcement('unknown', '8000000', null)])).kills;
  expect(rows.map(r => r.id).sort()).toEqual(['announcement:unknown', 'fight:boss', 'fight:event_boss', 'fight:miniboss', 'fight:quest']);
  const unknown = killed('typed-by-kind', '7000000', { target_type: null });
  expect(realmView(snapshot(), windowFor([unknown])).kills[0].targetType).toBeNull();
  expect(realmView(snapshot(), windowFor([killed('not-killed', '7000000', { kill: null })])).kills).toHaveLength(0);
});

it('does not merge unknown types, different types, remote respawns or ambiguous simultaneous bosses', () => {
  const boss = killed('boss');
  for (const event of [announcement('unknown', '7000100', null), announcement('other', '7000100', 301), announcement('late', '30000000')]) {
    expect(realmView(snapshot(), windowFor([boss], [event])).kills).toHaveLength(2);
  }
  expect(realmView(snapshot(), windowFor([boss, killed('twin')], [announcement('ambiguous')])).kills).toHaveLength(3);
  expect(realmView(snapshot(), windowFor([boss], [announcement('a'), announcement('b')])).kills).toHaveLength(3);
});

it('pairs repeated bosses one-to-one and deterministically regardless of input order', () => {
  const early = killed('early'), late = killed('late', '12000000');
  const a = announcement('early', '7000100'), b = announcement('late', '12000100');
  const left = realmView(snapshot(), windowFor([early, late], [a, b])).kills;
  const right = realmView(snapshot(), windowFor([late, early], [b, a])).kills;
  expect(left).toEqual(right);
  expect(left.map(r => [r.id, r.killer])).toEqual([['fight:late', 'LastHit'], ['fight:early', 'LastHit']]);
});

it('never imports a prior visit, another session, dungeon run or unverified contribution', () => {
  const boss = killed('boss'), event = announcement('notice');
  const later = { ...windowFor([boss], [event]), visitStarted: '9000000', visitKey: 'visit:2' };
  expect(realmView(snapshot(), later).kills).toHaveLength(0);
  expect(realmView(snapshot(), { ...windowFor([boss], [event]), streamId: 'other' }).kills).toHaveLength(0);
  const wrong = [killed('session', '7000000', { session_id: 'other' }), killed('run', '7000000', { run_id: 'dungeon' })];
  expect(realmView(snapshot(), windowFor(wrong, [{ ...event, run_id: 'dungeon' }, { ...event, id: 'other', session_id: 'other' }])).kills).toHaveLength(0);
  const s = snapshot(); s.character!.identity_verified = false;
  expect(realmView(s, windowFor([boss], [event])).kills[0]).toMatchObject({ self: null, killer: 'LastHit' });
  const merelyAnnounced = killed('announcement-fight', '7000000', {
    kill: { at: '7000000', evidence: 'realm_announcement', provisional: true },
  });
  expect(realmView(snapshot(), windowFor([merelyAnnounced], [event])).kills).toMatchObject([{ id: 'announcement:notice', self: null }]);
});

it('reads fame from the engine and returns unknown for missing, invalid or non-realm data', () => {
  const s = snapshot(); s.instance!.visit_fame_delta = 37;
  expect(realmView(s).visitFame).toBe(37);
  // Rebuilding the UI/window cannot change a backend baseline.
  expect(realmView(structuredClone(s), emptyHomeLiveWindow()).visitFame).toBe(37);
  for (const value of [null, undefined, -1, Number.NaN]) {
    s.instance!.visit_fame_delta = value;
    expect(realmView(s).visitFame).toBeNull();
  }
  s.instance!.visit_fame_delta = 0;
  expect(realmView(s).visitFame).toBe(0);
  expect(realmView({ ...s, area_kind: 'dungeon' }).visitFame).toBeNull();
  expect(realmView(null).visitFame).toBeNull();
});

it('lists every smoke-test boss including the quest miniboss', () => {
  const types = [34550, 47927, 34605, 34489, 22023, 22018, 999];
  const names = ['Goblin Patriarch', 'Cube Deity', 'Daughter of Limon', 'The Plague Doctor', 'Hermit God', 'Pentaract Tower', 'Maiden of the Sea'];
  const fights = types.map((target_type, i) => killed(names[i], '7000000', {
    target_type, target_kind: i === 6 ? 'quest' : 'event_boss',
    kill: { at: '7000000', evidence: i < 4 ? 'announced_death_removed' : 'hp_zero_removed', provisional: false },
  }));
  expect(realmView(snapshot(), windowFor(fights)).kills.map(row => row.fightId).sort()).toEqual([...names].sort());
});
