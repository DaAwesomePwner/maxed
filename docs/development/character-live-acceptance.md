# Character live acceptance

## Gate U: pass for the observed unexalted character

On 2026-10-17, **Wizard L20 (second account)** had explicit zero values for all eight exalt-level stats
105–112 at initial object materialization and at both subsequent re-materializations. This supports Task 4's
known-zero path for this tested situation. Later sparse updates omitted the fields; those omissions do not
replace the observed zeros with unknown values. Missing initial fields in other captures must still stay unknown.

**Gate P remains open.** Zero exalt levels cannot distinguish placement in base versus boost. No matching
catalog, gear bonuses, class caps, controlled equipment transition or nonzero-exalt SPD counter-case was available.
The owner accepted the scoped Gate U pass on 2026-10-17 in
private-repo PR 176 (archived, not public). This acceptance does not establish stat 113
semantics, resolve Gate P or Gate X, or constitute Task 11 acceptance.

See [Task 2 and the gate definitions](../superpowers/plans/2026-10-17-character-tab.md) and the
[stat-split specification](../superpowers/specs/2026-10-15-character-tab-design.md).

## Production enablement

On 2026-10-20, the owner (sole user) approved enabling `character` in production before Task 11,
following Party's rollout precedent. The flag is on; Windows live acceptance has **not** been run
and is being verified in use. Pending checks include weapon/regen values versus the current client,
equipment/stat/condition changes, death, capture stop, reconnect, identity/account transitions,
missing-asset recovery, 100%/150% WebView2 scaling and keyboard behavior. The full Task 11 checklist
below remains pending. This enablement does not claim live Npcap, game-client or WebView2 verification.

On 2026-11-06 the owner accepted weapon/regen values versus the client, 100%/150% scaling and keyboard
behavior from Windows use. That is in-use acceptance, not a recorded Task 11 pass; the checklist below
stays unexecuted.

Gate P (exalt placement) remains open: at-cap, pots-left and maxed stay null when exalt is nonzero
or unknown. Exalt damage (stat 113) remains deferred. Gate U acceptance does not resolve either gate.

## Provenance and method

| Item | Evidence |
| --- | --- |
| Review date | 2026-10-17 |
| Input | private owner capture, sha256 prefix fa7694a0 |
| Format | Classic little-endian pcap, Ethernet link type |
| Decoder and code-reading revision | `347fbd18a53aff84f6fe3dea7bfd269787c91269` |
| Capturing Maxed build/commit | Not supplied; decoder revision is not the capture application's build |
| Windows version | Not supplied; Windows capture platform is owner-supplied |
| Npcap version | 1.88, owner-supplied on 2026-10-17; not independently recoverable from this capture |
| Game client version | RotMG 7.1.0.2.0, owner-supplied on 2026-10-17 |
| Decoded Hello build | 7.1.0.2.0 in all three Hello packets; matches the owner-supplied client version |
| Extracted catalog/version/fingerprint | Not supplied |
| Synthetic character label | Wizard L20 (second account) |
| Owner confirmation | Level 20 Wizard, no exaltations; different account from usual; same Windows Maxed database |
| Observed corroboration | Level 20 at all three materializations; consistent class type and zero exalt fields |

The evidence checklist was recorded before analysis: provenance, local identity, initial versus later
materialization, sparse updates, totals/boosts, packet 114, stat 113, coverage, separate U/P dispositions,
multi-account risks, and the Task 11 checklist below.

Built the repository CLI with `source scripts/mac/env.sh && cargo build --locked -p maxed-cli` (exit 0).
The CLI decode output-path guard permits only the repository's recordings directory. To honor the stricter
private-output requirement, a throwaway runner outside the repository used the unchanged compiled
`maxed_cli::Pipeline`, `maxed_recording::PcapReader`, `canonical_line`, `write_jsonl` and `ReplayReport` APIs.
It retained connection and alignment metadata separately, enabled structural explanations, and wrote every
decoded/intermediate output to the private owner's mode-700 output directory. No repository code was changed.
The private decode and an independent `maxed-cli replay` both exited 0 with matching aggregate counts below.

Analysis matched CREATE_SUCCESS to the local UPDATE object within each connection, rather than selecting a
player by name or reusing an object number across connections. Every exported packet had verified cipher
alignment. Nonempty account stat 38 and the same character number were present for each local materialization,
meeting the engine's identity inputs. This verifies the wire identity association, not the Windows application's
persisted `identity_verified` flag: the application database and matching catalog were not inspected.
The Wizard class label is owner-supplied; no catalog-derived class/cap assertion is made.

Only sanitized measurements and provenance are published. No fixture, raw packet, catalog, account identifier,
player name, private path or credential is included. This is not scrub certification or fixture publication approval.

## Coverage and materialization boundaries

Times are seconds relative to the first pcap frame, rounded to six decimal places. The file spans 43.565222 seconds.
It begins with the first connection handshake, before Hello at 0.033635 and before character entry/materialization.
It covers Nexus → Vault → Nexus, with two RECONNECT packets at 20.892029 and 28.030436 seconds and three
SYN/SYN-ACK pairs. These are map transfers/reconnections, not an observed logout and login to another account.

| Observation | Map | CREATE_SUCCESS | First local UPDATE | Later local NEWTICK statuses | Later status range |
| --- | --- | ---: | ---: | ---: | --- |
| Initial materialization | Nexus | 1.336995 | 1.372925 | 30 | 1.482966–20.185584 |
| Re-materialization A | Vault | 23.552402 | 23.584305 | 14 | 23.713655–27.977262 |
| Re-materialization B | Nexus | 29.851662 | 29.884146 | 13 | 29.999218–41.043532 |

Each connection contains exactly one local object materialization. There are 529 UPDATE packets and 187 NEWTICK
packets overall; only the three local materializations and 57 local status entries are counted in the tables.
Other players' objects/statuses are excluded. No additional local materialization was inferred from sparse ticks.

| Decoder/coverage measure | Count/result |
| --- | ---: |
| Pcap frames | 1,727 |
| Exported decoded packets | 1,617 |
| Typed packets | 784 |
| Opaque/unknown-layout packets | 833 |
| Outer decode failures / field failures / trailing-byte packets | 0 / 0 / 0 |
| Unknown packet-ID diagnostics / wrong-direction diagnostics | 0 / 0 |
| Unverified-alignment packets | 0 |
| Recovering / Recovered diagnostics | 0 / 0 |
| Stream opens with fresh handshake provenance | 6 directional opens |
| Stream Closed diagnostics | 9; not nine distinct connections or packet-loss events |
| Unsupported-link frames / snap-length-truncated frames | 0 / 0 |
| Truncated final pcap record | No |

Opaque packet counts by ID are: 61: 3, 122: 3, 182: 2, 81: 529, 120: 3, 165: 3, 183: 2, 62: 186,
11: 100, 47: 1, 117: 1. They have framing/alignment evidence but no typed field interpretation here. No unknown
layout was treated as a missing exalt field. UPDATE, NEWTICK, CREATE_SUCCESS and packet 114 decoded successfully.

Separately, all three CREATE_SUCCESS embedded completion-stat blobs have 79 decoded bytes, of which the current
PcStats interpretation consumes 61 and leaves 18. This is an incomplete embedded interpretation, despite zero
outer packet failures; completion-history semantics are not certified. The local object/character association
and exalt measurements use decoded outer fields and UPDATE/NEWTICK, not those embedded completion counters.

No reassembler gap/resynchronization was reported. This is not proof of loss-free Npcap acquisition: pcap alone
cannot establish the capture application's dropped-frame counters or behavior before/after this short recording.
The positive zero observations do not rely on assuming an unseen packet or missing field was zero. Coverage is one
owner-confirmed character, one client build and hub transfers; no dungeon, death, combat acceptance, second local
account, controlled equipment change or independent full logout/relogin was observed/tested.

## Exalt-level presence: direct observations, not carried values

P0 = **present-zero**; PN = **present-nonzero**; A = **absent**; I = **invalid** (including negative values).
The initial/A/B columns describe the fields in each actual local UPDATE object. Later counts apply separately
to each stat and each local NEWTICK status entry, before carrying forward any previously observed value.

| Level stat | Stat | Initial | Re-materialization A | Re-materialization B | Later counts P0 / PN / A / I |
| --- | --- | --- | --- | --- | --- |
| 105 | ATT | P0 | P0 | P0 | 0 / 0 / 57 / 0 |
| 106 | DEF | P0 | P0 | P0 | 0 / 0 / 57 / 0 |
| 107 | SPD | P0 | P0 | P0 | 0 / 0 / 57 / 0 |
| 108 | VIT | P0 | P0 | P0 | 0 / 0 / 57 / 0 |
| 109 | WIS | P0 | P0 | P0 | 0 / 0 / 57 / 0 |
| 110 | DEX | P0 | P0 | P0 | 0 / 0 / 57 / 0 |
| 111 | HP | P0 | P0 | P0 | 0 / 0 / 57 / 0 |
| 112 | MP | P0 | P0 | P0 | 0 / 0 / 57 / 0 |

For **each** of the eight fields, later absence counts split as initial Nexus 30, Vault 14, final Nexus 13;
all three groups have zero later present-zero, present-nonzero or invalid values. Across materializations there
are 24 explicit zero fields, no absent/invalid fields, and no duplicate stat entries in the analyzed local records.
A later absent field means "not included in this sparse update," not an omitted initial observation or a reset.

### Packet 114 and stat 113

Packet 114 (`ExaltationUpdate`, canonical name `EXALTATION_BONUS_CHANGED`) appears three times, once per
materialization, at 1.372925, 23.584305 and 29.884146 seconds. Its class type matches the local object type.
Every packet reports the following progress counts; these are raw observed counters, not inferred stat bonuses:

| Progress | ATT | DEF | SPD | VIT | WIS | DEX | HP | MP |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Each of the three packets | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

Stat 113 is present-nonzero with raw value **1000** at each of the three local materializations. It is absent
from all 57 later local statuses (30/14/13), with no later value or invalid value observed. This is solely a
presence/value report: no meaning, neutral multiplier, damage effect or calculation rule is established.
Packet 114 is distinct from stat 114; no stat 114 interpretation is added.

## Synchronized totals and raw boosts: Gate P evidence only

All eight totals, raw boosts and levels below occur together in **each** of the three materialization records.
The resulting observed state is unchanged across the 57 later local status entries (60 local records including
materializations). Carry-forward describes that state only; it is not counted as new wire presence.
Equipment slots also have one unchanged configuration across the sample, without catalog interpretation.

| Stat | Total stat | Total T | Boost stat | Raw boost U | Level | Gear bonus | Matching class cap |
| --- | ---: | ---: | ---: | ---: | ---: | --- | --- |
| ATT | 20 | 57 | 48 | 2 | 0 | Unavailable | Unavailable |
| DEF | 21 | 32 | 49 | 10 | 0 | Unavailable | Unavailable |
| SPD | 22 | 32 | 50 | 0 | 0 | Unavailable | Unavailable |
| VIT | 26 | 28 | 51 | 0 | 0 | Unavailable | Unavailable |
| WIS | 27 | 57 | 52 | 10 | 0 | Unavailable | Unavailable |
| DEX | 28 | 55 | 53 | 0 | 0 | Unavailable | Unavailable |
| HP | 0 | 707 | 46 | 120 | 0 | Unavailable | Unavailable |
| MP | 3 | 362 | 47 | 75 | 0 | Unavailable | Unavailable |

Raw boosts retain their wire meaning. With zero exalt, the two proposed placement formulas coincide, so this
sample cannot select either one. In particular SPD's zero boost/zero level does not resolve the earlier
nonzero-level SPD counter-case. Gate P still requires matching catalog caps/gear and controlled nonzero-exalt
observations/transitions. No cap, maxed-count or pots-left conclusion follows from the unavailable catalog.

## Multi-account observations and code-reading risks

The owner reports a second account sharing the usual account's Maxed database. Within this capture there is
one distinct local account-stat identity, one character number and one local display identity across three
connections. Identity inputs are present at every materialization; no local account/character switch is visible.
Other players in Nexus are not evidence of another locally played account. No database was supplied, so existing
mixed history, actual UI retention and personal-best outcomes cannot be verified from this capture.

The following references are at the decoder/code-reading revision above. Risks are distinguished from observed
capture facts; none is a demonstrated collision or an instruction to change code in this task.

- **Character identity has account scoping.** `crates/maxed-engine/src/engine.rs:768` uses CREATE_SUCCESS to
  associate the local object and character number; provisional identity includes stream/map/create sequence.
  `crates/maxed-engine/src/world.rs:290` hashes nonempty account stat 38.
  `crates/maxed-engine/src/identity.rs:4` and `:11` derive a keyed account hash and then a character hash from
  that account hash plus character number. `crates/maxed-engine/src/characters.rs:98` marks that identity
  verified; names are display-only. Equal character numbers on different correctly observed accounts therefore
  do **not** by themselves collide. Missing identity leaves provisional history; incorrect association or reuse
  of the same account/character-number pair would require separate lifecycle evidence. This capture tests neither.
- **Stored history depends on that derived identity.** `crates/maxed-store/migrations/001_initial.sql:4` keys
  characters by derived ID; runs at `:7` reference it and preserve an entry snapshot. There is no separate
  account selector/key column in these tables. `crates/maxed-store/src/project.rs:264` upserts by character ID;
  `:809` merges aliases and rekeys run/fame/death history. An erroneous identity merge could therefore propagate
  into history. Correct cross-account keys prevent the simple character-number collision scenario; this is code
  reasoning, not a review of the owner's stored rows.
- **The live rail is snapshot-scoped, not a database-wide character lookup.**
  `app/ui/src/lib/shell/Shell.svelte:146` supplies a retained live observation;
  `app/ui/src/lib/shell/rail.ts:147` reads its character. The retention policy resets on reconnecting bridge or
  stream change at `app/ui/src/lib/home/mode.ts:49`, clears on run/character change at `:69`, and marks paused
  observations at `:78`. Before a new identity arrives, an older snapshot can remain visible as paused; an
  account switch still needs Windows UI testing. Shared database contents alone do not prove rail mixing.
  Engine connection replacement also clears local/entity/character state at `crates/maxed-engine/src/engine.rs:741`.
- **Home summaries and some records can mix accounts by scope.**
  `crates/maxed-store/src/home_scope.sql:1` selects session or time, not account; its fame baselines partition
  by character at `:24` and then aggregate. `crates/maxed-store/src/home.sql:20` and
  `crates/maxed-store/src/dungeon_bests.sql:1` select all-time clears by dungeon across the database.
  `crates/maxed-store/src/records_runs.sql:11` selects earlier runs by dungeon without character/account filtering.
  Thus Today, a shared session and dungeon clear records can combine both accounts even with correct identities.
  Home boss performance baselines accept an optional character filter at
  `crates/maxed-store/src/home_boss_baselines.sql:17`; a null filter includes all characters.
- **DPS personal bests have stronger scoping than dungeon clear records.**
  `crates/maxed-store/migrations/004_alerts.sql:1` keys them by character and boss.
  `crates/maxed-store/src/alerts.rs:187` resolves aliases, and `:217` reads that character/boss baseline.
  `crates/maxed-engine/src/characters.rs:24` requires verified identity and complete eligible fight evidence.
  `crates/maxed-store/src/records.rs:116` likewise keys previous DPS comparisons by character/boss. Correct
  account-derived character IDs keep these baselines separate; absent/wrong identity can prevent attribution or
  contaminate it if wrongly merged. The capture contains no eligible fight comparison to test this behavior.
- **Class/cap assumptions remain observation-dependent.** `crates/maxed-engine/src/characters.rs:105` takes class
  from the current entity; `:55` looks up that class in the catalog, while `:15` compares total minus raw boost
  to caps without the planned exalt gate. Sharing a database does not establish common caps or exalt levels
  across accounts. Missing/stale class/catalog data and applying one account's exalt assumptions to another are
  risks requiring the shared split and live transition checks; this catalog-free sample establishes no maxed state.

## Task 11 Windows acceptance checklist — all not tested here

This checklist remains unexecuted. The production approval above supersedes the original temporary
flag-on branch and flag-off baseline requirements; it does not establish Windows live acceptance.
Record **pass / fail / not-tested**, evidence, command exit codes/test counts and remaining limitations for each
item. Task 11 depends on Tasks 1–10 and must retain separate U, P, X and coefficient dispositions.

- [ ] Record current main SHA, Windows/client/Npcap/WebView2 versions, matching catalog fingerprint, synthetic
  identity, capture coverage and owner confirmation. Reconfirm this U disposition and explicitly retain P limits.
- [ ] Record the production-enabled main SHA and acceptance artifact revision for reproducibility.
  No temporary flag flip is needed after production enablement.
- [ ] Build the clean native Windows artifact using the plan's locked dependency/UI/cross-build workflow.
  Record artifact SHA-256; verify the privately transferred zip hash and BUILD-INFO on Windows. Run the real
  WebView2 screen with Npcap, without a preview fixture/query. A browser preview or link-only build is insufficient.
- [ ] Run full deterministic and route-flag checks on the production-enabled main baseline, including explicit
  flag-off fallback coverage. Run focused/native checks on the artifact revision; report scopes separately.
  Rebuild/retest after separately reviewed code fixes.
- [ ] **Weapon coefficient gate:** compare supported ordinary firing/damage with controlled current-client ATT,
  DEX, conditions, defense, piercing, exclusive roll bounds and rounding. Verify normal-rate coefficients and
  rate-factor units. Record unsupported schedules/effects as exclusions; do not use stat 113 in the baseline.
- [ ] Exercise equipment/stat/condition changes, death, capture stop, reconnect and identity/account transitions.
  No stale result may be Live; picks clear on character/stream change. Verify missing assets recover and capture
  survives catalog rebuild failure. Damage attribution, Party requirements and personal bests must not be
  recomputed from calculator output. Include two accounts sharing a database and distinguish aggregate history.
- [ ] **Regeneration coefficient gate:** verify current-client coefficients and XML unit normalization with
  controlled WIS, supported flat/percentage enchants, pets and combat/healing effects. Investigate/document the
  excluded base MP constant without assigning a v1 value. Missing verification blocks production acceptance;
  it never justifies guessed zero base VIT/MP or automatic out-of-combat doubling.
- [ ] Verify Windows WebView2 at 100%/150% scaling: keyboard input, Reset, comparisons, four slots, readable
  exclusions, partial states and long reason text. Keep any screenshots sanitized and free of game art/identities.
- [ ] Execute native application and assets tests on Windows, plus the focused commands below. Record actual
  test counts and exit codes; zero-test filters and macOS-linked Windows binaries are not execution passes.
- [ ] Record owner acceptance/refusal, remaining gate failures and explicit not-tested items. Verify Task 11's PR
  contains only its owned docs. Production enablement was separately owner-approved before this acceptance;
  installer acceptance remains a separate release activity.

Focused Windows checks from Task 11 (not run for this production enablement):

```text
cargo test --locked -p maxed-app --lib runtime::character_tests::
cargo test --locked -p maxed-engine --test integration character_calculation::
npm --prefix app/ui run test:ui -- tests/character
npm --prefix app/ui run check
cargo test --locked -p maxed-app
cargo test --locked -p maxed-assets
git diff --check
```

Also record the full Windows checks on the production-enabled revision: `cargo test --locked --workspace`,
`cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets -- -D warnings`, and
`cargo clippy --locked -p maxed-app --all-targets --features perf-live -- -D warnings`.
Neither this U pass nor later baseline acceptance unlocks the separate exalt-damage gate.
