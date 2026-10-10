import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createRunHistory, type HistoryInput, type HistoryQuery, type HistoryState } from '../../src/lib/runs/history';
import demoSnapshot from '../fixtures/t0-live';

const disposals: Array<() => void> = [];
beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); vi.useRealTimers(); vi.restoreAllMocks(); });
const all = { range: null, character_id: null };
const sessions = (cursor: { started: string; id: string } | null): HistoryQuery =>
  ({ kind: 'stats-sessions', filter: { ...all, include_empty: false, cursor, limit: 50 } });
const cases: Array<{ query: HistoryQuery; method: 'getStatsMaps' | 'listStatsSessions' | 'getStatsSession' | 'getStatsDungeons'; kind: string }> = [
  { query: { kind: 'stats-maps', filter: all }, method: 'getStatsMaps', kind: 'stats-maps' },
  { query: sessions(null), method: 'listStatsSessions', kind: 'stats-sessions' },
  { query: { kind: 'stats-session', request: { id: 's1', character_id: null } }, method: 'getStatsSession', kind: 'stats-session' },
  { query: { kind: 'stats-dungeons', filter: all }, method: 'getStatsDungeons', kind: 'stats-dungeons' },
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

describe.each(cases)('stats history $method', ({ query, method, kind }) => {
  it('reads once and wraps the value with its kind', async () => {
    const h = harness(); const read = vi.spyOn(h.client, method);
    h.controller.update(h.input(query)); await flush();
    expect(read).toHaveBeenCalledOnce();
    expect(h.last().value?.kind).toBe(kind);
  });
});
it('treats a new session cursor as a new query and identical filters as the same one', async () => {
  const h = harness(); const read = vi.spyOn(h.client, 'listStatsSessions');
  h.controller.update(h.input(sessions(null))); await flush();
  h.controller.update(h.input({ kind: 'stats-sessions', filter: { limit: 50, cursor: null, include_empty: false, character_id: null, range: null } }));
  await vi.advanceTimersByTimeAsync(2000);
  expect(read).toHaveBeenCalledTimes(1);
  h.controller.update(h.input(sessions({ started: '5', id: 's5' }))); await vi.advanceTimersByTimeAsync(2000);
  expect(read).toHaveBeenCalledTimes(2);
});
