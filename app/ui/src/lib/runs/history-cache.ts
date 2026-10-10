import type { EventEnvelope } from '../bindings/EventEnvelope';
import type { Route } from '../navigation/routes';
import { queryInvalidates } from './history-dependencies';
import type { HistoryQuery, HistoryValue } from './history';

export const HISTORY_CACHE_CAPACITY = 32;
export const HISTORY_CACHE_MAX_AGE_MS = 300_000;

export interface HistoryCacheEntry {
  value: HistoryValue;
  route: Route;
  query: HistoryQuery;
  required: string | null;
  storedAt: number;
  dirtySince: string | null;
  dirtyAll: boolean;
}
export interface HistoryCache {
  get(key: string): HistoryCacheEntry | undefined;
  begin(key: string, route: Route, query: HistoryQuery): number;
  end(token: number): void;
  put(key: string, entry: { value: HistoryValue; route: Route; query: HistoryQuery; required: string | null }, token?: number): void;
  observe(event: EventEnvelope): void;
  invalidateAll(): void;
  clear(): void;
  route(): Route | undefined;
  fresh(entry: HistoryCacheEntry): boolean;
}

export function isFresh(entry: HistoryCacheEntry, now: number, maxAgeMs: number): boolean {
  return !entry.dirtyAll && entry.dirtySince === null && now - entry.storedAt < maxAgeMs;
}

export function createHistoryCache(options: {
  invalidates: (route: Route, event: EventEnvelope) => boolean;
  route: () => Route | undefined;
  now?: () => number;
  capacity?: number;
  maxAgeMs?: number;
}): HistoryCache {
  const entries = new Map<string, HistoryCacheEntry>();
  const reservations = new Map<number, { key: string; route: Route; query: HistoryQuery; dirtySince: string | null; dirtyAll: boolean }>();
  let nextToken = 0;
  const now = options.now ?? (() => performance.now());
  const capacity = options.capacity ?? HISTORY_CACHE_CAPACITY;
  const maxAgeMs = options.maxAgeMs ?? HISTORY_CACHE_MAX_AGE_MS;
  return {
    get(key) {
      const entry = entries.get(key);
      if (entry) { entries.delete(key); entries.set(key, entry); }
      return entry;
    },
    begin(key, route, query) {
      const token = ++nextToken;
      reservations.set(token, { key, route, query, dirtySince: null, dirtyAll: false });
      return token;
    },
    end(token) { reservations.delete(token); },
    put(key, entry, token) {
      const reservation = token === undefined ? undefined : reservations.get(token);
      if (token !== undefined) reservations.delete(token);
      entries.delete(key);
      entries.set(key, { ...entry, storedAt: now(), dirtySince: reservation?.key === key ? reservation.dirtySince : null,
        dirtyAll: reservation?.key === key && reservation.dirtyAll });
      if (entries.size > capacity) entries.delete(entries.keys().next().value!);
    },
    observe(event) {
      for (const entry of [...entries.values(), ...reservations.values()]) {
        if ((options.invalidates(entry.route, event) || queryInvalidates(entry.query, event)) && entry.dirtySince === null) entry.dirtySince = event.sequence;
      }
    },
    invalidateAll() { for (const entry of [...entries.values(), ...reservations.values()]) entry.dirtyAll = true; },
    clear() { entries.clear(); reservations.clear(); },
    route: () => options.route(),
    fresh: entry => isFresh(entry, now(), maxAgeMs),
  };
}
