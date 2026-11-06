import { afterEach, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import { subscribeLive } from '../src/lib/bridge/live';
import { createVisibilityPause, observePresentationVisibility, setNativePresentationVisible } from '../src/lib/bridge/visibility';
import { connectShell } from '../src/lib/shell/connection';
import { createHomeLiveWindow } from '../src/lib/home/v2/live';
import DungeonHome from '../src/lib/home/DungeonHome.svelte';
import { dungeonObservations, dungeonTimeline } from '../src/lib/home/dungeon';
import { createRunHistory, type HistoryInput } from '../src/lib/runs/history';
import { createHomeReads } from '../src/lib/home/v3/dungeon-bests';
import type { EngineEvent } from '../src/lib/bindings/EngineEvent';
import type { HomeLiveWindow } from '../src/lib/home/v2/contracts';
import { catalog, fight, homeSnapshot, realmSnapshot } from './home-v2/fixtures';
import fixture from './fixtures/t0-live';

vi.mock('../src/lib/home/v2/live', async importOriginal => {
  const actual = await importOriginal<typeof import('../src/lib/home/v2/live')>();
  return { ...actual, createHomeLiveWindow: vi.fn(actual.createHomeLiveWindow) };
});
const releases: (() => void)[] = [];
const flush = async () => { for (let i = 0; i < 60; i++) await Promise.resolve(); };
const hidden = (value: boolean) => {
  Object.defineProperty(document, 'visibilityState', { configurable: true, value: value ? 'hidden' : 'visible' });
  document.dispatchEvent(new Event('visibilitychange'));
};
afterEach(() => {
  releases.splice(0).forEach(release => release());
  hidden(false); setNativePresentationVisible(true);
  vi.useRealTimers(); vi.clearAllMocks();
});
const saved = (sequence = fixture.sequence) => ({ state: 'ready' as const, queued: 0, durable: sequence, stream_id: fixture.stream_id, message: '' });

it('combines document and native visibility and releases waiters on disposal', async () => {
  const seen = vi.fn(); const release = observePresentationVisibility(seen);
  hidden(true); setNativePresentationVisible(false); hidden(false);
  expect(seen).toHaveBeenLastCalledWith(false);
  const pause = createVisibilityPause(); let completed: boolean | undefined;
  void pause.wait().then(value => { completed = value; }); await flush();
  expect(completed).toBeUndefined(); pause.dispose(); await flush();
  expect(completed).toBe(false);
  release(); seen.mockClear(); setNativePresentationVisible(true);
  expect(seen).not.toHaveBeenCalled();
});

it('stops storage polling and snapshots while hidden but delivers events once without a restore resync', async () => {
  vi.useFakeTimers();
  const transport = createFixtureTransport(fixture); const client = createClient(transport);
  const query = vi.spyOn(client, 'querySnapshot');
  const poll = vi.spyOn(client, 'queryStore').mockResolvedValue(saved('50'));
  const snapshot = vi.fn(), event = vi.fn(), baseline = vi.fn(), state = vi.fn();
  const live = subscribeLive(client, { snapshot, event, state, historyBaseline: baseline }); releases.push(live.dispose);
  await flush(); expect(query).toHaveBeenCalledOnce();
  hidden(true); await flush();
  const polls = poll.mock.calls.length; snapshot.mockClear();
  transport.publishSnapshot({ ...fixture, sequence: '50' });
  transport.publishEngineEvent({ stream_id: fixture.stream_id, sequence: String(BigInt(fixture.sequence) + 1n), at: fixture.at,
    event: { player_died: { at: fixture.at, character_id: null, run_id: null } } });
  await vi.advanceTimersByTimeAsync(60_000); await flush();
  expect(poll).toHaveBeenCalledTimes(polls); expect(snapshot).not.toHaveBeenCalled(); expect(event).toHaveBeenCalledOnce();
  hidden(false); await flush();
  expect(query).toHaveBeenCalledTimes(2);
  expect(snapshot).toHaveBeenLastCalledWith(expect.objectContaining({ sequence: '50' }));
  expect(baseline).toHaveBeenLastCalledWith(expect.objectContaining({ sequence: '50' }));
  expect(state).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'live' }));
  expect(event).toHaveBeenCalledOnce();
  expect(state.mock.calls.some(([value]) => value.phase === 'resyncing')).toBe(false);
});

it('rejects an in-flight pre-hide query and reseeds even if no new sequence arrives', async () => {
  const client = createClient(createFixtureTransport(fixture));
  let finish!: (value: typeof fixture) => void;
  const query = vi.spyOn(client, 'querySnapshot').mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const snapshot = vi.fn(); const live = subscribeLive(client, { snapshot }); releases.push(live.dispose);
  await flush(); setNativePresentationVisible(false); finish({ ...fixture, sequence: '999' }); await flush();
  expect(snapshot).not.toHaveBeenCalled(); setNativePresentationVisible(true); await flush();
  expect(query).toHaveBeenCalledTimes(2); expect(snapshot).toHaveBeenLastCalledWith(fixture);
  setNativePresentationVisible(false); setNativePresentationVisible(true); await flush();
  expect(snapshot).toHaveBeenCalledTimes(2);
});

it.each([false, true])('establishes a hidden baseline and preserves Home through over 512 messages (retry: %s)', async retry => {
  vi.useFakeTimers(); hidden(true);
  const source = realmSnapshot();
  const transport = createFixtureTransport(source), client = createClient(transport);
  const query = vi.spyOn(client, 'querySnapshot');
  if (retry) query.mockRejectedValueOnce(new Error('Transient baseline failure'));
  const homeWindow = vi.fn(), state = vi.fn(), event = vi.fn();
  const connection = connectShell(client, { snapshot: vi.fn(), homeWindow, state, event,
    capture: vi.fn(), visuals: vi.fn(), store: vi.fn(), error: vi.fn() });
  releases.push(connection); await flush();
  if (retry) await vi.advanceTimersByTimeAsync(250);
  expect(query).toHaveBeenCalledTimes(retry ? 2 : 1);
  let sequence = BigInt(source.sequence);
  const send = (event: EngineEvent, at: string) => transport.publishEngineEvent({ stream_id: source.stream_id, sequence: String(++sequence), at, event });
  send({ map_entered: { at: '6000000', kind: 'realm', name: 'Synthetic Realm', realm_instance_key: 'realm-a' } }, '6000000');
  const visitKey = `${source.stream_id}:${sequence}`;
  const bag = { ...homeSnapshot().drops[0], id: 'hidden-baseline-bag', at: '6100000', run_id: null, session_id: source.session!.id };
  send({ bag_dropped: bag }, bag.at);
  // Separate deliveries exceed the recovery message cap, but a healthy hidden
  // subscription drains its baseline and keeps processing ordered events.
  for (let i = 0; i < 513; i++) {
    send({ session_updated: source.session! }, '6200000');
    await flush();
  }
  expect(homeWindow).not.toHaveBeenCalled();
  transport.publishSnapshot({ ...source, sequence: String(sequence), at: '6300000' });
  hidden(false); await flush();
  expect(state).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'live' }));
  expect(state.mock.calls.some(([value]) => value.phase === 'failed')).toBe(false);
  expect(event).toHaveBeenCalledTimes(515);
  expect(homeWindow).toHaveBeenLastCalledWith(expect.objectContaining({ visitKey, partial: false,
    drops: [expect.objectContaining({ id: bag.id })], realmPeriods: [expect.objectContaining({ started: '6000000', ended: null })] }));
  expect(query).toHaveBeenCalledTimes(retry ? 3 : 2);
  expect(createHomeLiveWindow).toHaveBeenCalledOnce();
});

it('rejects restore pushes and queries older than hidden events but re-shows the same sequence', async () => {
  const transport = createFixtureTransport(fixture), client = createClient(transport);
  const snapshot = vi.fn(), project = vi.fn(), state = vi.fn();
  const live = subscribeLive(client, { snapshot, project, state }); releases.push(live.dispose);
  await flush(); setNativePresentationVisible(false); snapshot.mockClear();
  const sequence = String(BigInt(fixture.sequence) + 1n);
  transport.publishEngineEvent({ stream_id: fixture.stream_id, sequence, at: fixture.at,
    event: { player_died: { at: fixture.at, character_id: null, run_id: null } } });
  expect(project).toHaveBeenCalledOnce();
  let finish!: (value: typeof fixture) => void;
  vi.spyOn(client, 'querySnapshot').mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  setNativePresentationVisible(true);
  transport.publishSnapshot(fixture); // Native restore push of the last materialized snapshot.
  finish(fixture); await flush();
  expect(snapshot).not.toHaveBeenCalled();
  expect(state).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'live' }));
  transport.publishSnapshot({ ...fixture, sequence });
  expect(snapshot).toHaveBeenCalledOnce();
  expect(snapshot).toHaveBeenLastCalledWith(expect.objectContaining({ sequence }));
  setNativePresentationVisible(false); setNativePresentationVisible(true); await flush();
  expect(snapshot).toHaveBeenCalledTimes(2);
  expect(snapshot).toHaveBeenLastCalledWith(expect.objectContaining({ sequence }));
});

it('delivers events during and after a restore query that overtakes the native event queue', async () => {
  const transport = createFixtureTransport(fixture), client = createClient(transport);
  const event = vi.fn(), state = vi.fn(), project = vi.fn();
  const live = subscribeLive(client, { snapshot: vi.fn(), event, state, project }); releases.push(live.dispose);
  await flush();
  setNativePresentationVisible(false);
  let finish!: (value: typeof fixture) => void;
  vi.spyOn(client, 'querySnapshot').mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  setNativePresentationVisible(true);
  const seq = BigInt(fixture.sequence);
  const envelope = (sequence: bigint) => ({ stream_id: fixture.stream_id, sequence: String(sequence), at: fixture.at,
    event: { player_died: { at: fixture.at, character_id: null, run_id: null } } });
  transport.publishEngineEvent(envelope(seq + 1n));
  expect(event).toHaveBeenCalledOnce();
  finish({ ...fixture, sequence: String(seq + 3n) }); await flush();
  transport.publishEngineEvent({ stream_id: fixture.stream_id, first_sequence: String(seq + 2n), last_sequence: String(seq + 3n),
    events: [envelope(seq + 2n), envelope(seq + 3n)] });
  expect(event.mock.calls.map(([value]) => value.sequence)).toEqual([1n, 2n, 3n].map(n => String(seq + n)));
  expect(project).toHaveBeenCalledOnce(); // Snapshot-covered events still drive side effects, never numeric projection.
  expect(state.mock.calls.some(([value]) => value.phase === 'resyncing')).toBe(false);
});

it('retains buffered hidden gap events and still reports actual sequence loss', async () => {
  const transport = createFixtureTransport(fixture), client = createClient(transport);
  const event = vi.fn(), state = vi.fn();
  const live = subscribeLive(client, { snapshot: vi.fn(), event, state }); releases.push(live.dispose);
  await flush(); setNativePresentationVisible(false);
  const sequence = String(BigInt(fixture.sequence) + 2n);
  transport.publishEngineEvent({ stream_id: fixture.stream_id, sequence, at: fixture.at,
    event: { player_died: { at: fixture.at, character_id: null, run_id: null } } });
  expect(state).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'resyncing' }));
  transport.publishSnapshot({ ...fixture, sequence });
  setNativePresentationVisible(true); await flush();
  expect(event).toHaveBeenCalledOnce();
  expect(event).toHaveBeenLastCalledWith(expect.objectContaining({ sequence }));
  expect(state).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'live' }));
});

it('cancels pending history refresh timers and coalesces hidden changes on resume', async () => {
  vi.useFakeTimers();
  const client = createClient(createFixtureTransport(fixture));
  const getRun = vi.spyOn(client, 'getRun').mockResolvedValue({ status: 'ready', value: null });
  const controller = createRunHistory(client, vi.fn()); releases.push(controller.dispose);
  const input: HistoryInput = { query: { kind: 'run', id: 'synthetic-run' }, scopeKey: 'live', streamId: fixture.stream_id, required: '12', store: saved('50') };
  controller.update(input); await flush(); expect(getRun).toHaveBeenCalledOnce();
  controller.update({ ...input, required: '13' });
  setNativePresentationVisible(false);
  controller.update({ ...input, required: '50' });
  await vi.advanceTimersByTimeAsync(60_000); expect(getRun).toHaveBeenCalledOnce();
  setNativePresentationVisible(true); await flush();
  expect(getRun).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1000); await flush();
  expect(getRun).toHaveBeenCalledTimes(2);
});

it('suspends serial Home reads without a timer and disposes hidden work', async () => {
  setNativePresentationVisible(false);
  const client = createClient(createFixtureTransport(fixture));
  const query = vi.spyOn(client, 'queryDungeonBests').mockResolvedValue({ status: 'ready', value: [] });
  const reads = createHomeReads(client); releases.push(reads.dispose);
  const result = reads.client.queryDungeonBests(); await flush(); expect(query).not.toHaveBeenCalled();
  setNativePresentationVisible(true); expect(await result).toEqual({ status: 'ready', value: [] }); expect(query).toHaveBeenCalledOnce();
  setNativePresentationVisible(false); const cancelled = reads.client.queryDungeonBests(); await flush(); reads.dispose();
  expect(await cancelled).toEqual({ status: 'superseded' }); expect(query).toHaveBeenCalledOnce();
});

it.each(['route', 'native', 'document'] as const)('retains realm visits, loot, kills and away periods across %s visibility changes', async mode => {
  const source = realmSnapshot();
  const transport = createFixtureTransport(source); const client = createClient(transport);
  const snapshot = vi.fn(), homeWindow = vi.fn(), event = vi.fn();
  const connection = connectShell(client, { snapshot, homeWindow, event, capture: vi.fn(), visuals: vi.fn(), store: vi.fn(), error: vi.fn() });
  releases.push(connection); await flush();
  let sequence = BigInt(source.sequence);
  const send = (event: EngineEvent, at: string) => transport.publishEngineEvent({ stream_id: source.stream_id, sequence: String(++sequence), at, event });
  const enter = (at: string, kind: 'realm' | 'dungeon', key = 'realm-a') => send({ map_entered: { at, kind, name: kind, realm_instance_key: key } }, at);
  const show = (visible: boolean) => mode === 'route' ? connection.setHomeActive(visible)
    : mode === 'native' ? setNativePresentationVisible(visible) : hidden(!visible);
  const current = () => homeWindow.mock.lastCall![0] as HomeLiveWindow;
  const bag = (id: string, at: string) => ({ ...homeSnapshot().drops[0], id, at, run_id: null, session_id: source.session!.id });
  enter('6000000', 'realm');
  send({ bag_dropped: bag('before', '6100000') }, '6100000');
  const visit = current().visitKey;
  expect(current().partial).toBe(false);
  show(false); await flush(); homeWindow.mockClear();
  send({ fight_ended: fight('hidden-kill', { session_id: source.session!.id, run_id: null,
    started: '6200000', ended: '6300000', kill: { at: '6300000', evidence: 'hp_zero_removed', provisional: false } }) }, '6300000');
  send({ instance_event: { id: 'notice', session_id: source.session!.id, run_id: null, at: '6300000', provisional: true,
    kind: { type: 'realm_event_killed', boss_key: 'Synthetic_Boss', object_type: 300, killer: 'Synthetic_Killer' } } }, '6300000');
  enter('7000000', 'dungeon');
  enter('8000000', 'realm', 'realm-b');
  enter('9000000', 'realm'); // Recover the cached realm-a visit while still hidden.
  send({ bag_dropped: bag('hidden', '9100000') }, '9100000');
  transport.publishSnapshot({ ...source, sequence: String(sequence), at: '9200000' });
  expect(homeWindow).not.toHaveBeenCalled();
  show(true); await flush();
  expect(createHomeLiveWindow).toHaveBeenCalledOnce();
  expect(current()).toMatchObject({ visitKey: visit, partial: false,
    drops: [{ id: 'before' }, { id: 'hidden' }], realmKills: [{ id: 'hidden-kill' }], realmAnnouncements: [{ id: 'notice' }],
    realmPeriods: [{ started: '6000000', ended: '7000000' }, { started: '9000000', ended: null }] });
  show(false); show(true); await flush();
  expect(current().visitKey).toBe(visit); expect(current().partial).toBe(false);
  expect(createHomeLiveWindow).toHaveBeenCalledOnce();
});

it.each([
  ['realm', 'callback'], ['realm', 'event'], ['realm', 'both'],
  ['dungeon', 'callback'], ['dungeon', 'event'], ['dungeon', 'both'],
] as const)('retains hidden %s visit history after brief capture recovery via %s without snapshots', async (area, delivery) => {
  vi.useFakeTimers();
  const source = area === 'realm' ? realmSnapshot() : homeSnapshot({ fights: [], drops: [] });
  const transport = createFixtureTransport(source), client = createClient(transport);
  const query = vi.spyOn(client, 'querySnapshot');
  const homeWindow = vi.fn(), snapshot = vi.fn();
  const connection = connectShell(client, { snapshot, homeWindow,
    capture: vi.fn(), visuals: vi.fn(), store: vi.fn(), error: vi.fn() });
  releases.push(connection); await flush();
  let sequence = BigInt(source.sequence);
  const send = (event: EngineEvent, at: string) => transport.publishEngineEvent({ stream_id: source.stream_id, sequence: String(++sequence), at, event });
  const capture = (state: 'Recovering' | 'Live') => {
    const status = { ...source.capture, state };
    if (delivery !== 'event') transport.publishCapture(status);
    if (delivery !== 'callback') send({ capture_state_changed: status }, '6200000');
  };
  const bag = (id: string, at: string) => ({ ...homeSnapshot().drops[0], id, at,
    session_id: source.session!.id, run_id: source.run?.id ?? null });
  send({ map_entered: { at: '6000000', kind: area, name: 'Synthetic Map', realm_instance_key: 'realm-a' } }, '6000000');
  if (source.run) send({ run_started: source.run }, '6000000');
  send({ bag_dropped: bag('before-recovery', '6100000') }, '6100000');
  const visitKey = (homeWindow.mock.lastCall![0] as HomeLiveWindow).visitKey;
  expect(visitKey).not.toBeNull();
  setNativePresentationVisible(false); homeWindow.mockClear(); snapshot.mockClear();
  capture('Recovering');
  await vi.advanceTimersByTimeAsync(500);
  capture('Live');
  send({ bag_dropped: bag('immediately-resumed', '6300000') }, '6300000');
  await vi.advanceTimersByTimeAsync(2000);
  const boss = fight('after-recovery', { session_id: source.session!.id, run_id: source.run?.id ?? null,
    started: '6400000', ended: null, kill: null });
  send({ fight_started: boss }, boss.started);
  send({ fight_ended: { ...boss, ended: '6500000' } }, '6500000');
  send({ bag_dropped: bag('after-grace-period', '6600000') }, '6600000');
  expect(query).toHaveBeenCalledOnce();
  expect(snapshot).not.toHaveBeenCalled();
  expect(homeWindow).not.toHaveBeenCalled();
  // Native emits no hidden snapshots. The restore query has no finished fights
  // or bags, so only the retained ordered model can supply these observations.
  query.mockResolvedValue({ ...source, sequence: String(sequence), at: '6700000' });
  setNativePresentationVisible(true); await flush();
  expect(query).toHaveBeenCalledTimes(2);
  expect(homeWindow).toHaveBeenLastCalledWith(expect.objectContaining({ visitKey,
    runId: source.run?.id ?? null, partial: true,
    reasons: ['Stale observations during capture recovery.'],
    fights: [expect.objectContaining({ id: boss.id, ended: '6500000' })],
    drops: [expect.objectContaining({ id: 'before-recovery' }),
      expect.objectContaining({ id: 'immediately-resumed' }), expect.objectContaining({ id: 'after-grace-period' })],
  }));
});

it('keeps finished dungeon boss tabs when Home starts inactive and across hide/restore', async () => {
  const source = homeSnapshot();
  const transport = createFixtureTransport(source), client = createClient(transport);
  const homeWindow = vi.fn();
  const connection = connectShell(client, { snapshot: vi.fn(), homeWindow, capture: vi.fn(), visuals: vi.fn(), store: vi.fn(), error: vi.fn() }, { homeActive: false });
  releases.push(connection); await flush();
  let sequence = BigInt(source.sequence);
  const send = (event: EngineEvent, at: string) => transport.publishEngineEvent({ stream_id: source.stream_id, sequence: String(++sequence), at, event });
  send({ map_entered: { at: '6000000', name: 'Synthetic Dungeon', kind: 'dungeon' } }, '6000000');
  send({ run_started: source.run! }, '6000000');
  const boss = fight('finished-boss', { session_id: source.session!.id, run_id: source.run!.id, started: '6100000', ended: '6200000' });
  send({ fight_ended: boss }, '6200000');
  expect(homeWindow).not.toHaveBeenCalled();
  connection.setHomeActive(true);
  expect(homeWindow).toHaveBeenLastCalledWith(expect.objectContaining({ partial: false, fights: [expect.objectContaining({ id: boss.id })] }));
  setNativePresentationVisible(false);
  transport.publishSnapshot({ ...source, sequence: String(sequence), at: '6300000', fights: [] });
  setNativePresentationVisible(true); await flush();
  expect(homeWindow).toHaveBeenLastCalledWith(expect.objectContaining({ partial: false, fights: [expect.objectContaining({ id: boss.id })] }));
  expect(createHomeLiveWindow).toHaveBeenCalledOnce();
});

it('keeps dungeon fight starts, ends and boss kills received while hidden in the timeline and tabs', async () => {
  const source = homeSnapshot({ fights: [], drops: [] });
  const transport = createFixtureTransport(source), client = createClient(transport);
  const homeWindow = vi.fn(), snapshot = vi.fn(), state = vi.fn();
  const connection = connectShell(client, { snapshot, homeWindow, state,
    capture: vi.fn(), visuals: vi.fn(), store: vi.fn(), error: vi.fn() });
  releases.push(connection); await flush();
  let sequence = BigInt(source.sequence);
  const send = (event: EngineEvent, at: string) => transport.publishEngineEvent({ stream_id: source.stream_id, sequence: String(++sequence), at, event });
  send({ map_entered: { at: '6000000', name: 'Synthetic Dungeon', kind: 'dungeon' } }, '6000000');
  send({ run_started: source.run! }, '6000000');
  const visitKey = (homeWindow.mock.lastCall![0] as HomeLiveWindow).visitKey;
  setNativePresentationVisible(false); homeWindow.mockClear();
  const first = fight('hidden-first', { session_id: source.session!.id, run_id: source.run!.id,
    target_type: 300, started: '6100000', ended: null, kill: null });
  send({ fight_started: first }, first.started);
  send({ fight_ended: { ...first, ended: '6200000' } }, '6200000');
  const boss = fight('hidden-boss', { session_id: source.session!.id, run_id: source.run!.id,
    target_type: 301, started: '6300000', ended: null, kill: null });
  send({ fight_started: boss }, boss.started);
  const killed = { ...boss, target_hp: 0, kill: { at: '6400000', evidence: 'hp_zero_removed' as const, provisional: false } };
  send({ fight_updated: killed }, '6400000');
  send({ fight_ended: { ...killed, ended: '6500000' } }, '6500000');
  // Finished fights are absent from this snapshot; only ordered events retain them.
  transport.publishSnapshot({ ...source, sequence: String(sequence), at: '6600000' });
  expect(homeWindow).not.toHaveBeenCalled();
  setNativePresentationVisible(true); await flush();
  const window = homeWindow.mock.lastCall![0] as HomeLiveWindow;
  expect(state).toHaveBeenLastCalledWith(expect.objectContaining({ phase: 'live' }));
  expect(window).toMatchObject({ visitKey, runId: source.run!.id, partial: false,
    fights: [{ id: first.id, started: '6100000', ended: '6200000' },
      { id: boss.id, started: '6300000', ended: '6500000', kill: killed.kill }] });
  const restored = snapshot.mock.lastCall![0] as typeof source;
  const observations = dungeonObservations(restored, window);
  expect(observations.partial).toBe(false);
  expect(dungeonTimeline(observations.fights, observations.drops, observations.events)).toMatchObject([
    { id: `fight:${first.id}`, at: '6100000', until: '6200000', detail: { outcome: 'ended' } },
    { id: `fight:${boss.id}`, at: '6300000', until: '6400000', detail: { outcome: 'killed' } },
  ]);
  const page = render(DungeonHome, { snapshot: restored, stale: false, liveWindow: window,
    visuals: { phase: 'ready', client_fingerprint: '', objects: [catalog(300, 'Synthetic First'), catalog(301, 'Synthetic Boss')] } });
  expect(screen.getByRole('tab', { name: 'Synthetic First' }).classList.contains('finished')).toBe(true);
  expect(screen.getByRole('tab', { name: 'Synthetic Boss' }).getAttribute('aria-selected')).toBe('true');
  const markers = [...page.container.querySelectorAll<HTMLElement>('[data-marker-members]')]
    .flatMap(row => JSON.parse(row.dataset.markerMembers!) as string[]);
  expect(markers).toEqual(expect.arrayContaining([`fight:${first.id}`, `fight:${boss.id}`]));
  expect(createHomeLiveWindow).toHaveBeenCalledOnce();
});
