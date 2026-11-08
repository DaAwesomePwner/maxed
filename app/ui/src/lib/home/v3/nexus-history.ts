import type { Client } from '../../bridge/client';
import { createVisibilityPause } from '../../bridge/visibility';
import type { ScopeLootPage } from '../../bindings/ScopeLootPage';
import type { HistoryReply } from '../../bindings/HistoryReply';
import { createHomeHistory, type HomeHistoryInput, type HomeHistoryState } from '../summary';
import { historyControl, historyFailure, historyWaiting } from '../../runs/policy';

export interface LootHistoryState {
  phase: HomeHistoryState['phase']; value: ScopeLootPage | null; message: string; retryAvailable: boolean;
}
export interface NexusHistory { summary: HomeHistoryState; loot: LootHistoryState; page: number }
export function createNexusHistory(
  client: Pick<Client, 'queryHome' | 'queryScopeLoot' | 'queryScopeLootPage'>,
  receive: (state: NexusHistory) => void,
) {
  let input: HomeHistoryInput | null = null, disposed = false, revision = 0;
  const visibility = createVisibilityPause();
  let identity = '', key = '', page = 1, cursor: string | null = null;
  let requestedPage = 1, requestedCursor: string | null = null;
  let seen: HomeHistoryState['summary'] = null;
  let summary: HomeHistoryState = { phase: 'unavailable', summary: null, message: '', retryAvailable: false };
  const emptyLoot = (): LootHistoryState => ({ phase: 'unavailable', value: null, message: '', retryAvailable: false });
  let loot = emptyLoot();
  let queue: Promise<unknown> = Promise.resolve();
  function serial<T>(read: () => Promise<T>): Promise<T> {
    const result = queue.then(read); queue = result.catch(() => {}); return result;
  }
  async function readCurrent<T>(request: number, read: () => Promise<HistoryReply<T>>): Promise<HistoryReply<T>> {
    return serial(async () => {
      if (!await visibility.wait() || disposed || request !== revision) return { status: 'superseded' };
      return read();
    });
  }
  const emit = () => { if (!disposed) receive({ summary, loot, page }); };
  async function readLoot(nextCursor: string | null, nextPage: number) {
    if (disposed || !input?.scope || summary.phase !== 'ready') return;
    const scope = input.scope, request = ++revision;
    requestedCursor = nextCursor; requestedPage = nextPage;
    loot = { ...loot, phase: 'loading', message: '', retryAvailable: false }; emit();
    try {
      for (let attempt = 0; attempt < 2; attempt++) {
        const reply = await readCurrent<ScopeLootPage>(request, () => {
          return nextCursor === null ? client.queryScopeLoot(scope, 32) : client.queryScopeLootPage(scope, nextCursor, 32);
        });
        if (disposed || request !== revision) return;
        if (reply.status === 'ready') {
          cursor = nextCursor; page = nextPage;
          loot = { phase: 'ready', value: reply.value, message: '', retryAvailable: false }; emit(); return;
        }
        const policy = historyControl(reply.status, attempt > 0);
        loot = { ...loot, phase: 'busy', message: policy.message, retryAvailable: policy.retryAvailable }; emit();
        if (!policy.retry) return;
        await new Promise(resolve => setTimeout(resolve, 2000));
      }
    } catch (error) {
      if (disposed || request !== revision) return;
      loot = { ...loot, phase: 'error', message: historyFailure(error), retryAvailable: true }; emit();
    }
  }
  const history = createHomeHistory({ queryHome: scope => {
    const request = revision;
    return readCurrent(request, () => client.queryHome(scope));
  } }, state => {
    summary = state;
    if (state.phase === 'ready' && state.summary && state.summary !== seen) {
      seen = state.summary; void readLoot(null, 1);
    } else emit();
  });
  return {
    update(next: HomeHistoryInput) {
      if (disposed) return;
      const nextIdentity = JSON.stringify([next.streamId, next.scope, next.mode]);
      const nextKey = JSON.stringify([nextIdentity, next.required, next.initial === true, historyWaiting(next) !== null]);
      if (key !== nextKey) {
        revision++; seen = null;
        loot = identity !== nextIdentity ? emptyLoot() : { ...loot, phase: 'stale', retryAvailable: false };
        if (identity !== nextIdentity) { page = requestedPage = 1; cursor = requestedCursor = null; }
      }
      identity = nextIdentity; key = nextKey; input = next; history.update(next);
    },
    retry() { if (!disposed) { revision++; seen = null; history.retry(); } },
    retryLoot() { if (loot.retryAvailable) void readLoot(requestedCursor, requestedPage); },
    next() { if (loot.phase === 'ready' && loot.value?.next) void readLoot(loot.value.next, page + 1); },
    first() { if (loot.phase !== 'loading' && page > 1 && cursor !== null) void readLoot(null, 1); },
    dispose() { disposed = true; visibility.dispose(); revision++; history.dispose(); input = null; },
  };
}
