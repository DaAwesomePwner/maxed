# Synthetic UI preview

From the repository root:

```powershell
npm --prefix app/ui ci
npm --prefix app/ui run dev
```

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
npm --prefix app/ui run test:ui
npm --prefix app/ui run build
rg -n 'LOGS_SYNTHETIC_PREVIEW_ONLY|LOOT_SYNTHETIC_PREVIEW_ONLY|RUNS_SYNTHETIC_PREVIEW_ONLY|HOME_V2_SYNTHETIC_PREVIEW_ONLY|Synthetic Moonlit Sanctum|home-preview-character|Invented Home preview character' app/ui/dist
```

The final search must have no matches (exit code 1). jsdom validates content and state, not geometry.

Open `http://127.0.0.1:1420/?preview=nexus&variant=stress#/` in a real browser. In DevTools Console:

```javascript
await (await import('/tests/preview/layout-acceptance.ts')).runHomeAcceptance();
```

Expected: `PASS Home v3 acceptance: 16 real-browser mounts at 1100/900 × 760`, with nonzero measured box counts for every case/width/debug combination. Prove the checker can reject a real overflow:

```javascript
try {
  await (await import('/tests/preview/layout-acceptance.ts')).runHomeAcceptance({ injectOverflow: true });
  throw new Error('Probe unexpectedly passed');
} catch (error) {
  if (!/Overflow|overflows|Escapes parent/.test(String(error))) throw error;
  console.info('PASS overflow detector rejected the 600px stat cell');
}
```

Rerun without injection for acceptance. Do not substitute mocked geometry or broaden exclusions to hide failures. Intentional ellipsis needs a title containing the full value; character names, stats and meters must fit. Main vertical scrolling is allowed; card overflow and horizontal page scrolling fail.

## Visual and interaction checks

At 1100×760 and 900×760 CSS pixels, DPR 1 and 100% zoom, inspect normal and stress modes plus Realm busy. Capture comparable screenshots of the application itself. Check long synthetic names, negative boosts, missing fame goals, Divine pips, large populations, low local ranks, dense bags, empty/error states and unavailable values. Busy Realm retains an earlier focused encounter; its final snapshot population is not necessarily the displayed fight's participant count.

Verify rail navigation stays reachable, only its cards scroll at short heights, and the main scroller is independent. Check keyboard focus, Back/Forward, stat tooltips, boss focus, loot pagination, session/day selection and reduced motion during an actual event-triggered effect. Synthetic missing art cannot validate real portal/sprite mappings: check those locally against the active extracted catalog without publishing game art.

For startup debug state, set `localStorage.setItem('maxed.debug', '1')`, reload each stress screen and open evidence details; verify debug caveats. Then remove the key and reload; verify they are absent while values and layout remain usable. Browser storage before reload tests a different path from toggling an already mounted component.

Windows WebView2, 100%/150% display scale, physical mouse side buttons, screen reader behavior and release startup remain separate live checks. Record the tested commit, viewport, scenario, observed result and sanitized screenshots; do not reuse prior preview passes as current acceptance.

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
