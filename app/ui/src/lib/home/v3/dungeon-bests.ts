import type { Client } from '../../bridge/client';
import type { DungeonBest } from '../../bindings/DungeonBest';
import type { EventEnvelope } from '../../bindings/EventEnvelope';
import type { LiveSnapshot } from '../../bindings/LiveSnapshot';
import type { HistoryReply } from '../../bindings/HistoryReply';
import type { StoreStatus } from '../../bindings/StoreStatus';
import { historyWaiting } from '../../runs/policy';

/** Home's history commands share the native scheduler's single pending slot. */
export function createHomeReads(owner: Client) {
  let disposed = false;
  let tail: Promise<unknown> = Promise.resolve();
  const latest = new Map<string, symbol>();
  function read<T>(kind: string, query: () => Promise<HistoryReply<T>>): Promise<HistoryReply<T>> {
    const token = Symbol(kind);
    latest.set(kind, token);
    const result = tail.then(async (): Promise<HistoryReply<T>> => {
      while (!disposed && latest.get(kind) === token) {
        const reply = await query();
        if (disposed || latest.get(kind) !== token) return { status: 'superseded' };
        if (reply.status !== 'superseded') return reply;
      }
      return { status: 'superseded' };
    });
    tail = result.catch(() => {});
    return result;
  }
  const client: Client = { ...owner,
    queryHome: scope => read('summary', () => owner.queryHome(scope)),
    queryScopeLoot: (scope, limit) => read('loot', () => owner.queryScopeLoot(scope, limit)),
    queryScopeLootPage: (scope, cursor, limit) => read('loot', () => owner.queryScopeLootPage(scope, cursor, limit)),
    queryDungeonBests: () => read('bests', () => owner.queryDungeonBests()),
  };
  return { client, dispose() { disposed = true; latest.clear(); } };
}

export function createDungeonBests(client: Pick<Client, 'queryDungeonBests'>,
  publish: (rows: DungeonBest[]) => void) {
  let disposed = false, running = false, pending = false;
  const completed = new Map<string, boolean>();
  let completionSequence = -1n;
  async function refresh() {
    if (disposed) return;
    pending = true;
    if (running) return;
    running = true;
    try {
      while (pending && !disposed) {
        pending = false;
        try {
          const reply = await client.queryDungeonBests();
          if (!disposed && reply.status === 'ready') publish(reply.value);
          else if (!disposed && reply.status === 'superseded') pending = true;
        } catch { /* Keep a saved baseline; an absent one stays PB —. No polling. */ }
      }
    } finally { running = false; }
  }
  // A completion is read back only once the store has saved through it; an
  // earlier read would miss the new clear and keep the old PB for the next run.
  let checkpoint: { stream: string; sequence: string } | null = null;
  let store: StoreStatus | null = null;
  let live: string | null = null;
  function flush() {
    if (!checkpoint) return;
    const saved = historyWaiting({ streamId: checkpoint.stream, required: checkpoint.sequence, store }) === null
      // The writer moved on to the current live stream, so the old one is fully
      // saved. A store status still describing an older stream proves nothing.
      || (store?.state === 'ready' && live !== checkpoint.stream && store.stream_id === live);
    if (!saved) return;
    checkpoint = null; void refresh();
  }
  void refresh();
  return {
    update(snapshot: LiveSnapshot | null, envelope: EventEnvelope | null, nextStore: StoreStatus | null) {
      if (disposed) return;
      store = nextStore;
      const nextLive = snapshot?.stream_id ?? envelope?.stream_id ?? live;
      if (nextLive !== live) {
        completed.clear(); completionSequence = -1n;
      }
      live = nextLive;
      const event = envelope?.event;
      const eventRun = event && 'run_ended' in event ? event.run_ended
        : event && 'run_updated' in event ? event.run_updated : null;
      for (const [stream, run, sequence] of [[snapshot?.stream_id, snapshot?.run, snapshot?.sequence],
        [envelope?.stream_id, eventRun, envelope?.sequence]] as const) {
        if (!stream || stream !== live || !run || !sequence || !/^\d+$/.test(sequence)
          || (run.completed_at === null && run.ended === null)) continue;
        const key = `${stream}:${run.id}`;
        const hasCompletion = run.completed_at !== null;
        // Leaving a run can precede its completion-counter evidence. Refresh
        // again when that ended run is subsequently confirmed complete.
        if ((completed.has(key) && (completed.get(key) || !hasCompletion)) || BigInt(sequence) < completionSequence) continue;
        completionSequence = BigInt(sequence);
        completed.set(key, hasCompletion);
        // Native can revise the current and immediately preceding run. Keep a
        // generous bounded horizon; older sequence replays remain rejected.
        if (completed.size > 512) completed.delete(completed.keys().next().value!);
        if (!checkpoint || checkpoint.stream !== stream || BigInt(sequence) > BigInt(checkpoint.sequence)) {
          checkpoint = { stream, sequence };
        }
      }
      flush();
    },
    dispose() { disposed = true; pending = false; },
  };
}

export type PbBaseline = { key: string; rows: DungeonBest[] | null };
/** The bests a run is judged against: the last read before it completed. A
 *  read after completion includes the run itself, so it only serves the next run. */
export function pbBaseline(previous: PbBaseline | null, snapshot: LiveSnapshot | null,
  rows: DungeonBest[] | null): PbBaseline {
  const run = snapshot?.run;
  const key = run ? `${snapshot.stream_id}:${run.id}` : '';
  const finished = !!run && (run.completed_at !== null || run.ended !== null);
  if (previous?.key === key && (previous.rows === rows || (finished && previous.rows !== null))) return previous;
  return { key, rows };
}
