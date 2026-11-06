# Synthetic UI preview

## Storage and history upgrade preview

Open `?preview=storage` for the real Settings route with synthetic storage data.
Use `&variant=pending`, `cancelled`, `pressure`, `complete`, `backups`, `unknown`, or
`superseded` to inspect backup-first progress, Resume, pressure, last success,
initial/rolling backup deletion, unavailable counters, and safe failure copy.
`unknown` also includes a byte count above JavaScript's safe integer range.

Open `?preview=upgrade&variant=required` for the legacy Setup prompt. Other variants
are `low-space`, `unknown`, `backing-up`, `upgrading`, `failed`, and `ready`.
Storage defaults to `#/settings`, upgrade to `#/setup`; supplied hashes are preserved.
All commands use the synthetic fixture transport. Controls are interactive: review
and confirm optimization or upgrade, then use the synthetic controls to advance
the job or fail a backup. Reload resets state. Mounting only queries status.
The development-only marker `STORAGE_SYNTHETIC_PREVIEW_ONLY` must be absent from
the production build. Run content tests with:

```powershell
npm --prefix app/ui run test:ui -- tests/preview/storage-preview.test.ts
```

At 1100×760 and 900×760, inspect wrapping, keyboard access, backup deletion review,
upgrade space refusal and navigation away/back during requested work. Content tests
do not establish browser geometry, native capture, or installed upgrade acceptance.

From the repository root:

```powershell
npm --prefix app/ui ci
npm --prefix app/ui run dev
```

The default port is 1420. If it is occupied, choose a free port and start your own
server from the repository root with this cross-platform command, replacing
`<free-port>` with the port you chose:

```text
node app/ui/node_modules/vite/bin/vite.js app/ui --host 127.0.0.1 --port <free-port> --strictPort
```

Never touch another developer's server. Stop your own server when finished.
The example URLs below use port 1420; substitute your server's port throughout.

Open `http://127.0.0.1:1420/?preview=dungeon`, `?preview=realm` or `?preview=nexus`. Add `&variant=stress` for each screen or `&variant=busy` for the larger workloads. Use the Vite server directly; no separate mockup renderer is needed.

The dev-only harness mounts the real Shell and Home routes through a synthetic transport. It supplies ordered events and snapshot watermarks, an in-memory history context and invented art/fallbacks. It never starts native capture or accesses user recordings, game art or saved data. Native controls are not supplied; use the Runs preview below for run/fight drilldowns. Wait for history and catalog rendering to settle; reload to reset.

## Runs preview

Open `http://127.0.0.1:1420/?preview=runs` for the real Runs routes backed by synthetic saved history. With no hash it starts at `#/runs`; an initial hash is preserved. Example fresh-load URLs:

- `http://127.0.0.1:1420/?preview=runs#/runs/preview-run-0`
- `http://127.0.0.1:1420/?preview=runs#/runs/preview-run-0/fights/preview-run-0-fight-0`
- `http://127.0.0.1:1420/?preview=runs#/runs/preview-run-0/fights/preview-run-0-fight-0/players/1`

The nine runs include died (`preview-run-2`), left (`preview-run-5`), open (`preview-run-7`, no end or completion) and partial capture (`preview-run-8`, stream gap and unknown fame) cases. Every run has four boss summaries and drilldowns, seven synthetic players with two damage sources each, nine ordinary targets, six bags and a timeline. Add `&variant=stress` before the hash for 64 bosses, 85 players, 128 bags and long synthetic names. These are content fixtures, not live-capture evidence. They use the current DTOs; later redesign tasks add richer saved-history fields.

Initial-hash regression: jsdom startup checks preserve the Runs list, run, fight and player hashes through the final snapshot and history polling. The earlier scratch report of a fresh-load redirect was not reproduced. A separate reset to `#/` is reproducible when an already mounted app receives an unowned history entry: the shared router deliberately discards foreign/retired entries in its `popstate` handler (`app/ui/src/lib/navigation/history.ts`). The preview client does not change the hash or stream during startup. Navigate via the rail and in-app links after loading, or use a fresh page load for the URLs above; editing only the address-bar hash can invoke that shared reset. Production routing is unchanged. Interactive browser verification remains a separate coordinator check.

## Automated content and real-browser geometry

```powershell
npm --prefix app/ui run check
npm --prefix app/ui run check:tests
npm --prefix app/ui run test:ui -- tests/preview/preview.test.ts
npm --prefix app/ui run test:ui -- tests/preview/runs-preview.test.ts tests/runs/breadcrumbs.test.ts
npm --prefix app/ui run test:ui -- tests/preview/party-preview.test.ts
npm --prefix app/ui run test:ui
npm --prefix app/ui run build
rg -n 'STORAGE_SYNTHETIC_PREVIEW_ONLY|PARTY_SYNTHETIC_PREVIEW_ONLY|LOGS_SYNTHETIC_PREVIEW_ONLY|LOOT_SYNTHETIC_PREVIEW_ONLY|RUNS_SYNTHETIC_PREVIEW_ONLY|HOME_V2_SYNTHETIC_PREVIEW_ONLY|Synthetic Moonlit Sanctum|home-preview-character|Invented Home preview character' app/ui/dist
```

The final search must have no matches (exit code 1). jsdom validates content and state, not geometry.

Open `http://127.0.0.1:1420/?preview=nexus&variant=stress#/` in a real browser. In DevTools Console:

```javascript
await (await import('/tests/preview/layout-acceptance.ts')).runHomeAcceptance();
```

Expected: `PASS Home v3/v4 acceptance: 44 real-browser mounts at 1100/900 × 760`, with nonzero measured box counts for every case/width/debug combination. This includes the three saved recap scenes, the live v4 screens, Nexus and the legacy-party-line variant, as well as all existing v3 cases. Prove the checker can reject a real overflow:

```javascript
try {
  await (await import('/tests/preview/layout-acceptance.ts')).runHomeAcceptance({ injectOverflow: true });
  throw new Error('Probe unexpectedly passed');
} catch (error) {
  if (!/Overflow|overflows|Escapes parent/.test(String(error))) throw error;
  console.info('PASS overflow detector rejected the 600px stat cell');
}
```

The checker also compares visible sibling Home cards, including cards spanning
grid rows. More than 1px of intersection on both axes fails, even when each
card's content fits inside its own box. Prove this check rejects an overlap:

```javascript
try {
  await (await import('/tests/preview/layout-acceptance.ts')).runHomeAcceptance({ injectOverlap: true });
  throw new Error('Probe unexpectedly passed');
} catch (error) {
  if (!/Sibling cards overlap/.test(String(error))) throw error;
  console.info('PASS overlap detector rejected the 20px Realm card overlap');
}
```

Both probes are confined to disposable acceptance iframes. Rerun without injection for acceptance. Do not substitute mocked geometry or broaden exclusions to hide failures. Intentional ellipsis needs a title or item hover card containing the full value; character names, stats and meters must fit. Main vertical scrolling is allowed; card overflow, sibling card overlap and horizontal page scrolling fail.

Two layout-acceptance exclusions are intentional: SVG descendants use SVG
geometry rather than HTML scroll boxes, so only the outer `<svg>` is measured;
textless boxes with positive border boxes can have zero-width content boxes
(for example, border-only timeline markers), so their border geometry is
measured instead of requiring a positive client width. These boxes still must
fit inside their parents.

## Home v4 previews

The following URLs explicitly enable `homeRedesign`, matching the production
default. The v3 preview modes explicitly disable it. Each mounts the real Shell
and Home route:

- `?preview=home-v4-dungeon`: three learned bosses, a live nine-player fight with
  self at rank 2, a usual for each boss and a 23-run dry streak.
- `?preview=home-v4-realm`: a live 37-player event with self at rank 9, top six
  plus self, realm score and the visit events log.
- `?preview=home-v4-realm&variant=legacy`: an ended saved fight without party
  bucket fields, showing “Party average line not recorded for this fight”.
- `?preview=home-v4-nexus`: existing Session/Today content beneath the new HUD.
- `?preview=home-v4-recap`: a live run ends through the fixture transport, so
  Shell reads the saved dungeon recap: three bosses, a BEST DPS record, PB clear,
  three runs in a row, clear #38, a white ending a 23-run streak and eight players.
  Its realm → Castle → run chain includes four item sprites on the final step.
- `?preview=home-v4-recap-died`: a saved death with a catalog-resolved synthetic
  killer and 12 observed HP samples with one gap.
- `?preview=home-v4-recap-realm`: five killed events, two announced-only events
  and an inferred chain into the Castle.

The three recap scenes start on Nexus with a compact recap-ready banner. Choose
View recap to inspect the full saved recap, then Back to return to Nexus. Last
recap remains available after Back, banner dismissal, expiry or area entry.
The banner expires after 15 seconds of visible, mounted Home time and pauses on
hover/focus; leaving Home or hiding the window pauses the countdown too.

All seven modes have the complete session HUD and synthetic history callbacks.
Live fight buckets include non-local damage and player-seconds for the party
line; an older ended dungeon fight omits them. Names and data are invented;
no game assets, capture or persisted user data are required.

Run the acceptance command once from any preview URL on the local Vite origin;
it covers all mounts:

```javascript
await (await import('/tests/preview/layout-acceptance.ts')).runHomeAcceptance();
```

The checker measures 1100×760 and 900×760, with debug off/on, and exercises the
whole-run meter, each Nexus banner, all three View recap/Back flows and the
retained Last recap control. Each recap table must satisfy
`scrollWidth <= clientWidth + 1` at both widths; at 900px its cards must stack.
The returned rows include the measured table widths. A separate unit test
retains the saving placeholder, without adding another scene.

For phase-2 acceptance, open each recap URL on your own server's origin using
the free-port guidance above. Do the overflow probe once and finish with a clean
run. Do not click Copy recap image during
layout acceptance: these scenes require no clipboard output or game sprites.

## Visual and interaction checks

At 1100×760 and 900×760 CSS pixels, DPR 1 and 100% zoom, inspect normal and stress modes plus Realm busy. Capture comparable screenshots of the application itself. Check long synthetic names, negative boosts, missing fame goals, Divine pips, large populations, low local ranks, dense bags, empty/error states and unavailable values. Busy Realm retains an earlier focused encounter; its final snapshot population is not necessarily the displayed fight's participant count.

Verify rail navigation stays reachable, only its cards scroll at short heights, and the main scroller is independent. Check keyboard focus, Back/Forward, stat tooltips, boss focus, loot pagination, session/day selection and reduced motion during an actual event-triggered effect. Synthetic missing art cannot validate real portal/sprite mappings: check those locally against the active extracted catalog without publishing game art.

For startup debug state, set `localStorage.setItem('maxed.debug', '1')`, reload each stress screen and open evidence details; verify debug caveats. Then remove the key and reload; verify they are absent while values and layout remain usable. Browser storage before reload tests a different path from toggling an already mounted component.

Windows WebView2, 100%/150% display scale, physical mouse side buttons, screen reader behavior and release startup remain separate live checks. Record the tested commit, viewport, scenario, observed result and sanitized screenshots; do not reuse prior preview passes as current acceptance.

For the item hover card, manually verify in Windows WebView2:

- Placement at all window edges and on rail equipment, with full text visible.
- Placement within a scrolled Loot feed, including scrolling while the card is open.
- The 250 ms hover delay, immediate keyboard focus, and 300 ms warm-swap feel when scanning items.
- Arrow through item-search results: the card follows the selection after the delay.
- Escape, leave and blur dismissal, preserved item links and no added tile tab stops.
- After opening an item from Collection and pressing Back, and after alt-tabbing away and back, no card appears unless the pointer rests on an item or keyboard focus lands on it.
- Real enchant names and descriptions against the locally extracted catalog; synthetic fixtures do not verify these.

## Party preview

Open `http://127.0.0.1:1420/?preview=party` for the real Shell with Party enabled
only in this development harness. It starts at `#/party` when no hash is supplied;
fresh-load deep links are preserved:

- `http://127.0.0.1:1420/?preview=party#/party/runs/synth-party-run`
- `http://127.0.0.1:1420/?preview=party#/party/players`
- `http://127.0.0.1:1420/?preview=party#/party/players/synthalpha`
- `http://127.0.0.1:1420/?preview=party#/party/presets`

The dungeon Live roster has 12 synthetic members: Pass, Fail, Check, Not seen,
Out of view, Left, No damage seen, Mostly trash, Watched, Banned, No name yet and
the local player. Failure reasons and flags can appear together. The saved-run
view adds Left early and removes live presence. Players includes three saved runs
per named non-local player. Presets starts with Synthetic standard and Synthetic
practice, with invented dungeon options and tier labels. Art uses fallbacks.

Add `&variant=stress` before the hash for 85 members. The last 21 have unknown
damage totals and shares, matching the live 64-player totals limit. Marks, notes,
clear and preset edits affect only this preview instance and reset on reload.
The RealmEye action is blocked here; it never launches a browser or makes a request.
Capture and saved user data are not accessed.

At 1100×760 and 900×760, inspect both variants, open a row by keyboard, Close back
to it, filter the roster, follow the saved-run and player-history links, and edit
a preset. Check the roster's horizontal scroller and independent main scrolling.
The module carries `PARTY_SYNTHETIC_PREVIEW_ONLY`; the build leak search above must
exit 1 with no matches. jsdom covers content, navigation and state; it does not
establish browser geometry, WebView2, live capture or the native RealmEye handoff.

## Loot preview

Open `?preview=loot` for synthetic Loot Feed, Item, Stats and Collection data.
The default route is `#/loot`; supplied deep links are preserved. Add `&variant=stress`
for 20 pages of 50 bags, 128 dungeons, 100 chase items and 64-character names.
The default has 66 bags across three local days, 18 dungeons and six chase items,
including unseen items and partial, estimated and outside-a-run observations.
Chase edits stay in memory. No installed game assets or native capture are required.
The DEV-only module carries `LOOT_SYNTHETIC_PREVIEW_ONLY`; it must be absent from `dist`.
These synthetic content checks do not establish native or browser layout acceptance.

## Logs preview

Open `?preview=logs` for synthetic Chat, Key pops, Events and Alerts. The default
address is `#/logs`; supplied deep links are preserved. The harness uses the existing
views until their redesigns land, with the shared Logs header above them.

The default supplies 300 chat lines over three local days, nine whisper partners,
200 events and 120 alerts. NPC, hidden, local and truncated lines, unknown whisper
directions, provisional events and missing alert presentations exercise partial data.
Chat hide settings are mutable in memory (three hidden senders, five keywords,
30-day retention and party storage off). Nothing is written to native storage.

Use `?preview=logs&variant=stress` for 40 pages of 100 chat lines, 200 whisper
partners, 128 dungeons, 64-character names and 1,024-byte messages. No game sprites
are supplied. The module carries `LOGS_SYNTHETIC_PREVIEW_ONLY`, which must be absent
from the production build using the leak search above. Synthetic rendering does not
verify live capture, WebView2 layout or installer behavior.

## Character preview

Open `http://127.0.0.1:1420/?preview=character` on your own Vite server (substitute
its free port as described above). This DEV harness mounts the real Shell using
synthetic Character data, defaults to `#/character`, and preserves supplied hashes
such as `?preview=character#/settings`. Production `character` is on for owner use;
[Windows live acceptance](character-live-acceptance.md#production-enablement)
remains pending.

Use `&variant=normal`, `empty`, `partial`, `stale`, `unknown-weapon` or
`missing-catalog`. Normal is the default. Partial includes an unsupported attack
group, missing percentage maximum, negative-Other evidence and long safe reasons.
Stale labels retained results Last seen; unknown weapon retains its synthetic item
identifier, and missing catalog links to Settings without inventing metadata.
All identities and item names are invented; no game sprites are supplied.

Controls affect fixture callbacks only. Defense 0 and 50 have authored results;
50 is the Task 5 vector (ATT/DEX 75, fixed 100, rate 1, one projectile → hit 150,
8 attacks/sec, DPS 1,200). Other valid defenses return unavailable fixture results.
This is not a TypeScript formula engine. Search for Synthetic to add up to two
catalog comparisons; remove them to return to zero. Try invalid defense, Reset,
Settings and Back/Forward. Native capture, extraction, account and network actions
are unavailable; no observations or calculator controls are persisted.

```sh
npm --prefix app/ui run test:ui -- tests/preview/character-preview.test.ts
npm --prefix app/ui run build
bash -c 'if rg -n CHARACTER_SYNTHETIC_PREVIEW_ONLY app/ui/dist; then exit 1; else s=$?; test "$s" -eq 1; fi'
```

The wrapper must exit 0: `rg` must find no marker (exit 1), not encounter an error.
The dynamic preview import is behind the compile-time DEV guard in `src/main.ts`.
From a real browser on that Vite origin, run:

```javascript
await (await import('/tests/preview/character-layout-acceptance.ts')).runCharacterAcceptance();
```

Expected: `PASS Character acceptance: 24 real-browser mounts at 1100/900 × 760`.
Each returned row names variant, CSS viewport, startup debug state, measured box
count and table client/scroll widths. The harness opens details in debug runs,
checks cards/tables/reasons, overlap, page overflow, independent main/rail scrolling,
reachable Settings, focusability/document tab order and visible exclusions.
Normal and partial are measured with zero and then two comparisons in each mount.
Overflowing main/rail containers must use computed `overflow-y: auto` or `scroll`;
programmatic movement alone does not establish user scrolling. An overflowing rail
must actually move while the main scroller, Settings and page remain stationary.
Main vertical scrolling is allowed; hidden overflow is not acceptance. SVG children
use SVG geometry; the outer SVG viewport is measured. Intentional ellipsis needs
full text in a title or item hover card. Frames and debug storage are restored.

Prove that a real injected overflow is rejected, then finish with a clean run:

```javascript
try {
  await (await import('/tests/preview/character-layout-acceptance.ts'))
    .runCharacterAcceptance({ injectOverflow: true });
  throw new Error('Probe unexpectedly passed');
} catch (error) {
  if (!/Overflow|overflows|Escapes parent/.test(String(error))) throw error;
  console.info('PASS Character checker rejected the injected overflow');
}
await (await import('/tests/preview/character-layout-acceptance.ts')).runCharacterAcceptance();
```

Also prove the checker rejects disabled user scrolling. The rail probe constrains
its card container to 80px to guarantee overflow, then applies `overflow-y: hidden`.
These mutations affect only disposable acceptance frames. Probe each container
separately so a main failure cannot mask a rail failure, then both together:

```javascript
for (const target of ['main', 'rail', 'both']) {
  try {
    await (await import('/tests/preview/character-layout-acceptance.ts'))
      .runCharacterAcceptance({ injectHiddenScroll: target });
    throw new Error('Hidden-scroll probe unexpectedly passed');
  } catch (error) {
    if (!/User scrolling disabled/.test(String(error))) throw error;
    console.info(`PASS Character checker rejected hidden scrolling: ${target}`);
  }
}
await (await import('/tests/preview/character-layout-acceptance.ts')).runCharacterAcceptance();
```

Record the exact tested commit, viewport and scenario with synthetic screenshots
(normal at both widths, missing-catalog and stale at minimum). A headless Chrome
run needs an external kill timeout because Chrome may not exit after capture.
Browser content/geometry does not establish native capture, WebView2 keyboard/DPI,
current-client coefficients or installer acceptance; those remain Windows checks.
