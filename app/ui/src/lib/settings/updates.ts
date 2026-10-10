import { writable } from 'svelte/store';
import type { Client } from '../bridge/client';
import type { UpdateStatus } from '../bindings/UpdateStatus';

export function createUpdateSession(client: Client, preview = false) {
  const state = writable<{ status: UpdateStatus | null; busy: boolean; error: string }>({ status: null, busy: false, error: '' });
  let disposed = false;
  let busy = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let current: UpdateStatus | null = null;
  async function run(action: () => Promise<UpdateStatus>, errorMessage = 'Couldn’t load updates. Retry.') {
    if (disposed || busy) return;
    if (timer !== undefined) { clearTimeout(timer); timer = undefined; }
    busy = true;
    state.set({ status: current, busy, error: '' });
    try {
      const result = await action();
      if (disposed) return;
      current = result;
      state.set({ status: current, busy: false, error: '' });
      if (['checking', 'downloading', 'installing'].includes(result.phase)) {
        timer = setTimeout(() => { timer = undefined; void run(() => client.queryUpdate()); }, 250);
      }
    } catch {
      if (!disposed) state.set({ status: current, busy: false, error: errorMessage });
    } finally { busy = false; }
  }
  return {
    subscribe: state.subscribe,
    refresh: () => timer !== undefined ? Promise.resolve() : run(() => client.queryUpdate()),
    check: () => preview ? Promise.resolve() : run(() => client.checkForUpdates()),
    download: () => preview || !current?.offer ? Promise.resolve() : run(() => client.downloadUpdate(current!.offer!.request_id)),
    install: () => preview || !current?.offer ? Promise.resolve() : run(() => client.installUpdate(current!.offer!.request_id)),
    restartCapture: () => preview ? Promise.resolve() : run(async () => {
      await client.startCapture();
      return client.queryUpdate();
    }, 'Couldn’t restart capture. Retry.'),
    cancel: () => preview ? Promise.resolve() : run(() => client.cancelUpdate()),
    dispose() { disposed = true; if (timer !== undefined) clearTimeout(timer); },
  };
}
