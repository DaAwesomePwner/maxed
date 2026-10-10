import { cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Client } from '../../src/lib/bridge/client';
import type { DropPage } from '../../src/lib/bindings/DropPage';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { advance, deferred, flush, follow, main, mountRuns, ready } from './fixtures';
import { homeSnapshot, mountPhase2, primary, saved } from '../phase2/fixtures';
import { statsMapRows } from '../shell/fixtures/stats';
import { logsQueries } from '../shell/fixtures/logs';

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

const drops = (id = 'cached-bag'): DropPage => ({ rows: [{ dungeon: null,
  drop: { ...homeSnapshot().drops[0], id, run_id: null } }], next: null });
function loot() {
  const listDrops = vi.fn<Client['listDrops']>(async () => ready(drops()));
  const getDropSummary = vi.fn<Client['getDropSummary']>(async () => ready({
    totals: { bags: 1, runs: 0, runs_ended: 0, whites: 0, best_enchants: null, last_white: null, first_at: null }, days: [],
  }));
  const page = mountPhase2({ hash: '#/settings', configure(client) { Object.assign(client, { listDrops, getDropSummary }); } });
  return { ...page, listDrops, getDropSummary };
}
const bagRow = (id = 'cached-bag') => main().getByRole('table', { name: 'Bags' }).querySelector(`[data-row-id="drop:${id}"]`);

it('runs_revisit_renders_rows_without_second_read', async () => {
  const page = mountRuns({ hash: '#/settings' }); await flush(); await follow('#/runs', primary());
  expect(main().getByRole('link', { name: /Invented Sanctum/ })).toBeTruthy();
  await follow('#/settings', primary()); await follow('#/runs', primary());
  expect(main().getByRole('link', { name: /Invented Sanctum/ })).toBeTruthy();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(1);
});

it('loot_feed_revisit_renders_without_second_read', async () => {
  const page = loot(); await flush(); await follow('#/loot', primary());
  expect(bagRow()).toBeTruthy();
  expect(page.listDrops).toHaveBeenCalledTimes(1);
  expect(page.getDropSummary).toHaveBeenCalledTimes(2);
  expect(page.getDropSummary.mock.calls.map(([request]) => request.include_items)).toEqual([false, true]);
  await follow('#/settings', primary()); await follow('#/loot', primary());
  expect(bagRow()).toBeTruthy();
  expect(page.listDrops).toHaveBeenCalledTimes(1);
  expect(page.getDropSummary).toHaveBeenCalledTimes(2);
});

it('stats_maps_revisit_renders_without_second_read', async () => {
  const getStatsMaps = vi.fn<Client['getStatsMaps']>(async () => ready({ rows: statsMapRows, truncated: false, tracking_since: null, characters: [] }));
  mountPhase2({ hash: '#/settings', configure(client) { client.getStatsMaps = getStatsMaps; } }); await flush(); await follow('#/stats', primary());
  expect(main().getByRole('rowheader', { name: 'Lost Halls' })).toBeTruthy();
  await follow('#/settings', primary()); await follow('#/stats', primary());
  expect(main().getByRole('rowheader', { name: 'Lost Halls' })).toBeTruthy();
  expect(getStatsMaps).toHaveBeenCalledTimes(1);
});

it('logs_chat_revisit_renders_without_second_read', async () => {
  const listChat = vi.fn<Client['listChat']>(async filter => ready(logsQueries.listChat!(filter)));
  mountPhase2({ hash: '#/settings', configure(client) { client.listChat = listChat; } }); await flush(); await follow('#/logs', primary());
  expect(main().getByText('synthetic guild run ready')).toBeTruthy();
  await follow('#/settings', primary()); await follow('#/logs', primary());
  expect(main().getByText('synthetic guild run ready')).toBeTruthy();
  expect(listChat).toHaveBeenCalledTimes(1);
});

it('revisit_after_matching_event_refreshes_once', async () => {
  const page = loot(); await flush(); await follow('#/loot', primary());
  const pending = deferred<HistoryReply<DropPage>>();
  page.listDrops.mockImplementationOnce(() => pending.promise);
  await follow('#/settings', primary());
  const sequence = String(BigInt(page.snapshot.sequence) + 1n);
  page.setStore(saved(sequence, page.snapshot.stream_id));
  page.event({ bag_dropped: drops('new-bag').rows[0].drop }, sequence);
  page.publish({ ...page.snapshot, sequence }); await advance();
  await follow('#/loot', primary());
  expect(bagRow()).toBeTruthy();
  expect(page.listDrops).toHaveBeenCalledTimes(2);
  pending.resolve(ready(drops('new-bag'))); await flush();
  expect(bagRow('new-bag')).toBeTruthy();
  expect(bagRow()).toBeNull();
  expect(page.listDrops).toHaveBeenCalledTimes(2);
});

it('back_forward_restores_from_cache', async () => {
  const page = mountRuns({ hash: '#/settings' }); await flush(); await follow('#/runs', primary());
  await follow('#/runs/run-1');
  page.browser.adapter.go(-1); await flush();
  expect(main().getByRole('link', { name: /Invented Sanctum/ })).toBeTruthy();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(1);
  page.browser.adapter.go(1); await flush();
  expect(main().getByRole('heading', { level: 1, name: 'Run' })).toBeTruthy();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
});
