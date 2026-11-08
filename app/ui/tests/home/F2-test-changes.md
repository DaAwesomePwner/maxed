# F2 test-change audit

## RISKS

The following existing tests changed only to replace global cross-reader displacement, terminal supersession, or immediate retry expectations. Parameterized titles cover all listed variants; other assertions remain. New regressions are listed separately below.

- `app/ui/tests/home-v3/dungeon-bests.test.ts` — `reissues superseded PB reads without waiting for a new snapshot`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home-v3/dungeon-bests.test.ts` — `serializes summary, scope loot and PB reads and reissues a superseded read in place`: The shared queue returns supersession to its caller, which owns delayed retry, instead of retrying immediately inside the queue.
- `app/ui/tests/home-v3/nexus-history.test.ts` — `keeps a summary %s valid when the blocked reason changes`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home-v3/nexus-history.test.ts` — `keeps a ready summary when loot is %s`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home-v3/nexus-history.test.ts` — `reissues superseded summary and loot reads serially without an input change`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home-v4/reads.test.ts` — `superseded_reply_does_not_spin_or_discard_previous_value`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home-v4/reads.test.ts` — `%s_reissues_superseded_inputs_without_errors`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home-v4/recap-reads.test.ts` — `superseded_reads_retry_while_the_recap_is_current`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home-v4/recap-reads.test.ts` — `busy_after_supersession_waits_for_the_attempt_window_before_its_one_retry`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home-v4/recap-reads.test.ts` — `superseded_visit_read_retries`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home/audit-history.test.ts` — `Superseded during initial read-ahead stays inert until the checkpoint catches up`: Replace terminal supersession with delayed bounded retry and a visible manual retry; preserve saved-data/error checks.
- `app/ui/tests/home/audit-history.test.ts` — `initial read-ahead retries Busy once while durability lags (second reply: %s)`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home/nexus.test.ts` — `superseded history retries unchanged inputs without a Retry control`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/home/summary.test.ts` — `keeps supersession transient without error, retry or a read loop`: Replace terminal supersession with delayed bounded retry and a visible manual retry; preserve saved-data/error checks.
- `app/ui/tests/home/summary.test.ts` — `retries Busy once and retains the spent budget across refresh ticks until manual retry`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/logs/events.test.ts` — `loads run and chat after the %s summary settles with %s`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/logs/history.test.ts` — `offers manual retry after the bounded automatic busy retry`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/loot/collection.test.ts` — `releases rows-ready for exhausted busy retries and offers Retry`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/loot/feed.test.ts` — `offers Retry after busy and shows an error instead of an empty feed after a read failure`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/loot/feed.test.ts` — `clears old filter summary numbers after %s`: Wait for bounded retries to exhaust and verify manual recovery for Superseded as well as Busy/error.
- `app/ui/tests/loot/feed.test.ts` — `retains summary numbers through one auto-retried Busy followed by ready`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/loot/feed.test.ts` — `hides previous-filter totals when the new drops page ends in %s without a summary read`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/loot/history.test.ts` — `offers manual retry after the bounded automatic busy retry`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/loot/item.test.ts` — `renders busy retry and errors without reporting a zero state`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/loot/item.test.ts` — `hides a retained zero summary when a refresh returns %s`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/loot/stats.test.ts` — `keeps a busy read distinct from empty data and offers Retry`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/loot/stats.test.ts` — `does not show a retained empty state after a %s refresh`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/runs-a1/history.test.ts` — `keeps Superseded inert until a new eligible input and never offers Retry`: Replace terminal supersession with delayed bounded retry and a visible manual retry; preserve saved-data/error checks.
- `app/ui/tests/runs-a1/history.test.ts` — `automatically retries deferred Busy once, then waits for manual Retry`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/runs/damage.test.ts` — `player_context_unwraps_control_before_breakdown (%s)`: Superseded now receives the same single delayed retry as Busy before breakdown reads can proceed.
- `app/ui/tests/runs/history-cache-controller.test.ts` — `non_ready_replies_are_not_cached`: Advance the fake clock through the delayed contention retry before checking the existing outcome.
- `app/ui/tests/runs/history.test.ts` — `deferred_busy_retries_once_then_requires_stable_manual_retry`: Wait one second before retry; explicitly return a second Busy because the original admission window has drained.
- `app/ui/tests/runs/history.test.ts` — `current_superseded_does_not_spin_and_permanent_failure_stops`: Replace terminal supersession with delayed bounded retry and a visible manual retry; preserve saved-data/error checks.
- `app/src-tauri/src/runtime/history_tests.rs` — `ordinary_navigation_never_returns_busy`: Distinct commands remain queued and both complete instead of the second being globally superseded.
- `app/src-tauri/src/runtime/history_tests.rs` — `alerts_history_home_run_alert_competition_never_returns_busy`: Home, run and alert readers retain two queued entries and all complete.
- `app/src-tauri/src/runtime/history_tests.rs` — `newer_same_key_supersedes_active_even_when_newest_pending_changes_again`: Keep same-key active supersession, but require the queued refresh to survive an unrelated subsequent command.
- `app/src-tauri/src/runtime/history_tests.rs` — `startup_coalesces_before_any_reader_is_dispatched`: Renamed startup_keeps_distinct_lanes_with_bounded_admission: preserve all 14 command cases, admit eight distinct keys, return Busy for overflow, then complete all admitted reads. Same-key startup coalescing is a separate new regression.
- `app/src-tauri/src/runtime/loot_tests.rs` — `loot_reads_share_history_admission`: Preserve the abuse-budget case; without abuse, keep both queued run and Loot reads rather than globally displacing the run.

The shared `tests/runs/fixtures.ts` queue is explicitly labeled a legacy single-slot contention simulator. Its behavior is unchanged so existing navigation tests and the new displacement-recovery test retain that stress condition.

## New regressions

- Loot: three concurrent drops/totals/run controllers recover a displaced totals read after one second, with no further event required.
- Home: summary retries wait two seconds, release the shared queue to HUD/recap, exhaust their automatic budget, and recover through manual retry.
- Native: same-key replacement preserves another lane and FIFO position; overflow returns Busy without evicting eight queued lanes; startup same-key coalescing remains bounded.
- Recap: hidden time does not consume the save deadline; SQL does not start or advance while hidden; disposal releases a suspended reader.
- Party: durability catching up after six seconds refreshes the saved run without a new event.
- RunStory: ordinary run refreshes reuse PBs, and a superseded account-history refresh preserves the saved PBs.

Both new Loot/Home contention tests failed against original source at `b7a811e` (exit 1) and passed with the implementation (exit 0). Native tests are cross-linked only on this macOS host; Windows execution remains required.

The native queue uses exact query keys as lanes, avoiding conflation of same-kind reads with different arguments. Page backlog increases from one global entry to a fixed eight; the active-reader, storage-FIFO, argument-validation and 32-attempt admission bounds remain unchanged.
