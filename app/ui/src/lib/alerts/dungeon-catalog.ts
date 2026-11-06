import { get, writable } from 'svelte/store';
import type { Client } from '../bridge/client';
import type { DungeonDef } from '../bindings/DungeonDef';

export const dungeonCatalog = writable<DungeonDef[] | undefined>(undefined);
const cache = new WeakMap<Client, Promise<DungeonDef[]>>();
let active: Client | undefined;
/** Share in-flight reads and cached metadata; visual changes explicitly refresh it. */
export function loadDungeonCatalog(client: Client, refresh = false): Promise<DungeonDef[]> {
  if (active !== client) { active = client; dungeonCatalog.set(undefined); }
  let request = cache.get(client);
  if (!request || refresh) {
    request = client.listCatalogDungeonOptions();
    cache.set(client, request);
  }
  const current = request;
  void request.then(rows => {
    if (active === client && cache.get(client) === current) dungeonCatalog.set(rows);
  }, () => {
    if (cache.get(client) === current) {
      cache.delete(client);
      if (active === client) dungeonCatalog.set(undefined);
    }
  });
  return request;
}
const watchers = new WeakMap<Client, { count: number; release: () => void; listeners: Set<() => void> }>();
export function watchDungeonCatalog(client: Client, changed?: () => void): () => void {
  let watcher = watchers.get(client);
  if (!watcher) {
    const listeners = new Set<() => void>();
    watcher = { count: 0, listeners, release: client.onVisualsChanged(() => {
      void loadDungeonCatalog(client, true).catch(() => {});
      for (const listener of listeners) listener();
    }) };
    watchers.set(client, watcher);
  }
  watcher.count++;
  if (changed) watcher.listeners.add(changed);
  void loadDungeonCatalog(client).catch(() => {});
  return () => {
    if (changed) watcher.listeners.delete(changed);
    if (--watcher.count === 0) {
      watcher.release(); watchers.delete(client); cache.delete(client);
      if (active === client) { active = undefined; dungeonCatalog.set(undefined); }
    }
  };
}

const asciiLower = (value: string) => value.replace(/[A-Z]/g, letter => letter.toLowerCase());
export function dungeonMetadata(name: string, rows = get(dungeonCatalog)) {
  return rows === undefined ? undefined : rows.find(row => asciiLower(row.name) === asciiLower(name)) ?? null;
}
