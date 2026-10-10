import type { HistoryAdapter } from '../../src/lib/navigation/history';

/** Same push/replace/traverse contract as window.history, with deterministic popstate. */
export function fakeHistory(hash = '#/') {
  let position = 0;
  let entries: { state: unknown; hash: string }[] = [{ state: null, hash }];
  const listeners = new Set<() => void>();
  const adapter: HistoryAdapter = {
    get hash() { return entries[position].hash; },
    get state() { return structuredClone(entries[position].state); },
    push(state, nextHash) { entries = entries.slice(0, position + 1); entries.push({ state: structuredClone(state), hash: nextHash }); position++; },
    replace(state, nextHash) { entries[position] = { state: structuredClone(state), hash: nextHash }; },
    go(delta) {
      const next = position + delta;
      if (next < 0 || next >= entries.length) return;
      position = next;
      for (const listener of listeners) listener();
    },
    listen(receive) { listeners.add(receive); return () => { listeners.delete(receive); }; },
  };
  return { adapter, get listeners() { return listeners.size; }, get length() { return entries.length; } };
}
