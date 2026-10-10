# Live realm UI allocation churn - 2026-10-06

Phase 2a investigation for issue #40. **Sampled JS allocation fell 93.1%, from 47.965 MB/s to 3.304 MB/s. The uninstrumented busy peak increased to 552,910,848 bytes; idle fell to 215,126,016 bytes. Both memory gates still fail.** This is an allocation reduction, not a completed memory fix or live-game verification.

The [JSON evidence](2026-10-06-live-realm-ui-churn.json) contains exact bytes, sanitized process decompositions, heap dumps reduced to allocator aggregates, sampling details and comparisons with [Phase 1](2026-10-06-live-realm-attribution.md). Raw profiles, process identities, traces, allocation profiles and source maps remain in temporary storage.

## Implementation

- Numeric deltas without drop changes/removals retain the drops array. The visit controller retains its published drops array when the same objects remain in the same order. Realm and dungeon scoping reuse unchanged results while still checking session, run and visit boundaries.
- Realm and dungeon Home share one `homeLoot(drops, visuals)` result for rank, best bag, remaining bags and slot counts. BagList reuses the ranked result; overflow summaries, item expansion and loot evidence are cached with weak input keys. Evidence reasons and partial status still compose from the current observation.
- Ranking parses exact timestamps once per drop and item sorting computes tier keys before comparisons. Visual lookups use a shared index, preserving first-match object lookup and last-match tier semantics. Loot visual-type collection is cached separately from changing fight observations.
- Snapshot holders in Shell, the rail and Home use `$state.raw`; accepted observations already replace DTOs. Catalog arrays remain immutable after publication. Caches require row/array replacement for revisions and are released with their weak-key owners.
- Complete retained visit history, existing bounds/evidence reasons, ordering, focus rules, kill handling and bridge sequence/generation/resync decisions are unchanged. No Rust, capture or native-memory-policy code changed.

Identity tests cover unchanged inputs, changed/new/removed bags, catalog replacement and changed scopes. A frozen copy of the prior loot algorithms checks ranking, exact timestamp/ID ties, duplicate IDs, item tiers, legacy/unknown contents, quantities and evidence reasons across 150 deterministic randomized cases. The existing enchant-revision UI test now sends an immutable replacement bag, matching bridge delivery, with its assertions unchanged.

## Build and environment

- Source: `48f4974fe954c68595c398bf78d040313bee46ec`, branch `perf/40-attribution`, clean at build; based on Phase 1 commit `b7d663a`. Only local commits; no push or PR.
- Executable SHA-256: `647bc06c45ca67b85d094de044e3e5e17c065b7c36c5fdeaaa9620bb5bfada9f`.
- UI bundle SHA-256: `132684abbd9f340f13ab6d1a7a93d64f768e6b9173b0f813a85f74f15c4d7868` (`index-BZc9ae7O.js`).
- Release features: `maxed-app/custom-protocol,maxed-app/perf-live`; built frontend, non-packaged executable; bundled synthetic fixture hashes verified.
- Same host as Phase 1: Windows 11 Pro `10.0.26200`, Ryzen 9 7900X, 12 cores / 24 logical processors, 67,794,673,664 bytes usable RAM; WebView2 `154.0.4258.53`, Rust `1.99.0 (b940084d7 2026-09-28)`, Node `26.7.0`, npm `11.19.0`.
- Acceleration off, verified from owned browser arguments. Each launch used a fresh temporary native/WebView profile. Normal installed-client asset discovery produced nine cache files / 103,159,076 bytes per successful profile; events and identities were synthetic. No normal Maxed profile or real capture was used.

The build followed Phase 1 exactly: `npm ci`, a fresh temporary absolute `TAURI_CONFIG.identifier` and main-window `dataDirectory` with the same window options, `npm run build`, then:

```powershell
cargo build --locked --release -p maxed-app -p maxed-cli --features maxed-app/custom-protocol,maxed-app/perf-live
```

Runs were strictly sequential with no builds/tests during a workload. The uninstrumented `live-realm-memory.ps1 -Mode after` run and its report finished before the diagnostic `-Mode after -Diagnostic -Attribution` launch. Used profiles were archived inside temporary evidence and the exact compiled profile path recreated empty. After all runs stopped, `npm.cmd --prefix app/ui run build -- --sourcemap hidden` produced byte-identical JavaScript. Maps were moved to temporary evidence; the native executable was not rebuilt and its hash was rechecked. `live-realm-attribution-report.mjs` then analyzed the completed run offline.

One diagnostic attempt is **excluded**: after 63.319 seconds the collector recorded one incomplete sample (`ProcessCommandException`, counter-read line 77); the driver subsequently stopped the unfinished profiler. No allocation sample was captured. A fresh-profile retry using the unchanged executable and collector completed with zero accounting gaps and no profiler errors. This failure is retained in temporary evidence and summarized in JSON.

## Uninstrumented comparison

Workload: 1,536 fights, 24 players, 320 bags and 24 hits/s; 610 scheduled busy seconds, then Nexus and 180 scheduled idle seconds. Actual candidate busy time was 612.739 seconds versus 612.492 in Phase 1. Idle comparison remains +30 through +175 seconds after Nexus. Both rendered phases were verified, with no subscription failure, missing counters or detected competing compiler/test/active-Node load.

| Phase | Phase 1 combined peak, bytes | Candidate combined peak, bytes | Change, bytes | Candidate result |
| --- | ---: | ---: | ---: | --- |
| Busy | 511,078,400 | 552,910,848 | +41,832,448 | **FAIL**, limit <200,000,000 |
| Idle | 221,417,472 | 215,126,016 | -6,291,456 | **FAIL**, limit <150,000,000 |

Busy increased **41,832,448 bytes (8.2%)**; idle decreased **6,291,456 bytes (2.8%)**. This single candidate run does not establish the cause or repeatability of the larger transition peak. The previous 134,463,488-byte fresh-profile floor was not rerun.

Concurrent contributions **at each combined peak**, in exact bytes (independent process peaks must not be added):

| Process | Phase 1 busy | Candidate busy | Phase 1 idle | Candidate idle |
| --- | ---: | ---: | ---: | ---: |
| Renderer | 338,472,960 | 420,728,832 | 102,592,512 | 100,114,432 |
| Browser | 102,576,128 | 65,466,368 | 56,467,456 | 55,398,400 |
| Maxed | 58,929,152 | 55,685,120 | 51,519,488 | 48,730,112 |
| Storage utility | 7,983,104 | 7,925,760 | 7,815,168 | 7,868,416 |
| Crashpad | 3,117,056 | 3,104,768 | 3,022,848 | 3,014,656 |

The candidate busy peak was the last busy sample, 0.978 seconds before the recorded Nexus transition, with 420,728,832 renderer bytes. Browser contribution at that sample was lower than Phase 1, while renderer contribution was higher. The +600-second diagnostic dump is a different observation and does not explain this peak.

| Independent process peak | Phase 1 busy | Candidate busy | Phase 1 idle | Candidate idle |
| --- | ---: | ---: | ---: | ---: |
| Renderer | 372,256,768 | 420,728,832 | 102,592,512 | 100,114,432 |
| Maxed | 80,011,264 | 77,193,216 | 51,519,488 | 48,730,112 |

Renderer busy CPU averaged 0.121740 -> 0.091782 CPU-seconds/wall-second (cores), or 0.5073% -> 0.3824% of this 24-logical-processor machine. These renderer-only numbers do not certify the whole-app CPU gate. The collector's 595,505,152-byte overall peak includes startup/seed outside the busy window and is not a startup-memory acceptance result.

## Diagnostic attribution

Only the successful retry is used below. Its combined phase peaks were **542,216,192 busy / 235,921,408 idle bytes**, versus Phase 1's diagnostic 534,347,776 / 246,505,472 bytes. CDP adds a tracing utility and perturbs memory; these are not release-gate measurements. The retry had complete PID-plus-birth accounting, verified rendering, no detected competing load and seven successful dumps.

### Allocation rate and remaining sites

| Sampling | Phase 1 | Candidate |
| --- | ---: | ---: |
| Seconds | 60.376 | 60.187 |
| Estimated self bytes | 2,895,918,112 | 198,850,052 |
| Estimated bytes/second | 47,964,722.936266 | 3,303,870.470367 |
| Samples | 87,564 | 6,043 |

Both samples used a 32,768-byte sampling interval during busy +330 to +390 seconds and included objects collected by minor and major GC. The rate fell **93.1119%**. These are sampled allocation-flow estimates, not retained/live heap or process private bytes.

No sampled self-allocation sites mapped to `home/v3/loot-rank.ts`, `home/v2/loot.ts` or `BagList.svelte` in the candidate window. This is not proof of zero allocations. The remaining `screen-data.ts` sites were damage-row mapping and `fightDps`, rather than the removed loot-count callback. Native `set` fell from 476,184,576 to 8,070,996 estimated bytes; native `Map` from 204,714,612 to 3,412,812. Native frames are not assigned to particular callers.

Largest remaining sites, estimated self bytes over the 60.187-second candidate sample:

| Site | Source | Estimated self bytes |
| --- | --- | ---: |
| Native/runtime push | `(native or runtime)` | 17,809,704 |
| Navigation state getter | `src/lib/navigation/history.ts:93` | 14,929,288 |
| Fight visual-type callback | `src/lib/home/visuals.ts:44` | 13,908,656 |
| Svelte update_reaction | `node_modules/svelte/src/internal/client/runtime.js:225` | 12,819,008 |
| Native channel decode callback | `src/lib/bridge/native.ts:24` | 9,130,700 |
| Native/runtime set | `(native or runtime)` | 8,070,996 |
| Native/runtime delete | `(native or runtime)` | 7,053,852 |
| scopedFights | `src/lib/shell/rail.ts:87` | 6,853,952 |
| Unmapped native/runtime callback | `(native or runtime)` | 6,235,988 |
| Svelte branch callback | `node_modules/svelte/src/internal/client/dom/blocks/branches.js:76` | 5,542,296 |
| isRealmFightActive | `src/lib/shell/rail.ts:57` | 5,343,372 |

The next product-level investigations are navigation-state copying, repeated fight visual collection, bridge payload decoding and shared fight scoping/focus projections. Their sampled costs are much smaller than the removed loot passes. Any focus work must preserve the existing activity/focus/kill rules. Svelte reaction cost and native/runtime frames need caller-level investigation before assigning ownership.

### Renderer heap checkpoints

Exact bytes. V8 allocated objects may include collectible garbage; they are not a post-GC live set. Blink object bytes likewise differ from committed capacity.

| Measure | Phase 1 +300 | Candidate +300 | Phase 1 +600 | Candidate +600 |
| --- | ---: | ---: | ---: | ---: |
| Dump private bytes | 226,000,896 | 197,496,832 | 210,481,152 | 196,841,472 |
| V8 heap spaces | 104,730,624 | 96,866,304 | 102,174,720 | 100,864,000 |
| V8 allocated objects | 41,377,424 | 26,554,336 | 20,606,436 | 24,862,236 |
| V8 space minus objects | 63,353,200 | 70,311,968 | 81,568,284 | 76,001,764 |
| Blink GC committed | 39,125,072 | 28,573,696 | 39,059,456 | 28,573,696 |
| Blink GC allocated objects | 10,823,152 | 6,440,464 | 3,540,640 | 7,083,736 |
| Blink GC pooled | 22,151,168 | 12,582,912 | 22,675,456 | 12,058,624 |

**The proposed large V8 capacity reduction did not materialize.** Despite much lower allocation flow, heap space remained 96.9-100.9 MB; at +600 it was only 1,310,720 bytes below Phase 1. Blink committed capacity was 10,485,760 bytes lower at +600, but allocated objects were higher at that point. GC timing and capacity are distinct from allocation rate.

Candidate +600 renderer reconciliation: malloc 46,275,536 bytes, Blink GC 28,573,696, Blink PartitionAlloc 5,730,304, V8 spaces 100,864,000, and a **15,397,936-byte unattributed/basis gap**, against 196,841,472 dump private bytes. These are distinct selected pools; suballocations are not added again. Provider size and private commit use different accounting bases. Neither this reconciliation nor the lower point-in-time dump demonstrates a smaller transition peak.

Decoded image-cache accounting remained 268,435,456 bytes at +300 and 134,217,728 at +600, the same as Phase 1. These logical/shared resource costs cannot be deducted from private bytes. Transition-time image/native allocation ownership remains a separate investigation.

At natural idle +30, renderer dump private bytes were 98,332,672 versus 103,497,728 before; V8 space was 17,960,960 versus 18,747,392. Post-GC +220 values were 75,063,296 renderer bytes and 7,340,032 V8-space bytes. Pressure/GC aggregates remain in JSON as diagnostics only; driver teardown before GC is not credited to GC.

### DOM and activity metrics

| Interval | Nodes | JS listeners | Documents | Layouts/s | Style recalcs/s | Script seconds/s | Task seconds/s |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| Busy +30-300 | 4,831 -> 4,887 | 69 -> 73 | 2 | 4.3430 | 2.5311 | 0.016333 | 0.048581 |
| Busy +400-610 | 4,893 -> 4,893 | 73 -> 73 | 2 | 4.5049 | 2.5229 | 0.016729 | 0.051290 |
| Idle +30-90 | 1,145 -> 1,145 | 27 -> 27 | 2 | 1.0029 | 0.0000 | 0.000618 | 0.002089 |
| Idle +200-250, after GC | 1,158 -> 1,158 | 27 -> 27 | 2 | 1.0055 | 0.0000 | 0.000173 | 0.001661 |

Late-busy nodes/listeners stayed at the same 4,893 / 73 as Phase 1. Late-busy script time fell from approximately 0.0719 to 0.0167 seconds/second. Layout frequency remained about 4.5/s busy and 1/s idle. Ten-second samples cannot exclude short spikes, other leaks or longer-session growth.

## Validation and remaining limits

- `npm ci`; full Vitest **154 files / 1,537 tests passed**, including live-payload, native-transport, Home realm/dungeon/rail and smoke replay; Node tests **4 passed**.
- `npm run check` and `npm run check:tests`: **0 errors / 0 warnings**; UI production build and locked perf-live/custom-protocol release build passed; `git diff --check` passed.
- Collector self-test; interactive-startup self-test **83 assertions**; memory-tooling tests under PowerShell 7 and Windows PowerShell 5.1; diagnostic/analyzer self-tests passed. Bundle/resource hashes and byte-identical source maps verified.
- No Rust changed, so Rust lint/test suites were not rerun. Build output retained existing ts-rs attribute and Vite chunk-size warnings.
- No live Npcap/gameplay, installed package, startup, installer or long-session acceptance. One successful candidate workload and one successful diagnostic do not establish run-to-run variance.
- Both memory gates remain unmet. Keep the measured allocation/CPU improvement distinct from heap-capacity and transition/native-retention work; do not claim this phase fixed the memory issue.
