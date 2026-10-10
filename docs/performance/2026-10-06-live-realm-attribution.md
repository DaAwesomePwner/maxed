# Live realm memory attribution — 2026-10-06

Phase 1 investigation for issue #40. **The uninstrumented synthetic workload still fails both limits: 511,078,400 busy / 221,417,472 idle combined private bytes.** The fresh-profile idle floor is 134,463,488 bytes. No product-code fix, live-game verification, installed-build acceptance, push or pull request is included.

The accompanying [JSON evidence](2026-10-06-live-realm-attribution.json) records exact measurements and sanitized aggregates. Raw process records, traces, allocation profiles, source maps and temporary application profiles remain outside the repository.

## Build and environment

- Source: `734104bff317eaba8ab22256a7fff4ac463cc698`, branch `perf/40-attribution`; includes the compact whole-visit history integration. Product sources were unchanged. Collector/tooling changes were present during this investigation.
- Executable SHA-256: `f3302ca5805a8911a50ef3ee45a5669ec8f37ea5dcc26eb387abb558532d2dc1`.
- Release features: `maxed-app/custom-protocol,maxed-app/perf-live`; built frontend; non-packaged executable. Synthetic bundled resource hashes matched the repository fixtures.
- Windows 11 Pro `10.0.26200`, AMD Ryzen 9 7900X, 12 cores / 24 logical processors, 67,794,673,664 bytes usable physical RAM. WebView2 `154.0.4258.53`; Rust `1.99.0 (b940084d7 2026-09-28)`, Node `26.7.0`, npm `11.19.0`.
- Hardware acceleration off, verified from the owned browser's arguments. Existing policy folds GPU and network service into the browser. The separate utility observed was `storage.mojom.StorageService`.

This is a different host from the October 5 report's Core Ultra / 16.6 GB machine. These measurements re-baseline current HEAD; they do **not** isolate the effect of PR #78 from host differences or run-to-run variation.

Build sequence, with a newly created `$profile` under the Windows temporary directory:

```powershell
npm.cmd --prefix app/ui ci
$env:TAURI_CONFIG = @{
    identifier = $profile
    app = @{ windows = @(@{
        label='main'; create=$false; title='Maxed'
        width=1100; height=760; minWidth=800; minHeight=560
        dataDirectory=(Join-Path $profile 'webview')
    }) }
} | ConvertTo-Json -Depth 6 -Compress
npm.cmd --prefix app/ui run build
cargo build --locked --release -p maxed-app -p maxed-cli --features maxed-app/custom-protocol,maxed-app/perf-live
```

The compiled absolute identifier isolates native data and the WebView profile. The driver attests the literal profile and `perf-live` marker in the exact executable. For offline symbolization, a second UI-only `vite build --sourcemap hidden` produced a byte-identical JavaScript bundle; maps were moved to temporary evidence, and the native executable was not rebuilt.

## Uninstrumented measurements

Runs were strictly sequential, with a fresh profile at each launch. The baseline used the maintained `-Mode after` workload: 1,536 fights, 24 synthetic players, 320 bags, 24 hits/s, 610 seconds scheduled busy, then Nexus and 180 seconds idle. Busy actually lasted 612.492 seconds. The floor used `-FreshIdle`: no live feed, 30 seconds settling, then approximately 180 seconds idle.

Both runs had complete PID-plus-birth process accounting, verified rendering, no failed subscription and no detected compiler/test/active-Node load. Private bytes are summed concurrently before selecting a peak. The baseline idle comparison is +30 to +175 seconds after Nexus, as in the earlier report.

Starting with an empty profile still triggers normal installed-client asset discovery. Each run built its own local asset cache inside its temporary profile; the engine event stream and identities remained synthetic. This is not an asset-free floor. No existing Maxed profile, real capture or account data was used, and no sprites are committed.

| Measurement | Combined peak, bytes | Renderer independent peak | Maxed independent peak | Result |
| --- | ---: | ---: | ---: | --- |
| Baseline busy | 511,078,400 | 372,256,768 | 80,011,264 | **FAIL**, limit <200,000,000 |
| Baseline idle | 221,417,472 | 102,592,512 | 51,519,488 | **FAIL**, limit <150,000,000 |
| Fresh-profile idle floor | 134,463,488 | 37,425,152 | 34,123,776 | Floor only; no acceptance claim |

Ranked process contributions **at each combined peak**, decimal MB:

| Process | Baseline busy | Baseline idle | Fresh floor |
| --- | ---: | ---: | ---: |
| Renderer | 338.473 | 102.593 | 35.078 |
| Browser | 102.576 | 56.467 | 54.419 |
| Maxed | 58.929 | 51.519 | 33.997 |
| Utility: `storage.mojom.StorageService` | 7.983 | 7.815 | 7.901 |
| Crashpad handler | 3.117 | 3.023 | 3.068 |
| **Combined** | **511.078** | **221.417** | **134.463** |

All observed WebView2 processes appear in this table. There was no separate GPU or network process. The browser's busy peak contribution occurred at the final busy sample, near the map-change boundary; the dump at busy +600 seconds is a different instant. Independent component peaks must not be added to reconstruct the combined peak.

The post-realm idle peak exceeds the fresh floor by **86,953,984 bytes**: renderer +67,514,368, Maxed +17,522,688, browser +2,048,000, and a small net decrease in the two helper processes. These are differences between runs, not independently proven reclaimable allocations. The collector's overall baseline peak of 604,020,736 bytes includes startup/seed outside the busy window and is not a startup gate measurement.

## Diagnostic attribution

The separate visible-window CDP run also completed without detected competing load or accounting gaps. Its combined phase peaks were **534,347,776 busy / 246,505,472 idle bytes**; these are **diagnostic only**, not gate evidence. Tracing introduced a sixth process, `tracing.mojom.TracingService`: 12.7 MB at those peaks, growing to approximately 21.1 MB during later trace collection. The JSON retains every process at both diagnostic peaks.

Browser-level CDP connected through `/json/version` and recorded only `disabled-by-default-memory-infra`, with a 32 MiB trace buffer and explicit detailed, non-deterministic dumps. All seven dumps succeeded: busy +300/+600, idle +30/+120/+150/+190/+220 seconds. The last four bracket pressure and GC. Chromium's JSON exporter renumbered dump IDs; the analyzer joins ordered dump timestamps to request timestamps and verifies their spacing, rather than incorrectly joining the returned GUID strings. Nearby independent process samples were within 547 ms.

### Private pools and the unattributed remainder

The following is a **selected-pool reconciliation estimate**, in decimal MB. It uses the dump's own private-byte total and four distinct underlying pools: malloc (including its PartitionAlloc backend and Windows heap), Blink GC, Blink's separate PartitionAlloc partitions, and V8 heap spaces. It does not add their suballocations again. Provider `size` and Windows private commit are different measures; the remainder is explicitly an **unattributed/basis gap**, not an identified allocation. Shared memory, logical cache sizes, disk-backed data and timing differences prevent a complete private-byte ledger.

Renderer pools ranked by busy +300 contribution:

| Pool / remainder | Busy +300 | Busy +600 | Idle +30 | Idle +150, after pressure | Idle +220, after GC |
| --- | ---: | ---: | ---: | ---: | ---: |
| V8 heap spaces | 104.731 | 102.175 | 18.747 | 19.272 | 7.078 |
| malloc, including its PA backend | 59.803 | 49.591 | 45.609 | 42.109 | 34.826 |
| Blink GC, including its objects | 39.125 | 39.059 | 14.549 | 15.336 | 14.025 |
| **Unattributed/basis gap** | **14.945** | **14.982** | **19.984** | **20.027** | **18.488** |
| Blink PartitionAlloc | 7.397 | 4.674 | 4.608 | 4.801 | 3.105 |
| **Dump private bytes** | **226.001** | **210.481** | **103.498** | **101.544** | **77.521** |

The process peaks and these point-in-time dumps are different observations. In particular, neither busy dump captures the larger map-transition peak. At idle +30, approximately 83.5 MB of the renderer's 103.5 MB is represented by the selected pools; about 20.0 MB remains unreconciled.

Other processes, at busy +300 / idle +30, decimal MB:

| Process | Private bytes | Selected malloc pool | Unattributed/basis gap |
| --- | ---: | ---: | ---: |
| Maxed | 71.201 / 54.444 | Not instrumented | 71.201 / 54.444 |
| Browser | 56.660 / 58.335 | 33.199 / 34.520 | 23.461 / 23.815 |
| Tracing utility, diagnostic only | 12.567 / 12.607 | 1.331 / 1.386 | 11.235 / 11.221 |
| Storage utility | 8.188 / 8.237 | 1.334 / 1.406 | 6.854 / 6.831 |
| Crashpad handler | 3.174 / 3.109 | No provider | 3.174 / 3.109 |

Maxed and crashpad use the nearest independent process counters because memory-infra did not report allocators/private totals for them. Maxed-side ownership below the process level remains unmeasured. The browser's roughly 23.5–23.8 MB gap is still not attributed to individual native subsystems; it must not be relabeled as GPU or utility overhead without evidence.

### Heap capacity, allocator slack and caches

One main V8 isolate was reported; no worker isolate appeared. In this run its summed `size` and heap-space `virtual_size` were equal. The first is V8's physical-space statistic; the second is its space-size statistic, not an arbitrary reserved address-space range. Objects include collectible garbage until collection. These field meanings follow the [Chromium V8 dump provider](https://chromium.googlesource.com/chromium/src/+/main/gin/v8_isolate_memory_dump_provider.cc).

| V8 main isolate | Heap space, bytes | Allocated objects, bytes | Space minus objects |
| --- | ---: | ---: | ---: |
| Busy +300 | 104,730,624 | 41,377,424 | 63,353,200 |
| Busy +600 | 102,174,720 | 20,606,436 | 81,568,284 |
| Idle +30 | 18,747,392 | 16,731,332 | 2,016,060 |
| Idle +150, after pressure | 19,271,680 | 17,728,964 | 1,542,716 |
| Idle +190, before GC | 19,795,968 | 13,720,256 | 6,075,712 |
| Idle +220, after GC | 7,077,888 | 5,170,888 | 1,907,000 |

At busy +600, Blink GC committed 39,059,456 bytes for 3,540,640 allocated object bytes, including 22,675,456 pooled bytes. At idle +30 it committed 14,549,072 bytes for 2,505,576 object bytes, with no pooled bytes. This is allocator capacity/slack, not evidence that all of it can be released safely.

Blink PartitionAlloc had `buffer` and `array_buffer` partitions. Their reported size was respectively 5,890,048 / 1,507,328 bytes at busy +300 and 4,603,904 / 4,096 at idle +30. The separate malloc PA `allocator` partition committed 56,020,992 bytes at busy +300 and 43,024,384 at idle +30; at the latter point it held 17,727,328 active bytes and reported 25,297,056 wasted bytes. Its high-water committed value reached 289,509,376 bytes by idle, but this counter includes startup/transition activity and is not a concurrent phase peak. Named `fast_malloc` and `layout` partitions were **not reported**; they are not assumed to be zero. Full partition attributes are in the JSON.

Cache/provider observations, reported separately from the private-pool sum:

| Provider | Busy +300 | Busy +600 | Idle +30 | Idle +150 |
| --- | ---: | ---: | ---: | ---: |
| Renderer decoded image cache, bytes | 268,435,456 | 134,217,728 | 134,217,728 | Node absent after pressure |
| Renderer cc effective size, bytes | 272,379,904 | 138,162,176 | 136,341,504 | 2,123,776 |
| Renderer Skia effective size, bytes | 574,856 | 579,634 | 518,210 | 514,114 |
| Renderer web cache, logical bytes | 71,557,254 | 71,557,254 | 71,557,254 | 71,557,254 |
| Encoded image data parked on disk, bytes | 70,339,154 | 70,339,154 | 70,339,154 | 70,339,154 |

The decoded cache has ownership links through discardable/shared-memory nodes. The `discardable` and `shared_memory` raw sizes therefore repeat memory attributed to cc; adding them would double-count it. Moreover, removing the 134,217,728-byte image-cache node after critical pressure barely changed the dump's renderer private total: 101,556,224 → 101,543,936 bytes. Thus **134 MB of decoded-cache accounting is not 134 MB of recoverable private bytes**. The 70.3 MB encoded image backing was already parked on disk, not a 70.3 MB private-RAM charge. Chromium's [memory-infra accounting documentation](https://chromium.googlesource.com/chromium/src/+/main/docs/memory-infra/README.md) distinguishes requested `size` from ownership-adjusted `effective_size`; neither turns logical disk/shared caches into Windows private commit.

The browser reported approximately 1,178,012 SQLite bytes and 32,832 LevelDB bytes. Storage reported 8,208 LevelDB bytes. These are suballocations, not additions to malloc. Renderer font data was represented by Skia's glyph cache (232,022 bytes at idle +30); a standalone `font_caches` provider was absent. No renderer SQLite or LevelDB provider was reported. The JSON distinguishes absent providers from reported zero values.

### Allocation sites and DOM trends

Sampling covered busy +330 to +390 seconds (60.376 seconds), at a 32,768-byte Poisson interval, including objects collected by both minor and major GC. It estimated **2,895,918,112 allocated bytes**, approximately **47.96 MB/s**, across 87,564 samples. These are sampled allocation-volume estimates, not retained bytes or guaranteed savings. Source maps came from the verified identical bundle; native frames remain unmapped.

| Allocation site / source | Estimated self bytes |
| --- | ---: |
| `rankedDrops` map callback, `home/v3/loot-rank.ts:13` | 672,263,324 |
| Native/runtime `set`, callers not split in this table | 476,184,576 |
| `bestDrop` flat-map callback, `home/v3/loot-rank.ts:22` | 365,476,660 |
| `lootCounts` map callback, `home/v3/screen-data.ts:46` | 354,173,152 |
| Native/runtime `Map` | 204,714,612 |
| Svelte proxy getter, `internal/client/proxy.js:170` | 85,939,976 |
| `homeVisualTypes`, `home/visuals.ts:22` | 50,484,592 |
| Realm loot-count derivation, `RealmHome.svelte:54` | 44,619,944 |
| Realm ranked-bag derivation, `RealmHome.svelte:53` | 34,852,328 |
| Bag-list derivation, `home/v3/BagList.svelte:20` | 34,649,888 |

The three directly mapped ranking/counting callbacks account for **1,391,913,136 bytes (48.1%)** without assigning the native Map/set allocations to them. Source inspection shows repeated deduplication, sorting and item expansion in RealmHome and BagList derivations. Phase 2 should test invalidating those computations only when loot/visual inputs change, while preserving complete visit history and all existing partial-data behavior. This is a hypothesis grounded in allocation sites, not a product change made here.

| Interval | Nodes | JS listeners | Documents | Layouts/s | Style recalcs/s | Script seconds/s | Task seconds/s |
| --- | --- | --- | --- | ---: | ---: | ---: | ---: |
| Busy +30–300 | 4,831 → 4,887 | 69 → 73 | 2 | 4.36 | 2.51 | 0.0710 | 0.1041 |
| Busy +400–610 | 4,893 throughout | 73 throughout | 2 | 4.57 | 2.56 | 0.0719 | 0.1082 |
| Idle +30–90 | 1,145 throughout | 27 throughout | 2 | 1.00 | 0 | 0.000620 | 0.002036 |
| Idle +200–250, after GC | 1,158 throughout | 27 throughout | 2 | 1.01 | 0 | 0.000160 | 0.001658 |

There is **no continuing late-busy DOM/listener growth in this run**. Early small increases plateaued. Busy JS heap used fluctuated roughly 16.5–46.6 MB; late-busy total heap remained roughly 99.7–107.7 MB. Natural idle used about 17 MB, and post-GC used about 5.2 MB. Ten-second samples can miss short spikes and do not rule out other leaks or longer-session growth. Idle layout still runs at about once per second.

### Pressure and forced-GC probes

Critical pressure was sent at idle +120 seconds after a +90–120 baseline. GC was sent at +190, after the synthetic driver's +180 completion; its native teardown is therefore not credited to GC. Mean private bytes in decimal MB:

| Process | Pressure before, 30 s | Pressure after, +30–60 s | GC before, 10 s | GC after, +30–60 s |
| --- | ---: | ---: | ---: | ---: |
| Renderer | 99.290 | 100.127 | 98.615 | 75.477 |
| Browser | 58.364 | 59.608 | 58.158 | 58.525 |
| Maxed | 49.261 | 45.867 | 41.587 | 41.693 |
| Tracing utility | 12.517 | 16.937 | 16.945 | 21.131 |
| Storage utility | 8.135 | 8.147 | 8.147 | 8.159 |
| Crashpad | 3.019 | 3.019 | 3.019 | 3.019 |
| **Combined** | **230.587** | **233.705** | **226.471** | **208.005** |

Pressure showed no sustained private-byte reduction: renderer means increased 0.84 MB, while medians decreased only 1.03 MB and same-dump private bytes changed by just 12,288 bytes. Natural native cleanup and trace growth also affect the combined series. Do not attribute Maxed's pre-probe downward drift to browser pressure. GC reduced renderer means by **23,137,357 bytes**; V8 heap space shrank 12,718,080 bytes and its allocated objects shrank 8,549,368 bytes between the bracketing dumps. The remaining drop includes native allocator changes and differing sample times.

The real `ICoreWebView2_19` memory-target API was **not tested**: it requires a native COM hook in a throwaway build, beyond the CDP-only tooling used here. Pressure is not an equivalent implementation. Microsoft's [API documentation](https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2_19) describes a best-effort asynchronous operation that may swap memory to disk while scripts continue; working-set reduction is not necessarily private-commit reduction. No private-byte saving is established for `Low` by this run.

## Ranked Phase 2 candidates

These are experiment priorities and opportunity bounds, **not additive or promised savings**.

| Rank | Candidate | Measured opportunity / estimated saving to test | Evidence and next experiment |
| --- | --- | --- | --- |
| 1 | Cut per-update UI allocations | Target the 1.39 GB/min directly mapped loot churn first. Busy V8 unused space was 63–82 MB; GC demonstrated about 23 MB renderer recovery at idle. A 60–100 MB busy pool reduction is a test target, not a prediction. | Preserve stable loot inputs; share ranking/count results; avoid repeated Map/array construction and proxy traversal on numeric-only updates. Re-run the same uninstrumented workload and inspect the transition peak. |
| 2 | Investigate Maxed-side growth and release idle state | Up to about 17.5 MB idle / 46 MB independent busy excess over fresh Maxed. Ownership and safe savings remain unknown. | Attribute engine, writer/SQLite and allocator retention separately; keep capture and history correctness. About 4.4 MB more native memory fell after driver completion in the diagnostic, which is not representative of a still-running live session. |
| 3 | Bound sprite decoding by bytes / serve cropped tiles | 134–268 MB decoded cache accounting and a 70.3 MB encoded backing file are real resource costs. **Private-byte saving is unquantified; pressure showed approximately zero for clearing the remaining decoded entry.** | The existing three-atlas cap does not bound decoded bytes. Compare cropped tiles with the same catalog and test decode/transition scratch peaks, resident/shared memory and private bytes separately. |
| 4 | Pause/coalesce renderer pushes while hidden | Potentially avoid much of the measured 48 MB/s allocation flow while hidden; retained/private-byte saving is **not measured**. | The measured page was visible throughout. Test hide/show, a single fresh resync, continuity, alerts and persistence; do not pause passive ingestion or the writer. This cannot solve the visible busy gate alone. |
| 5 | `MemoryUsageTargetLevel.Low` on idle/hidden | **0 MB demonstrated by the pressure proxy**; GC's separate 23 MB reduction is an upper opportunity for collection, not an estimate of `Low`. | Test the actual API behind a diagnostic gate, restoring Normal on activation. Measure private commit as well as working set and responsiveness. |
| 6 | Fold remaining WebView utility into browser | Less than the storage process's approximately 8 MB footprint; much of its allocations would move rather than disappear. | GPU/network are already folded. Storage folding requires a supported effective mechanism and stability/isolation review. The diagnostic tracing utility is not normal product overhead. Do not remove crash reporting to manufacture a win. |

**Reachability:** the measured 134.5 MB fresh floor leaves only 15.5 MB idle / 65.5 MB busy headroom. The gates are not ruled out by that floor, but are **not shown reachable with a single reclamation switch**. Current live idle needs more than 71.4 MB removed and busy more than 311.1 MB. Even returning only the renderer to its fresh peak contribution would leave roughly 154 MB at the baseline idle peak. Forced GC alone is far short; the image-cache shared-memory release cannot be deducted from the private-byte gate. Meeting both limits likely requires combined UI allocation, transition/native-pool and native-retention work, followed by new uninstrumented measurements.

## Tooling and validation

Collector output now includes bounded `process_type` and `utility_sub_type` labels without exporting raw command lines. The scenario driver adds fresh-idle and explicit attribution modes, continuous competing-load observations, temporary-path checks, and fail-closed launch-lock handling. It closes only its launched app and PID-plus-birth-attested descendants. The report includes concurrent process decompositions and excludes confounded evidence. The new CDP and offline-analysis tools retain all raw evidence in the caller's temporary directory.

To reproduce, pre-create empty `baseline`, `floor` and `diagnostic` output directories under a temporary `$evidence` directory. Run these commands separately and sequentially. Between runs, after the driver has closed its owned processes, archive the used temporary profile and recreate an empty directory at the exact compiled `$profile` path. Do not build or test during a run.

```powershell
$exe = (Resolve-Path target/release/maxed.exe).Path
pwsh -NoProfile -File tests/performance/live-realm-memory.ps1 -Exe $exe -Profile $profile -OutputDir "$evidence/baseline" -Mode after
# Archive and recreate the temporary profile before each subsequent launch.
pwsh -NoProfile -File tests/performance/live-realm-memory.ps1 -Exe $exe -Profile $profile -OutputDir "$evidence/floor" -Mode after -FreshIdle
pwsh -NoProfile -File tests/performance/live-realm-memory.ps1 -Exe $exe -Profile $profile -OutputDir "$evidence/diagnostic" -Mode after -Diagnostic -Attribution
```

Run `live-realm-report.ps1 -EvidenceDir <directory>` for each completed run. Place maps from the byte-identical bundle under `$evidence/source-maps`, then run `node tests/performance/live-realm-attribution-report.mjs $evidence` for the offline allocator, allocation-site, metrics and probe analysis. Its output is a review candidate; inspect and sanitize aggregates before committing any report.

Validation: locked release build; `npm ci`; matching bundled synthetic resources and source-map JavaScript hash; collector and interactive-startup self-tests (the latter 83 assertions); memory-tooling tests under PowerShell 7 and Windows PowerShell 5.1; Node diagnostic/analyzer self-tests; and `git diff --check`. No Rust or product UI source changed, so Rust lint/test and UI test suites were not rerun. No builds or tests overlapped any workload. This report does not establish real-game, Npcap, installer, startup or installed-release behavior.
