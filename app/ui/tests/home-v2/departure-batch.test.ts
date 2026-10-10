import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { subscribeLive } from '../../src/lib/bridge/live';
import { createHomeLiveWindow as createOracle } from './departure-oracle';
import { expect, it, vi } from 'vitest';
import { fight, realmSnapshot } from './fixtures';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import { homeVisualTypes, emptyHomeVisuals } from '../../src/lib/home/visuals';
import { homeLoot } from '../../src/lib/home/v3/loot-rank';
import { realmView } from '../../src/lib/home/realm';
import { homeSnapshot } from '../home/fixtures';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import * as rail from '../../src/lib/shell/rail';

it('a 1536-fight departure preserves the sequential window with one emission and bounded focus derivation', () => {
  const source = realmSnapshot(); source.sequence = '0'; source.at = '100';
  let sequential = emptyHomeLiveWindow(), batched = emptyHomeLiveWindow();
  const derive = vi.fn(value => {
    homeVisualTypes(source, null, value);
    homeLoot(value.drops, emptyHomeVisuals());
    realmView(source, value); rail.railView(source, [], false, null, value);
  });
  const receive = vi.fn(value => { batched = value; derive(value); });
  const one = createOracle(value => { sequential = value; }), many = createHomeLiveWindow(receive);
  one.snapshot(source); many.snapshot(source);
  const entry: EventEnvelope = { stream_id: source.stream_id, sequence: '1', at: '100', event: {
    map_entered: { name: 'Synthetic Realm', kind: 'realm', at: '100', realm_instance_key: 'realm-a' } } };
  one.event(entry); many.event(entry);
  const events: EventEnvelope[] = Array.from({ length: 1536 }, (_, i) => ({ stream_id: source.stream_id,
    sequence: String(i + 2), at: '200', event: { fight_ended: fight(`ordinary-${i}`, {
      session_id: source.session!.id, run_id: null, started: '110', ended: '200', target_kind: 'ordinary',
      kill: null, player_series: null, buckets: [] }) } }));
  for (const event of events) one.event(event);
  receive.mockClear(); derive.mockClear();
  const focus = vi.spyOn(rail, 'selectBossFight');
  many.batch(() => { for (const event of events) many.event(event); });
  expect(batched).toEqual(sequential);
  expect(receive).toHaveBeenCalledTimes(1);
  expect(derive).toHaveBeenCalledTimes(1);
  expect(focus.mock.calls.length).toBeLessThanOrEqual(2);
  focus.mockRestore(); one.dispose(); many.dispose();
});

it.each(['realm', 'dungeon'] as const)('batch oracle preserves %s focus, kill corrections, bounded history, loot and map continuity', area => {
  const source = area === 'realm' ? realmSnapshot() : homeSnapshot(); source.sequence = '0'; source.at = '100';
  let expected = emptyHomeLiveWindow(), actual = emptyHomeLiveWindow();
  const single = createOracle(value => { expected = value; }), batched = createHomeLiveWindow(value => { actual = value; });
  single.snapshot(source); batched.snapshot(source);
  let sequence = 0;
  const events: EventEnvelope[] = [];
  const add = (event: EngineEvent, at = '200') => events.push({ stream_id: source.stream_id, sequence: String(++sequence), at, event });
  add({ map_entered: { name: 'Synthetic', kind: area, at: '100', realm_instance_key: area === 'realm' ? 'realm-a' : null } }, '100');
  if (area === 'dungeon') add({ run_started: source.run! }, '100');
  add({ character_updated: source.character! }, '100');
  const row = (id: string, extra = {}) => fight(id, { session_id: source.session!.id, run_id: source.run?.id ?? null,
    started: '110', ended: null, kill: null, last_hit: '150', local_last_hit: null, ...extra });
  add({ fight_started: row('focus') });
  add({ fight_started: row('quest', { target_kind: 'quest', is_current_quest: true }) });
  const kill = { at: '210', evidence: 'hp_zero_removed' as const, provisional: false };
  for (let i = 0; i < 520; i++) {
    add({ fight_ended: row(`kill-${i}`, { ended: '220', kill }) }, '220');
    add({ instance_event: { id: `announcement-${i}`, session_id: source.session!.id, run_id: source.run?.id ?? null,
      at: '210', provisional: false, kind: { type: 'realm_event_killed', boss_key: `Synthetic ${i}`, object_type: 300, killer: null } } }, '220');
    add({ bag_dropped: { ...homeSnapshot().drops[0], id: `bag-${i}`, session_id: source.session!.id,
      run_id: source.run?.id ?? null, at: '210' } }, '220');
  }
  add({ fight_ended: row('quest', { ended: '250', kill: { ...kill, at: '250' } }) }, '250');
  // A revision is allowed to retract kill evidence, without reopening another scope.
  add({ fight_updated: row('kill-519', { ended: '220', kill: null }) }, '260');
  add({ drop_updated: { ...homeSnapshot().drops[0], id: 'bag-519', session_id: source.session!.id,
    run_id: source.run?.id ?? null, at: '210', items: [1234], contents: [] } }, '260');
  add({ map_entered: { name: 'Nexus', kind: 'nexus', at: '300' } }, '300');
  add({ map_entered: { name: 'Synthetic', kind: area, at: '400', realm_instance_key: area === 'realm' ? 'realm-a' : null } }, '400');
  for (const event of events) single.event(event);
  batched.batch(() => { for (const event of events) batched.event(event); });
  expect(actual).toEqual(expected);
  if (area === 'realm') {
    expect(actual.realmKills).toHaveLength(499);
    expect(actual.drops).toHaveLength(512);
    expect(actual.realmAnnouncements).toHaveLength(500);
    expect(actual.events).toHaveLength(256);
    expect(actual.realmPeriods).toHaveLength(2);
  }
  single.dispose(); batched.dispose();
});


it.each(['boss eviction', 'selection', 'snapshot', 'resync', 'character change'] as const)(
  'frozen pre-change oracle matches every published window across %s', scenario => {
    const source = realmSnapshot(); source.sequence = '0'; source.at = '100';
    let expected = emptyHomeLiveWindow(), actual = emptyHomeLiveWindow(), sequence = 0;
    const oracle = createOracle(value => { expected = value; });
    const receive = vi.fn(value => { actual = value; });
    const live = createHomeLiveWindow(receive);
    const envelope = (event: EngineEvent, at = '200'): EventEnvelope => ({
      stream_id: source.stream_id, sequence: String(++sequence), at, event });
    const apply = (events: EventEnvelope[]) => {
      for (const event of events) oracle.event(event);
      receive.mockClear();
      live.batch(() => { for (const event of events) live.event(event); });
      expect(receive).toHaveBeenCalledTimes(1);
      expect(actual).toEqual(expected);
    };
    const row = (id: string, extra = {}) => fight(id, { session_id: source.session!.id, run_id: null,
      started: '110', ended: '190', kill: null, local_last_hit: null, last_hit: '180', ...extra });
    try {
      oracle.snapshot(source); live.snapshot(source);
      apply([
        envelope({ map_entered: { name: 'Realm', kind: 'realm', at: '100', realm_instance_key: 'realm-a' } }, '100'),
        envelope({ character_updated: source.character! }, '100'),
        envelope({ fight_ended: row('old-boss') }),
        envelope({ fight_started: row('quest', { started: '120', ended: null, target_kind: 'quest', is_current_quest: true }) }),
      ]);
      if (scenario === 'selection') {
        oracle.select('old-boss'); live.select('old-boss'); expect(actual).toEqual(expected);
      }
      if (scenario === 'snapshot') {
        const next = { ...source, sequence: String(++sequence), at: '210', fights: [row('snapshot-boss', { ended: null, local_last_hit: '210' })] };
        oracle.snapshot(next); live.snapshot(next); expect(actual).toEqual(expected);
      }
      const events: EventEnvelope[] = [];
      for (let i = 0; i < 70; i++) {
        if (scenario === 'character change' && i === 35) events.push(envelope({ character_updated: { ...source.character!, id: 'other-character' } }, '220'));
        events.push(envelope({ fight_ended: row(`ordinary-${i}`, { target_kind: 'ordinary', started: String(130 + i) }) }, '220'));
      }
      if (scenario === 'resync') {
        const state = { phase: 'resyncing' as const, historyStale: true, historyFailed: false, historyLagging: false, message: '' };
        for (const event of events.slice(0, 35)) oracle.event(event);
        oracle.state(state);
        for (const event of events.slice(35)) oracle.event(event);
        receive.mockClear();
        live.batch(() => {
          for (const event of events.slice(0, 35)) live.event(event);
          live.state(state);
          for (const event of events.slice(35)) live.event(event);
        });
        expect(receive).toHaveBeenCalledTimes(1); expect(actual).toEqual(expected);
        expect(actual.reasons).toContain('Live sequence gap; visit observations are incomplete.');
      } else apply(events);
      if (scenario === 'boss eviction') expect(actual.fights.some(row => row.id === 'old-boss')).toBe(false);
      if (scenario === 'selection') expect(actual.fights.some(row => row.id === 'old-boss')).toBe(true);
      if (scenario === 'character change') expect(actual.reasons).toContain('Character changed; visit entry was not observed.');
      // Force another same-timestamp focus decision after trim and/or reset.
      apply([envelope({ fight_ended: row('last-ordinary', { target_kind: 'ordinary', started: '250' }) }, '220')]);
    } finally { oracle.dispose(); live.dispose(); }
  });


it.each(['snapshot', 'delta'] as const)('preserves #34 snapshot-first departure evidence when a %s overtakes a batch', async kind => {
  const source = realmSnapshot(); source.sequence = '0'; source.at = '100';
  let expected = emptyHomeLiveWindow(), actual = emptyHomeLiveWindow();
  const oracle = createOracle(value => { expected = value; }), live = createHomeLiveWindow(value => { actual = value; });
  const transport = createFixtureTransport(source), sideEffects = vi.fn();
  const connection = subscribeLive(createClient(transport), {
    batch: live.batch,
    snapshot: value => { oracle.snapshot(value); live.snapshot(value); },
    event: value => { sideEffects(value); oracle.event(value); live.event(value); },
    state: value => { oracle.state(value); live.state(value); },
  });
  const event = (sequence: number, event: EngineEvent, at = '200'): EventEnvelope => ({ stream_id: source.stream_id, sequence: String(sequence), at, event });
  const entry = (sequence: number, at: string) => event(sequence, { map_entered: { name: 'Realm', kind: 'realm', at, realm_instance_key: 'realm-a' } }, at);
  const close = fight('boss', { session_id: source.session!.id, run_id: null, started: '110', ended: '200',
    kill: { at: '200', evidence: 'hp_zero_removed', provisional: false } });
  try {
    for (let i = 0; i < 40; i++) await Promise.resolve();
    transport.publishEngineEvent(entry(1, '100'));
    transport.publishEngineEvent(event(2, { character_updated: source.character! }));
    transport.publishEngineEvent(event(3, { fight_ended: close }));
    const nexus = { ...source, sequence: '6', at: '300', area_kind: 'nexus' as const, map: 'Nexus' };
    transport.publishSnapshotUpdate(kind === 'snapshot' ? nexus : { base_sequence: '0', snapshot: nexus, removed_fights: [], removed_drops: [] });
    const events = [event(4, { fight_updated: close }), event(5, { character_updated: source.character! }),
      event(6, { map_entered: { name: 'Nexus', kind: 'nexus', at: '300' } }, '300')];
    const batch = { stream_id: source.stream_id, first_sequence: '4', last_sequence: '6', events };
    transport.publishEngineEvent(batch); transport.publishEngineEvent(batch);
    expect(actual).toEqual(expected);
    transport.publishEngineEvent(entry(7, '400'));
    expect(actual).toEqual(expected);
    expect(sideEffects.mock.calls.map(([value]) => value.sequence)).toEqual(['1', '2', '3', '4', '5', '6', '7']);
    expect(actual.partial).toBe(true);
    expect(actual.reasons).toContain('Realm departure was first observed in a snapshot; the segment boundary is incomplete.');
    expect(actual.realmPeriods?.[0].ended).toBe('201');
    expect(actual.realmKills?.map(row => row.id)).toEqual(['boss']);
  } finally { connection.dispose(); oracle.dispose(); live.dispose(); }
});
