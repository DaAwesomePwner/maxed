import { setDebugMode } from '../../src/lib/debug/debug';
import { render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import DungeonHome from '../../src/lib/home/DungeonHome.svelte';
import Shell from '../../src/lib/shell/Shell.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createHomeObservation, RECOVERY_GRACE_MS } from '../../src/lib/home/mode';
import { railView } from '../../src/lib/shell/rail';
import { deferred, homeSnapshot } from './fixtures';
import { flushHomeRender, mountHome } from './shell-fixture';
import { fakeHistory } from '../navigation/fake-history';

beforeEach(() => { setDebugMode(true); vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { setDebugMode(false); vi.useRealTimers(); vi.restoreAllMocks(); });

it('verified character B cannot claim character A\'s still-local fight', async () => {
  const snapshot = homeSnapshot();
  const page = render(DungeonHome, { snapshot, stale: false });
  await flushHomeRender();
  expect(screen.getByRole('rowheader', { name: /\bYou$/ })).toBeTruthy();
  expect(railView(snapshot)).toMatchObject({ dps: 150, rank: 1 });
  const swapped = structuredClone(snapshot);
  swapped.character!.id = 'verified-character-B';
  await page.rerender({ snapshot: swapped, stale: false }); await flushHomeRender();
  expect(swapped.fights[0].players[0].is_local).toBe(true);
  expect(railView(swapped)).toMatchObject({ dps: null, rank: null });
  expect(page.container.querySelector('.damage-row.self')).toBeNull();
  expect(screen.queryByRole('rowheader', { name: /\bYou$/ })).toBeNull();
  expect(screen.getByText('Your DPS and rank are unavailable: your identity or a unique local player is unavailable.')).toBeTruthy();
  const observedRow = page.container.querySelector('.damage-row')! as HTMLElement;
  expect(observedRow.querySelector('td:nth-child(5)')?.textContent).toBe('150');
  expect(within(observedRow).getByRole('cell', { name: '1' })).toBeTruthy();
  expect(page.container.querySelector('.fight-meta .gold')?.textContent).toContain('best hit \u2014');
  expect(page.container.querySelector('.fight-meta .gold')?.textContent).toContain('\u2014 hits');
});

it.each(['Recovering', 'WaitingForGame'] as const)('rail freezes and labels %s observations, and capture Live alone cannot unpause them', async state => {
  const snapshot = homeSnapshot(); const page = mountHome(snapshot, undefined, '#/runs');
  await flushHomeRender();
  const rail = screen.getByRole('complementary', { name: 'Character' });
  const dps = () => rail.querySelector('.fight-stats div:nth-child(2) dd')!.textContent;
  expect(dps()).toBe('150');
  expect(within(rail).queryByText('Last seen')).toBeNull();
  page.transport.publishCapture({ ...snapshot.capture, state });
  await flushHomeRender();
  expect(within(rail).getByText('Paused · last observation')).toBeTruthy();
  expect(within(rail).getByText('Last seen')).toBeTruthy();
  const newer = structuredClone(snapshot); newer.sequence = '13'; newer.at = '9000000';
  newer.fights[0].players[0].dps = 999;
  page.transport.publishSnapshot(newer);
  await vi.advanceTimersByTimeAsync(RECOVERY_GRACE_MS + 1); await flushHomeRender();
  expect(dps()).toBe('150');
  expect(rail.querySelector('.fight-stats.stale')).toBeTruthy();
  expect(rail.querySelector('.target.stale')).toBeTruthy();
  page.transport.publishCapture(snapshot.capture); await flushHomeRender();
  expect(dps()).toBe('150');
  expect(within(rail).getByText('Paused · last observation')).toBeTruthy();
  page.transport.publishSnapshot({ ...newer, sequence: '14' }); await flushHomeRender();
  expect(dps()).toBe('999');
  expect(within(rail).queryByText('Paused · last observation')).toBeNull();
  expect(within(rail).queryByText('Last seen')).toBeNull();
});

it.each(['Stopped', 'Failed'] as const)('%s capture alone does not freeze accepted rail updates', state => {
  const observe = createHomeObservation();
  const snapshot = homeSnapshot();
  expect(observe(snapshot, snapshot.capture, null, 0)).toMatchObject({ paused: false, retained: snapshot });
  const next = structuredClone(snapshot);
  next.sequence = '13'; next.fights[0].players[0].dps = 999;
  const result = observe(next, { ...snapshot.capture, state }, null, 100);
  expect(result).toMatchObject({ paused: false, retained: next });
  expect(railView(result.retained).dps).toBe(999);
});

it('resync and unknown-area uncertainty share Home\'s exact grace deadline', () => {
  for (const resync of [false, true]) {
    const observe = createHomeObservation();
    const snapshot = homeSnapshot();
    const live = { phase: 'live' as const, historyStale: false, historyFailed: false, historyLagging: false, message: '' };
    expect(observe(snapshot, snapshot.capture, live, 0)).toMatchObject({ paused: false, mode: { mode: 'dungeon' } });
    const uncertain = resync ? snapshot : { ...snapshot, area_kind: 'unknown' as const };
    const state = resync ? { ...live, phase: 'resyncing' as const } : live;
    expect(observe(uncertain, snapshot.capture, state, 100)).toMatchObject({ paused: true, delay: RECOVERY_GRACE_MS });
    expect(observe(uncertain, snapshot.capture, state, 1599)).toMatchObject({ paused: true, mode: { mode: 'dungeon' } });
    expect(observe(uncertain, snapshot.capture, state, 1600)).toMatchObject({ paused: true, retained: snapshot, mode: { mode: 'nexus' } });
    expect(observe({ ...snapshot }, snapshot.capture, live, 1601)).toMatchObject({ paused: false, mode: { mode: 'dungeon' } });
  }
});

it.each(['Recovering', 'WaitingForGame'] as const)('realm server switching freezes the rail during %s until a fresh Live snapshot', async state => {
  const snapshot = homeSnapshot({ area_kind: 'realm', map: 'Synthetic Realm', run: null });
  snapshot.fights[0].run_id = null;
  const page = mountHome(snapshot, undefined, '#/runs'); await flushHomeRender();
  const rail = screen.getByRole('complementary', { name: 'Character' });
  const values = () => [...rail.querySelectorAll('.fight-stats dd')].map(node => node.textContent);
  expect(values().slice(1)).toEqual(['150', '#1']);
  expect(within(rail).queryByText('Last seen')).toBeNull();
  const target = rail.querySelector('.target')!.textContent;
  page.transport.publishCapture({ ...snapshot.capture, state }); await flushHomeRender();
  expect(within(rail).getByText('Paused · last observation')).toBeTruthy();
  expect(within(rail).getByText('Last seen')).toBeTruthy();
  const newer = structuredClone(snapshot); newer.sequence = '13';
  newer.fights[0].players[0].dps = 999; newer.fights[0].players[0].rank = 2;
  page.transport.publishSnapshot(newer);
  await vi.advanceTimersByTimeAsync(RECOVERY_GRACE_MS + 1); await flushHomeRender();
  expect(values().slice(1)).toEqual(['150', '#1']);
  expect(rail.querySelector('.fight-stats.stale')).toBeTruthy();
  expect(rail.querySelector('.target.stale')?.textContent).toBe(target);
  page.transport.publishCapture(snapshot.capture); await flushHomeRender();
  expect(within(rail).getByText('Paused · last observation')).toBeTruthy();
  expect(values().slice(1)).toEqual(['150', '#1']);
  page.transport.publishSnapshot({ ...newer, sequence: '14' }); await flushHomeRender();
  expect(values().slice(1)).toEqual(['999', '#2']);
  expect(within(rail).queryByText('Paused · last observation')).toBeNull();
  expect(within(rail).queryByText('Last seen')).toBeNull();
});

it.each(['realm', 'nexus', 'other_hub', 'unknown'] as const)('rail recovery applies in %s without changing Home layout rules', area_kind => {
  const snapshot = homeSnapshot({ area_kind, run: null });
  const home = createHomeObservation(), rail = createHomeObservation({ pauseOnCaptureRecovery: true });
  expect(home(snapshot, snapshot.capture, null, 0).mode.mode).toBe(area_kind === 'realm' ? 'realm' : 'nexus');
  expect(rail(snapshot, snapshot.capture, null, 0).paused).toBe(false);
  const capture = { ...snapshot.capture, state: 'Recovering' as const };
  expect(home(snapshot, capture, null, 100)).toMatchObject({ mode: { mode: area_kind === 'realm' ? 'realm' : 'nexus' } });
  expect(rail(snapshot, capture, null, 100)).toMatchObject({ paused: true, retained: snapshot });
});

it.each(['connecting', 'live', 'resyncing', 'failed'] as const)('no snapshot has no last-observation claim during %s', phase => {
  const observe = createHomeObservation({ pauseOnCaptureRecovery: true });
  const state = { phase, historyStale: true, historyFailed: false, historyLagging: false, message: '' };
  const observation = observe(null, { ...homeSnapshot().capture, state: 'WaitingForGame' }, state, 0);
  expect(observation).toMatchObject({ retained: null, paused: false });
  expect(railView(null, [], true).stale).toBe(false);
  const snapshot = homeSnapshot();
  expect(railView(snapshot, [], true).stale).toBe(true);
});

it('startup and an accepted empty snapshot never claim a last observation', async () => {
  const empty = homeSnapshot({ stream_id: '', sequence: '0', character: null, session: null,
    map: null, area_kind: 'unknown', run: null, fights: [], drops: [] });
  empty.capture.state = 'WaitingForGame';
  const transport = createFixtureTransport(empty);
  const client = createClient(transport);
  const pending = deferred<typeof empty>();
  client.querySnapshot = vi.fn(() => pending.promise);
  render(Shell, { client, historyAdapter: fakeHistory('#/runs').adapter }); await flushHomeRender();
  const rail = screen.getByRole('complementary', { name: 'Character' });
  expect(client.querySnapshot).toHaveBeenCalledOnce();
  expect(within(rail).getByRole('heading', { name: 'No Character' })).toBeTruthy();
  expect(within(rail).queryByText('Paused · last observation')).toBeNull();
  pending.resolve(empty); await flushHomeRender();
  expect(within(rail).getByText('Waiting for Game')).toBeTruthy();
  expect(within(rail).queryByText('Paused · last observation')).toBeNull();
  expect(within(rail).queryByText('Last seen')).toBeNull();
  const observed = homeSnapshot({ stream_id: '', sequence: '1', capture: empty.capture });
  transport.publishSnapshot(observed); await flushHomeRender();
  expect(within(rail).getByRole('heading', { name: observed.character!.name! })).toBeTruthy();
  expect(within(rail).getByText('Paused · last observation')).toBeTruthy();
  expect(within(rail).getByText('Last seen')).toBeTruthy();
  transport.publishCapture({ ...empty.capture, state: 'Failed', message: 'Live updates interrupted; reconnect or restart Maxed' });
  await flushHomeRender();
  expect(within(rail).getByRole('heading', { name: 'No Character' })).toBeTruthy();
  expect(within(rail).queryByText('Paused · last observation')).toBeNull();
  expect(within(rail).queryByText('Last seen')).toBeNull();
});
