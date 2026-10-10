# Native runtime

The native application joins capture, decoding, interpretation, storage and presentation. The engine remains pure; filesystem, Npcap, WebView and notification operations belong to their native owners.

## Capture and clocks

The capture worker owns TCP reassembly and directional decode, then passes owned packets and diagnostics to one engine. Parsed game TCP payload counts as traffic; UI timers and unrelated frames do not. Unverified cipher alignment cannot establish game facts. A 250 ms tick handles idle expiry and dirty presentation even before the first game frame. Queued frames take priority. Quiet periods longer than five seconds advance a conservative wall-clock estimate with a five-second buffer for delayed capture batches.

A stream owns an immutable validated catalog and the installation's random 32-byte identity key. Account strings are immediately HMAC-SHA256 hashed with domain separation; only opaque character IDs leave the engine. Remote identity is bound to observed object generations, not player-name joins. `observe_live_traffic` and `apply_observed` separate traffic timing from decoded packets; offline adapters use replay timestamps. Thirty minutes without game traffic ends a session.

Asset extraction runs off the UI thread. The retained catalog lease protects its cache generation. Unavailable or rejected assets produce explicit partial capture with `MissingCatalog`; a stale cache never silently substitutes for the selected client.

## Ordered delivery and durability

The adapter numbers every engine envelope and submits the original batch to storage before bridge delivery. A snapshot's stream/sequence watermark and the ordered event cursor have different roles. Subscribe before querying `get_live_snapshot`; use `get_store_status().durable` before reading history at that watermark. Queue acceptance is not a disk acknowledgment.

`SnapshotGate` publishes dirty snapshots at most once per 250 ms, with explicit opening/finalization flushes. Domain events bypass that gate. A bounded eight-transaction FIFO backpressures bridge producers instead of dropping envelopes; event transactions split at 1,024 envelopes before enqueueing. Storage and alert contexts retain the full originals first; `publish` compacts each presentation copy exactly once before queueing, and later delivery stages retain that compact payload. Thus the FIFO holds at most 8 * 1,024 = 8,192 compact event envelopes (plus one dequeued batch and the producer's pending transaction outside the queue), rather than 32,768 full envelopes. This is an event-count bound, not a universal byte bound: event shapes vary. The perf-only `before` mode intentionally retains full envelopes. Native lifecycle locks do not emit to the WebView. Storage receives full domain events before any presentation compaction.

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

## Storage pressure and shutdown

When the writer rejects an envelope because its queue is full, capture consumption pauses with the rejected envelope and remaining batch retained. Npcap's bounded input queue may then drop frames; frame-index gaps enter normal recovery diagnostics. Storage failure stops capture and retains accepted and unaccepted summaries. `retry_store` retries the original ordered queue; a terminated writer transfers pending ownership to its replacement.

Stop uses `CaptureStopped`; application shutdown uses `AppClosed`. A directional transport close alone is not a map exit. Finalization publishes terminal events, recording is joined, and storage flush/shutdown completes off the UI thread. Only retained unsaved summaries veto the first exit attempt. The native title provides a second-close discard fallback, and `exit_without_saving` supports an explicit action. A successful retry clears the warning. Discard preserves committed history; memory-only pending changes are vulnerable to process or power loss.

Live uses `live.sqlite3` and a fresh stream per capture. Exclusive Windows namespace leases prevent concurrent owners. Demo/replay use separate scratch databases, recreated on the next playback. A successful stop leaves their last playback available for durable history queries. They do not send game packets or external alerts and never feed the raw recorder.

History reads run on one lazily started `history-reader` thread, which keeps a query-only SQLite connection between successful reads of the same database and generation. Every reuse checks the schema version; errors, panics, namespace retirement, shutdown and 60 seconds of idle time close the connection. Idle connections hold no transaction, so writer checkpoints remain unaffected. Lifecycle waits track both active work and idle kept connections: after `invalidate_and_wait` or `shutdown_reads` returns, no reader remains open, including before Windows playback file cleanup and update installation. Application shutdown closes the idle reader before the writer.

## Engine bounds and uncertainty

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
