import { cleanup, render, screen, fireEvent } from '@testing-library/svelte';
import Home from '../../src/routes/Home.svelte';
import { afterEach, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createHomeReads } from '../../src/lib/home/v3/dungeon-bests';
import { deferred, flush, homeSnapshot, homeSummary, saved } from './fixtures';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';

afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); });

it('Home summary retries back off, release the shared queue to HUD and recap, and stay bounded', async () => {
  vi.useFakeTimers();
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
  const client = createClient(createFixtureTransport(snapshot));
  const pending = deferred<HistoryReply<HomeSummary>>();
  const summary = vi.spyOn(client, 'queryHome').mockReturnValueOnce(pending.promise).mockResolvedValue({ status: 'superseded' });
  const hud = vi.spyOn(client, 'getHomeHud');
  const recap = vi.spyOn(client, 'getRunRecap').mockResolvedValue({ status: 'ready', value: null });
  const reads = createHomeReads(client), onRowsReady = vi.fn();
  const scope = { session: { id: 'synthetic-session' } };
  try {
    render(Home, { client: reads.client, snapshot, store: saved(), redesign: false, onRowsReady, entryKey: 'nexus' });
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
    expect(onRowsReady).toHaveBeenCalledWith('nexus');
    expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy();
    summary.mockResolvedValue({ status: 'ready', value: homeSummary() });
    await fireEvent.click(screen.getByRole('button', { name: 'Retry' })); await flush();
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull();
    expect(screen.getByRole('link', { name: /Synthetic Sanctum/ })).toBeTruthy();
  } finally { cleanup(); reads.dispose(); }
});

it('Nexus exposes Retry loot after repeated supersession and the rendered control recovers', async () => {
  vi.useFakeTimers();
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
  const client = createClient(createFixtureTransport(snapshot, undefined, homeSummary()));
  const loot = vi.spyOn(client, 'queryScopeLoot').mockResolvedValue({ status: 'superseded' });
  render(Home, { client, snapshot, store: saved(), redesign: false });
  await flush(); await vi.advanceTimersByTimeAsync(2000); await flush();
  expect(loot).toHaveBeenCalledTimes(2);
  const retry = screen.getByRole('button', { name: 'Retry loot' });
  loot.mockResolvedValue({ status: 'ready', value: { items: [], next: null, total_bags: 0 } });
  await fireEvent.click(retry); await flush();
  expect(loot).toHaveBeenCalledTimes(3);
  expect(screen.queryByRole('button', { name: 'Retry loot' })).toBeNull();
});
