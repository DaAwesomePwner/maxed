# Live realm memory evidence — 2026-10-05

The live-realm memory limitation remains unresolved. **Both busy and idle release memory gates fail.** Accepting the mitigation did not change the limits or establish release acceptance. The byte-channel candidate was measured at `4cae79f`, based on `4eacecf`; its subsequent integration at `dc11454` on `8503329` was tested but not remeasured. Later correctness fixes through `4632f05` also have no new memory measurement.

The [JSON evidence](2026-10-05-live-realm-memory.json) retains exact artifact hashes, counts, durations, diagnostic findings and historical validation results. Those are dated records, not checks run against today's HEAD. Raw heaps and process records remain private.

## Workload and method

A feature-gated synthetic native driver exercised the engine, store writer, bridge and built renderer: 1,536 open targets, 24 invented players, 320 bags, 24 hits/second, 610 seconds busy, map change to Nexus and 180 seconds idle. It used a temporary profile and no real captures or player data. Acceleration was off. The before mode approximated the original full-snapshot publisher in the same application, not an untouched baseline executable.

Environment: Windows 11 Pro build 26200; Core Ultra 7 265U, 12 cores / 14 logical processors; 16,597,598,208 bytes RAM; Rust 1.99.0, Node 26.7.0, npm 11.19.0 and WebView2 154.0.4258.53. The historical Node version is measurement provenance, not the contributor toolchain pin.

The collector followed PID-plus-birth descendants at one-second intervals, attested executable/profile identity, and recorded UTC phase markers. Idle comparison excluded the first 30 seconds after leaving. UI Automation confirmed Realm/Nexus rendering and subscription health. Combined memory is concurrent Maxed plus all observed WebView2 private bytes, never summed independent peaks. Renderer CPU is CPU-seconds divided by sampled wall seconds (cores).

Short before runs with CDP are diagnostics, not matched ten-minute acceptance. The synthetic writer does not populate the Nexus saved-history query context, so populated history requires a separate workload. Runs with overflowing seed/event buffers, disconnected renderers or storage pressure that prevented the intended schedule were excluded from acceptance.

## Findings

Filtering, deltas and chart compaction reduced live wire size. High-frequency payloads use UTF-8 JSON byte channels instead of JavaScript source embedding. A 2,048-fight synthetic regression serialized 2,578,084 full-domain bytes versus 7,807 presentation bytes. This is payload evidence, not a memory measurement.

In a before-mode idle diagnostic, JS heap use was 703,446,532 bytes before a heap snapshot forced collection and 4,936,384 afterward. Retainers included 64 fight rows, one snapshot and three player-series arrays; surviving wire-script strings totaled 14,812 bytes. There was no reachable 730 MB fight graph. This supports collectible payload churn, but does not attribute all native, decoded-image or allocator private memory. It does not reproduce or certify the original real-game session.

### Completed memory and CPU results

All private-byte figures below are exact decimal bytes. Combined is the peak
concurrent Maxed plus all WebView2 processes; separate renderer and Maxed peaks
need not occur at that instant. Renderer CPU is mean cores over the phase.
Busy requires strictly less than 200,000,000 combined bytes; idle requires
strictly less than 150,000,000. Before diagnostics have observed samples but
are `not_measured` for acceptance because of CDP and the short busy duration.

| Run / phase | Renderer private | Maxed private | Combined private | Renderer cores | Gate |
| --- | ---: | ---: | ---: | ---: | --- |
| Before short / busy | 1,893,556,224 | 69,529,600 | 2,010,578,944 | 0.897043 | not_measured |
| Before short / idle | 502,571,008 | 42,799,104 | 608,620,544 | 0.010584 | not_measured |
| Before delayed heap / busy | 1,919,385,600 | 73,367,552 | 2,052,493,312 | 1.082560 | not_measured |
| Before delayed heap / idle | 1,617,801,216 | 45,441,024 | 1,725,169,664 | 0.070023 | not_measured |
| JSON-text after / busy | 691,609,600 | 90,730,496 | 815,357,952 | 0.171361 | **FAIL** |
| JSON-text after / idle | 412,200,960 | 48,992,256 | 521,605,120 | 0.009123 | **FAIL** |
| Byte-channel after / busy | 327,507,968 | 77,754,368 | 487,919,616 | 0.186438 | **FAIL** |
| Byte-channel after / idle | 105,639,936 | 47,869,952 | 215,904,256 | 0.006712 | **FAIL** |
| Same binary, boundary UIA / busy | 467,701,760 | 80,207,872 | 594,735,104 | 0.122651 | **FAIL** |
| Same binary, boundary UIA / idle | 105,369,600 | 50,434,048 | 219,209,728 | 0.004575 | **FAIL** |

The byte-channel candidate is retained as the selected measured
candidate. Its repeat completed with UI Automation observations reduced to one
successful observation per phase. This **did not lower memory**: combined busy
rose by 106,815,488 bytes and idle by 3,305,472 bytes. There is no evidence here
for discounting memory as observer inflation. The conservative representative
envelope across its two runs is **594,735,104 busy / 219,209,728 idle**, while
487,919,616 / 215,904,256 is its best individual run. Both are failures.

| Run | Busy actual / sampled seconds | Busy / idle samples | Idle sampled seconds | Machine load |
| --- | ---: | ---: | ---: | --- |
| Before short | 62.202 / 61.104 | 47 / 110 | 143.193 | Quiet at launch; diagnostic retry |
| Before delayed heap | 65.388 / 64.806 | 48 / 89 | 143.033 | Not quiet; two check processes |
| JSON-text after | 671.657 / 669.184 | 442 / 53 | 114.748 | Waited about 8 min; heavy-process counts 6, 2, 3, 2, 0; compilation began during busy |
| Byte-channel after | 612.340 / 610.793 | 480 / 114 | 144.329 | Waited about 4 min; counts 2, 3, 0; quiet at launch |
| Boundary UIA repeat | 612.753 / 611.366 | 476 / 111 | 143.458 | Count 0 at preflight; compilation began during busy |

The JSON-text and repeat CPU figures are load-confounded. The quiet-launch
byte-channel run uses about 0.186 cores versus 0.897–1.083 in the short
diagnostics: a large observed reduction, not a matched-duration CPU benchmark.
Its 0.015-core increase over the JSON-text run cannot establish a CPU regression
under these different loads; the same byte binary also measured 0.123 cores.
All accepted after runs retained a connected subscription, rendered Realm and
Nexus, and had complete process accounting. The JSON-text collector ended
before the full idle comparison interval; its 114.748 sampled seconds still
meet the report's minimum. The byte-channel collector's overall 645,382,144-byte
peak includes startup/seed outside the busy table window, and is not a startup
gate measurement. Executable hashes, exact durations, counts and aggregates
are in the [JSON evidence](2026-10-05-live-realm-memory.json). Raw heaps and
process records remain local temporary evidence.

## Remaining acceptance

The representative envelope remains **594,735,104 busy / 219,209,728 idle** combined private bytes. Remaining private-memory retention is not fully attributed or fixed. Follow the [performance policy](../release/performance.md); do not subtract presumed observer cost or substitute JS heap size for private bytes.

The historical retained replay audit found only one of nine boss kills in both the full and bounded final UI windows. It consumed all events but did not simulate every 250 ms snapshot. The 64-row eviction of kill history is now fixed with compact, separately bounded visit history; the regression replay retains all nine kills with both payloads. Kill, announcement and loot caps report specific partial-visit reasons; see [known limitations](../known-limitations.md#active-limitations). This correctness fix has no new Windows renderer private-byte measurement and does not change the historical memory results above.

Retest a 30+ minute real realm session, then idle in Nexus, tracking the complete process tree. Check local-hit boss focus and quest priority, confirmed-kill timing, same-instance return with time away excluded, player DPS and under-three-second unavailable values, dungeon totals, late bags, PBs, reconnect continuity and honest chart gaps. Repeat with real catalogs/sprites, populated history, many encounters and installed builds. None of those live checks is claimed here.
