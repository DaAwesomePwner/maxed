# Route-read allocation churn — 2026-10-07

**The history getter disappeared from sampled allocation sites; estimated allocation fell from about 1.29 to 0.907 MB/s. Both new synthetic gate runs pass the revised limits, but these runs do not resolve a causal idle-memory saving.** Neither meets the investigation targets.

## Change and validation

`createRouter` exposes a borrowed `Readonly<Route>` getter. Shell reads it once per `historyInvalidates` invocation and passes that local to the same five predicates in the same short-circuit order. The existing copying `state` getter is unchanged. The predicates only inspect route fields; none mutates the borrowed route.

Searching `app/ui/src` found only the five replaced per-event `router.state` reads. Other `history.ts` copies serve initialization, publication/subscription, traversal restoration and explicit position saves, rather than unconditional live-event/snapshot route reads; they remain unchanged. Scroll/focus saves can be frequent, but changing their persistence semantics is outside this fix.

Tests cover initial route, `replaceInitial`, navigation, save identity, back/forward/native traversal and the existing state-copy isolation. A real routed Shell accepts a fight update while a `JSON.stringify` spy rejects any serialization of a history entry. Filtering for entry fields avoids coupling the assertion to unrelated JSON use; this exercises all five predicates without requiring a component-private test seam.

Navigation/Shell: **121 tests passed**. Full UI: **1,882 passed, two 20-second timeout failures** in the stress cases of `tests/preview/runs-preview.test.ts`; its isolated rerun passed all **15 tests**. Both `check` and `check:tests` reported **0 errors / 0 warnings**; the four Node tests passed. Release build, collector/startup/tooling self-tests, source-map byte comparison, bundled synthetic resource hashes and `git diff --check` passed. No test timeout or assertion was weakened.

## Method

Shared workload and phase definitions follow the [rebaseline](2026-10-07-live-realm-rebaseline.md) and [attribution method](2026-10-06-live-realm-attribution.md). Source was `c9f8a7f9009a2856b9ea24fdeb395e0b5bba71ea` plus this change. After `npm.cmd --prefix app/ui ci`, the built frontend and locked release used `maxed-app/custom-protocol,maxed-app/perf-live` and the documented temporary `TAURI_CONFIG` profile/window configuration. Executable SHA-256: `f88e4cdbe244632dcc52055b9757a63ba8b1eff4ee79aa7dbd53207cc98aae68`. Hidden source maps reproduced byte-identical JavaScript and were moved to temporary evidence.

Same host/runtime as the rebaseline: Windows 11 Pro 10.0.26300, Ryzen 7 5825U (8 cores / 16 logical processors), 33,718,435,840 bytes RAM, WebView2 154.0.4258.62, Rust 1.99.0 and Node 22.16.0. Acceleration was off. Each sequential launch used an empty profile at the compiled temporary path; no build, test or heavy analysis overlapped collection. Native checks confirmed visible, responsive, non-minimized windows; both rendered phases were verified without subscription failure. No collector attempts failed or were excluded.

One `live-realm-memory.ps1 -Mode after -Diagnostic -Attribution` run collected 930 seconds, 891 complete samples, seven successful dumps and zero profiler/accounting errors. Two subsequent `-Mode after` runs used no CDP and collected 840 seconds each: 812 / 811 complete samples, zero incomplete memory samples or CPU intervals, no detected competing load. Gate busy durations were 613.328 / 613.458 seconds. The maintained report scripts produced temporary summaries; steady-busy means and departure peaks were calculated from the same totals CSV windows as the rebaseline. All raw records, profiles, traces and maps remain under the temporary `maxed-route-read-churn-20261007` evidence directory. Measurements occurred October 7 in America/Chicago (UTC October 8).

## Allocation flow

Busy +330–390 sampling lasted **59.790 seconds** at the same 32,768-byte interval, including objects collected by minor and major GC: **1,646 samples**, **54,218,960 estimated allocated bytes**, **0.9068 MB/s**. This is approximately **30% less sampled allocation flow** than the rebaseline's approximately 1.29 MB/s, not retained memory or promised savings. No sampled self-allocation site mapped to `navigation/history.ts`; its rebaseline getter accounted for 21.2 MB / 27.1% of sampled self bytes.

Top ten sites, source-mapped estimated self bytes:

| Site | Estimated bytes |
| --- | ---: |
| Byte-channel decode, `bridge/native.ts:24` | 9,688,376 |
| Native/runtime `push` | 4,122,792 |
| Svelte `update_reaction`, `internal/client/runtime.js:225` | 3,213,152 |
| `snapshot`, `home/v2/live.ts:269` | 2,229,068 |
| Native/runtime `set` | 2,010,708 |
| Native/runtime `add` | 1,983,240 |
| `scopedFights`, `shell/rail.ts:87` | 1,935,236 |
| Native/runtime `delete` | 1,903,116 |
| `bound`, `home/v2/live.ts:220` | 1,508,576 |
| Svelte branch callback, `internal/client/dom/blocks/branches.js:76` | 1,443,232 |

Unchanged candidate files total 9,688,376 sampled self bytes in `bridge/native.ts`, 6,557,844 in `home/v2/live.ts`, and 4,459,828 in `shell/rail.ts`. Native/runtime sites are not assigned to those callers or added again.

## Gate results

Measured decimal MB; renderer cores are CPU-seconds per wall-second over the busy gate. Steady busy is +30–580 seconds; departure is busy +610 through idle +30; idle gate is +30–175 seconds. Busy gate ends at the logged idle marker. Startup/seed peaks outside these windows are not gate results.

| Run | Steady busy mean | Busy gate peak | Departure peak | Idle gate peak | Renderer busy cores |
| --- | ---: | ---: | ---: | ---: | ---: |
| Rebaseline 1 | 232.943 | 267.497 | 255.873 | 208.040 | 0.07661 |
| Rebaseline 2 | 255.809 | 267.354 | 256.037 | 177.226 | 0.08575 |
| Route getter 1 | 225.006 | 260.051 | 246.493 | 172.306 | 0.06878 |
| Route getter 2 | 227.788 | 271.155 | 247.325 | 175.854 | 0.03646 |

Each new run has 577 busy-gate and 137 idle-gate samples.

| Run / phase | Exact peak bytes | Revised limit | Investigation target |
| --- | ---: | --- | --- |
| 1 busy | 260,050,944 | Pass, <300,000,000 | Fail, <140,000,000 |
| 1 idle | 172,306,432 | Pass, <200,000,000 | Fail, <130,000,000 |
| 2 busy | 271,155,200 | Pass, <300,000,000 | Fail, <140,000,000 |
| 2 idle | 175,853,568 | Pass, <200,000,000 | Fail, <130,000,000 |

Concurrent measured contributions at each new idle gate peak:

| Process | Run 1 bytes | Run 2 bytes |
| --- | ---: | ---: |
| Renderer | 69,066,752 | 68,567,040 |
| Browser | 49,020,928 | 49,733,632 |
| Maxed | 43,581,440 | 46,776,320 |
| Storage utility | 7,626,752 | 7,770,112 |
| Crashpad | 3,010,560 | 3,006,464 |
| **Combined** | **172,306,432** | **175,853,568** |

Steady-busy means and departure peaks are lower, but busy gate peaks do not consistently improve. The new idle peaks are only **4.919 / 1.372 MB below the lower rebaseline idle peak**, versus **30.814 MB variation between identical rebaseline runs**. These two runs cannot resolve the fix's effect on idle private memory; the lower idle readings must not be attributed to this fix. CPU observations also vary substantially and do not isolate a CPU saving. Allocation sampling and polling are estimates with finite coverage, and diagnostic pressure/GC results are not gate evidence. This is synthetic, visible-window, non-packaged evidence only: no claim about live gameplay, Npcap, installed packages or startup acceptance.
