# Live-window allocation churn — 2026-10-07

## Before-merge revision: identity memoization (not separately measured)

The measured build below used a watermark-based snapshot skip. That design was replaced with identity memoization before merge: every accepted fight again calls `putFight`, and `put`, kill eligibility, membership/removal and focus logic retain the base sequence semantics. Bounded fights are cached by source object, `keepSeries` and the `verifiedSelf` result; kill summaries are cached by source object because `self` and `partial` read only its fields. Map value revisions reuse unchanged arrays, while separate sequence revisions keep `fightSequences` current. The scoped-fight cache and dungeon sorting copy remain.

**The replacement design was not separately measured. The next measurement will cover it.** All build hashes, allocation estimates and gate results below describe the earlier watermark-skip design, not the final implementation.

Replacement validation: **584 focused tests passed**; full UI **1,886 passed with two stress-preview timeout failures**, followed by an isolated **15/15** pass for that file. Both type checks reported **0 errors / 0 warnings**, and `git diff --check` passed. Direct comparison with the base confirmed that `put` and the entire `snapshot()` callback are unchanged. Identity tests now check reused bounded objects alongside advancing fight sequences, and cover changed kill `self`/`partial` data without watermark-specific tests.

**Earlier-design result:** both synthetic gates pass the revised limits, but neither meets the investigation targets. Targeted allocation sites are lower; total sampled allocation is 0.934 MB/s versus 0.907 MB/s previously. These runs do not resolve a causal memory saving.

## Earlier design: change and validation

`scopedFights` caches by snapshot and window identity, including a null window. Published inputs are immutable. The caller audit found one in-place mutation: `dungeonObservations` now sorts a copy. Connection visual discovery filters/maps the result; `snapshotFight` and the live-window focus path only read it. The new test verifies that dungeon sorting leaves the cached order intact.

The live window remembers each retained fight's source and bounding context. An identical source skips `putFight` only when verified character identity, series retention and kill bookkeeping still match. Revisions on the four projection maps cover writes, removal, trim, chart stripping, reset and visit restoration. Unchanged maps reuse `fights`, `fightSequences`, `realmKills`, `realmAnnouncements` and `events`; each emitted window still has a new identity.

Membership checks, `activeIds` and the `row.sequence <= sequence` removal condition are unchanged. A skipped snapshot advances an internal write watermark, so delayed events cannot overwrite it, while retaining the older published projection sequence. Accepted snapshot sequences are monotonic: an older projection sequence and the sequence a re-put would have assigned are both at most the current snapshot sequence, so neither wins `scopedFights`' strict-newer comparison. A still-newer event row keeps its sequence and freshness. Kill rows also advance their write watermark; an evicted or revised kill row, changed visit eligibility, character verification or series selection forces reprocessing. A fight cannot be cached as unchanged while a newer retained kill summary blocks its correction. Events still call `putFight`; snapshot focus invalidation and `bound`/`selectBossFight` selection logic are unchanged.

Focused Home/rail/navigation tests: **584 passed**. Full UI: **1,886 passed, two 20-second stress-preview timeouts**; the isolated `tests/preview/runs-preview.test.ts` rerun passed all **15 tests**. Both `check` and `check:tests` reported **0 errors / 0 warnings**; four Node tests passed. Four existing tests were updated, with coordinator approval, to construct fresh snapshot identities between steps; all behavioral assertions and step ordering remain unchanged. New coverage includes projection identity, snapshot/window cache invalidation, sorting isolation, changed/removed fights, fresher and delayed events, kill revisions, verified-local bounding and selected series restoration.

## Earlier design: measurement method

Workload and phase definitions follow the [route-read report](2026-10-07-route-read-churn.md) and [rebaseline](2026-10-07-live-realm-rebaseline.md). Source is `6eb6560f02fff690724d59972e3cd5681af5835b` plus the earlier watermark-skip design. Dependencies were restored with `npm.cmd --prefix app/ui ci`; the built frontend and locked release use `maxed-app/custom-protocol,maxed-app/perf-live`, with the documented temporary `TAURI_CONFIG` profile/window configuration. Executable SHA-256: `71fe469a1b14ff27442a9063134f76113e13c35d0c0f692e2776068d91f04687`. The hidden-source-map build reproduced byte-identical JavaScript, and bundled synthetic resource hashes matched. Collector, interactive-startup and memory-tooling self-tests passed before launch.

Same host as the route-read report: Windows 11 Pro 10.0.26300, Ryzen 7 5825U (8 cores / 16 logical processors), 33,718,435,840 bytes usable RAM, Rust 1.99.0, Node 22.16.0, npm 10.9.2 and PowerShell 7.6.6. Raw evidence and source maps remain under the temporary `maxed-live-window-churn-20261007-ffb6355b2ae9488a9a5c69e9091541fe` directory. Measurements occurred October 8 in America/Chicago and UTC; the filename retains the task's requested date.

All three launches were sequential, each with an empty profile at the compiled temporary path. No build, test or heavy analysis overlapped a workload. Normal installed-client asset discovery remained enabled. No collector attempts failed or were excluded. Both uninstrumented gates used no CDP or browser-argument override, verified acceleration off and busy/idle rendering without subscription failure, and had no detected competing load. Native checks confirmed visible, responsive, non-minimized windows. Each collected 840 seconds with **810 complete samples**, zero incomplete memory samples and zero incomplete CPU intervals. Busy durations were **613.129 / 613.568 seconds**. Maintained gate, peak and attribution reports completed; independent phase arithmetic and concurrent-sum checks agreed.

An initial cross-check compared departure against only the maintained idle +0–30 window and failed. Combining that window with busy +610 through the logged idle marker matched the independently calculated departure peaks; neither the measurements nor report values changed.

The synthetic workload has 1,536 fights, 24 players, 320 bags and 24 hits/s, with 610 scheduled busy seconds followed by Nexus and 180 scheduled idle seconds. Steady busy is +30–580 seconds; departure is busy +610 through idle +30; the busy gate ends at the logged idle marker; the idle gate is +30–175 seconds. Diagnostic allocation sampling uses busy +330–390 seconds and a 32,768-byte interval, including minor- and major-collected objects. Diagnostic pressure/GC is not gate evidence. All values are synthetic, visible-window, non-packaged evidence; no live-gameplay, Npcap, installed-package or startup acceptance is claimed.

## Earlier design: allocation flow

The separate `-Mode after -Diagnostic -Attribution` run collected 930 seconds with **893 complete samples**, seven successful allocator dumps, no incomplete memory samples or CPU intervals, zero profiler errors, verified busy/idle rendering and no detected competing load. WebView2 was **154.0.4258.62**, with acceleration off. Native checks confirmed a visible, responsive, non-minimized window; diagnostic document checks also reported visible.

Busy sampling lasted **60.266 seconds**: **1,708 samples**, **56,283,968 estimated allocated bytes**, **0.9339 MB/s**. This is about **3% above** the route-read run's **0.9068 MB/s**, despite lower sampled self bytes at the targeted `snapshot` and `scopedFights` sites. This single diagnostic does **not** establish an overall allocation reduction. These are allocation-flow estimates, not retained bytes or promised memory savings.

Top ten sites in this run, with the corresponding published route-read values for comparison. “Outside top ten” does not mean zero allocation.

| Site | Estimated bytes, this run | Estimated bytes, route-read |
| --- | ---: | ---: |
| Byte-channel decode, `bridge/native.ts:24` | 8,703,572 | 9,688,376 |
| Native/runtime `push` | 4,150,932 | 4,122,792 |
| Svelte `update_reaction`, `internal/client/runtime.js:225` | 3,213,152 | 3,213,152 |
| Native/runtime `add` | 2,664,620 | 1,983,240 |
| Native/runtime `delete` | 2,132,744 | 1,903,116 |
| Svelte branch callback, `internal/client/dom/blocks/branches.js:76` | 1,869,584 | 1,443,232 |
| Native/runtime `set` | 1,517,752 | 2,010,708 |
| `bound`, `home/v2/live.ts:253` | 1,410,248 | 1,508,576 |
| `isRealmFightActive`, `shell/rail.ts:57` | 1,278,536 | Outside top ten |
| Svelte `reconcile`, `internal/client/dom/blocks/each.js:432` | 1,157,676 | Outside top ten |

The two other route-read top-ten sites fell outside this run's top ten: `snapshot` (`home/v2/live.ts:302`) went from **2,229,068 to 786,800** estimated self bytes; `scopedFights` (`shell/rail.ts:91`) from **1,935,236 to 688,776**. File totals were **5,837,544** for `home/v2/live.ts` versus **6,557,844**, and **4,032,924** for `shell/rail.ts` versus **4,459,828**. Native/runtime sites are not assigned to callers or added to those totals. Sampling variability and changed allocation distribution limit attribution; reduced work at individual sites does not imply reduced total flow or private memory.

## Earlier design: gate results

Measured decimal MB; renderer cores are CPU-seconds per wall-second over the busy gate. Windows are defined above. Startup/seed peaks outside them are not gate results.

| Run | Steady busy mean | Busy gate peak | Departure peak | Idle gate peak | Renderer busy cores |
| --- | ---: | ---: | ---: | ---: | ---: |
| Route getter 1 | 225.006 | 260.051 | 246.493 | 172.306 | 0.06878 |
| Route getter 2 | 227.788 | 271.155 | 247.325 | 175.854 | 0.03646 |
| Live window 1 | 222.131 | 261.104 | 232.915 | 174.629 | 0.08485 |
| Live window 2 | 223.769 | 264.675 | 237.666 | 169.185 | 0.04681 |

Busy gates have **576 / 577** samples; idle gates **136 / 137**. Steady-busy means use **516 / 517** samples; departure windows have **31** each.

| Run / phase | Exact peak bytes | Revised limit | Investigation target |
| --- | ---: | --- | --- |
| 1 busy | 261,103,616 | Pass, <300,000,000 | Fail, <140,000,000 |
| 1 idle | 174,628,864 | Pass, <200,000,000 | Fail, <130,000,000 |
| 2 busy | 264,675,328 | Pass, <300,000,000 | Fail, <140,000,000 |
| 2 idle | 169,185,280 | Pass, <200,000,000 | Fail, <130,000,000 |

Concurrent measured contributions at each new idle gate peak:

| Process | Run 1 bytes | Run 2 bytes |
| --- | ---: | ---: |
| Renderer | 67,072,000 | 64,860,160 |
| Browser | 48,820,224 | 49,926,144 |
| Maxed | 47,546,368 | 43,745,280 |
| Storage utility | 8,167,424 | 7,630,848 |
| Crashpad | 3,022,848 | 3,022,848 |
| **Combined** | **174,628,864** | **169,185,280** |

Steady-busy means and departure peaks are lower in these observations, while busy peaks remain within the route-read runs' range. Idle run 1 is **2.322 MB above** the lower route-read idle peak; run 2 is **3.121 MB below** it. The new idle peaks differ by **5.444 MB**, and the earlier identical-code rebaseline varied by **30.814 MB**. These gate runs cannot resolve the change's effect on private memory; no savings are promised. Renderer busy CPU also varies substantially and does not establish a CPU saving or whole-app CPU acceptance. Polling can miss shorter peaks, and one allocation sample does not establish a stable allocation-rate change.
