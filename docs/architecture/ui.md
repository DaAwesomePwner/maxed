# UI data ownership

`App.svelte` creates the native client. The persistent Shell owns one ordered live subscription, the shared live window, navigation, the character rail and alert outlet. Routes consume that connection; they do not construct capture listeners, decode packets or open SQLite. Generated DTOs in `app/ui/src/lib/bindings` come from `maxed-types`.

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
| Settings/Setup | Guarded native queries and commands | Capture, asset extraction, recording, acceleration and manual updates |

The live window retains at most 64 fights, 128 bags and 256 events. Same-instance realm continuity retains up to eight suspended visits and 128 entry periods. These bounds are presentation limits, not all-time totals: early kills can leave the retained window during long busy visits. Boss focus uses shared classification, quest evidence and local-hit recency; ordinary rows do not define dungeon boss tabs.

Capture, snapshot and storage status are independent. Live capture does not imply history is saved. History queries wait for the required durable sequence; stale or failed history keeps an explicit state. Query generations reject late results after namespace/route changes. Visuals are keyed by the client fingerprint; sprites use local catalog references and labeled fallbacks when unavailable. No client art ships in the browser preview.

The rail and main scroller remain mounted across routes. Navigation restores route state, filters, anchors and scroll positions; live updates do not move focus. Native history owns physical mouse side buttons unless release testing establishes a need for the explicit fallback.

Current fame comes from fresh stat 57 samples. Core values and boosts use typed stat IDs; equipment bonuses come from catalog metadata. Missing class caps, gear, conditions or exaltation remain unavailable rather than becoming zero. Per-player DPS is unavailable (null) when that player's active window is shorter than three seconds. Run DPS uses its run-wide window without this cutoff. Stat-80 content is not treated as a fame goal.

[Debug notes](../../app/ui/src/lib/debug/README.md) keep technical caveats behind `maxed.debug` without changing the underlying evidence. The dev-only [preview harness](../development/ui-preview.md) mounts the real shell with synthetic data. jsdom tests establish content and state, while browser/WebView testing establishes geometry, keyboard behavior and DPI handling.

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
