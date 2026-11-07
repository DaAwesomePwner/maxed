# Summary storage

`maxed-store` persists the domain/query DTOs. It stores typed summaries,
indexed scalar projections, and SHA-256 event receipts. It never stores packets,
transport state, or raw hit rows. Capture state changes consume receipts only; map entries also project visits.

## Integration

- Call `start_writer(path, config)` on the background runtime thread. Give
  live, demo, and replay separate database paths. One writer owns one stream;
  shut it down before opening another stream against that database. Duplicate
  writers for the same canonical path are rejected within this process. The app
  must enforce single-process ownership of a namespace.
- Begin each new stream at sequence 1. A receipt-only `CaptureStateChanged` may precede `SessionUpdated`; publish sessions and
  characters before references to them. Reopening a stream resumes its durable
  head. Exact receipt retries are no-ops; changed payloads and gaps fail atomically.
- `try_submit` never waits for disk or a mutex. Every rejection returns the event.
  On `Full`, retain it and the rest of the current engine batch and pause capture
  consumption. Transient producer/worker contention can also return `Full`.
  Do not allocate more batches while waiting. Surface capture gaps separately.
- Defaults/maxima: 1,024 accepted events, 128 events per transaction, 250 ms timer,
  8 MiB serialized per event, 32 MiB total accepted serialized estimate. Configuration
  can reduce counts and set a timer from 1 to 60,000 ms. Serialization validates
  finite numbers, signed SQLite timestamp/sequence bounds, and stream identity.
- `StoreStatus.queued` includes in-flight work. `durable` advances only after a
  successful commit. An empty stream flush returns sequence 0 while status keeps
  `durable: None`. Flush uses an acceptance barrier; concurrent later submissions
  cannot starve it. A successful receipt covers at least that barrier.
- Disk/sequence/projection failures retain the ordered pending queue and reject
  new submissions. Display the safe status message and unsaved sequence range.
  Repeated flushes return the original storage error kind while the worker is alive.
  `retry()` retries that same queue; it never skips an event. Fix invalid input at
  its producer instead of repeatedly retrying a deterministic projection error.
- Finalize the engine, submit terminal events, flush, then explicitly `shutdown()`.
  Flush, retry, shutdown, and Drop may wait and belong off the UI thread.
  **Handle failed shutdown:** `StoreError::WorkerFailed { pending }` transfers every
  accepted, uncommitted event to the caller. Keep that vector until a restarted
  writer has durably accepted it. Non-consuming flush/retry errors leave ownership
  inside the existing writer. A panic is terminal for that worker; non-consuming
  requests return recovery copies in WorkerFailed.pending, and shutdown transfers
  its retained queue. A final acknowledgment is checked again after timeout if
  the worker has finished, so successful shutdown cannot lose a racing receipt.
- Dropping without explicit shutdown attempts a flush. If it fails, an error is
  logged and pending work is retained for process lifetime. `take_abandoned_events()`
  retrieves this exceptional recovery data; callers must not use Drop as normal
  error handling. Repeated abandoned failures consume memory until retrieved.

Acceptance is volatile: a process crash can lose every accepted but uncommitted
event within the configured bounds. A flush receipt, not successful submission,
acknowledges durability. WAL with synchronous FULL and a 250 ms busy timeout protects
committed summaries. Passive checkpoints run after explicit flush/shutdown and do
not delete history; active readers may keep committed pages in the WAL.

## Schema and recovery

Migrations 001–019 are transactional, with their SHA-256 script checksums and `user_version`
written together. Unknown existing databases, changed checksums, and future schema
versions are rejected. Foreign keys are enabled on all connections. Readers open
read-only and query-only, with the same bounded busy timeout, and cannot move across
threads. Chat messages and chat alerts have the bounded retention described below; other summary history has no age-based purge.

Migration 015 is index-only: it adds indexes for loot statistics and dry-streak
gap reads. It does not rewrite saved fight JSON; fights recorded before the
party bucket fields were introduced still have no party-average line.

Writer startup closes interrupted sessions/runs/fights through deterministic new
recovery streams. Interrupted runs retain their completion/death facts; an otherwise
unresolved outcome becomes Unknown with AppClosed evidence and partial coverage.
Recovery emits no new death event. Already closed historical rows remain unchanged.
Recovering an interrupted run never shortens its session's already recorded end.
An open reader alone never performs recovery or migration.

Migration 016 adds empty maintenance/backup metadata and binary receipt tables.
It does not scan or copy legacy receipts, build an index over them, back up history,
delete detail, or run VACUUM. Exact v12/v14/v15 histories can start capture without
requesting maintenance. v12 still pays the existing 013/014 upgrade cost; v12/v14
also build main’s shipped 015 loot indexes. All three versions still run
interrupted-session recovery. Installed v1–v10 histories
require the explicit stopped **Back up and upgrade** action before either native
entry point opens history. Unknown or insufficient destination space and cancellation
leave that upgrade pending and capture blocked; there is no backup bypass.

New receipts store the same canonical SHA-256 digest as 32 bytes, keyed by an
interned stream and sequence in a WITHOUT ROWID table. During conversion retries
check both formats and reject conflicts; copy, verification, legacy deletion and
cursor advancement commit together. Settings explicitly requests conversion and its
verified Online Backup API backup. Restart can resume an unpaused requested job;
cancellation requires explicit resume. Every receipt remains an idempotency proof,
and all fight detail, players, sources and history readers remain authoritative.

The first destructive operation retains an initial verified backup; later classes
or renewals use at most one rolling slot. A failed replacement preserves the previous
ready image. Backups include committed WAL and have private manifests; database
backups do not include settings and post-016 backups are not exact v12/v14/v15 downgrade
images. Failed jobs never automatically rewind the live database. See
[storage maintenance](../development/storage-maintenance.md) for stopped restore,
space accounting, measured synthetic costs and pending owner acceptance.

Character aliases rewrite run/fame/death identity references and the entry snapshot
ID while retaining historical name, gear, and stats. Merges also re-key stored player
profiles, gear logs, and timelines. Combat player IDs are independent;
no player is joined by name. Run revisions replace child rows atomically. Drop appearances are retained by
`(drop_id, slot, item_type)`; revisions cannot erase them. `taken_observed_at`
records the first observed removal/replacement and remains on reappearance.
It establishes neither ownership nor who took the item. Legacy erased items cannot
be recovered.
Assigning or changing a run's character outside an alias merge recaptures that
character's entry snapshot; later updates for the same character keep it frozen.
Completion timeline IDs are stable facts, so stronger evidence revises one entry.

## Query behavior

- `list_runs`: 1-100 rows (caller default 30), exact dungeon/outcome filters,
  half-open timestamp range, stable `(sort value, id)` cursors. Newest and descending
  duration break ties by descending ID; other sorts use ascending ID. Visit duration
  is `ended - started`; missing/regressed durations sort last. Reuse cursors only with
  the same filter and sort. Missing IDs return None; malformed queries return a safe
  InvalidEvent error.
- `get_run`: one read transaction, fixed queries for the frozen entry snapshot,
  lightweight fight summaries, scalar run totals, drops, timeline, and comparison.
  Full fight/player drill-down is loaded separately.
  Each story collection is limited to 10,000 rows, and JSON collections/pages to
  32 MiB of serialized data; excess is an explicit error,
  never a silently truncated story. No per-player or per-source query loop exists.
- `get_player_breakdown(fight, object_id)` returns None if multiple generations reuse
  the object ID. Use `get_player_by_id(fight, player_id)` for reliable drilldown.
- `home_summary`: one transaction; counts run starts in the scope and counts actual
  death events independently, including deaths after clears. Best drop requires a
  recorded rank, with newest/ID ties. Top fight requires exactly one local row and
  recorded DPS with the current player-active-window basis. Top Active DPS requires
  complete, ended, issue-free fights with at least 20 seconds between the fight's
  first and last hit; this is not the local player's active-window duration.
- Today boundaries are supplied by the caller using local midnight/DST, never an
  assumed 24-hour window. Runs, fights, drops, fame and deaths use explicit session
  IDs in Session scope. Fame and runless deaths inherit the most recent SessionUpdated
  for their stream, retained across writer reopen; run deaths inherit the run's session.
  Clock overlap/regression and corrected session endpoints do not change ownership.
  Migration 003 assigns legacy run deaths from their run's explicit session first.
  Legacy fame/runless deaths use unambiguous interval matches: closed ends are
  inclusive; open sessions end exclusively at the next distinct session start, or
  remain unbounded when none follows. Saved last_traffic never truncates an open
  session. Equal-start/overlapping candidates remain NULL rather than guessed.
  Indexed range joins avoid scanning every session for each observation.
  Session coverage issues are retained across updates and included in Home, even if
  no runs exist. Today includes coverage from sessions whose observed intervals overlap.
- Fame charts keep every character segment's first/last point. Remaining interior
  samples share `max(0, 120 - mandatory endpoints)` proportionally within segments.
  If mandatory endpoints exceed 512, discard oldest whole segments until the newest
  remaining segments fit. Points remain chronological; `fame_sampled` reports omitted
  samples. Delta still uses all scoped samples, never the sampled chart. Each
  character's baseline is its first scoped sample, so fame earned while Maxed was not
  capturing, or before Today's supplied midnight, is not credited to the scope. The
  result is None when there are no scoped samples or the sum cannot fit i32.
  `fame_characters` gives each character's first and latest scoped sample, also
  unsampled, ordered by latest sample; the Home Fame tile reads it.
- Clear comparison requires at least three earlier same-dungeon Cleared runs and
  observed positive `completed_at - started` durations. Victory or dialogue supports
  observed timing; counter-only next-area fallback timing is excluded, even when
  its delay is zero. Missing current timing yields None.
- Bag counts match exact captured type in `[start,end)`. Catalog grouping of boosted
  and plain bags belongs to the caller.

Synthetic contract fixtures in `maxed_types::demo` provide example DTOs. The tests
exercise their storage round trip plus missing identity/content/rank, partial fights,
late outcome revisions, failure recovery, and 10,000-row indexed pagination.
Failure tests distinguish real SQLite busy/full/read-only errors from test-only
begin/commit boundary injection for full/read-only/busy/I/O and worker panic.
No owner recording, client asset, gameplay, or power-loss hardware test is involved.


## Migration inventory

| Version | Script | Purpose |
| --- | --- | --- |
| 001 | `001_initial.sql` | Initial summaries and receipts |
| 002 | `002_fight_summaries.sql` | Fight summary projections |
| 003 | `003_loot_and_sessions.sql` | Durable drop appearances and explicit session attribution |
| 004 | `004_alerts.sql` | Alert preferences, occurrences, delivery and personal-best records |
| 005 | `005_instance.sql` | Instance context, events and run totals |
| 006 | `006_summary_target_kinds.sql` | Target classification in summary queries |
| 007 | `007_pb_lookup_indexes.sql` | Indexed current-basis PB repair and reset of incompatible DPS baselines |
| 008 | `008_loot_browsing.sql` | Source-enemy projection and indexes for Loot history reads |
| 009 | `009_chat.sql` | Chat messages, FTS5 search and instance-event query indexes |
| 010 | `010_map_visits.sql` | Map visits and session-start query index |
| 011 | `011_exclude_junk_bags.sql` | Remove excluded junk-bag drops and related items, timeline entries and alerts |
| 012 | `012_event_fts.sql` | Event-name FTS5 index, synchronization triggers and backfill |
| 013 | `013_player_profiles.sql` | Scope player profiles, gear changes and damage timelines |
| 014 | `014_party.sql` | Party history and run members, backfilled from saved runs |
| 015 | `015_loot_stats_indexes.sql` | Index-only migration for loot statistics and dry-streak gaps |
| 016 | `016_maintenance_receipts.sql` | Additive maintenance jobs, backup metadata, stream registry and binary receipts |
| 017 | `017_accounts.sql` | Account membership, observation owners, identity spans and recovery progress |

The coordinator measured the migration 014 backfill at 0.30 s on a copy of a
412 MB real database with 89 runs, producing 1,383 players and 2,849 member rows.
This measurement applied the raw SQL for 013+014 with Python sqlite3, outside the
app's migrate path; it is not an app migration timing measurement.

Scripts live in [maxed-store/migrations](../../crates/maxed-store/migrations). Alert claims become visible only after commit; replay/demo contexts suppress external celebrations. Home keeps its Session/Today scoped loot queries; the Loot tab uses separate filtered and all-time history reads through the same history scheduler. These are observations, not an account inventory or proof of who took an item. See [personal bests](personal-bests.md) for revision and migration semantics.

Migration 008 adds nullable `drops.source_type`, backfilled only from valid JSON with an integer `source_enemy` in 0–65535. Malformed JSON, non-integers and out-of-range values leave the projection NULL. New and revised drops maintain the projection. Its indexes are:

- `drops_source(source_type, at, id)`
- `runs_dungeon_ended(dungeon, ended, id)`
- `drops_id_at(id, at)`
- `drops_stats(at, id, run_id, bag_rank, source_type)`

The last two are covering indexes for appearance counts and Stats scans. Their recorded cost is roughly 100 MB at 500,000 bags; see [Loot history read measurements](../release/performance.md#loot-history-reads). Installed migrations retain their checksums; future schema changes require a new migration.

## Loot history queries

`list_drops`, `get_drop_summary`, `get_item_history`, `get_loot_stats` and `get_collection` share the native history scheduler and existing store reader. Feed and item bag pages use `(at, id)` keyset cursors with 1–50 rows. Item history bounds enchant variants to 32, dungeon groups to 64 and source groups to 64. Stats bounds dungeon rows to 128 and source rows to 256; Collection bounds its grid to 256 and chase list to 100. Replies are bounded to 1 MiB; oversized replies and storage failures are errors, not empty successes.

Item totals count retained appearances independently of bag counts. Stats counts bags without multiplying them by item joins. Date ranges are half-open: bags use observation timestamps and denominators use ended runs of every outcome in the same dungeon. The all-time white streak remains independent of the selected range. Collection receives chase items and catalog tier metadata from the caller; the store does not load settings or infer loot tables. See [Loot interpretation](ui.md#loot) for rate and evidence rules.

`get_drop_summary` reads filtered totals and up to eight day blocks in one snapshot. Item totals are counted only when `include_items` is true (the legacy default) and the block matches at most `MAX_SUMMARY_ITEM_BAGS` (10,000) bags. `items_counted` distinguishes unavailable item totals from zero. Catalog tier inputs are bounded to 65,536 entries and 16 bytes per tier.

Item place groups expose optional `runs_since`: ended runs started after the latest observation in that dungeon. Stats tier counts and best-bag references expose optional `bag_type` for catalog labels and sprites. These additive fields remain optional for older replies.

## Chase settings

The chase list is local `settings.json` data in `AppSettings.loot.chase`, with at most 100 entries containing `item_type`, optional `dungeon` and `pinned_at`. `set_chase_list` patches settings; an ordinary Settings save preserves the current chase list so a stale form cannot overwrite pins. No chase history table is added.

Settings remain version 1. The `loot` and `party` fields use `#[serde(default)]`, so older settings load with an empty chase list and no Party presets. **Downgrading is not settings-compatible:** older builds reject unrecognized `loot`, `chat`, `party` and alert fields under `deny_unknown_fields`. `AppSettings.party` is always serialized, so any settings saved by this build are rejected by older builds, even with no presets.

## Chat storage and retention

Migration 009 adds `chat_messages` and the external-content FTS5 `chat_fts`
index over sender and text, maintained by triggers. Sender matching uses SQLite
`NOCASE` (ASCII case-insensitive), matching Rust hide-list behavior. Text is
limited to 1,024 UTF-8 bytes at a character boundary, with `truncated: true` when
cut. Sender, counterpart and map are cut at UTF-8 boundaries to 256 bytes;
chat IDs use a zero-padded counter to preserve emission order for timestamp ties.
`list_chat` and `list_instance_events` use the shared history scheduler,
1–100-row pages and stable cursors. Search allows at most 128 characters and
512 bytes, without NUL.

Migration 012 adds `event_fts`: searchable names are copied from instance-event
JSON and kept in sync by insert/update/delete triggers; malformed JSON remains
stored but is not indexed. Event search uses token-prefix matching like chat:
`Vex` matches `Vexlin`, while `xlin` does not; punctuation-only input applies no text filter.
Both `chat_fts` and `event_fts` key on SQLite rowids: a `VACUUM` that renumbers rowids requires dropping and backfilling both indexes; production never runs `VACUUM`.

Retention defaults to 90 days (1–3,650). Only the live namespace is pruned, in
1,000-row batches; demo and replay are untouched. Startup pruning uses only the
saved retention setting and is skipped if settings are unreadable. Startup stops
after 20 batches; live maintenance finishes any remainder. Hourly retention during
capture processes at most two prune batches per drain or maintenance tick.
Whisper, Guild message and Party message alerts and their non-pending occurrences
(including suppressed occurrences without alerts) follow chat retention. Each
table’s deletes are bounded per batch, and any full batch keeps the pass running.

`clear_chat_history` deletes live chat in 5,000-row batches, through the writer
while capture runs or directly under the namespace lease while stopped. It also
clears eligible chat alerts, but retains alerts whose delivery is still pending
until a later prune. The 250 ms capture tick services maintenance even before
the first frame anchors the capture clock. It leaves runs, instance events and other alerts intact.
Deleting rows does not shrink the SQLite file; freed pages are reusable.

Settings remain version 1. `chat` and the three alert fields (`whisper`,
`guild_message`, `party_message`) use `#[serde(default)]` for older input files.
An older build with `deny_unknown_fields` rejects settings saved with these
fields, just as it does with `loot` and `party`; downgrading is not settings-compatible.

## Map visits and Stats

Migration 010 adds `map_visits`: visit ID, stream, session, character and optional run
IDs, map name and kind, optional realm instance key, start/end timestamps, source
(`live` or `backfill`) and the JSON view. Ended runs are backfilled as dungeon
visits; runs still open at upgrade are skipped, and earlier realm/hub visits
cannot be reconstructed.

`MapEntered` closes the stream's open visit and opens one for its current session.
It inherits the preceding visit's character within that session; character updates
replace that identity and dungeon run starts attach the run ID. Session end and
startup recovery close open visits, including sessions without runs. Character
alias merges also update visits. Play resumed after an idle session ends waits
for the next map entry.

Each visit's fame window begins at its start, stops before the next visit's start
in that session, and includes its own end when that comes first. The baseline is
the character's last sample in the same session strictly before the visit;
without it, the first sample in the window is the baseline. Fame earned between
sessions is never credited. The final visit includes a fame sample at session end.
Session fame uses Home's rule, so a session's visits add up to Home's session fame;
missing map coverage is shown as Unrecorded in session detail. Backfilled runs
cannot absorb later realm fame.
Fame/hour is unavailable below five minutes of recorded time.

Stats uses the shared history reader: Maps and Dungeons return at most 200 rows,
Sessions pages contain 1–100 rows, and session detail/export includes at most
500 runs. Session CSV export is capped at 10,000 rows. Exports are local saves
through the native dialog; paths, player names and CSV contents are not logged.

## Player observations

Migration 013 stores `player_profiles` by `(scope_id, player_id)` with indexed
`(run_id, player_id)` lookup. The optional run ID is plain text: a missing run row
must not reject an observation or its transaction. Profiles retain the latest
observed JSON plus object ID, local-player flag and first/last-seen timestamps.

`PlayerObserved` upserts the profile and records starting gear or an equipment
change in `player_gear_log` at `last_seen`. Each scope/player log retains at most
64 rows: the first 63 changes and the latest gear. Equipment JSON is a list of
`EquipmentView` values.

`ScopeTimelineUpdated` writes bucket bounds, party totals/active counts, and
compacted/disabled flags to `scope_timelines`; `player_timelines` stores object ID,
local-player flag and points per scope/player. Partial updates upsert only supplied
players; omitted rows keep their existing points, even if shorter than the updated
bucket list. Full updates replace every player row for that scope. A disabled
update deletes the scope's player rows and stores empty buckets with the disabled
flag. Character merges use `UPDATE OR REPLACE` to re-key all three player tables.

## Clear party history

Clear party history is available in Settings even while the Party tab is hidden,
and in Party › Players when enabled. It deletes `party_run_members` and
`party_players` history, keeping marks and notes unless **Also clear marks and
notes** is ticked. When marks and notes are kept, each marked or noted player's
`party_players` row keeps their name and first and last seen times. Only the last
profile, scope and map are removed from that row. It also deletes other players' `player_profiles`,
`player_gear_log` and `player_timelines`. It never deletes local player data,
runs, fights, chat or events.

## Account ownership (017)

Migration 017 adds nullable indexed `characters.account_id`, known `accounts`,
a singleton `current_account`, first-event `observation_characters`, stream/session
`identity_spans`, and per-account `account_recovery` progress. No legacy JSON is
rewritten and existing characters remain unknown. Account IDs are HMAC pseudonyms;
identity keys never enter the store. Account readers require explicit ownership.

Reader contract: `character_accounts(id,canonical_id,account_id)` resolves flattened
aliases; `run_accounts(id,account_id)` follows the run's character. `fame_accounts`
and `death_accounts` key by `(stream_id,sequence)`. Fame follows its character;
death uses its explicit character, otherwise its run; differing known accounts
between explicit character and run yield NULL. An unknown explicit character
never falls back to the run. `fight_accounts` and `drop_accounts` use the immutable
first-event owner, otherwise the linked run; an unresolved owner stays unknown.
Drop items inherit `drop_accounts` through `drop_id`. Never join ownership by name,
character number, session, or the current-account pointer.

Elapsed time uses half-open identity spans, intersected with the requested time
range and the recorded session interval (ended, otherwise last_traffic). Clamp
negative durations to zero; unknown spans contribute no account time. A loss closes
the previous span and opens an unknown span. Provisional spans become attributable
when their character merges; merges flatten aliases and rewrite span/observation
owners. Session end closes that session's currently open spans. The engine then
emits loss, opening an unknown span in the ended session; it stays open until the
next session or identity event on that stream. Readers clip it to session bounds,
and unknown spans contribute nothing. Never extend an old stream to wall time.
`current_account` is the last verified acquisition, retained across loss, provisional
acquisition, and reopen. Character updates and legacy recovery do not change it.
Conflicting character reassignment keeps the original membership. A conflicting
verified acquisition opens an unknown span (both identity fields NULL), keeps the
current pointer unchanged, and does not fail the writer. Cross-account merge still
fails atomically. A cross-account merge can only come from corrupt data and fails
the batch into the writer's existing failed state. Stale character snapshots may fill NULL membership without
replacing newer snapshot fields.

`accounts::attach_legacy_characters` accepts batches of exact candidate IDs and
returns unique attached/already-attached/conflicting/unmatched counts. It only fills
NULL membership in one immediate transaction. Use `StoreWriter::attach_legacy_characters` and
`StoreWriter::record_recovery_progress` during capture; both flush accepted events
before executing on the writer connection. Direct connection APIs are for the
single writer connection or stopped capture. Recovery progress is an inclusive completed number prefix;
record it after attaching each batch, with 65535 marking completion. Retry after a
crash is safe. Native recovery computes candidates for 0..=65535; storage never
receives the identity key. Unattributed history remains stored, excluded from
account totals, with an attribution notice supplied by later readers/UI. A sole
known account is never evidence of ownership.

HomeSummary.unattributed_history reports database-wide unknown run/fight/drop ownership; Home shows one notice for excluded totals, records and bests, while dungeon_bests retains its account-scoped vector response.

Migration 018 adds `runs_character(character_id,id)` for indexed account-owned and unknown-run lookups.

Migration 019 maintains a small `unresolved_observations` marker through insert/update/delete triggers for observations with missing owner references (including runless observations without an owner row); the Home notice combines this with indexed unknown-character seeks, so fully attributed history does not require a history scan.

Selected visit stories retain their own period and unknown-owner observations, excluding only different known owners; account records and period/chain merging still require known matching ownership.
