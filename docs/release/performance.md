# Performance policy and measurement

Budgets are acceptance criteria, not claims of current compliance. On 2026-10-06 the owner raised the idle and busy release limits (issue #40). With the live-realm fixes, the synthetic realm workload measured 272,547,840 busy / 174,989,312 idle combined bytes, within the revised limits ([evidence](../performance/2026-10-06-live-realm-fixes.md)). A 2026-10-07 rebaseline on another host found one idle run of two above the limit (208,039,936 bytes; [rebaseline](../performance/2026-10-07-live-realm-rebaseline.md)). After the follow-up allocation fixes, both runs measured 254,164,992 / 262,967,296 busy and 172,941,312 / 175,013,888 idle bytes ([evidence](../performance/2026-10-08-boss-chart-dedupe.md)); idle stays within run-to-run variance, so no idle saving is established. Live gameplay and installed-release acceptance remain open.

## 0.1.0 limits

Hardware acceleration defaults off. Count Maxed plus every owned WebView2 process using concurrent private bytes, not only the renderer or working set. [scripts/memory-policy.ps1](../../scripts/memory-policy.ps1) is the executable policy source.

| Workload | Release limit, acceleration off | Investigation target |
| --- | ---: | ---: |
| Idle | strictly below 200,000,000 bytes | at most 130,000,000 bytes |
| Busy, history browsing and long runs | strictly below 300,000,000 bytes | at most 140,000,000 bytes |
| Startup | strictly below 150,000,000 bytes | at most 150,000,000 bytes |

The original 2026-10-04 limits were 150,000,000 idle and 200,000,000 busy. They were raised because a fresh, idle app already measures about 134,000,000 bytes, most of it in WebView2 processes Maxed does not control. Acceleration on is opt-in and reported without a 0.1.0 memory gate; its cost is disclosed in Settings. Investigation targets do not replace release limits. Warm process-start-to-interactive launch must be below 1.5 seconds, busy CPU below 2% of total machine capacity, and the installer strictly below 20,000,000 bytes. Report CPU-seconds/wall-seconds (cores) as well as machine-normalized percentage so the denominator is explicit.

## Valid measurement

1. Build with `scripts/build-release.ps1`, including the built frontend and `custom-protocol`. Use the installed executable or byte-verified NSIS-packed equivalent for release acceptance. A bare Cargo build, development server, error page or disconnected subscription is invalid evidence.
2. Record source commit, dirty status, executable SHA-256, build features, packaging state, OS/CPU/RAM/WebView2/tool versions, effective acceleration and workload. Hash the exact executable that ran. Do not reuse measurements after a rebuild or integration change.
3. Verify the real screen renders and responds. Track process identity by PID plus birth time and include every descendant. Sum each sample's private bytes before taking the maximum; summing independent process peaks overstates a simultaneous peak.
4. Use the maintained collectors (`scripts/Measure-MaxedMemory.ps1`, `tests/performance/interactive-startup.ps1` and scenario drivers). Run their `-SelfTest` first. Preserve machine-readable evidence, timing windows, per-process attribution and completeness flags. Missing counters/processes, invalid rendering or incomplete coverage cannot pass.
5. Measure representative busy dungeon/realm use, populated history, transitions, idle recovery and a long session. Tiny demo and empty Nexus workloads cannot certify realistic busy/history budgets. Record other machine load and repeat matched workloads without overlapping compilation.
6. Measure warm startup from process creation to actual interactive rail/render readiness, not native-window creation alone. A collector attached after window readiness cannot prove the whole startup-memory gate. Check cold asset extraction separately.

CDP/DevTools, heap snapshots and forced GC are diagnostic only. JS heap usage, sampled allocation and process private bytes are different measures. A heap snapshot can force collection; it cannot prove that the uninstrumented app releases memory normally. UI Automation may alter cost, but discounting it requires controlled evidence. Keep diagnostics separate from uninstrumented release acceptance.

## Existing evidence and next checks

The 2026-10-06 investigation ([attribution](../performance/2026-10-06-live-realm-attribution.md), [peaks](../performance/2026-10-06-live-realm-peaks.md), [fixes](../performance/2026-10-06-live-realm-fixes.md)) replaced per-update loot recomputation, per-event realm departure delivery and whole-atlas sprite delivery. Each configuration was measured once, with synthetic data, and the original real-session symptom and installed package have not been retested. The 2026-10-07/08 follow-up ([rebaseline](../performance/2026-10-07-live-realm-rebaseline.md), [route reads](../performance/2026-10-07-route-read-churn.md), [live window](../performance/2026-10-07-live-window-churn.md), [chart dedupe](../performance/2026-10-08-boss-chart-dedupe.md)) removed per-event history-entry copies, reused unchanged Home live-window projections and stopped repeating open priority fight charts in update events; snapshots still carry those charts. Sampled steady-busy JS allocation fell from about 1.29 to 0.89 MB/s. A 30+ minute live realm session remains the acceptance check.

Recording-enabled tests must additionally cover CPU, disk use, real quota/volume failures and transitions while capture and summary storage continue. Windows gameplay, long sessions, startup, populated history and clean installed builds remain required.

## Snapshot microbenchmark provenance

The ignored `snapshot_2048_fights_regression_bound` test in `crates/maxed-engine/tests/d_totals.rs` retains a historical whole-snapshot p95 baseline of **43.219 ms**, plus a 2 ms allowance, for 2,048 fights × 50 players. The historical **26.678 ms** figure measured cloning only and is not the full baseline. This relative synthetic timing check is neither a portable absolute performance guarantee nor a process-memory/startup acceptance test. Reproduce with the pinned toolchain, release profile and a quiet Windows machine before interpreting a change.

## WebView2 memory policy

With acceleration off, [window_policy.rs](../../app/src-tauri/src/window_policy.rs)
retains wry's `--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection`
defaults and appends
`--disable-gpu --in-process-gpu --enable-features=NetworkServiceInProcess2`.
Keeping the GPU and network service in the browser process reduces process
memory overhead. It also reduces process isolation: a GPU or network-service
crash can terminate the browser process. `NetworkServiceInProcess2` is experimental
and may become ineffective with a future runtime. Local UI content limits
exposure but does not remove the stability trade-off. Acceleration on keeps only
the stock arguments; a preference change requires an application restart.

Site-isolation-disabling switches and JavaScript heap-limit flags are rejected
mitigations and are not production policy. Recheck memory and stability after
WebView2 updates. `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` appends arguments; the
last duplicate feature-list switch wins. An override supplying
`--disable-features=` must repeat `msWebOOUI,msPdfOOUI,msSmartScreenProtection`.
An override supplying `--enable-features=` must repeat `NetworkServiceInProcess2`
to preserve the acceleration-off mitigation. Clear diagnostic overrides for
controlled comparisons.

The Settings text **80–385 MB** describes historical measurements from
2026-10-04, not a guarantee for another device or workload. The measured build is
`a97a816`, a non-packaged release with `custom-protocol`, executable SHA-256
`af4d536cac48720fcc6525770fc6bfe1153a0b319e31fc2fa9193e1eabca07f9`, on Windows
`10.0.26200.0`, WebView2 `154.0.4258.53` and 14 logical processors. Runs use an
isolated synthetic profile/catalog and 10,000-run history; no live capture or
personal history is involved. Acceleration changes occur while stopped, followed
by restart. Accepted observations include 180-second idle and 120-second history
runs, with no intentional concurrent build/test load in the final repeats.

| Workload | Off peak, decimal MB | On peak, decimal MB | Difference | Accepted off/on repeats |
| --- | ---: | ---: | ---: | ---: |
| Idle | 125.50 | 205.13 | 79.63 | 2 / 1 |
| History, selected run with 400 fights | 168.56 | 553.96 | 385.40 | 3 / 2 |
| History, selected run with 3,000 fights | 177.48 | 557.02 | 379.55 | 3 / 3 |

These are maximum accepted concurrent process-tree private-byte peaks. The
400-fight acceleration-on peak includes 385,048,576 bytes in the GPU process;
this locates a process cost, not individual GPU allocations. The Settings copy
change is later than the measured executable. These results do not establish
current installed-build, startup-memory or live-realm acceptance.

Earlier 2026-10-02 visible-Home probes on WebView2 `154.0.4258.48` report 146.0 MB
with `--disable-gpu`, 131.7 MB with `--in-process-gpu` added, and 123.3 MB with the
network-service feature plus two unnecessary flags. The last probe does not
isolate the selected minimal combination. Separate runs without `custom-protocol`
show an error page and are invalid UI performance evidence.

## Synthetic store/engine gates

Run serially in release on a quiet machine with the pinned toolchain. Measure
matched baseline and candidate workloads on the same host; record build identity,
conditions and timing output. These gates do not replace process-memory or
Windows gameplay acceptance.

[Store gates](../../crates/maxed-store/tests/d_perf.rs) use the warm-read and
writer helpers in [d_workload.rs](../../crates/maxed-store/tests/common/d_workload.rs).
Set `MAXED_PD_P4_REALISTIC_STORY_P95_US` and `MAXED_PD_P4_WORST_STORY_P95_US`
to fresh same-machine baseline p95 values in microseconds. The historical story
baselines are **18.178 ms** realistic and **28.768 ms** worst; their **1.10×**
limits are **19.9958 ms** and **31.6448 ms**. They are provenance, not portable
hardcoded defaults: the test requires positive finite environment values.

```powershell
# Replace these historical example values with matched measurements.
$env:MAXED_PD_P4_REALISTIC_STORY_P95_US = '18178'
$env:MAXED_PD_P4_WORST_STORY_P95_US = '28768'
cargo test --release -p maxed-store --test d_perf -- --ignored --test-threads=1
cargo test --locked --release -p maxed-engine snapshot_pd_additions_under_2ms_p95 -- --ignored --test-threads=1 --nocapture
```

- `dungeon_stats_and_story_totals_thirty_warm_reads`: after an untimed read,
  measure 30 warm reads; p95 is sorted sample 29. Home Session and Today over
  10,000 runs and 20 dungeon groups must each stay **under 50 ms**. Realistic
  (400 fights) and worst (3,000 fights) story p95 must be **at most 1.10×** their
  supplied baselines.
- `fight_json_growth_six_series_by_1800_buckets`: adding six 1,800-bucket player
  series must keep serialized fight size **under 8 MiB** and growth **at most
  35%** against the same fight without those series.
- `instance_event_volume_projection_batches`: compare two **1.2M-event** writer
  runs, 100 events per transaction, with matched ordinary-bag and instance-event
  workloads. Projection batch p95 must be **at most 1.10×** the measured baseline.
  The selected run's story must remain below 20,000 timeline rows with 60
  departure rows.
- `captured_bags_fourfold_and_overflow_returns_limit`: fourfold realistic/worst
  bags remain readable; a separate 2,004-bag burst returns a limit error.
- `worst_run_repeated_fight_updates_do_not_grow_with_history`: paired early
  (20 fights) and late (3,000 fights) samples require late writer p95 at most
  1.10× early p95.
- `snapshot_pd_additions_under_2ms_p95` in
  [profiling_tests.rs](../../crates/maxed-engine/src/engine/profiling_tests.rs)
  requires both cached and one-dirty-fight projection p95 **under 2 ms** for
  2,048 fights × 50 players. It measures totals and bounded series selection/copy,
  including rebuilding the dirty contribution, after five warmups and across
  100 samples. Legacy fight cloning/sorting/source vectors are excluded; this is
  not a two-millisecond bound on the full snapshot.

### Loot history reads

The drop-summary performance gate at 50,000 bags is a median of at most 150 ms or at most 1.2 times the Stats median. This synthetic gate does not establish live release acceptance.

The ignored [Loot benchmark](../../crates/maxed-store/tests/loot_performance.rs) uses synthetic 50,000- and 500,000-bag histories, 20,000 ended runs and 40 dungeons. It measures Feed, Item history, Stats and Collection, including a 100-item chase list and dungeon grid. Run with the pinned toolchain in release on a quiet machine (source `scripts/mac/env.sh` first on macOS):

```sh
cargo test --locked --release -p maxed-store --test loot_performance -- --ignored --nocapture
```

Recorded evidence from the Windows development PC on 2026-10-05 passes every 50,000-bag read gate (strictly under 150 ms). At 500,000 bags, Stats takes about 0.53 s and Collection about 0.85–0.93 s. Collection exceeds the **report-only 750 ms target**; that target is not a failing test gate or a release acceptance claim. The covering indexes `drops_id_at` and `drops_stats` add roughly 100 MB at this size.

A 2026-10-05 rerun on an Apple-silicon macOS development machine also passed every 50,000-bag gate (slowest: Stats all time, about 106 ms). At 500,000 bags, Stats took about 0.39 s and Collection about 0.51 s. Results differ by machine; the Windows figures above remain the relevant evidence for the target platform.

These are recorded synthetic measurements, not a fresh benchmark or Windows live/installed-release acceptance. Repeat on the target machine and retain timing output before comparing a change.

### Chat history reads and pruning

The chat benchmark (`crates/maxed-store/tests/chat_performance.rs`) uses 500,000 synthetic messages in a release build. Run
with the pinned toolchain (source `scripts/mac/env.sh` first on macOS):

```sh
cargo test --release --locked -p maxed-store --test chat_performance -- --ignored --nocapture
```

Recorded on a macOS development machine: first page **0.21 ms**; a search page
whose query matches every row **0.57 s**; pruning 250,000 rows **10.1 s**
(about **40 ms per 1,000-row batch**). These are report-only synthetic timings,
not Windows acceptance or a fresh measurement from this documentation change.
