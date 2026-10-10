import { tick, untrack } from 'svelte';
import type { ListState } from '../navigation/state';

/** Release navigation restoration only once, after the rows have reached the DOM. */
export function releaseLogsRows(options: {
  ready: () => boolean; enabled: () => boolean; entryKey: () => string;
  onReady: (key: string) => void;
}) {
  let releasedEntry: string | undefined;
  $effect(() => {
    const ready = options.ready(), key = options.entryKey();
    if (!ready || !options.enabled() || releasedEntry === key) return;
    let current = true;
    void tick().then(() => {
      if (current) { releasedEntry = key; options.onReady(key); }
    });
    return () => { current = false; };
  });
}

export function createLogsList<Cursor>(options: {
  previous: Array<Cursor | null>; scrollTop: number;
  required: () => string | null; namespace: () => string; pendingCount: () => number;
  cursor: () => Cursor | null; setCursor: (cursor: Cursor | null) => void;
  next: () => Cursor | null | undefined; persist: () => void;
  region: () => HTMLElement | undefined; rows: () => unknown[] | undefined;
  available: () => boolean; enabled: () => boolean; entryKey: () => string;
  onReady: (key: string) => void; onStateChange: (patch: Partial<ListState>) => void;
  innerScroll?: boolean; removedMessage?: string;
}) {
  let previous = $state(options.previous), paging = $state(false), missingRow = $state('');
  let scrollTop = $state(options.scrollTop);
  let appliedRequired = $state(untrack(options.required)), pendingRequired = $state<string | null>(null);
  let namespace = untrack(options.namespace);
  const atHead = $derived(options.cursor() === null);
  const inner = (region: HTMLElement | undefined) => options.innerScroll ? region?.querySelector<HTMLElement>('.scroll') : null;
  const position = (region: HTMLElement | undefined, fallback: number) => Math.max(region?.closest('main')?.scrollTop ?? fallback, inner(region)?.scrollTop ?? 0);
  $effect(() => {
    const region = options.region(), pane = region?.closest('main');
    if (!pane || !region) return;
    const onScroll = () => { scrollTop = position(region, 0); };
    pane.addEventListener('scroll', onScroll, { passive: true });
    if (options.innerScroll) region.addEventListener('scroll', onScroll, { passive: true, capture: true });
    return () => {
      pane.removeEventListener('scroll', onScroll);
      if (options.innerScroll) region.removeEventListener('scroll', onScroll, true);
    };
  });
  $effect(() => {
    const latest = options.required(), head = atHead, fallback = scrollTop, scope = options.namespace();
    untrack(() => {
      // Scroll events can lag the DOM position when a history update arrives.
      const top = position(options.region(), fallback) < 8;
      if (scope !== namespace) {
        namespace = scope; appliedRequired = latest; pendingRequired = null;
      } else if (head && top) {
        appliedRequired = latest; pendingRequired = null;
      } else if (latest !== appliedRequired) pendingRequired = latest;
    });
  });
  $effect.pre(() => {
    const rows = options.rows(), region = options.region();
    if (!rows || !region) return;
    const active = document.activeElement;
    const id = active instanceof HTMLElement && region.contains(active) ? active.closest<HTMLElement>('[data-row-id]')?.dataset.rowId : null;
    if (!id) return;
    void tick().then(() => {
      const region = options.region();
      if (!region?.isConnected || active?.isConnected) return;
      const pane = region.closest('main');
      const replacement = [...region.querySelectorAll<HTMLElement>('[data-row-id]')].find(row => row.dataset.rowId === id);
      if (replacement) { replacement.focus({ preventScroll: true }); return; }
      pane?.querySelector<HTMLElement>('h1')?.focus({ preventScroll: true });
      if (pane) pane.scrollTop = Math.max(0, Math.min(pane.scrollTop, pane.scrollHeight - pane.clientHeight));
      missingRow = options.removedMessage ?? 'Row Removed';
      options.onStateChange({ focusedRowId: null, ...(pane ? { scrollTop: pane.scrollTop } : {}) });
    });
  });
  releaseLogsRows({ ready: options.available, enabled: options.enabled, entryKey: options.entryKey, onReady: options.onReady });
  return {
    get previous() { return previous; },
    get paging() { return paging; }, set paging(value: boolean) { paging = value; },
    get missingRow() { return missingRow; },
    get appliedRequired() { return appliedRequired; },
    get hasNew() { return !!pendingRequired && options.pendingCount() > 0; },
    persist(patch: Partial<ListState>, resetScroll = true) {
      missingRow = '';
      options.onStateChange({ ...patch, ...(resetScroll ? { scrollTop: 0, focusedRowId: null } : {}) });
      if (!resetScroll) return;
      const region = options.region(), pane = region?.closest('main'), table = inner(region);
      if (pane) pane.scrollTop = 0;
      if (table) table.scrollTop = 0;
      scrollTop = 0;
    },
    apply<Filter>(next: Filter, current: Filter, commit: () => void, retry: () => void) {
      const same = JSON.stringify(next) === JSON.stringify(current);
      previous = []; paging = false; commit(); options.persist();
      if (same) retry();
    },
    showNew() {
      options.setCursor(null); previous = []; appliedRequired = options.required(); pendingRequired = null; options.persist();
    },
    next() {
      const next = options.next();
      if (paging || !next) return;
      paging = true; previous = [...previous, options.cursor()].slice(-100);
      options.setCursor(next); options.persist();
    },
    back() {
      if (paging || !previous.length) return;
      paging = true;
      const cursor = previous[previous.length - 1]; previous = previous.slice(0, -1);
      options.setCursor(cursor); options.persist();
    },
    first() {
      if (paging || options.cursor() === null) return;
      paging = true; previous = []; options.setCursor(null); options.persist();
    },
  };
}
