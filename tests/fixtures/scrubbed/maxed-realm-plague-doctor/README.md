# Realm Plague Doctor regression fixture

A scrubbed capture of one realm connection, about 40 s, taken from the owner's realm smoke test on 2026-10-04. The player re-enters a realm instance straight from Ocean Trench and joins The Plague Doctor fight late.
- **Kill evidence:** the boss holds at HP 1 and is removed in the same server tick as Oryx's `stringlist.The_Plague_Doctor.death.0` announcement.
- **Player activity window:** the local player starts hitting 5.5 s after the fight's first observed hit.


## Provenance

- **Approved for publication:** owner (maintainer), 2026-11-05; see the [recorded approval and review basis](../../../../docs/development/fixtures.md#publication-approval-2026-11-05). Capture bytes and historical limitations are unchanged.

- Captured live by the app built from `a4f0e08`. Scrubbed with `dd6cead`, which keeps Oryx `stringlist` announcement keys and redacts `KILLER`.
- The file is exactly one TCP connection, from its SYN at the realm entry to its FIN at the Nexus exit: 2,975 packets. It was cut by flow (server port 2050) from the session recording. Nothing inside the connection was trimmed.
- `maxed-cli scrub` reported `verified=true provisional=false`.
- `maxed-cli export-domain` gives identical output for the original and the scrubbed file (200 of 200 rows; only `inputSha256` differs).
- The scrubbed file was decoded and checked against all 124 player names, guild names, account IDs and killer names in the original. None are present.
- The Oryx death announcement key is kept verbatim.
- SHA-256 of `realm-plague-doctor.pcap` is recorded in `expected.json`.

## Historical observation (see `expected.json`)

| Fact | Value |
|---|---|
| Realm identity (MAPINFO) | `NexusPortal.Zephyr`, seed 1486421971, gameOpenedTime 1791146155. This is the same instance as the owner's earlier visit, before Ocean Trench. |
| The Plague Doctor (34489) | event boss, max HP 236,600, 12 players |
| Terminal phase | three HP 1 samples (decode `i` 6033, 6061, 6086), then removal at `i` 6166 |
| Death announcement | `stringlist.The_Plague_Doctor.death.0` at `i` 6152, the same tick as the removal (1 µs apart) |
| Historical capture without announcement detection | no kill, `target_hp` 1, the fight ended at the map change |
| Current announcement/removal regression | a non-provisional kill (announcement plus removal); the fight ends at the removal |
| Historical whole-target local DPS | 29,250 damage. Stored as 1,097.97 DPS over the 26.6 s fight window. The local player was active 21.25 s, so about 1,376 DPS over their own window. |

## Limitations

- **Only the return visit is included.** The original full-session scrub refusal was subsequently addressed for the separate [realm/dungeon/return fixture](../maxed-realm-dungeon-return/README.md). This capture and its original provenance remain unchanged; use that sibling for multi-visit evidence.
- **The connection starts at a realm reconnect, not at character select.** Don't assert `complete_capture` or verified identity from this file alone.
- Notification and chat text is filled. Only Oryx announcement keys are kept.

Current product DPS uses each player's accepted active window. Per-player DPS is unavailable (null) when that player's active window is shorter than three seconds. The stored whole-fight rate above is historical. The announcement/removal detector is implemented. Live acceptance and complete identity/capture must not be inferred from this reconnect-only input.
