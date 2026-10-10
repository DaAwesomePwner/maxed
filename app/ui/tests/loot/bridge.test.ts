import { describe, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport, type LootFixtureQueries } from '../../src/lib/bridge/fixture';
import type { DropFilter } from '../../src/lib/bindings/DropFilter';
import demoSnapshot from '../fixtures/t0-live';
import { createRunHistory, type HistoryInput, type HistoryState } from '../../src/lib/runs/history';

const filter: DropFilter = { min_rank: 255, place: { kind: 'dungeon', name: 'Synthetic' }, item_type: 3180,
  enchanted_only: true, range: { start: '1', end: '9' }, cursor: { at: '5', id: 'bag' }, limit: 10 };
const page = { rows: [], next: { at: '2', id: 'next' } };
const item = { item_type: 3180, totals: { appearances: 0, bags: 0, taken: 0, first_at: null, last_at: null },
  enchant_variants: [], by_dungeon: [], by_source: [], bags: page };
const stats = { dungeons: [], sources: [], totals: [], whites: 3 };
const collection = { chase: [], grid_dungeon: 'Synthetic', grid: [] };
const request = { item_type: 3180, cursor: filter.cursor, limit: 10 };
const collectionRequest = { dungeon: 'Synthetic', chase: [{ item_type: 3180, dungeon: null, pinned_at: '1' }] };
function fixture(loot?: LootFixtureQueries, strict = false) {
  return createFixtureTransport(demoSnapshot, undefined, undefined, undefined, undefined, undefined, undefined,
    { requireHistoryFixtures: strict }, loot);
}

describe('loot bridge', () => {
  it('routes every query with native argument keys and isolates arguments and results', async () => {
    const queries = { listDrops: vi.fn(() => page), itemHistory: vi.fn(() => item),
      lootStats: vi.fn(() => stats), collection: vi.fn(() => collection),
      searchItems: vi.fn(() => [{ object_type: 3180, name: 'Synthetic item', tier: 'UT' }]) };
    const transport = fixture(queries);
    const invoke = vi.spyOn(transport, 'invoke');
    const client = createClient(transport);
    const drops = await client.listDrops(filter);
    expect(drops).toEqual({ status: 'ready', value: page });
    await expect(client.getItemHistory(request)).resolves.toEqual({ status: 'ready', value: item });
    await expect(client.getLootStats({ range: filter.range })).resolves.toEqual({ status: 'ready', value: stats });
    await expect(client.getCollection(collectionRequest)).resolves.toEqual({ status: 'ready', value: collection });
    const matches = await client.searchItems('Synthetic', 10);
    expect(matches).toEqual(queries.searchItems.mock.results[0].value);
    expect(invoke.mock.calls).toEqual([
      ['list_drops', { filter }], ['get_item_history', { request }],
      ['get_loot_stats', { filter: { range: filter.range } }],
      ['get_collection', { request: collectionRequest }], ['search_items', { query: 'Synthetic', limit: 10 }],
    ]);
    const expected = [[filter], [request], [{ range: filter.range }], [collectionRequest], ['Synthetic', 10]];
    Object.values(queries).forEach((query, index) => {
      expect(query.mock.calls).toEqual([expected[index]]);
    });
    if (drops.status === 'ready') { drops.value.next!.id = 'changed'; }
    expect(page.next.id).toBe('next');
    matches[0].name = 'changed';
    expect(queries.searchItems.mock.results[0].value[0].name).toBe('Synthetic item');
  });

  it('clones nested request arguments before handing them to callbacks', async () => {
    const original = structuredClone(filter);
    const client = createClient(fixture({ listDrops: input => {
      input.range!.start = '99'; input.cursor!.id = 'changed'; return page;
    }, collection: input => { input.chase[0].pinned_at = '99'; return collection; } }));
    await client.listDrops(original);
    await client.getCollection(structuredClone(collectionRequest));
    expect(original).toEqual(filter);
    expect(collectionRequest.chase[0].pinned_at).toBe('1');
  });

  it('round-trips chase settings in memory with isolated inputs, outputs and transports', async () => {
    const transport = fixture(); const invoke = vi.spyOn(transport, 'invoke');
    const client = createClient(transport); const before = await client.querySettings();
    const items = structuredClone(collectionRequest.chase);
    await expect(client.setChaseList(items)).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith('set_chase_list', { items });
    items[0].pinned_at = '99';
    const after = await client.querySettings();
    expect(after).toEqual({ ...before, loot: { chase: collectionRequest.chase } });
    after.loot.chase.length = 0;
    expect((await client.querySettings()).loot.chase).toEqual(collectionRequest.chase);
    expect((await createClient(fixture()).querySettings()).loot.chase).toEqual([]);
    await client.setChaseList([]);
    expect((await client.querySettings()).loot.chase).toEqual([]);
  });

  it('provides empty defaults but rejects missing queries in strict previews', async () => {
    const client = createClient(fixture());
    await expect(client.listDrops(filter)).resolves.toEqual({ status: 'ready', value: { rows: [], next: null } });
    await expect(client.getItemHistory(request)).resolves.toMatchObject({ status: 'ready', value: { item_type: 3180, totals: { appearances: 0 } } });
    await expect(client.getLootStats({ range: null })).resolves.toEqual({ status: 'ready', value: { dungeons: [], sources: [], totals: [], whites: 0 } });
    await expect(client.getCollection(collectionRequest)).resolves.toEqual({ status: 'ready', value: {
      chase: collectionRequest.chase.map(item => ({ item, seen: 0, last_at: null, dry_runs: 0 })), grid_dungeon: 'Synthetic', grid: [],
    } });
    await expect(client.searchItems('Synthetic', 10)).resolves.toEqual([]);
    const strict = createClient(fixture({}, true));
    for (const read of [() => strict.listDrops(filter), () => strict.getItemHistory(request),
      () => strict.getLootStats({ range: null }), () => strict.getCollection(collectionRequest),
      () => strict.searchItems('Synthetic', 10)]) await expect(read()).rejects.toThrow('not supplied');
  });

  it('propagates callback failures instead of substituting empty data', async () => {
    const fail = () => { throw new Error('Synthetic failure'); };
    const client = createClient(fixture({ listDrops: fail, itemHistory: fail, lootStats: fail, collection: fail, searchItems: fail }));
    for (const read of [() => client.listDrops(filter), () => client.getItemHistory(request),
      () => client.getLootStats({ range: null }), () => client.getCollection(collectionRequest),
      () => client.searchItems('Synthetic', 10)]) await expect(read()).rejects.toThrow('Synthetic failure');
  });

  it.each([
    ['over 100 items', Array.from({ length: 101 }, (_, item_type) => ({ item_type, dungeon: null, pinned_at: '1' }))],
    ['duplicate global pair', [{ item_type: 7, dungeon: null, pinned_at: '1' }, { item_type: 7, dungeon: null, pinned_at: '2' }]],
    ['duplicate dungeon pair', [{ item_type: 7, dungeon: 'Synthetic', pinned_at: '1' }, { item_type: 7, dungeon: 'Synthetic', pinned_at: '2' }]],
  ] as const)('rejects %s without changing settings', async (_name, items) => {
    const client = createClient(fixture());
    await client.setChaseList(collectionRequest.chase);
    const before = await client.querySettings();
    await expect(client.setChaseList([...items])).rejects.toThrow('Local settings could not be read or saved. Restore a supported settings file and retry.');
    expect(await client.querySettings()).toEqual(before);
  });

  it('accepts 100 distinct pairs including one type in multiple dungeons', async () => {
    const client = createClient(fixture());
    const items = Array.from({ length: 100 }, (_, index) => ({ item_type: 7, dungeon: index === 0 ? null : `Synthetic ${index}`, pinned_at: '1' }));
    await client.setChaseList(items);
    expect((await client.querySettings()).loot.chase).toEqual(items);
  });

  it.each(['', 'a'.repeat(257), 'é'.repeat(129), 'Synthetic\u0000', 'Synthetic\n', 'Synthetic\u007f', 'Synthetic\u0085'])('rejects invalid chase dungeon %j without changing settings', async dungeon => {
    const client = createClient(fixture());
    await client.setChaseList(collectionRequest.chase);
    const before = await client.querySettings();
    await expect(client.setChaseList([{ item_type: 7, dungeon, pinned_at: '1' }])).rejects.toThrow('Local settings could not be read or saved. Restore a supported settings file and retry.');
    expect(await client.querySettings()).toEqual(before);
  });

  it.each(['', 'no', '-1', '+1', '1.5', '1e2', ' 1', '1\n', '9223372036854775808'])('rejects invalid chase timestamp %j without changing settings', async pinned_at => {
    const client = createClient(fixture());
    await client.setChaseList(collectionRequest.chase);
    const before = await client.querySettings();
    await expect(client.setChaseList([{ item_type: 7, dungeon: null, pinned_at }])).rejects.toThrow('Local settings could not be read or saved. Restore a supported settings file and retry.');
    expect(await client.querySettings()).toEqual(before);
  });

  it('accepts chase dungeon byte and timestamp boundaries', async () => {
    const client = createClient(fixture());
    const items = [{ item_type: 7, dungeon: 'é'.repeat(128), pinned_at: '9223372036854775807' },
      { item_type: 8, dungeon: null, pinned_at: '0' }];
    await client.setChaseList(items);
    expect((await client.querySettings()).loot.chase).toEqual(items);
  });

  it('isolates fallback collection chase rows from the request', async () => {
    const client = createClient(fixture());
    const input = structuredClone(collectionRequest);
    const reply = await client.getCollection(input);
    if (reply.status !== 'ready') throw new Error('Expected ready collection');
    expect(reply.value.chase).toHaveLength(1);
    reply.value.chase[0].item.pinned_at = '99';
    expect(input).toEqual(collectionRequest);
  });
});


describe('drop summary bridge', () => {
  const summaryRequest = { filter, days: [{ start: '1', end: '9' }, { start: '9', end: '20' }] };
  const totals = { bags: 3, runs: 2, runs_ended: 4, whites: 1, ut_items: 2, st_items: 1,
    enchanted_items: 1, items_counted: true, best_enchants: 2, first_at: '2',
    last_white: { drop_id: 'white', run_id: 'run', dungeon: 'Synthetic', at: '3' } };
  it('uses get_drop_summary and { request }, isolating callback arguments and results', async () => {
    const getDropSummary = vi.fn((input: import('../../src/lib/bindings/DropSummaryRequest').DropSummaryRequest) => {
      input.days[0].start = '7'; input.filter.cursor!.id = 'changed';
      return { totals, days: [totals, totals] };
    });
    const transport = fixture({ getDropSummary });
    const invoke = vi.spyOn(transport, 'invoke');
    const original = structuredClone(summaryRequest);
    const result = await createClient(transport).getDropSummary(original);
    expect(invoke).toHaveBeenCalledWith('get_drop_summary', { request: summaryRequest });
    expect(original).toEqual(summaryRequest);
    expect(result).toEqual({ status: 'ready', value: { totals, days: [totals, totals] } });
    if (result.status !== 'ready') throw new Error('Expected ready summary');
    result.value.totals.last_white!.drop_id = 'changed';
    expect(totals.last_white.drop_id).toBe('white');
  });
  it('refreshes when include_items changes and treats absence as true', async () => {
    vi.useFakeTimers();
    const client = createClient(fixture());
    const read = vi.spyOn(client, 'getDropSummary');
    const states: HistoryState[] = [];
    const controller = createRunHistory(client, state => states.push(state));
    const input = (request: typeof summaryRequest & { include_items?: boolean }): HistoryInput => ({
      query: { kind: 'drop-summary', request }, scopeKey: 'live', streamId: null, required: null, store: null,
    });
    try {
      controller.update(input({ ...summaryRequest, include_items: false }));
      await vi.advanceTimersByTimeAsync(1000);
      expect(states.at(-1)).toMatchObject({ phase: 'ready', value: { kind: 'drop-summary', summary: { totals: { items_counted: false } } } });
      controller.update(input(summaryRequest));
      await vi.advanceTimersByTimeAsync(1000);
      expect(read).toHaveBeenCalledTimes(2);
      expect(states.at(-1)).toMatchObject({ phase: 'ready', value: { kind: 'drop-summary', summary: { totals: { items_counted: true } } } });
      controller.update(input({ ...summaryRequest, include_items: true }));
      await vi.advanceTimersByTimeAsync(1000);
      expect(read).toHaveBeenCalledTimes(2);
    } finally {
      controller.dispose(); read.mockRestore(); vi.clearAllTimers(); vi.useRealTimers();
    }
  });
  it('provides ordered empty days and preserves the outside denominator', async () => {
    const client = createClient(fixture());
    const result = await client.getDropSummary(summaryRequest);
    expect(result).toMatchObject({ status: 'ready', value: {
      totals: { bags: 0, runs_ended: 0, first_at: null, last_white: null },
      days: [{ bags: 0, runs_ended: 0 }, { bags: 0, runs_ended: 0 }],
    } });
    await expect(client.getDropSummary({ ...summaryRequest, filter: { ...filter, place: { kind: 'outside' } } }))
      .resolves.toMatchObject({ status: 'ready', value: { totals: { runs_ended: null }, days: [{ runs_ended: null }, { runs_ended: null }] } });
    await expect(client.getDropSummary({ ...summaryRequest, include_items: false })).resolves.toMatchObject({
      status: 'ready', value: {
        totals: { items_counted: false, ut_items: null, st_items: null, enchanted_items: null, best_enchants: null },
        days: [{ items_counted: false, ut_items: null }, { items_counted: false, ut_items: null }],
      },
    });
    if (result.status !== 'ready') throw new Error('Expected ready summary');
    expect(result.value.totals).toMatchObject({ items_counted: true, ut_items: 0, st_items: 0, enchanted_items: 0 });
    await expect(createClient(fixture({}, true)).getDropSummary(summaryRequest)).rejects.toThrow('not supplied');
    await expect(createClient(fixture({ getDropSummary: () => { throw new Error('Synthetic failure'); } }))
      .getDropSummary(summaryRequest)).rejects.toThrow('Synthetic failure');
  });
});
