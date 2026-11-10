# Kept scrub notices

The scrubber keeps exact Oryx `stringlist.*.death.N` / `.killed.N` grammar
(redacting `KILLER`) and `s.dungeon_opened_by` (pseudonymizing `player`).

Every structural key and localization value must use its literal wire spelling,
without JSON escapes. For death notices this covers `k`, `t`, `player`, `enemy`,
`level` and `s.death`; for portal notices, `k`, `t`, `player` and
`s.dungeon_opened_by`; for Oryx, `k`, `t`, `KILLER` and the full stringlist key.
Any escaped spelling makes the entire message an unknown notice and fully filled.
Portal and Oryx structural keys and localization values require literal spelling. Because
portal/Oryx parsing is shared with the engine, live recognition rejects these
escaped structural spellings too. Live `s.death` recognition is different: the
engine keeps its own, unchanged parser (`maxed-engine` `engine/notices.rs`), which
decodes escapes. The strict death grammar here is a scrubbing guard only. An
escaped death notice, which the game does not send, is fully filled in a
scrubbed capture, so its replay can't cancel that player's departure, while live
capture still can. Escaped identity values remain supported by
their separate redaction path. Other exact-string public allowlists do not
decode JSON structure and have no equivalent escaped-key acceptance path.

For PlayerDeath notifications it also keeps the exact `s.death` template:

- Player names use the display text before the first comma. NAME_STAT and
  notice display names share a length-preserving pseudonym namespace; suffix
  contents are filled separately, preserving every comma. Suffix text is
  treated as private, not as an allowlisted structural token.
- Death `player` matches NAME_STAT display names exactly, including decoded
  JSON escapes and observations later in the recording. A case-only mismatch
  gets a distinct pseudonym and remains unmatched on replay. Portal matching
  remains ASCII case-insensitive. Multiple NAME_STAT display spellings with
  the same ASCII-lowercase key refuse certification rather than merge exact
  identities. Deaths without even a case-insensitive observed display-name
  candidate still refuse certification.
- Retained fields are checked against individual display-name and suffix
  components as well as the full original identity.
- `enemy` and `level` remain unchanged game data. Values must be bounded JSON
  strings; level must contain unsigned decimal digits fitting `u16`.
- Unknown, missing, duplicate or extra fields, wrong value types and malformed
  JSON shapes are filled like unknown notices. The known trailing-comma wire
  form is supported. Replacement preserves byte lengths and JSON structure.
- Retained data is still checked for identity/credential collisions, including
  decoded JSON escapes in enemy/level. Identity matching is ASCII case-insensitive,
  like NAME_STAT alias matching; credentials retain exact-byte matching. Such
  collisions refuse certification even when the same value also occurs in chat.
- Player-text coincidences in enemy/level are admitted and listed as
  `PublicLiteralCoincidence`, using the existing public-literal policy. Chat
  matching in these fields is also ASCII case-insensitive: `cube` within
  `Cube Deity`, or exact `20` against level `20`, does not block certification.
  The existing short-value rule still requires whole-value equality below four
  bytes; longer values also receive substring checks. Diagnostics name the
  field and secret class, not the source text.
- Templates are effect-specific: code 7 with a portal template and code 8 with
  a death template are fully filled, including their keys.

## Opaque item strings

`UNIQUE_DATA_STRING` in NEWTICK and UPDATE contains comma-separated URL-safe
base64 item blobs, retained for enchant/export parity. Only commas and the
field edges are item boundaries: `-`, `_` and `=` belong to the item alphabet.
The delimited-match and interior-coincidence checks share this item definition.

- A whole item equal to a secret, including whole-field equality, refuses.
- Player text and non-numeric identity values shorter than four bytes are
  otherwise ignored, following the ordinary retained-field short-value scan.
  Thus chat `f`, `ab` and `xyz` do not collide with `_f_`, `-ab-` and `xyz=`;
  digit-only chat `1` and `20` does not collide with `AA1b_20_CC`. These ignored
  substrings are not reported as coincidences; avoiding a substring scan keeps
  common one-letter chat from flooding the summary.
- Four-to-seven-byte alphanumeric identity/player-text secrets containing at
  least one letter may coincide strictly inside a base64-alphabet item. Every
  occurrence must have item bytes on both sides. `_Qzxy_` and `-Qzxy-` qualify;
  `,Qzxy,`, `Qzxy_` and `_Qzxy` do not. Items with non-base64 bytes cannot use
  this allowance. Every admitted field/secret-class coincidence is listed as
  `OpaqueTokenCoincidence`, without printing secret values.
- Credentials and all-digit identity values never receive this allowance. Only
  those secret classes refuse on any substring below four bytes. This is
  stricter than the earlier punctuation-bounded rule, so a 1–3 byte credential
  can now cause a new, fail-closed refusal inside an item; digit-only chat
  follows the short player-text rule above. Secrets of eight bytes or more also
  continue to refuse on any substring match. Other retained fields keep their
  existing rules.

The expected summary change is that short chat and non-numeric name fragments no
longer produce false refusals inside these items, and qualifying four-to-seven-
byte matches next to `-`, `_` or `=` produce listed coincidences. This is an
opaque-data coincidence rule, not a decoded item schema or publication approval.

## Profile and existing fixtures

The profile SHA-256 already covers `defs.rs`, `reader.rs`, `privacy.rs`,
`notices.rs`, `scrub.rs` and `scrub/identities.rs`; changing these rules changes the hash
automatically. The scrub tests verify newly written metadata against those
sources. The metadata/source hash equality test is the current profile guard.
Committed fixture replay tests do not require existing captures to match
today's profile hash. Their original provenance remains valid historical
evidence, not certification by this newer grammar. The profile does not
retroactively change any fixture pcap.

Shaitan still requires a reviewed local re-scrub. The URL-safe base64 delimiter
false positive has synthetic regression coverage; the owner's private recording
has not been re-scrubbed here. No committed fixtures or their expected replay
results change with this fix; see
[known limitations](../known-limitations.md#active-limitations). Its current
filled death notice cannot identify the victim, producing one Left before Victory.
The replay test detects whether the wire notice has a victim: it requires that
documented one until re-scrub, then requires zero. Left after Victory is
always zero.
