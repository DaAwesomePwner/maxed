import type { LiveSnapshot } from '../../bindings/LiveSnapshot';

/** The same run id in another stream/session must not inherit a banner value. */
export function dungeonGeneration(snapshot: LiveSnapshot | null): string | null {
  return snapshot?.area_kind === 'dungeon' && snapshot.run
    ? JSON.stringify([snapshot.stream_id, snapshot.session?.id ?? null, snapshot.run.id]) : null;
}

export interface EndedStreaks {
  record(dungeon: string, runs: number): void;
  get(dungeon: string): number | null;
  reset(): void;
}

/** Owned by the session; reset when the session changes, not when Home unmounts. */
export function createEndedStreaks(): EndedStreaks {
  const ended = new Map<string, number>();
  return {
    record(dungeon, runs) { ended.set(dungeon, runs); },
    get(dungeon) { return ended.get(dungeon) ?? null; },
    reset() { ended.clear(); },
  };
}
