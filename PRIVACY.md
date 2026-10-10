# Privacy

## Scope

This describes the inspected production application code and installer configuration.
Maxed's own code has no account service, telemetry or analytics. It does not automatically
upload captures, summaries, settings or assets. Installed-process network behavior
still needs the installed-process audit in the [release checklist](docs/release/CHECKLIST.md).

## Capture

Maxed passively reads the stock client's traffic through Npcap on the selected
adapter. The capture filter is TCP port 2050. It does not inject or modify game
traffic. Maxed runs unelevated with default, non-admin-restricted Npcap.
The captured stream can include login tokens and data about other players.

## Local data

The application data directory is `%APPDATA%\com.maxed.companion\`, resolved by
Tauri from the application's identifier. The app's managed data includes:

| Location within that directory | Contents |
| --- | --- |
| `settings.json` | Selected adapter and local game asset path, alert preferences, `chat` channel toggles, retention and hide lists, Loot chase list, recording choice and limit, hardware acceleration choice and update channel. |
| `live.sqlite3` | Local character observations, sessions, runs, fights and player summaries, drop observations, fame/death records, timelines, chat messages and alert history. Fight summaries, chat and stored instance events (`instance_events.json`) include other players' names and data; see [other players' data](#other-players-data). |
| `demo.sqlite3`, `replay.sqlite3` | Separate history namespaces for demo and replay when used. |
| `recordings/` | Opt-in raw traffic recordings and recording ownership metadata. |
| `assets/v1/` | Catalog, sprite images, manifests and cache ownership metadata extracted from the user's local game installation. |
| `startup-assets.json` | Advisory receipt used to check the extracted asset cache at startup. |
| `updates-v1/` | Temporary verified installer staging and its ownership lock. |

Map visits record which maps you entered, when, and with which character, used for Stats.
They are stored locally in the history database.

Settings writes also use temporary and lock files. SQLite and cache operations
can create journal, staging and ownership files alongside their data. These are
local implementation files, not uploads. History summaries are retained; the
recording quota does not delete them. Runtime-managed WebView2 files are separate
from the app's own data formats listed here.

## Other players' data

Maxed stores data about other players that your game client receives, on by
default, so it can show it in the app. During normal play `live.sqlite3`
already keeps:

- **Fight summaries** for every player observed in a fight: in-game name,
  class, damage, hits, biggest hit, deaths, DPS, share and rank, plus damage
  per weapon or ability source with its item type and enchant details.
- **Instance events** naming other players: death victims, departures, portal
  openers and realm-event killers.
- **Chat** from all channels (All, Guild, Party and Whisper) in `chat_messages`,
  each channel toggleable in Settings › Chat. Chat is kept for 90 days by
  default (adjustable from 1 to 3,650 days) and can be removed with
  **Clear chat history**. Whisper, Guild and Party message alerts keep the
  sender and a 120-character excerpt and follow the same retention; clearing
  keeps a chat alert whose delivery is still pending until a later prune. Hide
  lists hide messages and suppress alerts but do not delete stored chat. Stored
  chat is shown in the Logs tab. Demo and replay chat is kept in their own
  databases.

Fight summaries and instance events have no age-based cleanup; they are kept
with the rest of your history. **Stats** CSV exports can include other players'
names from your runs. Exports are files you choose to save; treat them like the
rest of your local data.

Planned tabs add more (see the [roadmap](docs/roadmap.md)):

- **Party / Parse** will store the gear and stats of players in your instance
  and link them across sessions by name, so you can see players you've met
  before. This history is kept until you clear it.

Apart from exports you save, this data stays in the local database. Maxed does
not upload it or write it to diagnostic logs, and it is excluded from committed
fixtures. Do not paste chat, player names or other players' gear into public
issues. This section is updated when each tab
ships with its exact storage and controls.

## Recordings

Recording is off by default. Enable it only if you accept keeping sensitive raw
traffic locally. Recordings can contain login access tokens, passwords and other
players' data. Keep them private; do not attach them to public issues.

The default and maximum quota is 1 GB (1,000,000,000 bytes). Cleanup removes the
oldest closed owned recordings first. It does not delete the active recording or
unrelated files. A recording error is shown without turning summaries into raw
recordings. You can remove retained recordings after closing Maxed.

## Outbound requests

Maxed's own code makes no network requests except the manual, user-initiated
update check, download and install from GitHub:

- Clicking the update check fetches the selected channel's fixed GitHub release
  metadata. Clicking download fetches the offered version's fixed installer URL.
  Startup does not check automatically. See [the update guide](docs/release/UPDATES.md) for
  the exact repository and channel URLs.
- Each update request begins at `github.com` over HTTPS. Redirects are limited to
  `release-assets.githubusercontent.com`, with a validated release-asset path
  containing a decimal repository ID and canonical UUID. Later redirects must
  retain that delivery path. At most five redirects are allowed. Arbitrary hosts,
  URL credentials, explicit ports and fragments are rejected. The updater disables
  proxy use, automatic retries and referrer headers, and identifies itself as
  `Maxed-Updater`. It uses local TLS trust roots without network certificate lookups.

Separately, when WebView2 is missing, the installer downloads Microsoft's
bootstrapper from `https://go.microsoft.com/fwlink/p/?LinkId=2124703` and runs it.
This is separate from the updater's redirect policy. Microsoft's bootstrap
delivery and runtime installation require a network audit on the target machine.

Update requests disclose ordinary connection information, such as the source IP
address, to the receiving service. They do not upload local game data. Npcap is
installed separately by the user; Maxed's Npcap guidance does not open a browser
or download it. No remote fonts, images, analytics or account requests are part
of the inspected application flows.

The embedded Microsoft Edge WebView2 runtime independently contacts Microsoft
services, even while Maxed displays only local content. This is default behaviour
of the Microsoft WebView2 runtime rather than of Maxed: the account-related
connection was reproduced in a separate minimal WebView2 application on the
same PC. Which connections occur can vary with runtime version, profile and
configuration.

The release audit allows these runtime service hosts on port 443, only when the
connection belongs to a WebView2 process:

| Microsoft host | Purpose |
| --- | --- |
| `substrate.office.com` | Account information through Windows sign-in components (the most likely explanation of the observed connection; its exact request and contents could not be inspected). |
| `config.edge.skype.com` | Edge configuration, feature rollouts and experiments. |
| `edge.microsoft.com` | Runtime component and security updates. |

Component and security updates also download their signed payloads from these
Microsoft hosts, allowed on port 80 or 443 for WebView2 only:

- `msedge.b.tlu.dl.delivery.mp.microsoft.com`
- `msedge.f.tlu.dl.delivery.mp.microsoft.com`
- `msedge.b.dl.delivery.mp.microsoft.com`
- `msedge.f.dl.delivery.mp.microsoft.com`

Microsoft lists these exact hosts among its
[browser support and component-update endpoints](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-security-endpoints#other-browser-support-services).
HTTP on port 80 is accepted for these downloads because the component updater
verifies signed CRX3 packages before unpacking them; see Chromium's
[component verifier](https://github.com/chromium/chromium/blob/main/components/update_client/unpacker.cc)
and [CRX3 signature verification](https://github.com/chromium/chromium/blob/main/components/crx_file/crx_verifier.cc).
Signatures protect the payload's authenticity and integrity; HTTP does not
encrypt the connection. This exception does not allow HTTP for the three service
hosts above or network requests from Maxed's own code.

These requests are Microsoft's runtime behaviour. Maxed does not initiate them,
control Microsoft's services or receive their request/response contents. Maxed
keeps the existing runtime defaults, including configuration and component
updates; it does not add switches to disable them. Microsoft documents
[WebView2 data and privacy](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/data-privacy),
[Windows account integration](https://learn.microsoft.com/en-us/entra/msal/dotnet/acquiring-tokens/desktop-mobile/wam),
[Edge configuration and experiments](https://learn.microsoft.com/en-us/deployedge/edge-configuration-and-experiments),
and the security role of [component updates](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-policies/componentupdatesenabled).
WebView2 also has Microsoft's own diagnostic-data practices, described in that
privacy documentation; these allowed hosts are not a universal inventory
of every runtime version's network behaviour.

The release policy permits and discloses this narrowly scoped runtime traffic. This is not a zero-outbound promise. The audit
uses local DNS names correlated with socket addresses, which cannot prove the
requested host on a shared address. It retains every correlated name and allows
an endpoint only when all names are approved for its port. An approved name
mixed with any other name is inconclusive, even if the other name is another
Microsoft service or is approved only on a different port. Unresolved runtime
destinations are also inconclusive; an endpoint with no approved names is a
violation. See the [network audit](docs/release/network-audit.md) for evidence
and limitations. No upload of Maxed's local game data was established by these
observations. Installed-build acceptance remains in the release checklist.

## Remove data

See [uninstall and manual data removal](docs/release/INSTALL.md#repair-reinstall-and-removal).
Uninstall retention remains a release acceptance check, not a completed observation.
