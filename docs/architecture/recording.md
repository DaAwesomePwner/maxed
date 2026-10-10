# Recording

Recording is implemented, off by default, and enabled explicitly in Settings. Capture decoding and summary storage continue independently of recording failures. Raw recordings are sensitive; see [privacy](../../PRIVACY.md).

## Caller and lifecycle

Native setup supplies Tauri's resolved per-user app-data directory. Only its
absolute `recordings` child reaches the disk worker. The library verifies the
absolute child name and rejects reparse ancestors. The native caller attests the
parent is app-data; paths never come from the WebView. Tests use temporary roots.
Default/legacy-off startup, live capture, demo and replay create no recording
files or leases. Explicit opt-in validates storage and attaches a sink to an
already running live pipeline; idle opt-in validates then reports Armed. Disable
and cap changes detach and join only the recording worker. Capture, stream,
session and open fights are preserved. A recording enabled mid-connection is a
partial traffic observation. Clean stop and launch with a persisted opt-in report
Armed; failure status remains visible. During a live cap change the old owner is
detached, drained and joined before its replacement opens the namespace. Frames
arriving in this swap gap are skipped without blocking capture and counted in
dropped_frames, alongside queue-overflow drops. The runtime carries a base offset
across cap swaps so status never resets or decreases during the same recording.
After capture stops the final count is retained as past recording history; a new
recording or disabling recordings resets it.

Setup serializes recording configuration with settings, adapter/catalog changes,
demo, update and exit. Admission acknowledges open/inventory before committing
true; failed settings persistence restores the previous recording choice. Disable
joins and clears a failed recorder before committing false. Startup recording
failure does not prevent capture. Demo refuses new opt-in; demo and replay never
feed the recorder. Existing stop/update/exit paths quiesce capture, join recording
and then close summary storage. Repeated retirement consumes no second worker.

RecordingRuntime also serializes its own configure/retire calls. Capture admission
holds only a short slot mutex; it never holds that mutex across open, scanning,
write, sync or join. The worker status mutex likewise guards only fixed-size
status updates. Disk work is confined to one named joinable worker. The queue
reserves at most 16 MiB (16 * 1024 * 1024) of queued payload bytes, with a
secondary 16,384-frame cap bounding metadata overhead even for empty frames.
One in-flight worker frame can coexist. Oversize frames are rejected before
copying. Full queues drop the incoming frame without blocking or retiring the
recording. A monotonic per-recording dropped_frames counter reports these gaps
and native cap-swap gaps;
classic pcap contains only accepted frames. Space released by the worker admits
later frames. QueueFull remains a compatibility code, not a normal failure.
Write/flush/sync failures still stop recording.
Capture decoding, bridge delivery and SQLite storage proceed independently.

## Namespace and ownership

The zero-byte `.maxed-recording-v1.lease` uses safe Windows exclusive share mode
and OPEN_REPARSE_POINT; its handle lives until data handles close. It is never
removed. A nonempty/reparse lease is rejected. Cross-process exclusion is tested.
The recordings directory and its immediate app-data parent are held by metadata
handles denying deletion/rename. Higher ancestors are checked for reparse points
but not held open; same-user hostile changes above the app-data directory are not
a security boundary this cooperative owner can establish.

A segment stem is `maxed-v2-{20 decimal sequence digits}-{32 lowercase hex digits}`.
The independent local random suffix comes from RandomState's random seeds and
contains no account, character, server or clock input; it is not a secret. The
validated sequence orders creation; duplicate sequences or sequence exhaustion
block admission. No access-time ordering or wall-clock assumption exists. Tests
inject creation order through invented owned fixtures, without a real clock.

Each segment has an immutable `.owner` sidecar containing the format magic,
exact stem and Windows creation timestamp of its data file. It is created with
create_new and synced before the pcap header is written. Sidecars are opened
without following reparse points, deny mutation during the bounded read, and
must exactly match; reads never exceed 4,096 bytes. Filename alone never confers
ownership. Sidecars are provenance within this cooperative namespace, not a
cryptographic defence against another process running as the same user.

Inventory visits at most 4,096 total directory entries, including lease and
sidecars (at most 2,047 data/sidecar pairs). Unknown names, unpaired sidecars,
legacy S3 v1 files, directories and reparse entries are preserved and block admission.
The entire inventory is validated at open, before recovery rename or eviction,
and at segment close and rotation. Between those boundaries, periodic sweeps
stat only the tracked closed data files, checking file type, reparse flag, length
and creation/write timestamps. Each frame checks at most four closed entries,
in addition to one metadata query on the exclusive active handle. Periodic work
never walks the directory or reopens sidecars. A sweep starts no sooner than five
seconds after admission, full revalidation or completion of the preceding sweep;
it resumes across frames until all tracked closed entries have been checked.

External growth or replacement of a tracked closed file stops recording when
that entry is next checked. With sparse traffic a sweep takes longer to finish;
close/rotation and eviction still perform full validation. New unknown entries
and changed sidecars are detected at those full-validation boundaries, not by
periodic metadata checks. Concurrent external mutations can temporarily exceed
the byte cap; this cooperative owner does not promise instantaneous enforcement.

## Quota and interrupted files

Pcap quota counts actual/reserved data bytes: 24 global-header bytes and
16 + captured length per frame. Lease and bounded ownership metadata are outside
the pcap quota. All prospective arithmetic is checked. Limits remain compatible
with settings' 1..=1,000,000,000 range; the disk owner rejects values below 41 before
namespace access and rejects an individual frame that cannot fit before eviction.
Maximum frame storage is 65,551 bytes; a maximum first frame requires 65,575.

Segments start as `.partial`, with ceiling min(64,000,000, quota - retained closed
bytes). Rotation flushes, syncs, closes and renames to `.pcap` before that segment
can be a cleanup candidate. Only the oldest validated closed owned files are
evicted to meet both the byte quota and the 4,096-entry bound. A new pair evicts
the oldest closed pair when 2,047 pairs already exist; short sessions therefore
do not permanently exhaust inventory capacity. Already over-bound or ambiguous
external inventories still block admission without deletion. The active exclusive file is included in accounting and never deleted
to meet quota. Delete failure stops recording. Sidecars are removed only after
their data file; an interrupted deletion leaving an orphan blocks future admission.

After obtaining the lease, leftover `.partial` files require valid sidecars and
a complete bounded classic-pcap scan. Per admission, at most 64,000,000 partial
bytes are scanned, with at most 4,000,000 frame headers per file and a single
65,535-byte frame allocation. Valid complete prefixes are renamed `.interrupted`
to preserve the interrupted label, then enter chronological closed cleanup.
Malformed/truncated/over-budget or ambiguous partials remain untouched and block
admission for local repair. No truncation or silent salvage discards tail bytes.

## Validation limits

Synthetic Windows tests cover ownership spoofing/missing/oversize sidecars,
unknown-file refusal, quota boundaries/rotation, active protection, external
growth, malformed/truncated recovery, inventory bounds, junction rejection,
namespace exclusion in a separate process, partial write/disk-full/flush/sync
fault injection, and nonfatal burst overflow. App tests drive the real
capture/pipeline/engine/bridge/SQLite path while the recording disk is blocked,
and reread the admitted pcap after repeated stop/shutdown. A regression proves
retirement does not hold the capture-admission mutex during disk join.

These tests do not claim actual volume exhaustion, installed-app behavior,
full enabled capture CPU/memory/disk budgets, or the complete concurrent
adapter/update/cancelled-exit matrix with recording enabled. Those checks remain
Windows live acceptance; they do not block the available opt-in setting.
