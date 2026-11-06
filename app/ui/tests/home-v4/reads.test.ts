import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { createClient } from '../../src/lib/bridge/client';
import { createFixtureTransport } from '../../src/lib/bridge/fixture';
import { setNativePresentationVisible } from '../../src/lib/bridge/visibility';
import type { HomeBossBaseline } from '../../src/lib/bindings/HomeBossBaseline';
import type { HomeBossBaselines } from '../../src/lib/bindings/HomeBossBaselines';
import type { DungeonProfile } from '../../src/lib/bindings/DungeonProfile';
import type { EventEnvelope } from '../../src/lib/bindings/EventEnvelope';
import type { HistoryReply } from '../../src/lib/bindings/HistoryReply';
import type { HomeHud } from '../../src/lib/bindings/HomeHud';
import type { HomeDungeonStreak } from '../../src/lib/bindings/HomeDungeonStreak';
import { createHomeV4Reads, USUAL_WINDOW_MICROS, type HomeV4ReadInput, type HomeV4ReadState } from '../../src/lib/home/v4/reads';
import { deferred, homeSnapshot } from '../home/fixtures';

const disposals: Array<() => void> = [];
beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => {
  disposals.splice(0).forEach(dispose => dispose());
  setNativePresentationVisible(true); documentHidden(false);
  vi.useRealTimers(); vi.restoreAllMocks();
});
function documentHidden(hidden: boolean) {
  Object.defineProperty(document, 'visibilityState', { configurable: true, value: hidden ? 'hidden' : 'visible' });
  document.dispatchEvent(new Event('visibilitychange'));
}
const settle = () => vi.advanceTimersByTimeAsync(0);
const hud: HomeHud = { fame_per_hour: null, combat: null, whites: 2, ut_st: [], ut_st_count: 0, potions: [] };
const streak: HomeDungeonStreak = { dungeon: 'Synthetic Sanctum', runs_ended: 46,
  // Ordinary read tests model a caught-up writer; race tests override this timestamp.
  runs_since_last_white: 23, avg_runs_per_white: 23, last_white_at: homeSnapshot().drops[0].at };
const taggedStreak = { generation: 'synthetic-run', value: streak };
const profile = (dungeon: string): DungeonProfile => ({ dungeon, clears: 3, bosses: [],
  usual_run_dps: null, run_dps_samples: 0, usual_clear_micros: null, clear_samples: 0 });
const boss = (target_type: number): HomeBossBaseline => ({ target_type, fights: 3,
  avg_dps: 250, best_dps: 300,
  kill_samples: 0, median_kill_micros: null });
const event = (value: EventEnvelope['event']): EventEnvelope => ({
  stream_id: 'live-synthetic-home', sequence: '12', at: '1791806400000000', event: value,
});
const runEnd = () => event({ run_ended: { ...homeSnapshot().run!, ended: '1791806400000000' } });
const drop = () => event({ bag_dropped: { ...homeSnapshot().drops[0], bag_rank: 255 } });

it('non_white_bursts_enrichment_and_session_updates_refresh_only_the_hud', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.calls.hud.mockClear(); h.calls.streak.mockClear();
  const bag = { ...homeSnapshot().drops[0], bag_rank: 10 };
  for (let i = 0; i < 5; i++) {
    h.controller.onEvent(event({ bag_dropped: { ...bag, id: `ordinary-${i}` } }));
    h.controller.onEvent(event({ drop_updated: { ...bag, id: `ordinary-${i}`, source_enemy: 123 } }));
    await vi.advanceTimersByTimeAsync(90);
  }
  h.controller.onEvent(event({ session_updated: homeSnapshot().session! }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.hud).toHaveBeenCalledOnce(); expect(h.calls.streak).not.toHaveBeenCalled();
});

it('white_membership_changes_refresh_once_but_unchanged_white_enrichment_does_not', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.calls.streak.mockClear();
  const bag = { ...homeSnapshot().drops[0], bag_rank: 255 };
  h.controller.onEvent(event({ bag_dropped: bag })); await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.streak).toHaveBeenCalledOnce();
  h.calls.streak.mockClear();
  h.controller.onEvent(event({ drop_updated: { ...bag, source_enemy: 777, contents_known: true } }));
  await vi.advanceTimersByTimeAsync(500); expect(h.calls.streak).not.toHaveBeenCalled();
  for (const changed of [{ ...bag, run_id: 'different-run' }, { ...bag, at: '123' }, { ...bag, bag_rank: 10 }]) {
    h.controller.onEvent(event({ drop_updated: changed })); await vi.advanceTimersByTimeAsync(500);
    expect(h.calls.streak).toHaveBeenCalledOnce(); h.calls.streak.mockClear();
  }
});

it('restore_without_invalidation_does_not_read_again', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  setNativePresentationVisible(false); await vi.advanceTimersByTimeAsync(5000);
  setNativePresentationVisible(true); await settle();
  for (const call of Object.values(h.calls)) expect(call).toHaveBeenCalledOnce();
});

it('snapshot_white_membership_seeds_enrichment_without_a_refresh', async () => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], bag_rank: 255 };
  h.controller.update({ ...h.input, drops: [bag] }); await settle();
  h.controller.onEvent(event({ drop_updated: { ...bag, source_enemy: 777 } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.streak).toHaveBeenCalledOnce(); expect(h.calls.hud).toHaveBeenCalledTimes(2);
  h.controller.onEvent(event({ drop_updated: { ...bag, bag_rank: 10 } }));
  await vi.advanceTimersByTimeAsync(500); expect(h.calls.streak).toHaveBeenCalledTimes(2);
});

it('remembers_white_membership_for_late_reclassification_after_run_entry', async () => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], bag_rank: 255 };
  h.controller.update(h.input); await settle();
  h.controller.onEvent(event({ bag_dropped: bag })); await vi.advanceTimersByTimeAsync(500);
  h.controller.update({ ...h.input, generation: 'synthetic-run-B' }); await settle();
  h.calls.streak.mockClear();
  h.controller.onEvent(event({ drop_updated: { ...bag, bag_rank: 10 } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.streak).toHaveBeenCalledOnce();
});

it.each(['synthetic-previous-run', null])('a_late_white_from_run_%s_does_not_gate_current_dungeon_replies', async run_id => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], run_id, bag_rank: 10, at: '9007199254740993' };
  h.controller.update({ ...h.input, dungeon: 'Synthetic Crypt', generation: 'synthetic-previous-generation' }); await settle();
  h.controller.onEvent(event({ bag_dropped: bag })); await vi.advanceTimersByTimeAsync(500);
  h.controller.update(h.input); await settle();
  h.calls.streak.mockClear();
  const updated = { ...streak, runs_ended: 47 };
  h.calls.streak.mockResolvedValue({ status: 'ready', value: updated });
  h.controller.onEvent(event({ drop_updated: { ...bag, bag_rank: 255 } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value).toEqual(updated);
  await vi.advanceTimersByTimeAsync(5000); expect(h.calls.streak).toHaveBeenCalledOnce();
  const ended = { ...updated, runs_ended: 48 };
  h.calls.streak.mockResolvedValue({ status: 'ready', value: ended });
  h.controller.onEvent(runEnd()); await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value).toEqual(ended);
  h.calls.streak.mockResolvedValue({ status: 'ready', value: { ...ended, runs_ended: 49 } });
  h.controller.onEvent(event({ session_updated: homeSnapshot().session! })); await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value.runs_ended).toBe(49);
  await vi.advanceTimersByTimeAsync(5000); expect(h.calls.streak).toHaveBeenCalledTimes(3);
});

it('a_satisfied_white_allows_a_run_end_reply_with_the_same_timestamp', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.controller.onEvent(drop()); await vi.advanceTimersByTimeAsync(500);
  const ended = { ...streak, runs_ended: 47 };
  h.calls.streak.mockResolvedValue({ status: 'ready', value: ended });
  h.controller.onEvent(runEnd()); await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value).toEqual(ended);
  await vi.advanceTimersByTimeAsync(5000); expect(h.calls.streak).toHaveBeenCalledTimes(3);
});

it('a_satisfied_checkpoint_does_not_gate_a_later_older_white', async () => {
  const h = harness(), newer = '9007199254740993', older = '9007199254740992';
  h.controller.update(h.input); await settle();
  h.calls.streak.mockResolvedValueOnce({ status: 'ready', value: { ...streak, last_white_at: newer } });
  h.controller.onEvent(event({ bag_dropped: { ...homeSnapshot().drops[0], bag_rank: 255, at: newer } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value.last_white_at).toBe(newer);
  // Once satisfied, that checkpoint must not outlive subsequent history corrections.
  const corrected = { ...streak, last_white_at: older, runs_ended: 47 };
  h.calls.streak.mockResolvedValue({ status: 'ready', value: corrected });
  h.controller.onEvent(event({ drop_updated: {
    ...homeSnapshot().drops[0], id: 'synthetic-older-white', bag_rank: 255, at: older } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value).toEqual(corrected);
  await vi.advanceTimersByTimeAsync(5000); expect(h.calls.streak).toHaveBeenCalledTimes(3);
});

it.each(['event-first', 'snapshot-first'] as const)('in_dungeon_death_%s_invalidates_profile_without_reading_until_the_next_run', async order => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.calls.profile.mockClear(); h.calls.streak.mockClear();
  if (order === 'event-first') h.controller.onEvent(runEnd());
  h.controller.update({ ...h.input, profileDungeon: null });
  if (order === 'snapshot-first') h.controller.onEvent(runEnd());
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.streak).toHaveBeenCalledOnce();
  expect(h.calls.profile).not.toHaveBeenCalled();
  h.controller.update({ ...h.input, generation: 'synthetic-next-run' }); await settle();
  expect(h.calls.profile).toHaveBeenCalledOnce();
});

it.each([null, '9007199254740992'])('retries_a_white_reply_with_stale_last_white_at_%s_after_the_debounce', async last_white_at => {
  const h = harness(), at = '9007199254740993';
  h.controller.update(h.input); await settle();
  h.controller.consumeStreak(h.input.generation!);
  const consumed = h.states.length;
  h.calls.streak.mockResolvedValueOnce({ status: 'ready', value: { ...streak, last_white_at } })
    .mockResolvedValueOnce({ status: 'ready', value: { ...streak, last_white_at: at, runs_since_last_white: 0 } });
  h.controller.onEvent(event({ bag_dropped: { ...homeSnapshot().drops[0], bag_rank: 255, at } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
  expect(h.states.slice(consumed).some(state => state.streak?.value.runs_since_last_white === 23)).toBe(false);
  await vi.advanceTimersByTimeAsync(499); expect(h.calls.streak).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1);
  expect(h.calls.streak).toHaveBeenCalledTimes(3);
  expect(h.last().streak?.value.last_white_at).toBe(at);
  await vi.advanceTimersByTimeAsync(5000);
  expect(h.calls.streak).toHaveBeenCalledTimes(3);
  expect(h.calls.hud).toHaveBeenCalledTimes(2);
});

it('caps_stale_streak_retries_at_three_per_white', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.calls.streak.mockClear().mockResolvedValue({ status: 'ready', value: streak });
  for (const at of ['9007199254740993', '9007199254740994']) {
    h.controller.consumeStreak(h.input.generation!);
    h.controller.onEvent(event({ bag_dropped: { ...homeSnapshot().drops[0], id: `synthetic-white-${at}`, bag_rank: 255, at } }));
    for (let attempt = 1; attempt <= 4; attempt++) {
      await vi.advanceTimersByTimeAsync(500);
      expect(h.calls.streak).toHaveBeenCalledTimes(attempt);
    }
    await vi.advanceTimersByTimeAsync(10_000);
    expect(h.calls.streak).toHaveBeenCalledTimes(4);
    expect(h.last().streak?.value.runs_since_last_white).toBe(0);
    h.calls.streak.mockClear();
  }
});

it('an_older_white_update_cannot_lower_the_pending_white_timestamp', async () => {
  const h = harness(), newer = '9007199254740993', older = '9007199254740992';
  h.controller.update(h.input); await settle();
  h.controller.consumeStreak(h.input.generation!);
  h.calls.streak.mockResolvedValueOnce({ status: 'ready', value: { ...streak, last_white_at: older } })
    .mockResolvedValueOnce({ status: 'ready', value: { ...streak, last_white_at: newer, runs_since_last_white: 0 } });
  h.controller.onEvent(event({ bag_dropped: { ...homeSnapshot().drops[0], id: 'synthetic-new-white', bag_rank: 255, at: newer } }));
  h.controller.onEvent(event({ drop_updated: { ...homeSnapshot().drops[0], id: 'synthetic-old-white', bag_rank: 255, at: older } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.streak).toHaveBeenCalledTimes(3);
  expect(h.last().streak?.value.last_white_at).toBe(newer);
});

it('correcting_the_same_whites_timestamp_does_not_reset_its_retry_budget', async () => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], bag_rank: 255, at: '9007199254740993' };
  h.controller.update(h.input); await settle();
  h.controller.consumeStreak(h.input.generation!);
  h.controller.onEvent(event({ bag_dropped: bag }));
  await vi.advanceTimersByTimeAsync(2000);
  expect(h.calls.streak).toHaveBeenCalledTimes(5);
  h.controller.onEvent(event({ drop_updated: { ...bag, at: '9007199254740994' } }));
  await vi.advanceTimersByTimeAsync(10_000);
  // The membership change requests one read, but this white already used its three retries.
  expect(h.calls.streak).toHaveBeenCalledTimes(6);
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
});

it.each(['9007199254740993', '9007199254740994'])('reclassifying_the_newer_white_retries_until_the_remaining_white_is_saved_at_%s', async savedAt => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], bag_rank: 255 };
  const older = { ...bag, id: 'synthetic-older-white', at: '9007199254740993' };
  const newer = { ...bag, id: 'synthetic-newer-white', at: '9007199254740995' };
  h.controller.update(h.input); await settle();
  h.controller.onEvent(event({ bag_dropped: older }));
  h.controller.onEvent(event({ bag_dropped: newer }));
  h.controller.consumeStreak(h.input.generation!);
  const consumed = h.states.length;
  const saved = { ...streak, last_white_at: savedAt, runs_since_last_white: 0, runs_ended: 47 };
  h.calls.streak.mockResolvedValueOnce({ status: 'ready', value: { ...streak, last_white_at: '9007199254740992' } })
    .mockResolvedValueOnce({ status: 'ready', value: saved });
  h.controller.onEvent(event({ drop_updated: { ...newer, bag_rank: 10 } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
  expect(h.states.slice(consumed).some(state => state.streak?.value.runs_since_last_white === 23)).toBe(false);
  await vi.advanceTimersByTimeAsync(499); expect(h.calls.streak).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1);
  expect(h.calls.streak).toHaveBeenCalledTimes(3);
  expect(h.last().streak?.value).toEqual(saved);
  await vi.advanceTimersByTimeAsync(5000); expect(h.calls.streak).toHaveBeenCalledTimes(3);
});

it('reclassification_selects_the_newest_remaining_current_run_white_by_bigint_timestamp', async () => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], bag_rank: 255 };
  const remaining = { ...bag, id: 'synthetic-remaining-white', at: '9007199254740993' };
  const newest = { ...bag, id: 'synthetic-newest-white', at: '9007199254740994' };
  h.controller.update(h.input); await settle();
  for (const white of [remaining, { ...bag, id: 'synthetic-older-white', at: '9007199254740992' },
    { ...bag, id: 'synthetic-foreign-white', run_id: 'synthetic-other-run', at: '9007199254740996' },
    { ...bag, id: 'synthetic-unassigned-white', run_id: null, at: '9007199254740997' }, newest]) {
    h.controller.onEvent(event({ bag_dropped: white }));
  }
  h.controller.consumeStreak(h.input.generation!);
  const saved = { ...streak, last_white_at: remaining.at, runs_since_last_white: 0, runs_ended: 47 };
  h.calls.streak.mockResolvedValueOnce({ status: 'ready', value: { ...streak, last_white_at: '9007199254740992' } })
    .mockResolvedValueOnce({ status: 'ready', value: saved });
  h.controller.onEvent(event({ drop_updated: { ...newest, bag_rank: 10 } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
  await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value).toEqual(saved);
  await vi.advanceTimersByTimeAsync(5000); expect(h.calls.streak).toHaveBeenCalledTimes(3);
});

it('falling_back_to_a_remaining_white_preserves_the_pending_retry', async () => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], bag_rank: 255, at: '9007199254740993' };
  const newer = { ...bag, id: 'synthetic-newer-white', at: '9007199254740994' };
  h.controller.update(h.input); await settle();
  h.controller.onEvent(event({ bag_dropped: bag }));
  h.controller.onEvent(event({ bag_dropped: newer }));
  h.controller.consumeStreak(h.input.generation!);
  await vi.advanceTimersByTimeAsync(600);
  expect(h.calls.streak).toHaveBeenCalledTimes(2);
  h.controller.onEvent(event({ drop_updated: { ...newer, bag_rank: 10 } }));
  await vi.advanceTimersByTimeAsync(399); expect(h.calls.streak).toHaveBeenCalledTimes(2);
  // The pending retry is due before the reclassification's debounced refresh.
  await vi.advanceTimersByTimeAsync(1);
  expect(h.calls.streak).toHaveBeenCalledTimes(3);
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
});

it('falling_back_to_a_remaining_white_starts_a_fresh_three_retry_budget', async () => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], bag_rank: 255, at: '9007199254740993' };
  const newer = { ...bag, id: 'synthetic-newer-white', at: '9007199254740994' };
  h.controller.update(h.input); await settle();
  h.controller.onEvent(event({ bag_dropped: bag }));
  h.controller.onEvent(event({ bag_dropped: newer }));
  h.controller.consumeStreak(h.input.generation!);
  await vi.advanceTimersByTimeAsync(2000);
  expect(h.calls.streak).toHaveBeenCalledTimes(5);
  h.calls.streak.mockClear();
  h.controller.onEvent(event({ drop_updated: { ...newer, bag_rank: 10 } }));
  for (let attempt = 1; attempt <= 4; attempt++) {
    await vi.advanceTimersByTimeAsync(500);
    expect(h.calls.streak).toHaveBeenCalledTimes(attempt);
    expect(h.last().streak?.value.runs_since_last_white).toBe(0);
  }
  await vi.advanceTimersByTimeAsync(5000); expect(h.calls.streak).toHaveBeenCalledTimes(4);
});

it('reclassifying_the_only_current_run_white_clears_the_check_despite_foreign_whites', async () => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], bag_rank: 255, at: '9007199254740993' };
  h.controller.update(h.input); await settle();
  for (const run_id of ['synthetic-other-run', null]) {
    h.controller.onEvent(event({ bag_dropped: { ...bag, id: `synthetic-foreign-${run_id}`, run_id } }));
  }
  h.controller.onEvent(event({ bag_dropped: bag }));
  h.controller.consumeStreak(h.input.generation!);
  await vi.advanceTimersByTimeAsync(600);
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
  h.controller.onEvent(event({ drop_updated: { ...bag, bag_rank: 10 } }));
  await vi.advanceTimersByTimeAsync(400); expect(h.calls.streak).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(100);
  expect(h.last().streak?.value).toEqual(streak);
  await vi.advanceTimersByTimeAsync(5000); expect(h.calls.streak).toHaveBeenCalledTimes(3);
});

it('run_end_reads_streak_once_more_on_the_next_hud_refresh', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.controller.onEvent(runEnd()); h.controller.update({ ...h.input, profileDungeon: null });
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.streak).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(30_000);
  expect(h.calls.streak).toHaveBeenCalledTimes(2);
  h.calls.streak.mockResolvedValueOnce({ status: 'ready', value: { ...streak, runs_ended: 47, runs_since_last_white: 24 } });
  h.controller.onEvent(event({ session_updated: homeSnapshot().session! }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.streak).toHaveBeenCalledTimes(3);
  expect(h.last().streak?.value.runs_ended).toBe(47);
  h.controller.onEvent(event({ session_updated: homeSnapshot().session! }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.streak).toHaveBeenCalledTimes(3);
});

it.each(['generation', 'dungeon', 'dispose', 'reclassification'] as const)('cancels_a_stale_white_retry_on_%s', async change => {
  const h = harness(), bag = { ...homeSnapshot().drops[0], bag_rank: 255, at: '9007199254740993' };
  h.controller.update(h.input); await settle();
  h.controller.consumeStreak(h.input.generation!);
  h.controller.onEvent(event({ bag_dropped: bag }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
  if (change === 'dispose') h.controller.dispose();
  else if (change === 'reclassification') h.controller.onEvent(event({ drop_updated: { ...bag, bag_rank: 10 } }));
  else h.controller.update({ ...h.input, [change]: 'synthetic-next' });
  await vi.advanceTimersByTimeAsync(5000);
  expect(h.calls.streak).toHaveBeenCalledTimes(change === 'dispose' ? 2 : 3);
  if (change !== 'dispose') expect(h.last().streak?.value.runs_since_last_white).toBe(23);
});

it.each(['events', 'snapshot'] as const)('white_memberships_evict_oldest_above_512_from_%s', async seed => {
  const h = harness();
  const bags = Array.from({ length: 513 }, (_, i) => ({ ...homeSnapshot().drops[0], id: `synthetic-white-${i}`, bag_rank: 255 }));
  h.controller.update({ ...h.input, drops: seed === 'snapshot' ? bags : [] }); await settle();
  if (seed === 'events') {
    for (const bag of bags) h.controller.onEvent(event({ bag_dropped: bag }));
    await vi.advanceTimersByTimeAsync(500);
  }
  h.controller.update({ ...h.input, generation: 'synthetic-next-run' }); await settle();
  h.calls.streak.mockClear();
  h.controller.onEvent(event({ drop_updated: { ...bags[0], bag_rank: 10 } }));
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.streak).not.toHaveBeenCalled();
  for (const bag of [bags[1], bags[512]]) {
    h.controller.onEvent(event({ drop_updated: { ...bag, bag_rank: 10 } }));
    await vi.advanceTimersByTimeAsync(500);
    expect(h.calls.streak).toHaveBeenCalledOnce(); h.calls.streak.mockClear();
  }
});

it('consuming_a_streak_discards_in_flight_values_until_a_fresh_read', async () => {
  const h = harness(), pending = deferred<HistoryReply<HomeDungeonStreak>>();
  h.controller.update(h.input); await settle();
  h.calls.streak.mockReturnValueOnce(pending.promise);
  h.controller.onEvent(runEnd()); await vi.advanceTimersByTimeAsync(500);
  h.controller.consumeStreak(h.input.generation!);
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
  pending.resolve({ status: 'ready', value: streak }); await settle();
  expect(h.last().streak?.value.runs_since_last_white).toBe(0);
  h.controller.onEvent(runEnd()); await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak?.value.runs_since_last_white).toBe(23);
});

it.each(['hud', 'streak'] as const)('%s_failure_does_not_mark_the_other_owner_failed', async owner => {
  const h = harness(); h.calls[owner].mockRejectedValueOnce(new Error('synthetic'));
  h.controller.update(h.input); await settle();
  expect(h.last().errors[owner]).toBe(true);
  const other = owner === 'hud' ? 'streak' : 'hud';
  expect(h.last().errors[other]).toBe(false); expect(h.last()[other]).not.toBeNull();
});

it.each(['dungeon', 'generation'] as const)('%s_change_discards_obsolete_streak_reply_and_fetches_once', async field => {
  const h = harness(), pending = deferred<HistoryReply<HomeDungeonStreak>>();
  h.calls.streak.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle();
  const next = { ...h.input, [field]: 'synthetic-next' };
  h.controller.update(next);
  expect(h.last().streak).toBeNull();
  pending.resolve({ status: 'ready', value: { ...streak, runs_ended: 999 } }); await settle();
  expect(h.calls.streak).toHaveBeenCalledTimes(2);
  expect(h.calls.streak).toHaveBeenLastCalledWith({ dungeon: next.dungeon });
  expect(h.states.some(state => state.streak?.value.runs_ended === 999)).toBe(false);
  expect(h.last().streak).toEqual({ generation: next.generation, value: { ...streak, dungeon: next.dungeon } });
});

it('leaving_a_dungeon_clears_streak_and_ignores_events_and_old_replies', async () => {
  const h = harness(), pending = deferred<HistoryReply<HomeDungeonStreak>>();
  h.calls.streak.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle();
  h.controller.update({ ...h.input, dungeon: null, generation: null });
  pending.resolve({ status: 'ready', value: streak }); await settle();
  h.controller.onEvent(drop()); h.controller.onEvent(runEnd()); await vi.advanceTimersByTimeAsync(500);
  expect(h.last().streak).toBeNull(); expect(h.calls.streak).toHaveBeenCalledOnce();
});

it.each(['native', 'document'] as const)('pauses all v4 store reads while %s is hidden and resumes on restore', async kind => {
  const visible = (value: boolean) => kind === 'native' ? setNativePresentationVisible(value) : documentHidden(!value);
  visible(false);
  const h = harness(); h.controller.update(h.input);
  await vi.advanceTimersByTimeAsync(5000);
  for (const read of Object.values(h.calls)) expect(read).not.toHaveBeenCalled();
  visible(true); await settle();
  for (const read of Object.values(h.calls)) expect(read).toHaveBeenCalledOnce();
  expect(h.last()).toMatchObject({ hud, streak: taggedStreak, profile: profile(h.input.dungeon!) });
  expect([...h.last().usual.keys()]).toEqual([100, 200]);
});

it('disposes hidden waiters without issuing reads or delivering results after restore', async () => {
  setNativePresentationVisible(false);
  const removed = vi.spyOn(document, 'removeEventListener');
  const h = harness(); h.controller.update(h.input); await settle();
  const delivered = h.states.length;
  h.controller.dispose(); await settle();
  expect(removed).toHaveBeenCalledWith('visibilitychange', expect.any(Function));
  setNativePresentationVisible(true); await vi.advanceTimersByTimeAsync(5000);
  for (const read of Object.values(h.calls)) expect(read).not.toHaveBeenCalled();
  expect(h.states).toHaveLength(delivered);
});

it('pauses queued reads and an in-flight HUD follow-up until restore', async () => {
  const pending = deferred<HistoryReply<HomeHud>>();
  const h = harness(); h.calls.hud.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle();
  expect(h.calls.hud).toHaveBeenCalledOnce();
  setNativePresentationVisible(false);
  h.controller.onEvent(runEnd()); await vi.advanceTimersByTimeAsync(2000);
  pending.resolve({ status: 'ready', value: hud }); await settle();
  expect(h.last().hud).toEqual(hud);
  expect(h.calls.hud).toHaveBeenCalledOnce();
  for (const read of [h.calls.streak, h.calls.profile, h.calls.usual]) expect(read).not.toHaveBeenCalled();
  setNativePresentationVisible(true); await settle();
  expect(h.calls.hud).toHaveBeenCalledTimes(2); expect(h.calls.streak).toHaveBeenCalledTimes(2);
  expect(h.calls.profile).toHaveBeenCalledOnce(); expect(h.calls.usual).toHaveBeenCalledOnce();
});

it('waits before reissuing a superseded request if the presentation became hidden', async () => {
  const h = harness();
  h.calls.hud.mockImplementationOnce(async () => { setNativePresentationVisible(false); return { status: 'superseded' }; });
  h.controller.update(h.input); await vi.advanceTimersByTimeAsync(5000);
  expect(h.calls.hud).toHaveBeenCalledOnce();
  for (const read of [h.calls.streak, h.calls.profile, h.calls.usual]) expect(read).not.toHaveBeenCalled();
  setNativePresentationVisible(true); await settle();
  expect(h.calls.hud).toHaveBeenCalledTimes(2); expect(h.last().hud).toEqual(hud);
});
function harness(receive?: (state: HomeV4ReadState) => void) {
  const client = createClient(createFixtureTransport(homeSnapshot(), undefined, undefined, undefined,
    undefined, undefined, undefined, {}, undefined, undefined, {
      homeDungeonStreak: request => ({ ...streak, dungeon: request.dungeon }), homeHud: () => hud, dungeonProfile: request => profile(request.dungeon),
      homeBossBaselines: request => ({ rows: (request.target_types ?? []).map(boss), truncated: false }),
    }));
  vi.spyOn(client, 'getBossHistory').mockImplementation(() => { throw new Error('Home must use lean boss baselines'); });
  vi.spyOn(client, 'getLootStats').mockImplementation(() => { throw new Error('Home must not read full Loot stats'); });
  const calls = { hud: vi.spyOn(client, 'getHomeHud'), streak: vi.spyOn(client, 'getHomeDungeonStreak'),
    profile: vi.spyOn(client, 'getDungeonProfile'), usual: vi.spyOn(client, 'getHomeBossBaselines') };
  const states: HomeV4ReadState[] = [];
  const controller = createHomeV4Reads(client, state => { states.push(state); receive?.(state); });
  disposals.push(controller.dispose);
  const input: HomeV4ReadInput = { scope: { session: { id: 'synthetic-session' } },
    runId: homeSnapshot().run!.id,
    dungeon: 'Synthetic Sanctum', profileDungeon: 'Synthetic Sanctum', generation: 'synthetic-run', characterId: 'synthetic-character', bossTypes: [100, 200], nowMicros: 1_791_806_400_000_000 };
  return { controller, calls, states, input, last: () => states.at(-1)! };
}

it('hud_refetches_on_run_end_not_on_snapshot_updates', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  for (let i = 1; i <= 20; i++) h.controller.update({ ...h.input, scope: structuredClone(h.input.scope), nowMicros: h.input.nowMicros + i });
  await vi.advanceTimersByTimeAsync(5000);
  expect(h.calls.hud).toHaveBeenCalledOnce(); expect(h.calls.streak).toHaveBeenCalledOnce();
  h.controller.onEvent(runEnd()); await vi.advanceTimersByTimeAsync(499);
  expect(h.calls.hud).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(1);
  expect(h.calls.hud).toHaveBeenCalledTimes(2); expect(h.calls.streak).toHaveBeenCalledTimes(2);
  expect(h.calls.streak).toHaveBeenLastCalledWith({ dungeon: h.input.dungeon });
  expect(h.calls.profile).toHaveBeenCalledTimes(2);
  expect(h.calls.usual).toHaveBeenCalledOnce();
});

it('hud_debounces_bursts_of_drop_events', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  for (let i = 0; i < 5; i++) { h.controller.onEvent(drop()); await vi.advanceTimersByTimeAsync(90); }
  expect(h.calls.hud).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(410);
  expect(h.calls.hud).toHaveBeenCalledTimes(2); expect(h.calls.streak).toHaveBeenCalledTimes(2);
  expect(h.calls.profile).toHaveBeenCalledOnce();
});

it('profile_requested_once_per_dungeon_and_character', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  const range = { start: String(h.input.nowMicros - 7_776_000_000_000), end: String(h.input.nowMicros) };
  expect(USUAL_WINDOW_MICROS).toBe(7_776_000_000_000);
  expect(h.calls.profile).toHaveBeenLastCalledWith({ dungeon: h.input.dungeon, character_id: h.input.characterId, range });
  h.controller.update({ ...h.input, nowMicros: h.input.nowMicros + 1 }); await settle();
  expect(h.calls.profile).toHaveBeenCalledOnce();
  h.controller.update({ ...h.input, dungeon: 'Synthetic Crypt', profileDungeon: 'Synthetic Crypt' }); await settle();
  h.controller.update(h.input); await settle();
  expect(h.calls.profile).toHaveBeenCalledTimes(2); expect(h.last().profile?.dungeon).toBe(h.input.dungeon);
  h.controller.update({ ...h.input, characterId: 'synthetic-character-2' }); await settle();
  expect(h.calls.profile).toHaveBeenCalledTimes(3);
});

it('usual_batches_only_new_boss_types', async () => {
  const h = harness(); h.controller.update({ ...h.input, bossTypes: [100, 200, 100] }); await settle();
  expect(h.calls.usual).toHaveBeenCalledOnce();
  expect(h.calls.usual).toHaveBeenLastCalledWith({ character_id: h.input.characterId, target_types: [100, 200],
    range: { start: String(h.input.nowMicros - USUAL_WINDOW_MICROS), end: String(h.input.nowMicros) } });
  const previous = h.last().usual;
  h.controller.update({ ...h.input, bossTypes: [200, 100, 300, 400] }); await settle();
  expect(h.calls.usual).toHaveBeenCalledTimes(2);
  expect(h.calls.usual.mock.lastCall![0].target_types).toEqual([300, 400]);
  expect([...h.last().usual.keys()]).toEqual([100, 200, 300, 400]);
  expect([...previous.keys()]).toEqual([100, 200]);
  h.controller.update({ ...h.input, bossTypes: [400, 100] }); await settle();
  expect(h.calls.usual).toHaveBeenCalledTimes(2);
});

it('usual_cache_resets_on_character_change', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.controller.update({ ...h.input, characterId: 'synthetic-character-2', bossTypes: [200] });
  expect(h.last().usual.size).toBe(0); await settle();
  expect(h.calls.usual).toHaveBeenCalledTimes(2);
  expect(h.calls.usual.mock.lastCall![0]).toMatchObject({ character_id: 'synthetic-character-2', target_types: [200] });
  expect([...h.last().usual.keys()]).toEqual([200]);
});

it('read_error_keeps_previous_values', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  const previous = h.last();
  h.calls.hud.mockRejectedValueOnce(new Error('synthetic'));
  h.calls.streak.mockRejectedValueOnce(new Error('synthetic'));
  h.calls.profile.mockRejectedValueOnce(new Error('synthetic'));
  h.calls.usual.mockRejectedValueOnce(new Error('synthetic')).mockRejectedValueOnce(new Error('synthetic'));
  h.controller.onEvent(runEnd());
  h.controller.update({ ...h.input, bossTypes: [100, 200, 300] });
  await vi.advanceTimersByTimeAsync(500);
  await settle();
  expect(h.last()).toEqual({ ...previous, failedUsualTypes: new Set([300]), errors: { hud: true, streak: true, profile: true, usual: true } });
  h.controller.onEvent(runEnd()); await vi.advanceTimersByTimeAsync(500);
  expect(h.last().errors).toEqual({ hud: false, streak: false, profile: false, usual: false });
});

it('retries_failed_usual_types_on_refresh_and_preserves_successful_types', async () => {
  const h = harness(); h.controller.update({ ...h.input, bossTypes: [100] }); await settle();
  h.calls.usual.mockRejectedValueOnce(new Error('synthetic'));
  h.controller.update(h.input); await settle();
  expect(h.last().usual.get(100)).toEqual(boss(100));
  expect(h.last().usual.has(200)).toBe(false);
  expect(h.last().failedUsualTypes).toEqual(new Set([200]));
  const failed = h.last();
  h.controller.onEvent(drop()); await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.usual).toHaveBeenCalledTimes(3);
  expect(h.calls.usual.mock.lastCall![0].target_types).toEqual([200]);
  expect(h.last().usual.get(200)).toEqual(boss(200));
  expect(h.last().failedUsualTypes.size).toBe(0);
  expect(h.last().errors.usual).toBe(false);
  expect(failed.failedUsualTypes).toEqual(new Set([200]));
});

it.each(['hud', 'streak', 'profile', 'usual'] as const)('%s_retries_busy_once_after_two_seconds', async read => {
  const h = harness(); h.calls[read].mockResolvedValue({ status: 'busy' });
  h.controller.update(h.input); await settle();
  expect(h.calls[read]).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(1999); expect(h.calls[read]).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(1); expect(h.calls[read]).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(10_000); expect(h.calls[read]).toHaveBeenCalledTimes(2);
  expect(h.last().errors[read]).toBe(true);
});

it('successful_busy_retry_publishes_value', async () => {
  const h = harness(); h.calls.hud.mockResolvedValueOnce({ status: 'busy' });
  h.controller.update(h.input); await settle();
  await vi.advanceTimersByTimeAsync(2000);
  expect(h.last().hud).toEqual(hud); expect(h.last().errors.hud).toBe(false);
});

it('superseded_reply_does_not_spin_or_discard_previous_value', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.calls.hud.mockResolvedValue({ status: 'superseded' });
  h.controller.onEvent(drop()); await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.hud).toHaveBeenCalledTimes(3);
  await vi.advanceTimersByTimeAsync(24); expect(h.calls.hud).toHaveBeenCalledTimes(3);
  await vi.advanceTimersByTimeAsync(1); expect(h.calls.hud).toHaveBeenCalledTimes(4);
  expect(h.last().hud).toEqual(hud); expect(h.last().errors.hud).toBe(false);
  h.controller.dispose(); await vi.advanceTimersByTimeAsync(1000);
  expect(h.calls.hud).toHaveBeenCalledTimes(4); expect(vi.getTimerCount()).toBe(0);
});

it.each(['hud', 'streak', 'profile', 'usual'] as const)('%s_reissues_superseded_inputs_without_errors', async read => {
  const h = harness();
  h.calls[read].mockResolvedValueOnce({ status: 'superseded' }).mockResolvedValueOnce({ status: 'superseded' });
  h.controller.update(h.input); await settle();
  expect(h.calls[read]).toHaveBeenCalledTimes(2);
  h.controller.update({ ...h.input, nowMicros: h.input.nowMicros + 1000 });
  await vi.advanceTimersByTimeAsync(25);
  expect(h.calls[read]).toHaveBeenCalledTimes(3);
  expect(h.calls[read].mock.calls[1]).toEqual(h.calls[read].mock.calls[0]);
  expect(h.calls[read].mock.calls[2]).toEqual(h.calls[read].mock.calls[0]);
  expect(h.last()).toMatchObject({ hud, streak: taggedStreak, profile: profile(h.input.dungeon!) });
  expect([...h.last().usual.keys()]).toEqual([100, 200]);
  h.controller.update({ ...h.input, bossTypes: [100, 200, 300] }); await settle();
  expect([...h.last().usual.keys()]).toEqual([100, 200, 300]);
  expect(h.states.every(state => Object.values(state.errors).every(failed => !failed))).toBe(true);
});

it.each(['success', 'failure'] as const)('throwing_receive_on_%s_does_not_stop_later_reads', async result => {
  const h = harness(state => {
    if (state.hud !== null || state.errors.hud) throw new Error('synthetic receiver failure');
  });
  if (result === 'failure') h.calls.hud.mockRejectedValueOnce(new Error('synthetic read failure'));
  h.controller.update(h.input); await settle();
  expect(h.calls.streak).toHaveBeenCalledOnce(); expect(h.calls.profile).toHaveBeenCalledOnce();
  expect(h.calls.usual).toHaveBeenCalledOnce();
  expect(h.last().errors.hud).toBe(result === 'failure');
  h.controller.onEvent(drop()); await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.hud).toHaveBeenCalledTimes(2); expect(h.calls.streak).toHaveBeenCalledTimes(2);
  expect(h.last().errors).toEqual({ hud: false, streak: false, profile: false, usual: false });
});

it('continuous_events_refresh_hud_and_streak_by_two_seconds', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  for (let window = 0; window < 2; window++) {
    for (let i = 0; i < 5; i++) {
      h.controller.onEvent(drop()); await vi.advanceTimersByTimeAsync(400);
    }
    expect(h.calls.hud).toHaveBeenCalledTimes(window + 2);
    expect(h.calls.streak).toHaveBeenCalledTimes(window + 2);
  }
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.hud).toHaveBeenCalledTimes(3); expect(h.calls.streak).toHaveBeenCalledTimes(3);
});

it.each(['hud', 'streak'] as const)('in_flight_%s_publishes_then_runs_exactly_one_follow_up', async read => {
  const h = harness(); h.controller.update(h.input); await settle();
  const pending = deferred<HistoryReply<HomeHud & HomeDungeonStreak>>();
  h.calls[read].mockReturnValueOnce(pending.promise);
  h.controller.onEvent(drop()); await vi.advanceTimersByTimeAsync(500);
  for (let i = 0; i < 3; i++) {
    h.controller.onEvent(drop()); await vi.advanceTimersByTimeAsync(500);
  }
  expect(h.calls[read]).toHaveBeenCalledTimes(2);
  pending.resolve({ status: 'ready', value: { ...hud, ...streak, whites: 7, runs_ended: 7 } }); await settle();
  const count = (state: HomeV4ReadState) => read === 'hud' ? state.hud?.whites : state.streak?.value.runs_ended;
  expect(h.states.some(state => count(state) === 7)).toBe(true);
  expect(h.calls.hud).toHaveBeenCalledTimes(read === 'hud' ? 3 : 4); expect(h.calls.streak).toHaveBeenCalledTimes(3);
  expect(count(h.last())).toBe(read === 'hud' ? 2 : 46);
  await vi.advanceTimersByTimeAsync(5000);
  expect(h.calls.hud).toHaveBeenCalledTimes(read === 'hud' ? 3 : 4); expect(h.calls.streak).toHaveBeenCalledTimes(3);
});

it('scope_changes_fetch_only_hud_and_ignore_obsolete_hud', async () => {
  const h = harness(), pending = deferred<HistoryReply<HomeHud>>();
  h.calls.hud.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle();
  const scope = { today: { range: { start: '1791763200000000', end: '1791849600000000' } } };
  h.controller.update({ ...h.input, scope });
  pending.resolve({ status: 'ready', value: { ...hud, whites: 999 } }); await settle();
  expect(h.calls.hud).toHaveBeenLastCalledWith(scope);
  expect(h.calls.streak).toHaveBeenCalledOnce();
  expect(h.states.some(state => state.hud?.whites === 999)).toBe(false);
  expect(h.last().hud).toEqual(hud);
});

it('absent_and_failed_usual_rows_do_not_poll_on_snapshots', async () => {
  const h = harness(); h.calls.usual.mockResolvedValueOnce({ status: 'ready', value: { rows: [], truncated: false } });
  h.controller.update(h.input); await settle();
  h.calls.usual.mockRejectedValueOnce(new Error('synthetic'));
  const next = { ...h.input, bossTypes: [100, 200, 300] };
  h.controller.update(next); await settle();
  for (let i = 0; i < 20; i++) h.controller.update({ ...next, nowMicros: next.nowMicros + i });
  await vi.advanceTimersByTimeAsync(5000);
  expect(h.calls.usual).toHaveBeenCalledTimes(2); expect(h.last().usual.size).toBe(0);
});

it('only_history_invalidating_events_refresh_the_hud', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.controller.onEvent(event({ fight_updated: homeSnapshot().fights[0] }));
  await vi.advanceTimersByTimeAsync(500); expect(h.calls.hud).toHaveBeenCalledOnce();
  h.controller.onEvent(event({ map_entered: { at: '1791806400000000', name: 'Nexus', kind: 'nexus' } }));
  await vi.advanceTimersByTimeAsync(500); expect(h.calls.hud).toHaveBeenCalledTimes(2);
  h.controller.onEvent(event({ drop_updated: homeSnapshot().drops[0] }));
  await vi.advanceTimersByTimeAsync(500); expect(h.calls.hud).toHaveBeenCalledTimes(3);
  h.controller.onEvent(event({ session_updated: { id: 'synthetic-session', started: '1791763200000000',
    last_traffic: '1791806400000000', ended: null, issues: [] } }));
  await vi.advanceTimersByTimeAsync(500); expect(h.calls.hud).toHaveBeenCalledTimes(4);
});

it('in_flight_usual_types_are_reserved_and_old_character_replies_are_ignored', async () => {
  const h = harness(), pending = deferred<HistoryReply<HomeBossBaselines>>();
  h.calls.usual.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle();
  h.controller.update({ ...h.input, bossTypes: [100, 200, 300] }); await settle();
  h.controller.update({ ...h.input, characterId: 'synthetic-character-2', bossTypes: [400] });
  pending.resolve({ status: 'ready', value: { rows: [boss(100), boss(200)], truncated: false } }); await settle();
  expect(h.calls.usual).toHaveBeenCalledTimes(2);
  expect(h.calls.usual.mock.lastCall![0].target_types).toEqual([400]);
  expect([...h.last().usual.keys()]).toEqual([400]);
  expect(h.states.some(state => state.usual.has(100))).toBe(false);
});

it('profile_reply_cannot_overwrite_a_new_dungeon', async () => {
  const h = harness(), pending = deferred<HistoryReply<DungeonProfile>>();
  h.calls.profile.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle();
  h.controller.update({ ...h.input, dungeon: 'Synthetic Crypt', profileDungeon: 'Synthetic Crypt' });
  pending.resolve({ status: 'ready', value: profile(h.input.dungeon!) }); await settle();
  expect(h.last().profile?.dungeon).toBe('Synthetic Crypt');
  expect(h.states.some(state => state.profile?.dungeon === h.input.dungeon)).toBe(false);
  h.controller.update(h.input); await settle();
  expect(h.last().profile?.dungeon).toBe(h.input.dungeon);
  expect(h.calls.profile).toHaveBeenCalledTimes(2);
});

it('run_end_invalidates_cached_profiles_for_later_visits', async () => {
  const h = harness(); h.controller.update(h.input); await settle();
  h.controller.update({ ...h.input, dungeon: 'Synthetic Crypt', profileDungeon: 'Synthetic Crypt' }); await settle();
  h.controller.onEvent(runEnd());
  h.controller.update({ ...h.input, dungeon: 'Synthetic Crypt', profileDungeon: null });
  await vi.advanceTimersByTimeAsync(500);
  expect(h.calls.profile).toHaveBeenCalledTimes(2);
  h.controller.update({ ...h.input, dungeon: 'Synthetic Crypt', profileDungeon: 'Synthetic Crypt', generation: 'synthetic-next-crypt' });
  await settle();
  expect(h.calls.profile).toHaveBeenCalledTimes(3);
  h.controller.update(h.input); await settle();
  expect(h.calls.profile).toHaveBeenCalledTimes(4);
});

it('boss_batches_respect_the_command_limit', async () => {
  const h = harness(); h.controller.update({ ...h.input, bossTypes: Array.from({ length: 65 }, (_, i) => i + 1) });
  await settle();
  expect(h.calls.usual).toHaveBeenCalledTimes(2);
  expect(h.calls.usual.mock.calls.map(([request]) => request.target_types!.length)).toEqual([64, 1]);
  expect(h.last().usual.size).toBe(65);
});

it('missing_scope_dungeon_and_character_remain_unknown', async () => {
  const h = harness(); h.controller.update({ ...h.input, scope: null, dungeon: null, profileDungeon: null, characterId: null }); await settle();
  expect(h.last()).toEqual({ hud: null, streak: null, profile: null, usual: new Map(), failedUsualTypes: new Set(),
    errors: { hud: false, streak: false, profile: false, usual: false } });
  for (const call of Object.values(h.calls)) expect(call).not.toHaveBeenCalled();
  h.controller.update(h.input); await settle();
  expect(h.last().hud).toEqual(hud); expect(h.last().profile).toEqual(profile(h.input.dungeon!));
});

it('accepts_reactive_scope_proxies_and_captures_the_request_values', async () => {
  const h = harness(), session = { id: 'synthetic-session' };
  const scope = new Proxy({ session: new Proxy(session, {}) }, {});
  h.controller.update({ ...h.input, scope });
  session.id = 'synthetic-session-2';
  await settle();
  expect(h.calls.hud).toHaveBeenCalledWith({ session: { id: 'synthetic-session' } });
});

it('scope_change_cancels_obsolete_busy_retries', async () => {
  const h = harness(); h.calls.hud.mockResolvedValueOnce({ status: 'busy' });
  h.controller.update(h.input); await settle();
  h.controller.update({ ...h.input, scope: { session: { id: 'synthetic-session-2' } } }); await settle();
  await vi.advanceTimersByTimeAsync(2000);
  expect(h.calls.hud).toHaveBeenCalledTimes(2); expect(h.last().hud).toEqual(hud);
});

it('dispose_ignores_late_replies_and_drops_queued_reads', async () => {
  const h = harness(), pending = deferred<HistoryReply<HomeHud>>();
  h.calls.hud.mockReturnValueOnce(pending.promise);
  h.controller.update(h.input); await settle(); h.controller.dispose(); const count = h.states.length;
  pending.resolve({ status: 'ready', value: hud }); await settle();
  expect(h.states).toHaveLength(count);
  expect(h.calls.streak).not.toHaveBeenCalled(); expect(h.calls.profile).not.toHaveBeenCalled();
  expect(h.calls.usual).not.toHaveBeenCalled();
});

it('dispose_cancels_debounce_and_busy_retry_and_ignores_updates', async () => {
  const h = harness(); h.calls.hud.mockResolvedValue({ status: 'busy' });
  h.controller.update(h.input); await settle(); h.controller.onEvent(runEnd());
  h.controller.dispose(); const count = h.states.length;
  h.controller.update({ ...h.input, characterId: 'synthetic-character-2' }); h.controller.onEvent(drop());
  await vi.advanceTimersByTimeAsync(10_000);
  expect(h.states).toHaveLength(count); expect(h.calls.hud).toHaveBeenCalledOnce();
  expect(vi.getTimerCount()).toBe(0);
});
