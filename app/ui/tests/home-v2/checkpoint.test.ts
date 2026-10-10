import { fireEvent, screen, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import type { EngineEvent } from '../../src/lib/bindings/EngineEvent';
import type { FightView } from '../../src/lib/bindings/FightView';
import type { LiveSnapshot } from '../../src/lib/bindings/LiveSnapshot';
import type { LiveState } from '../../src/lib/bridge/live';
import { createHomeLiveWindow, emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { createHomeHistory } from '../../src/lib/home/summary';
import { createHomeObservation } from '../../src/lib/home/mode';
import { connectShell } from '../../src/lib/shell/connection';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { deferred, flush, homeSnapshot, homeSummary, saved } from '../home/fixtures';
import { flushHomeRender, mountHome } from '../home/shell-fixture';

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });
const live: LiveState = { phase: 'live', historyStale: false, historyFailed: false, historyLagging: false, message: '' };
function harness(realm = false) {
  const snapshot = homeSnapshot(realm ? { area_kind: 'realm', run: null } : {});
  let value = emptyHomeLiveWindow(), sequence = 12;
  const window = createHomeLiveWindow(next => { value = next; });
  window.snapshot(snapshot); window.state(live);
  const send = (event: EngineEvent, at = '6000000') => window.event({
    stream_id: snapshot.stream_id, sequence: String(++sequence), at, event,
  });
  const enter = (known = true) => send({ map_entered: { at: '6000000', name: 'Same Realm', kind: 'realm', realm_instance_key: known ? 'synthetic-instance' : null } });
  const publishSnapshot = (overrides: Partial<LiveSnapshot>) => window.snapshot({ ...snapshot, ...overrides, sequence: String(++sequence) });
  const drop = (id: string) => ({ ...snapshot.drops[0], id, run_id: realm ? null : snapshot.run!.id, at: '6000001' });
  return { window, snapshot, send, enter, drop, publishSnapshot, get value() { return value; } };
}

it.each(['realm', 'dungeon'] as const)('%s visit survives engine identity establishment and interleaved snapshots', area_kind => {
  const h = harness(area_kind === 'realm');
  const verified = h.snapshot.character!;
  const provisional = { ...verified, id: `${h.snapshot.stream_id}:provisional:2:2`, identity_verified: false };
  const run = area_kind === 'dungeon' ? { ...h.snapshot.run!, id: 'new-run', character_id: null, started: '6000000' } : null;
  h.send({ map_entered: { at: '6000000', name: 'New map', kind: area_kind, realm_instance_key: area_kind === 'realm' ? 'synthetic-instance' : null } });
  if (run) h.send({ run_started: run });
  const visitKey = h.value.visitKey;
  const assertVisit = () => expect(h.value).toMatchObject({ visitKey, runId: run?.id ?? null, partial: false, reasons: [] });
  h.publishSnapshot({ area_kind, run, character: null, fights: [], drops: [] });
  assertVisit();
  h.send({ character_updated: provisional }); assertVisit();
  h.publishSnapshot({ area_kind, run, character: provisional, fights: [], drops: [] }); assertVisit();
  h.send({ character_updated: verified }); assertVisit();
  h.publishSnapshot({ area_kind, run, character: verified, fights: [], drops: [] }); assertVisit();
  h.send({ character_merged: { provisional_id: provisional.id, verified_id: verified.id } }); assertVisit();
  h.send({ bag_dropped: { ...h.drop('new-bag'), run_id: run?.id ?? null } });
  h.send({ fight_started: { ...h.snapshot.fights[0], id: 'new-fight', run_id: run?.id ?? null, started: '6000001', player_series: null } });
  h.send({ instance_event: { id: 'new-event', session_id: h.snapshot.session!.id, run_id: run?.id ?? null,
    at: '6000001', kind: { type: 'player_left', object_id: 2, player_name: null, class_id: null }, provisional: true } });
  assertVisit();
  expect(h.value.drops.map(row => row.id)).toEqual(['new-bag']);
  expect(h.value.fights.map(row => row.id)).toEqual(['new-fight']);
  expect(h.value.events.map(row => row.id)).toEqual(['new-event']);
  h.window.dispose();
});

it.each(['event', 'snapshot'] as const)('a different verified character resets even through a provisional identity (%s)', delivery => {
  const h = harness(true); h.enter();
  const provisional = { ...h.snapshot.character!, id: 'per-map-provisional', identity_verified: false };
  h.send({ character_updated: provisional });
  h.send({ bag_dropped: h.drop('old-character-bag') });
  const switched = { ...h.snapshot.character!, id: 'different-verified-character' };
  if (delivery === 'event') h.send({ character_updated: switched });
  else h.publishSnapshot({ character: switched });
  expect(h.value).toMatchObject({ visitKey: null, drops: [], fights: [], events: [], partial: true });
  h.window.dispose();
});

it('snapshot-only identity establishment and merge notifications preserve the accepted entry', () => {
  const h = harness(true); h.enter();
  const visitKey = h.value.visitKey;
  const provisional = { ...h.snapshot.character!, id: 'snapshot-provisional', identity_verified: false };
  h.publishSnapshot({ character: provisional });
  h.send({ bag_dropped: h.drop('during-establishment') });
  h.send({ character_merged: { provisional_id: provisional.id, verified_id: h.snapshot.character!.id } });
  expect(h.value).toMatchObject({ visitKey, partial: false });
  h.publishSnapshot({ character: h.snapshot.character });
  expect(h.value).toMatchObject({ visitKey, partial: false, reasons: [] });
  expect(h.value.drops.map(row => row.id)).toEqual(['during-establishment']);
  h.window.dispose();
});

it.each(['event', 'snapshot'] as const)('identity replacement without a new map entry invalidates continuity (%s)', delivery => {
  const h = harness(true); h.enter();
  h.send({ character_updated: h.snapshot.character! });
  h.send({ bag_dropped: h.drop('before-reconnect') });
  const provisional = { ...h.snapshot.character!, id: 'reconnect-provisional', identity_verified: false };
  if (delivery === 'event') h.send({ character_updated: provisional });
  else h.publishSnapshot({ character: provisional });
  expect(h.value).toMatchObject({ visitKey: null, drops: [], partial: true });
  h.window.dispose();
});

it('series omission, sampling and selection do not mark visit coverage partial', () => {
  const h = harness(true); h.enter();
  const buckets = Array.from({ length: 241 }, (_, index) => ({ from: String(6000000 + index), until: String(6000001 + index), total: 1, local: null }));
  // An ordinary fight cannot become the retained realm boss focus.
  const fight = { ...h.snapshot.fights[0], id: 'ended', target_kind: 'ordinary' as const, run_id: null, started: '6000001', ended: '6000241', buckets,
    player_series: [{ player_id: 'p', object_id: 1, is_local: false, points: buckets.map(() => 1) }] };
  h.window.select('another-fight');
  h.send({ fight_ended: fight });
  expect(h.value.fights[0].player_series).toBeNull();
  expect(h.value.fights[0].buckets).toHaveLength(120);
  expect(h.value).toMatchObject({ partial: false, reasons: [] });
  h.window.select(fight.id); h.send({ fight_ended: fight });
  expect(h.value.fights[0].player_series![0].points).toHaveLength(120);
  h.window.select(null);
  expect(h.value.fights[0].player_series).toBeNull();
  h.send({ fight_started: { ...fight, id: 'active', ended: null, player_series: null } });
  expect(h.value).toMatchObject({ partial: false, reasons: [] });
  h.window.dispose();
});

it('same_name_map_entry_starts_new_visit', () => {
  const h = harness(true);
  expect(h.value.drops).toEqual([]);
  expect(h.value.partial).toBe(true);
  h.enter(false); const firstKey = h.value.visitKey;
  h.send({ bag_dropped: h.drop('first') });
  expect(h.value.drops.map(row => row.id)).toEqual(['first']);
  h.enter(false);
  expect(h.value.visitKey).not.toBe(firstKey);
  expect(h.value.drops).toEqual([]);
  expect(h.value.partial).toBe(true);
  h.send({ map_entered: { at: '7000000', name: 'Dungeon', kind: 'dungeon' } });
  h.send({ bag_dropped: h.drop('wrong-area') });
  expect(h.value.drops).toEqual([]);
  h.enter(false); expect(h.value.drops).toEqual([]);
  h.window.dispose();
});

it('home_window_bounded_route_independent_and_gap_reset', async () => {
  vi.useFakeTimers();
  const snapshot = homeSnapshot({ area_kind: 'realm', run: null });
  const transport = createFixtureTransport(snapshot);
  const owner = createClient(transport);
  let value = emptyHomeLiveWindow();
  const event = vi.fn(() => expect(value.drops.map(row => row.id)).toContain('away-from-home'));
  const release = connectShell(owner, { snapshot: vi.fn(), capture: vi.fn(), store: vi.fn(), visuals: vi.fn(), error: vi.fn(),
    homeWindow: next => { value = next; }, event: next => { if ('bag_dropped' in next.event) event(); } });
  await flush();
  transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: '13', at: '6000000',
    event: { map_entered: { at: '6000000', name: 'Realm', kind: 'realm' } } });
  transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: '14', at: '6000001',
    event: { bag_dropped: { ...snapshot.drops[0], id: 'away-from-home', run_id: null, at: '6000001' } } });
  // No route listener or route mount exists, and the window precedes event props.
  expect(event).toHaveBeenCalledOnce();
  const pending = deferred<typeof snapshot>(); owner.querySnapshot = vi.fn(() => pending.promise);
  transport.publishEngineEvent({ stream_id: snapshot.stream_id, sequence: '16', at: '6000002',
    event: { bag_dropped: { ...snapshot.drops[0], id: 'gap', run_id: null, at: '6000002' } } });
  expect(value).toMatchObject({ visitKey: null, partial: true, drops: [] });
  pending.resolve({ ...snapshot, sequence: '16' }); await flush();
  expect(value.drops).toEqual([]);
  release();
  expect(value.streamId).toBe(snapshot.stream_id);
  expect(value.drops).toEqual([]);
});

it('caps every collection with deterministic oldest eviction and selected fight retention', () => {
  const h = harness();
  h.window.select('selected');
  const ended: FightView = { ...h.snapshot.fights[0], id: 'selected', started: '1', ended: '6000000' };
  h.send({ fight_ended: ended });
  for (let index = 0; index < 300; index++) {
    const id = String(index).padStart(3, '0');
    h.send({ fight_ended: { ...ended, id: `fight-${id}`, started: String(index + 10) } });
    h.send({ bag_dropped: h.drop(`drop-${id}`) });
    h.send({ instance_event: { id: `event-${id}`, session_id: h.snapshot.session!.id, run_id: h.snapshot.run!.id,
      at: '6000001', kind: { type: 'player_left', object_id: index, player_name: null, class_id: null }, provisional: true } });
  }
  expect(h.value.fights).toHaveLength(64);
  expect(h.value.fights.some(row => row.id === 'selected')).toBe(true);
  expect(h.value.drops).toHaveLength(128);
  expect(h.value.drops[0].id).toBe('drop-172');
  expect(h.value.events).toHaveLength(256);
  expect(h.value.events[0].id).toBe('event-044');
  expect(h.value.partial).toBe(true);
  const revised = { ...h.drop('drop-299'), items: [42, 42] };
  h.send({ drop_updated: revised });
  expect(h.value.drops).toHaveLength(128);
  expect(h.value.drops.find(row => row.id === revised.id)?.items).toEqual([42, 42]);
  h.window.dispose();
});

it('snapshots supersede matching projections including omitted series; ended copies stay bounded and immutable', () => {
  const h = harness();
  const fight = h.snapshot.fights[0];
  const full: FightView = { ...fight, player_series: [{ player_id: 'p', object_id: 1, is_local: false, points: [1] }] };
  h.send({ fight_updated: full });
  h.window.snapshot({ ...h.snapshot, sequence: '20', fights: [{ ...full, total_damage: 123, player_series: null }] });
  h.send({ fight_updated: full }); // Covered event cannot override the newer snapshot.
  expect(h.value.fights.find(row => row.id === fight.id)).toMatchObject({ total_damage: 123, player_series: null });
  const buckets = Array.from({ length: 241 }, (_, i) => ({ from: String(i * 1000000), until: String((i + 1) * 1000000), total: i, local: i === 1 ? null : i }));
  const ended = { ...full, id: 'ended', buckets, ended: '241000000', player_series: [{ player_id: 'p', object_id: 1, is_local: false, points: buckets.map(row => row.total) }] };
  h.window.select(ended.id); h.send({ fight_ended: ended });
  const retained = h.value.fights.find(row => row.id === ended.id)!;
  expect(retained.buckets).toHaveLength(120);
  expect(retained.buckets[1].local).toBeNull();
  expect(retained.player_series![0].points).toHaveLength(120);
  expect(retained.total_damage).toBe(ended.total_damage);
  expect(ended.buckets).toHaveLength(241);
  expect(ended.player_series[0].points).toHaveLength(241);
  h.window.snapshot({ ...h.snapshot, sequence: '21', fights: [] });
  expect(h.value.fights.some(row => row.id === ended.id)).toBe(true);
  h.window.select(null);
  expect(h.value.fights.find(row => row.id === ended.id)?.player_series).toBeNull();
  h.window.dispose();
});

it('Realm snapshots cannot seed loot or pre-entry fights; gaps require a new entry and namespace changes retire everything', () => {
  const h = harness(true); h.enter();
  const fight = { ...h.snapshot.fights[0], run_id: null, started: '6000001' };
  h.window.snapshot({ ...h.snapshot, sequence: '14', fights: [fight], drops: [h.drop('unobserved')] });
  expect(h.value.fights).toHaveLength(1); expect(h.value.drops).toEqual([]);
  h.window.state({ ...live, phase: 'resyncing' });
  h.window.snapshot({ ...h.snapshot, sequence: '15', fights: [fight] });
  expect(h.value.fights).toEqual([]); expect(h.value.visitKey).toBeNull();
  h.enter();
  h.window.state({ ...live, phase: 'resyncing', historyStale: true });
  expect(h.value.visitKey).not.toBeNull(); // Repeated durability state is not another gap.
  expect(h.value.partial).toBe(false);
  h.window.state(live);
  h.send({ bag_dropped: h.drop('observed') });
  for (const overrides of [{ stream_id: 'new-source' }, { session: { ...h.snapshot.session!, id: 'new-session' } }, { character: { ...h.snapshot.character!, id: 'new-character' } }]) {
    const isolated = harness(true); isolated.enter(); isolated.send({ bag_dropped: isolated.drop('old') });
    isolated.window.snapshot({ ...isolated.snapshot, ...overrides, sequence: '20' });
    expect(isolated.value.drops).toEqual([]); expect(isolated.value.visitKey).toBeNull();
    isolated.window.dispose();
  }
  h.window.dispose();
});

it('keeps supplied top ranks plus verified self without synthesizing kill or population evidence', () => {
  const h = harness();
  const local = { ...h.snapshot.fights[0].players[0], player_id: h.snapshot.character!.id, is_local: true, rank: 80, share: 0.01 };
  const players = Array.from({ length: 6 }, (_, index) => ({ ...local, player_id: `remote-${index}`, is_local: false, rank: index + 1 }));
  h.send({ fight_ended: { ...h.snapshot.fights[0], id: 'ended-without-kill', players: [...players, local], ended: '6000000', kill: null } });
  const retained = h.value.fights.find(row => row.id === 'ended-without-kill')!;
  expect(retained.players.map(row => row.rank)).toEqual([1, 2, 3, 4, 80]);
  expect(retained.players[4].share).toBe(0.01);
  expect(retained.kill).toBeNull();
  expect(h.value.events).toEqual([]);
  h.window.snapshot({ ...h.snapshot, sequence: '20', instance: { context: { instance_cap: null, realm_name: null,
    entered_from_realm: false, difficulty: null, game_opened_epoch_s: null, instance_age_micros: null, modifiers: [], grade: null },
    population: null, population_peak: null, population_provisional: true, quest_target: null, realm_score: null, events_dropped: 1 } });
  expect(h.value.reasons).toContain('Capture omitted instance events.');
  h.window.dispose();
});

it('leaving Nexus cancels queued queries and discards in-flight replies', async () => {
  const pending = deferred<{ status: 'ready'; value: ReturnType<typeof homeSummary> }>();
  const queryHome = vi.fn(() => pending.promise), receive = vi.fn();
  const controller = createHomeHistory({ queryHome }, receive);
  const input = { scope: { session: { id: 'demo-session' } }, streamId: 'live-synthetic-home', required: null, store: saved() };
  controller.update({ ...input, mode: 'nexus' });
  controller.update({ ...input, mode: 'realm' }); await flush();
  expect(queryHome).not.toHaveBeenCalled();
  controller.update({ ...input, mode: 'nexus' }); await flush();
  expect(queryHome).toHaveBeenCalledOnce();
  controller.update({ ...input, mode: 'realm' });
  pending.resolve({ status: 'ready', value: homeSummary() }); await flush();
  expect(receive).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'stale', summary: null }));
  controller.dispose();
});

it('recovery retains only labelled stale observations for 1500ms; stop and disposal clear them', async () => {
  vi.useFakeTimers();
  const h = harness();
  h.window.capture({ ...h.snapshot.capture, state: 'Recovering' });
  expect(h.value.reasons).toContain('Stale observations during capture recovery.');
  expect(h.value.fights.length).toBeGreaterThan(0);
  h.window.snapshot({ ...h.snapshot, sequence: '13' });
  await vi.advanceTimersByTimeAsync(1500);
  expect(h.value.fights).toEqual([]);
  h.window.capture(h.snapshot.capture);
  h.window.snapshot({ ...h.snapshot, sequence: '14' });
  expect(h.value.fights.length).toBeGreaterThan(0);
  h.window.capture({ ...h.snapshot.capture, state: 'Stopped' });
  expect(h.value.fights).toEqual([]);
  h.window.dispose(); h.send({ bag_dropped: h.drop('after-dispose') });
  expect(h.value.drops).toEqual([]);
});

it('history_only_queries_nexus', async () => {
  const queryHome = vi.fn(async () => ({ status: 'ready' as const, value: homeSummary() }));
  const controller = createHomeHistory({ queryHome }, vi.fn());
  const input = { scope: { session: { id: 'demo-session' } }, streamId: 'live-synthetic-home', required: null, store: saved() };
  for (const mode of ['realm', 'dungeon', 'realm'] as const) {
    controller.update({ ...input, mode }); controller.retry(); await flush();
  }
  expect(queryHome).not.toHaveBeenCalled();
  controller.update({ ...input, mode: 'nexus' }); await flush();
  expect(queryHome).toHaveBeenCalledOnce(); controller.dispose();

  const page = mountHome(homeSnapshot({ area_kind: 'realm', run: null }));
  await flushHomeRender();
  expect(screen.queryByRole('group', { name: 'Saved history period' })).toBeNull();
  expect(page.query).not.toHaveBeenCalled();
  const rail = within(screen.getByRole('complementary', { name: 'Character' }));
  await fireEvent.click(rail.getByRole('link', { name: 'Settings' })); await flushHomeRender();
  await fireEvent.click(rail.getByRole('link', { name: 'Home' })); await flushHomeRender();
  expect(page.query).not.toHaveBeenCalled();
  page.unmount();
});

it('Realm uses the same observation recovery deadline as Dungeon', () => {
  const snapshot = homeSnapshot({ area_kind: 'realm', run: null });
  const observe = createHomeObservation();
  expect(observe(snapshot, snapshot.capture, live, 0).mode.mode).toBe('realm');
  const recovering = { ...snapshot.capture, state: 'Recovering' as const };
  expect(observe(snapshot, recovering, live, 100)).toMatchObject({ mode: { mode: 'realm', stale: true }, delay: 1500 });
  expect(observe(snapshot, recovering, live, 1600).mode.mode).toBe('nexus');
});

it('mid-visit focus is shared without claiming visit coverage and resets on entry', () => {
  const h = harness(true);
  const first = { ...h.snapshot.fights[0], id: 'first', run_id: null, last_hit: '8000000' };
  const second = { ...first, id: 'second', last_hit: '7000000' };
  h.publishSnapshot({ at: '8000000', fights: [first, second] });
  expect(h.value).toMatchObject({ focusedFightId: 'first', visitKey: null, fights: [], partial: true });
  h.publishSnapshot({ at: '9000000', fights: [first, { ...second, last_hit: '9000000' }] });
  expect(h.value.focusedFightId).toBe('first');
  h.enter();
  expect(h.value).toMatchObject({ focusedFightId: null, fights: [], partial: false, reasons: [] });
  h.window.dispose();
});
