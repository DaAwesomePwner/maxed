# Roadmap

This is the living plan for Maxed's tabs. Update the status table when a tab changes state, add owner decisions
to the decision log with their date, and move answered questions out of [Open decisions](#open-decisions).

Each tab gets a detailed spec just before it is built. Once the tab ships, its lasting content moves into the
[architecture docs](architecture/README.md) and [known limitations](known-limitations.md), and the spec is
retired. This roadmap keeps only scope, order, decisions and cross-tab rules.

## Goal

Grow Maxed from Home / Runs / Alerts / Settings into a full companion app with five more tabs. Most are inspired
by RealmShark's GUI (upstream `X-com/RealmShark`, branch `tomato`, v1.9.2), rebuilt for Maxed's local-first,
honest-evidence model. [Candidate features](#candidate-features) collects further ideas from RealmShark and
RealmHound that are not yet scheduled.

## Status

| Tab | Status | Feature flag | Notes |
| --- | --- | --- | --- |
| Home | Redesign phase 2 accepted by the owner (2026-11-06) | `homeRedesign`, on | Home keeps its live screen with a 15-second recap-ready banner and persistent Last recap. Journey and Progress are next |
| Runs | Shipped | — | Run story and fight drill-downs; run-level player view with gear, stats and DPS timelines |
| Alerts | Shipped | — | Now the **Logs › Alerts** sub-view; `#/alerts` redirects there |
| Settings | Shipped | — | Pinned at the bottom of the rail |
| **Loot** | Shipped (private-repo PR 87) | `lootBrowsing`, on | Feed, item pages, statistics, collection and chase list |
| **Logs** | Shipped (private-repo PR 89) | `logs`, on | Chat, key pops, events, alerts. Chat storage and Settings › Chat are active regardless of the flag |
| **Stats / Sessions** | Shipped (private-repo PR 94) | `stats`, on | Fame per hour by map, sessions, lifetime dungeon record, CSV export |
| **Character** | Implemented, accepted in owner use (2026-11-06) | `character`, on | Own-character calculator; Gate P resolved (2026-11-08), exalt damage deferred. See [live acceptance](development/character-live-acceptance.md) |
| **Party / Parse** | Implemented, in owner use | `party`, on | Live roster, saved-run Party, player history with marks/notes and requirement presets; migration 014. See [UI architecture](architecture/ui.md#party) and [preview](development/ui-preview.md#party-preview) |

Not in scope for these tabs: account-synced Daily Quests and Pets, Exalts / account sync (needs the login token and
network), theme and font menus. Observed-only quests and pets are in scope by owner decision (2026-11-06) and
await their own specs. Account sync is deferred, not ruled out; see
[Later: opt-in account sync](#later-opt-in-account-sync).

## Rail

Home · Runs · Loot · Logs · Stats · Character · Party, with Settings pinned at the bottom.

The rail's `section` mapping in `app/ui/src/lib/shell/CharacterRail.svelte` changes: `alerts` → Logs.
`productionFeatures.lootBrowsing` becomes `true` once the Loot tab is accepted. Each later tab ships behind its
own flag until it is accepted, following the `Features` pattern in `app/ui/src/lib/navigation/routes.ts`.

## Tabs

| # | Tab | Contents | Data today | New plumbing |
| --- | --- | --- | --- | --- |
| 1 | **Loot** | Feed, item pages, statistics, collection / chase list | Bags and item appearances are fully stored (`drops`, `drop_items` with `taken_observed_at`) | Queries, migration 008 (source-enemy column), item search, chase list in settings |
| 2 | **Logs** | Chat (All / Guild / Party / Whisper), key pops, events (deaths, departures, realm kills), alerts | TEXT is decoded but discarded. Key pops and events are stored in `instance_events` with no reader. Alerts exist | Chat event → store → retention path, a reader for `instance_events`, moving Alerts. Revises the "do not retain raw text" notice rule in [protocol evidence](parity/protocol-evidence.md) |
| 3 | **Stats / Sessions** | Fame per hour, session gain, fame per map, saved sessions with CSV export, lifetime per-dungeon stats (entries, time, clears, deaths) | `fame_samples`, `play_sessions` and `runs` are stored | Aggregate queries, CSV export (local save dialog) |
| 4 | **Character** | Your own character: theoretical DPS per weapon, rate of fire, damage ranges, regen, exalt multipliers | Own stats and equipment are live. Weapon and projectile data is in the game catalog | Catalog fields for projectiles and rate of fire, a calculator crate or module |
| 5 | **Party / Parse** | Players observed in view: gear, maxed stats, requirements, contribution flags and history | Native roster plus migration 014 name-key history; per-run gear from migration 013 | Implemented behind `party`; live Windows acceptance remains |

## Candidate features

Ideas from a 2026-10-06 review of RealmShark (`tomato` at `27d02b70`, newer than v1.9.2) and RealmHound
(`pillarzz/RealmHound` v0.21.1 at `8e55fbaa`, MIT). Every candidate works from passive capture and local files.
A candidate becomes scope only when the owner moves it into a tab spec or the decision log. Source-only
findings: neither tool was run, and no packet layout below is verified against the current client.

### For planned tabs

| Tab | Idea | Seen in | Data path |
| --- | --- | --- | --- |
| Stats | Fame per map: time, gain and fame per minute for each map visited, with the current character pinned | RealmShark | `fame_samples`, `runs` |
| Stats | Fame graph with a selectable span (5, 10, 30 minutes, all) and the rate over the selection | RealmShark | `fame_samples` |
| Character | The current character's exalt damage multiplier, read from stat 113 (`EXALTATION_BONUS_DAMAGE`) rather than the account API | RealmShark | Deferred by owner decision 2026-10-15; needs an all-stat-exalted character capture and non-1000 evidence. Shown as unavailable |
| Character | Weapon DPS against an adjustable enemy defense; HP and MP regen that labels what it leaves out (base VIT, pets) | Both | Catalog projectiles, observed stats |
| Party | Item-specific rules, class rules or quotas, and preset import | RealmShark | Later scope; v1 has four-slot tiers, maxed stats and announcement text only |

### For shipped tabs

| Tab | Idea | Seen in | Data path |
| --- | --- | --- | --- |
| Runs | Damage taken per player: hits, damage taken, damage blocked and guarded | Both | `PLAYERHIT`, `AOE`, `GROUNDDAMAGE`, `ENEMYSHOOT` are decoded with no reducer |
| Runs | Death recap: killer, fame breakdown and stats at death | RealmShark (death labels) | `DEATH` fields are decoded but unused |
| Runs | Filter or highlight fight rankings by name, guild or class, keeping your own row marked | RealmShark | Fight player data |
| Runs | Gear contribution: which items did the most damage for each player | RealmShark | `damage_sources` |
| Runs | Encounter notes beside damage, such as O3 guard hits and Walled Garden reflectors, labelled as inferred | RealmShark | Encounter-specific rules |
| Runs | Export selected fights as JSON, copy a fight card as an image, delete selected or all fight history | Both | Local save dialog, clipboard, a store delete path |
| Loot | Alerts for chase-list items and chosen enchants, with a badge on the item that explains why it alerted | RealmShark | `drop_items`, chase list |
| Loot | Link each bag to the fight it came from | RealmHound | `drops` and fights |
| Loot | Import RealmShark loot and dungeon logs into Collection, labelled as imported | RealmHound | Local files |
| Logs | Chat alerts for your own name and custom keywords | Both | Chat events |
| Logs | Key Pops also lists runes, vials and incantations | RealmShark | `NOTIFICATION` server-message text; format-sensitive |
| Alerts | A portal for a chosen dungeon appears (not a key pop) | RealmShark | Portal object types in `UPDATE`, catalog |
| Alerts | A chosen enemy appears | RealmShark | New objects in `UPDATE` |
| Alerts | Mark key pops and portals for dungeons this character has never completed | RealmShark | `CREATE_SUCCESS` completion counters (partly decoded) |
| Alerts | Trade request sound | Both | `TRADEREQUESTED` is named but not decoded |
| Alerts | Choose a sound per alert (bundled or a local file) and its volume; preview without saving | Both | Settings |

### Candidate specs after Party

These are separate specs, not unfinished parts of Party v1:

- **Train view:** follow a sequence of runs and show train-level progress and key-pop tallies.
- **Leader alerts:** watched/banned arrivals, failing arrivals and raider deaths, with explicit observation limits.
- **Clipboard and overlay:** callouts, party commands and run-card copying, plus an optional overlay. Clipboard text is for the player to paste; nothing is sent to the game.

### New surfaces

Each needs an owner decision before it is planned.

| Idea | Seen in | Data path | Note |
| --- | --- | --- | --- |
| Tinkerer quests: requirements, rewards, completed quests hidden | Both | `QUEST_FETCH_RESPONSE` when the player visits the quest room; not decoded | In scope (observed only) since 2026-11-06; needs a spec |
| Pet Yard viewer and feed calculator | RealmShark | Pet objects in `UPDATE` while in the Pet Yard | In scope (observed only) since 2026-11-06; needs a spec; feed formulas need checking |
| Observed vault, gift chest and potion storage with a last-seen time, and a potion maxing planner | Both | `VAULT_UPDATE` / vault content while the vault is open; not decoded | Never account-complete; show what is missing |
| Copy a formatted dungeon or realm-event callout to the clipboard | RealmHound | `MAPINFO`, realm events | Nothing is sent to the game |
| Capture health: traffic, packet types, decode failures, server latency | RealmShark | Capture counters; `NEWTICK` timing fields are decoded but unused | Partly covered by Setup and Settings |
| An overlay for single-monitor players | Neither ships one | — | RealmHound issue #6 asks for it; RealmShark's separate Potato overlay dates from 2024 |

### Across tabs

- **Freshness labels.** Show whether a value is live, last seen at a given time, or unavailable, as RealmHound
  does for vaults and characters. Applies to Character, Party and any vault view.
- **Optional data never blocks capture.** Both tools have had startup or loot failures caused by missing
  metadata or external calls (RealmShark #65 and PR #76).
- **Regression scenarios** from the other tools' trackers, worth checking against fixtures: Moonlight Village,
  Shatters HM and Spectral Penitentiary HM loot attribution; Kogbold kills recorded as escapes (RealmHound #12);
  starting HP for DoT-only fights (#17); unattributed solo bleed damage (#18); duplicate item types showing
  copied enchants (#3).

### Later: opt-in account sync

Between them, the two tools reuse the login token the game sends in `HELLO` to call the account API:
`char/list`, `account/listPowerUpStats`, `account/getOwnedPetSkins`, `missions/getClientSeasons`,
`missions/getPlayerMissions`, `season/seasonInfo` and `season/bpInfo`. That fills gaps passive capture cannot:
characters not played since install, vaults and storage not opened since install, the exaltation baseline,
mission progress, season dates and owned pet skins. Neither tool has a graveyard endpoint; deaths are only
known from capture. Maxed builds the passive versions first. Account sync may follow as a separate feature
layered on them, never a replacement for them. Capture itself stays passive either way.

What is known (2026-10-06 research; sources are public, nothing was tested live):

- **Permission is unresolved.** DECA's Terms of Service v1.3 (effective 2026-05-01) prohibit automated
  requests or queries to the game and automated login. DECA's June 2026 Realm Guardians announcement says
  "Sniffers are okay for now". That covers passive capture, not token reuse. In 2017 DECA kept Muledump
  working with a `muleDump=true` parameter, but that is not current approval for a new tool. No public case
  ties a ban to Muledump, RealmShark or RealmHound alone.
- **It can affect the player's own login.** Muledump documented a rate limit shared with the game client
  (about 60 requests per 5 minutes in 2018; the current limit is unknown), "account in use" rejections, and
  `do_login=true` requests counting toward the daily login calendar. RealmHound delays sync until the game has
  been relaunched that day so it does not take the daily forge-fire grant first.
- **It breaks.** RealmShark lost `char/list` in February 2026 and kept only Exalts and Pets. RealmHound still
  syncs as of v0.21.1, but has had account-mixing and season-rollover bugs.
- **Recordings carry the token.** Raw captures include `HELLO`, so a memory-only token policy is not enough on
  its own. RealmHound's raw recordings keep the token.

If it is built, the starting shape is:

- Ask DECA about this exact design first: token reuse, the named endpoints, `do_login`, request rate, and use
  while the game is running.
- Off by default. Each batch runs only when the user presses **Sync now**: no request at startup, on room
  entry or on a timer. When sync is off, no reusable token is kept.
- Reuse only the token the game already sent; never ask for a password. Keep it in native memory under a
  type that cannot be serialized or printed. Sync's copy of the token never reaches the UI, URLs, logs or
  exports. Clear it on disable, account switch, rejection and exit. No sync during replay.
- Recordings need their own rule. The capture loop writes raw frames before it parses them
  (`try_record` in `app/src-tauri/src/runtime.rs`), so any opt-in recording already holds the `HELLO` token,
  as [PRIVACY.md](../PRIVACY.md) discloses. Sync's token handling cannot change that. Until `HELLO` is
  redacted before frames are written (including fragmented or coalesced TCP segments), sync and recording
  cannot be on together: Sync now is unavailable while recording, and recording cannot start with a sync
  token held.
- One request queue: at most one batch per account per 5 minutes (a cautious guess, not a known-safe limit).
  Stop the whole batch on a rate-limit response and keep the cooldown across restarts.
- On a rejected token, "account in use" or a captcha, stop and send the player to the official client. Never
  fall back to password login, token refresh or other hosts.
- Tie every response to its account. Save only allow-listed fields with their source and fetch time, keep
  the old snapshot when a sync fails, and leave missing fields unknown.
- Before calling sync read-only, check its effect on daily rewards across midnight, with the game open and
  closed.
- Disclose it in [PRIVACY.md](../PRIVACY.md), including that DECA has not approved it.

Until the owner approves it, the "no account-service calls" rule in [CONTRIBUTING.md](../CONTRIBUTING.md)
still applies.

### Ruled out

These appear in one or both tools but break Maxed's rules (passive capture, no uploads):

- Uploads: RealmShark's loot sharing (v1.9.2, removed since) and the Potato overlay's shared hero state.
- External metadata: RealmShark's downloaded chat spam list and Crucible API (Maxed reads Crucible data from the
  client).

## Decision log

| Date | Decision |
| --- | --- |
| 2026-11-10 | #39: Crucible and Blood Ritual damage multipliers come from the `CRUCIBLE_RESPONSE` packet (183), not the asset catalog. Definitions are saved locally (migration 020) and reused across connections and restarts; a hit is known when the active ID is in the saved table. No HTTP or account-service lookup is used |
| 2026-11-10 | #39: A known Crucible entry with no type-5 bonus counts as a known 1.0× multiplier (RealmShark's rule). An ID never seen in a definition keeps the counted neutral default and stays ineligible for personal bests. Bonus-type meanings and Blood Ritual (stat 155) matching are not yet checked against observed damage; fights stored before the fix stay as recorded |
| 2026-11-08 | Gate P resolved from sanitized owner evidence (#37/#38): exalt inside boost; HP/MP fields carry bonus amounts. Nonzero exalt permits cap/maxed decisions with observed operands and catalog caps. Exalt damage remains deferred |
| 2026-11-07 | #4: Home follows the current account, with no selector. Legacy attribution uses local recomputation; unmatched history stays stored and is excluded from totals, records and bests with a notice. Windows account-switch acceptance remains pending |
| 2026-11-06 | Observed-only Daily Quests (Tinkerer) and Pets are in scope, built from passive capture with no token. Each needs its own spec before it is built; neither is scheduled yet |
| 2026-11-06 | Opt-in account sync stays deferred until DECA has been asked about the exact design in [Later: opt-in account sync](#later-opt-in-account-sync) |
| 2026-11-06 | No candidate features are scheduled for now |
| 2026-11-06 | Owner acceptance from use: Home redesign phase 2 (live recap, chains, Discord paste, sprite copy), Character weapon/regen values, 100%/150% scaling and keyboard behavior, the shared chat cue, boss closure, departures, realm return and personal-best alerts. Clean install/update/uninstall worked. Gate P stays open and exalt damage stays deferred |
| 2026-10-20 | Character: the owner approved the production `character` flag before Task 11. Windows live weapon/regen values, state transitions, scaling and keyboard behavior are being verified in use; Gate P remains open and exalt damage remains deferred |
| 2026-10-16 | Home recap no longer takes over Nexus: show a compact banner for 15 seconds of visible Home time, paused on hover/focus, hidden on dungeon/realm entry. Last recap retains the latest finished run/visit in memory until replacement or stream reset; Back returns to the current live area |
| 2026-10-15 | Exalt stat bonuses (+1 per level; +5 HP/MP) are in scope for v1; exalt damage stays deferred |
| 2026-10-15 | Character exalt damage remains deferred, preserving the 2026-10-05 decision. All four reviewed scrubbed fixtures show stat 113 fixed at 1000 for the same Warrior; its meaning is unproven. Wait for a character with at least one exaltation in every stat, including life/mana. The owner's hypothesis (+5% damage per full +1 tier across all stats, likely stat 113 per-mille) is unverified. Show unavailable, not 1.0×; compare 113/1000 with the in-game bonus and local DAMAGE, then commit a scrubbed fixture with 113 != 1000 and update protocol evidence. See the Character spec (private design notes) |
| 2026-10-13 | Home redesign phase 2 merged without a new flag, by owner decision: saved dungeon, death and realm-visit recaps and copy image use the existing `homeRedesign` flag. Owner acceptance of live recap, chain, Discord paste and sprite copy is pending. Recap reads wait for the stream's durable watermark, then load the run story or visit recap, run records and comparison reads. Existing stored JSON and visit data support the reads; no migration was needed. Journey and Progress follow |
| 2026-10-13 | Home redesign phase 1 turned on in production after its history reads were slimmed (#152); the owner verifies in use. Home reads a dungeon-scoped dry streak instead of all-time loot stats, lean 90-day boss baselines that seek `fights_started`, and a split dungeon profile; the live window is built once per batch plus area-boundary checkpoints. An index on extracted `target_type` stays deferred until measured; see [Home slim reads](performance/2026-10-13-home-slim-reads.md) |
| 2026-10-12 | Home redesign phase 1 initially shipped behind `homeRedesign`, off in production: live dungeon/realm, session HUD, party-average buckets and Shell-owned placeholder recap. Detailed recaps, copy image, Journey and Progress follow in later phases. Migration 015 adds indexes only for loot statistics and dry-streak gaps |
| 2026-10-05 | Tabs in scope: Loot, Logs, Stats / Sessions, Character, Party / Parse. Build order: Loot → Logs → Stats → Character → Party |
| 2026-10-05 | Spec structure: this roadmap plus one detailed spec per tab, written just before that tab is built |
| 2026-10-05 | Alerts folds into Logs as **Logs › Alerts**. Old `#/alerts` links redirect there. Alert preferences stay in Settings |
| 2026-10-05 | Loot statistics show observed per-run rates with an explicit "not a drop rate" caption |
| 2026-10-05 | Loot collection: chase list (user-pinned items) plus a per-dungeon grid of items you've seen. No bundled loot tables |
| 2026-10-05 | Chat is captured and stored locally for **all channels, on by default** (matching RealmShark), with retention and "clear chat history" controls. This replaces the earlier "chat is not ported" stance |
| 2026-10-05 | Party / Parse **stores** other players' gear and stats (not live-only), on by default |
| 2026-10-05 | Privacy stance: data about other players that your client receives (names in notices, chat, gear and stats) is stored locally by default and shown in the app. See [privacy](../PRIVACY.md#other-players-data) |
| 2026-10-05 | Chat is kept for 90 days by default, adjustable in Settings, with a "clear chat history" control |
| 2026-10-05 | Chat, player names and other players' gear and stats never appear in diagnostic logs. "Logged" means stored and shown in the Logs tab |
| 2026-10-05 | Party data is linked across sessions, so a player you've seen before shows their history. Players are matched by name, or by an account ID if the capture turns out to provide one. A renamed player shows up as a new player; that edge case is accepted |
| 2026-10-05 | Each chat channel (All, Guild, Party, Whisper) has its own storage toggle in Settings; all are on by default |
| 2026-10-05 | Party history is kept until the user clears it, with a "clear party history" control |
| 2026-10-05 | Stats CSV exports can include other players' names from your runs. Exports are local files the user saves |
| 2026-10-05 | The Character tab ships without exalt multipliers, shown as unavailable, until exaltation data is understood ([protocol evidence](parity/protocol-evidence.md)) |
| 2026-10-05 | Logs: channel storage toggles are applied in the store so event sequences never gap; a switched-off channel still alerts |
| 2026-10-05 | Logs: Whisper, Guild and Party alerts are pruned and cleared with the chat they came from |
| 2026-10-05 | Logs: Settings › Chat (toggles, retention, hide lists, Clear chat history) is available regardless of the `logs` flag, because chat is stored by default |
| 2026-10-05 | Logs: one generated chat cue is shared by the three chat alert kinds; owner ear approval pending |
| 2026-10-06 | Logs: the production `logs` flag is on; live channel classification is still verified in use |
| 2026-10-06 | Build out passive capture fully first. Opt-in account sync through the account API is deferred, not ruled out; it must not cap what Maxed can do |
| 2026-10-06 | Stats: sessions are the automatic `play_sessions`; no manual timer, and no rename, pin or delete in v1 |
| 2026-10-06 | Stats: fame per map covers Realm and hubs through a new `map_visits` table (migration 010) projected by the store from `MapEntered`; existing runs are backfilled as dungeon visits. Fame per visit uses Home's baseline rule at read time |
| 2026-10-06 | Stats: the default view ranks maps by fame per hour. Filters are time range and character |
| 2026-10-06 | Stats: CSV exports each table as shown, plus a session's runs with each run's party. Saving uses `tauri-plugin-dialog` from Rust only |
| 2026-10-07 | Stats: accepted by the owner; the production `stats` flag is on |
| 2026-10-10 | Party: live first, with roster, requirements, contribution flags and saved history. Train view, Leader alerts, and Clipboard and overlay need separate specs; use the approved Party layout |
| 2026-10-10 | Party requirements v1: maxed-stat count and minimum tier in all four equipment slots, with a per-preset UT/ST toggle. No item lists, class rules or class quotas; at most 32 presets, with dungeon auto-apply and one default |
| 2026-10-10 | Party name actions: copy name, copy `/t Name `, copy failing names and Open on RealmEye in the default browser. The browser sends the name; Maxed does not request the page |
| 2026-10-10 | Party architecture: engine presence feeds a native current-scope roster queried separately from LiveSnapshot. Merge and link by `name_key`, text before the first comma and ASCII-lowercased; renamed players start new history |
| 2026-10-10 | Clear party history removes Party history and other players' profiles, gear logs and DPS timelines. Keep local-player data, runs, chat and events; keep marks and notes unless explicitly ticked for removal |
| 2026-10-11 | Party bystanders outside runs use non-persisted `take_roster_profiles`; players who never fought are visible live without being stored. Deliberate marks and notes can create a history entry |
| 2026-10-11 | Party Live refreshes from the presentation-only `party-changed` notice, at most once per second with leading and trailing delivery and immediate pending delivery on flush. Saved views keep durable engine-event invalidation |
| 2026-10-11 | Party Left early is a saved-run signal because departures are known at run end. Keep the row label Left when a departure arrives; do not show a Live Left early count |
| 2026-10-11 | Party presets save through dedicated `update_party_settings`; general settings saves preserve stored presets to prevent stale overwrites |
| 2026-10-11 | RealmEye handoff uses the system `explorer.exe` and a validated 1–10 ASCII-letter name. Spawning the handoff does not verify that the default browser opened |
| 2026-10-11 | Party: the production `party` flag is on for owner use; live in-game roster, parse and departure behavior is still verified in use |

## Open decisions

Each must be answered before the named tab's spec is written. The recommendation is the default if no other
answer is given.

| Tab | Question | Recommendation |
| --- | --- | --- |
| Character | Resolved 2026-10-15: exalt damage multiplier deferred | Keep the 2026-10-05 "ships without exalt multipliers" decision. Show unavailable until an all-stat-exalted character, including life/mana, supplies evidence; follow the Character verification plan (private design notes) |
| — | Opt-in account sync, once DECA has answered? Deferred 2026-11-06 until DECA is asked; see [Later: opt-in account sync](#later-opt-in-account-sync) | Ask DECA about this exact design before building it |

## Rules for every tab

- **One history reader.** Every new read is a `HistoryKey` in `app/src-tauri/src/runtime/history.rs` with a
  store reader in `maxed-store`. No second reader, no reads on the UI thread, bounded page sizes.
- **Migrations are numbered in the plans.** Loot took 008, Logs 009, Stats 010,
  excluded-bag cleanup 011, event-name search 012, player observations 013
  Party 014, Home loot-statistics indexes 015, maintenance receipts 016, accounts 017,
  account read index 018, attribution notice 019 and Crucible definitions 020. The next spec claims 021 so parallel workers never collide.
  Never rewrite an installed migration.
- **Home stays the Session/Today view; Stats owns lifetime views.** Don't duplicate Home's KPI strip or
  By-dungeon section; link from Home to the matching Stats view instead. The Home redesign phase 1,
  on in production with `homeRedesign` retained for rollback, is the exception with live dungeon/realm screens and a placeholder recap.
- **Live invalidation** follows the `app/ui/src/lib/runs/invalidation.ts` pattern: a tab re-reads only when
  an event affects what it shows.
- **Preview.** Every tab has a fixture or `?preview` mode for visual checks; see [UI preview](development/ui-preview.md).
- **Privacy.** Other players' data is stored and shown locally by default (decision log). Maxed never uploads
  it. Exports the user saves (such as Stats CSV) carry it off the database; the explicit
  RealmEye browser handoff sends a player name from the browser. It never goes into diagnostic
  logs, issues, PR text or committed fixtures (see
  [fixtures](development/fixtures.md)). Each tab that adds a new kind of stored data updates
  [PRIVACY.md](../PRIVACY.md) in the same change.
- **Workflow.** Codex implements through Orchestra (at most three at once), Claude reviews, and the
  coordinator plans, verifies and merges. Feature PRs target `main`.
