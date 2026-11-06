# Native alert delivery

First real cues and explicit tests are due immediately. A playing voice or a
300 ms start-to-start cooldown retains one waiting real cue, replaced by the
newest. The worker is signalled on enqueue;
it polls every 20 ms only while pending work needs a playing voice to finish.
Demo/replay, retired streams, stale claims, Off and Quiet log only remain silent.
Mute and zero volume suppress audio without suppressing opted-in notifications.

The default volume is 70 for new settings. Stored values are preserved. Gain is
the original linear `percent / 100`: stored 35% remains 0.35, and 0% is muted.
Cue derivation, pinned source hashes and normalization parameters are in the
[sounds README](../../app/src-tauri/resources/sounds/README.md). The owner approved
the pre-chat bundled sounds by ear on 2026-10-05, including the white-bag transient
reduction and makeup gain.

## Windows identity and silence

The Tauri notification plugin is registered. Its cached 2.5.1 desktop source
selects the configured app ID only outside literal `target/debug` and
`target/release` paths; its background `show()` also discards delivery errors.
Maxed instead uses the same cached WinRT wrapper directly for delivery and the
WinRT notifier's `Setting()` for availability, independent of executable path.

At native sink creation, Maxed creates or updates only
`HKCU\Software\Classes\AppUserModelId\com.maxed.companion`, setting
`DisplayName` to `Maxed` and `ShowInSettings` to DWORD 1. The ID matches
`tauri.conf.json` and the NSIS app identity. This follows the cached
`tauri-winrt-notification` 0.8.1 `examples/unpackaged_app.rs` registration approach;
no Start-menu shortcut, machine-wide registration or COM activator is added.
No toast actions or application activation are requested. Registration is retained
across exits so Windows can retain the sender and its notification settings.
Removing that exact per-user key manually removes the portable registration;
the next Maxed launch recreates it. Existing unrelated registry values are untouched.

The wrapper's `sound(None)` creates `<audio silent="true"/>`. Titles and bodies
use fixed kind labels and explanations, except chat alerts (see below) and Key Popped,
which uses the canonical dungeon name resolved from the catalog. No opener
identity, raw notice text or paths enter a toast. Native errors are returned to
delivery status. The webview
has only `core:default` and no notification plugin permissions; all notification
queries and delivery go through guarded Maxed commands.

The delivery guard only prepares a notification task with a cheap shared handle.
WinRT permission refresh, window-focus queries and toast submission run after
releasing it. Native permission is cached atomically for `Delivery::status()`;
the public status refresh releases the guard before querying Windows. Retirement
and preference edits cancel prepared tasks via a generation counter, checked
again after blocking calls and before submission. A toast already submitted to
Windows cannot be recalled by this cancellation. Explicit audio-device recovery
and device teardown also run outside the guard; existing mixer playback/stop operations remain
synchronous so retirement can stop the active voice. A blocking fake notifier
tests that clock observation and cached status remain available during permission,
focus and toast calls, including a public status refresh.
During audio recovery, status retains the last known sound availability and
notification permission, and Test can replace the pending sample. Recovery
completion clears the snapshot and exposes the recovered sink's current status.

Real notifications require the saved toggle, visible/fresh alert, eligible live
capture and an unfocused window. The explicit Test notification uses the first
supported kind saved as Toast or Banner and permits a focused window. It still
requires live capture and an observed capture clock. Enable Windows
notifications, and Apply alerts before testing. Missing audio devices do not
disable notification tests. Windows Settings and Do not disturb can suppress
display even after the native API accepts a toast.

## Latency measurement and remaining validation

The writer marks the latest queued ticket containing a visible alert candidate
(white-bag drop/revision, eligible key pop, or ended fight) for immediate commit. Earlier
events retain their order and commit with it. Personal-best eligibility and prior
records are still resolved inside the transaction; an ended fight is only a
flush hint, never permission to deliver. Other batches retain the normal size
threshold and 250 ms interval. Off and Quiet log only do not request a fast flush.

After a successful transaction and durable-head update, the writer sends a
nonblocking permit through a capacity-one channel. The live join loop selects
this wake ahead of capture frames, with Stop retaining first priority. It drains
every durable claim page immediately without waiting for the 250 ms capture tick. The permit
coalesces commits, and the receiver reads the current durable head. Failed
transactions never signal or expose claimable alerts; explicit retry still owns
recovery. External sound/notification calls remain on the delivery worker.

`alert_commit_wakes_claim_and_enqueue_without_advancing_capture_tick` uses a
synthetic white-bag event, a real temporary SQLite writer, the normal claim path and
a fake sink on the delivery worker. Its capture clock stays frozen and no timer
permit is sent. Separate tests advance a fake elapsed batch clock from 0 to 249
to 250 ms: normal work waits, while an urgent ticket makes the batch immediately
ready. They also cover ordered tickets, normal work after the urgent ticket,
coalesced wakes, rollback and retry.

Synthetic scheduling tests do not measure packet-to-speaker latency.

The sink tests cover focus/permission/mute/volume/staleness, replay/demo guards,
immediate first cues, cooldown replacement, focused explicit tests, errors and
asset levels. Real speaker onset and toast display on portable
and NSIS builds remain unverified and require Windows live testing.

Death is a legacy history kind only. Its saved preference is accepted with an
Off default and ignored; no death event creates an alert or requests a prompt
commit. Death records, timeline entries and Home counts are unchanged.

## Key Popped

Key Popped defaults to Off for new or unset preferences and uses its own generated, pinned `key_popped.wav`: two
short descending notes at a softer level than Personal Best. The cue is MIT licensed;
its deterministic derivation and measurements are in the sounds README. The owner approved its audibility and distinction by ear on 2026-10-05. Flawless remains unavailable.

The engine accepts non-provisional PortalOpened notices only from live streams,
resolves the dungeon to its canonical catalog name, and compares the opener
against the current local entity identity without exporting it to alert data.
A case-insensitive match to the local identity suppresses an own pop. While the
local identity is unknown, an own pop can't be ruled out, so key pops stay
silent. This happens only when capture attaches to an already-open connection;
the next map change establishes the identity. Unresolved dungeons remain silent. The adapter also rejects ephemeral history/demo contexts; the existing
external-delivery guard still checks the originating live stream, capture state,
freshness and retirement before sound or Windows notification delivery.

The instance event id is the occurrence source key. Re-publishing an accepted
notice cannot create a second occurrence. The log and toast say, for example,
"Lost Halls key popped"; alert details contain only the dungeon. Ordinary portal
observations remain in instance history even when no alert is eligible.

Settings remain version 1: `key_popped` defaults to `off` when absent, and
a missing legacy `key_pop_dungeons` remains null (All dungeons), preserving explicit saved delivery choices. New preferences use an empty selection and prompt the user to choose dungeons. Explicitly saved loudness and filters are unchanged. Settings files that predate
the `key_popped` field and have never been saved since now load key-pop alerts as Off. A list selects canonical
catalog dungeon names; an empty list selects none. The Alerts picker loads only
dungeon names through `list_catalog_dungeons`, retains saved selections if assets
are temporarily unavailable, and persists edits with the existing alert settings
command. No destructive settings or history migration is needed.

Dungeon filters accept at most 256 names of at most 64 UTF-8 bytes each, leaving
room beyond the built-in catalog's 129 dungeon names (longest: 31 bytes). Escaped
filter JSON stays below 34 KiB. Before replacing saved settings, both write paths
reject a complete encoded document exceeding the 64 KiB read limit, including
its trailing newline, with the existing settings validation error. The previous
file remains unchanged and readable.

Accepted dungeon-filter changes remove excluded key pops from the visible banner
and waiting queues without replaying them if the filter is later widened. Native
claim, cue and notification delivery recheck the current selection using the same
preferences rule as occurrence creation. Explicit Test still previews its cue
independently of dungeon selection; other alert kinds are unaffected.

## Chat alerts

Whisper defaults to `toast`; Guild message and Party message default to
`quiet_log_only`. The `alertable` rule requires live capture and a known local
identity. Only other players' messages qualify: outgoing/unknown-direction
whispers, local messages, NPC lines, demo and replay never alert. Whispers must
be incoming. ASCII case-insensitive sender and keyword hide lists suppress alerts.
Channel storage toggles do not replace alert preferences.

In-app and Windows toasts include the sender and message excerpt, cut to 120
characters. Key Popped still never shows the opener. All three chat kinds share
`chat_message.wav` and the existing loudness/global-mute policy. The owner approved
the chat cue on 2026-11-06. Chat text and sender names never enter diagnostics.

Chat alerts follow the chat retention window (90 days by default, 1–3,650 days).
Clear chat history removes eligible chat alerts along with chat rows; an alert
with delivery still pending is retained until a later prune. Other alert kinds
are unaffected. Alert preferences live in Settings; Logs › Alerts is history only
when the Logs feature is enabled.
