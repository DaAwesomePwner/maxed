# Live realm seed, transition and idle peaks - 2026-10-06

Research follow-up to Phase 1 and Phase 2a, issue #40. **Phase 2a steady busy private bytes average 268.3 MB, down from 350.6 MB in Phase 1. Its 552.9 MB labelled busy peak is transition work, not a steady plateau. Both release limits remain unmet.** All MB below are decimal. No product code fix, live-game verification or installed-build acceptance is claimed.

The accompanying [JSON evidence](2026-10-06-live-realm-peaks.json) contains sanitized aggregates. Raw process records, CDP traces and temporary profiles stay outside the repository.

## Timing correction and uninstrumented time series

The existing per-process evidence survived for both phases. Summing all owned processes at each sample reproduces the published peaks exactly. The nominal one-second collector actually sampled about every 1.2 seconds during busy work. Thirty-second bins and boundary samples in JSON cover the whole observed run; collector attachment does not establish the entire process-start startup peak.

The driver schedules 2,440 quarter-second busy steps, **then** calls `feed(leave())`, publishes the snapshot and waits for the publisher to flush, **then** prints `PERF_LIVE idle`. Its log marker lags scheduled map change by 2.492 seconds in Phase 1 and 2.739 seconds in Phase 2a. Thus the last samples classified as busy in the older reports contain transition work. The schedule is a source-derived boundary, not a new native timestamp probe.

A second correction: the older reports incorrectly described their overall peaks as startup/seed. Their retained CSVs locate **604,020,736 bytes at busy +614.232 / logged idle +1.741 seconds** in Phase 1 and **595,505,152 bytes at busy +612.960 / logged idle +0.221 seconds** in Phase 2a. These are transition peaks. The actual observed seed maxima were 540,262,400 and 549,187,584 bytes respectively (exact aggregates in JSON are authoritative).

Means in MB; every combined value includes Maxed, renderer, browser, storage utility and crashpad concurrently. “Steady” excludes the first 30 busy seconds and the last 30 seconds before scheduled departure; the last 30 seconds are also reported, so exclusion cannot hide late growth.

| Run / interval | Renderer mean | Browser mean | Maxed mean | Combined mean | Combined median | Combined maximum |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Phase 1 observed startup/seed | 98.226 | 54.177 | 56.979 | 219.770 | 256.584 | 540.262 |
| Phase 2a observed startup/seed | 89.349 | 54.283 | 54.070 | 208.086 | 235.188 | 549.188 |
| Phase 1 early busy +0–30 s | 269.679 | 62.420 | 62.869 | 406.057 | 390.918 | 506.032 |
| Phase 2a early busy +0–30 s | 160.001 | 59.653 | 62.928 | 293.661 | 250.823 | 409.158 |
| Phase 1 steady busy +30–580 s | 213.386 | 55.004 | 71.452 | 350.628 | 351.144 | 386.576 |
| Phase 2a steady busy +30–580 s | 133.250 | 54.549 | 69.648 | 268.291 | 272.183 | 292.663 |
| Phase 1 final pre-transition +580–610 s | 218.339 | 54.791 | 79.246 | 363.379 | 361.632 | 377.692 |
| Phase 2a final pre-transition +580–610 s | 143.841 | 54.248 | 76.606 | 285.725 | 284.991 | 294.253 |
| Phase 1 logged idle +0–30 s | 239.123 | 60.174 | 50.930 | 361.284 | 232.677 | 604.021 |
| Phase 2a logged idle +0–30 s | 194.517 | 58.198 | 48.845 | 312.589 | 217.231 | 595.505 |
| Phase 1 idle +30–175 s | 100.983 | 56.049 | 51.152 | 219.063 | 219.398 | 221.417 |
| Phase 2a idle +30–175 s | 95.957 | 55.597 | 48.697 | 211.111 | 210.661 | 215.126 |

Phase 2a reduces the steady mean by **82.337 MB (23.5%)**, including **80.136 MB in the renderer**. There is continued growth: the +30–60 s mean is 237.851 MB (renderer 108.448, Maxed 63.781), versus 287.011 MB at +540–570 s (renderer 145.394, Maxed 75.649). The driver revisits each ordinary target with a different contributor because 1,536 modulo 23 is 18; accumulating per-fight player state is one source-grounded contributor, not a measured complete allocation attribution. “Steady” names the interval between transients, not an indefinitely flat equilibrium. Use the 285.725 MB late-busy mean for conservative planning. Both uninstrumented runs put their labelled busy maximum in the final busy sample, already within transition. Two runs on different product revisions establish the same location, not a distribution of repeatability.

| Phase 2a seconds from busy start | Renderer | Browser | Maxed | Combined |
| --- | ---: | ---: | ---: | ---: |
| 609.31, before departure | 151.10 | 54.21 | 77.19 | 293.53 |
| 610.54, departure underway | 203.57 | 110.12 | 57.81 | 382.53 |
| 611.76, last labelled busy | 420.73 | 65.47 | 55.69 | 552.91 |
| 612.96, logged idle +0.22 | 465.66 | 64.97 | 53.84 | 595.51 |
| 614.18, logged idle +1.44 | 418.84 | 64.63 | 48.43 | 542.93 |

The short departure excursion is distinct from the 270–295 MB late-busy level. The release workload still includes departure; relabelling its phase does not remove a cost or create a gate pass.

## Build and diagnostic method

Investigation checkout: `b7fe92993ba47afe8119838415b8e5a3b1b4f7da`, branch `perf/40-attribution`, initially clean. Reused the byte-verified Phase 2a executable built from `48f4974fe954c68595c398bf78d040313bee46ec`, SHA-256 `647bc06c45ca67b85d094de044e3e5e17c065b7c36c5fdeaaa9620bb5bfada9f`; release, built frontend, `custom-protocol,perf-live`, non-packaged. HEAD differs from the build source only in the two Phase 2a report files. No rebuild or Rust/product-source edit was necessary. Same Windows 11/Ryzen 9 7900X/24-logical-processor/67.8 GB host and WebView2 154.0.4258.53 as the preceding reports.

Each diagnostic uses a newly empty directory at the already compiled temporary profile path. Previous profiles are archived in temp after owned-process shutdown. Normal installed-client asset discovery is unchanged; synthetic packets and identities only. No normal Maxed profile, account service, main checkout, live capture or raw game evidence is used.

The new diagnostic schedule samples seed, seed end, busy +590/+605/+609 seconds, every second around scheduled departure, and logged idle +1/+3/+10 seconds. Detailed memory-infra dumps and early/transition 250 ms V8 metrics are diagnostic interventions. All process-counter sums include the diagnostic tracing utility. No release acceptance is derived from them.

CDP is materially intrusive here: the previous Phase 2a diagnostic renderer mean over +30–580 was **193.629 MB**, versus **133.250 MB** uninstrumented. Hence the approximately 100 MB diagnostic V8 capacity cannot simply be subtracted from the uninstrumented 268.291 MB combined mean to predict a fix.

## Seed and departure allocation ownership

The natural control completed with 1,155 process samples, no missing counters, both rendered phases verified and no detected competing load. Its overall peak was 648.270 MB at busy +613.175 / logged idle +0.659 seconds, including a 38.539 MB tracing utility. The driver retained the same 612.516-second logged busy interval. This diagnostic total is not comparable to an uninstrumented gate.

At busy +605, renderer dump-private was 198.140 MB. At +612 it was 446.333 MB, and at logged idle +1 it was 481.907 MB. The dominant change is **malloc backed by Chromium PartitionAlloc**, not V8 heap capacity:

| Renderer pool, MB | Busy +605 | Busy +612 | Busy +613 | Idle +10 | Idle +30 |
| --- | ---: | ---: | ---: | ---: | ---: |
| malloc | 46.001 | 260.349 | 255.739 | 52.567 | 44.063 |
| Blink PartitionAlloc | 5.075 | 16.937 | 54.047 | 4.784 | 4.407 |
| Blink GC committed | 26.477 | 32.936 | 33.001 | 12.059 | 12.059 |
| V8 heap space | 102.175 | 118.624 | 118.624 | 20.582 | 20.845 |

Exact values in JSON take precedence over rounded table entries. These selected pools exclude their own suballocations; provider sizes and process private commitment still have different accounting bases. At +612, the malloc allocator partition reports **236.509 MB active allocations**, against 31.354 MB at +605, and 17.762 MB wasted. It is not mostly empty retained pages at the peak. The largest active buckets are 896 bytes (43.889 MB), 5,120 (37.586), 640 (35.798), 768 (32.239), 320 (25.316) and 2,560 (17.958). They describe many small native allocations, not a single atlas-sized buffer. A memory-infra dump supplies no native allocation stacks; exact C++ ownership remains unresolved. DOM nodes remain about 4,893–5,151 and listeners 73–81 through the departure burst, falling to 1,145/27 after collection; this is not a hundreds-of-thousands-of-DOM-nodes explanation.

The browser briefly contributes roughly 100 MB near +611, then falls back toward 65 MB; Maxed simultaneously falls from about 79.9 MB before departure to 48.8 MB at +613. The large departure peak is therefore not a new native-engine retention plateau. Source sequence: [`runs.rs::close_map`](../../crates/maxed-engine/src/runs.rs) retires every active fight, emits `FightEnded` for each, flushes bags, then clears the world; [`runtime/join.rs`](../../app/src-tauri/src/runtime/join.rs) queues full ordered events to storage and compact presentation events to the publisher, waiting on writer backpressure when necessary. [`runtime/perf.rs`](../../app/src-tauri/src/runtime/perf.rs) then publishes and flushes before printing idle. This produces a concentrated stream of roughly 1,536 fight endings into WebView2. Preserving these events for durability and side effects is required; a future fix should address delivery/projection allocation, not drop them.

A full snapshot **resync is not automatic on this map change**: the synthetic stream ID remains constant, the [native encoder](../../app/src-tauri/src/bridge/payload.rs) can send a delta, and the [UI subscription](../../app/ui/src/lib/bridge/live.ts) requests an authoritative snapshot for initial setup, stream changes or sequence/base gaps. A gap-driven resync is possible but not established by the control trace. The attempted counter wrapper in later probes could not replace Tauri's protected invoke function; its empty counts are not evidence of zero commands.

[Home's source](../../app/ui/src/routes/Home.svelte) switches from RealmHome to NexusHome after map observation and changes the requested visual types. The decoded image cache rises from 134.218 to 268.435 MB at +613, alongside a 38 MB Blink buffer excursion, consistent with additional atlas decode/encoded-buffer work. However, malloc already reaches 260.349 MB at +612 while decoded cache accounting is still unchanged at 134.218 MB. Thus a new image decode cannot by itself explain the entire departure rise. Shared/logical image sizes are not deducted from private bytes.

The [normal Home history reader](../../app/src-tauri/src/runtime/home.rs) points to `live.sqlite3`, whereas perf-live opens a separate `perf-live-<pid>.sqlite3` and leaves `Adapter.home` unset; it does not call the normal `open_home` setup. Consequently this workload does not establish populated Nexus history-query cost. UI sources do request saved summaries/loot on Nexus entry and dungeon bests through the Home controller, but blaming those reads for hundreds of MB here is unsupported. No separate stream-change workload was run.

The earlier **seed-period peak is predominantly Maxed**: at the uninstrumented combined seed maxima its native contribution is 440.361 MB in Phase 1 and 387.285 MB in Phase 2a. The new control observes 347.963 MB Maxed at its 513.204 MB combined seed peak. Seed dumps show a separate renderer Blink buffer/image transient: at seed -14.87 s, Blink PartitionAlloc is 77.03 MB and decoded cache 268.435 MB; neither is the native Maxed allocator.

The short sprite-blocking isolation uses CDP `Network.setBlockedURLs` only for the local sprite origin, an empty temporary profile, the same executable and the unchanged synthetic feed. Realm renders with fallback icons; this is intentionally incomplete visual coverage. Its independent Maxed seed peak is **66.298 MB**, versus **347.963 MB** control (281.666 MB lower). Its combined seed peak is 352.928 MB, with Maxed 47.620 MB at that instant; its later overall peak is still 512.934 MB, primarily renderer. This supports **whole-atlas delivery as the native seed-spike trigger**, without proving a particular native heap call stack or making fallback rendering a mitigation. The [native sprite path](../../crates/maxed-assets/src/sprite_delivery.rs) reads an entire 38.533 MB characters or 31.806 MB mapObjects PNG into a Rust byte vector, then wry 0.57.0 passes the response through `SHCreateMemStream`. Allocation/copy concurrency within that path needs native heap tracing before assigning individual bytes. Fresh-idle without the seed had only a 46.858 MB observed native maximum; sparse process polling cannot exclude an earlier sub-sample spike.

## V8 high-water verdict

**The approximately 100 MB capacity is not acquired entirely during seed and then permanently pinned.** In the control it is 45.613 MB at seed end, 97.255 MB at busy +10, 86.245 MB at +30, 98.701 MB at +300 and 102.175 MB late busy. The late heap contains a 67.109 MB new-space component. At departure it expands to 118.624 MB, then naturally shrinks to 20.582 MB by logged idle +10 and 11.407 MB by +600. Objects, capacity and native malloc are distinct.

The post-seed `HeapProfiler.collectGarbage` probe immediately reduces capacity from 47.186 to 7.864 MB, with 6.979 MB objects. Capacity regrows to 56.361 MB at +10, falls to 18.874 MB at +30, then reaches **59.183 MB at +605**, versus the control's 102.175 MB. Therefore the seed/early-busy collection history **does prime a later, larger capacity/slack regime** in this instrumented workload. It is path-dependent rather than an immutable 100 MB seed allocation. One pair does not establish repeatability or the same saving without CDP.

At +605, the GC run's renderer dump-private is 135.369 MB versus 198.140 MB control; V8 is 42.992 MB smaller, Blink GC 10.093 MB smaller and malloc 6.455 MB smaller. Over +30–580, renderer mean is **128.938 MB versus 194.888 MB** (65.950 MB lower), and all-process mean is 297.145 versus 364.157 MB. The mass-ending burst still reaches 619.692 MB combined, with a 451.793 MB renderer contribution at that peak. Later transition allocation sampling makes that peak a more intrusive observation than the control; the steady comparison precedes sampling.

The separate `--js-flags=--optimize-for-size` run confirms the sizing mechanism. The flag was verified on both owned browser and renderer, alongside `--disable-gpu` and `NetworkServiceInProcess2`. V8 capacity was 15.729 MB at seed end, 23.990 MB at +300 and **21.172 MB at +605** with 17.584 MB objects. At +605 renderer dump-private was **93.381 MB**, versus 198.140 MB control. The steady renderer mean was **94.395 MB**, and combined mean **260.974 MB including tracing**. This strongly supports heap-sizing/collection heuristics as a large contributor under instrumentation, not a 100 MB required live object set.

Departure was also smaller: its sampled combined maximum across scheduled departure through idle +30 was **360.444 MB**, and the +612 malloc pool was 76.810 MB rather than 260.349 MB. Collection policy therefore affects retained native allocations around the burst as well as V8 space. This does not establish the C++ owner of the control's native allocations, and native sampling after +605 adds another diagnostic difference. The flag run's overall 405.254 MB peak was instead during seed, with a 254.263 MB browser contribution: reducing one peak does not eliminate the other stages.

The native sampler returned 364 samples at +612 and 365 at idle +3, with unsymbolized address stacks. Its largest +612 allocation was **38,533,152 bytes**, matching the encoded character atlas size plus 32 bytes; that is consistent with the observed image-buffer transient, not proof that it explains the control's earlier 214 MB malloc rise. Raw stacks remain in temp. No native function-level owner is claimed without symbols. The flag run has only short idle coverage and supplies no +30–175 s idle-gate comparison.

| Diagnostic | Steady renderer mean, MB | Late V8 space at +605, MB | V8 objects at +605, MB | Renderer dump-private at +605, MB |
| --- | ---: | ---: | ---: | ---: |
| Natural control | 194.888 | 102.175 | 23.223 | 198.140 |
| Post-seed GC | 128.938 | 59.183 | 18.302 | 135.369 |
| Optimize for size | 94.395 | 21.172 | 17.584 | 93.381 |

Neither intervention certifies a smaller uninstrumented app. The original Phase 2a renderer mean was already only 133.250 MB without CDP, and idle remains far above the fresh floor. GC and heap-sizing flags are learning tools, not recommendations to ship.

V8's current flag definition makes `optimize_for_size` imply a 1 MiB maximum semi-space; V8's documentation describes the throughput/latency/memory tradeoff in growth slack and collection heuristics. Those sources explain why this is a useful **diagnostic**, not evidence that a rejected production heap flag should ship. No duplicate feature-list switch was added, so the existing acceleration-off GPU/network policy was preserved. [V8 flag definitions](https://raw.githubusercontent.com/v8/v8/main/src/flags/flag-definitions.h), [V8 memory heuristics](https://v8.dev/blog/optimizing-v8-memory).

The GC run also samples allocations from busy +605 through logged idle +10, at 32,768-byte intervals including minor/major collected objects. Over 17.677 seconds it estimates **360,677,328 bytes** across 10,862 samples, about **20.40 MB/s** averaged over that mixed window. Largest directly mapped application sites include fight visual-type collection (`home/visuals.ts:44`, 22.946 MB), visual-type normalization (`bridge/visuals.ts:7`, 22.155 MB), live-window `emit` (11.692 MB), its trim comparator (`home/v2/live.ts:140`, 12.064 MB), `bound` (9.973 MB) and rail fight scoping (9.484 MB). Native/runtime `set`, `add`, `push` and Svelte update/reconciliation are also large. These are allocation flows over a mixed short window, not live bytes or attribution of Chromium's native malloc stack. They corroborate repeated per-event presentation work during fight finalization. The exact native PA owner still needs stack evidence.


## Long idle and legitimate reclamation

The first ten minutes have no forced GC or pressure command. Native driver completion remains at +180 seconds, so subsequent results describe an open WebView after synthetic capture teardown, not ten minutes of a continuing real capture. Renderer and Maxed means, MB:

| Logged idle window | Renderer | Maxed | Combined including tracing | Tracing utility |
| --- | ---: | ---: | ---: | ---: |
| +30–175 s | 101.341 | 48.158 | 267.156 | 46.913 |
| +200–300 s | 97.888 | 47.403 | 266.055 | 51.072 |
| +300–450 s | 88.262 | 47.320 | 254.208 | 51.146 |
| +450–600 s | 83.170 | 47.353 | 250.922 | 51.258 |
| +570–600 s, before GC | 79.798 | 47.317 | 245.976 | 51.148 |
| +625–640 s, after GC at +610 | 72.465 | 47.329 | 244.997 | 55.577 |
| +670–700 s, after reload at +640 | 81.480 | 42.401 | 250.743 | 55.374 |

Natural waiting recovers about **21.543 MB renderer** between the usual idle mean and the final 30-second natural window. By +450 the remaining decoded image-cache entries are gone. Malloc declines from 44.063 MB at +30 to 31.473 MB at +600, and V8 from 20.845 to 11.407 MB, while Blink GC stays around 12–14 MB. The malloc allocator partition reports 23.854 MB wasted at +30, falling to 13.418 MB at +600 (only 0.659 MB decommittable plus 1.053 MB discardable then). The approximately 21.6–23.3 MB residual reconciliation gap is still unattributed; wasted is committed minus active memory, not a promise of reclaimable pages. Chromium documents periodic purging of empty/discardable spans, while occupied spans may retain slack. [PartitionAlloc design](https://chromium.googlesource.com/chromium/src/+/HEAD/base/allocator/partition_allocator/PartitionAlloc.md), [malloc dump provider](https://chromium.googlesource.com/chromium/src.git/+/main/base/trace_event/malloc_dump_provider.cc).

Late GC recovers another **7.333 MB renderer mean**, much less than the earlier Phase 1/2a probes because natural collection has already reclaimed part of it. At +625 V8 capacity is 6.029 MB with 5.075 MB objects. It is a diagnostic collection, not a production API recommendation.

Reload does **not** return the renderer to the fresh approximately 35 MB contribution. It temporarily raises dump-private to 106.54 MB at +660 and settles to 78.57 MB at +700; its window mean is 9.015 MB above the post-GC mean. It does release about 4.9 MB native Maxed in this sequence, which may reflect refreshed subscription/snapshot ownership, not proven allocator purging. Reload reparses/remounts UI and reestablishes subscriptions; it can lose transient visit/navigation/focus/selection state and introduce resync latency. No post-reload interactive-readiness, state-continuity or subscription-recovery acceptance was performed; the initial rendered-phase check predates reload. This experiment does not justify automatic long-idle reload.

A Home-only remount was not measured. Source ownership limits its expected reach: live visit state belongs to the shell connection, and the three-atlas cache is module-level. Remounting Realm/Nexus content alone does not release those owners or recreate Chromium's allocator. RealmHome already unmounts when NexusHome mounts. No byte saving is established for an extra subtree remount.


## Workload representativeness

The seed is a stress preparation, not a measured ordinary realm-entry trace. `crates/maxed-engine/src/perf.rs` adds all 24 players and 1,536 enemies in a single update, then 320 bags in 20 batches of 16, then 1,536 damage packets. `runtime/perf.rs` sleeps 100 ms after each of the first four packets and 10 ms after the rest, with bridge flushing per packet. All seed packets share one synthetic timestamp. Minimum scheduled seed sleep is 15.96 seconds; wall duration includes processing and asset discovery. This is paced, but remains a highly concentrated arrival of retained content.

Busy keeps all these high-HP enemies alive, cycles 24 hits/s across them, and ends them together on one Nexus map packet. It therefore stresses mass fight finalization as well as entry. A session that gradually encounters and ends fights has a different arrival/lifetime distribution. No live-session recording is collected under this task's synthetic-only constraint; the amount by which this exaggerates *real* gameplay is unmeasured. A seed peak above steady state alone does not prove that the later steady state is permanently inflated by seed.

Proposed additional matched diagnostic: distribute the same 1,536 fights and 320 bags over a 10-minute ramp using advancing event timestamps and realistic bounded concurrent enemies; retire fights throughout the ramp; then hold the same retained totals for a matched busy interval and leave. Retain the existing gate workload unchanged. Report preparation, stable busy and departure separately for both, with the same total counts, asset cache condition, profile freshness and hit rate. Separate pacing-only from lifetime-distribution experiments so a smaller working set is not credited to pacing.

## Reachability and priorities

The uninstrumented steady mean is **68.291 MB above the 200 MB busy ceiling**, its steady-window maximum **92.663 MB above**, and its late-busy maximum **94.253 MB above**. The ordinary post-transition idle mean is **61.111 MB above the 150 MB ceiling**, and the release comparison peak is **65.126 MB above**. Strictly-below gates require slightly more than these differences. Departure additionally reaches 595.505 MB overall; removing that excursion alone does not meet steady busy or idle.

The prior fresh floor of **134.463 MB** leaves only **15.537 MB idle** and **65.537 MB busy** headroom. Replacing only Phase 2a's renderer contribution at its idle peak with the prior fresh contribution still leaves about 150.090 MB combined, just above the idle ceiling. WebView2 does not make the gates mathematically impossible, but these experiments do not establish an attainable production path. It is unreasonable to infer impossibility from the 553 MB labelled busy peak, or to promise success from GC or a heap flag. Retained native state, renderer pools and departure bursts need separate reductions.

These are ranked experiments, not additive promised production savings. Estimated opportunity ranges must be remeasured without CDP and without a changed gate workload.

| Rank | Candidate | Bytes to target / expected evidence boundary | Next safe experiment |
| --- | --- | --- | --- |
| 1 | Bound departure delivery/projection allocation while preserving ordered events | **100–200 MB transient renderer** is a reasonable experiment target against the measured 214 MB malloc rise; **0 MB steady/idle saving established**. | Profile native small-allocation stacks and transition JS callers, then batch presentation work without dropping storage events, alerts, sequence coverage or visit history. Compare departure wall time and maximum as well as steady state. |
| 2 | Serve cropped sprite tiles / reduce native whole-atlas response copying | **200–300 MB native seed transient** is an opportunity suggested by request blocking; **steady/idle private savings unquantified**. | Keep identical visible sprites, bounds and validation, compare streaming/cropped responses against full PNG delivery. Measure native copies and decode scratch separately from shared decoded-cache accounting. Blocking all images is not a product candidate. |
| 3 | Reduce sustained bridge/projection allocation and retained fight state | The GC pair demonstrates **66 MB lower steady renderer mean under CDP**; sizing flags demonstrate **100 MB lower** and **81 MB less late V8 capacity**. Normal-run saving from an equivalent product change remains unknown; removed Phase 2a churn already saved about 82 MB steady mean. | Target measured callers and repeated state reconstruction. Preserve full retained visits and evidence semantics. Compare ordinary uninstrumented steady means/late maxima, not only an instrumented heap capacity. Heap-limit/size flags remain diagnostics only. |
| 4 | Attribute and reduce native engine/writer/snapshot retention | **10–15 MB idle** and part of the roughly **35–45 MB busy** native excess over fresh are opportunity bounds, not all safely freeable. | Native heap tracing around seed, departure, writer completion and subscription reload; distinguish necessary active fight state from allocator slack and stale references. The reload sequence released roughly 4.9 MB native, but that is not proof of a particular owner. |
| 5 | Reduce long-lived renderer native slack and owner lifetime | Natural waiting recovered **21.5 MB renderer**, then late GC another **7.3 MB**; a product change has **0 MB demonstrated saving**. | Test actual supported WebView memory-target behavior and view-owner lifetime with responsiveness and private-commit measurements. Preserve active subscriptions and visit state. Waiting ten minutes cannot satisfy the existing +30–175 s idle gate. |
| 6 | Extra subtree remount or automatic WebView reload | **0 MB saving established**; measured reload renderer mean was **9.0 MB worse** than after GC. | Do not adopt automatic reload on this evidence. A subtree-only experiment must show which shell/module owners it actually releases and preserve navigation, focus, selection and capture continuity. |

Peak work alone does not solve the busy gate: even ignoring departure, the late mean is about 85.7 MB above 200 MB. Likewise, long idle and forced collection do not restore the fresh floor. Gates may be reachable with a combination of smaller active state, lower allocation/slack and reduced native retention, but this investigation provides neither a WebView2 impossibility proof nor a credible single-switch route to compliance.

## Validation and limits

The original collector self-test passed, as did interactive-startup's 83 self-test assertions. Both new diagnostic/timeline tools have self-tests registered in `memory-tooling.test.ps1`; that suite passed under PowerShell 7 and Windows PowerShell 5.1 between workloads. No builds or tests intentionally overlapped a workload. All four new runs had zero incomplete process/CPU samples and no detected competing load; 115 detailed dump requests succeeded across them. Timeline analysis reproduced the earlier uninstrumented peaks exactly. JSON structure, concurrent sums, claimed deltas, local-path/privacy filtering and `git diff --check` were checked before commit.

The tooling preserves temporary-path checks, executable/profile attestation, exclusive launch lock, PID-plus-birth cleanup, competing-load observation and complete process-tree accounting. `seed-no-atlas` explicitly has no idle observation; its partial busy series is not a full busy comparison. Normal workload behavior is unchanged when `-PeaksProbe` is omitted. The extended idle still uses the original native driver's +180-second completion; only the observation window is extended.

To reproduce with the same compiled temporary-profile executable, use a fresh temporary output and empty the *owned temporary profile* by archiving it only after each driver has finished. Run separately and sequentially:

```powershell
pwsh -NoProfile -File tests/performance/live-realm-memory.ps1 -Exe $exe -Profile $profile -OutputDir $natural -Mode after -Diagnostic -Attribution -PeaksProbe natural
pwsh -NoProfile -File tests/performance/live-realm-memory.ps1 -Exe $exe -Profile $profile -OutputDir $noAtlas -Mode after -Diagnostic -Attribution -PeaksProbe seed-no-atlas
pwsh -NoProfile -File tests/performance/live-realm-memory.ps1 -Exe $exe -Profile $profile -OutputDir $seedGc -Mode after -Diagnostic -Attribution -PeaksProbe seed-gc
pwsh -NoProfile -File tests/performance/live-realm-memory.ps1 -Exe $exe -Profile $profile -OutputDir $size -Mode after -Diagnostic -Attribution -PeaksProbe optimize-size
node tests/performance/live-realm-peaks-report.mjs $natural
```

Run the offline report tool on each completed directory, including the recovered Phase 1/2a baseline directories. It validates concurrent process sums and produces a temporary review candidate with thirty-second bins, selected interval statistics, boundary samples and attributed dumps. The committed JSON is a reduced, sanitized selection, not raw trace export.

No product Rust/UI changes were made, so product Rust lint/tests and UI suites were not rerun for this documentation/tooling task. No live Npcap, gameplay, normal user profile, main checkout, installer, installed-release or startup acceptance was performed. One run per diagnostic intervention cannot quantify run-to-run variance. Native Maxed heap stacks, a representative gradual live session, an actual memory-target COM probe and continuing-capture ten-minute idle remain unmeasured. No push or pull request is included.
