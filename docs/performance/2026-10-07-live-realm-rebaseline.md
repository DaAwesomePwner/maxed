# Live realm memory rebaseline — 2026-10-07

**Current HEAD passes busy in both runs, but idle is inconsistent: 208,039,936 / 177,225,728 bytes, with one failure against the revised 200 MB limit.** Busy peaks are 267,497,472 / 267,354,112 bytes. Neither run meets the investigation targets. Do not treat this as reliable idle acceptance.

The [sanitized JSON evidence](2026-10-07-live-realm-rebaseline.json) contains exact counters, concurrent process breakdowns, phase coverage, allocator aggregates, allocation sites and structural heap summaries. This is synthetic, visible-window, non-packaged evidence only. No product or measurement-tooling code changed. Raw process records, profiles, traces, heap snapshots and source maps remain in temporary storage.

## Build and method

- Source: `c9f8a7f9009a2856b9ea24fdeb395e0b5bba71ea`; clean at build. Documentation was written after measurements.
- Executable SHA-256: `58971598c8acd5bebb61c5159975199c02284c489b7f237416ecb21ca75c70f7`.
- Features: `maxed-app/custom-protocol,maxed-app/perf-live`; release, built frontend, non-packaged `maxed.exe`.
- Windows 11 Pro `10.0.26300`; Ryzen 7 5825U, 8 cores / 16 logical processors; 33,718,435,840 bytes usable RAM. WebView2 `154.0.4258.62`.
- Rust `1.99.0 (b940084d7 2026-09-28)`; Cargo `1.99.0 (5f94df478 2026-08-27)`; Node `22.16.0`; npm `10.9.2`; PowerShell `7.6.6`.
- Acceleration off, verified by the collector from owned browser arguments. No CDP, heap probe or browser-argument override in either gate run.

The build followed the [prior attribution build sequence](2026-10-06-live-realm-attribution.md): `npm.cmd --prefix app/ui ci`; set `TAURI_CONFIG` to an absolute temporary identifier and temporary main-window data directory, with `create=false`, 1100×760 and 800×560 minimum; `npm.cmd --prefix app/ui run build`; then:

```powershell
cargo build --locked --release -p maxed-app -p maxed-cli --features maxed-app/custom-protocol,maxed-app/perf-live
```

The exact compiled profile path was recreated empty after archiving each stopped launch's profile in temporary storage. Executable bytes did not change. After both gate runs, `npm.cmd --prefix app/ui run build -- --sourcemap hidden` reproduced byte-identical JavaScript; maps were moved out of the checkout. The executable was not rebuilt.

Each gate used `live-realm-memory.ps1 -Mode after`: 1,536 fights, 24 synthetic players, 320 bags, 24 hits/s, 610 scheduled busy seconds, then Nexus and 180 scheduled idle seconds. Steady busy is +30–580 s; departure is busy +610 s through idle +30 s; busy gate ends at the logged idle marker; idle gate is +30–175 s. Actual busy durations were 614.842 / 613.505 s. The collector ran for 840 seconds per accepted launch.

All launches were sequential. No builds, tests or unrelated heavy analysis ran during measured workloads. Accepted gates had no detected competing compiler/test/active-Node load, no incomplete memory samples or CPU intervals, and both rendered phases verified without subscription failure. Five processes were counted, using PID plus birth identity and concurrent totals. A native window check also confirmed visible, responsive and not minimized during the initial attempt; diagnostic page checks reported visible at every dump.

Collector and interactive-startup self-tests and `memory-tooling.test.ps1` passed before launch. Two initial gate attempts failed closed after 163.366 / 64.046 seconds, each with one incomplete sample and `ProcessCommandException` at collector line 77. One fresh-idle attempt similarly failed after 117.731 seconds. They are excluded, not repaired or treated as partial passes. The first launch command also used the wrong filename, `maxed-app.exe`, and failed before starting an app; the built binary is `maxed.exe`.

**Historical comparability is limited.** The prior `46d2569` measurement used a Ryzen 9 7900X, approximately 67.8 GB RAM, Windows `10.0.26200`, WebView2 `154.0.4258.53`, and different Node/npm versions. Normal installed-client asset discovery is still enabled: this host's initial temporary cache contained 11 files / 131,950,493 bytes, versus the prior reported 9 / 103,159,076. Cache disk bytes are not RAM. No normal Maxed profile, real traffic or account data was used. Measurements ran on October 7 in America/Chicago; JSON timestamps are UTC October 8.

## Gate results and variance

Decimal MB, except cores (renderer CPU-seconds / wall-second). Historical numbers come from [the fixes report](2026-10-06-live-realm-fixes.md).

| Run | Steady busy mean (+30–580 s) | Busy gate peak | Departure (+610 s → idle +30 s) peak | Idle gate peak | Renderer busy cores |
| --- | ---: | ---: | ---: | ---: | ---: |
| Historical combined `46d2569` | 260.6 | 272.5 | 257.2 | 175.0 | 0.073 |
| HEAD run 1 | 232.943 | 267.497 | 255.873 | 208.040 | 0.07661 |
| HEAD run 2 | 255.809 | 267.354 | 256.037 | 177.226 | 0.08575 |

| Run / phase | Exact peak bytes | Revised release limit | Investigation target |
| --- | ---: | --- | --- |
| 1 busy | 267,497,472 | **Pass**, <300,000,000 | Fail, <140,000,000 |
| 1 idle | 208,039,936 | **Fail**, <200,000,000 | Fail, <130,000,000 |
| 2 busy | 267,354,112 | **Pass**, <300,000,000 | Fail, <140,000,000 |
| 2 idle | 177,225,728 | **Pass**, <200,000,000 | Fail, <130,000,000 |

There were 794 / 812 complete collector samples. The busy gate had 564 / 577 samples; the idle gate had 135 / 136. Busy peak spread is just 143,360 bytes, but steady-busy means differ by 22,866,818 bytes and idle peaks by **30,814,208 bytes**. Renderer busy CPU is 0.479% / 0.536% of this 16-logical-processor host; these renderer-only values do not certify whole-app CPU.

Concurrent process contributions at each idle gate peak:

| Process | Run 1 bytes | Run 2 bytes | Fresh-idle floor peak bytes |
| --- | ---: | ---: | ---: |
| Renderer | 102,797,312 | 71,458,816 | 34,988,032 |
| Browser | 49,274,880 | 49,119,232 | 61,829,120 |
| Maxed | 45,023,232 | 45,445,120 | 32,845,824 |
| Storage utility | 7,843,840 | 8,204,288 | 7,606,272 |
| Crashpad | 3,100,672 | 2,998,272 | 3,096,576 |
| **Combined** | **208,039,936** | **177,225,728** | **140,365,824** |

The fresh-idle launch used the same executable, empty profile, no feed and no CDP, with 30 seconds settling and 180 seconds measurement. It had 218 complete collector samples, verified Home rendering, and no detected competing load. Its mean was **126,037,630 bytes**; its peak includes a transient browser increase. It is a floor comparison, not startup or live-idle acceptance. Post-realm peak excess over that floor is 67,674,112 / 36,859,904 bytes. Renderer contributions differ by +67,809,280 / +36,470,784 and Maxed by +12,177,408 / +12,599,296 bytes; the browser contribution is lower at the realm-idle peaks. These differences are not reclaimable-byte estimates.

### Regression verdict

Compared with `46d2569`, busy peaks are **5,050,368 / 5,193,728 bytes lower**. Steady-busy means are approximately 27.7 / 4.8 MB lower (the historical mean is only published to 0.1 MB). Idle is **33,050,624 / 2,236,416 bytes higher**. Thus **only one run exceeds the baseline by more than 15 MB on any specified metric (run 1 idle, +33.05 MB), so the both-runs trigger is not met**. The brief's two-run trigger for ordered merge measurements is not met; no historical-commit builds were run. This is a mixed idle outcome, not “neither run regresses,” and it does not identify a causal merge or establish regression-free idle behavior. Host/runtime/assets differences further prevent causal comparison with the old result.

## Diagnostic attribution

One separate `-Mode after -Diagnostic -Attribution` run completed all seven allocator dumps, allocation sampling and 930 seconds of process collection, with 893 samples and zero accounting gaps. CDP added a tracing utility. An additional existing `live-realm-heap.mjs` probe took a snapshot at approximately idle +90 s. That forces collection and caused the diagnostic overall process peak around that point; diagnostic idle peaks and subsequent pressure/GC changes are **not gate evidence or natural-idle recovery evidence**. No profiler errors or detected unrelated load occurred. The JSON preserves the perturbation explicitly, even though the existing active-Node load sampler did not flag the short heap probe.

### What accounts for steady busy above idle?

The gate process means themselves demonstrate variability:

| Steady-busy mean minus idle-gate mean | Run 1 MB | Run 2 MB |
| --- | ---: | ---: |
| Renderer | 1.420 | 57.793 |
| Maxed | 23.187 | 22.808 |
| Combined | 25.271 | 81.224 |

Renderer pool ownership is only sampled at two steady-busy instants, not continuously measured across the steady mean. The following **selected-pool reconciliation estimate** compares those dumps with natural idle +30 s, before the heap probe. Pools are disjoint at the selected level; their children are not added again. Provider `size` and Windows private commit have different bases, so the remainder is not an identified allocation.

| Renderer pool / process | Busy +300 MB | Busy +600 MB | Idle +30 MB | Busy +300 minus idle MB |
| --- | ---: | ---: | ---: | ---: |
| V8 heap spaces | 46.399 | 49.021 | 7.864 | +38.535 |
| Blink GC / DOM pool | 24.773 | 24.773 | 13.894 | +10.879 |
| malloc, including its PartitionAlloc backend | 33.905 | 33.768 | 25.333 | +8.572 |
| Separate Blink PartitionAlloc | 4.903 | 5.005 | 3.080 | +1.823 |
| Unattributed / basis gap | 13.855 | 17.338 | 16.180 | −2.326 |
| **Renderer dump private bytes** | **123.834** | **129.905** | **66.351** | **+57.483** |
| **Maxed nearby private counter** | **68.801** | **74.777** | **46.031** | **+22.770** |

The diagnostic resembles the lower-idle gate path. **It does not explain why run 1 retained another roughly 31 MB in the renderer.** Diagnosing that variability needs matched repeated diagnostics, rather than assigning its bytes to the V8 pool from a different launch.

At busy +300/+600, V8 allocated objects were 12.541 / 11.700 MB inside 46.399 / 49.021 MB of heap space. Space minus objects was 33.859 / 37.320 MB; objects can themselves include collectible garbage. Blink committed 24.773 MB for 5.126 / 5.810 MB of objects, with 7.471 / 5.767 MB pooled. At natural idle +30, Blink committed 13.894 MB for 2.790 MB of objects; V8 held 6.630 MB of objects in 7.864 MB of space. Capacity/slack is not promised savings.

Maxed's 22.8–28.7 MB point-in-time busy excess is measured only at process level. Engine fights/series (`crates/maxed-engine/src/engine.rs:113`, `:122`), snapshot projection (`:270`), writer/SQLite and the native allocator remain competing ownership hypotheses. CDP does not instrument Maxed. At idle, the browser remains about 49 MB and storage/crashpad about 11 MB combined; these are real floor costs, not attributed product allocations.

Decoded-image cache size at busy +300 was only 12,288 bytes, and the renderer web-cache image provider 15,050 bytes. The prior whole-atlas cache signature is absent in this observation. These logical provider values are not deductions from private bytes.

### Allocation sites and DOM trend

Sampling estimated **78,159,552 allocated bytes over 60.457 s**, about **1.293 MB/s**, from 2,372 samples at a 32,768-byte interval, including minor- and major-collected objects. These are allocation-flow estimates, not retained bytes. Hidden source maps matched the executable's JavaScript bytes. The table lists the top 12 sampled sites in rank order.

| Site / source | Estimated self bytes in sample |
| --- | ---: |
| Router `state` getter, `app/ui/src/lib/navigation/history.ts:93` | 21,158,916 |
| Channel receive/decoding, `app/ui/src/lib/bridge/native.ts:24` | 9,389,816 |
| Runtime/native `push`, callers not assigned | 3,710,876 |
| Runtime/native `add`, callers not assigned | 2,487,080 |
| Svelte `update_reaction`, `node_modules/svelte/src/internal/client/runtime.js:225` | 2,393,504 |
| Home live-window `snapshot`, `app/ui/src/lib/home/v2/live.ts:269` | 2,360,208 |
| Runtime/native `delete` | 2,133,232 |
| Runtime/native `getOwnPropertyDescriptor` | 1,934,268 |
| `scopedFights`, `app/ui/src/lib/shell/rail.ts:87` | 1,771,140 |
| Runtime/native `set` | 1,681,700 |
| Svelte `batch`, `node_modules/svelte/src/internal/client/dom/blocks/branches.js:76` | 1,443,240 |
| Home live-window `bound`, `app/ui/src/lib/home/v2/live.ts:220` | 1,344,912 |

The router getter accounts for 27.1% of sampled self bytes. Its `state()` copies the history entry through JSON (`history.ts:33`, `:59`). `Shell.svelte:178–182` separately reads `router.state` for each history invalidator, including Stats. Caching one route read per event or publishing an immutable route projection is a concrete next experiment; no causal attribution to a particular merge or private-byte saving is established.

| Window | DOM nodes | JS listeners | Documents | Layouts/s | Style recalcs/s |
| --- | --- | --- | ---: | ---: | ---: |
| Busy +30–300 | 4,786 → 4,838 | 35 throughout | 2 | 4.266 | 2.502 |
| Busy +400–610 | 4,844 throughout | 35 throughout | 2 | 4.512 | 2.532 |
| Natural idle +30–90 | 1,146 throughout | 25 throughout | 2 | 1.006 | 0 |

Late-busy node and listener counts plateau; this observation supports churn rather than continuing DOM/listener growth. Ten-second sampling and a ten-minute busy session do not exclude longer-lived leaks. Layout continues at roughly once per second in idle.

### Retained idle objects versus fresh idle

The post-realm snapshot began around idle +90 s, before the scheduled +120 s pressure probe. A separate fresh-profile, no-feed CDP launch took its snapshot at approximately +60 s. Both snapshots force GC. The fresh heap probe is a structural comparison, not another gate run, and differs in warm-up duration and tracing state. Natural post-realm +30–90 s V8 used bytes ranged 6.409–7.085 MB; the snapshot reduced reported V8 used size from 6.747 to 4.674 MB.

| Heap snapshot category | Post-realm bytes | Fresh bytes | Difference bytes |
| --- | ---: | ---: | ---: |
| All node shallow bytes (includes native nodes) | 7,395,090 | 4,662,565 | +2,732,525 |
| Code | 2,253,224 | 1,036,164 | +1,217,060 |
| Arrays | 831,828 | 151,516 | +680,312 |
| Native nodes | 3,249,838 | 2,815,381 | +434,457 |
| Objects | 285,612 | 93,376 | +192,236 |
| Strings, including concatenated strings | 422,548 | 263,592 | +158,956 |

These categories overlap process pools and must not be added to the earlier private-byte table. Code/native categories do not identify individual product owners. Structural ownership gives a much smaller directly actionable retained graph:

| Structural owner | Post-realm / fresh count | Post-realm shallow bytes | Estimated group-retained bytes |
| --- | ---: | ---: | ---: |
| Realm visit (`window`, `fights`, `focused`) | 1 / 0 | 40 | 236,283 |
| Fight DTO (`target_generation`, `players`) | 64 / 0 | 7,680 | 124,636 |
| Home visit windows | 3 / 1 | 144 | 9,327 |
| Live snapshot | 1 / 1 | 68 | 4,604 |
| Player-series DTO | 5 / 0 | 140 | 2,704 |

Group-retained estimates remove each structural group from the snapshot's strong-edge graph and sum newly unreachable shallow bytes. Weak edges are excluded; ephemeron semantics are not modeled. Groups overlap, so these are estimates and cannot be summed. A strong retainer path to a fight traverses a Map, a visit object, `window`, and `fights`, consistent with `createHomeLiveWindow`'s retained visits at `app/ui/src/lib/home/v2/live.ts:31–34`. This supports the owner hypothesis without claiming exact source-variable dominance. Wire-script strings total only 14,812 bytes versus 14,784 fresh. The snapshot does not show a multi-megabyte backlog of wire strings or complete 1,536-fight DTO history.

## Validation and limits

Both collector self-tests, `pwsh -NoProfile -File tests/performance/memory-tooling.test.ps1`, dependency installation, frontend/release builds, byte-identical hidden-map build, maintained gate/peak/attribution reports, and both heap probes completed successfully. `npm ci` reported one moderate and two critical dependency audit findings; dependency changes were outside this measurement task. Build warnings included the existing Vite chunk-size and ts-rs serde-attribute warnings. Two gate attempts and one floor attempt were rejected as incomplete, as recorded above. Evidence consistency checks passed for phase coverage, concurrent sums, gate arithmetic, allocator reconciliation, heap totals and sanitized fields. No tooling fix was needed; the ad hoc aggregate import initially failed because stdin execution supplied no `process.argv[1]`, and succeeded after supplying an entry-point sentinel without changing the analyzer.

This does not verify live Npcap/gameplay, installed packages, startup acceptance or long-session stability. The high-idle run was not instrumented, so its pool ownership remains unresolved. Native allocations below process level remain unmeasured. The next task should reproduce the idle split on one fixed host/runtime/assets set and, for historical code attribution, include `46d2569` on that same set.

## Ranked product-change candidates

Ranking combines measured ownership, allocation cost and uncertainty. Values are observations or explicitly labeled estimates, **not promised savings**.

| Rank | Candidate / `file:line` owner | Bytes accounted for | Next experiment |
| --- | --- | --- | --- |
| 1 | Avoid repeated history-entry copies in event invalidation: `app/ui/src/lib/shell/Shell.svelte:178`, `app/ui/src/lib/navigation/history.ts:93` | **Estimated allocation flow:** 21,158,916 bytes / 60.457 s (27.1%); retained/private share unknown | Read the route once per event or expose an immutable route view; repeat allocation sampling and both gates, preserving history navigation semantics. |
| 2 | Reduce unchanged live-projection churn: `app/ui/src/lib/bridge/native.ts:24`, `app/ui/src/lib/bridge/live.ts:300`, `app/ui/src/lib/home/v2/live.ts:269` | **Estimated flow:** decoder 9,389,816 and window snapshot 2,360,208 bytes/sample. **Measured diagnostic pool envelope:** V8 46.4–49.0 MB busy, 7.9 MB early idle; exact site share unknown | Compare payload/projection cadence and unchanged-array reuse, then measure V8/Blink capacity and idle variance across matched repeats. Do not infer run 1's extra 31 MB from this diagnostic. |
| 3 | Attribute and reduce native retained capacity: `crates/maxed-engine/src/engine.rs:113`, `:122`, `:270`; `app/src-tauri/src/bridge.rs:208` | **Measured process envelope:** about 22.8–23.2 MB mean busy excess over idle; 12.2–12.6 MB idle peak contribution above fresh. Ownership within Maxed unknown | Instrument engine fights/series, bridge buffers and writer/SQLite separately, then test capacity release at visit/session boundaries without losing observations. |
| 4 | Reduce repeated DOM/layout work: `app/ui/src/lib/home/RealmHome.svelte:53`, `app/ui/src/lib/shell/Shell.svelte:313` | **Measured pool envelope:** Blink 24,772,608 busy / 13,893,632 idle bytes; approximately 4.5 layouts/s busy. No per-component byte attribution | Measure stable keyed rows and narrower update boundaries; compare DOM counts, Blink objects/capacity and visible rendering correctness. |
| 5 | Review bounded visit-window retention: `app/ui/src/lib/home/v2/live.ts:31` | **Estimated retained graph:** 236,283 bytes for one visit; overlapping fight graph 124,636 bytes. Far smaller than renderer idle excess | Verify expected visit lifetime and 64-fight bound over repeated visits; retain complete history semantics. Lower priority than allocation/native attribution. |
