# Nexus key-pop regression fixture

A scrubbed capture of one short Nexus connection, about 20 s, taken from the owner's smoke test on 2026-10-04, just before the owner entered Lair of Shaitan. Another player pops a **Lair of Shaitan** key, so it exercises the key-pop alert and its dungeon filter.

## Provenance

- **Approved for publication:** owner (maintainer), 2026-11-05; see the [recorded approval and review basis](../../../../docs/development/fixtures.md#publication-approval-2026-11-05). Capture bytes and historical limitations are unchanged.

- Captured live by the app built from `a4f0e08`. Scrubbed with `4eacecf`, which keeps the `s.dungeon_opened_by` key and pseudonymizes the opener consistently with their NAME_STAT.
- The file is exactly one TCP connection, SYN to FIN: 1,030 packets. Nothing inside it was trimmed.
- `maxed-cli scrub` reported `verified=true provisional=false pseudonymized=349 filled_unclassified=40 kept_allowlisted=264`.
- `maxed-cli export-domain` gives identical output for the original and the scrubbed file.
- The scrubbed file was decoded and checked against the 418 names in the original. Only a public portal name ("Lair of Shaitan" in RECONNECT) remains.

## What it shows (see `expected.json`)

- **One `PortalOpened` notification:** `{"k":"s.dungeon_opened_by","t":{"player":"000000007"}}` at `1791143211431886`. The portal is a Lair of Shaitan portal (object type 28057).
- **The opener is not the local player**, so this is the positive case.
- **Expected alerts:**
  - one key-pop alert when Lair of Shaitan is in the filter (or the filter is "All");
  - none when the filter excludes it.

## Limitations

- The connection starts at a Nexus reconnect. Live identity has to be established inside it (CREATE_SUCCESS is present) before a key-pop alert can fire, as the verified-identity rule requires.
- An own-pop case isn't included.
