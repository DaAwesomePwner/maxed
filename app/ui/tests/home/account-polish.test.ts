import { writable } from 'svelte/store';
import { tick } from 'svelte';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, it, vi } from 'vitest';
import Home from '../../src/routes/Home.svelte';
import { createHomeObservation } from '../../src/lib/home/mode';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { SHELL_CONNECTION } from '../../src/lib/shell/context';
import { emptyHomeLiveWindow } from '../../src/lib/home/v2/live';
import { homeSnapshot, homeSummary, saved } from './fixtures';
import { mountHome, flushHomeRender } from './shell-fixture';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { HomeSummary } from '../../src/lib/bindings/HomeSummary';

afterEach(cleanup);
const a = homeSnapshot();
a.character!.account_id = 'account-a';
const b = { ...a, character: { ...a.character!, id: 'synthetic-b', name: 'Synthetic B', account_id: 'account-b' } };
function identity(account: string | null, character: string | null, sequence = '13'): EventEnvelope {
  return { stream_id: a.stream_id, sequence, at: a.at,
    event: { local_identity_changed: { at: a.at, account_id: account, character_id: character } } };
}

it.each(['notice-first', 'identity-first'] as const)('adopts first verified B with %s', order => {
  const observe = createHomeObservation({ pauseOnCaptureRecovery: true });
  observe(a, a.capture, null, 0);
  if (order === 'notice-first') {
    observe.historyChanged();
    expect(observe(a, a.capture, null, 1).retained).toBeNull();
  }
  observe.onEvent(identity('account-b', b.character.id));
  expect(observe(b, b.capture, null, 2).retained?.character?.id).toBe(b.character.id);
  if (order === 'identity-first') observe.historyChanged();
  expect(observe(b, b.capture, null, 3).retained?.character?.id).toBe(b.character.id);
});

it.each(['notice-first', 'identity-first'] as const)('Shell rail switches with %s across separate renders', async order => {
  const h = mountHome(a);
  const rail = await screen.findByRole('complementary', { name: 'Character' });
  await within(rail).findByRole('heading', { name: a.character!.name! });
  if (order === 'notice-first') {
    h.transport.publishAccountHistoryChanged({ revision: 1, reason: 'account_switched' });
    await flushHomeRender();
    expect(within(rail).queryByRole('heading', { name: a.character!.name! })).toBeNull();
  }
  const sequence = String(BigInt(a.sequence) + 1n);
  h.transport.publishEngineEvent(identity('account-b', b.character.id, sequence));
  h.transport.publishSnapshot({ ...b, sequence });
  await within(rail).findByRole('heading', { name: 'Synthetic B' });
  if (order === 'identity-first') {
    h.transport.publishAccountHistoryChanged({ revision: 1, reason: 'account_switched' });
    await flushHomeRender();
  }
  expect(within(rail).getByRole('heading', { name: 'Synthetic B' })).toBeTruthy();
});

it('late ready A cannot overwrite B and dismissal transfers focus to Home', async () => {
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
  const fixture = createFixtureTransport(snapshot);
  const client = createClient(fixture);
  let finish!: (reply: HistoryReply<HomeSummary>) => void;
  const query = vi.spyOn(client, 'queryHome')
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValue({ status: 'ready', value: homeSummary({ unattributed_history: true }) });
  render(Home, { snapshot, client, store: saved() });
  await waitFor(() => expect(query).toHaveBeenCalledTimes(1));
  fixture.publishAccountHistoryChanged({ revision: 1, reason: 'account_switched' });
  await screen.findByText(/Some older history/);
  finish({ status: 'ready', value: homeSummary({ unattributed_history: false, runs: 98765 }) });
  await flushHomeRender();
  expect(screen.getByText(/Some older history/)).toBeTruthy();
  expect(document.body.textContent).not.toContain('98,765');
  const dismiss = screen.getByRole('button', { name: 'Dismiss older history notice' });
  dismiss.focus();
  await fireEvent.click(dismiss);
  expect(document.activeElement).toBe(screen.getByRole('region', { name: 'Home content' }));
});

it('loss, provisional and verified identities do not refetch Home reads', async () => {
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
  const fixture = createFixtureTransport(snapshot, undefined, homeSummary());
  const client = createClient(fixture);
  const queries = [vi.spyOn(client, 'queryHome'), vi.spyOn(client, 'queryDungeonBests'), vi.spyOn(client, 'getHomeHud')];
  const page = render(Home, { snapshot, client, store: saved(), redesign: true });
  await waitFor(() => queries.forEach(query => expect(query).toHaveBeenCalled()));
  const counts = queries.map(query => query.mock.calls.length);
  for (const event of [identity('account-a', a.character!.id), identity(null, null),
    identity(null, 'provisional'), identity('account-a', a.character!.id), identity('account-b', b.character.id)]) {
    await page.rerender({ event });
    await flushHomeRender();
  }
  expect(queries.map(query => query.mock.calls.length)).toEqual(counts);
});

it.each(['notice', 'identity'] as const)('Home drops retained A during recovery on %s', async source => {
  const fixture = createFixtureTransport(a);
  const client = createClient(fixture);
  const page = render(Home, { snapshot: a, client, store: saved() });
  await flushHomeRender();
  await page.rerender({ capture: { ...a.capture, state: 'Recovering' } });
  await flushHomeRender();
  expect(page.container.querySelector('.dungeon-home')).not.toBeNull();
  if (source === 'notice') fixture.publishAccountHistoryChanged({ revision: 1, reason: 'account_switched' });
  else await page.rerender({ event: identity('account-b', b.character.id) });
  await flushHomeRender();
  expect(page.container.querySelector('.dungeon-home')).toBeNull();
});

it('clears the rendered ended streak without changing the run', async () => {
  const snapshot = homeSnapshot({ drops: [] });
  const fixture = createFixtureTransport(snapshot);
  const client = createClient(fixture);
  vi.spyOn(client, 'getHomeDungeonStreak').mockResolvedValue({ status: 'ready', value: {
    dungeon: snapshot.run!.dungeon, runs_ended: 10, runs_since_last_white: 7, avg_runs_per_white: null, last_white_at: null,
  } });
  const initial = { ...emptyHomeLiveWindow(), streamId: snapshot.stream_id, runId: snapshot.run!.id, fights: snapshot.fights };
  const window = writable(initial);
  render(Home, { props: { snapshot, client, store: saved(), redesign: true },
    context: new Map([[SHELL_CONNECTION, { homeWindow: window }]]) });
  await screen.findByText(/dry streak 7/);
  window.set({ ...initial, drops: [{ ...a.drops[0], id: 'synthetic-white', bag_rank: 255 }] });
  await screen.findByText('Ended at 7 runs');
  fixture.publishAccountHistoryChanged({ revision: 1, reason: 'legacy_recovered' });
  await tick();
  expect(screen.queryByText('Ended at 7 runs')).toBeNull();
});
