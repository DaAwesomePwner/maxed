# Personal-best eligibility

DPS personal bests are per verified character and catalog boss type. The first eligible fight establishes a baseline silently; a later eligible improvement may alert once. Run-time bests compare the exact dungeon across characters.

Reconstructed sources retain `client-projectile-reconstruction` provenance.
Deterministic reconstruction with known projectiles, seed and stats is
eligible; an uncertain input assumption or RNG synchronization marks it
estimated. Explicitly empty Crucible/Blood Ritual strings mean observed
inactive channels with multiplier 1. An omitted secondary condition (96) on a
typed target UPDATE has a known zero baseline. Neither case increments
missing-input diagnostics. Unknown active bonus IDs, missing bonus channels,
target states seen only through deltas, missing attacker stats, missing
scaling/enchant metadata, ambiguous hits and coverage gaps remain ineligible.
Eligibility also requires settled local pairs and the current DPS basis.

The engine publishes a confirmed kill as an ended `FightUpdated` before
`FightEnded`. PB evaluation accepts terminal updates, including a later
settled eligible revision. Live context and the absence of an alert occurrence
decide whether it can alert, not the stored fight's end flag. Re-publication
cannot alert twice.

When the owning record changes, the store recomputes the best other eligible
fight for that character/boss from persisted fight projections, then compares
the revision. Lowered or ineligible revisions therefore restore the correct
baseline. Migration 7 indexes the owning fight lookup and finalized boss
lookup; repair selects that boss, the current DPS basis, and the canonical
character or its verified aliases before deserializing candidate fights. An
already delivered alert cannot be retracted.

The Shaitan replay uses a minimal synthetic catalog subset with neutral
metadata for the recorded enchant and Crucible IDs. It establishes that those
source paths preserve known neutral evidence; it cannot establish the
completeness or identity of a capture starting at reconnect. A separate fully
observed synthetic boss fight proves complete capture, clean issues and an
eligible candidate after Victory. Confirmed-kill closure behavior remains
covered separately.

Run-time alerts compare `completed_at - started` with Home's history-wide best
for the exact dungeon, across characters. The previous baseline uses Home's
Victory/completion-dialogue evidence rule; the new candidate requires Victory,
a cleared outcome. The engine establishes both at Victory, so the alert fires
on that RunUpdated, before looting or departure. Partial stats do not
participate. The current run is excluded from the baseline even if RunUpdated
already stored its completion. Run IDs are durable occurrence keys; the
occurrence is checked before the history minimum query, so RunEnded and later
RunUpdated revisions neither duplicate the alert nor repeat that comparison.
Adapters default to non-live; only explicitly live contexts may create DPS or
run-time PB alerts. Database deletion has a separate explicit ephemeral flag,
also false by default; a non-live adapter does not imply disposable data.
Offline DPS baselines still update, with suppressed receipts preventing later
celebration. Existing Personal Best loudness, sound and delivery apply. First
clears establish a baseline silently.

### Typed UPDATE evidence and RNG trust

The committed Shaitan fixture's typed target UPDATE provides defense (21) and
primary condition (29), while omitting secondary condition (96). The historical parity audit
also documents omitted 96; neither establishes omit-means-zero for 21 or 29.
Accordingly, every typed newObjects UPDATE clears stale 21/29/96 values and
establishes only omitted 96 as known zero. Omitted 21/29 uses uncertain zero for
reconstruction and blocks PB eligibility. Delta NEWTICK updates still merge.
Synthetic view-exit/re-entry tests cover all three omitted stats.

Known stats alone cannot establish reconstruction accuracy. Map entry seeds the
RNG and establishes trust. A stream gap, PLAYERSHOOT (id 30) decode failure,
or PlayerShoot whose identity/item/projectile/seed prevents advancement
invalidates trust until the next map seed, even when no fight is open. Other
packet decode failures retain their coverage marking without invalidating RNG.
Subsequent local reconstructions are estimated and mark `MissingStats`
(`untrustedRng` assumption counter).

A paired server DAMAGE disagreement instead marks only its fight `AmbiguousHit`.
Target condition timing, server immunity and kill-shot clamps can explain a
mismatch without RNG drift. If RNG drift causes later mismatches, those fights
are tainted individually; proven gaps/lost PLAYERSHOOTs still latch distrust.
Matching server reconciliation keeps the exact local ENEMYHIT activity timestamp
with the server amount in either arrival order. Pending server-first timestamps
remain replaceable until pair expiry, so the activity window and three-second
DPS/PB cutoff are independent of packet order.

`pending_local_reconciliation` blocks DPS PB candidates while local hit pairs
await their counterpart (in either arrival order). A matching server report
makes the terminal revision eligible; a mismatch taints the fight and prevents
an alert. A server counterpart arriving after a newer tick still reconciles its
existing pre-kill estimate at the original hit time; new post-kill hits stay excluded. Pair expiry accepts the reliable estimate and queues a new revision.
Map change, capture stop and app close finalize remaining pairs before publishing
`FightEnded`, so a confirmed kill can still record and alert its PB at retirement.
Run-time PBs continue to fire at Victory independently of hit reconciliation.

Fight projections persist `dps_basis`: new engine output uses
`player_active_window_v1` (each player's active window). Per-player DPS is
unavailable (null) when that player's active window is shorter than three
seconds. JSON without the field loads as `whole_target_v0`. Legacy fights
remain readable but cannot seed or restore DPS PBs. Both candidate eligibility
and the repair query require the current basis.

Migration 7 clears all existing `personal_bests` rows because their whole-target
DPS basis is incompatible. DPS PBs restart silently from the first eligible fight
under the new basis; a later improvement can alert once. Fights, runs and alert
history are preserved. Run-time PB baselines are computed from runs and are not
reset. Run-level DPS retains its run-wide window without a minimum. Revision
repair uses eligible current-basis stored fights only, not missing historical rows.

No in-game verification is claimed. Release testing must verify both DPS and clear-time
PB alerts with the actual catalog and capture running before character selection.

Saved Runs history computes each clear-time PB within the run's own account,
resolved through character aliases. The Runs page still lists all accounts and
legacy rows: accountless runs retain observed clear timing but neither establish
nor beat a PB, and have no previous-best baseline. Best and previous-best values,
list summary PB counts, comparisons, recaps and dungeon bests use this same scope.
Equal clear times choose the earliest start, then run ID, for the Runs PB marker.
