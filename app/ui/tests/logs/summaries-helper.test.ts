import { afterEach, expect, it, vi } from 'vitest';
import { flushSync, tick } from 'svelte';
import { createLogsSummaries } from '../../src/lib/logs/summaries.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { EventSummary } from '../../src/lib/bindings/EventSummary';
import type { EventFilter } from '../../src/lib/bindings/EventFilter';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import fixture from '../fixtures/t0-live';
import { effectRoot, reactive } from './helper-state.svelte';

const disposals: Array<() => void> = [];
afterEach(() => disposals.splice(0).forEach(dispose => dispose()));
const summary: EventSummary = { total: 42, by_kind: { player_death: 2, player_left: 30, realm_event_killed: 7, portal_opened: 3 },
  dungeons: [], local_pops: 0, last_local_pop: null, flagged_since: null, first_at: '1' };
async function flush() { for (let i = 0; i < 12; i++) { await Promise.resolve(); await tick(); } }
function setup(range: EventFilter['range'] = { start: '1', end: '2' }) {
  const input = reactive({ ready: false, key: 'range-1', range });
  const client = createClient(createFixtureTransport(fixture));
  const replies: Array<{ resolve: (reply: HistoryReply<EventSummary>) => void; reject: (error: Error) => void }> = [];
  client.getEventSummary = vi.fn(() => new Promise<HistoryReply<EventSummary>>((resolve, reject) => { replies.push({ resolve, reject }); }));
  let summaries!: ReturnType<typeof createLogsSummaries>;
  const dispose = effectRoot(() => {
    summaries = createLogsSummaries({ reader: () => client, ready: () => input.ready, key: () => input.key,
      filter: () => ({ kinds: ['portal_opened'], run_id: 'synthetic-run', dungeon: null, cursor: null, limit: 50, range: input.range }),
      scopeKey: () => 'scope', streamId: () => null, store: () => null });
  });
  disposals.push(dispose); flushSync();
  return { input, client, replies, summaries, dispose };
}
it('waits for the page, reads range then all-time, and keeps all-time when the range refreshes', async () => {
  const { input, client, replies, summaries } = setup();
  await flush(); expect(client.getEventSummary).not.toHaveBeenCalled();
  input.ready = true; await flush(); expect(client.getEventSummary).toHaveBeenCalledOnce();
  expect(vi.mocked(client.getEventSummary).mock.calls[0][0].filter).toMatchObject({ range: { start: '1', end: '2' }, run_id: 'synthetic-run', query: '' });
  expect(summaries.settled).toBe(false);
  replies[0].resolve({ status: 'ready', value: summary }); await flush();
  expect(client.getEventSummary).toHaveBeenCalledTimes(2);
  expect(vi.mocked(client.getEventSummary).mock.calls[1][0].filter.range).toBeNull();
  expect(summaries.summary).toEqual(summary); expect(summaries.allTimeRead).toBe(false);
  replies[1].resolve({ status: 'ready', value: { ...summary, total: 100 } }); await flush();
  expect(summaries.settled).toBe(true); expect(summaries.allTime?.total).toBe(100);
  input.ready = false; input.key = 'range-2'; await flush(); expect(client.getEventSummary).toHaveBeenCalledTimes(2);
  input.ready = true; await flush(); replies[2].resolve({ status: 'ready', value: { ...summary, total: 50 } }); await flush();
  expect(client.getEventSummary).toHaveBeenCalledTimes(3); expect(summaries.summary?.total).toBe(50); expect(summaries.allTime?.total).toBe(100);
});
it('reuses an unbounded range summary for all-time without a second read', async () => {
  const { input, client, replies, summaries } = setup(null);
  input.ready = true; await flush(); replies[0].resolve({ status: 'ready', value: summary }); await flush();
  expect(client.getEventSummary).toHaveBeenCalledOnce(); expect(summaries.allTime).toEqual(summary); expect(summaries.settled).toBe(true);
});
it('allows all-time and downstream reads after a range error settles', async () => {
  const { input, replies, summaries } = setup();
  input.ready = true; await flush(); replies[0].reject(new Error('Synthetic failure')); await flush();
  expect(summaries.summary).toBeNull(); expect(replies).toHaveLength(2);
  replies[1].reject(new Error('Synthetic failure')); await flush();
  expect(summaries.allTime).toBeNull(); expect(summaries.settled).toBe(true);
});
it('discards an old range reply after the range changes and stops on disposal', async () => {
  const { input, replies, summaries, dispose } = setup();
  input.ready = true; await flush(); input.key = 'range-2'; input.range = { start: '3', end: '4' }; await flush();
  replies[0].resolve({ status: 'ready', value: summary }); await flush();
  expect(summaries.summary).toBeNull(); expect(summaries.summaryKey).toBeNull();
  dispose(); replies[1].resolve({ status: 'ready', value: summary }); await flush();
  expect(summaries.summary).toBeNull(); expect(replies).toHaveLength(2);
});
