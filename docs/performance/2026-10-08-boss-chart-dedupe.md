# Priority fight chart deduplication — 2026-10-08

**The sampled decoder site fell 26.8%, but total sampled allocation fell only 2.4% to 0.885 MB/s. Both uninstrumented runs pass the revised memory limits; idle-memory savings remain unresolved.** Neither run meets the investigation targets. This build combines the previously measured route-read fix, newly measured identity memoization, and chart deduplication; these runs do not isolate their individual effects on total allocation or private memory.

## Change and validation

Only open priority `FightUpdated` presentation envelopes omit `buckets` and `player_series`, with an explicit optional `fight_chart_omitted` marker. Priority means a current quest or boss/event-boss/miniboss/quest classification. Priority starts, ends and post-end updates retain compacted charts. Ordinary-fight shaping is unchanged. False markers are skipped during serialization and missing markers deserialize as false, preserving original envelope bytes and receipt digests. Rust owns the optional TypeScript binding; it was regenerated and reviewed.

Home merges the previous chart pair into marked update metadata **before** `boundedFight`. It checks stream, fight ID, session, run, target ID, target generation and start time. A WeakMap reuses merged source rows when the source and chart references are unchanged, preserving the merged `Rows`, bounded-fight and kill-summary caches. Bucket and series references move together; no prior matching row means no invented chart. Unmarked events and snapshots remain authoritative, including genuinely empty arrays and null series. `put` sequence comparison, snapshot membership/removal, `scopedFights`, event ordering and resync are unchanged by this fix.

Twelve new UI cases cover omitted versus genuinely empty charts, repeated source identity, snapshot refresh and rendered chart changes, no prior row, final close without a snapshot, departure batching, same-instance return, selection/caps, eviction, target/generation/start changes, stream/session retirement, stale sequences and resync. The frozen departure oracle is unchanged; all ten departure-batch tests pass. Rust tests cover every priority kind, ordinary/current-quest transitions, open/terminal variants, marker round trips/defaults, unchanged legacy bytes, and full storage envelopes/receipt digests despite stripped UI copies.

Validation:

- Full UI: **1,899 passed; one 20-second timeout** in the stress-story case of `tests/preview/runs-preview.test.ts`. Its isolated rerun passed all **15 tests**. The focused chart/departure run passed **22 tests**. No timeout or assertion changed.
- Affected Rust packages (`maxed-app`, `maxed-cli`, `maxed-engine`, `maxed-store`, `maxed-types`): **1,293 passed; 26 existing ignored tests** across completed package runs. The initial combined command stopped at `writer::tests::clear_chat_flushes_accepted_events_first`, which returned `Full` on its first `try_submit`. That test passed alone, and the full remaining store/types run passed (388 tests); the completed app/CLI/engine portion had 905 passes. Ignored tests require installed-client inputs or explicit performance scenarios; no tests were disabled.
- Windows `cargo clippy --locked --workspace --all-targets -- -D warnings`, `cargo fmt --check`, UI `check` and `check:tests` (both **0 errors / 0 warnings**), and the four Node tests passed. Binding export tests passed as part of `maxed-types`; Step B has no generated DTO diff.
- Locked release build, normal/hidden-map UI builds, JavaScript byte comparison, collector/startup/tooling/analyzer self-tests and final whitespace checks passed. npm ci reported the existing one moderate and two critical dependency audit findings; dependencies were unchanged. Existing ts-rs and Vite chunk-size warnings remain.

## Method

Source: `818619f222c4c870e882ec2d44c447c42d702083` plus the Home merge and its tests. That HEAD contains the Rust/DTO change and is based on merged memoization `05e751f`. Executable SHA-256: `ce9b0c2d1519e3f9ea28d79c1c1d9acc39b06cc5658b806747658e3a3114d951`.

The workload and phase definitions follow the [route-read report](2026-10-07-route-read-churn.md) and [rebaseline](2026-10-07-live-realm-rebaseline.md): 1,536 fights, 24 synthetic players, 320 bags, 24 hits/s, 610 scheduled busy seconds, then Nexus and 180 scheduled idle seconds. Steady busy is +30–580 s; departure is busy +610 through idle +30 s; the busy gate ends at the logged idle marker; idle gate is +30–175 s. Measured busy durations were **612.9919915 / 613.5524764 s**.

Same host/runtime as the route-read report: Windows 11 Pro `10.0.26300`, Ryzen 7 5825U (8 cores / 16 logical processors), 33,718,435,840 bytes RAM, WebView2 `154.0.4258.62`, Rust/Cargo `1.99.0`, Node `22.16.0`, npm `10.9.2`, PowerShell `7.6.6`. Acceleration was off, verified from owned browser arguments. Normal installed-client asset discovery remained enabled; each accepted temporary profile had 11 asset-cache files / 131,950,493 bytes, matching the rebaseline inventory size. Disk cache bytes are not RAM.

After `npm.cmd --prefix app/ui ci`, `TAURI_CONFIG` supplied an absolute temporary identifier/profile and main-window data directory (`create=false`, 1100×760, minimum 800×560). A normal UI build and `--sourcemap hidden` build produced byte-identical JavaScript. Maps were moved to temporary evidence **before** the native build, so they were not embedded. The locked release command was:

```powershell
cargo build --locked --release -p maxed-app -p maxed-cli --features maxed-app/custom-protocol,maxed-app/perf-live
```

One `live-realm-memory.ps1 -Mode after -Diagnostic -Attribution` run completed **930.4128001 s**, **890 complete samples**, seven successful dumps and zero profiler/accounting errors. Two accepted `-Mode after` runs used no CDP or browser-argument override: **840.3204418 / 840.3160613 s**, **811 / 811 complete samples**, zero incomplete memory samples or CPU intervals. Their busy/idle gate sample counts were **576/137** and **577/137**. All accepted launches were sequential, with no detected competing compiler/test/active-Node load. Native window checks confirmed visible, responsive and not minimized; both rendered phases were verified without subscription failure. Diagnostic document checks reported visible throughout.

One initial attempt for each gate failed closed after **195.4144875 / 273.5550326 s**, each with one incomplete sample and `ProcessCommandException` at collector line 77. Both are excluded, archived and repeated from empty profiles. No partial attempt is counted as a pass. The exact compiled profile path was recreated empty after every stopped launch; executable bytes did not change. No builds, tests or heavy analysis overlapped measured workloads. Raw logs, profiles, process CSVs, allocation data, traces and source maps remain in temporary `maxed-boss-chart-20261008` evidence. No existing Maxed profile or real traffic was used.

## Allocation flow and attribution

Busy +330–390 sampling lasted **59.822 s**, with a 32,768-byte sampling interval and objects collected by both minor and major GC included: **1,607 samples**, **52,968,340 estimated allocated bytes**, **0.885432 MB/s**. The route-read report measured 54,218,960 bytes over 59.790 s, approximately **0.907 MB/s**. The rate difference is **−2.36%**, a modest single-sample observation, not a demonstrated total-allocation or retained-memory reduction of 26%.

Top ten source-mapped self-allocation sites (paths under `app/ui/`):

| Site | Estimated bytes |
| --- | ---: |
| Byte-channel decode, `src/lib/bridge/native.ts:24` | 7,089,432 |
| Native/runtime `push` | 3,919,056 |
| Native/runtime `add` | 2,903,540 |
| Svelte `update_reaction`, `node_modules/svelte/src/internal/client/runtime.js:225` | 2,524,704 |
| Native/runtime `delete` | 2,395,248 |
| Home `snapshot`, `src/lib/home/v2/live.ts:323` | 1,966,860 |
| Svelte each-block `reconcile`, `node_modules/svelte/src/internal/client/dom/blocks/each.js:432` | 1,652,864 |
| Svelte branch `batch`, `node_modules/svelte/src/internal/client/dom/blocks/branches.js:76` | 1,574,456 |
| Native/runtime `set` | 1,485,812 |
| Realm focus path, `src/lib/shell/rail.ts:57` | 1,376,832 |

Owner comparisons, using the prior report's sampled self bytes rather than adding overlapping totals:

| Owner | Route-read bytes | Combined build bytes |
| --- | ---: | ---: |
| Decoder site / `bridge/native.ts` | 9,688,376 | 7,089,432 |
| Home `snapshot` site | 2,229,068 | 1,966,860 |
| `scopedFights` site (`shell/rail.ts:91` now) | 1,935,236 | 786,988 |
| Entire `home/v2/live.ts` | 6,557,844 | 5,870,028 |
| Entire `shell/rail.ts` | 4,459,828 | 3,672,172 |

The decoder site's **2,598,944-byte / 26.8%** decrease is consistent with the independently measured wire reduction below. The `scopedFights` site is **59.3% lower**, consistent with the newly included identity memoization; this deduplication change does not edit `rail.ts`. Home's whole-file total includes the new merge helper (32,880 sampled bytes) alongside memoized projections. Native/runtime rows are not attributed to product callers or added again. Neither combination-level rate nor private-byte changes can be separated causally between memoization and deduplication without isolated matched runs. Sampling variance and changes elsewhere in the allocation table prevent treating the site reductions as an equal reduction in total allocation. No `navigation/history.ts` site appeared in this sample.

Late-busy DOM nodes remained at 4,844 with 35 listeners and 2 documents; natural idle +30–90 held 1,146 nodes, 25 listeners and 2 documents. This matches the prior plateau shape, not proof against longer-session leaks. Pressure/GC observations in this diagnostic are not natural-idle acceptance evidence.

## Gate results

Decimal MB; renderer cores are CPU-seconds per wall-second over the busy gate. Startup/seed peaks are outside these phase gates.

| Run | Steady busy mean | Busy gate peak | Departure peak | Idle gate peak | Renderer busy cores |
| --- | ---: | ---: | ---: | ---: | ---: |
| Route-read 1 | 225.006 | 260.051 | 246.493 | 172.306 | 0.06878 |
| Route-read 2 | 227.788 | 271.155 | 247.325 | 175.854 | 0.03646 |
| Combined 1 | 222.742 | 254.165 | 238.727 | 172.941 | 0.06958 |
| Combined 2 | 228.461 | 262.967 | 252.269 | 175.014 | 0.06827 |

| Run / phase | Exact peak bytes | Revised limit | Investigation target |
| --- | ---: | --- | --- |
| 1 busy | 254,164,992 | Pass, <300,000,000 | Fail, <140,000,000 |
| 1 idle | 172,941,312 | Pass, <200,000,000 | Fail, <130,000,000 |
| 2 busy | 262,967,296 | Pass, <300,000,000 | Fail, <140,000,000 |
| 2 idle | 175,013,888 | Pass, <200,000,000 | Fail, <130,000,000 |

Concurrent process contributions at each idle peak:

| Process | Run 1 bytes | Run 2 bytes |
| --- | ---: | ---: |
| Renderer | 66,899,968 | 71,372,800 |
| Browser | 49,147,904 | 49,844,224 |
| Maxed | 46,223,360 | 43,012,096 |
| Storage utility | 7,655,424 | 7,757,824 |
| Crashpad | 3,014,656 | 3,026,944 |
| **Combined** | **172,941,312** | **175,013,888** |

Both busy peaks are lower than the corresponding route-read runs by about 5.9 / 8.2 MB, but steady-busy means and departure peaks do not consistently improve. Idle changes are only about **+0.635 / −0.840 MB** against corresponding route-read runs, and both idle peaks lie within approximately the same range. CPU does not show a consistent improvement. These measurements do **not** resolve an idle-memory saving or isolate the memory impact of either new optimization. Concurrent process sums and phase coverage were checked against the raw collector rows; independent process maxima were not summed.

## Consumers and ownership

This audit was performed at `6eb6560f02fff690724d59972e3cd5681af5835b`; line references in this table identify that pre-change source. The memoization and merge additions shift current line numbers.

| Consumer | Event-only chart possible? | Evidence |
| --- | --- | --- |
| `app/ui/src/lib/home/v2/live.ts:174`, `:200`, `:352` | Yes, retained data owner | `event()` feeds all three fight variants into `putFight`; `boundedFight` retains buckets and retains series for open/selected fights. Events can precede the first snapshot of a fight. |
| Same file `:87`, `:100`, `:229`, `:240`, `:311` | Yes, within existing bounds | Departure saves realm visits and their fight maps; returning to the same instance restores them. The window is capped at 64 fights, with selected/focused protection. A snapshot removes absent **open** rows, but retains ended rows. Ended unselected rows deliberately lose series, not buckets. Eight visits are retained. This is not unlimited retention of every departed/hidden fight. |
| `app/ui/src/lib/shell/rail.ts:87`, `:94`, `:98` | Yes, forwards event-only rows | Missing snapshot rows use the entire window row. When a snapshot row exists, a fresher event contributes only focus/lifecycle fields; snapshot buckets/series win. No chart merge is needed here if the Home window holds a coherent row. |
| `app/ui/src/lib/home/dungeon.ts:40`, `:45`; `app/ui/src/lib/home/DungeonHome.svelte:45`, `:60`, `:140` | **Yes, rendered chart** | Dungeon observations use `scopedFights`; selected/current boss can be a retained ended fight absent from the snapshot, or an event row before its first snapshot. `FightChart` renders its buckets. Leaving the dungeon resets its active scope; no claim that Dungeon Home remains visible after departure. |
| `app/ui/src/lib/home/v3/FightChart.svelte:6`, `:8`; `app/ui/src/lib/home/v2/DamageChart.svelte:13`, `:61` | Yes, via Dungeon Home | Shared chart renderer/helper accepts buckets and optional per-player points. The production v3 wrapper supplies an empty player-series list and no selected player. The v2 component has no direct production instantiation found; its helper is used by v3. |
| `app/ui/src/lib/home/realm.ts:79`; `app/ui/src/lib/home/RealmHome.svelte` | Retained data, **no chart rendered currently** | Realm view exposes the focused/retained fight for HP, damage rows and lifecycle. Realm Home does not instantiate a damage chart or read buckets/series. Same-instance visits still retain the chart arrays in their fight rows. |
| `app/ui/src/routes/RunStory.svelte:62`, `:67`, `:224` | No | Buckets come from saved `RunStory.boss_fights`; `runs/history.ts:91` queries `getRun`. Event invalidation triggers reads rather than supplying charts. |
| `app/ui/src/routes/Fight.svelte:45`, `:87`, `:160`; `app/ui/src/lib/runs/FightSeriesChart.svelte:14`, `:67`; `app/ui/src/lib/runs/fight-screen.ts:12`, `:33`, `:41` | No | Fight chart, KPI rates and player series come from `getFight` history results, not event rows. |
| `app/ui/src/routes/PlayerBreakdown.svelte:48`, `:108`, `:202`; `app/ui/src/lib/runs/PlayerSeriesChart.svelte:10`, `:47` | No | Saved fight context and player detail are queried together. Bucket/series alignment is checked by `fight-screen.ts:42`. |

The exhaustive fight-event search in `app/ui/src` found no other event-row stores. `runs/invalidation.ts:16` reads IDs/run IDs only; `alerts/invalidation.ts:17` and `bridge/live.ts:45` inspect event kinds. Shell event observers do not consume fight charts. Runs list updates, alerts and history invalidation therefore need no merge changes.

`app/ui/tests/home-v2/departure-batch.test.ts:90` compares the production window against the frozen pre-change `departure-oracle.ts` including fight arrays. The oracle remains unchanged. Existing full-event tests and separate omitted-chart cases pass.

Native ownership is safe to scope to presentation: `app/src-tauri/src/runtime/join.rs:258` through `:292` retains a full cloned envelope and alert context in `pending` before handing the presentation copy to the bridge. Writer submission is at `:376`. `app/src-tauri/src/bridge.rs:41` invokes `presentation_event` on the queued UI copy only. Storage, exports/history and native alerts continue to use the original envelope. Existing departure storage tests compare full receipt digests (`runtime/departure_tests.rs:144`).

## Pre-change JSON accounting

The phase-one throwaway example included the production payload module and drove the perf engine directly. This command passed twice (the second run included workload/open-update assertions). The example was deleted in Step A; the permanent payload test now guards chart omission. These are preserved pre-change measurements, not a second app benchmark:

```powershell
cargo run --locked -p maxed-engine --features perf-live --example boss_chart_audit --release
```

The script seeds 1,536 fights, 24 players and 320 bags at timestamp 1,000,000, then applies `perf::hits(step)` for 2,440 steps at 250,000 microsecond spacing. Every emitted event is enveloped and shaped with `presentation_event`; each packet transaction is batched with `event_batches`. A watermark and `SnapshotEncoder` update follow every step. No sleeps, app, storage writer or WebView are involved. Seed events are excluded. An initial full snapshot is 114,295 bytes. Step zero has no new watermark and therefore one additional full update; the two reported windows below contain **only deltas**.

Exact JSON lengths use `serde_json::to_vec`, without the channel's minimum 1,024-byte whitespace padding or IPC framing. Event share is the sum of serialized envelopes, excluding batch wrappers. Chart share is the exact length difference after replacing priority update `buckets` with `[]` and `player_series` with null; field names remain. All counted priority updates are asserted open. This is payload accounting, not an allocation or retained-memory estimate.

| Window | Seconds | Events, envelope bytes/s | Fight-event share bytes/s | Priority update chart share bytes/s | Event batches bytes/s | Snapshot deltas bytes/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Steady busy [30, 580) | 550 | 170,730.007 | 170,723.464 | **72,313.778** | 171,108.000 | 105,854.284 |
| Allocation-profile interval [330, 390) | 60 | 177,534.600 | 177,528.267 | **80,765.633** | 177,914.567 | 114,244.667 |

Steady exact totals: envelopes 93,901,504; fight envelopes 93,897,905; redundant event chart content 39,772,578; 4,433 priority updates; event batches 94,109,400; deltas 58,219,856. Profile-interval totals respectively: 10,652,076; 10,651,696; 4,845,938; 486; 10,674,874; 6,854,680.

Full 610-second replay totals: envelopes 104,594,739; fight envelopes 104,590,950; priority chart content 42,734,488 across 4,921 updates; event batches 104,826,628; snapshot updates 63,368,790 (includes that one full update). Priority fights' charts travel through both paths, although projections need not be byte-identical: `NewTick` can emit a fight update before that step's subsequent hits, while the snapshot follows them.

## Design tradeoffs and remaining limits

The omission marker costs 27 JSON bytes per stripped event. Applying it to the pre-change workload accounting predicts a net steady reduction of **72,096.158 bytes/s**, approximately **26.0% of combined event-batch and delta JSON** (4.326 MB per steady minute); the profile interval predicts **80,546.933 bytes/s**. The channel's 1,024-byte minimum padding can slightly reduce actual wire savings. These are JSON accounting estimates, not retained-memory savings.

The approved design uses an explicit marker because an empty bucket array or null series can be real data. It leaves snapshot/delta reconstruction independent and complete; stripping snapshots based on events would require cross-channel partial-row reconstruction and complicate query/resync.

The accepted **16-fight snapshot-series cap** remains a tradeoff: buckets are available for every visible priority fight, but per-player series beyond the cap may remain unavailable between complete events instead of briefly refreshing from every update. No Home chart currently renders those per-player series. The marker-setting code documents this limit. A null series from a snapshot is authoritative; old series are never spliced onto new bucket boundaries.

Chart evidence may lag newer metadata until the next accepted snapshot. Existing ordering rules still reject stale snapshots, and absent open rows are still removed. Ended retained rows and same-instance visits keep their existing bounded evidence; selected ended series remain subject to the existing selection and authoritative-null policies. No broader retention, focus/kill rule, history, export or alert behavior changed.

This is **synthetic, visible-window, non-packaged evidence only**. It does not verify live Npcap/gameplay, installed-package behavior, startup acceptance or long-session stability. No changes were made to release limits, known limitations, roadmap or earlier reports.
