import { isPresentationVisible, observePresentationVisibility } from '../bridge/visibility';
import type { Client } from '../bridge/client';
import type { FamePoint } from '../bindings/FamePoint';
import type { HomeScope } from '../bindings/HomeScope';
import type { HomeSummary } from '../bindings/HomeSummary';
import type { StoreStatus } from '../bindings/StoreStatus';
import type { HomeMode } from './mode';
import { historyControl, historyFailure, historyWaiting } from '../runs/policy';

export interface HomeHistoryInput {
  mode: HomeMode;
  scope: HomeScope | null;
  streamId: string | null;
  required: string | null;
  initial?: boolean;
  store: StoreStatus | null;
}
export interface HomeHistoryState {
  phase: 'unavailable' | 'loading' | 'ready' | 'stale' | 'superseded' | 'busy' | 'error';
  summary: HomeSummary | null;
  message: string;
  retryAvailable: boolean;
}
export function fameSegments(points: FamePoint[]): FamePoint[][] {
  const segments: FamePoint[][] = [];
  for (const point of points) {
    const last = segments[segments.length - 1];
    if (!last || last[last.length - 1].character_id !== point.character_id) segments.push([point]);
    else last.push(point);
  }
  return segments;
}

/** One in-flight read, one latest desired input, and no numeric snapshot polling. */
export function createHomeHistory(client: Pick<Client, 'queryHome'>, receive: (state: HomeHistoryState) => void) {
  let input: HomeHistoryInput | null = null;
  let identity = '', key = '', generation = 0, namespace = 0;
  let active = false, disposed = false, blocked = false;
  let completed = -1, attempted = -1;
  let summary: HomeSummary | null = null;
  let failed = false;
  let initialRead = true;
  let control: 'superseded' | 'busy' | null = null;
  let automaticRetryUsed = false, backingOff = false;
  let failureMessage = 'Saved history could not be loaded. Retry to refresh it.';
  let emitted: HomeHistoryState | undefined;
  const emit = (phase: HomeHistoryState['phase'], message = '', retryAvailable = phase !== 'loading') => {
    if (disposed || (emitted?.phase === phase && emitted.message === message && emitted.summary === summary && emitted.retryAvailable === retryAvailable)) return;
    emitted = { phase, summary, message, retryAvailable };
    receive(emitted);
  };
  const waiting = historyWaiting;
  function pump(retryInitial = false) {
    if (disposed || !input || !isPresentationVisible() || backingOff) return;
    if (!input.scope) { emit('unavailable', 'No session captured. Choose Today to view saved observations.'); return; }
    if (input.mode !== 'nexus') { emit('stale', 'Saved history refresh is paused while a live area is visible.'); return; }
    if (failed && attempted === generation) { emit('error', failureMessage); return; }
    if (control && attempted === generation) {
      emit(control, 'Saved history is busy. Retry to refresh it.'); return;
    }
    const reason = waiting(input);
    // A known, matching store may supply an initial saved preview while its
    // writer lags. It cannot establish readiness for the snapshot checkpoint.
    const readAhead = reason !== null && (initialRead || retryInitial) && input.initial === true && (input.store?.state === 'ready' || (retryInitial && input.store?.state === 'backpressured'))
      && input.store.stream_id === input.streamId && input.store.durable !== null;
    if (reason && !readAhead) { emit('stale', reason); return; }
    if (completed === generation) { emit('ready'); return; }
    if (active) { emit(summary ? 'stale' : 'loading', summary ? 'Refreshing saved observations.' : 'Loading saved observations.'); return; }
    active = true; attempted = generation; failed = false; control = null; initialRead = false;
    let retryBusy = false;
    const request = generation, requestNamespace = namespace, scope = input.scope, required = input.required;
    emit(summary ? 'stale' : 'loading', summary ? 'Refreshing saved observations.' : 'Loading saved observations.');
    void Promise.resolve().then(() => disposed || requestNamespace !== namespace ? { status: 'superseded' as const } : client.queryHome(scope)).then((result) => {
      // Judge this read against its issued checkpoint, not a newer event that
      // arrived while it was in flight. Keep newer work stale and coalesced.
      if (disposed || requestNamespace !== namespace || !input || input.mode !== 'nexus' || (!readAhead && waiting(input, required))) return;
      if (result.status !== 'ready') {
        if (request !== generation) return;
        const policy = historyControl(result.status, automaticRetryUsed);
        if (policy.retry) {
          automaticRetryUsed = true; retryBusy = true; backingOff = true;
          emit('busy', policy.message, policy.retryAvailable);
        } else {
          control = result.status;
          emit(control, policy.message, policy.retryAvailable);
        }
        return;
      }
      summary = result.value; completed = request;
      const stale = readAhead || request !== generation || waiting(input) !== null;
      emit(stale ? 'stale' : 'ready', stale ? waiting(input) ?? 'Refreshing saved observations.' : '');
    }).catch((error: unknown) => {
      if (disposed || request !== generation || requestNamespace !== namespace) return;
      failed = true;
      failureMessage = historyFailure(error);
      emit('error', failureMessage);
    }).finally(async () => {
      if (retryBusy) await new Promise(resolve => setTimeout(resolve, 2000));
      active = false; backingOff = false;
      if (!disposed && (request !== generation || retryBusy)) pump(retryBusy && readAhead && request === generation);
    });
  }
  const releaseVisibility = observePresentationVisibility(visible => { if (visible) pump(); });
  return {
    update(next: HomeHistoryInput) {
      if (disposed) return;
      const nextIdentity = JSON.stringify([next.streamId, next.scope]);
      const nextKey = JSON.stringify([nextIdentity, next.mode, next.required, next.initial === true]);
      const nextBlocked = waiting(next) !== null;
      if (identity !== nextIdentity || input?.mode !== next.mode) { namespace++; automaticRetryUsed = false; }
      if (identity !== nextIdentity) summary = null;
      if (input?.streamId !== next.streamId) initialRead = true;
      if (key !== nextKey || blocked !== nextBlocked) { generation++; failed = false; control = null; }
      identity = nextIdentity; key = nextKey; blocked = nextBlocked; input = next;
      pump();
    },
    retry() { if (!disposed && input && !active) { generation++; namespace++; failed = false; control = null; automaticRetryUsed = false; pump(); } },
    dispose() { releaseVisibility(); disposed = true; generation++; input = null; summary = null; emitted = undefined; },
  };
}
