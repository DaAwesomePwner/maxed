# Parity and evidence

RealmShark is a reference implementation, not ground truth. Maxed compares decoded packet output and derived domain records separately. Unexplained differences fail the comparator; a documented rationale does not automatically suppress a mismatch.

## Reproduction contract

`maxed-cli decode` writes canonical packet JSONL. `export-domain` emits `maxed-domain-v1` records; `parity-domain` compares them with a matching reference export. Run `maxed-cli --help` for every subcommand's arguments. Supply the same input and catalog bytes to both exporters, and keep private captures and their exports out of the repository.

Domain headers must match input SHA-256, catalog SHA-256 and `tick-receipt-ms-v1`. Hash the actual `catalog.json` bytes, not reserialized JSON. Compare both aggregate numbers and identity-aligned player/source rows: positional array differences can be consequences of one extra source. Match source tuples by kind, item type and enchant state/IDs; inspect coverage separately from numerical equality.

The [deliberate-differences ledger](deliberate-differences.md) defines packet/domain rule grammar. The comparator reads that file; preserve its table and exact fixture/value selectors. There are no approved executable exception rows. Run:

```powershell
cargo test --locked -p maxed-cli --test parity
cargo test --locked -p maxed-cli --test domain_parity
```

Product player DPS uses that player's accepted first-to-last hit window, clipped to fight end. Per-player DPS is unavailable (null) when that player's active window is shorter than three seconds. Domain export uses the whole target `first_hit .. last_hit` interval without that minimum for Java comparability. The synthetic two-second story therefore has 150 export DPS and unavailable product DPS; this requires no suppression.

## Historical numerical evidence: 2026-10-02

The recorded audit compared 82 records with a RealmShark exporter at local revision `22f2083`, using catalog SHA-256 `ac42ad5168eab34ed4fee14dad6574783b3a48de728da8e2ef9d3f7424a922e2`. The private capture and reference exporter are not distributed, and publication of the exporter is undecided. This is historical evidence, not a public reproducible or current-HEAD acceptance claim.

Both exports had 295,815 local-player damage and 2,922,639 summed fight damage. All individual fight totals agreed. Reconstruction reported 528 attempts, 528 successes (91 zero-damage), zero omissions, and explicit neutral-default assumptions. The comparator still printed `unexplained=710 suppressed=0`; manual classification explained those leaves without installing exceptions:

| Class | Leaves | Interpretation |
| --- | ---: | --- |
| Explicit summon ownership | 136 | 10,841 damage moved from unattributed into 17 null-item summon sources, plus derived/positional differences |
| Source provenance availability | 566 | Java null estimate provenance versus Maxed's known boolean |
| Run evidence availability | 6 | Extra evidence entries and displaced leaves |
| Run boundary/outcome | 2 | Observed exit versus later reference boundary; unknown versus null outcome |

Corrected historical bugs included dropping server damage before target metadata, exporting zero-hit UI rows, missing neutral reconstruction defaults/enchant metadata, and Java numeric projection rounding. These results neither certify complete capture nor authorize fixture-wide exceptions. Durable behavior and current uncertainty are in the ledger, [protocol evidence](protocol-evidence.md), [kill evidence](kill-evidence.md) and [personal-best rules](../architecture/personal-bests.md).

## Shared frame processing

Live capture, native replay/demo, CLI demo replay and domain export use
`maxed_stream::driver::FrameDriver`. The driver owns frame-index gaps, stream
expiry, decode ordering, synthetic cancellation at EOF, and directional traffic
trust. Capture/protocol adapters provide only wire parsing and decoder operations;
no dependency or second frame parse is needed. The parsed segment is consumed by
reassembly; its small flow/direction/SYN/payload metadata view accompanies the
result, without cloning its payload.

Traffic follows the live rule: a SYN-proven direction can count payload fragments
before a full packet, and verified decoded packets establish trust. Midstream
unverified payload cannot start or extend a session. Recovery revokes trust and
close removes it. Already-observed packets do not independently restart the
traffic clock. This preserves live behavior, including its SYN-fragment rule.

Clock sources deliberately differ: live idle expiry uses monotonic arrival time
and its buffered timer; offline playback uses recording timestamps and checks
idle expiry before accepting returning trusted traffic. Offline playback does
not invent wall-clock silence between frames. Domain export hashes the exact
read supplied to `PcapReader`, and validates both private output paths before
creating files. These rules do not certify live capture or publication safety.
