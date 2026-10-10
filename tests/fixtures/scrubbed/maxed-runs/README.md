# Maxed Runs synthetic acceptance fixtures

These are byte-for-byte copies of the already admitted `maxed-demo` and
`maxed-home` inputs. Every original packet byte was invented by
`crates/maxed-cli/src/demo.rs`. No private recording, owner export, account
request, external asset, or live connection was used. Identities are only
DemoHero / DemoAccount and DemoAlly / DemoAllyAccount. The pcap network endpoints
are RFC 5737 documentation addresses. No HELLO or credentials are present.

`manifest.json` pins all five input/output SHA-256 values and the generator
itself. Its numeric expectations come from the admitted story design, not a
normalization of test output. `generate.py` checks all inputs, its own pin,
existing output hashes, and the expected manifest before any write. A mismatch
requires explicit provenance review and repinning; generation cannot silently
replace changed data. Missing output files can be regenerated from admitted
inputs. `validate.py` also runs three isolated negative controls: changing an
input, output, or generator causes refusal without writing any file.

```powershell
python tests/fixtures/scrubbed/maxed-runs/generate.py
python tests/fixtures/scrubbed/maxed-runs/validate.py
cargo test --locked --offline -p maxed-app history_replay_tests -- --nocapture
```

When the native test output is saved, `validate.py --native-log <path>` verifies
that all five named tests actually ran and passed and all four corpora emitted
terminal measurements. Zero discovered tests is never accepted as evidence.

## Transformations and independent expectations

| File | Source | Packet/time transformation | Expected saved run |
| --- | --- | --- | --- |
| `demo.pcap` | `maxed-demo/story.pcap` | None, 49 whole frames | Starts 5,000,000 us; Cleared; 300 damage; null DPS (under three seconds); one bag |
| `complete.pcap` | `maxed-home/complete.pcap` | Copy; Home shifted original clock by 1,772,949,595 seconds; 49 frames | Starts 1,772,949,600,000,000 us; Cleared; 300 damage; null DPS (under three seconds); one bag |
| `disconnected.pcap` | `maxed-home/disconnected.pcap` | Copy; same shift, original prefix through 8,030,000 us; 37 frames | Unknown partial outcome; 300 damage; null DPS (under three seconds); no bag |
| `zero-duration.pcap` | `maxed-home/zero-duration.pcap` | Copy; same shift, original prefix through 6,030,000 us; 33 frames | Unknown partial outcome; 100 damage; null DPS; no bag |
| `catalog.json` | `maxed-demo/catalog.json` | None | Synthetic catalog only; IDs are not real item/drop claims |

The validator parses every pcap record boundary, validates Ethernet/IPv4/TCP
header lengths and documentation addresses, and compares every Home packet
byte and shifted timestamp with the corresponding original prefix. The native
test additionally runs the real decoder, requires verified alignment, permits
only the admitted packet kinds, and checks decoded identity-bearing UPDATE
stats against the four synthetic identities. It does not infer credential
absence from scanning encrypted bytes as plaintext.

## Pipeline boundaries

| Named test | Evidence path |
| --- | --- |
| `replay_run_fight_player_matches_stored_sources` | Full pcap parse/stream/protocol/engine/writer/native list/story/fight/player reads, followed by SQLite reopen and duplicate-envelope projection |
| `late_counter_revision_reopens_one_story` | Decoded packet injection: base packets omit Victory; injected next-area CREATE_SUCCESS PcStats and identity UPDATE; separate typed domain-event earlier-clear revision |
| `saved_death_and_partial_story_evidence` | Typed domain events for cleared-then-died, gap/AppClosed, missing identity/assets, unknown bag contents and nullable zero-duration metrics |
| `story_waits_for_writer_retry_and_survives_stop` | Full packet-prefix decode plus adapter AppClosed; real SQLite `BEGIN IMMEDIATE` write lock/failure/retry; existing native read-error seam |
| `reconstruction_counters_do_not_become_run_counts` | Decoded packet injection using the existing engine reconstruction helpers, adapter publication ticks, real writer/native queries and reopen |

The counter injection cases are delta one within 30 seconds (accepted), a
receipt beyond 30 seconds (rejected), delta two (rejected), and invalid PcStats
(rejected). They are not encrypted-pcap counter coverage. Typed death evidence
does not establish DEATH packet decoding. These tests make no UI acceptance claim. Separate namespace-pause tests cover cross-command retirement.

Reconstruction diagnostics count three attempts: two reconstructions, one
reconstructed zero, one omission, five assumed target-stat defaults, and one
missing projectile. Existing durable source projection records positive
contributions only: the initial source is estimated 100 damage / one hit with
`client-projectile-reconstruction`; server reconciliation replaces it with
observed 321 damage / one hit and empty reasons. Lifetime diagnostics remain
unchanged, coverage issues remain visible, and the database still has one run.

The native test log reports terminal min/median/max query measurements. Validate a newly captured log with `validate.py --native-log <path>` rather than reusing old results. These tiny corpora establish synthetic data behavior, not performance or live gameplay acceptance.
