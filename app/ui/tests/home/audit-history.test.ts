import { fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import { createClient, type Client } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { createHomeHistory, type HomeHistoryState } from '../../src/lib/home/summary';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { fakeHistory } from '../navigation/fake-history';
import { deferred, flush, homeSnapshot, homeSummary, saved } from './fixtures';
import { flushHomeRender } from './shell-fixture';

const ready = (value: ReturnType<typeof homeSummary>): HistoryReply<ReturnType<typeof homeSummary>> => ({ status: 'ready', value });
const disposals: (() => void)[] = [];
beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); vi.useRealTimers(); vi.restoreAllMocks(); });

it.each([false, true])('initial read ahead of durability is stale and re-reads exactly once (writer wins race: %s)', async writerFirst => {
  const first = deferred<ReturnType<typeof homeSummary>>();
  const queryHome = vi.fn<Client['queryHome']>().mockImplementationOnce(() => first.promise.then(ready)).mockResolvedValue(ready(homeSummary({ runs: 2 })));
  const states: HomeHistoryState[] = [];
  const controller = createHomeHistory({ queryHome }, state => states.push(state));
  disposals.push(controller.dispose);
  const input = { mode: 'nexus' as const, scope: { session: { id: 'demo-session' } },
    streamId: 'live-synthetic-home', required: '12', initial: true, store: saved('11') };
  controller.update(input); await flush();
  expect(queryHome).toHaveBeenCalledOnce();
  if (writerFirst) controller.update({ ...input, store: saved('12') });
  first.resolve(homeSummary({ runs: 0 })); await flush();
  expect(states.find(state => state.summary?.runs === 0)?.phase).toBe('stale');
  if (!writerFirst) {
    expect(queryHome).toHaveBeenCalledOnce();
    expect(states.at(-1)).toMatchObject({ phase: 'stale', summary: { runs: 0 } });
    controller.update({ ...input, store: saved('12') }); await flush();
  }
  expect(queryHome).toHaveBeenCalledTimes(2);
  expect(states.at(-1)).toMatchObject({ phase: 'ready', summary: { runs: 2 } });
  for (const durable of ['12', '13', '14']) controller.update({ ...input, store: saved(durable) });
  await flush();
  expect(queryHome).toHaveBeenCalledTimes(2);
});

it.each([false, undefined])('a remounted history controller waits for an event checkpoint (initial: %s)', async initial => {
  const states: HomeHistoryState[] = [];
  const queryHome = vi.fn<Client['queryHome']>().mockResolvedValue(ready(homeSummary({ runs: 2 })));
  const controller = createHomeHistory({ queryHome }, state => states.push(state));
  disposals.push(controller.dispose);
  const input = { mode: 'nexus' as const, scope: { session: { id: 'demo-session' } },
    streamId: 'live-synthetic-home', required: '13', initial, store: saved('12') };
  controller.update(input); await flush();
  expect(states.at(-1)).toMatchObject({ phase: 'stale', summary: null });
  expect(queryHome).not.toHaveBeenCalled();
  controller.update({ ...input, store: saved('13') }); await flush();
  expect(queryHome).toHaveBeenCalledOnce();
  expect(states.at(-1)).toMatchObject({ phase: 'ready', summary: { runs: 2 } });
});

it('Shell establishes the initial snapshot checkpoint before the writer catches up', async () => {
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
  const client = createClient(createFixtureTransport(snapshot, { client_fingerprint: 'a'.repeat(64), objects: [] }));
  let store = saved('11');
  client.queryStore = vi.fn(async () => store);
  client.queryHome = vi.fn<Client['queryHome']>().mockResolvedValueOnce(ready(homeSummary({ runs: 0 }))).mockResolvedValue(ready(homeSummary({ runs: 2 })));
  render(Shell, { client, historyAdapter: fakeHistory().adapter });
  await flushHomeRender();
  expect(client.queryHome).toHaveBeenCalledOnce();
  expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toContain('Refreshing history');
  store = saved('12');
  await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
  expect(client.queryHome).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('status', { name: 'Background activity' }).textContent).not.toContain('Refreshing history');
  expect(screen.getByText('Runs', { selector: 'dt' }).closest('[aria-label]')?.getAttribute('aria-label')).toBe('Session Stats');
  expect(screen.getByText('Runs', { selector: 'dt' }).parentElement?.querySelector('.value')?.textContent).toBe('2');
  await vi.advanceTimersByTimeAsync(2000); await flushHomeRender();
  expect(client.queryHome).toHaveBeenCalledTimes(2);
});

it('Superseded during initial read-ahead stays inert until the checkpoint catches up', async () => {
  const queryHome = vi.fn<Client['queryHome']>().mockResolvedValueOnce({ status: 'superseded' })
    .mockResolvedValue(ready(homeSummary({ runs: 2 })));
  const states: HomeHistoryState[] = [];
  const controller = createHomeHistory({ queryHome }, state => states.push(state));
  disposals.push(controller.dispose);
  const input = { mode: 'nexus' as const, scope: { session: { id: 'demo-session' } },
    streamId: 'live-synthetic-home', required: '12', initial: true, store: saved('11') };
  controller.update(input); await flush();
  expect(queryHome).toHaveBeenCalledOnce();
  expect(states.at(-1)).toMatchObject({ phase: 'superseded', retryAvailable: false, summary: null });
  controller.update(input); controller.retry(); await flush();
  expect(queryHome).toHaveBeenCalledOnce();
  expect(states.at(-1)).toMatchObject({ phase: 'superseded', retryAvailable: false });
  controller.update({ ...input, store: saved('12') }); await flush();
  expect(queryHome).toHaveBeenCalledTimes(2);
  expect(states.at(-1)).toMatchObject({ phase: 'ready', summary: { runs: 2 }, retryAvailable: true });
});

it.each(['ready', 'busy'] as const)('initial read-ahead retries Busy once while durability lags (second reply: %s)', async second => {
  const queryHome = vi.fn<Client['queryHome']>().mockResolvedValueOnce({ status: 'busy' })
    .mockResolvedValueOnce(second === 'busy' ? { status: 'busy' } : ready(homeSummary({ runs: 0 })))
    .mockResolvedValue(ready(homeSummary({ runs: 2 })));
  const states: HomeHistoryState[] = [];
  const controller = createHomeHistory({ queryHome }, state => states.push(state));
  disposals.push(controller.dispose);
  const input = { mode: 'nexus' as const, scope: { session: { id: 'demo-session' } },
    streamId: 'live-synthetic-home', required: '12', initial: true, store: saved('11') };
  controller.update(input); await flush();
  expect(queryHome).toHaveBeenCalledTimes(2);
  expect(states.some(state => state.phase === 'busy' && !state.retryAvailable)).toBe(true);
  expect(states.at(-1)).toMatchObject(second === 'busy'
    ? { phase: 'busy', retryAvailable: true, summary: null }
    : { phase: 'stale', summary: { runs: 0 } });
  controller.update(input); await flush();
  expect(queryHome).toHaveBeenCalledTimes(2);
  controller.update({ ...input, store: saved('12') }); await flush();
  expect(queryHome).toHaveBeenCalledTimes(3);
  expect(states.at(-1)).toMatchObject({ phase: 'ready', summary: { runs: 2 }, retryAvailable: true });
});

it.each([false, true])('idle default snapshot loads saved Today and any available Session (session: %s)', async withSession => {
  const snapshot = homeSnapshot({ stream_id: '', sequence: '0', area_kind: 'unknown', map: null,
    character: null, run: null, fights: [], drops: [], session: withSession ? homeSnapshot().session : null,
    capture: { ...homeSnapshot().capture, state: 'Stopped' } });
  const client = createClient(createFixtureTransport(snapshot, { client_fingerprint: '', objects: [] }, homeSummary({ runs: 3 })));
  client.queryStore = vi.fn<Client['queryStore']>(async () => ({ state: 'closed', stream_id: null, durable: null, queued: 0, message: '' }));
  const setup = client.querySetup;
  client.querySetup = vi.fn(async () => ({ ...await setup(), demo: false }));
  const query = vi.spyOn(client, 'queryHome');
  render(Shell, { client, historyAdapter: fakeHistory().adapter }); await flushHomeRender();
  expect(screen.getByText('Ready')).toBeTruthy();
  expect(query).toHaveBeenCalledTimes(withSession ? 1 : 0);
  if (withSession) expect(query).toHaveBeenLastCalledWith({ session: { id: snapshot.session!.id } });
  await fireEvent.click(screen.getByRole('button', { name: 'Today' })); await flushHomeRender();
  expect(query).toHaveBeenCalledTimes(withSession ? 2 : 1);
  expect(query).toHaveBeenLastCalledWith({ today: { range: expect.objectContaining({ start: expect.any(String), end: expect.any(String) }) } });
  expect(screen.getByText('Runs', { selector: 'dt' }).closest('[aria-label]')?.getAttribute('aria-label')).toBe('Today Stats');
  expect(screen.getByText('Runs', { selector: 'dt' }).parentElement?.querySelector('.value')?.textContent).toBe('3');
  expect(screen.queryByText('Waiting for saved history to catch up.')).toBeNull();
});
