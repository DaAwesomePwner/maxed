# Deliberate differences

| id | area | Java behavior | Maxed behavior | reason | evidence |
|---|---|---|---|---|---|

No parity exceptions are approved yet.

## Per-player fight DPS

RealmShark's domain export divides each player's damage by the whole target
report window (`J-domain:64-67`). `J-domain` identifies the reference domain
exporter derived from RealmShark; it is not distributed with this repository.
Maxed divides by that player's first-to-last accepted hit interval, clipped to
the fight end
(the confirmed kill time). Local ENEMYHIT activity uses exact packet receipt
timestamps; server DAMAGE activity retains the existing tick clock. Below
3 seconds of activity DPS is unavailable, including for personal-best
candidates. Rankings remain damage-based, and damage totals and shares are
unchanged. This avoids diluting late arrivals and displaying extreme rates
from brief realm sightings. The fight-total UI rate also requires 3 seconds.
The domain exporter separately divides player damage by the whole target's
`first_hit .. last_hit` interval, without the product's three-second availability cutoff.
That retains the Java-comparable decoding/attribution contract, so no DPS parity
suppression is needed. Reconciled local hits in either arrival order retain the
exact ENEMYHIT timestamp for the product window and the server-reported damage
amount.

## Reconstruction defaults

**D-crucible:** Maxed applies RealmShark's neutral defaults and counts each
assumption. `CrucibleBonusManager.java:25-36,219-223` treats missing/empty stats
128 and 155, missing configuration, and unknown IDs as 1.0 independently.
Maxed uses the same multiplication order and emits per-hit
`assumedCrucibleDefault` / `assumedBloodRitualDefault` counts for missing or
unknown evidence. Explicit empty strings are known inactive channels and do not
count as assumptions. Condition, attack and stat 113 have no Java defaults
(`Entity.java:269-275`, `Stat.java:25-30`, `Projectile.java:141`); missing values
remain separate counted omissions by the reconstruction policy.
These are attacker inputs. Missing target defense and either target condition
stat instead default to zero, matching `Entity.java:349-360,454-465`; each
uncertain absent stat increments `assumedTargetStatDefault` per reconstructed hit.
On typed UPDATE, stale defense/condition values are cleared. Only omitted stat 96
has evidence for a known zero baseline; absent 21/29 remain uncertain. A missing
target entity remains an omission.

**D-scaling:** Maxed applies RealmShark's neutral defaults and counts each
assumption. Missing or malformed matching-ability enchant evidence uses 1.0 and missing
scaling stat uses zero bonus (`AbilityScalingManager.java:413-448`). Applied
enchants use the catalog's `stat_damage_multiplier`: the f32 product of only
`StatModMult` equip mutators (`ParseEnchants.java:194-199,737-744,905`). Unknown
positive enchant IDs still refuse reconstruction, unlike Java's ignored unknown
IDs; older catalog definitions without combat metadata also remain explicit
omissions. Malformed codes use Java's neutral Totals
(`ParseEnchants.java:715-732`), increment `assumedAbilityEnchantDefault`, and
retain invalid source evidence. Re-extracting the catalog resolves missing
combat metadata; an omitted field never claims a neutral multiplier.

These are behavior/coverage decisions, not executable parity suppressions.
Defaults and source locations are specified in [Reconstruction defaults](#reconstruction-defaults)
and [Additional engine rationales](#additional-engine-rationales). The engine's
`reconstruction_coverage()` counts lifetime attempts as successful plus omitted
hits; zero-damage reconstructions are a subset of successes. Omissions count the
first failing prerequisite. `assumedTargetStatDefault` counts each uncertain
absent target stat per hit; other assumptions count at most once per reconstructed
hit. Server DAMAGE does not increment these counters, and later reconciliation
can change final totals without rewriting reconstruction diagnostics.

Named regression evidence includes `both_crucible_channels_use_counted_neutral_defaults`
and `scaling_only_requires_enchant_multiplier_for_matching_ability` in
[review.rs](../../crates/maxed-engine/tests/review.rs), plus
`default_counts_are_per_hit_survive_finish_and_known_bonuses_multiply` and
`ability_enchant_products_are_applied_and_unknown_ids_or_legacy_metadata_are_counted`
in [reconstruction.rs](../../crates/maxed-engine/tests/reconstruction.rs).
`typed_reentry_clears_omitted_stats_and_only_secondary_condition_defaults_known`
in [reconstruction_trust.rs](../../crates/maxed-engine/tests/reconstruction_trust.rs)
covers the typed-UPDATE distinction. The [historical numerical comparison](README.md#historical-numerical-evidence-2026-10-02)
is separate from these rules and tests. No executable suppressions are installed.

## Domain comparison rules

`parity-domain` compares `maxed-domain-v1` exports separately from packet parity.
Input and catalog SHA-256 headers must both match, with clock
`tick-receipt-ms-v1`. It reports unexplained counts by kind and structural field,
applied suppression IDs/counts, and coverage diagnostics separately. Matching
exports do not establish complete capture; missing metadata is explicitly reported.

`catalogSha256` hashes the exact bytes of `catalog.json`, including whitespace
and key order. A catalog-root writer must write sorted-key compact JSON and hash
those written bytes. Readers hash file bytes as-is without reserialization.

Reviewed domain rows use the same unique `MAXED-0000` ID format and six table
columns above, with evidence containing `domain-rule:` followed by one compact
JSON object. Its exact keys are `fixtureSha256`, `catalogSha256`, `kind`,
`recordKey`, `path`, `javaSha256`, and `maxedSha256`. `kind` is `fight`,
`encounter`, or `run`; `recordKey` is the exact canonical ordinal key. All hashes
are lowercase SHA-256. Paths are schema-authored JSON pointers into the record,
such as `/value/damage`, `/value/players/0/sources/0/damage`, or
`/value/evidence/0/kind`. The empty path selects a missing whole record, hashing
the complete `{kind,key,value}` object on its present side. Missing values hash
the non-JSON bytes `missing`; present values reuse compact sorted-key JSON hashing.
Unknown paths, wildcards, extra keys, duplicate IDs, and duplicate selectors are
rejected. Every suppression requires matching both provenance hashes, kind, key,
path, and both expected-value hashes. Packet and domain rules can share this
ledger; each comparator validates both rule types and applies only its own type.
Packet rules retain their existing private metadata and explicit fixture checks.

Additional engine rationales appear below. They are not executable suppression IDs. Actual mismatch hashes, assigned `MAXED-0000` IDs and review are required before adding a row.

## Coverage gaps

- NOTIFICATION effect 24, extra 32 has an observed two-byte numeric tail.
  Its field semantics are not established, so it remains a `Definition`
  failure. No enum alias or arbitrary tail consumption is permitted.
- Some CREATE_SUCCESS PcStats snapshots decode their outer packet but leave
  25 unexplained bytes after the existing RealmShark interpretation consumes
  119 of 144 decoded bytes. That interpretation uses a four-byte little-endian
  header and a 16-byte (128-bit), least-significant-bit-first-per-byte bitmap.
  These snapshots remain incomplete; completion-counter fallback is unavailable
  for them. The unexplained tail is not consumed by the production validator.
- Historical diagnostics establish that Recording A's 37 field failures all
  precede tick recovery on a reset-assumed fresh connection with no decoded
  NEWTICK. These are startup mis-decryptions shared with Java, not evidence for
  changed UPDATE, CREATE_SUCCESS, EXALTATION, or NOTIFICATION layouts. Existing
  layouts and strict rejection behavior are retained.

## Startup and parity

### D-trust: cipher-alignment provenance

Java emits packets immediately after a directional reset even when no SYN was
captured. Maxed preserves those packets and the same cipher/startup behavior,
but adds `DecodedPacket.alignment_verified` for library consumers. Engines
must ignore unverified packets as gameplay evidence; this flag does not certify
packet-field validity, complete capture coverage, or authenticated server data.

`StreamEvent::Opened.syn_proven` records the observed directional SYN separately
from `fresh`. A SYN-proven fresh stream is verified from its first packet. A
reset-assumed fresh stream starts unverified and becomes verified on the second
consecutive directional tick header (NEWTICK for server-to-client, MOVE for
client-to-server). The existing tick-pair `Recovered` transition anchors a
candidate alignment but leaves it unverified until the next consecutive
directional tick confirms it; packets emitted before that confirmation remain
unverified.
Recovering, stream gaps, bad framing, and a new Opened event discard the previous
trust state. Recovery emits no packets until alignment is reestablished; Closed
discards the stream. Trust is directional and never retroactively upgrades
earlier packets. Rejecting a packet's fields does not rewind the cipher or
discard otherwise valid alignment evidence.

Canonical JSONL is unchanged: the trust flag is library metadata, not a wire
field or parity exception. Decode/replay summaries add `unverifiedPackets`,
counting emitted decoder packets whose flag is false, including unknown packets
and packets that canonical export subsequently rejects as nonfinite. Decode
metadata includes this counter too. No suppression rule is needed in the table.
Synthetic provenance, tick-pair, gap, replacement, and canonical-export tests
cover this difference; private-recording acceptance requires separate local evidence.

Java-equivalent startup behavior: `StreamEvent::Opened.fresh` means initialize
a fresh directional cipher and bypass the startup gate. It is true when a SYN
was seen, when an earlier RST left a pending reset in that direction (including
a stale connection's RST), or when a new connection replaces the previous one
in that direction. This follows `TcpStreamBuilder.streamBuilder()` calling
`resetStream()` on SYN, RST, and connection replacement, with
`PacketConstructor.reset()` clearing the startup gate and resetting the cipher.
It does not assert that a SYN was captured. A first non-tick packet after such
a reset is decoded immediately from the initial cipher position; it does not
wait for tick alignment. ACK-only traffic does not select a connection, and
the opposite direction retains its own startup state. These are parity
semantics, not exceptions or suppression rules.

The parity command reads the table above. A reviewed row must have a unique
`MAXED-0000`-style ID and nonempty cells. Its evidence cell must contain exactly
`parity-rule:` followed by one compact JSON object, without a code fence. Required
keys are `fixtureSha256`, `dir`, `ordinal`, `packetId`, `path`, `javaSha256`, and
`maxedSha256`; additional keys and wildcards are rejected. Hashes are lowercase
SHA-256 hex, direction is `c2s` or `s2c`, and ordinal is zero-based within that
direction. Paths are exact schema-authored JSON pointers; the empty string denotes a missing
whole packet. Unknown wire-authored object keys cannot be suppressed.

Expected-value hashes use compact UTF-8 JSON with sorted object keys, no BOM and
no final newline. A missing value hashes the non-JSON bytes `missing`. For a whole
packet, hash the object containing `t`, `id`, `name`, and `f`, excluding global `i`.
The `value_hash` library helper implements this encoding.

Private provenance lives beside each export: `rust.jsonl.meta.json` accompanies
`rust.jsonl` and contains `inputSha256` (the input pcap SHA-256), plus
`fieldFailures`, `trailingBytePackets`, `unknownIdPackets`,
`wrongDirectionPackets`, `unverifiedPackets`, and `truncated`. The first four counters use Java golden
metadata names; known but untyped packets are not unknown IDs. Legacy metadata
containing only `inputSha256` remains readable. Both
metadata files must be under this checkout's ignored `recordings/` directory and
agree. CLI use of any populated ledger also requires `--fixture-sha256` matching
both files. The library `parity` function reads the metadata directly. Rules for
other fixtures or changed expected values suppress nothing. The CLI reports
each applied suppression ID and count, including on exit 0.

`decode` and `replay` retain every complete frame before a truncated final pcap
record, report `truncated=true`, and exit 1. Decode commits the complete JSONL
prefix and its metadata; `inputSha256` still hashes the entire input file.
Malformed record lengths/timestamps and invalid global headers remain errors.

For a private structural report, add `--explain-failures recordings/report.txt`
after `decode <input.pcap> --output recordings/output.jsonl`. Both output paths
must pass the same private-location checks and must not already exist. Each
report line includes a global zero-based `packetOrdinal`, connection ID,
direction, timestamp, most recently decoded NEWTICK ID on that connection, and
the directional post-Recovered window/count. Rejected and suppressed frames
count toward this ordinal; it is not canonical JSONL `i`. Lifecycle events
between frames use the next ordinal. Opened, Recovering, Recovered, and Closed
events appear in the same ordered report.

Failure details include packet name/id, framed length, zero-based framed byte
offset, error kind, and UPDATE/NEWTICK stat shapes. NEWTICK has no object type on
the wire, so its shape uses `unknown`. Numeric body-prefix previews extend at
most 64 bytes beyond the failing body offset and stop before the first string
under the current schema. Their interpretation does not establish a new layout.
Numeric CREATE_SUCCESS/EXALTATION truncation traces identify fields and widths.
Per owner authorization, unknown NOTIFICATION effects include effect/extra and
remaining-body hex only when at most eight bytes remain. PcStats diagnostics
include its little-endian header, bitmap interpretation, counter IDs/values/
widths, and unexplained tail hex. These diagnostic permissions do not relax
production validation or the scrubber's admission policy.

Byte previews require a synced stream with verified alignment. Unverified frames
emit `prefixHex=redacted-unverified` and omit `nextHex`, `remainingHex`, and
`tailHex` entirely, while retaining numeric offsets, lengths, ordinals, and
timestamps. Numeric trace values are also withheld except for string lengths;
successful outer CREATE_SUCCESS reports retain only inner encoded/decoded
lengths and omit counter values and tails until alignment is verified.

## D-sprite-alpha — sprite PNG alpha handling

RealmShark's Java PNG path premultiplies and unpremultiplies RGBA with fixed-point rounding, which changes RGB (and sometimes alpha by 1) on transparent and partially transparent pixels. Maxed keeps the game client's exact RGBA bytes. The asset oracle test reproduces Java's conversion exactly and requires RealmShark's sheets to equal that model applied to Maxed's pixels (88,080,384 real pixels, zero unexpected differences). Opaque pixels are identical. See [assets](../architecture/assets.md).

## Instance and combat projections

**D-bags-excluded:** Brown and pink bags (1280, 1286, 1709, 1710 and catalog
variants with brown/pink base types) are excluded like soulbound 1283, even if
catalog capture is enabled. No bags are captured in Nexus or OtherHub areas.
Realm, Dungeon and Unknown areas retain capture; unknown observations stay visible.
Every new drop carries `CaptureScope::RealmShark` and shares the existing bag limit.
Migration 011 purges legacy brown/pink rows by bag type (base Extended and
boosted RealmShark scope) and drops inside saved hub visits, including their items, timeline entries and alerts, and recomputes surviving
alert ordinals.
`CaptureScope::Extended` and the domain export filter remain only for legacy input.
Regressions: `brown_pink_bags_are_excluded_even_when_catalog_requests_capture`,
`bags_are_excluded_only_in_hubs`, `excluded_bags_do_not_consume_dialogue_window`
and `excluded_bags_do_not_consume_capacity_or_mark_fights`.

**D-kill:** Removal alone never confirms a kill. A current or recently cleared
quest target may receive Victory evidence (one-second window, either order).
The sole catalog-BOSS fight hit within ten seconds is a **provisional** Victory
fallback. HP zero followed by same-generation removal within two seconds,
without positive HP intervening, is non-provisional. An exact normalized Oryx
kill-template match is **provisional**; out-of-view announcements create only
an instance event, never a fight. Confirmation immediately publishes KillView. Non-provisional confirmation closes the
fight at the kill timestamp with Killed and rejects new post-kill hits while allowing
existing reconciliation to settle. A recognized announcement plus same-generation
removal within 500 ms in either order confirms independently of final HP. Provisional
announcement/sole-boss evidence alone does not close an active fight. Retirement
preserves the applicable stop/idle/close/resource-limit reason for unclosed fights.
Capture completeness and damage acceptance are unchanged. Tests:
`drop_without_hp_zero_is_not_a_kill`,
`victory_with_two_boss_fights_and_no_quest_is_unattributed`,
`kill_view_published_at_confirmation`, `killed_label_replaces_map_changed_only`,
`killed_retirement_keeps_complete_capture_rules`,
`capture_stopped_after_kill_keeps_capture_stopped_and_kill_view`,
`oryx_kill_out_of_view_is_event_only`, and the kill-detector window/generation tests.
PB eligibility accepts MapChanged and Killed equally, without requiring a kill or promoting
provisional evidence; `pb_accepts_killed_end_reason` and the five companion PB
regressions guard this. `boss_type` remains catalog BOSS, independent of the
Boss > EventBoss > Miniboss > Quest > Ordinary presentation precedence.
Quest membership is sticky per fight and bound to the observed object generation;
clearing/moving QUESTOBJID does not erase it. Regression:
`quest_kind_survives_clear_move_and_retirement_without_generation_leak`.

The synthetic demo receives a provisional sole-boss kill. Its 300 damage and clear outcome are unchanged; product DPS remains unavailable below three seconds.

**D-loot-kill:** Existing RealmShark attribution runs first. Only a remaining
`source-not-observed` bag may use the new fallback: birth in the kill tick or
next tick, within 1.5 tiles of exactly one killed target's last position.
The result is **estimated**, with reason `killed-target-same-tick` (including
that next-tick window). Neither a bag nor proximity confirms a kill.
Tests: `boss_bag_same_tick_as_kill_gets_killed_target_source`,
`existing_rs_attribution_wins_over_kill_rule`, and
`kill_loot_requires_unique_nearby_candidate_in_tick_window`.
Live captures still need to validate this inferred loot association.

Population remains provisional; party death and realm-announcement events are
provisional; departures mean cause unconfirmed. Portal opening is not
provisional (five samples). Unique normalized names link party deaths, and a
two-second pending departure permits a late death notice to cancel the leave.
Only departures before Victory (`completed_at`) count as Left:
the engine drops later ones at run end, and saved runs exclude them at read time.
Remote death counts increment only settled rows already present in active
fights; pending-only estimates do not create identity rows. Regression:
`remote_death_does_not_inject_row_for_unsettled_estimate`.
Local notices never emit PlayerDied or double-count the Death packet. Events
share a 4,096-per-map cap; overflow increments events_dropped without marking
capture coverage. See the d_instance/d_notices suites, particularly
`removal_after_death_notice_is_not_left`,
`removal_then_late_notice_within_2s_cancels_left`,
`local_name_death_notice_does_not_emit_player_died`,
`instance_event_cap_counts_drops`, and
`mixed_notice_and_departure_events_share_the_cap`.

Series and totals are additive product projections, outside domain parity.
Snapshot series are bounded to 16 non-ended fights: Boss/Quest first, the latest
local-hit fight next, then newest last hits; ties use ascending fight ID. All
other snapshot series are null, while emitted events retain full series.
`snapshot_series_cap_prioritizes_and_keeps_full_event_series` covers the 2,048-fight
bound, deterministic priorities, and full update/retirement event payloads.
Series retain at most 200,000 cells, emit the top five plus local (including a
zero-damage local row), align with compacted buckets, and become null without
changing fight damage/coverage when the cap trips. The permanent safety latch
counts observed estimate cells at input time, so cancelled estimates can still
consume the historical budget; snapshot frequency cannot change the latch.
Totals exclude realm fights, cap rows at 64 while preserving local rank/share,
and count observed kills without promoting their evidence. Cached run totals
subtract/add only changed fights, preserving full-row ranking and metadata
precedence. Direct mutation/drop invalidation avoids scanning unchanged fights;
a maintained damage-order index makes finalization a single player pass, reusing
the cached row allocations. Owned snapshot fields still require independent copies. Live fame_delta uses first/last fresh local stat-57 samples within
the run window and one CREATE_SUCCESS identity; fewer than two samples means
null. Remote stats, stale retained stats and post-death stats are excluded.
Regressions: `totals_incremental_replacement_removal_and_metadata_match_reducer`,
`fame_delta_uses_only_fresh_local_samples_in_current_run`, and
`fame_samples_do_not_cross_character_or_death_boundaries`. Stat breakdown is
provisional; exalt remains null and unknown equipment keeps gear/other null.
Tests: d_series, d_totals, d_stat_panel and the pure projection tests.

These are scoped behavior decisions, not executable parity suppressions.
Synthetic regressions do not establish fresh live or private-reference acceptance.

## D-quest-score — packet-parity note

The two quoted documentary rows below are outside the executable `MAXED-####` exception table and grant no fixture-wide suppression. Actual fixture hashes and field differences require separate verification.

> | id | area | Java behavior | Maxed behavior | reason | evidence |
> |---|---|---|---|---|---|
> | D-quest-score-82 | packet parity | RealmShark golden output records QUESTOBJID (82) as unknown with framed length | Decodes signed big-endian `objectId`, compressed count and compressed `list`; count outside 0..=64 is rejected | Quest-target evidence is numeric and useful to the instance tracker | RealmShark QuestObjectIdPacket layout and current protocol definitions; synthetic d_packets and CLI decode/scrub/parity tests |
> | D-quest-score-169 | packet parity | RealmShark golden output records REALM_SCORE_UPDATE (169) as unknown with framed length | Decodes one signed big-endian `score` as the absolute current score | Live realm score is numeric; its maximum remains MAPINFO data | RealmShark RealmScoreUpdatePacket layout and the protocol evidence guide; synthetic d_packets and CLI decode/scrub/parity tests |

Both numeric bodies survive scrubbing unchanged. No RealmShark repository or
golden exporter is changed. SHOWEFFECT (11) remains unknown. Domain export still
passes unknown packets to the engine; valid 82/169 packets do not mark
DecodeFailure, so completeCapture may differ on those inputs. This is recorded
for evidence classification, not suppressed or corrected by changing engine coverage.

## D-coverage-scope - record-local capture coverage

RealmShark-compatible session-sticky issues made every later fight and drop
partial after a single unknown packet or reconstruction default. New fights
and runs start with their own observed prerequisites, never a copy of past
issues. `snapshot.issues` continues to describe current-area coverage and clears
at each verified map boundary; aggregate capture counters survive that boundary.
A coverage issue marks fights open at observation time and the active,
non-ended run. Real
relevant decode failures, stream gaps and midstream observations retain run
`CaptureGap` evidence and `complete_capture=false`.

Drops use an inclusive two-second window on either side of their first
observation. This accommodates adjacent bag birth and inventory/update packets
without tainting earlier bags or loot observed much later. The window uses monotonic
microsecond arrival timestamps, not tick time or rounded exported timestamps;
recent issues are deduplicated by issue kind and cleared at map boundaries.
Fights starting even within that window do not inherit previous issues.

The direction registry includes known but unmodelled IDs; their `Packet::Unknown`
representation never marks records. Completely unknown IDs are counted but also
do not mark records: the decoder's fallback retains only ID and length, and no
schema or reducer establishes combat relevance for those IDs. Known field
failures (including trailing fields) mark records only with verified alignment
and a packet ID consumed by combat, drops, identity or map/run state. Bad framing
marks through the accompanying stream-recovery event. Wrong-direction packets
and unrelated modelled packets retain aggregate diagnostics only.

Decoder `Diagnostic::Packet` is numeric companion context for existing diagnostic
events, preserving aggregate totals and existing live adapters. `CaptureStatus`
adds at most 64 per-ID rows, retaining the first 64 distinct observed IDs in
sorted order, with saturating unknown and field-failure counts. Both BadFields
and TrailingBytes increment per-ID field failures. Later new IDs remain
represented by existing totals; retained IDs continue counting. No packet
payloads or traffic strings are retained. Empty diagnostics omit the new JSON
field; legacy stored JSON still loads with an empty default.

Damage reconstruction defaults mark only their target fight, including its
initial event. Reconstruction failures and missing catalog data are
fight-local; character missing stats stay on the character. None of these local
issues propagates to other fights, drops or the run.

Domain comparison classifies only Java `completeCapture=false` to Rust `true`
and removal of Java run `capture_gap` entries as `D-coverage-scope`, also counted
under `coverage=D-coverage-scope.fields`. Rust gaps must be an exact subset of
Java gaps, including duplicate counts, and all non-gap evidence must match.
Those gap entries are removed symmetrically for comparison, avoiding array-index
shifts. Reverse completeness changes, additional or changed Rust gaps,
null/unknown completeness and mixed evidence changes retain their original
ledger and unexplained comparisons. Outcomes, damage and all other fields
still compare normally. Metadata issue counts remain separately reported
for both sides. This reviewed coverage
classification is built in and does not relax fixture-bound rules for any
other domain fields or packet parity.

Validation uses synthetic coverage_scope, domain_parity and legacy-contract
tests; private recordings are not inputs to these tests.

## Personal-best reconstruction confidence

The eligibility audit in [personal-best eligibility](../architecture/personal-bests.md) documents the evidence behind
known neutral empty bonus channels and omitted secondary condition (96) on a
typed UPDATE. Missing defense (21) and primary condition (29) remain uncertain.
Reconstruction provenance alone does not mark a source estimated; trustworthy
inputs also require a trusted map-seeded RNG. Gaps, lost/garbled PLAYERSHOOTs
and unadvanceable PlayerShoots invalidate trust until a new map seed. Other
packet decode failures retain their coverage marking without latching RNG
uncertainty. A server/estimate mismatch taints only its fight: target-condition
timing, immunity or kill clamps can differ without RNG drift. Later mismatches
will taint their own fights, while known lost RNG advances remain latched.
Missing/unknown inputs and invalid RNG trust still block personal bests, and the
eligibility predicate still requires complete capture, clean issues and no
estimated local sources.

## Additional engine rationales

These explain behavior; they do not grant comparator suppressions.


- **D-trust (engine extension):** `apply` rejects packets whose
  `alignment_verified` is false before accepting connection, clock, world,
  identity, combat, run, loot, fame, or alert evidence. No rejected packet is
  buffered or replayed. The saturating lifetime `ignored_unverified_packets()`
  counter and a `CaptureStateChanged` message report the ignored count without
  relabelling it as a malformed-field decode failure. Known stale connections
  remain separately ignored. `CoverageIssue::Midstream` marks the snapshot,
  affected fights/runs/drops, and subsequent results in the same map; runs also
  retain `Evidence::CaptureGap`. Loss before any known map remains a snapshot
  issue until a verified MAPINFO starts a new visit. That boundary clears prior
  alignment loss, including loss on an unaccepted connection; a fully observed
  visit can still end `Left` with complete coverage. Rejected
  packets do not advance traffic/idle timers; adapters may separately report
  actual transport traffic with `observe_traffic`. Alignment verification alone
  does not certify full capture coverage or packet-field validity.
- **D-hit:** a unique estimate/server pair is represented by its server amount
  once, in either arrival order. Repeated server reports remain separate.
  Ambiguous estimates are omitted from numerical totals, with coverage evidence;
  reconciliation eviction also omits unresolved estimates instead of confirming
  a combined amount.
- **D-owner:** source lookup never falls back to another owner's bullet array.
  Shooter generation and unsigned bullet ID must match. Summons require an
  explicitly observed summoner that resolves to a player. SERVERPLAYERSHOOT can
  establish that link before the summon's UPDATE metadata arrives; damage then
  has a known player and `summon` source but a null summon item type. Java's
  DAMAGE lookup additionally requires the summon to exist in `entityList`, so
  it leaves those hits unattributed. Maxed does not use the projectile's
  container item as a fabricated summon object type. On an UPDATE view exit,
  Maxed clears that entity's shots and ownership links (including links from
  its children); Java clears the dropped summon mapping but retains shot source
  records and player-owned child links. These source/ownership differences
  require per-hit evidence before accepting numerical ledger rows.
- **D-crucible (reconstruction policy):** Maxed applies RealmShark's
  neutral defaults and counts each assumption. Missing/unknown stat 128 and 155
  configuration each contributes 1.0, like `CrucibleBonusManager.Offline`;
  explicit empty strings are observed inactive channels and contribute 1.0
  without a fallback diagnostic.
  Nonneutral values combine in Java's f32/f64 order. Missing
  condition, attack, or stat 113 still prevents reconstruction: Java has no
  defaults for these inputs. Each failed hit receives an omission reason.
  No HTTP lookup is used.
- **D-scaling (reconstruction policy):** Maxed applies RealmShark's
  neutral defaults and counts each assumption. Missing or malformed matching-ability
  enchant evidence uses 1.0; a missing scaling stat gives a zero bonus. Only the matching
  inventory stat 9 item uses the ability enchant multiplier. The catalog stores
  the f32 product of `Mutators/ActivateOnEquip amount="...">StatModMult` values;
  hit reconstruction multiplies positive enchant IDs in slot order, stopping
  at -3 and ignoring locked/empty IDs. Unknown positive IDs and absent combat
  metadata in legacy catalogs remain counted omissions. Malformed codes retain
  invalid source evidence and increment `assumedAbilityEnchantDefault`, matching
  Java's neutral Totals (`ParseEnchants.java:715-732`).
  Java ignores unknown positive IDs; Maxed's explicit refusal is retained.
- **D-fame:** use observed current-fame stat 57, not Java `Entity.fame`'s
  experience-derived formula. Preserve the last emitted value by opaque character
  ID across maps, connections and idle sessions, and migrate provisional samples
  when identity is verified, matching the journal's unchanged-sample suppression.
- **D-state:** view exits do not create new object generations. A changed observed
  type does; map/connection boundaries reset entity state. Unlike Java's retained
  entity list, unreferenced view exits are pruned after a 30-second grace period;
  forgotten deltas then remain unknown until observed again. Active referenced
  targets, player identities and bags survive until the map boundary; retired
  targets/bags release their full entities and retain bounded tombstones.
  Dirty fight projections are published on the 250 ms engine clock cadence
  instead of on every report; starts and finalization remain immediate.
- **D-clock:** deterministic tick-receipt milliseconds replace wall-clock
  scheduling jitter. Pre-tick receipt-time fallback is explicitly partial.
- **D-outcome:** preserve ordered completion and death facts, retain death-first
  outcomes, and promote stronger completion evidence. Later same-visit completion
  does not turn a death-first run into a clear. Signals after a new character
  creation cannot revise the frozen death visit. Recipient whispers are rejected
  even when their text reproduces a recognized NPC line. This concerns completion
  evidence only; Logs stores whispers as chat. ESCAPE ends the run at
  receipt time; the Java ActivityJournal ends its visit at MAPINFO or capture
  termination instead. Maxed retains `capture_gap` and `observed_exit` evidence,
  and emits `unknown` for an ended incomplete run lacking completion/death
  evidence. Java's domain adapter has no unified non-completion outcome and
  exports null; Maxed keeps its supported outcome and evidence.
- **D-provenance (domain availability):** Java `Damage` has no estimate/server
  provenance flag, so its exporter emits null for source `estimated`. Maxed
  retains the known boolean; null does not mean false. This availability
  difference never excuses source amounts, owner identity, or hit counts.
- **D-counter:** require a fully decoded baseline, verified same character,
  immediate next area, exact +1, no gap, and both entry and observation within
  30 seconds. Strict malformed/unsupported/trailing data yields no baseline.
- **D-loot:** publish bag identification immediately and enrich the same ID;
  flush unknown contents on closure. Spatial/dialogue attribution is explicitly
  estimated; tied candidates remain unknown. HM/TR suffixes remain reasons,
  never fabricated enemy types. Gear changes invalidate stale enchant evidence
  for the changed slot when no corresponding enchant update was observed.
  **Reference timing limitation:** `TomatoData.update` queues new bags through
  `entityUpdate` before adding removed hit targets to `killedEntitys`
  (`TomatoData.java:555–584, 619–621`). `updateNewTick` calls `lootTick`, which
  flips its queue selector before processing bags and clears `killedEntitys`
  every tick, including ticks with an empty selected queue (`655–711, 805–817`).
  A bag is therefore evaluated on its second subsequent tick, after a target
  removed in the same UPDATE has expired. Maxed preserves this limitation:
  without dialogue fallback, the source stays `source-not-observed`, not a
  confirmed dropper. A target removed between the bag's first and second ticks
  can supply the nearest-candidate estimate, as in
  `LootAttributionManager.findAttributionForBag` (`2298–2334`); Maxed labels it
  `nearest-removed-hit-target` with `source_estimated = true`. These timings were
  verified by source inspection, not by a Java gameplay replay.

Maxed does not download RealmShark's chat spam blocklist; spam is hidden by local hide lists.
