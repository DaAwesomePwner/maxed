# Known limitations

Maxed is pre-release 0.1.1. These limits describe the available behavior and its
evidence boundaries. Automated regressions cover several reported problems;
Windows gameplay and installed-release acceptance remain incomplete.

## Active limitations

| Area | Problem and impact | Workaround or interpretation |
| --- | --- | --- |
| Memory and CPU | Busy realms keep WebView2 at roughly 250–275 MB combined, settling to about 175 MB after leaving. That is within the revised 0.1.0 limits for the synthetic workload, but long live sessions are not yet verified. | Keep acceleration off. If resource use grows during a long session, restart Maxed and report it; see [measurement evidence](performance/2026-10-06-live-realm-fixes.md). |
| Realm focus | Shared quest/boss and local-hit selection has deterministic tests, but live focus remains unverified. Another player hitting a different boss can challenge focus selection when local engagement or quest evidence is absent or stale, showing a fight the local player is not engaging. | Check the displayed target and local contribution. Do not read the focused rate as the current encounter without that match; no verified live workaround exists. |
| Damage shares | Some server damage has no resolvable player/projectile owner. Shares among attributed players can look inflated relative to the full group, and ranks cannot describe unobserved contributions. | Read the unattributed amount alongside shares and ranks; a large displayed share is not proof of full-party contribution. |
| Filled fixture evidence | Some committed fixtures have irreversibly filled announcement or death-victim text. Missing victim identity leaves one extra pre-Victory departure in the Shaitan replay. | Preserve conservative expectations until a reviewed re-scrub retains the evidence. Replay cannot reconstruct the missing identity. |
| Windows key-pop notifications | Key-pop toasts include the portal sprite when the local catalog provides usable portal artwork; missing or unreadable artwork falls back to dungeon text. Real Windows display remains unverified. | Settings → Alerts → Test Notification uses a catalog dungeon when available. Verify sprite display on Windows. |
| Key-pop filters | All, All except… and Only… are supported with name/alias search, portal sprites and removable selections. An empty Only… filter silences key pops and shows a warning. Installed Windows interaction remains unverified. | Settings → Alerts → Key Popped groups delivery and filtering; press Save for picker changes. Mute on an in-app alert or log row saves immediately and offers Undo for 10 seconds; failed saves offer Retry. |
| Loot rates | Stats shows white rate (observed whites divided by ended runs of any outcome, as a percentage that may exceed 100%); capture can miss bags and contents. Sources outside a run have no rate. | These are observed per-run rates, not drop chances. See [Loot interpretation](architecture/ui.md#loot). |
| Collection coverage | The per-dungeon grid shows only seen UT/ST items with catalog tier metadata; no loot tables are bundled. | Missing items do not establish what a dungeon can drop. The chase list and seen items are not possessions or a complete inventory. |
| Settings downgrade | Older Maxed builds reject settings saved by this build because they do not recognize the new `loot` and `chat` fields and the `alerts.key_pop_excluded_dungeons`, `whisper`, `guild_message` and `party_message` alert fields, despite settings remaining version 1. | See [chase settings compatibility](architecture/storage.md#chase-settings) before downgrading. |
| Large Loot histories | Recorded 500,000-bag Collection reads take about 0.85–0.93 s, exceeding the report-only 750 ms target. | This is synthetic timing evidence, not a release-performance pass; see [Loot history reads](release/performance.md#loot-history-reads). |
| Chat channel rules | Live guild, party and whisper classification and whisper direction remain unverified on Windows. | Logs is on in production; a misclassified message still appears under Chat › All. |
| Clear chat history disk size | Deleted SQLite rows free reusable pages without shrinking the file. | Clearing chat is not a disk-compaction operation. |
| Pending chat alerts after clear | A chat alert with delivery still pending at clear time remains until a later prune. | Clearing chat does not immediately remove every retained chat alert. |

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
