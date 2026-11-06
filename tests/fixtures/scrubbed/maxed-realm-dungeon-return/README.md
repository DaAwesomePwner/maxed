# Realm → Ocean Trench → same realm regression fixture

A scrubbed capture of three consecutive connections from the owner's realm smoke test on 2026-10-04, about 2.8 minutes in total:
1. a realm visit with Goblin Patriarch, Cube Deity, four Pentaract Towers, Daughter of Limon, Maiden of the Sea and Hermit God;
2. a roughly 7-second Ocean Trench visit;
3. a return to **the same realm instance**, where The Plague Doctor is fought.

It covers:
- **Kill evidence:** Oryx `.death.` announcements together with removal at HP 1 (or HP 428) are kills.
- **Realm continuity:** the realm visit continues across the dungeon trip.
- **Player activity window:** very short Pentaract glimpses, and a late join on The Plague Doctor.

The third connection alone is also committed as `../maxed-realm-plague-doctor/`.

## Provenance

- **Approved for publication:** owner (maintainer), 2026-11-05; see the [recorded approval and review basis](../../../../docs/development/fixtures.md#publication-approval-2026-11-05). Capture bytes and historical limitations are unchanged.

- Captured live by the app built from `a4f0e08`. Scrubbed with `4eacecf`.
- Each connection is complete, from SYN to FIN, cut by flow (server port 2050) from the session recording: 11,837 packets in total. Nothing inside a connection was trimmed.
- `maxed-cli scrub` reported `verified=true provisional=false filled_unclassified=665 pseudonymized=282 kept_allowlisted=978`. It admitted exactly **two "short opaque token interior" coincidences** inside `NEWTICK` `UNIQUE_DATA_STRING`, one identity and one player-text. One of them is a 4-character player name that occurs by chance inside an unrelated 519-character item-data string. Under the short opaque-interior coincidence rule that substring stays in the file; no other player-identifying value remains.
- `maxed-cli export-domain` gives identical output for the original and the scrubbed file (422 of 422 rows; only `inputSha256` differs).
- The scrubbed file was decoded and checked against the 220 player, guild, pet and account names and killer names in the original. Only the admitted coincidence above, and public portal and map names ("Ocean Trench" in MAPINFO and RECONNECT), still appear.
- All six Oryx announcement keys are kept: `Goblin_Patriarch`, `Cube_Deity`, `Pentaract`, `Daughter_of_Limon`, `Hermit_God` (`.killed.`, KILLER redacted) and `The_Plague_Doctor`.

## What it shows (see `expected.json`)

| Boss | Last HP before removal | Oryx text vs removal |
|---|---|---|
| Goblin Patriarch | 1 | `.death.`, 6 µs before |
| Cube Deity | **428** | `.death.`, 7 µs before |
| Pentaract Towers (4) | 0 | one group `Pentaract.death` after the last tower |
| Daughter of Limon | 1 (3 samples) | `.death.`, same µs |
| Hermit God | 0 | `.killed.`, about 100 ms before |
| The Plague Doctor | 1 (3 samples) | `.death.`, 1 µs before |

- **Realm identity:** connections 1 and 3 both report `NexusPortal.Zephyr`, seed 1486421971, gameOpenedTime 1791146155. Ocean Trench has its own seed and open time.
- **Short window:** one Pentaract Tower fight lasts 0.4 s. Its historical whole-fight rate is 16,782 DPS for the top player; current per-player DPS is unavailable for activity shorter than three seconds.

## Limitations

- **Each connection starts at a reconnect, not at character select.** Don't assert `complete_capture` or verified identity from this file alone.
- Notification and chat text is filled. Only Oryx announcement keys and `s.dungeon_opened_by` keys are kept.
