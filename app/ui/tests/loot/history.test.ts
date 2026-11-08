import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createRunHistory, type HistoryInput, type HistoryQuery, type HistoryState } from '../../src/lib/runs/history';
import demoSnapshot from '../fixtures/t0-live';

const disposals: Array<() => void> = [];
beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });
const cases: Array<{ query: HistoryQuery; method: 'listDrops' | 'getItemHistory' | 'getLootStats' | 'getCollection'; field: string }> = [
  { query: { kind: 'drops', filter: { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 } }, method: 'listDrops', field: 'page' },
  { query: { kind: 'item-history', request: { item_type: 3180, cursor: null, limit: 50 } }, method: 'getItemHistory', field: 'history' },
  { query: { kind: 'loot-stats', filter: { range: null } }, method: 'getLootStats', field: 'stats' },
  { query: { kind: 'collection', request: { dungeon: null, chase: [] } }, method: 'getCollection', field: 'collection' },
];
function harness() {
  const client = createClient(createFixtureTransport(demoSnapshot));
  const states: HistoryState[] = [];
  const controller = createRunHistory(client, state => states.push(state));
  disposals.push(controller.dispose);
  const input = (query: HistoryQuery): HistoryInput => ({ query, scopeKey: 'live', streamId: null, required: null, store: null });
  return { client, controller, input, last: () => states.at(-1)! };
}
const flush = () => vi.advanceTimersByTimeAsync(0);

describe.each(cases)('loot history $method', ({ query, method, field }) => {
  it('reads once and wraps the ready value with its loot kind', async () => {
    const h = harness(); const read = vi.spyOn(h.client, method);
    h.controller.update(h.input(query)); await flush();
    expect(read).toHaveBeenCalledExactlyOnceWith('filter' in query ? query.filter : 'request' in query ? query.request : undefined);
    const reply = await read.mock.results[0].value;
    expect(reply.status).toBe('ready');
    if (reply.status === 'ready') expect(h.last()).toMatchObject({ phase: 'ready', value: { kind: query.kind, [field]: reply.value } });
  });
  it('offers manual retry after the bounded automatic busy retry', async () => {
    const h = harness(); const read = vi.spyOn(h.client, method).mockResolvedValue({ status: 'busy' });
    h.controller.update(h.input(query)); await flush(); await vi.advanceTimersByTimeAsync(1000);
    expect(read).toHaveBeenCalledTimes(2);
    expect(h.last()).toMatchObject({ phase: 'busy', retryAvailable: true, value: null });
    read.mockRestore(); h.controller.retry(); await flush();
    expect(h.last().phase).toBe('ready');
  });
  it('reports errors and recovers on retry', async () => {
    const h = harness(); const read = vi.spyOn(h.client, method).mockRejectedValue(new Error('Synthetic failure'));
    h.controller.update(h.input(query)); await flush();
    expect(read).toHaveBeenCalledOnce();
    expect(h.last()).toMatchObject({ phase: 'error', retryAvailable: true, value: null });
    read.mockRestore(); h.controller.retry(); await flush();
    expect(h.last().phase).toBe('ready');
  });
  it('refreshes when arguments change but ignores property order', async () => {
    const h = harness(); const read = vi.spyOn(h.client, method);
    h.controller.update(h.input(query)); await flush();
    const changed = structuredClone(query);
    if (changed.kind === 'drops') changed.filter.cursor = { at: '1', id: 'next' };
    if (changed.kind === 'item-history') changed.request.item_type = 7;
    if (changed.kind === 'loot-stats') changed.filter.range = { start: '1', end: '2' };
    if (changed.kind === 'collection') changed.request.chase = [{ item_type: 7, dungeon: null, pinned_at: '1' }];
    h.controller.update(h.input(changed)); await flush();
    expect(read).toHaveBeenCalledTimes(2);
    const reordered = structuredClone(changed);
    if (reordered.kind === 'drops') reordered.filter = Object.fromEntries(Object.entries(reordered.filter).reverse()) as typeof reordered.filter;
    if (reordered.kind === 'item-history') reordered.request = { limit: reordered.request.limit, cursor: reordered.request.cursor, item_type: reordered.request.item_type };
    if (reordered.kind === 'loot-stats') reordered.filter = { range: { end: '2', start: '1' } };
    if (reordered.kind === 'collection') reordered.request.chase = [{ pinned_at: '1', dungeon: null, item_type: 7 }];
    h.controller.update(h.input(reordered)); await vi.advanceTimersByTimeAsync(1000);
    expect(read).toHaveBeenCalledTimes(2);
  });
});
