import { untrack } from 'svelte';
import type { HistoryCache } from '../runs/history-cache';
import type { Client } from '../bridge/client';
import type { EventFilter } from '../bindings/EventFilter';
import type { EventSummary } from '../bindings/EventSummary';
import type { StoreStatus } from '../bindings/StoreStatus';
import { createRunHistory, type HistoryState } from '../runs/history';

/** Queue range, then all-time summaries only after the page settles. */
export function createLogsSummaries(options: {
  reader: () => Client | undefined; ready: () => boolean; key: () => string;
  filter: () => EventFilter; scopeKey: () => string; streamId: () => string | null;
  store: () => StoreStatus | null; cache?: HistoryCache;
}) {
  let summary = $state.raw<EventSummary | null>(null), allTime = $state.raw<EventSummary | null>(null);
  let summaryKey = $state<string | null>(null), allTimeRead = $state(false);
  const settled = (state: HistoryState) => state.phase === 'ready' || state.phase === 'error' || (state.phase === 'busy' && state.retryAvailable);
  $effect(() => {
    const owner = options.reader(), ready = options.ready(), key = options.key();
    if (!owner || !ready || summaryKey === key) return;
    const instance = createRunHistory(owner, next => {
      if (settled(next)) {
        summary = next.phase === 'ready' && next.value?.kind === 'event-summary' ? next.value.summary : null;
        summaryKey = key;
        if (!options.filter().range && !allTimeRead) { allTime = summary; allTimeRead = true; }
      }
    }, { cache: options.cache });
    untrack(() => instance.update({ query: { kind: 'event-summary', request: { filter: { ...options.filter(), query: '' } } }, scopeKey: options.scopeKey(), streamId: options.streamId(), required: null, store: options.store() }));
    return () => instance.dispose();
  });
  $effect(() => {
    const owner = options.reader(), ready = options.ready() && summaryKey === options.key();
    if (!owner || !ready || allTimeRead) return;
    const instance = createRunHistory(owner, next => {
      if (settled(next)) {
        allTime = next.phase === 'ready' && next.value?.kind === 'event-summary' ? next.value.summary : null;
        allTimeRead = true;
      }
    }, { cache: options.cache });
    untrack(() => instance.update({ query: { kind: 'event-summary', request: { filter: { ...options.filter(), range: null, query: '' } } }, scopeKey: options.scopeKey(), streamId: options.streamId(), required: null, store: options.store() }));
    return () => instance.dispose();
  });
  return {
    get summary() { return summary; }, get allTime() { return allTime; },
    get summaryKey() { return summaryKey; }, get allTimeRead() { return allTimeRead; },
    get settled() { return summaryKey === options.key() && allTimeRead; },
  };
}
