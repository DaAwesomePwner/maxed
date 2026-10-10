# Kill evidence

Oryx announces deaths with `stringlist.<Boss>.death.N` as well as `.killed.N`,
often as duplicated concatenated objects. The detector confirms a matching
announcement plus target removal within 500 ms in either direction,
independently of final HP. HP1 alone and quest switches do not establish a kill.

The committed realm-focus fixture has irreversibly filled announcement fields.
Its conservative replay expectations remain necessary until a reviewed
replacement preserves those fields with the current scrubber. The existing
capture cannot establish whether a matching announcement accompanies removal.

The fixture's [README](../../tests/fixtures/scrubbed/maxed-realm-focus/README.md)
and [expected.json](../../tests/fixtures/scrubbed/maxed-realm-focus/expected.json)
record provenance, object types, observed fight totals and capture limitations.
No client catalog or private capture is needed for this investigation.

## Evidence rules

The older realm-focus fixture contains four final HP1/removal sequences but no HP0, Victory or quest clear for those targets. Lost Sentry provides a separate HP0/removal control. The current regression checks the exact indices and timestamps; blanked announcement text is unavailable evidence.

A same-generation HP0 followed by removal within two seconds, without positive HP intervening, confirms a kill. Victory can bind a current/recently cleared quest within one second; a sole recently hit catalog boss is a provisional fallback. A recognized Oryx death/killed announcement plus matching removal within 500 ms in either order confirms independently of final HP. Announcements without local corroboration remain provisional instance evidence. Out-of-view announcements never create a fight. See the [ledger](deliberate-differences.md) for scope and regression names.

The [Plague Doctor fixture](../../tests/fixtures/scrubbed/maxed-realm-plague-doctor/README.md) preserves the HP1 announcement/removal case. The [realm return fixture](../../tests/fixtures/scrubbed/maxed-realm-dungeon-return/README.md) supplies multi-visit continuity evidence. Neither makes the older realm-focus fixture's filled text recoverable.

## A quest switch and drop can also accompany disappearance and return

Sword provides an earlier, distinct sequence before its final HP1 removal:

1. `i15583`: HP17910 at 1791044723949537.
2. `i15647`: quest target switches from Sword to Legion at 1791044724050184.
3. `i15708`: Sword is dropped at 1791044724151971, 101,787 us after the switch.
4. `i24562`: object ID 239725, type 34553, appears again at
   1791044742633674 with HP15080, at the same position (1026.5, 1249.5).

That is compatible with leaving and returning to visibility while other players
continue damaging it. A drop removes the object from the observed set; the wire
packet does not state a death reason. Reusing the same ID, type and position is
strong continuity evidence, but not independent proof that it is the same server
creature. The engine's generation is derived locally, not a server incarnation
identifier, and must not be treated as additional confirmation.

This earlier sequence rules out treating an arbitrary quest switch plus drop as
sufficient kill evidence. It does **not** prove that final HP1 drops are visibility
placeholders: this fixture contains no HP1-drop-and-positive-HP-return example.

## Original investigation and next fixture evidence

Do not broaden `HpZeroRemoved` to HP <= 1 from this sample. The four final
sequences are plausible deaths, yet an HP1 threshold would turn a hypothesis into
a confirmed kill for other targets and future visibility/phase transitions. A
boss-only threshold also needs justified catalog classification; these four
objects have no boss/miniboss/event-boss flags in the supplied catalog metadata.

To support a new kill rule, collect independent confirmation of a terminal
sequence (for example a synchronized visible death/reward, preserved recognized
announcement, or documented target-specific terminal behavior), together with
HP1 departures and phase transitions that do not kill the target. The current detector
uses the separate announcement/removal evidence rule. The current capture cannot distinguish all those cases. Blanked announcement text
cannot be used to assert either presence or absence of a relevant announcement.

The fixture starts at reconnect, so neither verified local identity nor complete
capture is established. Max HP minus observed damage is not a death test: damage
before arrival and outside the local observation scope is missing, and max HP
changes during these encounters. Focus selection therefore requires active
engagement/idle focus handling independently of kill detection.

## Reproduction and regression

Run from the repository root with the pinned Rust toolchain:

```powershell
cargo test --offline --locked -p maxed-cli --test realm_kill_evidence
cargo run --offline --locked -p maxed-cli -- decode tests/fixtures/scrubbed/maxed-realm-focus/realm-visit.pcap --output recordings/realm-kill-evidence.jsonl
```

The decoder requires a new output path; use another filename when repeating it.
The ignored export is diagnostic output, not another committed fixture.

The [regression test](../../crates/maxed-cli/tests/realm_kill_evidence.rs) checks the
fixture SHA-256, verified decode, exact terminal HP/drop sequences, absence of
Victory and quest clear, and Sword's earlier return. It also runs the actual
engine with fixture metadata: the four quest targets receive no inferred kills,
while the Sentry HP0 control still receives `HpZeroRemoved`. Those assertions
preserve the present conservative evidence policy, not a claim about the
unobserved server outcome.

Decode result: 11,303 frames, 28,206 decoded packets, 9,655 typed Unknown packets,
zero failures, zero unverified packets, no truncation. Unknown packets are not
interpreted as death evidence; their presence is another limit on conclusions
about the entire server protocol.
