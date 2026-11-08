import { afterEach, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createRunHistory, type HistoryQuery, type HistoryState } from '../../src/lib/runs/history';
import { homeSnapshot } from '../home/fixtures';
import { nativeQueue, flush } from '../runs/fixtures';

afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

it('Loot drops, header totals and selected run recover when another reader displaces totals', async () => {
  vi.useFakeTimers();
  const queue = nativeQueue(), client = createClient(createFixtureTransport(homeSnapshot()));
  const drops = vi.spyOn(client, 'listDrops').mockImplementation(() => queue.submit('drops'));
  const totals = vi.spyOn(client, 'getDropSummary').mockImplementation(() => queue.submit('totals'));
  const run = vi.spyOn(client, 'getRun').mockImplementation(() => queue.submit('run'));
  const filter = { min_rank: null, place: null, item_type: null, enchanted_only: false, range: null, cursor: null, limit: 50 };
  const queries: HistoryQuery[] = [
    { kind: 'drops', filter }, { kind: 'drop-summary', request: { filter, days: [] } }, { kind: 'run', id: 'synthetic-run' },
  ];
  const states: HistoryState[][] = queries.map(() => []);
  const readers = queries.map((query, i) => {
    const reader = createRunHistory(client, state => states[i].push(state));
    reader.update({ query, scopeKey: 'live', streamId: null, required: null, store: null });
    return reader;
  });
  try {
    await flush();
    expect(drops).toHaveBeenCalledOnce(); expect(totals).toHaveBeenCalledOnce(); expect(run).toHaveBeenCalledOnce();
    queue.finish({ rows: [], next: null }); await flush();
    queue.finish(null); await flush();
    await vi.advanceTimersByTimeAsync(999);
    expect(totals).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(1);
    expect(totals).toHaveBeenCalledTimes(2);
    queue.finish({ totals: {}, days: [] }); await flush();
    expect(states.map(history => history.at(-1)?.phase)).toEqual(['ready', 'ready', 'ready']);
    expect(states.flat().some(state => state.phase === 'error')).toBe(false);
    await vi.advanceTimersByTimeAsync(10000);
    expect(totals).toHaveBeenCalledTimes(2);
  } finally { readers.forEach(reader => reader.dispose()); }
});
