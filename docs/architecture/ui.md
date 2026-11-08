# UI data ownership

`App.svelte` creates the native client. The persistent Shell owns one ordered live subscription, navigation, the character rail and alert outlet. One Home live model retains event-sourced visit membership, loot and fight timelines for the session, including while another route is active or the window is hidden. Only its display updates are gated to visible Home; returning publishes the current retained window. Routes consume that connection; they do not construct capture listeners, decode packets or open SQLite. Generated DTOs in `app/ui/src/lib/bindings` come from `maxed-types`.

The Shell keeps up to 32 saved-history entries in memory only, never persisted. Revisiting a clean entry within the 5-minute max age renders its rows without another read; dirty or older entries show their cached rows while refreshing. A hit written under a different route refreshes, and a resync marks entries dirty. Accepted events dirty entries whose stored routes match the history invalidation predicates. Supplementary queries (chat overview, sender, whisper partners, event summaries, run stories) are also dirtied by events matching their own scope. Settings, asset, demo and chat mutations and catalog or tier changes invalidate entries. Connecting, stream changes and client or preview changes clear the cache. Home and Alerts loaders are excluded, as are RunStory, Fight and PlayerBreakdown, whose reads also populate component-local supplementary data.

| Surface | Data owner | Limits and interpretation |
| --- | --- | --- |
| Rail | Shared live window, character snapshot and catalog visual lookup | Verified local identity; shared boss/quest focus; engine timestamps; no inferred rank or stats |
| Realm Home | Live instance context, retained visit window, fights, bags and notices | Realm score/cap and population evidence; kill labels preserve confirmation/provisional state |
| Dungeon Home | Live fight focus and engine run totals | Boss-tier encounters drive tabs/timeline; totals include all run fights |
| Nexus Home | Durable Home summary and scoped loot queries | Session/Today scope, fame samples, recent runs and paginated observed items |
| Runs | Durable run/fight/player queries | Historical snapshots and attribution; live events invalidate after durable watermarks |
| Loot Feed (`#/loot`) | Durable paginated bag observations | Filters preserve partial contents and source uncertainty |
| Item history (`#/loot/item/<type>`) | Durable item appearances and bag pages | Totals, enchant variants, dungeon/source breakdowns and chase pins |
| Loot Stats (`#/loot/stats`) | Durable bag and ended-run aggregates | Observed per-run rates; all-time runs since last white |
| Collection (`#/loot/collection`) | Chase settings, durable observations and catalog tiers | Pinned items and seen UT/ST items from the selected dungeon |
| Logs | Durable chat, instance events and alert history | Feature-gated; channel/kind filters, bounded pages and retained uncertainty |
| Alerts | Stored occurrences and native delivery status | Legacy route while Logs is disabled; preferences in Settings when enabled |
| Party | Native current-scope roster, durable player/run history and local requirement presets | Behind `party`, on in production; unknown gear and contribution stay explicit |
| Settings/Setup | Guarded native queries and commands | Capture, asset extraction, recording, acceleration and manual updates |

The live window retains at most 64 fights, 128 bags and 256 events. Same-instance realm continuity retains up to eight suspended visits and 128 entry periods. These bounds are presentation limits, not all-time totals: early kills can leave the retained window during long busy visits. Boss focus uses shared classification, quest evidence and local-hit recency; ordinary rows do not define dungeon boss tabs.

Native window visibility and `document.visibilitychange` jointly pause snapshots,
storage polling, history refresh timers, serial Home reads, Party refreshes,
recording status polling and the rail display clock. Pending history checkpoints remain
bounded and resume when visible; stale in-flight live queries cannot overwrite
the restored baseline. Domain events continue through native delivery and the UI
while hidden, preserving Home history and pending event-driven refreshes. Restore
rebases numeric snapshots without resetting the live window; a real sequence gap
still marks observations incomplete. Skipped recording reads coalesce into one
refresh on restore, including from disabled or failed recording states.
Disposing an owner releases its visibility subscription and any suspended reads.
These synthetic lifecycle checks do not verify WebView2
minimize/restore or tray behavior on an installed build.

Capture, snapshot and storage status are independent. Live capture does not imply history is saved. History queries wait for the required durable sequence; stale or failed history keeps an explicit state. Query generations reject late results after namespace/route changes. Visuals are keyed by the client fingerprint; sprites use local catalog references and labeled fallbacks when unavailable. No client art ships in the browser preview.

The rail and main scroller remain mounted across routes. Navigation restores route state, filters, anchors and scroll positions; live updates do not move focus. Native history owns physical mouse side buttons unless release testing establishes a need for the explicit fallback.

Current fame comes from fresh stat 57 samples. Core values and boosts use typed stat IDs; equipment bonuses come from catalog metadata. Missing class caps, gear, conditions or exaltation remain unavailable rather than becoming zero. Per-player DPS is unavailable (null) when that player's active window is shorter than three seconds. Run DPS uses its run-wide window without this cutoff. Stat-80 content is not treated as a fame goal.

[Debug notes](../../app/ui/src/lib/debug/README.md) keep technical caveats behind `maxed.debug` without changing the underlying evidence. The dev-only [preview harness](../development/ui-preview.md) mounts the real shell with synthetic data. jsdom tests establish content and state, while browser/WebView testing establishes geometry, keyboard behavior and DPI handling.

## Home redesign phases 1 and 2

`homeRedesign` selects the v4 Home in `routes/Home.svelte`; it is on in
production. Phase 2 ships under this same flag, with no new flag or migration.
The flag remains for rollback; with it off, the v3 Home remains
available. Development previews select v3 or v4 explicitly per mode.
The dungeon screen adds a live DPS hero with party and usual comparisons, a
learned Bosses card, a dry streak and a top-six damage table that also keeps the
local player's actual rank. The realm screen adds the same comparisons, realm
score and a visit events log.
Nexus retains the existing Session/Today content for this phase.

The session HUD sits above every v4 screen, including the recap: fame/hour,
weighted combat comparison and average rank, white-bag appearances, UT/ST item
types, potions and the session clock. The existing fixture and native client
paths supply `homeHud`, `homeDungeonStreak`, `dungeonProfile` and `homeBossBaselines`.
Home requests boss baselines over the preceding 90 days in batches of at most
64 target types. This read returns local fight count, average/best DPS and
character-independent confirmed kill samples/median; full boss history remains
available for Progress. Profile learning follows cleared run starts, including
kills after the range ends; best DPS follows fight starts across all dungeons
and realm visits. Both reads use the shared history scheduler.
Home reads its all-time dry streak through `get_home_dungeon_streak` for the
current dungeon, on dungeon entry (including another run of the same
dungeon), run end or a white-membership change. White membership tracks each
drop's white classification, run and timestamp; content/source enrichment and
session updates refresh the HUD without creating a new streak invalidation.
Realm and Nexus have no streak read.
The HUD and streak have independent revisions, pending work and errors, sharing
the native history scheduler and visibility pause. Streak values belong to a
stream/session/run generation; old replies are rejected, and the first white
consumes the cached value so another run cannot celebrate it again.
Only whites in the current run set the staleness check. After such a white,
a streak reply whose `last_white_at` is absent or predates that
white is not accepted; the read retries after the normal 500 ms debounce, at
most three times per white. A satisfying reply clears the check. Whites from
other runs still trigger reads. Run end also arms one follow-up streak read on
the next HUD-triggered refresh. Reclassification falls back to the run's newest
remaining white with a fresh retry budget, preserving any pending retry. A dungeon
or generation change, reclassification leaving no current-run whites, or disposal
cancels pending stale-white retries; HUD scope changes do not.
Run end always clears the profile cache, but an ended run does not request a
profile; the next open run reads a fresh profile for its dungeon.
History reads refresh on relevant engine events with debouncing, never per packet.
Unknown values remain “—”; failed v4 reads show “—” without error copy, leave
live data available and retry on the next refresh (engine invalidation).

Shell owns the recap tracker, which follows ordered engine events and window
checkpoints even while another route is mounted or Maxed is hidden. It observes
the window before and after `map_entered`, `run_started`, `run_ended` and terminal
`run_updated`, before event-driven scope retirement, and at batch end.
Non-event entry points (snapshot, state, capture, recovery expiry and disposal)
skip no-op calls and checkpoint only pending changes on entry, before their own
mutations. Nested resets stay quiet, so a snapshot-first departure cannot pair the partially
retired realm window with the new Nexus snapshot. The final publication observes
the resulting window.
Fresh snapshots for the same area refine fields available only in snapshots.
An ended run or realm visit becomes the latest in-memory recap for this stream.
Home keeps its current live screen and shows a compact recap-ready banner. The
banner lasts 15 seconds of mounted, presentation-visible Home time, pausing on
hover or keyboard focus. Dismissal hides only the banner. Dungeon/realm entry
hides the banner and closes an open recap so the live screen wins, retaining
Last recap. A realm visit ending directly into a dungeon (including Oryx's
Castle) or another realm becomes Last recap without showing a banner.
A Last recap button above the live content remains available in Nexus, dungeon
and realm. View recap or Last recap opens the full recap; Back returns to the
current live area without forgetting it. A newer finished area replaces the
recap; a stream reset clears it. Banner removal returns focus to Last recap only
when focus was inside the banner. These controls apply only with `homeRedesign`.

Home's `createRecapReads` controller gates saved reads on the recap's fixed
watermark and matching stream in `StoreStatus.durable`. Runs read `getRun`,
`getRunRecap`, `getDungeonProfile`, `getHomeBossBaselines` and
`getHomeDungeonStreak`; realm visits read `getVisitRecap` and boss baselines.
Usual comparisons use the 90 days preceding the run or visit. Store updates
wake the gate; snapshots do not repeatedly query recap history. Generations
reject late replies after replacement or disposal. Closing the recap retains its
source and watermark. The existing visibility pause and history scheduler apply to these reads.

While waiting, `RecapPlaceholder` keeps retained observations and says
“Saving run…” or “Saving visit…”. After five seconds it marks them Partial; a recoverable failure
or timeout offers Retry. An unavailable visit remains Partial without Retry.
Missing optional comparisons do not prevent the saved story from rendering;
unknowns stay “—” and truncated fight, event or visit loot history stays labelled.
Failed record reads show “Records unavailable” with Retry. A saved view is only
displayed for the matching run or visit; merged visits can match any retained period.

`RecapScreen` shows the saved dungeon banner, five tiles, boss fight rows with
party sparklines and record chips, final loot and the top-three party line.
A dungeon death uses `DeathBanner`, with the catalog-resolved death-message
killer, character and fame, and observed HP over the final five seconds, with
gaps for missing readings. `VisitRecapScreen` shows visit tiles and event rows;
announced-only events are faded and have no DPS. Inferred chains retain their
label. Records and milestones are computed as of the run that earned them.
Journey, Progress and saved recap routes remain later work.

Both saved screens offer **Copy recap image**. An explicit click draws a
dedicated canvas card and writes a local PNG; it does not capture the DOM or
upload anything. Names, loot and fights default on and are saved in Settings.
Unknown values stay “—”; failed sprites fall back to names, and a failed write
says “Couldn't copy the image”. No raster work runs on mount or a live update.
The isolated [clipboard spike](../development/recap-clipboard-spike.md) verified
the WebView2 PNG write; real sprite copying, Discord paste and font rendering
still require owner acceptance. Synthetic checks do not establish live capture,
game-client or installer behavior, or read timing on the owner's database.

## Party

The `party` feature flag is on in production. When enabled, the rail opens
`#/party` with Live, Players and Presets sub-views. A Run story links to
`#/party/runs/:run`; player history deep links use `#/party/players/:key`.

Live queries the native current-scope roster. Its preset menu chooses requirements
until the next map change; automatic dungeon selection then takes precedence over
the default preset. The roster shows parse results and reasons, shared gear hover
cards, marks and prior-run counts. Dungeon rows also show damage shares and flags
alongside reasons. Out-of-view players remain dimmed. Left early appears on saved
runs, because departures become known at run end. Missing names, stats, tiers and
older runs' gear remain visibly unknown.

Live subscribes to `party-changed`, including reset and new-scope notices, and
coalesces reads to at most once per second with a trailing read. Engine-event
batches do not refresh Live. Saved-run views refresh on matching `run_ended` and
`fight_started`, `fight_updated` or `fight_ended` envelopes. Players and player
detail views refresh on `run_ended`. Saved reads wait for the event's durable store
watermark; late responses after route changes are discarded. Mark and clear
actions refresh the affected views. Catalog invalidation clears old names and
sprites immediately; ordinary type additions retain resolved visuals while
loading the missing types.

Players searches local name history with a 250 ms debounce, filters marks/notes
and sorts bounded 100-row pages. The shared side panel saves notes on blur, offers
mutually exclusive Watch/Ban marks and links to saved runs. Player detail pages
load run history in pages of 20; Live shows the five most recent. Clear history
requires confirmation, with marks and notes preserved by default. User Close
returns focus to the roster row or page heading; background panel closure only
restores focus when it was inside the panel or on the document body.

Presets edits at most 32 named sets of maxed-stat and four-slot tier requirements,
with a UT/ST toggle, catalog dungeon auto-apply and one default. Saves use the
dedicated `update_party_settings` command; general settings saves cannot overwrite
them. Copy as text makes an announcement, not an import format. Name, whisper and
failing-name actions use the clipboard; Open on RealmEye calls the native browser
handoff only after the user clicks it.

## Item hover card

The Shell mounts one `ItemHoverCard`; item surfaces opt in with the `itemHover` action,
passing their item type, fallback name/tier and available enchant evidence. Existing links
retain keyboard focus behavior; pointer-only tiles gain no tab stops. The card opens after
250 ms on hover, immediately on keyboard focus (`:focus-visible`), and immediately during the 300 ms warm-swap window.
The item-search keyboard-active option uses the same delay, with immediate swaps while open or warm.
It closes on leave, element blur, window blur or Escape; window blur cancels pending opens.
Search closes its card when results clear, the input blurs or the component is destroyed.
Shared `placement` logic keeps it within the viewport
and horizontal scroll boundaries and repositions it on scroll or resize.

`get_item_details` serves the card: it reads the local catalog on pointer entry, keyboard focus or search selection,
with results cached per client fingerprint. The item-details cache is cleared when the UI
changes asset settings (demo, assets path or settings) and replaced when a lookup returns
a new client fingerprint. Fully cached hovers make no native call. Failed lookups are
retried on the next hover. Live numeric updates do not request details.

The design wording (private design notes)
defines these enchant states:

| Context | Card content |
| --- | --- |
| Omitted (catalog-level item) or `recorded_empty` | No enchant section |
| `recorded` | Each enchant's name and description |
| Recorded ID absent from catalog | `Enchant #4127` and `not in catalog` |
| Empty or unusable description | Name only |
| `missing` or null evidence | `Enchants not observed` |
| `invalid` | `Enchant data unreadable` |
| Lookup failure | `Details unavailable`, with the surface's name and tier |

Collection grid and chase-list item links use catalog-level cards without enchant evidence.

Scope loot adds the formatted count and `Best copy` caption. Observed removals in Loot
add `Removal observed`; they do not establish who took the item.

## Loot

Feed, Item history, Stats and Collection use the single history scheduler through `list_drops`, `get_drop_summary`, `get_item_history`, `get_loot_stats` and `get_collection`. `search_items` searches the active local catalog outside that scheduler; `set_chase_list` updates [chase settings](storage.md#chase-settings). Feed and item bag pages contain at most 50 rows, newest first by `(at, id)`. The legacy `#/loot/<type>` address also resolves to Item history.

Item counts are retained `(bag, slot, item_type)` appearances, never pickups, possessions or drop chances. A taken marker records observed removal without identifying who took the item. Unknown contents, observed-empty bags, partial contents, unknown and estimated sources, and the four enchant states (`missing`, `invalid`, `recorded_empty`, `recorded`) remain distinct. White bags have `bag_rank = 255`; item tier comes only from the catalog, never from a name or bag rank. Missing catalog entries keep a labeled fallback.

Stats shows the white rate (observed whites divided by ended runs, as a percentage) for the same dungeon in the selected range, including every outcome. Multiple whites per run can make the rate exceed 100%. Bag timestamps select the numerator; run end timestamps select the denominator. Bags from open runs can count before those runs enter the denominator. Clears are shown separately; a zero denominator has no rate. Sources outside a run show counts only. Runs since last white is all-time, counting ended runs that started after that dungeon's latest observed white, even when Stats has a date range. No observed white leaves that value unavailable.

The Stats caption is:

> Counts the bags Maxed saw while capturing. Not a drop rate: missed captures and bags out of view aren't counted.

Collection combines the chase list with items Maxed has seen. The per-dungeon grid shows only observed UT/ST items with catalog tier metadata, up to 256 entries. Maxed bundles no loot tables; this is neither a list of possible drops nor a complete inventory. Chase dry streaks count ended runs started after the last matching observation, or after pinning if never seen, scoped to the pin's dungeon when supplied.

Bag observations invalidate Feed, Stats, Collection and affected item pages; ended runs invalidate all four views, including the Feed white rate and Item dry streak. Reads wait for durable storage; numeric live ticks do not trigger them. Errors stay errors rather than becoming empty history. Real-play updates across all four views still need [Windows live acceptance](../known-limitations.md#implemented-behavior-awaiting-live-acceptance).

## Logs

`productionFeatures.logs` is `true`. With the flag on, Logs replaces Alerts in
the rail and `#/alerts` redirects to `#/logs/alerts`. With it off, legacy Alerts
routing stays available. Preview fixtures enable Logs
and supply synthetic messages and events without modifying the demo recording.

| Route | View |
| --- | --- |
| `#/logs` | Chat › All, including public and unknown channels |
| `#/logs/chat/{guild,party,whisper}` | Channel chat |
| `#/logs/run/{runId}` | Chat during a run |
| `#/logs/chat/{guild,party,whisper}/run/{runId}` | Channel chat during a run |
| `#/logs/from/{sender}` | Chat seeded with a sender for this entry |
| `#/logs/chat/whisper/{partner}` | Whisper conversation |
| `#/logs/keys` | Key pops |
| `#/logs/events` | Instance events |
| `#/logs/events/run/{runId}` | Instance events during a run |
| `#/logs/alerts` | Alert history |

Search, sender and visibility filters live in navigation entry state, not URL
query strings. Back/Forward restores filters, selections and paging. Key pops,
Events and Alerts share one range in `logsPosition`; chat, whisper, keys and
events retain separate filter slots. New entries carry those filters but clear
selection and paging. Sender seeds do not carry to another entry. Alerts filters
remain per-entry; only the shared range carries. NPC and hidden lines are
optional; provisional events retain an unconfirmed label. Chat and event reads
wait for durable storage. Matching channel/run chat events invalidate Chat;
matching instance-event kinds invalidate Key pops and Events. Stream changes
reject stale query results.

On the first page at the top, new messages refresh automatically. While reading
older rows or scrolled down, a violet new-data pill returns to the latest rows
without moving the current reading position until clicked. Pills require matching
accepted envelopes; unrelated checkpoints stay deferred silently. Counts exclude
NPC lines when disabled, and numbers are omitted when hide lists, search or other
filters make the count uncertain. Events and Key pops omit numbers for a range.
Whispers refresh People only after matching whisper envelopes; People and thread
use separate throttled controllers. Chat serializes page and supplementary reads. Alert preferences
move to Settings when Logs is enabled; Logs › Alerts keeps the history log.
Settings › Chat is available with either flag state, including storage toggles,
retention, hide lists and Clear chat history. Saved hide lists apply to cold
history reads before capture starts. Keyword and sender additions enforce UTF-8
byte limits (64 and 32 respectively) and reject control characters inline.
Chat invalidates alert history only for non-local player guild/party messages
and incoming whispers.
Channel rules and live updates still await Windows acceptance.

## Stats

`productionFeatures.stats` is `true`. The preview enables it with synthetic
data. When enabled, Home's By dungeon section links to all-time dungeon records.

| Route | View |
| --- | --- |
| `#/stats` | Maps, ranked by fame/hour with range and character filters |
| `#/stats/sessions` | Sessions, newest first, with optional empty sessions |
| `#/stats/sessions/{id}` | Session totals, maps, Unrecorded fame and linked runs |
| `#/stats/dungeons` | Lifetime dungeon entries, time, clears, deaths and clear times |

Filters, sort and paging belong to the navigation entry. The shared history
scheduler waits for durable storage; `map_entered`, `run_ended` and
`session_updated` invalidate Stats, while individual fame updates do not.
Missing baselines show “≥”; fame/hour below five minutes shows “—”. Session
detail offers party-run CSV export and links an in-progress session to Home.
CSV files are saved through the native dialog without passing their contents
through the WebView.

## Character

`#/character` is gated by `productionFeatures.character: true`, on in production
for owner use. The DEV-only `?preview=character` mounts the real Shell using
invented DTOs. The owner approved production enablement on 2026-10-20 before
Task 11 and, on 2026-11-06, accepted weapon/regen values, 100%/150% scaling and keyboard behavior from
Windows use. The recorded [Task 11 checklist](../development/character-live-acceptance.md#production-enablement)
remains unexecuted.

The Shell's existing ordered live subscription supplies the local `CharacterView`.
Observed identity, equipment, totals, raw boosts and coverage stay distinct from
modeled weapon/regen values. Base/gear/exalt/Other is a provisional breakdown;
Other is an unexplained remainder. The shared stat split gates cap/pots-left.
Missing or inconsistent evidence stays unavailable, including negative Other.

`get_character_calculation` receives an expected revision, defense and up to two
comparison item types. Native code pairs the local observation with its catalog
on the engine owner, calculates without holding lifecycle/view locks, and checks
its epoch again before replying. Revisions include stream, character identity,
relevant-input hash and catalog fingerprint. Metadata replacement, reconnect and
identity changes reject stale work; timestamps alone do not force recalculation.
The command adds no storage query, history entry, persistence or capture listener.
Catalog names/search use `get_object_visuals`, `get_item_details`, `search_items`
and the existing catalog-availability query.

The Shell-owned controller coalesces changes, permits one request in flight and
retains only the latest pending request. It validates reply revision, fingerprint
and applied defense. Identity changes clear comparisons/results. Capture stop,
resync, failure and death immediately freeze freshness; they cannot leave a Live
label on retained values. Controls survive route navigation within this Shell,
not app restart. Defense defaults to 0, accepts integers 0–10,000 and has Reset.
Invalid text preserves the last valid input; displayed results name their own
applied defense while a replacement is pending. Catalog comparisons keep equipped
first and use current observed stats without recalculating equipment bonuses.

The Rust `character-v1` model uses observed ATT/DEX and the primary condition mask.
Normal attack rate is `(1.5 + 6.5 × DEX / 75) × catalog rate`; the attack multiplier
is `(ATT + 25) × 0.02`, with Weak taking precedence over Damaging. Integer catalog
rolls use `[min, max)` or a fixed equal bound. Damage is truncated before each
projectile's defense/floor operation; armor piercing bypasses defense. Authored
check vector: ATT/DEX 75, mask 0, fixed 100, one projectile, rate 1, defense 50 gives
150 damage per projectile, 8 attacks/sec and 1,200 theoretical DPS. Calculations
never feed observed combat attribution, personal bests or Party requirements.

Weapon rows show independently supported groups/subtotals and unavailable full
totals. Formula details retain inputs, version, evidence, reasons and exclusions.
The model assumes sustained fire, every projectile hitting one target once. It
excludes exalt damage, Crucible, Blood Ritual, weapon enchant damage/rate changes,
special schedules/effects, firing restrictions, enemy conditions, misses, range,
repeat hits and downtime. Observed ATT/DEX boosts remain included.

Regeneration reports baseline contributions: `0.12 × WIS`, reviewed flat units/sec
and fractional-maximum/sec enchant effects. Missing maximums block fractional
effects independently of known flat contributions; recorded-empty and missing
enchants differ. Base VIT, base MP recovery, pets, consumables, ability costs,
combat-state changes and suppression/healing effects are excluded. Exclusions
remain visible without hover or debug. Exalt damage always reads unavailable,
deferred pending verification, with no multiplier value.

## Account changes

Home follows the current account without a selector. The privacy-safe
`account-history-changed` revision resets only account-dependent read owners:
summary, scoped loot, dungeon bests/PB baseline, HUD, streak, dungeon profile and
boss baselines. Disposal rejects late replies, including Superseded reads, before
new owners refetch. Unrelated visual and navigation state stays mounted.
Confirmed account switches clear the rail's retained character; map-change
loss/provisional identities retain the existing recovery policy.
`HomeSummary.unattributed_history` shows one short Home notice, dismissible for
the shell session. False or absent values show nothing.

Saved-history page controllers retry Busy or Superseded once after at least one
second, then expose manual retry while retaining saved values. A successful read
renews the automatic retry allowance. Home uses one serial queue across summary, loot, PB, v4 cards and recap commands. Its summary
and loot readers retry once after two seconds; supplemental readers allow two
two-second supersession retries (and one Busy retry for v4/recap). Exhaustion
remains explicit through the owning card's error/retry state; PB baselines retain
their last value. Recap SQL and its save timeout pause while hidden. Party's
saved-run view continues its throttled durability checks until the store catches
up. A failed store or changed stream stops polling and offers manual retry.
