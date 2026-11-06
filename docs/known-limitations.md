# Known limitations

Maxed is pre-release 0.1.1. These limits describe the available behavior and its
evidence boundaries. Automated regressions cover several reported problems;
Windows gameplay and installed-release acceptance remain incomplete.

## Active limitations

| Area | Problem and impact | Workaround or interpretation |
| --- | --- | --- |
| Realm player timelines | Busy realm visits can exceed the scope timeline limit, making whole-visit timelines unavailable. | Fight charts show only the player's line when its fight series was retained; the party average stays unknown. |
| Local deaths outside runs | The local player's deaths appear in the Activity card only when the death notice was attributed. | An absent Activity entry does not establish that the player survived. |
| Memory and CPU | Busy realms keep WebView2 at roughly 250–275 MB combined, settling to about 175 MB after leaving. That is within the revised 0.1.0 limits for the synthetic workload, but long live sessions are not yet verified. | Keep acceleration off. If resource use grows during a long session, restart Maxed and report it; see [measurement evidence](performance/2026-10-06-live-realm-fixes.md). |
| Realm focus | Shared quest/boss and local-hit selection has deterministic tests, but live focus remains unverified. Another player hitting a different boss can challenge focus selection when local engagement or quest evidence is absent or stale, showing a fight the local player is not engaging. | Check the displayed target and local contribution. Do not read the focused rate as the current encounter without that match; no verified live workaround exists. |
| Damage shares | Some server damage has no resolvable player/projectile owner. Shares among attributed players can look inflated relative to the full group, and ranks cannot describe unobserved contributions. | Read the unattributed amount alongside shares and ranks; a large displayed share is not proof of full-party contribution. |
| Player rejoining | Damage drill-downs can retain separate instances when a player rejoins a dungeon. | Party merges same-name instances into one roster row; damage drill-down rows remain observations of separate instances. |
| Other players' deaths | Other players' deaths are attributed only on a unique name match. | Missing or ambiguous names leave death attribution unknown. |
| Scope timeline resolution | Timeline resolution coarsens after compaction above 1,800 buckets. | Read the bucket bounds when comparing damage over time. |
| Other players' stats | Other players' stats appear only if the server sends them. | Absent stats remain unknown. |
| Other players' weapon damage | Damage without an observed shot is inferred from the equipped weapon when its catalog entry has projectiles, and may include unannounced ability damage. | The source carries `est.` and “Inferred from the equipped weapon”. Missing, unknown or projectile-less weapons stay Other; local-player and summon attribution are unchanged. Previously saved fights keep their recorded sources. |
| Filled fixture evidence | Some committed fixtures have irreversibly filled announcement or death-victim text. Missing victim identity leaves one extra pre-Victory departure in the Shaitan replay. | Preserve conservative expectations until a reviewed re-scrub retains the evidence. Replay cannot reconstruct the missing identity. |
| Windows key-pop notifications | Key-pop toasts include the portal sprite when the local catalog provides usable portal artwork; missing or unreadable artwork falls back to dungeon text. Real Windows display remains unverified. | Settings → Alerts → Test Notification uses a catalog dungeon when available. Verify sprite display on Windows. |
| Key-pop filters | All, All except… and Only… are supported with name/alias search, portal sprites and removable selections. An empty Only… filter silences key pops and shows a warning. Installed Windows interaction remains unverified. | Settings → Alerts → Key Popped groups delivery and filtering; press Save for picker changes. Mute on an in-app alert or log row saves immediately and offers Undo for 10 seconds; failed saves offer Retry. |
| Loot rates | Stats shows white rate (observed whites divided by ended runs of any outcome, as a percentage that may exceed 100%); capture can miss bags and contents. Sources outside a run have no rate. | These are observed per-run rates, not drop chances. See [Loot interpretation](architecture/ui.md#loot). |
| Collection coverage | The per-dungeon grid shows only seen UT/ST items with catalog tier metadata; no loot tables are bundled. | Missing items do not establish what a dungeon can drop. The chase list and seen items are not possessions or a complete inventory. |
| Settings downgrade | Older Maxed builds reject settings saved by this build because they do not recognize the new `loot`, `chat` and `party` fields and the `alerts.key_pop_excluded_dungeons`, `whisper`, `guild_message` and `party_message` alert fields, despite settings remaining version 1. `AppSettings.party` is always serialized, so any settings saved by this build are rejected by older builds, even with no presets. | See [chase settings compatibility](architecture/storage.md#chase-settings) before downgrading. |
| Large Loot histories | Recorded 500,000-bag Collection reads take about 0.85–0.93 s, exceeding the report-only 750 ms target. | This is synthetic timing evidence, not a release-performance pass; see [Loot history reads](release/performance.md#loot-history-reads). |
| Chat channel rules | Live guild, party and whisper classification and whisper direction remain unverified on Windows. | Logs is on in production; a misclassified message still appears under Chat › All. |
| Clear chat history disk size | Deleted SQLite rows free reusable pages without shrinking the file. | Clearing chat is not a disk-compaction operation. |
| Pending chat alerts after clear | A chat alert with delivery still pending at clear time remains until a later prune. | Clearing chat does not immediately remove every retained chat alert. |

## Home redesign phases 1 and 2

- The v4 Home is the production default; `homeRedesign` remains for rollback.
  Phase 2 adds saved dungeon, death and realm-visit recaps and copy image under
  that same flag. Journey and Progress are later work.
- Party average is the mean DPS of the other players in the same fight whose
  DPS is established. It excludes the local player and unattributed damage.
  The live party line divides attributed non-local bucket damage by non-local
  player-seconds; merged buckets sum both. Fights saved before this release
  have no line and show “Party average line not recorded for this fight”.
- Realm score is not closure evidence or a closure prediction.
- White counts and dry streaks describe bag appearances, not pickups. A dry
  streak counts ended dungeon runs since the latest observed white; missing
  capture can miss a white. Average gaps are observed, not a drop rate.
- Recap times use the capture clock, not the wall clock. The placeholder uses
  retained observations while saving or partial, so missing values remain unavailable.
- Observed HP is a series of received stat readings, not damage taken. The
  death recap shows the final five seconds and leaves missing readings as gaps.
- The killer comes from death-message text, resolved through the catalog where
  possible; missing or unmatched text does not establish a confirmed enemy.
- Chains and runs in a row are inferred from ordered map visits in one session.
  Unobserved transitions can break that inference; chains are labelled inferred.
- Recap records are computed as of the run that earned them, against earlier
  stored observations. They are not a statement of the player's current record.
- After Maxed was hidden, placeholder fields carried only by snapshots (whole-run
  DPS, rank and player count) show “—” unless a fresh terminal snapshot supplies them.
- Synthetic previews and browser layout checks do not verify live Npcap,
  game-client observations, Windows WebView2 or installer behavior.
- The isolated [clipboard spike](development/recap-clipboard-spike.md) verified
  a WebView2 PNG write. Copying with real sprites, pasting into Discord and
  Segoe UI Variable rendering still need owner acceptance.

## Party

Party ships behind the `party` flag, on in production; live in-game behavior is still being verified.

- The roster observes only players in the client's view, not everyone in the
  instance. Previously seen players remain as dimmed out-of-view rows in the
  current scope. Nexus and realm view range can exclude most of the population;
  the roster admits at most 512 player instances per scope.
- No damage seen and damage shares cover only damage the client receives.
  Live run totals retain at most 64 players; totals and shares for omitted players
  stay unknown, and an omitted player is not treated as having zero damage.
- Left early is known only at run end, so it appears on saved runs. Out of view
  does not establish departure or lack of contribution.
- History uses the name before the first comma, ASCII-lowercased. Renamed players
  start new history; it is not an account identity lookup.
- Empty and unreceived gear slots are indistinguishable before a level arrives.
  Missing catalog tiers yield Check; missing profiles yield Not seen. Check
  results need judgment and are not confirmed failures.
- `AppSettings.party` is always serialized, so any settings saved by this build
  are rejected by older builds, even with no presets. The settings version remains
  1. Back up settings before downgrading.
- An older build opening a version 14 database stops with a newer-schema error.

Synthetic tests and the Party preview do not verify a crowded live roster, real
gear/departure observations, WebView2 behavior or the default-browser RealmEye
handoff. Those remain Windows acceptance checks.

## Stats

Stats is on in production; live fame and visit tracking across real Windows play is not yet verified.

- Realm and hub visits exist only from the Stats upgrade; earlier sessions count dungeons only.
- Capture started mid-map, or play resumed after a 30-minute idle gap, records no visit until the next map change.
- Time runs from entering a map to leaving it; idle time is not subtracted.
- Runs' per-run fame can be slightly lower than Stats' because Runs measures first to last sample within the run.
- A run open when the app crashed before the Stats upgrade has no map visit; Dungeons still counts it.

## Implemented behavior awaiting live acceptance

The open gameplay reports include behaviors covered by deterministic fixes.
The following evidence limits remain; the original symptoms are not claims that
the corresponding fixes are absent.

| Area | Supported behavior and remaining impact | Workaround or interpretation |
| --- | --- | --- |
| Logs live updates | Chat and instance-event invalidation and the New messages control have synthetic coverage; live updates and channel rules await Windows acceptance. | Verify real guild/party/whisper traffic, capture-running/stopped clear and long-session retention. Chat-cue ear-approval remains pending. |
| Loot live updates | Feed, Item history, Stats and Collection refresh from durable observations and ended-run changes. New bags appearing across these views during real Windows play are not yet verified. | Synthetic tests do not establish live capture or WebView2 acceptance. |
| Boss closure and kill announcements | Confirmed kills end fights at the kill timestamp; recognized Oryx `.death.N`/`.killed.N` announcements plus target removal can confirm a kill despite positive final HP. Windows acceptance of boss HP/timeline closure remains incomplete. | HP1 alone and a quest switch are insufficient evidence; filled-text fixtures cannot prove an announcement. See [kill evidence](parity/kill-evidence.md). |
| Departures | Only departures before Victory count as Left; a matched death cancels a departure. Live and stored counts still need a full-run check. | Treat the filled Shaitan victim case separately; its extra pre-Victory departure is a fixture limit. |
| Consumable ranking | Catalog consumable/stack metadata keeps ordinary potions from outranking equipment merely because game labels say UT. Actual-client best-bag display still needs acceptance. | Re-extract stale assets and inspect the item type rather than assuming UT means equipment. |
| Personal bests | DPS eligibility distinguishes known neutral reconstruction from uncertain inputs; terminal revisions and run-time PBs have regression coverage. Fully observed real-catalog alert acceptance remains incomplete. | Start capture before character selection. First eligible records establish silent baselines; incomplete or estimated DPS remains ineligible. See [personal-best rules](architecture/personal-bests.md). |
| DPS basis and display | Product player DPS uses each player's active window and is unavailable below three seconds; current UI formatting uses whole numbers. Run DPS retains its separate window. Live comparisons across surfaces still need acceptance. | Do not compare legacy whole-target stored rates or reference export rates with current player-window rates. |
| Realm kill history | Ordinary traffic no longer evicts early kills: compact visit history retains up to 500 fight kills and 500 announcement rows, independently of the 64 detailed fights. Realm loot retains up to 512 bags. Long-visit live acceptance remains incomplete. | Exceeding a history cap marks the visit partial with a specific reason. |
| Realm return | Same-instance continuity retains visits across dungeon trips, subject to bounded visit/history storage. Live realm → dungeon → realm acceptance remains incomplete. | A new realm identity starts a new visit; compact kill history and loot survive same-instance returns within their explicit caps. |
| Key-pop delivery | Dungeon filtering, local-pop suppression, sound and native delivery have regression coverage. Installed display/listening and real capture acceptance remain separate. | Save preferences and use Test for delivery; demo/replay stays silent and a reconnect-only fixture does not prove full identity/capture. |
| In-app alerts | Banners and toasts overlay the top of the content pane without moving route content; Key Popped uses local portal art when available. Windows/WebView2 layout, motion and narrow-width acceptance remain incomplete. | Unknown dungeons or missing catalogs/portal art show no icon. Check scrolling, reduced motion and Dismiss access at 900 px and 440 px on Windows. |
| Refresh status | Transient refresh/loading status uses a bottom-right overlay; actionable errors stay inline. Actual Windows layout, focus and screen-reader behavior remain unverified. | Use the panel's retry action for errors. |
| Death alerts | Death records and timelines remain available, but Death is only a legacy alert-history kind, not an active alert or sound. | Use history for death data; existing saved Death alerts do not imply new death notifications. |

Capture is a partial local observation. Mid-connection starts, packet loss,
absent catalog/identity and unsupported layouts can prevent complete statistics
or eligible personal bests. Population, remote death joins and inferred loot
sources retain evidence limits. Missing data is not zero or a confirmed fact.
Flawless detection, realm trip browsing, account-wide progression and an overlay
are not shipped features.

Unsupported modifier/grade semantics and exaltation breakdown
lack sufficient evidence; see [protocol evidence](parity/protocol-evidence.md).
Setup/demo-clock read failures can leave Today unavailable until a new stream.
Diagnostic path redaction is incomplete for some paths with spaces, so local
reports need inspection before sharing. Installer Npcap guidance is not
selectable; the [installation guide](release/INSTALL.md) provides the link.

The [release checklist](release/CHECKLIST.md) remains incomplete: clean
install/update/uninstall, real capture, recording under volume pressure, network
coverage, Windows input/DPI/accessibility and performance acceptance are not
established. Public readiness also requires rights review, fixture publication
approval, a Code of Conduct contact, private reporting and legal-notice
reconciliation.

## Character (live acceptance pending)

- `character` is on in production, approved by the owner on 2026-10-20 before
  Task 11. Windows live acceptance is still pending and is being verified in use:
  weapon/regen values versus the client, state transitions, 100%/150% scaling
  and keyboard behavior. Synthetic preview and browser geometry checks do not
  verify live Npcap, game-client or WebView2 behavior.
- Gate U was accepted PASS by the owner on 2026-10-17 in
  [PR #176](https://github.com/daplabs/maxed-rotmg/pull/176#issuecomment-6241933324), with the evidence in
  [Character live acceptance](development/character-live-acceptance.md).
  Gate P (exalt placement in base versus boost) is still open. The provisional
  split retains totals/raw boosts; nonzero or unknown exalt keeps at-cap,
  pots-left and maxed null without placement evidence. Negative Other remains
  inconsistent evidence, not a named effect. U does not unlock P.
- [#170](https://github.com/daplabs/maxed-rotmg/issues/170) tracks expanded rail
  exalt presentation. [#182](https://github.com/daplabs/maxed-rotmg/issues/182)
  tracks legacy saved local profiles retaining ungated maxed counts; current
  Character split safety does not retroactively repair those records.
- Normal-fire DPS and regen are baseline models, with source-derived
  [Gate X metadata](architecture/assets.md#character-metadata-gate-x-2026-10-17).
  Current-client rate/regen coefficients still need Windows verification.
  Theoretical DPS assumes every projectile hits one target once during sustained
  fire. Comparisons use current stats without recalculating equipment bonuses.
  Unsupported schedules/effects leave full totals unavailable; independent
  supported groups can still contribute a labeled subtotal.
- Base VIT recovery, base MP regeneration, pets, consumables, combat-state
  doubling and suppression/healing effects are absent from the regen model.
  Missing maxima block percentage contributions; missing enchant metadata does
  not establish zero. WIS alone is not the complete natural MP regeneration rate.
- Damage preserves the engine's f32 conversion/multiplication and integer
  truncation before defense. Exact means use bounded monotone partitions, not a
  midpoint approximation. The work budget is 65,536 interval nodes per weapon and
  the catalog group/projectile bound is 256; exhausted or invalid arithmetic is
  unavailable, never an approximate full total. Display rounding is separate.
- Exalt damage (stat 113) remains deferred with no value. Crucible, Blood Ritual, weapon
  enchant damage/rate modifiers and enemy conditions are excluded. This work
  establishes no new multiplier evidence or combat-attribution behavior.
- [#177](https://github.com/daplabs/maxed-rotmg/issues/177) tracks cross-account
  aggregation in saved history; Character uses only the current local observation
  and adds no account/history lookup. Object stat 114 remains out of scope in
  [#168](https://github.com/daplabs/maxed-rotmg/issues/168).
