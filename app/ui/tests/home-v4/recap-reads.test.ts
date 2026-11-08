import { setNativePresentationVisible } from '../../src/lib/bridge/visibility';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { Client } from '../../src/lib/bridge/client';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { RunStory } from '../../src/lib/bindings/RunStory';
import type { VisitRecap } from '../../src/lib/bindings/VisitRecap';
import type { RecapState } from '../../src/lib/home/v4/recap';
import { createRecapReads, RECAP_BUSY_RETRY_MS, RECAP_SAVE_TIMEOUT_MS, type RecapLoad } from '../../src/lib/home/v4/recap-reads';
import { USUAL_WINDOW_MICROS } from '../../src/lib/home/v4/reads';
import { deferred, homeSnapshot, saved } from '../home/fixtures';
import { fightSummary } from '../runs/fixtures';

const disposals: Array<() => void> = [];
beforeEach(() => vi.useFakeTimers());
afterEach(() => { disposals.splice(0).forEach(dispose => dispose()); vi.useRealTimers(); });
const settle = () => vi.advanceTimersByTimeAsync(0);
const ready = <T>(value: T): HistoryReply<T> => ({ status: 'ready', value });
const snapshot = homeSnapshot();
const story: RunStory = { run: { ...snapshot.run!, started: '1791806400000000', ended: '1791806460000000' },
  character: snapshot.character, fights: [], drops: [], timeline: [], comparison: null };
const recap: RecapState = { source: { kind: 'run', streamId: snapshot.stream_id, runId: story.run.id }, watermark: '13',
  summary: { kind: 'run', title: 'Synthetic recap', outcome: 'cleared', startedAt: story.run.started,
    endedAt: story.run.ended, localDps: null, rank: null, players: null, fights: [], best: null, runId: story.run.id } };
const visitSource: RecapState = { ...recap, source: { kind: 'visit', streamId: snapshot.stream_id,
  visitKey: 'synthetic-visit', enteredSequence: '4' } };
const visit: VisitRecap = { visit_id: `${snapshot.stream_id}:visit:4`, session_id: 'synthetic-session',
  character_id: story.run.character_id, title: 'Synthetic saved realm',
  periods: [{ visit_id: `${snapshot.stream_id}:visit:4`, started: story.run.started, ended: story.run.ended }],
  fights: [], boss_fights: [], boss_fights_limited: false, announcements: [], announcements_limited: false,
  drops: [], drops_limited: false, fame_delta: null, best_dps: [], chain: null };

function harness() {
  const client = {
    getRun: vi.fn<Client['getRun']>().mockResolvedValue(ready(story)),
    getRunRecap: vi.fn<Client['getRunRecap']>().mockResolvedValue(ready(null)),
    getVisitRecap: vi.fn<Client['getVisitRecap']>().mockResolvedValue(ready(visit)),
    getDungeonProfile: vi.fn<Client['getDungeonProfile']>().mockResolvedValue(ready({ dungeon: story.run.dungeon,
      clears: 1, bosses: [], usual_run_dps: null, run_dps_samples: 0, usual_clear_micros: null, clear_samples: 0 })),
    getHomeBossBaselines: vi.fn<Client['getHomeBossBaselines']>().mockResolvedValue(ready({ rows: [], truncated: false })),
    getHomeDungeonStreak: vi.fn<Client['getHomeDungeonStreak']>().mockResolvedValue(ready({ dungeon: story.run.dungeon,
      runs_ended: 2, runs_since_last_white: 2, avg_runs_per_white: null, last_white_at: null })),
  };
  const loads: Array<RecapLoad | null> = [];
  const controller = createRecapReads(client, load => loads.push(load));
  disposals.push(controller.dispose);
  const input = { recap, store: saved('13'), streamId: snapshot.stream_id };
  return { client, controller, input, loads, last: () => loads.at(-1) };
}

it('saving_then_saved_when_durable_catches_up', async () => {
  const h = harness(); h.controller.update({ ...h.input, store: saved('12') }); await settle();
  expect(h.last()).toEqual({ status: 'saving' }); expect(h.client.getRun).not.toHaveBeenCalled();
  h.controller.update(h.input); await settle();
  expect(h.last()).toMatchObject({ status: 'saved', data: { kind: 'run', story, streak: { runs_since_last_white: 2 } } });
  expect(h.client.getRun).toHaveBeenCalledExactlyOnceWith(story.run.id);
  h.controller.update({ ...h.input, recap: { ...recap } }); await settle();
  expect(h.client.getRun).toHaveBeenCalledOnce();
});

it('times_out_to_partial_after_five_seconds_then_upgrades', async () => {
  const h = harness(); h.controller.update({ ...h.input, store: saved('12') });
  expect(RECAP_SAVE_TIMEOUT_MS).toBe(5000);
  await vi.advanceTimersByTimeAsync(4999); expect(h.last()).toEqual({ status: 'saving' });
  await vi.advanceTimersByTimeAsync(1); expect(h.last()).toMatchObject({ status: 'partial', reason: 'timeout' });
  h.controller.update(h.input); await settle(); expect(h.last()?.status).toBe('saved');
});

it('saved_recap_reads_when_capture_has_no_live_stream', async () => {
  const h = harness(); h.controller.update({ ...h.input, store: saved('99'), streamId: null }); await settle();
  expect(h.client.getRun).toHaveBeenCalledExactlyOnceWith(story.run.id);
  expect(h.last()).toMatchObject({ status: 'saved', data: { kind: 'run', story } });
});

it('a_different_live_stream_blocks_even_when_the_source_is_durable', async () => {
  const h = harness(); h.controller.update({ ...h.input, store: saved('99'), streamId: 'synthetic-other' });
  await settle(); expect(h.client.getRun).not.toHaveBeenCalled(); expect(h.last()?.status).toBe('saving');
});

it.each(['failed', 'backpressured'] as const)('failed_store_shows_partial_with_retry: %s', async state => {
  const h = harness(); h.controller.update({ ...h.input, store: { ...saved('13'), state } }); await settle();
  expect(h.last()).toMatchObject({ status: 'partial', reason: 'failed' }); expect(h.client.getRun).not.toHaveBeenCalled();
  h.controller.update(h.input); h.controller.retry(); await settle(); expect(h.last()?.status).toBe('saved');
});

it('null_story_is_partial', async () => {
  const h = harness(); h.client.getRun.mockResolvedValue(ready(null)); h.controller.update(h.input); await settle();
  expect(h.last()).toMatchObject({ status: 'partial', reason: 'failed' }); expect(h.client.getRunRecap).not.toHaveBeenCalled();
  h.client.getRun.mockResolvedValue(ready(story)); h.controller.retry(); await settle(); expect(h.last()?.status).toBe('saved');
});

it.each(['getRunRecap', 'getDungeonProfile', 'getHomeBossBaselines', 'getHomeDungeonStreak'] as const)(
  'profile_failure_keeps_saved_recap: %s', async method => {
    const h = harness(); h.client[method].mockRejectedValue(new Error('synthetic failure'));
    h.controller.update(h.input); await settle();
    const field = { getRunRecap: 'extras', getDungeonProfile: 'profile', getHomeBossBaselines: 'baselines', getHomeDungeonStreak: 'streak' }[method];
    expect(h.last()).toMatchObject({ status: 'saved', data: { kind: 'run', [field]: null } });
    expect(h.last()).toMatchObject({ data: { recordsUnavailable: method === 'getRunRecap' } });
  });

it('a_successful_null_record_read_is_available_and_retry_clears_failure', async () => {
  const h = harness(); h.client.getRunRecap.mockRejectedValueOnce(new Error('Synthetic unavailable'));
  h.controller.update(h.input); await settle();
  expect(h.last()).toMatchObject({ status: 'saved', data: { extras: null, recordsUnavailable: true } });
  h.controller.retry(); await settle();
  expect(h.last()).toMatchObject({ status: 'saved', data: { extras: null, recordsUnavailable: false } });
});

it('usual_range_ends_at_run_start', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  const range = { start: String(Number(story.run.started) - USUAL_WINDOW_MICROS), end: story.run.started };
  expect(h.client.getDungeonProfile).toHaveBeenCalledExactlyOnceWith({ dungeon: story.run.dungeon, character_id: story.run.character_id, range });
  expect(h.client.getHomeBossBaselines).toHaveBeenCalledExactlyOnceWith({ character_id: story.run.character_id, range, target_types: [] });
  const order = [h.client.getRun, h.client.getRunRecap, h.client.getDungeonProfile, h.client.getHomeBossBaselines, h.client.getHomeDungeonStreak]
    .map(call => call.mock.invocationCallOrder[0]);
  expect(order).toEqual([...order].sort((a, b) => a - b));
});

it('visit_without_entered_sequence_is_unavailable', async () => {
  const h = harness(); h.controller.update({ ...h.input, recap: { ...visitSource,
    source: { kind: 'visit', streamId: snapshot.stream_id, visitKey: 'synthetic-visit', enteredSequence: null } } });
  await settle(); expect(h.last()).toMatchObject({ status: 'partial', reason: 'unavailable' });
  expect(h.client.getVisitRecap).not.toHaveBeenCalled();
});

it('visit_reads_saved_identity_and_first_period_window_without_changing_title', async () => {
  const h = harness(); h.controller.update({ ...h.input, recap: visitSource }); await settle();
  expect(h.client.getVisitRecap).toHaveBeenCalledExactlyOnceWith({ visit_id: visit.visit_id });
  expect(h.client.getHomeBossBaselines).toHaveBeenCalledExactlyOnceWith({ character_id: visit.character_id, target_types: [],
    range: { start: String(Number(story.run.started) - USUAL_WINDOW_MICROS), end: story.run.started } });
  expect(h.last()).toMatchObject({ status: 'saved', data: { kind: 'visit', visit } });
});

it('null_visit_is_unavailable', async () => {
  const h = harness(); h.client.getVisitRecap.mockResolvedValue(ready(null));
  h.controller.update({ ...h.input, recap: visitSource }); await settle();
  expect(h.last()).toMatchObject({ status: 'partial', reason: 'unavailable' });
});

it('visit_baselines_include_minibosses', async () => {
  const h = harness();
  h.client.getVisitRecap.mockResolvedValue(ready({ ...visit, fights: [
    fightSummary(1, { target_type: 300, boss_type: 999, target_kind: 'boss' }),
    fightSummary(2, { target_type: 301, boss_type: null, target_kind: 'miniboss' }),
    fightSummary(3, { target_type: 302, boss_type: null, target_kind: 'ordinary' }),
  ] }));
  h.controller.update({ ...h.input, recap: visitSource }); await settle();
  expect(h.client.getHomeBossBaselines.mock.calls[0][0].target_types).toEqual([300, 301]);
  expect(h.last()?.status).toBe('saved');
});

it('visit_baselines_include_quests_exclude_unknowns_and_are_distinct_and_capped_at_64', async () => {
  const h = harness();
  h.client.getVisitRecap.mockResolvedValue(ready({ ...visit, fights: [
    fightSummary(1, { target_type: null, boss_type: 999, target_kind: 'boss' }),
    fightSummary(2, { target_type: 300, boss_type: 300, target_kind: null }),
    fightSummary(3, { target_type: 301, boss_type: null, target_kind: undefined }),
    fightSummary(4, { target_type: 302, boss_type: 302, target_kind: 'ordinary' }),
    fightSummary(5, { target_type: 400, boss_type: null, target_kind: 'quest' }),
    ...Array.from({ length: 70 }, (_, i) => fightSummary(i + 6, { target_type: 400 + i, boss_type: null, target_kind: 'miniboss' })),
  ] }));
  h.controller.update({ ...h.input, recap: visitSource }); await settle();
  expect(h.client.getHomeBossBaselines.mock.calls[0][0].target_types).toEqual(Array.from({ length: 64 }, (_, i) => 400 + i));
});

it('late_reply_for_replaced_source_is_dropped', async () => {
  const h = harness(), pending = deferred<HistoryReply<RunStory | null>>(); h.client.getRun.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle();
  h.controller.update({ ...h.input, recap: visitSource }); await settle();
  pending.resolve(ready(story)); await settle();
  expect(h.last()).toMatchObject({ status: 'saved', data: { kind: 'visit' } });
  expect(h.client.getRunRecap).not.toHaveBeenCalled();
});

it.each(['dismiss', 'dispose'] as const)('dismiss_clears_load_and_timers: %s', async action => {
  const h = harness(), pending = deferred<HistoryReply<RunStory | null>>(); h.client.getRun.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle();
  if (action === 'dismiss') h.controller.update({ ...h.input, recap: null }); else h.controller.dispose();
  expect(h.last()).toBeNull(); expect(vi.getTimerCount()).toBe(0);
  pending.resolve(ready(story)); await vi.advanceTimersByTimeAsync(6000); expect(h.last()).toBeNull();
});

it.each([false, true])('busy_retries_once: staysBusy=%s', async staysBusy => {
  const h = harness(); h.client.getRun.mockResolvedValueOnce({ status: 'busy' });
  if (staysBusy) h.client.getRun.mockResolvedValue({ status: 'busy' });
  h.controller.update(h.input); await vi.advanceTimersByTimeAsync(5000);
  expect(h.client.getRun).toHaveBeenCalledTimes(2);
  expect(h.last()?.status).toBe(staysBusy ? 'partial' : 'saved');
});

it('superseded_reads_retry_while_the_recap_is_current', async () => {
  const h = harness();
  h.client.getRun.mockResolvedValueOnce({ status: 'superseded' }).mockResolvedValueOnce({ status: 'superseded' });
  h.client.getRunRecap.mockResolvedValueOnce({ status: 'superseded' });
  h.client.getDungeonProfile.mockResolvedValueOnce({ status: 'superseded' });
  h.client.getHomeBossBaselines.mockResolvedValueOnce({ status: 'superseded' });
  h.client.getHomeDungeonStreak.mockResolvedValueOnce({ status: 'superseded' });
  h.controller.update(h.input); await vi.advanceTimersByTimeAsync(12000);
  expect(h.client.getRun).toHaveBeenCalledTimes(3);
  expect(h.last()).toMatchObject({ status: 'saved', data: { kind: 'run', story, recordsUnavailable: false,
    profile: { clears: 1 }, baselines: { rows: [] }, streak: { runs_since_last_white: 2 } } });
});

it('busy_after_supersession_waits_for_the_attempt_window_before_its_one_retry', async () => {
  const h = harness();
  h.client.getRun.mockResolvedValueOnce({ status: 'superseded' }).mockResolvedValueOnce({ status: 'busy' });
  h.client.getHomeDungeonStreak.mockResolvedValueOnce({ status: 'busy' });
  h.controller.update(h.input); await settle();
  expect(h.client.getRun).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(RECAP_BUSY_RETRY_MS);
  expect(h.client.getRun).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(RECAP_BUSY_RETRY_MS - 1); expect(h.client.getRun).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1); expect(h.client.getRun).toHaveBeenCalledTimes(3);
  await vi.advanceTimersByTimeAsync(RECAP_BUSY_RETRY_MS);
  expect(h.client.getHomeDungeonStreak).toHaveBeenCalledTimes(2);
  expect(h.last()).toMatchObject({ status: 'saved', data: { kind: 'run', streak: { runs_since_last_white: 2 } } });
});

it('repeated_supersession_backs_off_to_at_most_200ms', async () => {
  const h = harness(); h.client.getRun.mockResolvedValue({ status: 'superseded' });
  h.controller.update(h.input); await vi.advanceTimersByTimeAsync(1000);
  // Immediate, then 25, 50, 100, 200, 200, 200 ms: 9 attempts in the first second, not 40.
  expect(h.client.getRun.mock.calls.length).toBeLessThanOrEqual(9);
});

it('superseded_visit_read_retries', async () => {
  const h = harness(); h.client.getVisitRecap.mockResolvedValueOnce({ status: 'superseded' });
  h.controller.update({ ...h.input, recap: visitSource }); await settle(); await vi.advanceTimersByTimeAsync(RECAP_BUSY_RETRY_MS);
  expect(h.client.getVisitRecap).toHaveBeenCalledTimes(2);
  expect(h.last()).toMatchObject({ status: 'saved', data: { kind: 'visit', visit } });
});

it('superseded_retries_stop_when_the_recap_is_dismissed', async () => {
  const h = harness(); h.client.getRun.mockResolvedValue({ status: 'superseded' });
  h.controller.update(h.input); await vi.advanceTimersByTimeAsync(100);
  h.controller.update({ ...h.input, recap: null }); const calls = h.client.getRun.mock.calls.length;
  await vi.advanceTimersByTimeAsync(6000);
  expect(h.client.getRun).toHaveBeenCalledTimes(calls); expect(h.last()).toBeNull(); expect(vi.getTimerCount()).toBe(0);
});

it('a_different_stream_cannot_authorize_the_watermark', async () => {
  const h = harness(); h.controller.update({ ...h.input, streamId: 'synthetic-other', store: saved('99', 'synthetic-other') });
  await settle(); expect(h.client.getRun).not.toHaveBeenCalled(); expect(h.last()?.status).toBe('saving');
});

it('boss_types_include_event_bosses_are_distinct_and_capped_at_64', async () => {
  const h = harness();
  const fights = [fightSummary(1, { target_type: 300, boss_type: null, target_kind: 'ordinary' }),
    fightSummary(2, { target_type: 301, boss_type: null, target_kind: 'event_boss' }),
    ...Array.from({ length: 70 }, (_, i) => fightSummary(i + 3, { target_type: 400 + i, boss_type: null, target_kind: 'boss' }))];
  h.client.getRun.mockResolvedValue(ready({ ...story, fights: [fights[1], ...fights] }));
  h.controller.update(h.input); await settle();
  expect(h.client.getHomeBossBaselines.mock.calls[0][0].target_types).toEqual([301, ...Array.from({ length: 63 }, (_, i) => 400 + i)]);
});

it('unknown_character_skips_personal_baselines_and_profile_but_reads_streak', async () => {
  const h = harness(); h.client.getRun.mockResolvedValue(ready({ ...story, run: { ...story.run, character_id: null } }));
  h.controller.update(h.input); await settle();
  expect(h.client.getDungeonProfile).not.toHaveBeenCalled(); expect(h.client.getHomeBossBaselines).not.toHaveBeenCalled();
  expect(h.client.getHomeDungeonStreak).toHaveBeenCalledExactlyOnceWith({ dungeon: story.run.dungeon });
  expect(h.last()).toMatchObject({ status: 'saved', data: { profile: null, baselines: null } });
});

it('slow_reads_time_out_then_upgrade_without_a_second_request', async () => {
  const h = harness(), pending = deferred<HistoryReply<RunStory | null>>(); h.client.getRun.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await vi.advanceTimersByTimeAsync(5000);
  expect(h.last()).toMatchObject({ status: 'partial', reason: 'timeout' });
  pending.resolve(ready(story)); await settle(); expect(h.last()?.status).toBe('saved'); expect(h.client.getRun).toHaveBeenCalledOnce();
});

it('store_failure_during_reads_drops_the_reply_until_retry', async () => {
  const h = harness(), pending = deferred<HistoryReply<RunStory | null>>(); h.client.getRun.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle(); h.controller.update({ ...h.input, store: { ...saved('13'), state: 'failed' } });
  pending.resolve(ready(story)); await settle(); expect(h.last()).toMatchObject({ status: 'partial', reason: 'failed' });
  expect(h.client.getRunRecap).not.toHaveBeenCalled();
  h.controller.update(h.input); h.controller.retry(); await settle(); expect(h.last()?.status).toBe('saved');
});

it.each(['run', 'visit'] as const)('rejected_%s_is_partial_with_a_safe_message', async kind => {
  const h = harness(); h.client[kind === 'run' ? 'getRun' : 'getVisitRecap'].mockRejectedValue(new Error('synthetic private detail'));
  h.controller.update({ ...h.input, recap: kind === 'run' ? recap : visitSource }); await settle();
  expect(h.last()).toEqual({ status: 'partial', reason: 'failed', message: 'Saved history could not be loaded. Retry to refresh it.' });
});

it('pauses recap SQL and the save timeout while hidden', async () => {
  const h = harness();
  try {
    h.controller.update({ ...h.input, store: saved('12') });
    await vi.advanceTimersByTimeAsync(2000);
    setNativePresentationVisible(false);
    await vi.advanceTimersByTimeAsync(10000);
    expect(h.last()).toEqual({ status: 'saving' });
    expect(h.client.getRun).not.toHaveBeenCalled();
    setNativePresentationVisible(true);
    await vi.advanceTimersByTimeAsync(2999);
    expect(h.last()).toEqual({ status: 'saving' });
    await vi.advanceTimersByTimeAsync(1);
    expect(h.last()).toMatchObject({ status: 'partial', reason: 'timeout' });
    setNativePresentationVisible(false);
    h.controller.update(h.input);
    await settle();
    expect(h.client.getRun).not.toHaveBeenCalled();
    setNativePresentationVisible(true);
    await settle();
    expect(h.last()?.status).toBe('saved');
  } finally { setNativePresentationVisible(true); }
});

it('pauses between recap SQL commands and releases a hidden waiter on disposal', async () => {
  const h = harness();
  const pending = deferred<HistoryReply<RunStory | null>>();
  h.client.getRun.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle();
  setNativePresentationVisible(false);
  try {
    pending.resolve(ready(story)); await settle();
    expect(h.client.getRunRecap).not.toHaveBeenCalled();
    h.controller.dispose();
    setNativePresentationVisible(true); await settle();
    expect(h.client.getRunRecap).not.toHaveBeenCalled();
    expect(h.last()).toBeNull();
  } finally { setNativePresentationVisible(true); }
});
