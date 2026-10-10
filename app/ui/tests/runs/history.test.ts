import { setDebugMode } from '../../src/lib/debug/debug';
import { cleanup, screen, within } from '@testing-library/svelte';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import type { RunPage } from '../../src/lib/bindings/RunPage';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import { advance, click, deferred, edit, fight, flush, follow, main, mountRuns, nativeQueue, ready, run, runPage, saved, story } from './fixtures';

beforeEach(() => { setDebugMode(true);
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2030-01-01T12:00:00Z'));
});
afterEach(() => { setDebugMode(false);
  cleanup();
  vi.clearAllTimers();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it('open_story_converges_after_durable_revision', async () => {
  const old = story({ run: run({ outcome: 'left', completed_at: null, evidence: [] }) });
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: async () => ready(old) } }); await flush();
  expect(main().getByText(/^Left$/i)).toBeTruthy();
  const focus = main().getByRole('heading', { name: 'Run', level: 1 }); focus.focus();
  page.queries.getRun.mockResolvedValue(ready(story()));
  page.event({ run_updated: run() }, '13'); await flush();
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  expect(main().getByText(/^Left$/i)).toBeTruthy();
  page.setStore(saved('13')); await advance();
  expect(main().getByText(/^Cleared$/i)).toBeTruthy();
  expect(main().queryByText(/^Left$/i)).toBeNull();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  expect(document.activeElement).toBe(focus);
  expect(main().getAllByRole('heading', { level: 1 })).toHaveLength(1);
});

it.each(['unrelated', 'home', 'matching'] as const)('fight_updated_advances_required_only_on_p4_routes (%s)', async kind => {
  const page = mountRuns({ hash: kind === 'home' ? '#/' : '#/runs/run-1' }); await flush();
  const home = vi.mocked(page.client.queryHome);
  if (kind === 'home') expect(home).toHaveBeenCalledTimes(1);
  else expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  page.queries.getRun.mockResolvedValue(ready(story({ run: run({ dungeon: 'Durable revised story' }) })));
  page.event({ fight_updated: fight({ run_id: kind === 'unrelated' ? 'other-run' : 'run-1' }) }, '13'); await flush();
  await advance();
  expect(page.queries.getRun).toHaveBeenCalledTimes(kind === 'home' ? 0 : 1);
  if (kind !== 'matching') {
    // Catch accidental invalidation even when the irrelevant tick is durable.
    page.setStore(saved('13')); await advance();
    expect(page.queries.getRun).toHaveBeenCalledTimes(kind === 'home' ? 0 : 1);
    if (kind === 'home') expect(home).toHaveBeenCalledTimes(1);
    // Positive control: a base run event is still accepted by this same shell.
    page.event({ run_updated: run() }, '14'); await flush(); page.setStore(saved('14')); await advance();
  } else { page.setStore(saved('13')); await advance(); }
  if (kind === 'home') expect(home).toHaveBeenCalledTimes(2);
  else {
    expect(page.queries.getRun).toHaveBeenCalledTimes(2);
    expect(main().getByText('Durable revised story')).toBeTruthy();
  }
});

it.each([null, 'wrong-stream', '9007199254740992'] as const)('watermark_gate_uses_same_stream_exact_decimal (%s)', async invalid => {
  const snapshot = (await import('../home/fixtures')).homeSnapshot({ sequence: '9007199254740992', area_kind: 'nexus', run: null });
  const page = mountRuns({ hash: '#/runs/run-1', snapshot }); await flush();
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  page.queries.getRun.mockResolvedValue(ready(story({ run: run({ dungeon: 'Exact checkpoint accepted' }) })));
  page.event({ run_updated: run() }, '9007199254740993');
  page.setStore(saved(invalid === 'wrong-stream' ? '9007199254740993' : invalid, invalid === 'wrong-stream' ? 'retired' : snapshot.stream_id));
  await advance();
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  page.setStore(saved('9007199254740993', snapshot.stream_id)); await advance();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  expect(main().getByText('Exact checkpoint accepted')).toBeTruthy();
});

it.each(['failed', 'backpressured'] as const)('writer_state_retains_stale_story_and_retry_obeys_durability (%s)', async state => {
  const page = mountRuns({ hash: '#/runs/run-1' }); await flush();
  page.queries.getRun.mockResolvedValue(ready(story({ run: run({ dungeon: 'Recovered durable story' }) })));
  page.setStore({ ...saved('13'), state, queued: 3, message: 'Synthetic writer fault' });
  page.event({ run_updated: run() }, '13'); await advance();
  expect(main().getByText('Invented Sanctum')).toBeTruthy();
  expect(screen.getAllByText(/saving.*failed|backlog|backpressure/i).length).toBeGreaterThan(0);
  const retry = main().queryByRole('button', { name: /Retry/i });
  if (retry) { await (await import('@testing-library/svelte')).fireEvent.click(retry); await flush(); }
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  page.setStore(saved('13')); await advance();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  expect(main().getByText('Recovered durable story')).toBeTruthy();
});

it('fixed_checkpoint_does_not_starve_under_sustained_fight_ticks', async () => {
  const page = mountRuns({ hash: '#/runs/run-1' }); await flush();
  page.queries.getRun.mockResolvedValue(ready(story({ run: run({ dungeon: 'First checkpoint' }) })));
  for (let sequence = 13; sequence <= 20; sequence++) {
    page.event({ fight_updated: fight() }, String(sequence));
    if (sequence >= 16) page.setStore(saved('13'));
    await advance(250);
  }
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  expect(main().getByText('First checkpoint')).toBeTruthy();
  page.queries.getRun.mockResolvedValue(ready(story({ run: run({ dungeon: 'Final checkpoint' }) })));
  page.setStore(saved('20')); await advance();
  expect(page.queries.getRun).toHaveBeenCalledTimes(3);
  expect(main().getByText('Final checkpoint')).toBeTruthy();
});

it('old_filter_reply_cannot_paint_latest_filter', async () => {
  const pending = deferred<HistoryReply<RunPage>>();
  const page = mountRuns({ queries: { listRuns: () => pending.promise } }); await flush();
  await edit('Dungeon', 'Latest dungeon'); await advance(300);
  page.queries.listRuns.mockResolvedValue(ready(runPage([run({ dungeon: 'Latest dungeon' })])));
  pending.resolve(ready(runPage([run({ dungeon: 'Obsolete dungeon' })]))); await flush();
  expect(main().queryByRole('link', { name: /Obsolete dungeon/ })).toBeNull();
  expect(main().getByRole('link', { name: /Latest dungeon/ })).toBeTruthy();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(2);
});

it('namespace_retirement_discards_late_reply_and_accepts_new_baseline', async () => {
  const pending = deferred<HistoryReply<RunStory | null>>();
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: () => pending.promise } }); await flush();
  page.queries.getRun.mockResolvedValue(ready(story({ run: run({ dungeon: 'Replacement namespace' }) })));
  const next = { ...page.snapshot, stream_id: 'opaque-new-namespace', sequence: '1' };
  page.setStore(saved('1', next.stream_id)); page.publish(next); await flush();
  pending.resolve(ready(story({ run: run({ dungeon: 'Retired namespace' }) }))); await flush();
  expect(main().queryByText('Retired namespace')).toBeNull();
  // Retirement may send the shell Home. Explicitly visit the new namespace's story.
  if (page.browser.adapter.hash !== '#/runs/run-1') {
    await follow('#/runs', within(screen.getByRole('navigation', { name: 'Navigation' })));
    await follow('#/runs/run-1');
  }
  expect(main().getByText('Replacement namespace')).toBeTruthy();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
});

it('snapshot_resync_refreshes_once_after_its_baseline_is_durable', async () => {
  const page = mountRuns({ hash: '#/runs/run-1' }); await flush();
  page.queries.getRun.mockResolvedValue(ready(story({ run: run({ dungeon: 'Resynchronized story' }) })));
  // A coalesced numeric snapshot is not a resync and must not trigger history.
  page.publish({ ...page.snapshot, sequence: '14' }); page.setStore(saved('14')); await advance();
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  const baseline = deferred<typeof page.snapshot>();
  page.client.querySnapshot = vi.fn(() => baseline.promise);
  // A gap in ordered events forces the real live layer into resyncing. This
  // event is not itself a history invalidator; the queried baseline owns 21.
  page.event({ capture_state_changed: page.snapshot.capture }, '20'); await flush();
  expect(page.client.querySnapshot).toHaveBeenCalledTimes(1);
  baseline.resolve({ ...page.snapshot, sequence: '21' }); await flush();
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  page.setStore(saved('20')); await advance();
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  page.setStore(saved('21')); await advance();
  expect(main().getByText('Resynchronized story')).toBeTruthy();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  await advance(2000); expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  expect(page.client.querySnapshot).toHaveBeenCalledTimes(1);
});

it('active_pending_replacement_is_silent_and_latest_route_renders', async () => {
  const queue = nativeQueue();
  const page = mountRuns({ hash: '#/runs/run-1', queries: {
    getRun: id => queue.submit(`run:${id}`), listRuns: () => queue.submit('runs'),
  } }); await flush();
  await follow('#/runs', within(screen.getByRole('navigation', { name: 'Navigation' })));
  await click('Back', within(screen.getByRole('group', { name: 'History' })));
  expect(queue.pendingKey).toBe('run:run-1');
  queue.finish(story({ run: run({ dungeon: 'Superseded active' }) })); await flush();
  expect(main().queryByRole('button', { name: /Retry/i })).toBeNull();
  expect(main().queryByText('Superseded active')).toBeNull();
  queue.finish(story({ run: run({ dungeon: 'Accepted latest story' }) })); await flush();
  expect(main().getByText('Accepted latest story')).toBeTruthy();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  expect(page.queries.listRuns).toHaveBeenCalledTimes(1);
});

it('deferred_busy_retries_once_then_requires_stable_manual_retry', async () => {
  const queue = nativeQueue();
  const active = queue.submit('external-active');
  const replaced = Array.from({ length: 31 }, (_, i) => queue.submit(`external-pending:${i}`));
  const page = mountRuns({ hash: '#/runs/run-1', queries: { getRun: () => queue.submit('run:run-1') } }); await flush();
  expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  await advance(0); expect(page.queries.getRun).toHaveBeenCalledTimes(1);
  queue.finish(null); await active; await flush();
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  queue.finish(null); await Promise.all(replaced); await flush();
  expect(main().getByText(/busy/i)).toBeTruthy();
  const retry = main().getByRole('button', { name: /Retry/i }); retry.focus();
  await advance(2000);
  expect(page.queries.getRun).toHaveBeenCalledTimes(2);
  expect(main().getByRole('button', { name: /Retry/i })).toBe(retry);
  await click(/Retry/i); queue.finish(story({ run: run({ dungeon: 'Manual recovery' }) })); await flush();
  expect(page.queries.getRun).toHaveBeenCalledTimes(3);
  expect(main().getByText('Manual recovery')).toBeTruthy();
});

it('current_superseded_does_not_spin_and_permanent_failure_stops', async () => {
  const page = mountRuns({ queries: { listRuns: async () => ({ status: 'superseded' }) } }); await flush();
  await advance(6000);
  expect(page.queries.listRuns).toHaveBeenCalledTimes(1);
  expect(main().queryByRole('button', { name: /Retry/i })).toBeNull();
  expect(screen.getByRole('status', { name: 'Background activity' }).textContent).toBe('');
  page.queries.listRuns.mockRejectedValueOnce(new Error('permanent read failure'));
  await edit('Dungeon', 'New input'); await advance(300);
  expect(main().getByText(/Run Error/i)).toBeTruthy();
  await advance(6000); expect(page.queries.listRuns).toHaveBeenCalledTimes(2);
  page.queries.listRuns.mockResolvedValue(ready(runPage())); await click(/Retry/i);
  expect(main().getByRole('link', { name: /Invented Sanctum/ })).toBeTruthy();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(3);
});

it.each(['manual', 'navigation'] as const)('startup_schema_error_stays_error_until_shared_recovery (%s)', async action => {
  const start = Date.now();
  const message = 'History schema upgrade failed; close other Maxed instances or resolve local storage access, then Retry.';
  const page = mountRuns({ queries: { listRuns: async () => {
    if (Date.now() - start < 5000) throw new Error(message);
    return ready(runPage([run({ dungeon: 'Upgraded populated history' })]));
  } } }); await flush();
  expect(main().getByText(message)).toBeTruthy();
  expect(main().queryByText(/No.*runs/i)).toBeNull();
  await click(/Retry/i); expect(main().getByText(message)).toBeTruthy();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(2);
  await advance(5000);
  if (action === 'manual') await click(/Retry/i);
  else {
    await follow('#/', within(screen.getByRole('navigation', { name: 'Navigation' })));
    await follow('#/runs', within(screen.getByRole('navigation', { name: 'Navigation' })));
  }
  expect(main().getByRole('link', { name: /Upgraded populated history/ })).toBeTruthy();
  expect(page.queries.listRuns).toHaveBeenCalledTimes(3);
});
