import type { StoreStatus } from '../bindings/StoreStatus';

export const HISTORY_CATCH_UP = 'Waiting for saved history to catch up.';

export function historyWaiting(input: { streamId: string | null; required: string | null; store: StoreStatus | null }, required = input.required): string | null {
  const store = input.store;
  if (store?.state === 'failed') return 'Saving failed. Saved history may be stale; some observations are unsaved. Check capture storage and retry.';
  if (store?.state === 'backpressured') return 'Saving is backlogged. Saved history may be stale while observations wait to be saved.';
  if (required === null) return null;
  try {
    if (input.streamId !== null && store?.stream_id === input.streamId && store.durable !== null
      && /^\d+$/.test(required) && /^\d+$/.test(store.durable) && BigInt(store.durable) >= BigInt(required)) return null;
  } catch { /* Malformed watermarks never authorize a read. */ }
  return HISTORY_CATCH_UP;
}

export function historyFailure(error: unknown): string {
  const message = error instanceof Error ? error.message : error;
  return typeof message === 'string' && [
    'History schema upgrade failed; close other Maxed instances or resolve local storage access, then Retry.',
    'History schema is newer than this Maxed build; update Maxed, then Retry.',
  ].includes(message) ? message : 'Saved history could not be loaded. Retry to refresh it.';
}

/** Busy receipt is the native retry-ready signal. Never retry Superseded. */
export function historyControl(status: 'busy' | 'superseded', automaticRetryUsed: boolean) {
  const retry = status === 'busy' && !automaticRetryUsed;
  return { retry, retryAvailable: status === 'busy' && !retry,
    message: status === 'superseded' || retry ? 'Refreshing saved history.' : 'Saved history is busy. Retry to refresh it.' };
}
