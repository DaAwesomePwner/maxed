import { EVENT_BATCH_LIMIT } from '../src/lib/bindings/limits';
import { expect, test, vi } from 'vitest';
import type { LiveSnapshot } from '../src/lib/bindings/LiveSnapshot';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import { subscribeLive } from '../src/lib/bridge/live';
import fixture from './fixtures/t0-live';
import { realmView } from '../src/lib/home/realm';
import { dungeonView } from '../src/lib/home/dungeon';
import { emptyHomeLiveWindow } from '../src/lib/home/v2/live';
import { railView } from '../src/lib/shell/rail';

const settle = async () => { for (let i = 0; i < 30; i++) await Promise.resolve(); };
const full = (sequence: string): LiveSnapshot => ({ ...structuredClone(fixture), sequence });

test('a newer missed delta during an in-flight query cannot be forgotten at Nexus idle', async () => {
  vi.useFakeTimers();
  const initial = full('10'), transport = createFixtureTransport(initial);
  let resolve!: (value: LiveSnapshot) => void;
  const pending = new Promise<LiveSnapshot>(done => { resolve = done; });
  let queries = 0;
  const seen: LiveSnapshot[] = [], states: string[] = [];
  const client = { ...createClient(transport), querySnapshot: () => {
    queries++; return queries === 1 ? Promise.resolve(initial) : queries === 2 ? pending : Promise.resolve(full('14'));
  } };
  const connection = subscribeLive(client, { snapshot: value => seen.push(value), state: value => states.push(value.phase) });
  try {
    await settle();
    for (const sequence of ['12', '14']) transport.publishSnapshotUpdate({ base_sequence: String(Number(sequence) - 1),
      snapshot: { ...full(sequence), fights: [], drops: [] }, removed_fights: [], removed_drops: [] });
    resolve(full('12')); await settle();
    await vi.advanceTimersByTimeAsync(300); await settle();
    expect(queries).toBe(3); expect(seen.at(-1)).toEqual(full('14'));
    expect(states).not.toContain('resyncing'); // Discrete visit events were not lost.
  } finally { connection.dispose(); vi.useRealTimers(); }
});

test('fight and bag deltas retain unchanged rows, replace revisions and remove retired rows', async () => {
  const initial = full('10');
  const transport = createFixtureTransport(initial);
  const seen: LiveSnapshot[] = [];
  const connection = subscribeLive(createClient(transport), { snapshot: value => seen.push(value) });
  try {
    await settle();
    const baselineDrops = seen.at(-1)!.drops;
    const next = full('11'); next.fights = []; next.drops = [];
    transport.publishSnapshotUpdate({ base_sequence: '10', snapshot: next, removed_fights: [], removed_drops: [] });
    expect(seen.at(-1)?.fights).toEqual(initial.fights);
    expect(seen.at(-1)?.drops).toEqual(initial.drops);
    expect(seen.at(-1)!.drops).toBe(baselineDrops);
    expect(seen.at(-1)!.drops[0]).toBe(baselineDrops[0]);
    expect(seen.at(-1)?.sequence).toBe('11');
    const revision = full('12'); revision.fights[0].total_damage += 1;
    transport.publishSnapshotUpdate({ base_sequence: '11', snapshot: revision, removed_fights: [], removed_drops: [] });
    expect(seen.at(-1)?.fights).toEqual(revision.fights);
    const removed = full('13'); removed.fights = []; removed.drops = [];
    transport.publishSnapshotUpdate({ base_sequence: '12', snapshot: removed,
      removed_fights: initial.fights.map(row => row.id), removed_drops: initial.drops.map(row => row.id) });
    expect(seen.at(-1)?.fights).toEqual([]); expect(seen.at(-1)?.drops).toEqual([]);
  } finally { connection.dispose(); }
});

test('a missed delta or new stream requires an authoritative full query', async () => {
  const initial = full('10');
  const transport = createFixtureTransport(initial);
  let baseline = initial, queries = 0;
  const seen: LiveSnapshot[] = [];
  const client = { ...createClient(transport), querySnapshot: async () => { queries++; return baseline; } };
  const connection = subscribeLive(client, { snapshot: value => seen.push(value) });
  try {
    await settle();
    baseline = full('15'); baseline.fights = [];
    transport.publishSnapshotUpdate({ base_sequence: '14', snapshot: { ...baseline, drops: [] }, removed_fights: [], removed_drops: [] });
    await settle();
    expect(queries).toBe(2); expect(seen.at(-1)).toEqual(baseline);
    baseline = { ...full('1'), stream_id: 'new-generation' };
    transport.publishSnapshotUpdate({ base_sequence: '0', snapshot: baseline, removed_fights: [], removed_drops: [] });
    await settle();
    expect(queries).toBe(3); expect(seen.at(-1)).toEqual(baseline);
  } finally { connection.dispose(); }
});

test.each(['realm', 'dungeon'] as const)('reduced %s payload preserves focus, DPS, kills, visit totals and bags', async area_kind => {
  const initial = full('10'); initial.area_kind = area_kind; initial.at = '12000000';
  if (initial.run) initial.run.ended = null;
  if (area_kind === 'realm') initial.run = null;
  for (const row of initial.fights) { row.run_id = initial.run?.id ?? null; row.target_kind = 'boss'; row.ended = null; row.kill = null; row.local_last_hit = '11000000'; row.last_hit = '11000000'; }
  const boss = initial.fights[0];
  initial.fights.push({ ...structuredClone(boss), id: 'killed-boss', started: '1000000', ended: '4000000',
    last_hit: '4000000', local_last_hit: '4000000',
    kill: { at: '4000000', evidence: 'hp_zero_removed', provisional: false } });
  for (const drop of initial.drops) { drop.run_id = initial.run?.id ?? null; drop.at = '2000000'; }
  const ordinary = Array.from({ length: 2048 }, (_, i) => ({ ...structuredClone(boss), id: `ordinary-${i}`,
    target_kind: 'ordinary' as const, local_last_hit: null }));
  const legacy = { ...initial, fights: [...initial.fights, ...ordinary] };
  const window = { ...emptyHomeLiveWindow(), streamId: initial.stream_id, visitKey: 'visit', visitStarted: '0',
    runId: initial.run?.id ?? null, fights: initial.fights, drops: initial.drops,
    realmPeriods: [{ started: '0', ended: '5000000', fame: 12 }, { started: '9000000', ended: null, fame: 8 }] };
  const transport = createFixtureTransport(legacy), seen: LiveSnapshot[] = [];
  const connection = subscribeLive(createClient(transport), { snapshot: value => seen.push(value) });
  try {
    await settle();
    transport.publishSnapshotUpdate({ base_sequence: '10', snapshot: { ...initial, sequence: '11', fights: [], drops: [] },
      removed_fights: ordinary.map(row => row.id), removed_drops: [] });
    const reduced = seen.at(-1)!;
    expect(reduced.fights).toHaveLength(initial.fights.length);
    const rail = (value: LiveSnapshot) => { const view = railView(value, [], false, boss.id, window);
      return { fight: view.fight, dps: view.dps, rank: view.rank, target: view.target }; };
    expect(rail(reduced)).toEqual(rail(legacy));
    if (area_kind === 'realm') {
      const project = (value: LiveSnapshot) => { const { fight, self, players, kills, drops, visitFame, visitElapsed } = realmView(value, window, boss.id);
        return { fight, self, players, kills, drops, visitFame, visitElapsed }; };
      expect(project(reduced)).toEqual(project(legacy));
      expect(project(reduced).kills).toHaveLength(1);
      expect(project(reduced).visitFame).toBe(20);
      expect(project(reduced).drops).toHaveLength(initial.drops.length);
    } else {
      expect(dungeonView(reduced, boss.id, window)).toEqual(dungeonView(legacy, boss.id, window));
      expect(dungeonView({ ...reduced, fights: [] }).self).toEqual(dungeonView({ ...legacy, fights: ordinary }).self);
    }
  } finally { connection.dispose(); }
});

const eventBatch = (stream_id: string, first: bigint, count: number) => ({ stream_id,
  first_sequence: String(first), last_sequence: String(first + BigInt(count) - 1n),
  events: Array.from({ length: count }, (_, i) => ({ stream_id, sequence: String(first + BigInt(i)), at: '100',
    event: { fight_ended: full('0').fights[0] } })),
});

test('contiguous batches deliver once, project only uncovered events, and retain u64 sequence precision', async () => {
  const initial = full('9007199254740993'), transport = createFixtureTransport(initial);
  const event = vi.fn(), project = vi.fn(), batch = vi.fn(apply => apply()), state = vi.fn();
  const connection = subscribeLive(createClient(transport), { snapshot() {}, event, project, batch, state });
  try {
    await settle();
    state.mockClear();
    const update = eventBatch(initial.stream_id, BigInt(initial.sequence) + 1n, 1024);
    transport.publishEngineEvent(update);
    expect(event).toHaveBeenCalledTimes(1024); expect(project).toHaveBeenCalledTimes(1024);
    expect(batch).toHaveBeenCalledTimes(1);
    expect(state).toHaveBeenCalledTimes(1);
    transport.publishEngineEvent(update);
    expect(event).toHaveBeenCalledTimes(1024);
    expect(event.mock.calls.at(-1)![0].sequence).toBe(update.last_sequence);
  } finally { connection.dispose(); }
});

test('a gap before a large batch resyncs without overflow and delivers covered side effects once', async () => {
  const initial = full('10'), transport = createFixtureTransport(initial);
  let resolve!: (value: LiveSnapshot) => void;
  const pending = new Promise<LiveSnapshot>(done => { resolve = done; });
  const client = { ...createClient(transport), querySnapshot: vi.fn().mockResolvedValueOnce(initial).mockReturnValue(pending) };
  const event = vi.fn(), project = vi.fn(), states: string[] = [];
  const connection = subscribeLive(client, { snapshot() {}, event, project, state: state => states.push(state.phase) });
  try {
    await settle();
    const batch = eventBatch(initial.stream_id, 12n, 1024);
    transport.publishEngineEvent(batch);
    expect(event).not.toHaveBeenCalled(); expect(states.at(-1)).toBe('resyncing');
    resolve(full(batch.last_sequence)); await settle();
    expect(event).toHaveBeenCalledTimes(1024); expect(project).not.toHaveBeenCalled();
    expect(states.at(-1)).toBe('live');
    transport.publishEngineEvent(batch); expect(event).toHaveBeenCalledTimes(1024);
  } finally { connection.dispose(); }
});

test('batch source reset retires old callbacks and an in-flight prior-generation query', async () => {
  const initial = full('10'), transport = createFixtureTransport(initial);
  let resolve!: (value: LiveSnapshot) => void;
  const pending = new Promise<LiveSnapshot>(done => { resolve = done; });
  const next = { ...full('2'), stream_id: 'new' };
  const client = { ...createClient(transport), querySnapshot: vi.fn().mockResolvedValueOnce(initial).mockReturnValueOnce(pending).mockResolvedValue(next) };
  const event = vi.fn(), snapshots: LiveSnapshot[] = [];
  const connection = subscribeLive(client, { snapshot: value => snapshots.push(value), event });
  try {
    await settle();
    transport.publishEngineEvent(eventBatch(initial.stream_id, 12n, 2));
    transport.publishEngineEvent(eventBatch('new', 1n, 3));
    resolve(full('13')); await settle();
    expect(snapshots.at(-1)?.stream_id).toBe('new');
    expect(event.mock.calls.map(([value]) => [value.stream_id, value.sequence])).toEqual([['new', '3']]);
    transport.publishEngineEvent(eventBatch(initial.stream_id, 14n, 2));
    expect(event).toHaveBeenCalledTimes(1);
  } finally { connection.dispose(); }
});

test.each(['range', 'hole', 'stream', 'empty', 'oversized'] as const)('rejects a malformed batch %s before applying members', async kind => {
  const initial = full('10'), transport = createFixtureTransport(initial), event = vi.fn(), states: string[] = [];
  const connection = subscribeLive(createClient(transport), { snapshot() {}, event, state: value => states.push(value.phase) });
  try {
    await settle();
    const batch = eventBatch(initial.stream_id, 11n, 3);
    if (kind === 'range') batch.last_sequence = '12';
    if (kind === 'hole') batch.events[1].sequence = '14';
    if (kind === 'stream') batch.events[1].stream_id = 'other';
    if (kind === 'empty') batch.events = [];
    if (kind === 'oversized') Object.assign(batch, eventBatch(initial.stream_id, 11n, EVENT_BATCH_LIMIT + 1));
    transport.publishEngineEvent(batch);
    expect(event).not.toHaveBeenCalled(); expect(states.at(-1)).toBe('failed');
  } finally { connection.dispose(); }
});


test('partially overlapping batches and mixed singles deliver each side effect once', async () => {
  const initial = full('10'), transport = createFixtureTransport(initial);
  const event = vi.fn(), project = vi.fn();
  const connection = subscribeLive(createClient(transport), { snapshot() {}, event, project });
  try {
    await settle();
    transport.publishEngineEvent(eventBatch(initial.stream_id, 11n, 3).events[0]);
    transport.publishEngineEvent(eventBatch(initial.stream_id, 11n, 4));
    transport.publishEngineEvent(eventBatch(initial.stream_id, 13n, 4));
    transport.publishEngineEvent(eventBatch(initial.stream_id, 16n, 1).events[0]);
    transport.publishEngineEvent(eventBatch(initial.stream_id, 17n, 1).events[0]);
    expect(event.mock.calls.map(([value]) => value.sequence)).toEqual(['11', '12', '13', '14', '15', '16', '17']);
    expect(project.mock.calls).toEqual(event.mock.calls);
  } finally { connection.dispose(); }
});

test('back-to-back cap-sized batches across a native split deliver once without resync', async () => {
  const initial = full('10'), transport = createFixtureTransport(initial);
  const event = vi.fn(), states: string[] = [], batch = vi.fn(apply => apply());
  const client = createClient(transport), query = vi.spyOn(client, 'querySnapshot');
  const connection = subscribeLive(client, { snapshot() {}, event, batch, state: value => states.push(value.phase) });
  try {
    await settle();
    const first = eventBatch(initial.stream_id, 11n, EVENT_BATCH_LIMIT);
    const second = eventBatch(initial.stream_id, BigInt(first.last_sequence) + 1n, EVENT_BATCH_LIMIT);
    transport.publishEngineEvent(first); transport.publishEngineEvent(second);
    transport.publishEngineEvent(first); transport.publishEngineEvent(second);
    expect(event).toHaveBeenCalledTimes(2 * EVENT_BATCH_LIMIT);
    expect(event.mock.calls.map(([row]) => row.sequence)).toEqual([...first.events, ...second.events].map(row => row.sequence));
    expect(batch).toHaveBeenCalledTimes(4);
    expect(states).not.toContain('resyncing'); expect(query).toHaveBeenCalledTimes(1);
  } finally { connection.dispose(); }
});

test.each(['snapshot', 'delta'] as const)('a newer %s can overtake a batch without suppressing or repeating side effects', async kind => {
  const initial = full('10'), transport = createFixtureTransport(initial);
  const event = vi.fn(), project = vi.fn();
  const connection = subscribeLive(createClient(transport), { snapshot() {}, event, project });
  try {
    await settle();
    const next = full('14');
    transport.publishSnapshotUpdate(kind === 'snapshot' ? next : { base_sequence: '10', snapshot: next, removed_fights: [], removed_drops: [] });
    const batch = eventBatch(initial.stream_id, 11n, 5);
    transport.publishEngineEvent(batch); transport.publishEngineEvent(batch);
    expect(event.mock.calls.map(([value]) => value.sequence)).toEqual(['11', '12', '13', '14', '15']);
    expect(project.mock.calls.map(([value]) => value.sequence)).toEqual(['15']);
  } finally { connection.dispose(); }
});

test('same-sequence deltas require the exact revision base', async () => {
  vi.useFakeTimers();
  const initial = { ...full('10'), revision: '100' };
  const transport = createFixtureTransport(initial);
  const seen: LiveSnapshot[] = [];
  const querySnapshot = vi.fn().mockResolvedValueOnce(initial)
    .mockResolvedValue({ ...initial, revision: '103', map: 'recovered' });
  const connection = subscribeLive({ ...createClient(transport), querySnapshot },
    { snapshot: value => seen.push(value) });
  try {
    await settle();
    transport.publishSnapshotUpdate({ base_sequence: '10', base_revision: '100',
      snapshot: { ...initial, revision: '101', map: 'silent', fights: [], drops: [] },
      removed_fights: [], removed_drops: [] });
    expect(seen.at(-1)?.map).toBe('silent');
    expect(seen.at(-1)?.fights).toEqual(initial.fights);
    transport.publishSnapshotUpdate({ base_sequence: '10', base_revision: '100',
      snapshot: { ...initial, revision: '103', map: 'wrong base', fights: [], drops: [] },
      removed_fights: [], removed_drops: [] });
    expect(seen.at(-1)?.map).toBe('silent');
    await settle();
    expect(querySnapshot).toHaveBeenCalledTimes(2);
    expect(seen.at(-1)?.map).toBe('recovered');
  } finally { connection.dispose(); vi.useRealTimers(); }
});

test('a same-watermark missed delta cannot be satisfied by an older query revision', async () => {
  vi.useFakeTimers();
  const initial = { ...full('10'), revision: '100' };
  const transport = createFixtureTransport(initial);
  const seen: LiveSnapshot[] = [];
  let resolve!: (snapshot: LiveSnapshot) => void;
  const pending = new Promise<LiveSnapshot>(done => { resolve = done; });
  const querySnapshot = vi.fn().mockResolvedValueOnce(initial).mockReturnValueOnce(pending)
    .mockResolvedValue({ ...initial, revision: '103', map: 'latest' });
  const connection = subscribeLive({ ...createClient(transport), querySnapshot },
    { snapshot: value => seen.push(value) });
  try {
    await settle();
    transport.publishSnapshotUpdate({ base_sequence: '10', base_revision: '102',
      snapshot: { ...initial, revision: '103', fights: [], drops: [] },
      removed_fights: [], removed_drops: [] });
    resolve({ ...initial, revision: '101', map: 'obsolete query' });
    await settle();
    expect(seen.at(-1)?.revision).toBe('100');
    await vi.advanceTimersByTimeAsync(300);
    expect(seen.at(-1)?.map).toBe('latest');
    expect(querySnapshot).toHaveBeenCalledTimes(3);
  } finally { connection.dispose(); vi.useRealTimers(); }
});

test('queried realm score followed by full publication and deltas needs no recovery query', async () => {
  const { realmSnapshot } = await import('./home-v2/fixtures');
  const source = realmSnapshot();
  const score = (revision: string, current: number): LiveSnapshot => ({
    ...source, revision,
    instance: { ...source.instance!, realm_score: { current, max: 300000 } },
  });
  // Published 10 had score 100; the direct query materialized 11 with score 120.
  const queried = score('11', 120);
  const transport = createFixtureTransport(queried);
  const querySnapshot = vi.fn().mockResolvedValue(queried);
  const queryStore = vi.fn().mockResolvedValue({
    state: 'ready', stream_id: source.stream_id, durable: source.sequence, queued: 0, message: '',
  });
  const historyBaseline = vi.fn(), seen: LiveSnapshot[] = [];
  const connection = subscribeLive({ ...createClient(transport), querySnapshot, queryStore },
    { snapshot: value => seen.push(value), historyBaseline });
  try {
    await settle();
    const stores = queryStore.mock.calls.length;
    // Native encoder sends full across the unpublished revision 11.
    transport.publishSnapshotUpdate(score('12', 150));
    transport.publishSnapshotUpdate({ base_sequence: source.sequence, base_revision: '12',
      snapshot: { ...score('13', 180), fights: [], drops: [] }, removed_fights: [], removed_drops: [] });
    await settle();
    expect(seen.map(value => value.instance?.realm_score?.current)).toEqual([120, 150, 180]);
    expect(seen.every(value => value.sequence === source.sequence)).toBe(true);
    expect(querySnapshot).toHaveBeenCalledTimes(1);
    expect(historyBaseline).toHaveBeenCalledTimes(1);
    expect(queryStore).toHaveBeenCalledTimes(stores);
  } finally { connection.dispose(); }
});
