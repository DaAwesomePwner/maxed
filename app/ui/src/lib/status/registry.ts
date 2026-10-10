import { writable } from 'svelte/store';

export const STATUS_REGISTRY = Symbol('background activity');

/** One registry per shell. Each mounted producer owns only its own receipt. */
export function createStatusRegistry() {
  const message = writable('');
  const entries = new Map<symbol, string>();
  const publish = () => message.set([...new Set(entries.values())].join(' · '));
  return {
    subscribe: message.subscribe,
    add(text: string) {
      const key = Symbol();
      entries.set(key, text); publish();
      return () => { entries.delete(key); publish(); };
    },
  };
}
export type StatusRegistry = ReturnType<typeof createStatusRegistry>;

/** Retry-ready errors belong to the panel; terminal superseded reads have no work in flight. */
export function historyStatus(state: { phase: string; retryAvailable: boolean; message?: string } | undefined, subject: string): string | null {
  if (!state || state.phase === 'error' || state.phase === 'ready' || state.phase === 'unavailable' || state.phase === 'superseded'
    || (state.phase === 'busy' && state.retryAvailable) || state.message?.startsWith('Saving failed.')) return null;
  return `${state.phase === 'loading' ? 'Loading' : 'Refreshing'} ${subject}`;
}
