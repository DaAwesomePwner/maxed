import { cleanup, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Client } from '../../src/lib/bridge/client';
import { fight, follow, homeSnapshot, microtasks, mountPhase2, primary, ready, saved } from '../phase2/fixtures';
import { homeSummary } from '../home/fixtures';

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

const fame = { fame_changed: { at: '10000000', character_id: homeSnapshot().character!.id, fame: 1300 } };
const stats = () => vi.fn<Client['getStatsMaps']>(async () => ready({ rows: [], truncated: false, tracking_since: null, characters: [] }));
async function advance(ms: number) { await vi.advanceTimersByTimeAsync(ms); await microtasks(); }

it('stats_ignores_fame_and_fight_events_while_open', async () => {
  const getStatsMaps = stats();
  const page = mountPhase2({ hash: '#/stats', configure(client) { client.getStatsMaps = getStatsMaps; } });
  await microtasks();
  expect(getStatsMaps).toHaveBeenCalledOnce();
  page.event(fame, '13');
  page.event({ fight_ended: fight() }, '14');
  page.setStore(saved('14', page.snapshot.stream_id));
  await advance(2000);
  expect(getStatsMaps).toHaveBeenCalledOnce();
  page.event({ map_entered: { at: '10000000', name: 'Nexus', kind: 'nexus' } }, '15');
  page.setStore(saved('15', page.snapshot.stream_id));
  await advance(1000);
  expect(getStatsMaps).toHaveBeenCalledTimes(2);
});

it('loot_ignores_fight_events_but_refreshes_on_bags', async () => {
  const listDrops = vi.fn<Client['listDrops']>(async () => ready({ rows: [], next: null }));
  const page = mountPhase2({ hash: '#/loot', configure(client) {
    client.listDrops = listDrops;
    client.getDropSummary = vi.fn(async () => ready({ totals: { bags: 0, runs: 0, runs_ended: 0, whites: 0,
      best_enchants: null, last_white: null, first_at: null }, days: [] }));
  } });
  await microtasks();
  expect(listDrops).toHaveBeenCalledOnce();
  page.event({ fight_ended: fight() }, '13');
  page.setStore(saved('13', page.snapshot.stream_id));
  await advance(2000);
  expect(listDrops).toHaveBeenCalledOnce();
  page.event({ bag_dropped: homeSnapshot().drops[0] }, '14');
  page.setStore(saved('14', page.snapshot.stream_id));
  await advance(1000);
  expect(listDrops).toHaveBeenCalledTimes(2);
});

it('navigation_gates_on_events_seen_elsewhere', async () => {
  const getStatsMaps = stats();
  const page = mountPhase2({ hash: '#/settings', configure(client) { client.getStatsMaps = getStatsMaps; } });
  await microtasks();
  page.event(fame, '13');
  await microtasks();
  await follow('#/stats', primary());
  expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toContain('Refreshing map stats');
  expect(getStatsMaps).not.toHaveBeenCalled();
  await advance(2000);
  expect(getStatsMaps).not.toHaveBeenCalled();
  page.setStore(saved('13', page.snapshot.stream_id));
  await advance(1000);
  expect(getStatsMaps).toHaveBeenCalledOnce();
  await advance(2000);
  expect(getStatsMaps).toHaveBeenCalledOnce();
});

it('home_still_refreshes_on_fame', async () => {
  const queryHome = vi.fn<Client['queryHome']>().mockResolvedValueOnce(ready(homeSummary({ runs: 0 })))
    .mockResolvedValue(ready(homeSummary({ runs: 2 })));
  const page = mountPhase2({ hash: '#/', configure(client) { client.queryHome = queryHome; } });
  await microtasks();
  expect(queryHome).toHaveBeenCalledOnce();
  page.event(fame, '13');
  page.setStore(saved('13', page.snapshot.stream_id));
  await advance(1000);
  expect(queryHome).toHaveBeenCalledTimes(2);
  expect(screen.getByText('Runs', { selector: 'dt' }).parentElement?.querySelector('.value')?.textContent).toBe('2');
  await advance(2000);
  expect(queryHome).toHaveBeenCalledTimes(2);
});
