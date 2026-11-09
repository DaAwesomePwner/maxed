import type { DropView } from '../../bindings/DropView';

export interface WhiteBannerItem {
  /** Retain observation evidence for catalog updates after arrival. */
  drop?: DropView;
  dropId: string;
  itemType: number | null;
  itemName: string;
  bagType: number;
  areaName: string;
  /** Include the estimate label when the source is inferred. */
  sourceName: string | null;
  at: string;
  streak: { kind: 'ended'; runs: number } | { kind: 'first'; runs: number } | null;
}
export interface WhiteBannerState { current: WhiteBannerItem | null; overflow: number }

/** The area/scope owner seeds all present IDs on mount/change, without pushing.
 * Keep this set across reads so replayed drops cannot celebrate twice. */
export function newWhites(drops: DropView[], seen: Set<string>): DropView[] {
  return drops.filter(drop => {
    if (drop.bag_rank !== 255 || seen.has(drop.id)) return false;
    seen.add(drop.id);
    return true;
  });
}

/** Callers capture a positive streak from the previous loot read for the first
 * white pushed per scope: ended = runs_since_last_white (also record it in
 * EndedStreaks), first = runs_ended. Later whites, zero runs and realm = null.
 * Dispose and replace the queue when the area or scope changes. */
export function createWhiteQueue(onChange: (state: WhiteBannerState) => void,
  options: { durationMs?: number; maxQueued?: number } = {}): { push(item: WhiteBannerItem): void; dispose(): void } {
  const { durationMs = 6000, maxQueued = 3 } = options;
  const waiting: WhiteBannerItem[] = [];
  let current: WhiteBannerItem | null = null;
  let overflow = 0, pendingOverflow = 0, disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const publish = () => onChange({ current, overflow });

  function show(item: WhiteBannerItem) {
    current = item;
    // Attach the summary to the last waiting banner, once. Later arrivals
    // can queue behind it without repeating the already displayed count.
    overflow = waiting.length === 0 ? pendingOverflow : 0;
    if (waiting.length === 0) pendingOverflow = 0;
    timer = setTimeout(() => {
      timer = undefined;
      const next = waiting.shift();
      if (next) show(next);
      else { current = null; overflow = 0; publish(); }
    }, durationMs);
    publish();
  }

  return {
    push(item) {
      if (disposed) return;
      // Retain the push-time streak even if a later read changes the caller's value.
      const captured = { ...item, streak: item.streak ? { ...item.streak } : null };
      if (!current) show(captured);
      else if (waiting.length < maxQueued) waiting.push(captured);
      else if (waiting.length === 0) { overflow++; publish(); }
      else pendingOverflow++;
    },
    dispose() {
      disposed = true;
      clearTimeout(timer);
      timer = undefined;
      waiting.length = 0;
      current = null;
      overflow = pendingOverflow = 0;
    },
  };
}
