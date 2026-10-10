import { writable } from 'svelte/store';
import type { Client } from '../bridge/client';
import type { RecordingFailure } from '../bindings/RecordingFailure';
import type { RecordingStatus } from '../bindings/RecordingStatus';

export function createRecordingSession(client: Client, preview = false) {
  const state = writable<{ status: RecordingStatus | null; busy: boolean; error: string }>({ status: null, busy: false, error: '' });
  let current: RecordingStatus | null = null;
  let disposed = false;
  let busy = false;
  let failed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const visible = () => typeof document === 'undefined' || document.visibilityState !== 'hidden';
  function schedule() {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    if (!disposed && !failed && !preview && visible() && current && ['armed', 'writing'].includes(current.state)) {
      timer = setTimeout(() => { timer = undefined; void refresh(); }, 1_000);
    }
  }
  function visibilityChanged() {
    // Resume the single status timer, never a backlog of hidden-window reads.
    if (!busy) schedule();
    else if (!visible() && timer !== undefined) { clearTimeout(timer); timer = undefined; }
  }
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', visibilityChanged);
  async function refresh() {
    if (disposed || busy) return;
    if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
    busy = true;
    state.set({ status: current, busy, error: '' });
    try {
      const status = await client.queryRecording();
      if (disposed) return;
      current = status;
      failed = false;
      state.set({ status, busy: false, error: '' });
      schedule();
    } catch {
      failed = true;
      if (!disposed) state.set({ status: current, busy: false, error: 'Couldn’t load recording. Retry.' });
    } finally { busy = false; }
  }
  return {
    subscribe: state.subscribe,
    refresh,
    dispose() {
      disposed = true;
      if (timer !== undefined) clearTimeout(timer);
      if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', visibilityChanged);
    },
  };
}

export const recordingFailureMessages: Record<RecordingFailure, string> = {
  in_use: 'Close other Maxed instances.',
  quota: 'Check recording storage.',
  queue_full: 'Toggle recording to retry.',
  disk: 'Check disk space and access.',
  invalid_frame: 'Recording Stopped',
};
