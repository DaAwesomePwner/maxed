# Home slim reads — 2026-10-13

These are synthetic measurements from the Home slim-reads implementation reports
for `cc765e07`, `466fc0df` and `1da2f72f`, starting from
`417d31fd`. Subsequent corrections preserve recap/profile behavior, skip no-op
window checkpoints and bound stale streak retries to whites in the current run;
the timings below were collected before those corrections, not remeasured.
The production `homeRedesign` flag was off when these were measured; these
reads were slimmed so the flag could be turned on (PR #151).

## Dungeon streak

Home now requests one dungeon's all-time streak instead of full Loot Stats.
Both fixtures contain 20,000 ended runs across 40 dungeons; the requested
`Dungeon-00` contains 500 runs. Six rounds alternate query order on the same
reader and assert equivalent results each round. Medians are milliseconds.

| Drops | Full Loot Stats | Scoped streak |
| ---: | ---: | ---: |
| 50,000 | 88.95025 | 0.49595 |
| 500,000 | 728.4898 | 0.56595 |

```powershell
cargo test --locked --offline --release -p maxed-store --test loot_performance home_streak_50k_500k -- --ignored --nocapture --test-threads=1
```

One test passed (39.04 s including the workload). No other test or Cargo command
ran during that measurement.

## Boss history and profiles

The fixture contains 25,200 fights over 180 days, 176,400 player rows, three
characters, four dungeons and 720 realm visits. Its 252,007,629 bytes of fight
JSON include player lists and 60 chart buckets per fight. There are 48 observed
target types within the 64-type request domain. Each case has a first read and
30 warm samples; the table reports warm median / p95 in milliseconds.

| Read | Before median / p95 | After median / p95 |
| --- | ---: | ---: |
| Home baselines, 90 days, 1 target | 1114.586 / 1230.653 | 339.736 / 378.160 |
| Home baselines, 90 days, 16 targets | 1367.696 / 1847.254 | 358.483 / 581.193 |
| Home baselines, 90 days, 64 targets | 2060.501 / 3245.710 | 490.725 / 854.850 |
| Boss history, 90 days, 1 target | 1120.654 / 1571.602 | 344.166 / 353.881 |
| Boss history, 90 days, 16 targets | 1609.041 / 2461.168 | 384.076 / 390.750 |
| Boss history, 90 days, 64 targets | 2102.118 / 4524.700 | 487.761 / 524.649 |
| Dungeon profile, 90 days | 680.573 / 722.822 | 501.562 / 604.104 |
| Boss history, all time | 4025.352 / 5394.233 | 1230.921 / 1852.829 |

Before, Home used full boss history; after, it uses lean baselines. The profile
is one dungeon request, independent of the 1/16/64 target cases. The after
64-target baseline response is 5,983 bytes versus 10,728 bytes for full history.
Their after medians are effectively tied on this run; a separate latency win
from the lean projection alone is not established for 64 targets.

Within the profile's boss SQL, extracted-column materialization had a
383.208 ms median / 392.001 ms p95, versus 1042.587 ms / 1347.673 ms for full
JSON. Both returned equal normalized results. This comparison isolates boss
materialization; the full profile includes unchanged clear-time and run-DPS SQL.

```powershell
cargo test --locked --offline --release -p maxed-store --test d_perf home_history_thirty_warm_reads -- --ignored --nocapture --test-threads=1
```

The same fixture was rebuilt for each implementation: one test passed before
(897.09 s) and after (560.02 s), including fixture construction. Warm samples
follow the maintained `d_workload::warm` convention (sorted sample 15 for median,
29 for p95); a first read does not imply an OS-cold cache.

No index migration was added. Narrow requests still inspect extracted target
types within the time range (about 340 ms for one target here). Measure an index
on extracted `target_type` as a follow-up if Home needs a tighter latency budget;
these results do not establish that a migration is necessary.

## Live-window projection

With the v4 observer attached, an ordinary N-event batch previously observed
N+1 projections; it now observes and publishes one, plus boundary checkpoints
when needed. Cached row and metadata revisions also reuse an unchanged window.
The bridge's same-phase history-stale notification does not add a checkpoint
inside the batch; real phase changes still checkpoint pending rows before retirement.

The scratch comparison used 128 cached synthetic drops, one ordinary fight,
preconstructed envelopes and ten batches per sample. It discarded four warmup
samples and took the median of 21 per-batch samples. Times exclude fixture
construction, rendering and recap computation.

| Fight updates per batch | Before median (ms) | After median (ms) |
| ---: | ---: | ---: |
| 1 | 0.016040 | 0.005700 |
| 8 | 0.045800 | 0.024970 |
| 64 | 0.184690 | 0.065970 |
| 1,024 | 3.186820 | 0.862150 |

```powershell
npm.cmd --prefix app/ui run test:ui '--' tests/home-v2/task3-timing.test.ts --maxWorkers=1
```

One scratch test passed (9.55 s overall, 1.217 s in the test). The command is
the historical invocation; the uncommitted harness and original live model
were subsequently retained beside the local reports as `task-3-timing-scratch.ts`
and `task-3-live-before.ts`, outside test discovery.

## Limits

All timings came from a shared Windows host and synthetic data. Other Cargo
processes and a short UI suite overlapped part of the boss-history before run; tail
variance is visible. The results are not isolated-host release gates, universal
latency guarantees or evidence of overall memory savings. No live Npcap,
WebView2, game-client or installer behavior was measured by these tests.
The earlier [live-window churn report](2026-10-07-live-window-churn.md) retains
its original evidence and its then-current identity behavior.
