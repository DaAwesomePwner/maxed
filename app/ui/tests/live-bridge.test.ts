import { afterEach, expect, test, vi } from 'vitest';
import type { CaptureStatus } from '../src/lib/bindings/CaptureStatus';
import type { EventEnvelope } from '../src/lib/bindings/EventEnvelope';
import type { LiveSnapshot } from '../src/lib/bindings/LiveSnapshot';
import type { StoreStatus } from '../src/lib/bindings/StoreStatus';
import type { Client } from '../src/lib/bridge/client';
import { createClient } from '../src/lib/bridge/client';
import { createFixtureTransport } from '../src/lib/bridge/fixture';
import { classifySequence, subscribeLive } from '../src/lib/bridge/live';
import type { LiveState } from '../src/lib/bridge/live';
import fixture from './fixtures/t0-live';
import { notifyEventObserver } from '../src/lib/shell/event-observers';
import { setNativePresentationVisible } from '../src/lib/bridge/visibility';
import { setDebugMode } from '../src/lib/debug/debug';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const snapshot = (sequence = '10', stream_id = 'live-a'): LiveSnapshot => ({ ...structuredClone(fixture), sequence, stream_id });
const event = (sequence: string, stream_id = 'live-a'): EventEnvelope => ({
  stream_id, sequence, at: '100', event: { player_died: { at: '100', character_id: null, run_id: null } },
});
const store = (durable = '10', stream_id = 'live-a'): StoreStatus => ({ state: 'ready', queued: 0, durable, stream_id, message: 'Saved' });
const cleanups: (() => void)[] = [];
afterEach(() => { for (const cleanup of cleanups.splice(0)) cleanup(); vi.useRealTimers(); });
async function settle() { for (let i = 0; i < 30; i++) await Promise.resolve(); }

function setup(overrides: Partial<Client> = {}) {
  const transport = createFixtureTransport(snapshot());
  const base = createClient(transport);
  const client: Client = { ...base, queryStore: vi.fn(async () => store()), ...overrides };
  const seen: LiveSnapshot[] = [], events: EventEnvelope[] = [], projects: EventEnvelope[] = [];
  const states: LiveState[] = [], histories: StoreStatus[] = [], captures: CaptureStatus[] = [];
  const control = subscribeLive(client, {
    snapshot: (value) => seen.push(value), event: (value) => events.push(value),
    project: (value) => projects.push(value), state: (value) => states.push(value),
    historyReady: (value) => histories.push(value), capture: (value) => captures.push(value),
  });
  cleanups.push(control.dispose);
  return { transport, client, control, seen, events, projects, states, histories, captures };
}

test('integer strings retain precision beyond the JS safe integer limit', () => {
  expect(classifySequence('9007199254740992', '9007199254740993')).toBe('next');
  expect(classifySequence('9007199254740992', '9007199254740994')).toBe('gap');
  expect(classifySequence('9', '9')).toBe('stale');
  expect(classifySequence('9', '8')).toBe('stale');
});

test('both domain listeners register before the first query', async () => {
  const calls: string[] = [];
  setup({
    onSnapshot: async () => { calls.push('snapshot'); return () => {}; },
    onEngineEvent: async () => { calls.push('event'); return () => {}; },
    querySnapshot: async () => { calls.push('query'); return snapshot(); },
  });
  await settle();
  expect(calls).toEqual(['snapshot', 'event', 'query']);
});

test('initial query buffers events, drops history and drains contiguous newer events', async () => {
  const query = deferred<LiveSnapshot>();
  const h = setup({ querySnapshot: () => query.promise });
  await settle();
  h.transport.publishEngineEvent(event('10'));
  h.transport.publishEngineEvent(event('12'));
  h.transport.publishEngineEvent(event('11'));
  query.resolve(snapshot());
  await settle();
  expect(h.events.map((value) => value.sequence)).toEqual(['11', '12']);
  expect(h.projects.map((value) => value.sequence)).toEqual(['11', '12']);
});

test('a newer snapshot never suppresses a received discrete event', async () => {
  const h = setup(); await settle();
  h.transport.publishSnapshot(snapshot('20'));
  h.transport.publishEngineEvent(event('11'));
  h.transport.publishEngineEvent(event('11'));
  expect(h.events.map((value) => value.sequence)).toEqual(['11']);
  expect(h.projects).toHaveLength(0);
  expect(h.seen.at(-1)?.sequence).toBe('20');
});

test('snapshot overtaking buffered events during first query preserves new side effects', async () => {
  const query = deferred<LiveSnapshot>();
  const h = setup({ querySnapshot: () => query.promise }); await settle();
  h.transport.publishEngineEvent(event('11'));
  h.transport.publishSnapshot(snapshot('20'));
  query.resolve(snapshot()); await settle();
  expect(h.seen.at(-1)?.sequence).toBe('20');
  expect(h.events.map((value) => value.sequence)).toEqual(['11']);
  expect(h.projects).toHaveLength(0);
});

test('duplicates and stale events do not query or repeat effects', async () => {
  const query = vi.fn(async () => snapshot());
  const h = setup({ querySnapshot: query }); await settle();
  for (const seq of ['11', '11', '9', '10', '12', '11']) h.transport.publishEngineEvent(event(seq));
  expect(h.events.map((value) => value.sequence)).toEqual(['11', '12']);
  expect(query).toHaveBeenCalledTimes(1);
});

test('live snapshot jumps are normal coalescing; stale snapshots do not regress projection', async () => {
  const query = vi.fn(async () => snapshot());
  const h = setup({ querySnapshot: query }); await settle();
  h.transport.publishSnapshot(snapshot('30'));
  h.transport.publishSnapshot(snapshot('20'));
  expect(h.seen.map((value) => value.sequence)).toEqual(['10', '30']);
  expect(query).toHaveBeenCalledTimes(1);
});

test('large sequence delivery has no Number rounding', async () => {
  const h = setup({ querySnapshot: async () => snapshot('9007199254740992') }); await settle();
  h.transport.publishEngineEvent(event('9007199254740993'));
  expect(h.events[0].sequence).toBe('9007199254740993');
});

test('repeated gaps share one query and wait for same-stream durability', async () => {
  vi.useFakeTimers();
  const pending = deferred<LiveSnapshot>();
  const query = vi.fn<() => Promise<LiveSnapshot>>().mockResolvedValueOnce(snapshot()).mockImplementation(() => pending.promise);
  let durable = store();
  const queryStore = vi.fn(async () => durable);
  const h = setup({ querySnapshot: query, queryStore }); await settle();
  h.transport.publishEngineEvent(event('13'));
  h.transport.publishEngineEvent(event('15'));
  h.transport.publishEngineEvent(event('14'));
  expect(query).toHaveBeenCalledTimes(2);
  expect(h.states.at(-1)?.historyStale).toBe(true);
  pending.resolve(snapshot('15')); await settle();
  expect(h.events.map((value) => value.sequence)).toEqual(['13', '14', '15']);
  expect(h.histories).toHaveLength(1); // only the initial baseline is durable
  durable = store('99', 'retired');
  await vi.advanceTimersByTimeAsync(1000);
  expect(h.histories).toHaveLength(1);
  durable = store('15');
  await vi.advanceTimersByTimeAsync(1000);
  expect(h.histories.at(-1)?.durable).toBe('15');
  expect(h.states.at(-1)?.historyStale).toBe(false);
  expect(query).toHaveBeenCalledTimes(2);
});

test('query cannot manufacture an event that was never observed', async () => {
  const query = vi.fn<() => Promise<LiveSnapshot>>().mockResolvedValueOnce(snapshot()).mockResolvedValue(snapshot('12'));
  const h = setup({ querySnapshot: query }); await settle();
  h.transport.publishEngineEvent(event('12')); await settle();
  expect(h.events.map((value) => value.sequence)).toEqual(['12']);
});

test('unknown stream suspends projection; retired streams cannot reactivate', async () => {
  const pending = deferred<LiveSnapshot>();
  const query = vi.fn<() => Promise<LiveSnapshot>>().mockResolvedValueOnce(snapshot()).mockImplementation(() => pending.promise);
  const h = setup({ querySnapshot: query }); await settle();
  h.transport.publishEngineEvent(event('1', 'live-b'));
  expect(h.events).toHaveLength(0);
  pending.resolve(snapshot('1', 'live-b')); await settle();
  h.transport.publishSnapshot(snapshot('100', 'live-a'));
  h.transport.publishEngineEvent(event('101', 'live-a'));
  h.transport.publishEngineEvent(event('2', 'live-b'));
  expect(h.seen.at(-1)?.stream_id).toBe('live-b');
  expect(h.events.map((value) => value.stream_id)).toEqual(['live-b']);
  expect(query).toHaveBeenCalledTimes(2);
});

test('an unknown stream invalidates an old resync query completion', async () => {
  const old = deferred<LiveSnapshot>();
  const query = vi.fn<() => Promise<LiveSnapshot>>().mockResolvedValueOnce(snapshot()).mockImplementationOnce(() => old.promise).mockResolvedValue(snapshot('2', 'live-b'));
  const h = setup({ querySnapshot: query }); await settle();
  h.transport.publishEngineEvent(event('12'));
  h.transport.publishEngineEvent(event('2', 'live-b'));
  old.resolve(snapshot('100')); await settle();
  expect(h.seen.map((value) => value.sequence)).toEqual(['10', '2']);
  expect(h.seen.at(-1)?.stream_id).toBe('live-b');
});

test('mode command invalidates in-flight query before awaiting native work', async () => {
  const old = deferred<LiveSnapshot>(), mode = deferred<void>();
  const query = vi.fn<() => Promise<LiveSnapshot>>().mockImplementationOnce(() => old.promise).mockResolvedValue(snapshot('2', 'demo-b'));
  const h = setup({ querySnapshot: query, setDemo: () => mode.promise }); await settle();
  const changed = h.control.setDemo(true);
  old.resolve(snapshot('100')); await settle();
  expect(h.seen).toHaveLength(0);
  mode.resolve(); await changed; await settle();
  expect(h.seen.at(-1)?.stream_id).toBe('demo-b');
});

test('listener rejection is fixed and does not leak private native errors', async () => {
  const query = vi.fn(async () => snapshot());
  const h = setup({ onSnapshot: async () => { throw new Error('secret/path/token'); }, querySnapshot: query });
  await settle();
  expect(h.states.at(-1)?.phase).toBe('failed');
  expect(h.states.at(-1)?.message).not.toContain('secret');
  expect(query).not.toHaveBeenCalled();
});

test('second listener failure releases the first successful listener', async () => {
  const release = vi.fn();
  setup({ onSnapshot: async () => release, onEngineEvent: async () => { throw new Error('private'); } });
  await settle();
  expect(release).toHaveBeenCalledTimes(1);
});

test('setDemo retries failed listener registration and synchronizes the new mode', async () => {
  const release = vi.fn();
  const onSnapshot = vi.fn<Client['onSnapshot']>()
    .mockRejectedValueOnce(new Error('private listener failure'))
    .mockResolvedValue(release);
  const setDemo = vi.fn(async () => {});
  const querySnapshot = vi.fn(async () => snapshot('1', 'demo-new'));
  const h = setup({ onSnapshot, setDemo, querySnapshot, queryStore: async () => store('1', 'demo-new') });
  await settle();
  expect(h.states.at(-1)?.phase).toBe('failed');
  expect(querySnapshot).not.toHaveBeenCalled();

  await h.control.setDemo(true);
  await settle();
  expect(setDemo).toHaveBeenCalledWith(true);
  expect(onSnapshot).toHaveBeenCalledTimes(2);
  expect(querySnapshot).toHaveBeenCalledTimes(1);
  expect(h.seen.at(-1)?.stream_id).toBe('demo-new');
  expect(h.states.at(-1)?.phase).toBe('live');
  h.control.dispose();
  expect(release).toHaveBeenCalledTimes(1);
});

test('disposal releases a listener whose registration finishes late', async () => {
  const registration = deferred<() => void>(), release = vi.fn();
  const query = vi.fn(async () => snapshot());
  const h = setup({ onSnapshot: () => registration.promise, querySnapshot: query });
  h.control.dispose(); h.control.dispose(); registration.resolve(release); await settle();
  expect(release).toHaveBeenCalledTimes(1);
  expect(query).not.toHaveBeenCalled();
});

test.each(['resolve', 'reject'] as const)('disposal during query suppresses late %s', async (completion) => {
  const pending = deferred<LiveSnapshot>();
  const h = setup({ querySnapshot: () => pending.promise }); await settle();
  h.control.dispose();
  const count = h.states.length;
  if (completion === 'resolve') pending.resolve(snapshot()); else pending.reject(new Error('private'));
  await settle();
  expect(h.seen).toHaveLength(0);
  expect(h.states).toHaveLength(count);
});

test('query failures back off, stop at four attempts and reconnect is actionable', async () => {
  vi.useFakeTimers();
  const query = vi.fn<() => Promise<LiveSnapshot>>().mockRejectedValue(new Error('private path'));
  const h = setup({ querySnapshot: query }); await settle();
  expect(query).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(249); expect(query).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1); expect(query).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1500); expect(query).toHaveBeenCalledTimes(4);
  await vi.advanceTimersByTimeAsync(10_000); expect(query).toHaveBeenCalledTimes(4);
  expect(h.states.at(-1)?.message).not.toContain('private');
  query.mockResolvedValue(snapshot()); h.control.reconnect(); await settle();
  expect(h.states.at(-1)?.phase).toBe('live');
});

test('buffer overflow fails visibly and stops automatic query storms', async () => {
  const pending = deferred<LiveSnapshot>(), query = vi.fn(() => pending.promise);
  const h = setup({ querySnapshot: query }); await settle();
  for (let i = 1; i <= 513; i++) h.transport.publishEngineEvent(event(String(i)));
  expect(h.states.at(-1)?.message).toContain('Disconnected. Reconnect.');
  pending.resolve(snapshot('600')); await settle();
  expect(h.seen).toHaveLength(0);
  expect(query).toHaveBeenCalledTimes(1);
});

test('capture status updates immediately with its independent query revision guard', async () => {
  const initial = deferred<LiveSnapshot>(), capture = deferred<CaptureStatus>();
  const h = setup({ querySnapshot: () => initial.promise, queryCapture: () => capture.promise }); await settle();
  const live: CaptureStatus = { ...fixture.capture, state: 'Live', message: 'Decoded' };
  h.transport.publishCapture(live);
  expect(h.captures).toEqual([live]);
  expect(h.seen).toHaveLength(0);
  capture.resolve(fixture.capture); await settle();
  expect(h.captures).toEqual([live]);
});

test('repeated mount, reconnect and dispose leave no active listeners', async () => {
  let active = 0;
  const listen = async () => { active++; return () => { active--; }; };
  for (let i = 0; i < 5; i++) {
    const h = setup({ onSnapshot: listen, onEngineEvent: listen, onCapture: listen }); await settle();
    expect(active).toBe(3);
    h.control.reconnect(); await settle(); expect(active).toBe(3);
    h.control.dispose(); h.control.dispose(); expect(active).toBe(0);
  }
});

test('storage failures gate history without failing live delivery; lifecycle errors remain safe', async () => {
  const h = setup({ queryStore: async () => ({ ...store(), state: 'failed', durable: null, message: 'private SQLite path' }) });
  await settle();
  expect(h.states.at(-1)).toMatchObject({ phase: 'live', historyFailed: true, historyStale: true });
  expect(h.histories).toHaveLength(0);
  h.transport.publishSnapshot(snapshot('11'));
  h.transport.publishEngineEvent(event('11'));
  expect(h.seen.at(-1)?.sequence).toBe('11');
  expect(h.events.at(-1)?.sequence).toBe('11');
  expect(h.states.at(-1)?.message).not.toContain('private');
  await h.control.transition(async () => { throw new Error('another private path'); });
  expect(h.states.at(-1)?.message).toBe('Capture failed. Reconnect.');
});

test('an initial query predating a newly observed stream cannot establish the old baseline', async () => {
  const old = deferred<LiveSnapshot>();
  const query = vi.fn<() => Promise<LiveSnapshot>>().mockImplementationOnce(() => old.promise).mockResolvedValue(snapshot('1', 'live-b'));
  const h = setup({ querySnapshot: query }); await settle();
  h.transport.publishEngineEvent(event('1', 'live-b'));
  old.resolve(snapshot()); await settle();
  expect(h.seen.map((value) => value.stream_id)).toEqual(['live-b']);
  expect(h.events).toHaveLength(0);
  expect(query).toHaveBeenCalledTimes(2);
});

test('a stopped preview epoch with no store is not polled forever', async () => {
  const queryStore = vi.fn(async (): Promise<StoreStatus> => ({ state: 'closed', queued: 0, stream_id: null, durable: null, message: 'Closed' }));
  const h = setup({ querySnapshot: async () => snapshot('0', 'stopped-unique'), queryStore }); await settle();
  expect(h.states.at(-1)?.historyStale).toBe(false);
  expect(h.histories).toHaveLength(1);
  expect(queryStore).toHaveBeenCalledTimes(1);
});

test('native emission failure stays degraded after a delayed successful snapshot query', async () => {
  const pending = deferred<LiveSnapshot>();
  const h = setup({ querySnapshot: () => pending.promise }); await settle();
  h.transport.publishCapture({ ...fixture.capture, state: 'Failed', message: 'Live updates interrupted; reconnect or restart Maxed' });
  pending.resolve(snapshot()); await settle();
  expect(h.states.at(-1)?.phase).toBe('failed');
  expect(h.states.at(-1)?.message).toBe('Restart Maxed.');
  expect(h.seen).toHaveLength(0);
});

test('disposal while the second listener is registering releases both exactly once', async () => {
  const second = deferred<() => void>(), firstRelease = vi.fn(), secondRelease = vi.fn();
  const h = setup({ onSnapshot: async () => firstRelease, onEngineEvent: () => second.promise }); await settle();
  h.control.dispose(); h.control.dispose();
  expect(firstRelease).toHaveBeenCalledTimes(1);
  second.resolve(secondRelease); await settle();
  expect(secondRelease).toHaveBeenCalledTimes(1);
});

test('coalesces steady events and snapshots into one store check per second without reporting normal flush lag', async () => {
  vi.useFakeTimers();
  let latest = 10;
  let caughtUp = false;
  const queryStore = vi.fn(async () => ({ ...store(String(latest === 10 || caughtUp ? latest : latest - 1)), queued: caughtUp ? 0 : 1 }));
  const h = setup({ queryStore }); await settle();
  for (let i = 0; i < 48; i++) {
    latest++;
    h.transport.publishEngineEvent(event(String(latest)));
    h.transport.publishSnapshot(snapshot(String(latest)));
    await vi.advanceTimersByTimeAsync(250);
  }
  expect(queryStore).toHaveBeenCalledTimes(13); // initial join plus 12 timed checks
  expect(h.states.every(state => !state.historyLagging && !state.historyFailed)).toBe(true);
  expect(h.states.at(-1)).toMatchObject({ phase: 'live', historyStale: true });
  expect(h.histories).toHaveLength(1);
  caughtUp = true;
  await vi.advanceTimersByTimeAsync(1000);
  expect(h.histories.at(-1)?.durable).toBe(String(latest));
  expect(h.states.at(-1)).toMatchObject({ historyStale: false, historyLagging: false });
  h.control.dispose();
  const count = queryStore.mock.calls.length;
  await vi.advanceTimersByTimeAsync(20_000);
  expect(queryStore).toHaveBeenCalledTimes(count);
});

test('storage query errors never halt events and recover history independently', async () => {
  vi.useFakeTimers();
  const queryStore = vi.fn<() => Promise<StoreStatus>>().mockRejectedValue(new Error('private database path'));
  const h = setup({ queryStore }); await settle();
  await vi.advanceTimersByTimeAsync(6000);
  h.transport.publishEngineEvent(event('11'));
  h.transport.publishSnapshot(snapshot('11'));
  expect(h.events.at(-1)?.sequence).toBe('11');
  expect(h.seen.at(-1)?.sequence).toBe('11');
  expect(h.states.at(-1)).toMatchObject({ phase: 'live', historyFailed: true, historyStale: true });
  expect(h.states.at(-1)?.message).toBe('');
  expect(h.histories).toHaveLength(0);
  queryStore.mockResolvedValue(store('11'));
  await vi.advanceTimersByTimeAsync(1000);
  expect(h.states.at(-1)).toMatchObject({ phase: 'live', historyFailed: false, historyStale: false, historyLagging: false });
  expect(h.histories.at(-1)?.durable).toBe('11');
});

test.each([2049, 8192])('recovers %i buffered envelopes behind a gap without disconnecting', async count => {
  const pending = deferred<LiveSnapshot>();
  const query = vi.fn<() => Promise<LiveSnapshot>>().mockResolvedValueOnce(snapshot()).mockImplementation(() => pending.promise);
  const h = setup({ querySnapshot: query }); await settle();
  const events = Array.from({ length: count }, (_, i) => event(String(i + 12)));
  for (let i = 0; i < events.length; i += 1024) {
    const chunk = events.slice(i, i + 1024);
    h.transport.publishEngineEvent({ stream_id: 'live-a', first_sequence: chunk[0].sequence,
      last_sequence: chunk.at(-1)!.sequence, events: chunk });
  }
  expect(h.states.some(state => state.phase === 'failed')).toBe(false);
  pending.resolve(snapshot(events.at(-1)!.sequence)); await settle();
  expect(h.events).toEqual(events);
  expect(h.states.at(-1)?.phase).toBe('live');
});

test('throwing side-effect observers do not cut off a batch or later observers and history', async () => {
  setDebugMode(true);
  const log = vi.spyOn(console, 'debug').mockImplementation(() => {});
  try {
    const transport = createFixtureTransport(snapshot());
    const accepted: string[] = [], received: string[] = [], projects: string[] = [];
    const states: LiveState[] = [];
    const control = subscribeLive(createClient(transport), {
      snapshot: () => {}, state: state => states.push(state),
      event: next => {
        notifyEventObserver(next, () => { if (next.sequence === '12') throw new Error('synthetic observer failure'); });
        notifyEventObserver(next, event => received.push(event.sequence));
        accepted.push(next.sequence);
      },
      project: next => projects.push(next.sequence),
    });
    cleanups.push(control.dispose); await settle();
    transport.publishEngineEvent({ stream_id: 'live-a', first_sequence: '11', last_sequence: '13',
      events: ['11', '12', '13'].map(sequence => event(sequence)) });
    expect(accepted).toEqual(['11', '12', '13']);
    expect(received).toEqual(accepted); expect(projects).toEqual(accepted);
    expect(states.at(-1)?.historyStale).toBe(true);
    expect(log).toHaveBeenCalledExactlyOnceWith('[maxed:shell] Live event observer failed', expect.any(Error));
  } finally { log.mockRestore(); setDebugMode(false); }
});

test.each(['resolve', 'reject'] as const)('a superseded transition cannot settle the current owner on %s', async completion => {
  const old = deferred<void>(), current = deferred<void>();
  const query = vi.fn(async () => snapshot());
  const h = setup({ querySnapshot: query }); await settle();
  const first = h.control.transition(() => old.promise);
  const second = h.control.transition(() => current.promise);
  if (completion === 'resolve') old.resolve(); else old.reject(new Error('private'));
  await first; await settle();
  expect(query).toHaveBeenCalledOnce();
  expect(h.states.at(-1)?.phase).toBe('connecting');
  current.resolve(); await second; await settle();
  expect(query).toHaveBeenCalledTimes(2);
  expect(h.states.at(-1)?.phase).toBe('live');
});

test.each(['reconnect', 'dispose'] as const)('a late transition rejection cannot override %s', async action => {
  const pending = deferred<void>();
  const h = setup(); await settle();
  const transition = h.control.transition(() => pending.promise);
  h.control[action](); await settle();
  const count = h.states.length;
  pending.reject(new Error('private')); await transition; await settle();
  expect(h.states).toHaveLength(count);
});

test.each(['resolve', 'reject'] as const)('a transition settling with %s preserves native delivery failure', async completion => {
  const pending = deferred<void>();
  const h = setup(); await settle();
  const transition = h.control.transition(() => pending.promise);
  h.transport.publishCapture({ ...fixture.capture, state: 'Failed', message: 'Live updates interrupted; reconnect or restart Maxed' });
  if (completion === 'resolve') pending.resolve(); else pending.reject(new Error('private'));
  await transition; await settle();
  expect(h.states.at(-1)).toMatchObject({ phase: 'failed', message: 'Restart Maxed.' });
});

test('same-watermark snapshots and reconnect baselines advance by revision only', async () => {
  const baseline = { ...snapshot(), revision: '9007199254740992' };
  const h = setup({ querySnapshot: vi.fn().mockResolvedValueOnce(baseline)
    .mockResolvedValue({ ...baseline, revision: '9007199254740994', map: 'reconnected' }) });
  await settle();
  h.transport.publishSnapshotUpdate({ ...baseline, revision: '9007199254740993', map: 'silent update' });
  expect(h.seen.at(-1)?.map).toBe('silent update');
  h.transport.publishSnapshotUpdate({ ...baseline, map: 'obsolete' });
  expect(h.seen.at(-1)?.map).toBe('silent update');
  h.control.reconnect();
  await settle();
  expect(h.seen.at(-1)?.map).toBe('reconnected');
  expect(h.seen.at(-1)?.sequence).toBe('10');
  // Revision is not a durability watermark.
  expect(h.histories.at(-1)?.durable).toBe('10');
});

test('visibility restore rejects an older revision at the same watermark', async () => {
  const initial = { ...snapshot(), revision: '100' };
  const h = setup({ querySnapshot: vi.fn().mockResolvedValueOnce(initial)
    .mockResolvedValue({ ...initial, revision: '99', map: 'obsolete' }) });
  await settle();
  setNativePresentationVisible(false);
  setNativePresentationVisible(true);
  await settle();
  expect(h.seen).toEqual([initial]);
  h.transport.publishSnapshotUpdate({ ...initial, revision: '101', map: 'fresh' });
  expect(h.seen.at(-1)?.map).toBe('fresh');
});

test('lost subscription ownership leaves live despite healthy snapshots and reconnects with a baseline', async () => {
  vi.useFakeTimers();
  const { LIVE_SUBSCRIPTION_LOST } = await import('../src/lib/bridge/subscription-health');
  const queryStore = vi.fn(async () => store());
  const h = setup({ queryStore });
  await settle();
  expect(h.states.at(-1)?.phase).toBe('live');
  queryStore.mockRejectedValueOnce(LIVE_SUBSCRIPTION_LOST);
  h.transport.publishSnapshot(snapshot('20'));
  await vi.advanceTimersByTimeAsync(1000);
  expect(h.states.at(-1)).toMatchObject({ phase: 'failed', message: 'Disconnected. Reconnect.' });
  h.transport.publishSnapshot(snapshot('30'));
  expect(h.states.at(-1)?.phase).toBe('failed');
  const polls = queryStore.mock.calls.length;
  await vi.advanceTimersByTimeAsync(10_000);
  expect(queryStore).toHaveBeenCalledTimes(polls);
  const onEngineEvent = vi.spyOn(h.client, 'onEngineEvent');
  h.control.reconnect();
  await settle();
  expect(onEngineEvent).toHaveBeenCalledOnce();
  expect(h.states.at(-1)?.phase).toBe('live');
  expect(h.seen.at(-1)?.sequence).toBe('30');
  h.transport.publishEngineEvent(event('31'));
  expect(h.events.at(-1)?.sequence).toBe('31');
});

test.each(['reconnect', 'dispose'] as const)('LOST preserves capture and visibility until %s, releasing every handle once', async action => {
  vi.useFakeTimers();
  const { LIVE_SUBSCRIPTION_LOST, withSubscriptionHealth } = await import('../src/lib/bridge/subscription-health');
  const { isPresentationVisible } = await import('../src/lib/bridge/visibility');
  const health = vi.fn(async () => store());
  const handles: { topic: string; release: ReturnType<typeof vi.fn> }[] = [];
  const handle = (topic: string, leased = false) => {
    const release = vi.fn(); handles.push({ topic, release });
    return leased ? withSubscriptionHealth(release, handles.length, health) : release;
  };
  let capture!: (value: CaptureStatus) => void;
  let visible!: (value: boolean) => void;
  const h = setup({
    onSnapshot: async () => handle('snapshot', true),
    onEngineEvent: async () => handle('event', true),
    onCapture: async receive => { capture = receive; return handle('capture'); },
    onPresentationVisible: async receive => { visible = receive; return handle('visibility'); },
  });
  await settle();
  health.mockRejectedValueOnce(LIVE_SUBSCRIPTION_LOST);
  await vi.advanceTimersByTimeAsync(10_000);
  expect(h.states.at(-1)?.phase).toBe('failed');
  expect(handles.map(row => row.release.mock.calls.length)).toEqual([1, 1, 0, 0]);
  const failed: CaptureStatus = { ...fixture.capture, state: 'Failed', message: 'Synthetic capture failure' };
  capture(failed);
  expect(h.captures.at(-1)).toEqual(failed);
  visible(false); expect(isPresentationVisible()).toBe(false);
  visible(true); expect(isPresentationVisible()).toBe(true);
  expect(h.states.at(-1)?.phase).toBe('failed');
  const oldCapture = capture, oldVisible = visible;
  h.control[action](); await settle();
  expect(handles.slice(0, 4).map(row => row.release.mock.calls.length)).toEqual([1, 1, 1, 1]);
  if (action === 'reconnect') {
    expect(handles).toHaveLength(8);
    expect(handles.slice(4).map(row => row.release.mock.calls.length)).toEqual([0, 0, 0, 0]);
    expect(h.states.at(-1)?.phase).toBe('live');
  }
  const captures = h.captures.length;
  oldCapture(failed); oldVisible(false);
  expect(h.captures).toHaveLength(captures);
  expect(isPresentationVisible()).toBe(true);
  h.control.dispose(); h.control.dispose();
  expect(handles.every(row => row.release.mock.calls.length === 1)).toBe(true);
});
