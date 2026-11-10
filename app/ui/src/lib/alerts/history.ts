import type { Client } from '../bridge/client';
import { isPresentationVisible, observePresentationVisibility } from '../bridge/visibility';
import type { AlertFilter } from '../bindings/AlertFilter';
import type { AlertPage } from '../bindings/AlertPage';
import type { StoreStatus } from '../bindings/StoreStatus';
import { historyControl, historyFailure, historyWaiting } from '../runs/policy';
import { ALERT_PAGE_SIZE } from './filters';

export interface AlertHistoryInput {
  filter: AlertFilter; scopeKey: string; streamId: string | null;
  required: string | null; store: StoreStatus | null;
}
export interface AlertHistoryState {
  phase: 'loading' | 'ready' | 'stale' | 'superseded' | 'busy' | 'error';
  page: AlertPage | null; message: string; retryAvailable: boolean;
}
export interface AlertHistoryController {
  update(input: AlertHistoryInput): void; retry(): void; dispose(): void;
}

/** One active native read, one captured waiting checkpoint, no timer.
 * Retains only the current 50-row page; navigation retains cursors, never payloads. */
export function createAlertHistory(client: Pick<Client, 'listAlerts'>,
  receive: (state: AlertHistoryState) => void): AlertHistoryController {
  type Checkpoint = { required: string | null; revision: number };
  let input: AlertHistoryInput | null = null, identity = '', epoch = 0, revision = 0;
  let page: AlertPage | null = null, pending: Checkpoint | null = null;
  let latest: Checkpoint = { required: null, revision: 0 };
  let active = false, disposed = false, retryUsed = false;
  let control: 'busy' | 'superseded' | 'error' | null = null, failure = '';
  let emitted: AlertHistoryState | undefined;
  function emit(phase: AlertHistoryState['phase'], message = '', retryAvailable = !['loading', 'superseded'].includes(phase)) {
    if (disposed || (emitted?.phase === phase && emitted.page === page && emitted.message === message && emitted.retryAvailable === retryAvailable)) return;
    emitted = { phase, page, message, retryAvailable }; receive(emitted);
  }
  function pump() {
    if (disposed || !input || !isPresentationVisible()) return;
    if (control) {
      if (control === 'error') emit('error', failure);
      else { const state = historyControl(control, retryUsed); emit(control, state.message, state.retryAvailable); }
      return;
    }
    const waiting = historyWaiting(input, pending?.required ?? input.required);
    if (waiting) { emit('stale', waiting); return; }
    if (active) { emit(page ? 'stale' : 'loading', 'Refreshing saved alerts.', false); return; }
    if (!pending) { emit('ready'); return; }
    const request = pending, token = epoch;
    const filter: AlertFilter = { ...input.filter, limit: ALERT_PAGE_SIZE };
    pending = null; active = true;
    emit(page ? 'stale' : 'loading', 'Refreshing saved alerts.', false);
    void Promise.resolve().then(() => disposed || token !== epoch
      ? { status: 'superseded' as const } : client.listAlerts(filter)).then(reply => {
      if (disposed || token !== epoch || !input) return;
      if (historyWaiting(input, request.required)) { pending = request; return; }
      if (reply.status === 'ready') {
        page = { rows: reply.value.rows.slice(0, ALERT_PAGE_SIZE), next: reply.value.next };
        if (latest.revision > request.revision && !pending) pending = latest;
        emit(pending ? 'stale' : 'ready', pending ? 'Refreshing saved alerts.' : '');
      } else if (reply.status === 'busy') {
        const state = historyControl('busy', retryUsed);
        if (state.retry) { retryUsed = true; pending ??= request; emit('busy', state.message, false); }
        else { control = 'busy'; emit('busy', state.message, true); }
      } else if (!pending) {
        // A newer captured checkpoint still owns its durable follow-up.
        // Only a superseded read with no pending work becomes terminal.
        control = 'superseded'; emit('superseded', '', false);
      }
    }).catch((error: unknown) => {
      if (disposed || token !== epoch) return;
      control = 'error'; failure = historyFailure(error); emit('error', failure);
    }).finally(() => { active = false; if (!disposed) pump(); });
  }
  const releaseVisibility = observePresentationVisibility(visible => {
    if (visible) {
      // Coalesce a durable hidden backlog without starving an older checkpoint.
      if (!active && pending && input && latest.revision > pending.revision
        && historyWaiting(input, latest.required) === null) pending = latest;
      pump();
    }
  });
  return {
    update(next) {
      if (disposed) return;
      const f = next.filter;
      const key = JSON.stringify([next.scopeKey, next.streamId, f.kind, f.kinds ?? [], f.exclude_kinds ?? [], f.range?.start, f.range?.end,
        f.query, f.cursor?.at, f.cursor?.id]);
      const changed = identity !== key, advanced = input?.required !== next.required;
      const recovered = input && historyWaiting(input, pending?.required ?? input.required) !== null
        && historyWaiting(next, pending?.required ?? next.required) === null;
      if (changed) { identity = key; epoch++; page = null; pending = null; control = null; retryUsed = false; }
      if (changed || advanced || (recovered && !pending && !active)) {
        latest = { required: next.required, revision: ++revision };
        if (!pending) pending = latest;
        // Busy twice and failures require Retry; source churn cannot restart them.
        if (control === 'superseded' && advanced) control = null;
      }
      input = { ...next, filter: { ...next.filter, limit: ALERT_PAGE_SIZE } }; pump();
    },
    retry() {
      if (disposed || !input || active || control === 'superseded') return;
      epoch++; retryUsed = false; control = null;
      pending = latest = { required: input.required, revision: ++revision }; pump();
    },
    dispose() { releaseVisibility(); disposed = true; epoch++; input = null; pending = null; page = null; emitted = undefined; },
  };
}
