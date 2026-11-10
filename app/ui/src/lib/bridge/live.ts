import { LIVE_SUBSCRIPTION_LOST, queryOwnedStore } from './subscription-health';
import { EVENT_BATCH_LIMIT as BATCH_LIMIT } from '../bindings/limits';
import type { CaptureStatus } from '../bindings/CaptureStatus';
import type { EventBatch } from '../bindings/EventBatch';
import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { LiveSnapshot } from '../bindings/LiveSnapshot';
import type { LiveSnapshotUpdate } from '../bindings/LiveSnapshotUpdate';
import type { StoreStatus } from '../bindings/StoreStatus';
import type { Client } from './client';
import { isPresentationVisible, observePresentationVisibility, setNativePresentationVisible } from './visibility';

export function classifySequence(previous: string, next: string) {
  const a = BigInt(previous), b = BigInt(next);
  return b <= a ? 'stale' : b === a + 1n ? 'next' : 'gap';
}

export interface LiveState {
  phase: 'connecting' | 'live' | 'resyncing' | 'failed';
  historyStale: boolean;
  historyFailed: boolean;
  historyLagging: boolean;
  message: string;
}
export interface LiveObserver {
  /** Execute ordered callbacks as one presentation transaction (also on resync). */
  batch?: (apply: () => void) => void;
  snapshot: (snapshot: LiveSnapshot) => void;
  capture?: (capture: CaptureStatus) => void;
  /** Received events only, exactly once after the initial baseline; may drive side effects. */
  event?: (event: EventEnvelope) => void;
  /** Optional incremental projection; never called for an event covered by a snapshot. */
  project?: (event: EventEnvelope) => void;
  state?: (state: LiveState) => void;
  /** Every observed storage result; historyReady remains durability-gated. */
  store?: (status: StoreStatus) => void;
  historyReady?: (status: StoreStatus) => void;
  historyInvalidates?: (event: EventEnvelope) => boolean;
  /** Authoritative query baseline only; never emitted for numeric snapshot ticks. */
  historyBaseline?: (snapshot: LiveSnapshot) => void;
}

// Legacy fixture/offline snapshots predate presentation revisions.
const revision = (snapshot: LiveSnapshot) => BigInt(snapshot.revision ?? snapshot.sequence);
const LIMIT = 512;
// Match the eight-batch native queue, independently of a single transaction.
const BUFFER_LIMIT = 8 * BATCH_LIMIT;
const FAILED = 'Disconnected. Reconnect.';
const HISTORY_EVENTS = new Set([
  'run_started', 'run_updated', 'run_ended', 'fight_ended', 'bag_dropped',
  'drop_updated', 'player_died', 'character_merged', 'fame_changed',
]);
export const invalidatesHomeHistory = (event: EventEnvelope) => Object.keys(event.event).some((kind) => HISTORY_EVENTS.has(kind));
const backoff = (attempt: number) => Math.min(250 * 2 ** attempt, 4000);

/** One owner per mounted app. Routes consume its state, never create listeners. */
export function subscribeLive(client: Client, observer: LiveObserver) {
  let disposed = false, registered = false, transitioning = false;
  let visible = isPresentationVisible();
  let visibilityRequest = 0;
  let transitionEpoch = 0;
  let generation = 0, registration = 0, captureRevision = 0;
  let releases: (() => void)[] = [];
  let stream: string | undefined;
  let candidate: string | undefined;
  const retired = new Set<string>();
  function retire(id: string) {
    retired.add(id);
    // This is a fast rejection cache, not authority. An evicted namespace must
    // still pass querySnapshot and generation checks before it can be shown.
    if (retired.size > LIMIT) retired.delete(retired.values().next().value!);
  }
  let cursor = 0n, projected = -1n, displayed = -1n;
  let redisplay = false;
  let buffered: EventEnvelope[] = [];
  const bufferedKeys = new Set<string>();
  let bufferedMessages = 0, messageBuffered = false;
  let batching = 0, previousState = '';
  function transaction(apply: () => void) {
    if (!batching) previousState = '';
    batching++;
    try { if (observer.batch) observer.batch(apply); else apply(); }
    finally { batching--; }
  }
  let pendingSnapshot: LiveSnapshot | undefined;
  let wireSnapshot: LiveSnapshot | undefined;
  let requiredSnapshot: { stream: string; revision: bigint } | undefined;
  const handled = new Set<string>();
  let queryRunning = false, requested = false, syncing = true, attempts = 0;
  let retryTimer: ReturnType<typeof setTimeout> | undefined;
  let storeTimer: ReturnType<typeof setTimeout> | undefined;
  let storeRunning = false, storeTimerIdle = false;
  let historyStale = true, durableTarget = 0n;
  let historyFailed = false, historyLagging = false;
  let durableSeen: bigint | undefined, lagTarget: bigint | undefined;
  let lagTimer: ReturnType<typeof setTimeout> | undefined;
  let phase: LiveState['phase'] = 'connecting';

  function state(next: LiveState['phase'], message = '') {
    phase = next;
    const key = `${phase}:${historyStale}:${historyFailed}:${historyLagging}:${message}`;
    if (batching && key === previousState) return;
    previousState = key;
    observer.state?.({ phase, historyStale, historyFailed, historyLagging, message });
  }
  function capture(value: CaptureStatus) {
    observer.capture?.(value);
    if (value.state === 'Failed' && value.message === 'Live updates interrupted; reconnect or restart Maxed') {
      fail('Restart Maxed.');
    }
  }
  function clearTimers() {
    clearTimeout(retryTimer); retryTimer = undefined;
    clearTimeout(storeTimer); storeTimer = undefined;
    clearLag();
  }
  function releaseListeners() {
    registration++;
    registered = false;
    for (const release of releases.splice(0)) release();
  }
  function fail(message = FAILED) {
    if (disposed) return;
    generation++;
    syncing = true;
    requested = false;
    historyStale = true;
    clearTimers();
    state('failed', message);
  }
  function stale(target: bigint) {
    durableTarget = target > durableTarget ? target : durableTarget;
    if (!historyFailed && durableSeen !== undefined && durableSeen >= durableTarget) return;
    historyStale = true;
    if (lagTarget === undefined) startLag();
    state(phase);
  }
  function clearLag() {
    clearTimeout(lagTimer); lagTimer = undefined;
    lagTarget = undefined;
    historyLagging = false;
  }
  function startLag() {
    clearLag();
    if (!visible) return;
    // Track an outstanding checkpoint, not the moving newest watermark. A
    // healthy writer one flush behind keeps retiring these checkpoints.
    lagTarget = durableTarget;
    lagTimer = setTimeout(() => {
      lagTimer = undefined;
      if (!disposed && historyStale && phase !== 'failed') {
        historyLagging = true;
        state(phase);
      }
    }, 5001);
  }
  function scheduleStore(idle = false) {
    if (disposed || !visible || transitioning || !registered || stream === undefined || phase === 'failed') return;
    if (storeTimer) {
      if (!storeTimerIdle || idle) return;
      clearTimeout(storeTimer);
    }
    storeTimerIdle = idle;
    // Do not reset an active timer on updates: continuous traffic still gets a
    // check within one second, with no per-event or per-snapshot IPC.
    storeTimer = setTimeout(() => { storeTimer = undefined; pollStore(); }, idle ? 10_000 : 1000);
  }
  function pollStore() {
    if (disposed || !visible || transitioning || !registered || stream === undefined || storeRunning || storeTimer || phase === 'failed') return;
    const token = generation, expected = stream;
    storeRunning = true;
    void queryOwnedStore(releases, () => client.queryStore()).then((status) => {
      if (disposed || token !== generation || expected !== stream) return;
      observer.store?.(status);
      const empty = status.state === 'closed' && status.stream_id === null && durableTarget === 0n;
      historyFailed = status.state === 'failed';
      const blocked = historyFailed || status.state === 'backpressured';
      durableSeen = status.stream_id === expected && status.durable !== null ? BigInt(status.durable) : undefined;
      if (!blocked && (empty || (durableSeen !== undefined && durableSeen >= durableTarget))) {
        historyStale = false;
        clearLag();
        state(phase);
        observer.historyReady?.(status);
      } else {
        historyStale = true;
        if (lagTarget === undefined || (!blocked && durableSeen !== undefined && durableSeen >= lagTarget)) startLag();
        state(phase);
      }
    }).catch((error: unknown) => {
      if (disposed || token !== generation || expected !== stream) return;
      if (error === LIVE_SUBSCRIPTION_LOST) { releaseListeners(); fail(); return; }
      historyFailed = true;
      stale(durableTarget);
    }).finally(() => {
      storeRunning = false;
      scheduleStore(!historyStale);
    });
  }
  function show(snapshot: LiveSnapshot) {
    const seq = BigInt(snapshot.sequence);
    const nextRevision = revision(snapshot);
    if (snapshot.stream_id !== stream || nextRevision < displayed
      || (nextRevision === displayed && !redisplay) || seq < projected) return;
    displayed = nextRevision;
    redisplay = false;
    projected = seq;
    observer.snapshot(snapshot);
  }
  function deliver(event: EventEnvelope) {
    const key = `${event.stream_id}:${event.sequence}`;
    if (handled.has(key)) return;
    handled.add(key);
    if (handled.size > LIMIT) handled.delete(handled.values().next().value!);
    const seq = BigInt(event.sequence);
    if (invalidatesHomeHistory(event) || observer.historyInvalidates?.(event)) {
      stale(seq);
      scheduleStore();
    }
    observer.event?.(event);
    if (seq > projected) {
      projected = seq;
      observer.project?.(event);
    }
  }
  function buffer(event: EventEnvelope) {
    const key = `${event.stream_id}:${event.sequence}`;
    if (bufferedKeys.has(key)) return;
    if (!messageBuffered) { bufferedMessages++; messageBuffered = true; }
    if (bufferedMessages > LIMIT || buffered.length >= BUFFER_LIMIT) {
      // Lost side effects cannot be reconstructed. Stop this subscription epoch
      // visibly and require an explicit reconnect with a new baseline.
      buffered = []; bufferedKeys.clear(); bufferedMessages = 0;
      generation++;
      fail('Disconnected. Reconnect.');
      return;
    }
    buffered.push(event); bufferedKeys.add(key);
  }
  function requestSync(preserveWindow = false) {
    if (disposed || phase === 'failed') return;
    // Numeric rebases must not pause or advance the ordered event cursor: the
    // query can overtake native events that have not reached the UI yet.
    syncing ||= !preserveWindow;
    requested = true;
    if (!preserveWindow) state(stream === undefined || candidate !== stream ? 'connecting' : 'resyncing');
    void query();
  }
  function observeStream(incoming: string) {
    if (candidate === incoming || stream === incoming || transitioning) return;
    if (candidate !== undefined || stream !== undefined) {
      // An in-flight query from before this stream was observed is obsolete.
      generation++;
      clearTimers();
      attempts = 0;
    }
    candidate = incoming;
    historyStale = true;
  }
  function receiveEvent(event: EventEnvelope) {
    if (disposed || phase === 'failed' || retired.has(event.stream_id)) return;
    if (stream === undefined || event.stream_id !== stream || syncing || transitioning) {
      observeStream(event.stream_id);
      buffer(event);
      if (!transitioning) requestSync(phase === 'live' && candidate === stream);
      return;
    }
    const relation = classifySequence(cursor.toString(), event.sequence);
    if (relation === 'stale') return;
    if (relation === 'gap') {
      buffer(event);
      stale(BigInt(event.sequence));
      requestSync();
      return;
    }
    cursor = BigInt(event.sequence);
    deliver(event);
  }
  function receiveBatch(batch: EventBatch) {
    if (disposed || phase === 'failed' || retired.has(batch.stream_id)) return;
    // Validate the complete range before applying any member. Never turn an
    // internal hole or malformed range into apparently contiguous delivery.
    let valid = batch.events.length > 0 && batch.events.length <= BATCH_LIMIT;
    try {
      const first = BigInt(batch.first_sequence), last = BigInt(batch.last_sequence);
      valid &&= last === first + BigInt(batch.events.length) - 1n;
      valid &&= batch.events.every((event, index) => event.stream_id === batch.stream_id
        && BigInt(event.sequence) === first + BigInt(index));
    } catch { valid = false; }
    if (!valid) { fail('Invalid live event batch. Reconnect.'); return; }
    messageBuffered = false;
    transaction(() => { for (const event of batch.events) receiveEvent(event); });
  }
  function receiveUpdate(update: LiveSnapshotUpdate) {
    const snapshot = 'base_sequence' in update ? update.snapshot : update;
    if (disposed || phase === 'failed' || retired.has(snapshot.stream_id)) return;
    if (wireSnapshot?.stream_id === snapshot.stream_id && revision(snapshot) <= revision(wireSnapshot)) return;
    if ('base_sequence' in update) {
      if (!wireSnapshot || wireSnapshot.stream_id !== snapshot.stream_id || wireSnapshot.sequence !== update.base_sequence || revision(wireSnapshot) !== BigInt(update.base_revision ?? update.base_sequence)) {
        observeStream(snapshot.stream_id);
        wireSnapshot = undefined;
        const nextRevision = revision(snapshot);
        if (requiredSnapshot?.stream !== snapshot.stream_id || nextRevision > requiredSnapshot.revision) {
          requiredSnapshot = { stream: snapshot.stream_id, revision: nextRevision };
        }
        // Missing numeric deltas do not imply lost ordered visit events.
        // Preserve their window while replacing the numeric baseline.
        requestSync(snapshot.stream_id === stream && phase === 'live');
        return;
      }
      const merge = <T extends { id: string }>(before: T[], changes: T[], removed: string[]) => {
        const rows = new Map(before.map(row => [row.id, row]));
        for (const id of removed) rows.delete(id);
        for (const row of changes) rows.set(row.id, row);
        return [...rows.values()].sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
      };
      wireSnapshot = { ...snapshot,
        fights: merge(wireSnapshot.fights, snapshot.fights, update.removed_fights),
        drops: snapshot.drops.length === 0 && update.removed_drops.length === 0 ? wireSnapshot.drops
          : merge(wireSnapshot.drops, snapshot.drops, update.removed_drops) };
    } else wireSnapshot = snapshot;
    receiveSnapshot(wireSnapshot);
  }
  function receiveSnapshot(snapshot: LiveSnapshot) {
    if (disposed || phase === 'failed' || retired.has(snapshot.stream_id)) return;
    if (syncing || transitioning || stream === undefined || snapshot.stream_id !== stream) {
      observeStream(snapshot.stream_id);
      if (!pendingSnapshot || pendingSnapshot.stream_id !== snapshot.stream_id || revision(snapshot) > revision(pendingSnapshot)) pendingSnapshot = snapshot;
      if (!transitioning && snapshot.stream_id !== stream) requestSync();
      return;
    }
    // Coalescing may skip snapshot revisions without losing ordered events.
    show(snapshot);
    scheduleStore();
  }
  async function query() {
    // Ordered events still arrive while hidden; their baseline must drain the
    // recovery buffer even while ordinary numeric refreshes are paused.
    if (disposed || (!visible && !syncing) || transitioning || !registered || queryRunning || retryTimer || !requested || phase === 'failed') return;
    queryRunning = true;
    requested = false;
    const token = generation;
    const observed = candidate;
    try {
      const snapshot = await client.querySnapshot();
      if (disposed || token !== generation) return;
      if (candidate !== observed && candidate !== snapshot.stream_id) {
        // The very first query can also race a stream that was not known when
        // it started. Subscribe-before-query does not make that result current.
        generation++;
        requested = true;
        return;
      }
      if (retired.has(snapshot.stream_id)) throw new Error('Retired stream');
      const changed = stream !== snapshot.stream_id;
      if (changed) {
        if (stream !== undefined) retire(stream);
        stream = snapshot.stream_id;
        cursor = BigInt(snapshot.sequence);
        projected = -1n;
        displayed = -1n;
        durableTarget = 0n;
        durableSeen = undefined;
        historyFailed = false;
        clearLag();
        handled.clear();
      }
      candidate = stream;
      // Any other stream observed before this authoritative query is retired.
      for (const event of buffered) {
        if (event.stream_id !== stream) retire(event.stream_id);
      }
      if (pendingSnapshot && pendingSnapshot.stream_id !== stream) retire(pendingSnapshot.stream_id);
      if (requiredSnapshot && requiredSnapshot.stream !== stream) {
        retire(requiredSnapshot.stream); requiredSnapshot = undefined;
      }
      const previous = cursor;
      let baseline = snapshot;
      for (const received of [pendingSnapshot, wireSnapshot]) {
        if (received && received.stream_id === stream && revision(received) > revision(baseline)) baseline = received;
      }
      if (requiredSnapshot && revision(baseline) < requiredSnapshot.revision) {
        // A delta can overtake an in-flight query. Keep its required watermark
        // even when no further tick arrives (for example, after leaving a map).
        scheduleRetry(phase === 'live');
        return;
      }
      requiredSnapshot = undefined;
      wireSnapshot = baseline;
      show(baseline);
      observer.historyBaseline?.(baseline);
      pendingSnapshot = undefined;
      const events = buffered.filter((event) => event.stream_id === stream)
        .sort((a, b) => BigInt(a.sequence) < BigInt(b.sequence) ? -1 : BigInt(a.sequence) > BigInt(b.sequence) ? 1 : 0);
      buffered = []; bufferedKeys.clear(); bufferedMessages = 0;
      if (syncing && BigInt(snapshot.sequence) > cursor) cursor = BigInt(snapshot.sequence);
      // During resync, received discrete events remain deliverable even when the
      // new snapshot covers them. Initial/changed-stream history stays silent.
      if (events.length) transaction(() => {
        for (const event of events) {
          const seq = BigInt(event.sequence);
          if (seq <= previous) continue;
          if (seq <= cursor || seq === cursor + 1n) {
            deliver(event);
            if (seq > cursor) cursor = seq;
          } else {
            buffered.push(event); bufferedKeys.add(`${event.stream_id}:${event.sequence}`);
          }
        }
      });
      if (buffered.length) bufferedMessages = 1;
      stale(projected > cursor ? projected : cursor);
      if (buffered.length) {
        stale(BigInt(buffered[buffered.length - 1].sequence));
        scheduleRetry();
      } else {
        attempts = 0;
        syncing = false;
        requested = false;
        state('live');
      }
      if (storeTimer) scheduleStore(); else pollStore();
    } catch {
      if (!disposed && token === generation) scheduleRetry(phase === 'live');
    } finally {
      queryRunning = false;
      if (!disposed && token !== generation && requested) void query();
    }
  }
  function scheduleRetry(preserveWindow = false) {
    if (!visible && !syncing) return;
    if (++attempts >= 4) { fail(); return; }
    syncing ||= !preserveWindow;
    requested = true;
    if (!preserveWindow) state('resyncing', 'Refreshing');
    retryTimer = setTimeout(() => { retryTimer = undefined; void query(); }, backoff(attempts - 1));
  }
  async function register() {
    const epoch = registration;
    const active = () => !disposed && epoch === registration;
    const add = async (pending: Promise<() => void>) => {
      const release = await pending;
      if (!active()) { release(); return false; }
      releases.push(release);
      return true;
    };
    try {
      if (!await add(client.onSnapshot((value) => { if (active() && visible) receiveUpdate(value); }))) return;
      if (!await add(client.onEngineEvent((value) => { if (active()) { messageBuffered = false; receiveEvent(value); } },
        (value) => { if (active()) receiveBatch(value); }))) return;
      if (!await add(client.onCapture((value) => {
        if (active()) {
          captureRevision++;
          capture(value);
        }
      }))) return;
      registered = true;
      if (!await add(client.onPresentationVisible(value => {
        if (active()) { visibilityRequest++; setNativePresentationVisible(value); }
      }))) return;
      syncDocumentVisibility();
      const before = captureRevision;
      void client.queryCapture().then((value) => {
        if (active() && before === captureRevision) capture(value);
      }).catch(() => { if (active()) fail(); });
      requestSync();
    } catch {
      if (active()) { releaseListeners(); fail(); }
    }
  }
  function invalidate() {
    generation++;
    captureRevision++;
    clearTimers();
    buffered = []; bufferedKeys.clear(); bufferedMessages = 0;
    pendingSnapshot = undefined;
    wireSnapshot = undefined;
    requiredSnapshot = undefined;
    candidate = undefined;
    syncing = true;
    requested = true;
    attempts = 0;
    historyStale = true;
    historyFailed = false;
    state('connecting');
  }
  async function transition(action: () => Promise<void>) {
    if (disposed) return;
    invalidate(); // Invalidate old completions before awaiting native lifecycle I/O.
    transitioning = true;
    // Visibility invalidates queries, but cannot cancel lifecycle ownership.
    const token = ++transitionEpoch;
    try {
      await action();
      if (disposed || token !== transitionEpoch) return;
      transitioning = false;
      if (phase === 'failed') return;
      if (!registered) {
        releaseListeners();
        await register();
      } else {
        requestSync();
      }
    } catch {
      if (!disposed && token === transitionEpoch) {
        transitioning = false;
        if (phase !== 'failed') fail('Capture failed. Reconnect.');
      }
    }
  }

  const releaseVisibility = observePresentationVisibility(next => {
    if (visible === next || disposed) return;
    visible = next;
    generation++; clearTimers();
    pendingSnapshot = undefined; wireSnapshot = undefined; requiredSnapshot = undefined;
    attempts = 0;
    if (visible && phase !== 'failed') {
      // Ordered events continue while hidden. Only numeric snapshots need a
      // fresh baseline, including when no sequence advanced while hidden.
      // Re-show an unchanged revision, without admitting an older revision or
      // regressing behind hidden events.
      redisplay = true;
      requestSync(phase === 'live');
    }
  });
  const syncDocumentVisibility = () => {
    const request = ++visibilityRequest;
    void client.setDocumentVisible(typeof document === 'undefined' || document.visibilityState !== 'hidden')
      .then(value => { if (!disposed && request === visibilityRequest) setNativePresentationVisible(value); })
      .catch(() => { /* Native window events still gate delivery if this invocation fails. */ });
  };
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', syncDocumentVisibility);
  state('connecting');
  void register();
  return {
    dispose() {
      if (disposed) return;
      disposed = true;
      transitionEpoch++;
      releaseVisibility();
      if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', syncDocumentVisibility);
      generation++;
      clearTimers();
      releaseListeners();
      buffered = []; bufferedKeys.clear(); bufferedMessages = 0;
      wireSnapshot = undefined; pendingSnapshot = undefined; requiredSnapshot = undefined;
    },
    reconnect() {
      if (disposed) return;
      transitionEpoch++;
      invalidate();
      transitioning = false;
      releaseListeners();
      void register();
    },
    transition,
    setDemo: (enabled: boolean) => transition(() => client.setDemo(enabled)),
    setAdapter: (adapterId: string | null) => transition(() => client.setAdapter(adapterId)),
  };
}
