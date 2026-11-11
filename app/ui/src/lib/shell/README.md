# Persistent shell

The shell owns one native client subscription, route-scoped Home live window, character rail, alert outlet and navigation. Routes replace only main content. History reads wait for store durability; capture and storage failures have independent status. Visual lookup uses catalog fingerprints and local sprite references.

The rail uses 15rem width (12rem below 900px). Its brand, navigation and Settings remain reachable while only the card group scrolls at short heights. Main content scrolls independently. Shared boss/quest/local-hit focus drives the rail and Home; values use supplied evidence and engine timestamps. Shell projects only the character (`characterRailView`); `CharacterRail` selects fight focus once per snapshot (`focusRailView`).

See [UI ownership](../../../../../docs/architecture/ui.md), [runtime](../../../../../docs/architecture/runtime.md) and [preview validation](../../../../../docs/development/ui-preview.md). Browser geometry, DPI, focus and physical input require real browser/Windows testing.
