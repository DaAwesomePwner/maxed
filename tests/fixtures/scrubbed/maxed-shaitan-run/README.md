# Lair of Shaitan run regression fixture

A scrubbed capture of one complete Lair of Shaitan run, taken from the owner's smoke test on 2026-10-04. The run lasts about 2 minutes 10 seconds, from entry to exit. It records the pre-fix behavior:
- **Fight closure:** the boss fight stays open after Victory;
- **Departures:** departures after Victory are counted as "Left";
- **Consumable classification:** a consumable labelled UT wins the best bag.

It also supplies evidence for personal-best eligibility through estimated local damage sources.

## Provenance

- **Approved for publication:** owner (maintainer), 2026-11-05; see the [recorded approval and review basis](../../../../docs/development/fixtures.md#publication-approval-2026-11-05). Capture bytes and historical limitations are unchanged.

- Captured live by the app built from `a4f0e08`.
- The run is exactly one TCP connection, from its SYN at the dungeon entry to its FIN after the exit: 6,585 packets. It was cut by flow (server port 2050) from the session recording. Nothing inside the connection was trimmed.
- Certified with `maxed-cli scrub`, which reported `verified=true provisional=false pseudonymized=58 filled_unclassified=924 kept_allowlisted=325`:
  - access, client and user tokens, keys and account IDs are filled;
  - player, guild and pet names are pseudonymized;
  - chat text and `NOTIFICATION.message` text are filled;
  - MAC and IP addresses are replaced with documentation addresses.
- `maxed-cli export-domain` gives identical output for the original and the scrubbed file (38 of 38 rows; only `inputSha256` differs).
- The scrubbed file was decoded and checked against every player name seen in this run. None are present.
- SHA-256 of `lair-of-shaitan-run.pcap` is recorded in `expected.json`.

## Catalog

No catalog is committed, because it is derived from the game client. Either build one locally with `maxed-cli catalog-root`, or construct a minimal catalog in the test from the types and flags listed in `expected.json`. The capture-time catalog SHA-256 is recorded there as well.

## Historical observation (see `expected.json`)

| Fact | Value |
|---|---|
| Boss | Shaitan the Advisor (28058), `is_boss`, max HP 1,080,000 |
| Victory (`completed_at`) | `1791143305741000`; kill evidence `victory_quest_target`, non-provisional |
| Boss HP at Victory | 48,167 (Shaitan disappears at low HP; it never reaches 0) |
| Boss fight end, as observed live | `1791143352149000` (map exit), about 46 s after Victory. This is the historical late-closure symptom. |
| Run | cleared, 35 fights, peak party 17, `complete_capture: true` |
| Party deaths | 1, before Victory |
| Departures, as observed live | 15, **all after Victory**. Under the owner's rule the run's Left count should be 0. Post-Victory departures must not count. |
| Bag 1 (at Victory) | T12 Wand of Recompense, T12 Acropolis Armor, Potion of Life, ATT and VIT potions, a relics stack |
| Bag 2 | Greater Health Potion (2795; catalog tier "UT", slot_type 10), two soulbound potions and a shard. The consumable label must not make this the best equipment bag. |
| Local damage sources | ability (589) and weapon (588), both `estimated: true` with recorded enchants. They block personal-best eligibility. |

## Limitations

- **The connection starts at the dungeon reconnect, not at character select.** Replay recognizes the local player (object 271 in this file), but the boss fight is `completeCapture=false`, as it was live. Don't assert `complete_capture` or verified identity from this file alone. The personal-best eligibility rule needs engine-level tests as well.
- `export-domain` doesn't emit fight end times, kill evidence or departures. The "observed live" values above come from the owner's live database for this run. Regression tests should replay through the engine and assert those facts directly.
- Notification and chat text is filled. The Victory notification is still visible through its `effect` field. Only `message` is filled.
- The filled PlayerDeath message also prevents replay from matching the victim: the notice effect survives at `1791143260982000`, but replay emits an extra pre-Victory departure at `1791143261185000`. The pre-Victory departure regression checks that all 15 documented post-Victory departures disappear, and records this extra event separately. An unchanged replay cannot assert a total Left count of zero; synthetic engine tests cover death cancellation without inventing the missing identity.

## Current regression status

Victory closure, exclusion of post-Victory departures, consumable classification and reconstruction/PB eligibility fixes are implemented and covered by regression tests. The historical live values above describe the original capture build, not current expected behavior. See [personal-best rules](../../../../docs/architecture/personal-bests.md).

A reviewed re-scrub is still needed to preserve the death victim; a URL-safe base64 delimiter false positive blocks scrub certification. Until then, retain the documented extra pre-Victory departure. Real capture and PB alert acceptance remain unverified by this reconnect-only fixture.
