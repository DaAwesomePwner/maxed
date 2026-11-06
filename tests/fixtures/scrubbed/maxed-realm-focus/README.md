# Realm quest-focus regression fixture

A scrubbed owner capture of one realm visit (2026-10-03, about 2.5 minutes). It
reproduces the stale-focus report: the first quest-target fight (Maze Minotaur) stays the focused
"Live realm fight" for the whole visit while the player moves on to Man-eating
Barnacle, Legion General and Lost Sentry. It also carries evidence for boss classification, quest priority
and kill detection.

## Provenance

- **Approved for publication:** owner (maintainer), 2026-11-05; see the [recorded approval and review basis](../../../../docs/development/fixtures.md#publication-approval-2026-11-05). Capture bytes and historical limitations are unchanged.

- Captured live by the app built from `b094bd9` (code identical to
  `11026ce`).
- The visit is exactly one TCP connection (the realm map, from its SYN to the
  map change). It was cut from the session recording by flow. Nothing inside
  the connection was trimmed.
- Certified with `maxed-cli scrub realm.pcap realm-visit.pcap`, which reported
  `verified=true provisional=false pseudonymized=229 filled_unclassified=846`:
  - access, client and user tokens, keys and account IDs are filled;
  - player, guild and pet names are pseudonymized;
  - chat text and `NOTIFICATION.message` text are filled;
  - MAC and IP addresses are replaced with documentation addresses.
- `maxed-cli export-domain` gives identical output for the original and the
  scrubbed file (85 of 85 rows; only `inputSha256` differs).
- SHA-256 of `realm-visit.pcap` is recorded in `expected.json`.

## Catalog

No catalog is committed, because it is derived from the game client. Either
build one locally with `maxed-cli catalog-root`, or construct a minimal catalog
in the test from the `target_type`, `name` and catalog flags listed in
`expected.json`. The capture-time catalog SHA-256 is recorded there as well.

## What it shows (see `expected.json`)

| Target | Kind observed | Local damage | Kill evidence |
|---|---|---|---|
| Maze Minotaur | quest | 0 | none (`target_hp` 1) |
| Man-eating Barnacle | quest | 24,569 | none (`target_hp` 1) |
| Sword in the Stone? | quest | 0 | none (`target_hp` 1) |
| Legion General | quest | 10,932 | none (`target_hp` 1) |
| Lost Sentry | ordinary | 14,730 | `hp_zero_removed` |

All 84 fights stay open until the map change at the end of the capture.

## Limitations

- Notification and chat text is filled. Realm-event announcement kills, such as
  the Lost Sentry `realm_event_killed` row seen live, do not reproduce from
  this file. Kill evidence that depends on the Victory notification *effect*
  field is not affected, because only `message` is filled.
- The connection begins at a realm reconnect, not at character select. Replay
  recognizes the local player (object 250728, local damage matches the table
  above), but `export-domain` marks every fight `completeCapture=false`,
  whereas the live app marked many of them complete. Do not assert
  `complete_capture` or verified identity from this file alone.


## Focus regression checks

The CLI test `realm_focus` replays this pcap with the documented targets, the
local class type observed in UPDATE, and explicitly synthetic weapon definitions
for observed PLAYERSHOOT types (fixed damage, armor piercing, ten-second lifetime).
These definitions exercise the real local contribution projection without bundling
client assets. They do not reproduce the original damage totals or every hit;
weapon/projectile and lifetime assumptions can limit which contributions resolve.
The test asserts current-quest transitions and the published local-hit timestamps
in `focus-hit-observations.json`, not verified identity or damage parity.
`first_published_local_hit_us` is the first emitted latest-local timestamp, not
necessarily the first raw local hit. Local ENEMYHIT recency uses exact receipt
timestamps in either pairing order; reports without a local receipt retain the
tick time. Regenerate with
`UPDATE_REALM_FOCUS_FIXTURE=1 cargo test -p maxed-cli --test realm_focus` when an
intentional reconstruction change affects that projection.

The UI test `realm-focus.test.ts` builds a sequential engagement scenario from
these target observations and timestamps, with explicitly verified synthetic UI
identity. It checks Maze Minotaur -> Barnacle -> Legion General and separately
covers quest-only events, stale snapshots, local identity guards, idle expiry,
kill evidence, and stable bystander focus. It does not claim end-to-end verified
identity from this reconnect capture. The idle policy changes presentation only;
it does not infer kills or retire engine damage totals.

## Announcement evidence limits

This fixture has filled Oryx TEXT, including the `stringlist.*.death.N` keys
needed to corroborate HP1 removals. The current scrubber preserves validated
announcement keys and redacts KILLER values. A reviewed replacement is needed
to retain that evidence; this fixture cannot recover the lost text. Its pcap and conservative no-inferred-kill expectations are unchanged.
