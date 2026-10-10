import { expect, it } from 'vitest';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { realmView } from '../../src/lib/home/realm';
import { scopedFights } from '../../src/lib/shell/rail';
import { fight, homeSnapshot, realmSnapshot } from './fixtures';

const key = (seed = 1486421971, opened = 1791146155) => JSON.stringify(['Realm of the Mad God', 'NexusPortal.Zephyr', seed, opened]);
function harness() {
  let value = emptyHomeLiveWindow(), sequence = 12;
  const source = realmSnapshot();
  const live = createHomeLiveWindow(next => { value = next; });
  live.snapshot(source);
  const send = (event: EngineEvent, at: string) => live.event({ stream_id: source.stream_id, sequence: String(++sequence), at, event });
  const defer = (event: EngineEvent, at: string) => {
    const envelope = { stream_id: source.stream_id, sequence: String(++sequence), at, event };
    return () => live.event(envelope);
  };
  const enter = (at: string, kind: 'realm' | 'dungeon' | 'nexus', realm_instance_key?: string) =>
    send({ map_entered: { at, name: kind, kind, realm_instance_key } }, at);
  const snapshot = (at: string, delta: number | null, overrides: Partial<LiveSnapshot> = {}) => {
    const next = { ...source, ...overrides, at, sequence: String(++sequence), instance: { ...source.instance!, visit_fame_delta: delta } };
    live.snapshot(next); return next;
  };
  const kill = (id: string, at: string) => fight(id, { session_id: source.session!.id, run_id: null, started: at, ended: at,
    kill: { at, evidence: 'hp_zero_removed', provisional: false } });
  const drop = (id: string, at: string) => ({ ...homeSnapshot().drops[0], session_id: source.session!.id, id, run_id: null, at });
  return { live, send, defer, enter, snapshot, kill, drop, source, get value() { return value; } };
}

it('reuses unchanged live projections while preserving removals and event freshness', () => {
  const h = harness(); h.enter('6000000', 'realm', key());
  const first = fight('first', { session_id: h.source.session!.id, run_id: null,
    started: '6100000', last_hit: '6100000', ended: null, kill: null });
  const second = { ...first, id: 'second' };
  h.snapshot('6200000', 0, { fights: [first, second] });
  const before = h.value;
  const repeated = h.snapshot('6300000', 0, { fights: [first, second] });
  expect(h.value).not.toBe(before);
  for (const field of ['fights', 'realmKills', 'realmAnnouncements', 'events'] as const) {
    expect(h.value[field]).toBe(before[field]);
  }
  expect(h.value.fights[0]).toBe(before.fights[0]);
  expect(h.value.fightSequences).not.toBe(before.fightSequences);
  expect(h.value.fightSequences).toEqual({ first: repeated.sequence, second: repeated.sequence });
  const sequences = h.value.fightSequences;
  h.live.snapshot(repeated);
  expect(h.value.fights).toBe(before.fights);
  expect(h.value.fightSequences).toBe(sequences);
  const changed = { ...first, target_hp: 123 };
  h.snapshot('6400000', 0, { fights: [changed] });
  expect(h.value.fights.map(row => row.id)).toEqual(['first']);
  expect(Object.keys(h.value.fightSequences!)).toEqual(['first']);
  expect(h.value.fights[0].target_hp).toBe(123);
  const newer = { ...changed, is_current_quest: true, local_last_hit: '6450000' };
  h.live.event({ stream_id: h.source.stream_id, sequence: '100', at: '6450000', event: { fight_updated: newer } });
  const eventRow = h.value.fights[0];
  const lagging = h.snapshot('6500000', 0, { fights: [changed] });
  expect(h.value.fights[0]).toBe(eventRow);
  expect(h.value.fightSequences?.first).toBe('100');
  expect(scopedFights(lagging, h.value)[0]).toMatchObject({ is_current_quest: true, local_last_hit: '6450000', target_hp: 123 });
  h.snapshot('6600000', 0, { fights: [] });
  expect(h.value.fights[0]).toBe(eventRow); // Fresher open rows survive removal.
  h.live.snapshot({ ...lagging, sequence: '101', fights: [] });
  expect(h.value.fights).toEqual([]);
  expect(h.value.fightSequences).toEqual({});
  h.live.dispose();
});

it('reuses kill summaries by source while applying revised evidence', () => {
  const h = harness(); h.enter('6000000', 'realm', key());
  const killed = { ...h.kill('killed', '6100000'), player_series: null };
  h.snapshot('6200000', 0, { fights: [killed] });
  // Focus selection can change series retention on the first publication.
  h.snapshot('6300000', 0, { fights: [killed] });
  const before = h.value;
  h.snapshot('6400000', 0, { fights: [killed] });
  expect(h.value.fights).toBe(before.fights);
  expect(h.value.realmKills).toBe(before.realmKills);
  expect(h.value.realmKills![0]).toBe(before.realmKills![0]);
  expect(h.value.realmKills![0].partial).toBe(false);
  const revised = { ...killed, players: killed.players.map(player => ({ ...player, damage: 789,
    sources: player.sources.map(source => ({ ...source, estimated: true })) })) };
  h.snapshot('6500000', 0, { fights: [revised] });
  expect(h.value.realmKills![0]).not.toBe(before.realmKills![0]);
  expect(h.value.realmKills![0]).toMatchObject({ partial: true, self: { damage: 789, sources: [] } });
  h.snapshot('6600000', 0, { fights: [{ ...revised, kill: null }] });
  expect(h.value.realmKills).toEqual([]);
  h.live.dispose();
});

it('reprojects the same source when verified identity or selected series changes', () => {
  const h = harness(); h.enter('6000000', 'realm', key());
  const template = h.source.character!;
  const source = { ...h.kill('ended', '6100000'), players: Array.from({ length: 6 }, (_, i) => ({
    ...fight('template').players[0], player_id: i === 5 ? template.id : `remote-${i}`, is_local: i === 5,
  })), player_series: [{ player_id: template.id, object_id: 1, is_local: true, points: [1] }] };
  h.live.select('different');
  h.snapshot('6200000', 0, { fights: [source], character: { ...template, identity_verified: false } });
  expect(h.value.fights[0].players.some(row => row.is_local)).toBe(false);
  expect(h.value.fights[0].player_series).toBeNull();
  const provisional = h.value, summary = h.value.realmKills![0];
  expect(summary.self?.player_id).toBe(template.id);
  expect(summary.self?.sources).toEqual([]);
  h.snapshot('6300000', 0, { fights: [source], character: template });
  expect(h.value.fights[0]).not.toBe(provisional.fights[0]);
  expect(h.value.fights[0].players.some(row => row.is_local)).toBe(true);
  expect(h.value.realmKills![0]).toBe(summary);
  h.live.select(source.id);
  h.snapshot('6400000', 0, { fights: [source] });
  expect(h.value.fights[0].player_series).toEqual(source.player_series);
  const before = h.value;
  h.snapshot('6500000', 0, { fights: [source] });
  expect(h.value.fights).toBe(before.fights);
  h.live.dispose();
});

it.each(['before map entry', 'before run start', 'without map entry'] as const)(
  'continues cached visits when snapshots arrive %s on repeated trips', order => {
    const h = harness(); h.enter('6000000', 'realm', key());
    const character = h.source.character!;
    h.send({ fight_ended: h.kill('before', '6500000') }, '6500000');
    h.send({ bag_dropped: h.drop('before-bag', '6600000') }, '6600000');
    h.snapshot('7000000', 12, { character: { ...character, fame: 112 } });
    h.send({ bag_dropped: h.drop('last-bag', '7000000') }, '7000000');
    const visit = h.value.visitKey;
    for (let trip = 0; trip < 2; trip++) {
      const at = 8000000 + trip * 10000000;
      const run = { ...homeSnapshot().run!, id: `trip-${trip}`, started: String(at) };
      const enter = h.defer({ map_entered: { at: String(at), name: 'Synthetic Dungeon', kind: 'dungeon' } }, String(at));
      const start = h.defer({ run_started: run }, String(at));
      if (order === 'before run start') enter();
      h.snapshot(String(at + 1000000), null, { area_kind: 'dungeon', run, character: { ...character, fame: 150 + trip * 50 } });
      if (order === 'before map entry') enter();
      start();
      // A second run mismatch must also retain the realm cache.
      h.send({ run_started: { ...run, id: `${run.id}-replacement` } }, String(at + 2000000));
      const back = String(at + 7000000);
      const returnToRealm = h.defer({ map_entered: { at: back, name: 'Synthetic Realm', kind: 'realm', realm_instance_key: key() } }, back);
      h.snapshot(String(at + 7500000), 3, { character: { ...character, fame: 153 + trip * 50 } });
      returnToRealm();
      h.send({ bag_dropped: h.drop(`return-${trip}`, String(at + 7600000)) }, String(at + 7600000));
      const snapshot = h.snapshot(String(at + 8000000), 8, { character: { ...character, fame: 158 + trip * 50 } });
      const view = realmView(snapshot, h.value);
      expect(h.value.visitKey).toBe(visit);
      expect(view.kills.map(row => row.fightId)).toEqual(['before']);
      expect(view.drops.map(row => row.id)).toContain('before-bag');
      expect(view.drops.map(row => row.id)).toContain('last-bag');
      expect(view.visitFame).toBe(12 + 8 * (trip + 1));
      expect(scopedFights(snapshot, h.value).map(row => row.id)).toEqual(['before']);
      if (order !== 'before run start') {
        expect(h.value.partial).toBe(true);
        expect(h.value.reasons.join(' ')).toMatch(/departure.*snapshot/i);
      }
    }
    h.live.dispose();
  },
);

it('continues realm A after a dungeon with retained kills, drops, fame and a shared rail window', () => {
  const h = harness(); h.enter('6000000', 'realm', key());
  h.send({ fight_ended: h.kill('before', '6500000') }, '6500000');
  h.send({ bag_dropped: h.drop('before-bag', '6600000') }, '6600000');
  h.snapshot('7000000', 12);
  const visit = h.value.visitKey;
  h.enter('8000000', 'dungeon');
  h.send({ run_started: homeSnapshot().run! }, '8000000');
  h.snapshot('9000000', null, { area_kind: 'dungeon', run: homeSnapshot().run });
  h.enter('15000000', 'realm', key());
  h.send({ fight_ended: h.kill('after', '16000000') }, '16000000');
  h.send({ bag_dropped: h.drop('after-bag', '16000000') }, '16000000');
  const away = h.kill('away', '10000000');
  h.send({ fight_ended: away }, '16000001');
  const snapshot = h.snapshot('17000000', 8, { fights: [away] });
  const view = realmView(snapshot, h.value);
  expect(h.value.visitKey).toBe(visit);
  expect(view.kills.map(row => row.fightId).sort()).toEqual(['after', 'before']);
  expect(view.drops.map(row => row.id)).toEqual(['before-bag', 'after-bag']);
  expect(view.visitFame).toBe(20);
  expect(view.visitElapsed).toBe('4000000');
  expect(scopedFights(snapshot, h.value).map(row => row.id).sort()).toEqual(['after', 'before']);
  h.live.dispose();
});

it.each([key(1486421972), key(1486421971, 1791146156)])('starts a different instance after Nexus and can return to the earlier instance (%s)', next => {
  const h = harness(); h.enter('6000000', 'realm', key());
  h.send({ bag_dropped: h.drop('a', '6000001') }, '6000001');
  const first = h.value.visitKey;
  h.enter('8000000', 'nexus'); h.snapshot('9000000', null, { area_kind: 'nexus' });
  h.enter('10000000', 'realm', next);
  expect(h.value.visitKey).not.toBe(first); expect(h.value.drops).toEqual([]);
  h.enter('12000000', 'realm', key());
  expect(h.value.visitKey).toBe(first); expect(h.value.drops.map(row => row.id)).toEqual(['a']);
  h.live.dispose();
});

it('falls back per entry with a partial reason when the realm key is missing', () => {
  const h = harness(); h.enter('6000000', 'realm'); const first = h.value.visitKey;
  h.send({ bag_dropped: h.drop('a', '6000001') }, '6000001');
  h.enter('8000000', 'realm');
  expect(h.value.visitKey).not.toBe(first); expect(h.value.drops).toEqual([]);
  expect(h.value.partial).toBe(true); expect(h.value.reasons.join(' ')).toMatch(/instance key.*missing/i);
  h.live.dispose();
});

it('does not resume cached visits across sessions or capture gaps', () => {
  const h = harness(); h.enter('6000000', 'realm', key());
  h.send({ bag_dropped: h.drop('a', '6000001') }, '6000001');
  h.enter('8000000', 'nexus');
  h.send({ session_updated: { ...h.source.session!, id: 'new-session' } }, '9000000');
  h.enter('10000000', 'realm', key());
  expect(h.value.drops).toEqual([]);
  h.live.dispose();
});

it('keeps final fame events between snapshots and excludes gains while away without double counting', () => {
  const h = harness(); h.enter('6000000', 'realm', key());
  const character = h.source.character!;
  h.send({ character_updated: { ...character, fame: 100 } }, '6100000');
  h.snapshot('7000000', 5, { character: { ...character, fame: 105 } });
  h.send({ character_updated: { ...character, fame: 109 } }, '7100000');
  h.enter('8000000', 'nexus');
  h.send({ character_updated: { ...character, fame: 150 } }, '9000000');
  h.enter('10000000', 'realm', key());
  h.send({ character_updated: { ...character, fame: 150 } }, '10100000');
  const next = h.snapshot('11000000', 3, { character: { ...character, fame: 153 } });
  expect(realmView(next, h.value).visitFame).toBe(12);
  h.snapshot('12000000', 3, { character: { ...character, fame: 153 } });
  expect(realmView(next, h.value).visitFame).toBe(12);
  h.live.dispose();
});

it('keeps unknown fame unknown across a return and retires cached visits on a gap', () => {
  const h = harness(); h.enter('6000000', 'realm', key());
  const unknown = h.snapshot('7000000', null);
  h.send({ character_updated: { ...h.source.character!, fame: (h.source.character!.fame ?? 0) + 10 } }, '7000001');
  expect(realmView(unknown, h.value).visitFame).toBeNull();
  h.send({ bag_dropped: h.drop('a', '7000001') }, '7000001');
  h.enter('8000000', 'nexus'); h.enter('10000000', 'realm', key());
  const next = h.snapshot('11000000', 3);
  expect(realmView(next, h.value).visitFame).toBeNull();
  h.enter('12000000', 'nexus');
  h.live.state({ phase: 'resyncing', historyStale: true, historyFailed: false, historyLagging: false, message: '' });
  h.enter('13000000', 'realm', key());
  expect(h.value.drops).toEqual([]);
  h.live.dispose();
});

it('marks a visit partial after the bounded session cache evicts earlier instances', () => {
  const h = harness(); h.enter('6000000', 'realm', key());
  h.send({ bag_dropped: h.drop('a', '6000001') }, '6000001');
  for (let i = 1; i <= 10; i++) h.enter(String(6000000 + i * 1000000), 'realm', key(i));
  h.enter('17000000', 'realm', key());
  expect(h.value.drops).toEqual([]); expect(h.value.partial).toBe(true);
  expect(h.value.reasons.join(' ')).toMatch(/bounded session window/);
  h.live.dispose();
});

it('does not merge a return announcement into a kill from an earlier realm segment', () => {
  const h = harness(); h.enter('6000000', 'realm', key());
  h.send({ fight_ended: h.kill('before', '6500000') }, '6500000');
  h.enter('8000000', 'nexus'); h.enter('15000000', 'realm', key());
  h.send({ instance_event: { id: 'return-announcement', session_id: h.source.session!.id, run_id: null,
    at: '16000000', kind: { type: 'realm_event_killed', boss_key: 'Synthetic_Boss', object_type: 300 }, provisional: true } }, '16000000');
  const view = realmView(h.snapshot('17000000', 0), h.value);
  expect(view.kills).toHaveLength(2);
  expect(view.kills.map(row => row.id).sort()).toEqual(['announcement:return-announcement', 'fight:before']);
  h.live.dispose();
});
