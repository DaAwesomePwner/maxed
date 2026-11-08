import { afterEach, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createHomeReads } from '../../src/lib/home/v3/dungeon-bests';
import { createHomeHistory, type HomeHistoryState } from '../../src/lib/home/summary';
import { deferred, flush, homeSnapshot, homeSummary, saved } from './fixtures';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';

afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

it('Home summary retries back off, release the shared queue to HUD and recap, and stay bounded', async () => {
  vi.useFakeTimers();
  const client = createClient(createFixtureTransport(homeSnapshot()));
  const pending = deferred<HistoryReply<HomeSummary>>();
  const summary = vi.spyOn(client, 'queryHome').mockReturnValueOnce(pending.promise).mockResolvedValue({ status: 'superseded' });
  const hud = vi.spyOn(client, 'getHomeHud');
  const recap = vi.spyOn(client, 'getRunRecap').mockResolvedValue({ status: 'ready', value: null });
  const reads = createHomeReads(client), states: HomeHistoryState[] = [];
  const history = createHomeHistory(reads.client, state => states.push(state));
  const scope = { session: { id: 'synthetic-session' } };
  try {
    history.update({ mode: 'nexus', scope, streamId: null, required: null, store: saved() });
    await flush();
    const hudRead = reads.client.getHomeHud(scope);
    const recapRead = reads.client.getRunRecap({ run_id: 'synthetic-run' });
    await flush();
    expect(summary).toHaveBeenCalledOnce();
    expect(hud).not.toHaveBeenCalled(); expect(recap).not.toHaveBeenCalled();
    pending.resolve({ status: 'superseded' }); await flush();
    await Promise.all([hudRead, recapRead]);
    expect(hud).toHaveBeenCalledOnce(); expect(recap).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(1999);
    expect(summary).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(1);
    expect(summary).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(10000);
    expect(summary).toHaveBeenCalledTimes(2);
    expect(states.at(-1)).toMatchObject({ retryAvailable: true });
    summary.mockResolvedValue({ status: 'ready', value: homeSummary() });
    history.retry(); await flush();
    expect(states.at(-1)?.phase).toBe('ready');
  } finally { history.dispose(); reads.dispose(); }
});
