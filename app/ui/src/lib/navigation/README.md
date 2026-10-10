# Navigation and P4 integration

`createRouter(browserHistory(window), features, scope)` owns one document's history. The browser stack is authoritative; the 50-entry in-memory cache only accelerates restoration. Each browser record retains its small `RunFilter`, scroll offset and row identity after cache eviction. No localStorage/sessionStorage or page rows are retained. `navigate`, `back`, `forward`, `save`, `setScope` and `dispose` are the consumption API. A new navigation truncates Forward. Scope changes retire old stream/demo entries; native traversal into retired entries replaces them with a fresh Home entry, never restores their filters or addresses.

Routes are hash addresses, including `/runs/:run/fights/:fight/players/:object`, standalone `/fights/:fight`, and Loot routes `/loot`, `/loot/item/:type`, `/loot/stats` and `/loot/collection` (with `/loot/:type` retained as a legacy item address). IDs are percent-encoded; malformed/unsupported addresses show a recoverable frame. Breadcrumbs follow route ancestry, not visitation order. See [Loot data ownership and interpretation](../../../../../docs/architecture/ui.md#loot).

For a list, save its generated `RunFilter` with `router.save({ filters })` on control changes. Mark each row/link with `data-row-id`; scroll/focus events update only the entry and bounded cache immediately. Browser `replaceState` is trailing-debounced until 250 ms without another save, avoiding frame-rate History API writes. `navigate`, `back`, and `forward` synchronously flush pending state before departure; Shell also saves/flushes on visibilitychange and pagehide. Native mouse traversal reads the fresh cache first and cancels the outgoing timer so it cannot overwrite the destination. Disposal clears the timer and lifecycle listeners.

Pass `rowsReady(key)` to Shell: this promise must resolve only after the destination's query and rows render (including its empty/error state). The shell waits for a Svelte tick before requesting readiness and again before restoring. Stale transitions cannot apply focus or scroll. Missing rows focus the h1 and populate an always-mounted, initially empty polite live region; offsets clamp to the remaining scroll extent. New destinations focus h1 only after explicit navigation. Snapshot/stream updates do not move focus.

`tests/shell/fixtures/FixtureApp.svelte` is an executable route consumption example, using a supplied synthetic run and the story order header → timeline → fights → loot. Its RouteFrame fallback receives the real client, saved router state, `onStateChange`, `onRowsReady` and store, so the four Loot views render from synthetic loot queries in `tests/shell/fixtures/loot.ts`; Logs views render from synthetic chat and instance-event queries in `tests/shell/fixtures/logs.ts`; it supplies no production history queries. `navigate(route, { anchor: 'fight' })` targets a focusable `data-anchor="fight"` inside main. Returning to the list restores the list record instead of the anchor.

Alt+Left/Right are handled once, with default prevented; inputs, composition, repeats, already-prevented events and additional modifiers are preserved. Mouse buttons remain owned by native browser/WebView2 history by default. `installNavigationInput(..., 'fallback')` installs a prevented-default mouseup handler for JS buttons 3/4 **only after native release testing proves the platform does not handle them**. Do not enable both owners. Physical mouse 4/5 and Alt-key release WebView2 validation is still required.

Run `npm.cmd --prefix app/ui run test:ui -- tests/navigation tests/shell` and both check scripts. Windows 100%/150% scale, actual focus visibility, device input and visual wrapping cannot be established by jsdom.

Logs routes include `/logs`, `/logs/chat/{guild,party,whisper}`, `/logs/run/:runId`,
`/logs/keys`, `/logs/events`, `/logs/events/run/:runId` and `/logs/alerts`.
With `logs` enabled, `/alerts` redirects to `/logs/alerts`; production enables
the flag. Logs filters and paging stay in navigation
entry state. See [Logs ownership and refresh behavior](../../../../../docs/architecture/ui.md#logs).

Logs also accepts `/logs/chat/{guild,party,whisper}/run/:runId`,
`/logs/chat/whisper/:partner` and `/logs/from/:sender`. Names are percent-decoded,
nonempty, at most 32 UTF-8 bytes and contain no control characters. Channel links
retain run scope; the Chat header tab always links to unscoped `/logs`.

`logsPosition` owns independent `chat`, `whisper`, `keys` and `events` slots and
one `range` shared by Key pops, Events and Alerts. Legacy portal-only `events`
state migrates to `keys`; invalid slots reset independently. New Logs entries carry
filters and range from the outgoing Logs/Alerts entry, reset cursors and previous
pages, and clear selections. Sender routes seed only Chat from its default filter;
partner routes seed only Whisper. A sender seed does not carry to the next entry.
Alerts tile, kind, search and Hide key pops intentionally remain per-entry; only
the shared range carries into a new Alerts entry. Back/Forward restores the saved entry unchanged.

Shell counts matching accepted engine envelopes for the active entry and supplies
`pendingCount` through RouteFrame and Logs. Views acknowledge a successfully applied
read with `onHistoryApplied(entryKey, required)`. The counter removes only occurrences
through that checkpoint; later occurrences remain pending. Readiness for navigation
continues to use the separate once-per-entry `onRowsReady` contract.
