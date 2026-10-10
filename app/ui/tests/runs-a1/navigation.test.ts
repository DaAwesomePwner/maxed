import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import Shell from '../../src/lib/shell/Shell.svelte';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunPage } from '../../src/lib/bindings/RunPage';
import { fakeHistory } from '../navigation/fake-history';
import { deferred, homeSnapshot, homeSummary, saved } from '../home/fixtures';
import { flushHomeRender } from '../home/shell-fixture';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date('2030-01-01T12:00:00Z')); });
afterEach(() => { cleanup(); vi.clearAllTimers(); vi.unstubAllGlobals(); vi.useRealTimers(); vi.restoreAllMocks(); });
function harness(hash = '#/') {
  const snapshot = homeSnapshot({ area_kind: 'nexus', run: null });
  const run = homeSummary().recent_runs[0];
  const pending: Array<ReturnType<typeof deferred<HistoryReply<RunPage>>>> = [];
  const listRuns = vi.fn(async () => {
    const job = deferred<HistoryReply<RunPage>>(); pending.push(job); return job.promise;
  });
  const transport = createFixtureTransport(snapshot, { client_fingerprint: '', objects: [] }, homeSummary(), {
    listRuns, getRun: async () => ({ status: 'ready', value: null }),
    getFight: async () => ({ status: 'ready', value: null }), getPlayerBreakdown: async () => ({ status: 'ready', value: null }),
  });
  const client = createClient(transport);
  let durable = snapshot.sequence;
  client.queryStore = vi.fn(async () => saved(durable, snapshot.stream_id));
  render(Shell, { client, historyAdapter: fakeHistory(hash).adapter });
  const reply = (dungeon = 'Before resync'): HistoryReply<RunPage> => ({ status: 'ready', value: { rows: [{ ...run, dungeon }], next: null } });
  return { snapshot, run, pending, listRuns, transport, reply, saveThrough: (sequence: string) => { durable = sequence; } };
}

it('same-stream gap resync keeps the focused Runs row mounted until one durable refresh completes', async () => {
  const h = harness('#/runs'); await flushHomeRender();
  expect(h.listRuns).toHaveBeenCalledOnce();
  h.pending[0].resolve(h.reply()); await flushHomeRender();
  const row = screen.getByRole('link', { name: /Before resync/ }); row.focus();
  expect(document.activeElement).toBe(row);
  // A numeric snapshot can cover the event before the event stream detects its gap.
  h.transport.publishSnapshot({ ...h.snapshot, sequence: '15' }); await flushHomeRender();
  expect(h.listRuns).toHaveBeenCalledOnce();
  h.transport.publishEngineEvent({ stream_id: h.snapshot.stream_id, sequence: '15', at: h.snapshot.at,
    event: { run_updated: { ...h.run, party_size: 4 } } });
  await flushHomeRender();
  expect(screen.getByRole('link', { name: /Before resync/ })).toBe(row);
  expect(document.activeElement).toBe(row);
  expect(h.listRuns).toHaveBeenCalledOnce();
  h.saveThrough('15'); await vi.advanceTimersByTimeAsync(1000); await flushHomeRender();
  expect(h.listRuns).toHaveBeenCalledTimes(2);
  expect(screen.getByRole('link', { name: /Before resync/ })).toBe(row);
  expect(document.activeElement).toBe(row);
  h.pending[1].resolve(h.reply('After resync')); await flushHomeRender();
  expect(screen.getByRole('link', { name: /After resync/ })).toBe(row);
  expect(document.activeElement).toBe(row);
  await vi.advanceTimersByTimeAsync(2000); await flushHomeRender();
  expect(h.listRuns).toHaveBeenCalledTimes(2);
});

it.each(['rows', 'fallback'] as const)('explicit navigation preserves user focus inside the pane when readiness ends via %s', async completion => {
  const h = harness(); await flushHomeRender();
  const primary = screen.getByRole('navigation', { name: 'Navigation' });
  const runs = within(primary).getByRole('link', { name: 'Runs' }); runs.focus();
  await fireEvent.click(runs); await flushHomeRender();
  expect(h.listRuns).toHaveBeenCalledOnce();
  const input = screen.getByRole('searchbox', { name: 'Dungeon' }); input.focus();
  await fireEvent.input(input, { target: { value: 'Unsubmitted filter' } }); await flushHomeRender();
  expect(document.activeElement).toBe(input);
  if (completion === 'fallback') {
    await vi.advanceTimersByTimeAsync(5000); await flushHomeRender();
    expect(document.activeElement).toBe(input);
  }
  if (completion === 'fallback') {
    h.pending[0].resolve({ status: 'superseded' }); await flushHomeRender();
    h.pending.at(-1)!.resolve(h.reply('Loaded after typing'));
  } else h.pending[0].resolve(h.reply('Loaded after typing')); await flushHomeRender();
  expect(screen.getByRole('link', { name: /Loaded after typing/ })).toBeTruthy();
  expect(document.activeElement).toBe(input);
  expect((input as HTMLInputElement).value).toBe('Unsubmitted filter');
  // Positive control: ordinary explicit navigation still focuses its ready heading.
  const settings = within(screen.getByRole('complementary', { name: 'Character' })).getByRole('link', { name: 'Settings' }); settings.focus();
  await fireEvent.click(settings); await flushHomeRender();
  expect(document.activeElement).toBe(screen.getByRole('heading', { name: 'Settings', level: 1 }));
});
