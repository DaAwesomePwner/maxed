import { writable, type Readable } from 'svelte/store';

// Debug mode reveals data-quality caveats (estimates, partial coverage, provisional splits)
// that production UI hides. It is a per-PC developer preference, never a saved app setting.
const KEY = 'maxed.debug';

function read(): boolean {
  try { return globalThis.localStorage?.getItem(KEY) === '1'; } catch { return false; }
}

const store = writable(read());
export const debugMode: Readable<boolean> = { subscribe: store.subscribe };
let enabled = false;
store.subscribe(value => { enabled = value; });

export function setDebugMode(value: boolean) {
  try { if (value) globalThis.localStorage?.setItem(KEY, '1'); else globalThis.localStorage?.removeItem(KEY); } catch { /* storage unavailable */ }
  store.set(value);
}

export const isDebugMode = () => enabled;

/** Logs a data-quality caveat for debugging. Never shown to users outside debug mode. */
export function caveat(scope: string, message: string, detail?: unknown) {
  if (!enabled) return;
  if (detail === undefined) console.debug(`[maxed:${scope}] ${message}`);
  else console.debug(`[maxed:${scope}] ${message}`, detail);
}
