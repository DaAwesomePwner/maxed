use crate::common::*;
use maxed_engine::{Engine, PROFILE_LIMIT};
use maxed_types::*;
use std::{collections::BTreeSet, sync::Arc};

fn presence(events: Vec<EngineEvent>, engine: &mut Engine) -> Vec<PlayerPresence> {
    assert!(events.iter().all(|event| serde_json::to_value(event)
        .unwrap()
        .get("player_presence")
        .is_none()));
    let updates = engine.take_roster_presence();
    assert!(updates
        .iter()
        .all(|update| update.event_index <= events.len()));
    assert!(updates
        .windows(2)
        .all(|pair| pair[0].event_index <= pair[1].event_index));
    assert!(engine.take_roster_presence().is_empty());
    updates.into_iter().map(|update| update.presence).collect()
}

fn enter(e: &mut Engine, at: u64, id: i32) -> PlayerPresence {
    let rows = presence(
        apply(
            e,
            at,
            update(
                vec![object(id, 100, vec![string(31, "SynthAlpha"), stat(7, 20)])],
                vec![],
            ),
        ),
        e,
    );
    assert_eq!(rows.len(), 1);
    rows.into_iter().next().unwrap()
}

#[test]
fn remote_player_enters_view_emits_present() {
    let mut e = engine();
    setup(&mut e);
    let p = enter(&mut e, 3000, 3);
    assert!(p.present);
    assert_eq!(p.object_id, 3);
    assert_eq!(p.at, Timestamp(3000));
    let events = e.advance_to(Timestamp(250_000));
    let profile = events
        .iter()
        .find_map(|event| match event {
            EngineEvent::PlayerObserved(p) if p.object_id == 3 => Some(p),
            _ => None,
        })
        .unwrap();
    assert_eq!(p.scope_id, profile.scope_id);
    assert_eq!(p.player_id, profile.player_id);
    assert_eq!(p.run_id, profile.run_id);
    assert_eq!(p.run_id.as_deref(), Some("test:run:1"));
    assert!(presence(events, &mut e).is_empty());
}

#[test]
fn remote_player_dropped_emits_absent() {
    let mut e = engine();
    setup(&mut e);
    let p = enter(&mut e, 3000, 3);
    let rows = presence(apply(&mut e, 4000, update(vec![], vec![3])), &mut e);
    assert_eq!(
        rows,
        [PlayerPresence {
            present: false,
            at: Timestamp(4000),
            ..p
        }]
    );
    assert!(presence(apply(&mut e, 5000, update(vec![], vec![3])), &mut e).is_empty());
}

#[test]
fn non_player_objects_emit_nothing() {
    let mut e = engine();
    setup(&mut e);
    assert!(presence(
        apply(&mut e, 3000, update(vec![object(3, 301, vec![])], vec![]),),
        &mut e
    )
    .is_empty());
    assert!(presence(apply(&mut e, 4000, update(vec![], vec![3])), &mut e).is_empty());
}

#[test]
fn same_type_reentry_reuses_player_id() {
    let mut catalog = catalog();
    let mut other = catalog.objects[0].clone();
    other.object_type = 101;
    catalog.objects.push(other);
    let mut e = Engine::new(config(), Arc::new(catalog)).unwrap();
    setup(&mut e);
    let p = enter(&mut e, 3000, 3);
    apply(&mut e, 4000, update(vec![], vec![3]));
    let reentry = enter(&mut e, 5000, 3);
    assert!(reentry.present);
    assert_eq!(reentry.player_id, p.player_id);
    apply(&mut e, 6000, update(vec![], vec![3]));
    let replacement = presence(
        apply(
            &mut e,
            7000,
            update(vec![object(3, 101, vec![string(31, "SynthBravo")])], vec![]),
        ),
        &mut e,
    );
    assert_eq!(replacement.len(), 1);
    assert!(replacement[0].present);
    assert_ne!(replacement[0].player_id, p.player_id);
}

#[test]
fn scope_end_marks_present_players_absent() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    let a = enter(&mut e, 1000, 3);
    let b = enter(&mut e, 2000, 4);
    let events = apply(&mut e, 3000, map("The Shatters"));
    let next_map = events
        .iter()
        .position(|event| matches!(event, EngineEvent::MapEntered { .. }))
        .unwrap();
    let updates = e.take_roster_presence();
    assert!(updates.iter().all(|update| update.event_index <= next_map));
    let rows = updates
        .into_iter()
        .map(|update| update.presence)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|p| !p.present
        && p.at == Timestamp(3000)
        && p.scope_id == a.scope_id
        && p.run_id == a.run_id));
    assert_eq!(
        rows.iter().map(|p| &p.player_id).collect::<BTreeSet<_>>(),
        BTreeSet::from([&a.player_id, &b.player_id])
    );
    assert!(presence(events[next_map..].to_vec(), &mut e).is_empty());
    let next = enter(&mut e, 4000, 3);
    assert_ne!(next.scope_id, a.scope_id);
    assert_ne!(next.player_id, a.player_id);
}

#[test]
fn presence_respects_profile_cap() {
    for same_packet in [false, true] {
        let mut e = engine();
        apply(&mut e, 0, map("The Void"));
        // Descending IDs ensure admission agrees even before the profile flush.
        let rows = presence(
            apply(
                &mut e,
                1000,
                update(
                    (1..=PROFILE_LIMIT as i32 + i32::from(same_packet))
                        .rev()
                        .map(|id| object(id, 100, vec![string(31, "SynthAlpha")]))
                        .collect(),
                    vec![],
                ),
            ),
            &mut e,
        );
        assert_eq!(rows.len(), PROFILE_LIMIT);
        let events = e.advance_to(Timestamp(250_000));
        let profiled: BTreeSet<_> = events
            .iter()
            .filter_map(|event| match event {
                EngineEvent::PlayerObserved(p) => Some(p.player_id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(profiled, rows.iter().map(|p| p.player_id.clone()).collect());
        let overflow = if same_packet {
            1
        } else {
            PROFILE_LIMIT as i32 + 1
        };
        assert!(presence(
            apply(
                &mut e,
                260_000,
                update(vec![object(overflow, 100, vec![])], vec![])
            ),
            &mut e
        )
        .is_empty());
        let admitted = rows[0].object_id;
        assert_eq!(
            presence(
                apply(&mut e, 270_000, update(vec![], vec![admitted])),
                &mut e
            )
            .len(),
            1
        );
        assert_eq!(
            enter(&mut e, 280_000, admitted).player_id,
            rows[0].player_id
        );
        assert!(presence(
            apply(&mut e, 290_000, update(vec![], vec![overflow])),
            &mut e
        )
        .is_empty());
        assert!(e.snapshot().issues.contains(&CoverageIssue::ResourceLimit));
    }
}

#[test]
fn local_player_presence_is_emitted() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let rows = presence(apply(&mut e, 2000, update(vec![player()], vec![])), &mut e);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].present);
    let events = e.advance_to(Timestamp(250_000));
    let profile = events
        .iter()
        .find_map(|event| match event {
            EngineEvent::PlayerObserved(p) if p.is_local => Some(p),
            _ => None,
        })
        .unwrap();
    assert_eq!(rows[0].player_id, profile.player_id);
    assert_eq!(rows[0].object_id, 1);
}

#[test]
fn repeated_observations_emit_only_presence_transitions() {
    let mut e = engine();
    setup(&mut e);
    enter(&mut e, 3000, 3);
    assert!(presence(
        apply(&mut e, 4000, update(vec![object(3, 100, vec![])], vec![])),
        &mut e
    )
    .is_empty());
    assert!(presence(
        apply(&mut e, 5000, tick(vec![status(3, vec![stat(30, 20)])])),
        &mut e
    )
    .is_empty());
    apply(&mut e, 6000, update(vec![], vec![3]));
    let rows = presence(apply(&mut e, 7000, tick(vec![status(3, vec![])])), &mut e);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].present);
}

#[test]
fn repeated_tick_merges_of_present_player_emit_no_presence() {
    let mut e = engine();
    setup(&mut e);
    let initial = enter(&mut e, 3000, 3);
    for stars in 1..=100 {
        assert!(presence(
            apply(
                &mut e,
                3000 + stars as u64 * 1000,
                tick(vec![status(3, vec![stat(30, stars)])]),
            ),
            &mut e
        )
        .is_empty());
    }
    let rows = presence(apply(&mut e, 104_000, update(vec![], vec![3])), &mut e);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].present);
    assert_eq!(rows[0].player_id, initial.player_id);
}

#[test]
fn outside_runs_presence_has_no_run_id() {
    for name in ["Nexus", "Realm"] {
        let mut e = engine();
        apply(&mut e, 0, map(name));
        let p = enter(&mut e, 1000, 3);
        assert_eq!(p.scope_id, "test:map:1");
        assert_eq!(p.run_id, None);
        let rows = presence(e.finish(Timestamp(2000), FightEnd::CaptureStopped), &mut e);
        assert_eq!(
            rows,
            [PlayerPresence {
                present: false,
                at: Timestamp(2000),
                ..p
            }]
        );
        assert!(presence(e.finish(Timestamp(3000), FightEnd::CaptureStopped), &mut e).is_empty());
    }
}

#[test]
fn scope_end_does_not_repeat_departures_or_lose_rekeyed_local_identity() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let local = enter(&mut e, 2000, 1);
    enter(&mut e, 3000, 3);
    apply(&mut e, 4000, update(vec![], vec![3]));
    let events = apply(
        &mut e,
        5000,
        tick(vec![status(1, vec![string(38, "synthetic-account")])]),
    );
    let verified = events
        .iter()
        .find_map(|event| match event {
            EngineEvent::CharacterMerged {
                provisional_id,
                verified_id,
            } => {
                assert_eq!(provisional_id, &local.player_id);
                Some(verified_id.clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(presence(events, &mut e).is_empty());
    let rows = presence(e.finish(Timestamp(6000), FightEnd::CaptureStopped), &mut e);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].present);
    assert_eq!(rows[0].player_id, verified);
    assert_eq!(rows[0].object_id, 1);
}

#[test]
fn present_player_replaced_by_enemy_emits_absent() {
    let mut e = engine();
    setup(&mut e);
    let p = enter(&mut e, 3000, 3);
    let rows = presence(
        apply(&mut e, 4000, update(vec![object(3, 301, vec![])], vec![])),
        &mut e,
    );
    assert_eq!(
        rows,
        [PlayerPresence {
            present: false,
            at: Timestamp(4000),
            ..p
        }]
    );
}

#[test]
fn buffered_local_player_enters_presence_when_scope_starts() {
    let mut e = engine();
    apply(&mut e, 0, create(1, 1));
    assert!(presence(apply(&mut e, 1000, update(vec![player()], vec![])), &mut e).is_empty());
    let rows = presence(apply(&mut e, 2000, map("The Void")), &mut e);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].present);
    assert_eq!(rows[0].scope_id, "test:run:1");
    assert_eq!(rows[0].run_id.as_deref(), Some("test:run:1"));
    assert_eq!(rows[0].object_id, 1);
    let absent = presence(e.finish(Timestamp(3000), FightEnd::CaptureStopped), &mut e);
    assert_eq!(
        absent,
        [PlayerPresence {
            present: false,
            at: Timestamp(3000),
            ..rows[0].clone()
        }]
    );
}

#[test]
fn all_buffered_present_players_enter_presence_when_scope_starts() {
    let mut e = engine();
    apply(&mut e, 0, create(1, 1));
    apply(
        &mut e,
        1000,
        update(
            vec![
                player(),
                object(3, 100, vec![string(31, "SynthBravo")]),
                object(4, 100, vec![]),
            ],
            vec![],
        ),
    );
    apply(&mut e, 1500, update(vec![], vec![4]));
    let rows = presence(apply(&mut e, 2000, map("The Void")), &mut e);
    assert_eq!(
        rows.iter().map(|p| p.object_id).collect::<BTreeSet<_>>(),
        BTreeSet::from([1, 3])
    );
    assert!(rows.iter().all(|p| p.present && p.scope_id == "test:run:1"));
}

#[test]
fn undrained_presence_does_not_accumulate_across_engine_updates() {
    let mut e = engine();
    apply(&mut e, 0, map("Nexus"));
    for at in 1..=100 {
        apply(&mut e, at * 2, update(vec![object(3, 100, vec![])], vec![]));
        apply(&mut e, at * 2 + 1, update(vec![], vec![3]));
    }
    let updates = e.take_roster_presence();
    assert_eq!(updates.len(), 1);
    assert!(!updates[0].presence.present);
    apply(&mut e, 300, update(vec![object(3, 100, vec![])], vec![]));
    let events = e.advance_to(Timestamp(400));
    assert!(presence(events, &mut e).is_empty());
}

#[test]
fn same_packet_presence_precedes_local_identity_merge() {
    let mut e = engine();
    apply(&mut e, 0, map("Nexus"));
    apply(&mut e, 1000, create(1, 1));
    let events = apply(
        &mut e,
        2000,
        update(
            vec![
                object(1, 100, vec![string(31, "SynthAlpha")]),
                object(1, 100, vec![string(38, "synthetic-account")]),
            ],
            vec![],
        ),
    );
    let updates = e.take_roster_presence();
    assert_eq!(updates.len(), 1);
    let (index, provisional_id) = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            EngineEvent::CharacterMerged { provisional_id, .. } => Some((index, provisional_id)),
            _ => None,
        })
        .unwrap();
    assert!(updates[0].event_index <= index);
    assert_eq!(&updates[0].presence.player_id, provisional_id);
}
