# Storage maintenance: Phase 1

Phase 1 preserves all fight detail and receipt retry semantics. Settings can
request lossless receipt optimization, show progress, cancel maintenance between
batches and explicitly resume it, and show backup location, size and managed-backup
deletion controls. Pauses are system-driven. Capture startup on v12/v14/v15 does
not request a backup or conversion. An earlier unpaused
request may resume after restart or navigation.

Requests answered **Queued** stay in the native FIFO until an owner persists or
applies them. Stopping capture transfers pending Compact/Resume and following
Cancel commands to the stopped owner, even if no runnable job exists yet. Entering
Demo/Replay suspends that FIFO; playback never runs live-database maintenance. The
requests remain suspended while the last playback remains selected after Stop.
Leaving preview (including toggling Demo off) restores stopped-Live and starts a
stopped maintenance owner after releasing the lifecycle and alert locks; starting
live capture instead transfers the requests to its owner. Commands still awaiting
a reply at playback entry receive **Queued** and retain their FIFO position;
synchronous backup deletions fail for retry. A waiting caller losing its owner
does not discard other queued work. Once persisted, unpaused jobs also use the
existing restart Restore path. The pre-persistence FIFO is process-local, not a
crash-durable receipt.

If queued work must be rejected, including on application shutdown, explicit
history upgrade or unavailable storage, the service publishes an **Unavailable**
failure to Settings even when the original command already received its reply.
These terminal failures bypass the normal progress-notice throttle. Discarding
only internal, reply-less Restore entries does not create a command failure; the
durable jobs remain available for restoration by the next owner.

## What storage counts mean

Main-file, WAL and SHM bytes are separate observations. Allocated database pages,
reusable free pages, retained backup bytes and temporary candidate bytes are also
separate. Recordings keep their independent owner and quota. Unknown measurements
remain unavailable, not zero. Receipt optimization can make pages reusable without
reducing the main-file length. Phase 1 has no physical main-file shrink or history
detail deletion; inserts can reuse free pages. A retained backup consumes disk and
must not be counted as reclaimed space.

Cheap storage measurements read the database header without opening SQLite. The
upgrade status probe deliberately opens a read-only SQLite connection as an
exception to that rule. On an idle WAL database, that probe may create WAL/SHM
sidecars; it does not write the main database.

## Backup, downgrade and recovery

A destructive class requires its own verified backup completed after the request
and before its first destructive unit. Ordinary subsequent units reuse that
authorization. There is one initial backup slot and at most one rolling slot per
lineage. A temporary candidate may coexist with both; free-space admission cannot
credit future deletion of the previous rolling image. Unknown/insufficient space
pauses maintenance. Verification checks schema, integrity, foreign keys, checksum,
size and durable heads before publishing readiness. Online Backup API steps include
committed WAL. A failed candidate cannot replace a ready image.

The initial image is not rotated automatically. Deletion by managed ID refuses
in-use backups; deleting a required image invalidates its authorization so future
destruction first requires a new verified image. There is no automatic rewind after
a failed job. Conversion resumes from durable cursors, and failed transactions roll
back only their own unit while preserving newer committed capture.

A post-additive-016 backup is a pre-destruction recovery image, not an exact v12/v14/v15
binary downgrade image. Database backups do not include settings. An exact downgrade
requires a privately preserved, stopped pre-upgrade database **and matching settings**
for the old binary. Older settings parsers reject newer fields. Never lower
SQLite's user_version or copy an open main file while ignoring WAL.

Restoration is a separate explicit stopped operation, not an automatic response to
maintenance failure or a Phase 1 one-click restore feature. Stop capture, resolve
unsaved observations, close readers and the writer, preserve the newer database and
settings first with adequate destination space, then use the verified compatible
image. Explain which newer observations would be displaced before replacing files.
Do not restore over an active database.

Backup lineage identity combines the canonical database path and file creation
time. Moving the data directory or restoring the database by file copy makes the
existing managed backup set Invalid. This is a known Phase 1 limit with manual
recovery only: while Maxed is closed, preserve the database, settings, manifest
and all managed backups elsewhere first. Only after preserving them, remove
`backups/<stem>/manifest.sqlite3` and its associated backups from the active
backup directory. The next explicit maintenance request can establish a new
managed backup set; preserve the old set separately for recovery.

Installed v1–v10 histories instead require the explicit stopped **Back up and
upgrade** flow because already-existing migrations are destructive. Its first
backup occupies the initial slot. Cancellation and insufficient or unknown space
leave capture blocked with retry available. “Upgrade without backup” remains an
owner open question and is not shipped behavior. Empty new databases need no
history backup; unrecognized nonempty databases and invalid checksums fail closed.

## Reproducing synthetic evidence

From the repository root on Windows with the pinned toolchain:

```powershell
cargo test --locked -p maxed-store --test retention_performance phase1_600mb -- --ignored --nocapture
cargo test --locked -p maxed-store --test integration phase1_
cargo test --locked -p maxed-types --test integration integration_registration::
cargo test --locked -p maxed-store --test integration
```

Run one Cargo process at a time, followed by the complete
[Windows deterministic checks](testing.md#windows-deterministic-checks). On Windows
these replace the Mac full-check wrapper; native maxed-app tests must execute.

The ignored test installs immutable 001–012, 001–014 or 001–015 scripts/checksums and verifies
the exact schema fingerprint before upgrade. Real projected synthetic fights with
1,800 chart buckets grow occupied storage past 600,000,000 bytes. It adds 24 streams
with 256 canonical receipts each and 37-byte stream IDs, as well as history-event
receipts, an interrupted session and committed WAL. It creates no padding files or
padding tables. The test uses a private OS temporary directory, removes it afterward,
and never opens installed history or recordings. Its backup free-space provider is
a synthetic admission seam, not a measurement of an owner's disk capacity.

Existing 013/014 scripts and checksum writes are timed in a separate transaction
before production migrate applies 016. Main’s immutable 015 loot indexes are
also timed separately before the maintenance DDL. This isolates their existing cost, but is
not the application's combined migration transaction timing. The 013/014 timing
row uses a near-empty run population and is not representative of real upgrades.
Recovery is reported as recovery-inclusive writer readiness after DDL, followed
by three warm opens;
it is not an isolated recovery function measurement or a cold-cache startup test.
First durable new-event time includes submission and flush. Additive DDL is checked
against every history table, and startup structurally precedes any backup request.
A small barrier test additionally holds the actual backup before its first copy step
while capture commits, without asserting a latency SLO.

Before/after conversion checks stream typed-row hashes over every non-maintenance
history table, every receipt key/digest and every durable head, plus reader results
for Runs, Fight/player detail, Home Session/Today and HUD, boss history, dungeon
profile, Stats session party, Party run, Loot and PBs. Hash comparisons avoid printing
identities and keep the collector bounded by rows instead of database size. The
authored fixture has a small run/Party population and large runless chart history; it does not reproduce the owner's
workload distribution. All receipts are compared, and sampled old retries,
conflicting payloads and sequence gaps are exercised after conversion.

Unit distributions cover all committed conversion units, including validation and
phase transitions with zero moved receipts; positive-copy-unit counts are reported
separately. Total conversion throughput includes validation, transitions and yields
but excludes fixture generation, query comparisons and backup. Units are driven
by per-stream granularity: one preflight unit plus copy units per stream. The real
database's stream count and stream-length distribution therefore affect throughput
independently of total receipts; a linear projection to 546,000 receipts is not a
prediction. WAL high-water is sampled at unit boundaries and can miss an intra-unit
peak. Process working set and
peak working set are unavailable with the crate's existing dependencies; allocator
or heap size is not substituted.

Fast recovery tests terminate a child process without destructors at awaiting
backup, verified backup, partial receipt copy, verification, deletion and completion.
They reopen, inject job transaction failures, append newer capture, finish conversion,
and verify every old retry and the new receipt. Existing suites cover backup
publication/rotation crashes and copy/verify/delete/cursor/commit failures. These are
synthetic process-exit and SQLite-failure checks, not power-loss hardware tests.

## Gate A: owner acceptance remains pending

Automated results below cover synthetic behavior only. Before PR A merge/release,
the Windows owner must privately preserve the original database and matching
settings, record the installed revision, and measure cold/warm startup and
capture-ready time on the real approximately 600 MB history before and after.
Record Settings main/WAL/SHM/reusable/backup/recording byte totals before and after
receipt optimization; physical main-file shrink is not expected.

The owner must also compare a private sample of Runs/Fight/Home/Stats/Party numbers
and drill-downs, exercise progress/cancel/restart, capture while backup is pending,
and unsaved-error recovery. Share only aggregate pass/fail counts, timings and byte
totals. These owner items are **pending**. Synthetic checks do not verify live Npcap,
the game client, WebView2, installer behavior or the real database. Phase 2 / PR B
is outside this change.

## Dated synthetic baseline: 2026-10-13 (merged schema v16)

This post-merge Windows debug run used clean revision
`f6117e5ba4e2c2f5590edf358e2371197988f027` (`dirty=false`), AMD Ryzen 7 5825U
with Radeon Graphics, Windows 11 Pro 10.0.26300, rustc 1.99.0
(b940084d7, 2026-09-28), and seed 0x20261012. One invocation completed one sample
each for exact installed v12, v14 and v15 in **989.94 seconds**, including fixture
generation and equality checks. No other build/test command ran alongside it.

Each merged fixture contained 2,312 scale fights and 8,464 legacy receipts, including
6,144 receipts with 37-byte IDs. After recovery and the first durable new event,
each compared 40 history tables, 21 query results, 8,467 receipt proofs and 27 durable
heads; 48 exact retries, 24 conflicting retries and 24 gaps passed. Integrity/FK
checks passed and every private fixture was removed. The earlier baseline below
is retained with its original revision label; its 3,344-fight population and 17
query results differ, so these runs are not a controlled before/after comparison.

The table is a debug-profile, unloaded, single-sample run per schema. The 013/014
row measures a near-empty run population, not representative real-upgrade work.

| Measurement | Exact v12 input | Exact v14 input | Exact v15 input |
| --- | ---: | ---: | ---: |
| Fixture generation, ms | 145555.460 | 208513.874 | 166243.214 |
| Existing 013/014 transaction, ms (n=1 when applied) | 4.284 | Not applied | Not applied |
| Existing shipped 015 indexes, ms (n=1 when applied) | 1.403 | 2.322 | Not applied |
| Additive maintenance 016, ms (n=1) | 19.304 | 5.265 | 3.641 |
| Recovery-inclusive ready, ms (n=1) | 49.391 | 75.168 | 47.007 |
| First durable new event, ms (n=1) | 9.941 | 67.944 | 4.849 |
| Warm ready p50 / p95 / max, ms | 20.179 / 26.868 / 26.868 (n=3) | 22.056 / 33.965 / 33.965 (n=3) | 22.036 / 27.099 / 27.099 (n=3) |
| Deferred verified backup, ms (n=1) | 38099.656 | 43266.080 | 38910.442 |
| Online backup copy steps | 1,152 | 1,152 | 1,152 |
| Complete conversion wall time, s | 0.457 | 0.450 | 0.451 |
| Conversion attempts / committed units / deferrals | 64 / 64 / 0 | 65 / 65 / 0 | 64 / 64 / 0 |
| Unit wall p50 / p95 / max, ms | 7.546 / 11.592 / 16.934 (n=64) | 7.393 / 11.421 / 16.839 (n=65) | 7.538 / 11.458 / 17.242 (n=64) |
| Converted receipts/unit p50 / p95 / max, all units | 0.000 / 256.000 / 699.000 (n=64) | 0.000 / 256.000 / 697.000 (n=65) | 0.000 / 256.000 / 695.000 (n=64) |
| Converted receipts/unit p50 / p95 / max, positive units | 256.000 / 500.000 / 699.000 (n=31) | 256.000 / 500.000 / 697.000 (n=31) | 256.000 / 500.000 / 695.000 (n=31) |
| Complete-conversion receipts/s | 18521.282 | 18826.846 | 18756.234 |
| Linear projection for 546,000 receipts, s | 29.480 | 29.001 | 29.110 |
| Sampled maintenance WAL high-water, bytes | 4,074,712 | 4,087,072 | 4,074,712 |
| Working set / peak working set | unavailable / unavailable | unavailable / unavailable | unavailable / unavailable |

Each job reported 8,464 eligible and processed receipts, 876,134 eligible and processed
input bytes, zero blocked records and unavailable blocked bytes. These stopped
conversion measurements use a small receipt population with warm comparisons;
they do not establish real-history startup or loaded capture acceptance. The linear
projections exclude backup; no completion-time promise or batch-budget change follows from these samples.

| Byte observation | Exact v12 input | Exact v14 input | Exact v15 input |
| --- | ---: | ---: | ---: |
| Installed main / allocated bytes | 603,639,808 / 603,639,808 | 603,688,960 / 603,688,960 | 603,701,248 / 603,701,248 |
| Installed WAL / SHM bytes | 4,152 / 32,768 | 4,152 / 32,768 | 4,152 / 32,768 |
| Installed reusable bytes | 438,272 | 434,176 | 434,176 |
| Installed occupied bytes | 603,201,536 | 603,254,784 | 603,267,072 |
| Before conversion main / allocated bytes | 603,639,808 / 603,639,808 | 603,688,960 / 603,688,960 | 603,701,248 / 603,701,248 |
| Before conversion WAL / SHM bytes | 288,432 / 32,768 | 210,152 / 32,768 | 181,312 / 32,768 |
| Before conversion reusable bytes | 339,968 | 389,120 | 401,408 |
| Before conversion occupied bytes | 603,299,840 | 603,299,840 | 603,299,840 |
| After conversion main / allocated bytes | 603,639,808 / 603,639,808 | 603,688,960 / 603,688,960 | 603,701,248 / 603,701,248 |
| After conversion WAL / SHM bytes | 4,074,712 / 32,768 | 4,087,072 / 32,768 | 4,074,712 / 32,768 |
| After conversion reusable bytes | 1,277,952 | 1,327,104 | 1,339,392 |
| After conversion occupied bytes | 602,361,856 | 602,361,856 | 602,361,856 |
| Retained backup / temporary backup bytes | 603,639,808 / 0 | 603,688,960 / 0 | 603,701,248 / 0 |
| After explicit checkpoint WAL / SHM bytes | 0 / 32,768 | 0 / 32,768 | 0 / 32,768 |

Conversion made another **937,984 bytes reusable** in each sample; main-file length
did not shrink, and the retained backup increased total disk use. These three samples
do not establish the design's projected savings for approximately 546,000 receipts.

The merged-tree checks passed: Rust workspace 2,084 tests with 41 explicit skips
(including this separately executed scale test), store integration 539 passed with
10 skips, native maxed-app 418 unit tests with one skip plus 123 integration tests,
types integration 84 passed and DTO export 3 passed. All 3,045 unique UI tests passed
after the corrected schema expectation and keyboard reruns with two workers; all
4 Node tests passed. Formatting, workspace/perf-live Clippy, both Svelte checks and
the production UI build passed. A transient backup-test queue rejection passed an
isolated rerun and the later full workspace run; no test timeout or production
behavior was changed to address these flakes. Generated bindings had no additional
drift. Gate A's real Windows history/capture/UI acceptance remains pending as described above.

## Earlier synthetic baseline: 2026-10-12 (before the main merge)

These earlier measurements used maintenance migration 015 before main shipped
015_loot_stats_indexes. They do not measure the merged schema v16 or installed v15.

This Windows host's debug-profile run used AMD Ryzen 7 5825U with Radeon Graphics,
Windows 11 Pro 10.0.26300, rustc 1.99.0 (b940084d7, 2026-09-28), seed
0x20261012, starting revision c051948005f20bf5014cf50cb3b6e74daeb762c8 plus
the Task 9 working-tree test. One full sample per installed schema completed in
524.20 seconds total, including generation and equality checks. Each contained
3,344 scale fights, 9,496 legacy receipts (6,144 with 37-byte IDs), and 9,499
receipt proofs after recovery and the first live event. Both samples compared
40 history tables, 17 query results and 27 durable heads, checked 48 exact retries,
24 conflicting retries and 24 gaps, passed integrity/FK checks, and removed their
private fixtures.

The following table also reports a debug-profile, unloaded, single-sample run per
schema; its 013/014 row likewise uses a near-empty run population.

| Measurement | Exact v12 input | Exact v14 input |
| --- | ---: | ---: |
| Fixture generation, ms | 152,445.765 | 148,709.279 |
| Existing 013/014 transaction, ms (n=1) | 4.340 | Not applied |
| Maintenance DDL, ms (n=1; then 015, now 016) | 4.412 | 3.558 |
| Recovery-inclusive ready, ms (n=1) | 38.288 | 38.980 |
| First durable new event, ms (n=1) | 5.695 | 5.672 |
| Warm ready p50 / p95 / max, ms (n=3) | 14.117 / 20.245 / 20.245 | 16.972 / 22.460 / 22.460 |
| Deferred verified backup, ms (n=1) | 35,840.350 | 35,714.415 |
| Online backup copy steps | 1,150 | 1,150 |
| Complete conversion wall time, s | 0.517 | 0.513 |
| Conversion attempts / committed units / deferrals | 69 / 69 / 0 | 69 / 69 / 0 |
| Unit wall p50 / p95 / max, ms (n=69) | 7.672 / 14.549 / 17.327 | 7.569 / 13.348 / 21.906 |
| Converted receipts/unit p50 / p95 / max, all units (n=69) | 0 / 500 / 680 | 0 / 500 / 689 |
| Converted receipts/unit p50 / p95 / max, positive units (n=33) | 256 / 500 / 680 | 256 / 500 / 689 |
| Complete-conversion receipts/s | 18,365.949 | 18,504.351 |
| Linear projection for 546,000 receipts, s | 29.729 | 29.507 |
| Sampled maintenance WAL high-water, bytes | 4,210,672 | 4,210,672 |
| Working set / peak working set | unavailable / unavailable | unavailable / unavailable |

Each completed job reported 9,496 eligible and processed records, 967,982 eligible
and processed input bytes, zero blocked records and unavailable blocked bytes.
Unknown blocked-byte estimates were not turned into zero.

| Byte observation | Exact v12 input | Exact v14 input |
| --- | ---: | ---: |
| Installed main / allocated bytes | 602,681,344 | 602,730,496 |
| Installed occupied bytes | 602,193,920 | 602,247,168 |
| Installed reusable bytes | 487,424 | 483,328 |
| Installed committed WAL / SHM bytes | 4,152 / 32,768 | 4,152 / 32,768 |
| Before conversion main / allocated bytes | 602,681,344 | 602,730,496 |
| Before conversion WAL / SHM bytes | 284,312 / 32,768 | 206,032 / 32,768 |
| Before conversion reusable bytes | 401,408 | 450,560 |
| After conversion main / allocated bytes | 602,681,344 | 602,730,496 |
| After conversion WAL / SHM bytes | 4,210,672 / 32,768 | 4,210,672 / 32,768 |
| After conversion reusable bytes | 1,421,312 | 1,470,464 |
| After conversion occupied bytes | 601,260,032 | 601,260,032 |
| Retained backup / temporary backup bytes | 602,681,344 / 0 | 602,730,496 / 0 |
| After explicit checkpoint WAL / SHM bytes | 0 / 32,768 | 0 / 32,768 |

Conversion made another 1,019,904 bytes reusable in each sample; main-file length
did not shrink. Backup overhead increased disk use. This small receipt population
does not establish the design's projected reusable-space savings for the owner's
approximately 546,000 receipts.

Receipt comparisons warm this small receipt population before timed conversion;
that stopped conversion has no queued capture or pinned-reader load. These are
measured distributions and projections, not release SLOs. The approximately
30-second linear extrapolations exclude backup and do not establish performance
for a real 546,000-receipt conversion. Earlier review runs reported adaptive units
falling to roughly one or two receipts under load because FULL-sync commit cost
counted toward the 10 ms target. The scheduler now sizes batches using executor
work time, excluding fixed transaction setup, authorization, job-status updates
and commit costs. Slow commits alone no longer shrink batches, and cheap work
can grow a previously reduced batch even while commits remain slow. Admission
still checks elapsed time from before BEGIN, along with capture interruption,
record/byte ceilings and the first-record allowance. FULL durability, WAL-pressure
deferral and yielding between units are unchanged; a single SQL statement or
commit can still exceed the cooperative target.

Deterministic clock tests cover slow commits and recovery after slow record work;
the historical disk measurements above predate this change. No new real-disk or
546,000-receipt throughput measurement is claimed. Measure loaded and real-history
behavior before making a completion-time promise. During stream validation the
public converted-row count does not advance: only verified, copied and deleted rows
count as processed. The committed validation cursor advances, but exposing it as
progress would require a separate validated-row count to avoid double counting.

The accompanying Windows checks passed: workspace 2,008 tests with 41 explicit
skips (including this separately executed scale test); standalone store integration
487 passed with 10 existing performance skips; native maxed-app 409 passed with
one timing test skipped; types integration 81 passed; DTO export 3 passed; UI
2,806 tests across 235 files plus 4 Node tests passed. Formatting, workspace and
perf-live Clippy, both Svelte checks and the production UI build passed. Generated
bindings had no diff. No listed Rust flake or UI timeout occurred, so no flake rerun
was needed. The Windows checks completed on 2026-10-13 after the dated scale run.
