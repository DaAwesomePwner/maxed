# Native runtime

The native application joins capture, decoding, interpretation, storage and presentation. The engine remains pure; filesystem, Npcap, WebView and notification operations belong to their native owners.

## Capture and clocks

The capture worker owns TCP reassembly and directional decode, then passes owned packets and diagnostics to one engine. Parsed game TCP payload counts as traffic; UI timers and unrelated frames do not. Unverified cipher alignment cannot establish game facts. A 250 ms tick handles idle expiry and dirty presentation even before the first game frame. Queued frames take priority. Quiet periods longer than five seconds advance a conservative wall-clock estimate with a five-second buffer for delayed capture batches.

A stream owns an immutable validated catalog and the installation's random 32-byte identity key. Account strings are immediately HMAC-SHA256 hashed with domain separation; only opaque character IDs leave the engine. Remote damage attribution is bound to observed object generations. Party separately merges roster rows and links history by the normalized display-name key. `observe_live_traffic` and `apply_observed` separate traffic timing from decoded packets; offline adapters use replay timestamps. Thirty minutes without game traffic ends a session.

Asset extraction runs off the UI thread. The retained catalog lease protects its cache generation. Unavailable or rejected assets produce explicit partial capture with `MissingCatalog`; a stale cache never silently substitutes for the selected client.

## Ordered delivery and durability

The adapter numbers every engine envelope and submits the original batch to storage before bridge delivery. A snapshot's stream/sequence watermark and the ordered event cursor have different roles. Subscribe before querying `get_live_snapshot`; use `get_store_status().durable` before reading history at that watermark. Queue acceptance is not a disk acknowledgment.

`SnapshotGate` publishes dirty snapshots at most once per 250 ms while an open fight has accepted damage within the preceding five seconds and once per second otherwise, with explicit opening/finalization flushes. Domain events bypass that gate. A bounded eight-transaction FIFO backpressures bridge producers instead of dropping envelopes; event transactions split at 1,024 envelopes before enqueueing. Storage and alert contexts retain the full originals first; `publish` compacts each presentation copy exactly once before queueing, and later delivery stages retain that compact payload. Thus the FIFO holds at most 8 * 1,024 = 8,192 compact event envelopes (plus one dequeued batch and the producer's pending transaction outside the queue), rather than 32,768 full envelopes. This is an event-count bound, not a universal byte bound: event shapes vary. The perf-only `before` mode intentionally retains full envelopes. Native lifecycle locks do not emit to the WebView. Storage receives full domain events before any presentation compaction.

Main-window resize/focus events reconcile native visibility and minimization;
`set_document_visible` adds the document visibility signal. Hidden snapshots and
Party notices are discarded before queueing, and visibility epochs reject these
already queued publications from before a hide/restore boundary. Ordered domain
events, capture status, storage, capture clocks and native alert delivery continue.
During live capture, restore requests a fresh full snapshot from the capture worker
before queueing it. Restore resets delta encoding and invalidates Party in every
mode. The UI queries a numeric baseline without retiring its event-sourced Home window or advancing the
ordered event cursor past deliveries still in flight. Hidden events have already
updated the model and history checkpoints; restore does not replay them.
An IPC send or snapshot build already in progress at hide time may finish. Hidden
live capture skips scheduled snapshot builds and leaves the publication gate dirty;
showing the window permits the next due publication. Explicit lifecycle snapshots
and direct reads still materialize while hidden.
During live capture, direct native reads (`runtime.snapshot()` and `get_live_snapshot`)
materialize on demand after the worker's current transaction, including metadata
changes that do not emit a domain event. These reads neither publish a snapshot
nor consume/reset the publication gate. Clean reads reuse the cached projection.
The Tauri query waits off the UI thread; storage/bridge backpressure can delay a
read. Stopped or failed workers leave their last finalized cached view available.

Demo and replay do not service fresh-snapshot requests. Their direct reads and
restore baselines use the last cached projection, which can lag by the publication
interval (up to about one second while idle) and omit metadata-only changes until
the next snapshot-producing update or finalization. They retain scheduled cache
builds while hidden; their snapshot publications are still suppressed.

The summary writer retains the validated canonical digest beside each immutable
queued envelope and reuses it at commit/retry. Direct batch projection still
validates its inputs. Digest semantics, duplicate checks and projections are unchanged;
the 32-byte binary digest counts toward the existing writer byte budget. Presentation
JSON remains separate because compacted events differ from stored envelopes.

The high-frequency logical topics `engine-event` and `live-snapshot` use fetch-backed Tauri byte channels. `subscribe_live_payload(event, channel)` returns an ownership ID; `unsubscribe_live_payload(id)` releases it. UTF-8 JSON bodies are padded with whitespace to at least 1,024 bytes to avoid the small-body inline-JavaScript path. The shared UI transport decodes them once. `capture-status` remains a typed event; queries return typed objects. Channel ownership is scoped to a WebView label, purged on page load/destruction, and bounded to 16 subscriptions; failed subscribers are removed independently.

`LiveSnapshotUpdate` is a full presentation baseline or `LiveDelta`. A delta carries replacement metadata, changed fights/bags, explicit removed IDs and the exact preceding snapshot sequence. `get_live_snapshot` always returns a full baseline. The client resyncs on a missing base, event gap or changed stream; BigInt comparisons preserve sequence precision. Retired streams and obsolete queries cannot overwrite current state. Event buffering is bounded to 512 received messages and 8,192 envelopes (matching the native queue's eight maximum-sized batches); repeated failures expose a reconnect action. Resync does not reconstruct missed celebrations.

`engine-event` accepts a legacy `EventEnvelope` or a generated `EventBatch`. The
adapter retains the full ordered engine transaction and alert contexts for storage
first. The publisher compacts each presentation envelope exactly as before and
sends a batch with stream ID and inclusive first/last sequence. Every member keeps
its own sequence; gaps and stream resets split batches. Capture publications,
snapshot watermarks and explicit flushes stay ordered after the transaction.
`maxed-types::EVENT_BATCH_LIMIT` owns the cap; `export_bindings` generates the UI constant in `bindings/limits.ts`.
The single-envelope UI path is used only by the perf-only `before` publisher and the fixture transport. Native singles enter the same bounded batch queue; its worker has no single-event variant or silently dropping fallback.

The UI validates every batch range before applying it, then uses the existing
per-envelope cursor, generation, side-effect and projection rules. Received events
covered by a recovery snapshot still drive side effects exactly once; covered
projections and initial-baseline history remain silent. Recovery replay is also a
presentation transaction. Home publishes one immutable visit window per batch.
As of 2026-10-13, Home reuses the complete window identity when its row,
fight-sequence and metadata revisions are unchanged; drops use the same cached
array projection as the other row collections. With Home v4 enabled, recap
observation occurs at the boundaries described in [UI](ui.md) and at batch end,
without requiring a new window object for an unchanged checkpoint. Ordered
event callbacks remain independent of projection reuse. See the
[synthetic timing evidence](../performance/2026-10-13-home-slim-reads.md).
Focus and bounded-history eviction decisions stay ordered, since deferring those
decisions could resurrect previously evicted rows or change sticky focus. Eviction
uses cached timestamps and a bounded minimum scan instead of repeated per-event
array sorts (bulk snapshot seeds still sort once);
ordinary-fight traffic at one timestamp reuses the focus decision. Visual-type
collection caches immutable fight-array identities. Storage/PB/alert evaluation,
whole-run totals, compact visit history limits and same-instance continuity do not
change. PB completion refreshes use Shell's ordered per-event observer, including
late completion-counter confirmation of an ended run, and wait for the matching
store watermark. Alerts/toasts and history invalidation (including logs) already
use this path. Native notifications evaluate full stored events. The recording
indicator queries current recording status on capture-state changes and needs no
per-event replay; its latest-status display can safely coalesce renders.

A deterministic 2,048-fight realm close test produces 2,049 envelopes (including
map entry): three presentation messages at the 1,024 cap, 2,310,811 JSON bytes
(largest message 1,156,184 bytes), versus 2,049 messages and 2,308,498 unpadded
JSON bytes before batching. The old 4,096 cap produced one 2,310,635-byte message. The usual following
`live-snapshot` delta remains a separate message. These are synthetic serialization
counts, not a memory or latency measurement. The synthetic WebView2 workload
measurements are in the [fixes report](../performance/2026-10-06-live-realm-fixes.md).

`Engine::live_snapshot` filters presentation to relevant boss/quest and recent local-hit fights. `Engine::snapshot` and storage retain the full domain view. Charts compact contiguous observations without bridging gaps; the live payload bound is `max(120, contiguous-run count)`, up to 1,800 engine buckets, while UI display is bounded separately. Per-player DPS uses accepted first-to-last hit time. Per-player DPS is unavailable (null) when that player's active window is shorter than three seconds. Rankings and shares remain damage-based. [Parity exports](../parity/README.md) use a separate reference-compatible rate.

## Party roster

The native `PartyRoster` keeps the current observation scope outside `LiveSnapshot`.
It combines `PlayerPresence`, profiles, instance events, run totals and boss fights,
using the engine's public `PROFILE_LIMIT` cap of 512 player instances per scope.
A scope change replaces the roster and clears the manual preset pick.
Same-name instances merge for display using
`name_key`: text before the first comma, ASCII-lowercased. The latest profile wins;
the row is present if any matching instance is present.

`Engine::take_roster_profiles` and `Engine::take_roster_presence` supply
non-persisted roster profiles and presence to the adapter, including bystanders
in Nexus and the realm. Neither path enters the writer, creates event receipts,
or sends engine events to the UI. Presence retains its position within the
accompanying engine batch so scope-end absences precede the new scope and identity
merges stay ordered. The adapter drains presence after each engine update;
undrained presence is discarded at the next update. Outside dungeon runs, only
players who fought become eligible for persisted `PlayerObserved` profiles.
Deliberate marks and notes can
still create a Party history entry.

Dirty roster changes produce a presentation-only `party-changed` notice containing
`{ scope_id }`. A change from the last delivered `scope_id` delivers immediately,
supersedes any pending notice and resets the throttle window. Within the same
scope, the bridge delivers at most once per second with a leading delivery and
the latest trailing notice, including when traffic goes quiet. Explicit flush
and disconnection deliver a pending notice immediately. This notice never enters
storage. Live re-reads on these notices; saved views use durable engine-event
invalidation.

`get_party_live` queries annotations for at most 512 keys through the shared
history reader on each read; it keeps no annotation cache. Both live and saved-run
queries resolve presets and evaluate requirements against the local catalog.
The `PartyView.live` flag comes from active live capture; demo, replay and stopped
capture do not get a Live label merely because a scope remains available.

Presets save through `update_party_settings`, which patches only Party settings.
General settings saves preserve the stored Party section. The validated RealmEye
action launches the system `explorer.exe` with the profile URL for the default
browser; the application does not fetch it.

## Storage pressure and shutdown

When the writer rejects an envelope because its queue is full, capture consumption pauses with the rejected envelope and remaining batch retained. Npcap's bounded input queue may then drop frames; frame-index gaps enter normal recovery diagnostics. Storage failure stops capture and retains accepted and unaccepted summaries. `retry_store` retries the original ordered queue; a terminated writer transfers pending ownership to its replacement.

Stop uses `CaptureStopped`; application shutdown uses `AppClosed`. A directional transport close alone is not a map exit. Finalization publishes terminal events, recording is joined, and storage flush/shutdown completes off the UI thread. Only retained unsaved summaries veto the first exit attempt. The native title provides a second-close discard fallback, and `exit_without_saving` supports an explicit action. A successful retry clears the warning. Discard preserves committed history; memory-only pending changes are vulnerable to process or power loss.

Live uses `live.sqlite3` and a fresh stream per capture. Exclusive Windows namespace leases prevent concurrent owners. Demo/replay use separate scratch databases, recreated on the next playback. A successful stop leaves their last playback available for durable history queries. They do not send game packets or external alerts and never feed the raw recorder.

History reads run on one lazily started `history-reader` thread, which keeps a query-only SQLite connection between successful reads of the same database and generation. Every reuse checks the schema version; errors, panics, namespace retirement, shutdown and 60 seconds of idle time close the connection. Idle connections hold no transaction, so writer checkpoints remain unaffected. Lifecycle waits track both active work and idle kept connections: after `invalidate_and_wait` or `shutdown_reads` returns, no reader remains open, including before Windows playback file cleanup and update installation. Application shutdown closes the idle reader before the writer.

Saved-history scheduling retains one active read, at most eight pending page
queries and the existing eight-entry storage FIFO. Only an identical query key
(including account revision) coalesces a pending page request; unrelated queries
retain FIFO order. A full queue returns Busy without displacing another reader.
The existing 32-attempts-per-second admission limit remains in force.
Saved-history page controllers retry Busy or Superseded once after at least one
second, then expose manual retry while retaining saved values. Home uses one
serial queue across summary, loot, PB, v4 cards and recap commands. Its summary
and loot readers retry once after two seconds; supplemental readers allow two
two-second supersession retries (and one Busy retry for v4/recap). Exhaustion
remains explicit through the owning card's error/retry state; PB baselines retain
their last value. Recap SQL and its save timeout pause while hidden. Party's
saved-run view continues its throttled durability checks until the store catches
up or the view is disposed.


Receipt maintenance uses the same storage owner as capture. Accepted events,
retry, flush, shutdown and retained unsaved work take priority. Default units admit
at most 1,000 receipts and 1 MiB serialized input with a cooperative 10 ms target;
batch sizing uses executor work, excluding fixed setup, status and commit costs.
`reserve` still bounds record admission using elapsed time from before BEGIN.
Batch-proportional commit work (WAL page writes) is bounded only by those ceilings;
its real-disk cost remains unmeasured. These are cooperative budgets, not hard
statement or transaction latency guarantees. Busy readers and
the 32 MiB maintenance WAL budget cause checkpoint attempts or deferral. Progress
notices are throttled to at most once per second; terminal rejection of queued
requests publishes immediately so it does not depend on another owner turn.

The native owner starts receipt backup/conversion only after the Settings request,
or resumes a persisted unpaused request. Backup steps run outside the capture owner;
failed or pending backup does not prevent v12/v14/v15 capture. Stopped maintenance uses
an exclusive owner and yields it when capture starts. Owner transitions retain the
process-local request FIFO, including Compact/Resume already answered Queued and
subsequent Cancel commands. Stop restarts pending work even without a runnable
persisted job. Demo/Replay suspend the FIFO until a live owner returns; neither
playback nor its retained Home context admits live maintenance. `reset_preview`
returns to stopped-Live and resumes the FIFO after releasing its session and alert
locks. Playback handoff answers pending control callers Queued while retaining
their order, and rejects synchronous deletions for retry. A reply waiter's loss
of an owner never drains unrelated requests. Shutdown joins the
live owner before rejecting outstanding requests, and rejection during shutdown,
explicit history upgrade or storage failure publishes Unavailable to the UI.
Rejecting only internal, reply-less Restore entries leaves the failure state
unchanged; their persisted jobs remain restorable.
Persisted unpaused jobs still restore after restart; the FIFO itself is not
crash-durable. Storage reads retain namespace
and generation context. Legacy v1–v10 preparation instead stops before opening
history and exposes the explicit backup/upgrade prompt, including space and retry
feedback; cancellation keeps capture blocked. The
[Phase 1 synthetic evidence](../development/storage-maintenance.md) does not establish
live Npcap, game-client, WebView2, installer or real-history timing acceptance.

## Engine bounds and uncertainty

Player profiles are capped at 512 per observation scope. A dungeon scope uses
the run id; other maps use a map id. The scope lasts until map closure even if
the run ends earlier. Dungeon scopes publish every observed player; outside runs,
profiles stay pending in memory until the player deals attributed damage in that
scope. Eligibility survives timeline overflow. Dirty profiles publish on the
250 ms engine flush;
`last_seen` changes alone publish with the next content change or at scope end.
Exceeding the profile cap marks `ResourceLimit` and stops admitting new profiles.
The stored gear log is capped at 64 rows per player: the first 63 changes and the
most recent gear are retained. Starting gear uses the profile's `first_seen`;
later swaps use the publishing profile's `last_seen`.

Scope damage timelines accumulate accepted attributed contributions across all
fights in the map. Dirty timelines publish every five seconds and flush at scope
closure, with per-bucket party damage and counts of players with positive damage.
Updates carry only changed player rows unless mid-array insertion, compaction or
identity merging requires every row. The sparse timeline compacts adjacent
buckets above 1,800 buckets, coarsening its resolution. Its 200,000-cell limit
also bounds the dense player-by-bucket projection; overflow disables the timeline
for that scope and publishes `disabled: true` once, without changing fight totals.

Default ceilings are 100,000 entities, 50,000 shots/reconciliation groups, 2,048 fights and 1,024 bags. Shot evidence expires after five seconds. Unreferenced entities leave memory 30 seconds after leaving view; bounded tombstones prevent reopening recently retired targets. Fight charts compact above 1,800 buckets. Each fight has at most 256 source summaries per player and 4,096 represented players; overflow retains explicit resource-limit evidence and unattributed totals. Series are capped at 200,000 cells; exceeding the bound disables the series rather than changing damage totals. Instance events are capped at 4,096 per map.

Server damage remains observed even before target metadata arrives. Reconstructed local hits retain provenance and assumptions, reconcile once with server amounts, and remain uncertain when inputs or RNG trust are missing. `reconstruction_coverage()` reports lifetime attempts, omissions and assumptions; these are diagnostic counters, not stored run counts. [Personal-best rules](personal-bests.md) require stronger evidence than displaying a damage estimate.

These bounds and synthetic regressions do not prove live performance, Npcap recovery, installed-client extraction or WebView lifecycle behavior. Use the [release checklist](../release/CHECKLIST.md) for those checks.

## Engine adapter requirements

Production adapters must supply the installation's random identity key through
`EngineConfig`; its default in `crates/maxed-engine/src/engine.rs` is all zeros
and is unsuitable for production identity hashing.
`CompletionCounts.by_index` and catalog completion indexes use the dungeon-array
position, not the wire bit number. The decoder follows the separate wire read
order. Class caps require all eight values, with all eight observed stats and
boosts available before the character's maxed-stat count is known.

WebView2 launch arguments and their isolation trade-offs are documented under
[WebView2 memory policy](../release/performance.md#webview2-memory-policy).

### Current-account history

The native history context retains the last verified account across identity loss
and provisional map transitions. Opening or restoring a source resolves its own
DB pointer; playback never runs live legacy recovery. Scoped queries use one
store snapshot for pointer resolution and aggregates, and verified identity reads
wait for the complete acquisition batch, including the identity transition after
CharacterUpdated. Repeated same-account stat and fame updates do not advance
that acquisition watermark. If the acquisition is not durable, a failed writer
returns unavailable immediately rather than occupying the reader until the
durability timeout. Once acquisition is durable, ordinary updates do not delay
reads. Account revisions participate in request deduplication, admission and
final-response checks: a same-stream
switch retires an earlier account-dependent request as `HistoryReply::Superseded`.
Account-independent queries, including storage status, retain their source lease
without depending on the account revision.

On verified live identity, a cancellable background worker recomputes character
numbers 0 through 65535 with the installation key in batches of 4096. Progress
reads use the single history-reader thread; attachments and progress commits use
the existing writer. Shutdown joins recovery before taking ownership of the
writer for shutdown. Namespace retirement stops the sweep. Completed prefixes
resume after restart; repeating an attached batch is safe. Unmatched rows remain
stored and excluded from account totals. Recovery is submitted only on verified
account changes; a failed attempt is not retried on repeated identity events.
Reclaiming the writer clears that attempt state so its replacement can resume.
A sweep that attached any character emits its notice even when cancelled before
completion, provided the same live namespace generation remains current. Retired
sweeps cannot invalidate playback or a newer live generation. Recovery emits no
identity diagnostics.

`account-history-changed` carries `AccountHistoryChanged { revision, reason }`,
with `account_switched` or `legacy_recovered`, never an account ID. The UI should
invalidate Home summary, HUD, scope loot, bests, streak, dungeon profile and boss
baselines, and retry superseded requests for its current view.
