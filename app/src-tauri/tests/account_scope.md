# Account-scope validation

Native tests must execute on Windows. macOS cross-compilation checks their types
and links their executables; it does not verify runtime behavior.

## Integration-head CI regressions addressed

All original result assertions are retained.

| Test | Root cause | Fix |
| --- | --- | --- |
| `runtime::home_tests::cold_v1_upgrades_after_confirmation_without_capture` | Identity fixture starts a new stream with `CharacterUpdated`, which is not an allowed first event. | Prefix the shared fixture stream with `CaptureStateChanged`, then number identity events consecutively. |
| `runtime::home_tests::cold_v2_upgrades_after_confirmation_without_capture` | Same invalid first event. | Same shared fixture fix. |
| `runtime::home_tests::cold_v3_upgrades_after_confirmation_without_capture` | Same invalid first event. | Same shared fixture fix. |
| `runtime::home_tests::cold_v4_upgrades_after_confirmation_without_capture` | Same invalid first event. | Same shared fixture fix. |
| `runtime::home_tests::cold_v5_upgrades_after_confirmation_without_capture` | Same invalid first event. | Same shared fixture fix. |
| `runtime::home_tests::capture_start_failure_preserves_upgraded_cold_history` | Same invalid identity fixture prevents reaching the capture-failure assertion. | Same shared fixture fix; the namespace lease still forces capture startup to fail. |
| `runtime::history_replay_tests::late_counter_revision_reopens_one_story` | The typed comparison fixture has ownerless runs, so account-scoped comparisons correctly omit them. | Seed a verified character and identity, and assign its ID to each comparison run. Preserve the three-clear mean and change-fraction assertions. |
| `runtime::home_tests::playback_replacement_waits_for_reader_and_rejects_same_namespace_generation` | The test's additional oracle reader remains open while Windows attempts to remove the playback database. | Drop that reader before releasing the blocked runtime reader. Keep the wait, supersession and generation assertions. |
| `runtime::maintenance_tests::probes_cover_absent_legacy_and_compatible_schemas_without_requesting_work` | The current migration creates schema 19 but the test labels it 16. | Use `SCHEMA_VERSION` for the current-schema case; retain a separately installed historical schema-16 case. |
| `runtime::stats_tests::dungeon_profile_read_returns_ready_on_seeded_store` | Shared Stats fixture has neither account membership nor a verified current-account acquisition. | Seed both in Stats setup; retain the one-clear/one-sample assertions. |
| `runtime::stats_tests::home_boss_baselines_return_ready_through_history_scheduler` | Same missing fixture ownership and account pointer. | Same Stats setup fix; retain the fight count and exact DPS assertions. |

## Additional coverage

- Store `account_scope` tests verify snapshot-consistent account resolution during
  a concurrent pointer switch and account-isolated clear-time PB alerts, including
  unknown owners. The PB test avoids the legacy fixture helper that deliberately
  attributes unknown history.
- Native `account_scope` tests cover live/stopped/unknown/demo resolution,
  provisional and lost identity, same-stream in-flight supersession, exact-match
  recovery batches, persisted-prefix resume, unmatched IDs, and notice delivery.
- The standalone `runtime/account_batches.rs` test runs on macOS with `rustc --test`.
- Recovery adds no diagnostic logging. The notice DTO contains only a numeric
  revision and a reason; it does not contain an account ID, candidate ID or key.
