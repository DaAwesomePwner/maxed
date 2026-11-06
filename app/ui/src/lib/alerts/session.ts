import { watchDungeonCatalog } from './dungeon-catalog';
import { readonly, writable, type Readable } from 'svelte/store';
import type { AlertPreferences } from '../bindings/AlertPreferences';
import type { Client } from '../bridge/client';

export type AlertSessionStatus = 'idle' | 'loading' | 'saving' | 'ready' | 'failed' | 'retired';
type AlertUpdate = AlertPreferences | (() => AlertPreferences | null);

export interface AlertSession {
  preferences: Readable<AlertPreferences | null>;
  status: Readable<AlertSessionStatus>;
  refresh(): Promise<void>;
  update(preferences: AlertUpdate): Promise<void>;
  dispose(): void;
}

/** One accepted namespace owns a session. Shell retires it before entering another. */
export function createAlertSession(client: Pick<Client, 'querySettings' | 'updateAlertPreferences'> & Partial<Pick<Client, 'listCatalogDungeonOptions' | 'onVisualsChanged'>>,
  receive: (preferences: AlertPreferences | null) => void = () => {},
  canUpdate: () => boolean = () => true): AlertSession {
  const releaseCatalog = client.listCatalogDungeonOptions && client.onVisualsChanged ? watchDungeonCatalog(client as Client) : () => {};
  const preferences = writable<AlertPreferences | null>(null);
  const status = writable<AlertSessionStatus>('idle');
  let disposed = false, revision = 0;
  let pending: Promise<void> = Promise.resolve();
  const publish = (value: AlertPreferences | null) => {
    const accepted = value ? Object.freeze({ ...value, key_pop_dungeons: value.key_pop_dungeons ?? null,
      key_pop_excluded_dungeons: value.key_pop_excluded_dungeons ?? [] }) : null;
    preferences.set(accepted); receive(accepted);
  };
  function guard() {
    if (!canUpdate()) throw new Error('Alert preferences cannot be saved in this session.');
  }
  function enqueue(next: AlertUpdate | null): Promise<void> {
    let request: number | undefined;
    const active = () => !disposed && request !== undefined && request === revision;
    let confirmed = false;
    const operation = pending.then(async () => {
      if (disposed) throw new Error('Alert preferences session has ended.');
      // A queued editor prepares its rebased draft only after the previous readback.
      // Queuing another operation must not suppress that accepted policy publication.
      // Preparation/guard rejections before IPC leave the accepted policy untouched.
      const prepared = typeof next === 'function' ? next() : next;
      if (next !== null && prepared === null) return;
      if (prepared) guard();
      request = ++revision;
      status.set(prepared ? 'saving' : 'loading');
      let saveFailed = false;
      if (prepared) {
        try { await client.updateAlertPreferences(prepared); }
        catch { saveFailed = true; }
      }
      if (disposed) throw new Error('Alert preferences session has ended.');
      try {
        // Even a rejected write needs a readback: retain the real saved policy,
        // while still rejecting the update so the editor keeps its failed draft.
        const settings = await client.querySettings();
        if (settings.version !== 1 || !settings.alerts) throw new Error('Unsupported saved alert preferences.');
        confirmed = true;
        if (active()) { publish(settings.alerts); status.set('ready'); }
      } catch {
        if (active()) publish(null);
        throw new Error(next ? 'Alert preferences could not be saved and confirmed.' : 'Saved alert preferences could not be read.');
      }
      if (saveFailed) throw new Error('Alert preferences could not be saved and confirmed.');
    });
    pending = operation.catch(() => {
      if (active() && !confirmed) status.set('failed');
    });
    return operation;
  }
  return {
    preferences: readonly(preferences),
    status: readonly(status),
    refresh: () => enqueue(null),
    async update(next) {
      // Invalid intent must not supersede an already pending valid read/save.
      guard();
      return enqueue(typeof next === 'function' ? next : { ...next });
    },
    dispose() { if (!disposed) { disposed = true; releaseCatalog(); revision++; publish(null); status.set('retired'); } },
  };
}
