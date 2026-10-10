mod common;
mod d_support;
use d_support::*;
use maxed_types::*;

#[test]
fn non_dungeon_removals_never_become_departures() {
    for name in ["Realm", "Nexus", "Unknown area"] {
        let mut e = engine();
        apply(&mut e, 0, map(name));
        apply(&mut e, 1000, create(1, 1));
        apply(
            &mut e,
            2000,
            update(vec![player(), player_obj(3, 100, "SynthBravo")], vec![]),
        );
        assert!(instances(apply(&mut e, 5000, update(vec![], vec![3]))).is_empty());
        assert!(instances(e.advance_to(Timestamp(10_000_000))).is_empty());
        assert!(instances(apply(&mut e, 11_000_000, map("The Void"))).is_empty());
        assert!(instances(e.finish(Timestamp(12_000_000), FightEnd::CaptureStopped)).is_empty());
    }
}

#[test]
fn run_end_emits_once_with_original_time_and_run() {
    for ending in 0..3 {
        let mut e = remote();
        let run_id = e.snapshot().run.unwrap().id;
        apply(&mut e, 5000, update(vec![], vec![3]));
        assert!(instances(e.advance_to(Timestamp(10_000_000))).is_empty());
        let events = match ending {
            0 => apply(&mut e, 11_000_000, map("Nexus")),
            1 => apply(&mut e, 11_000_000, death()),
            _ => e.finish(Timestamp(11_000_000), FightEnd::CaptureStopped),
        };
        let rows = instances(events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].at, Timestamp(5000));
        assert_eq!(rows[0].run_id.as_deref(), Some(run_id.as_str()));
        assert_eq!(rows[0].map.as_deref(), Some("The Void"));
        assert!(rows[0].provisional);
        assert!(matches!(
            rows[0].kind,
            InstanceEventKind::PlayerLeft { object_id: 3, .. }
        ));
        assert!(instances(e.finish(Timestamp(12_000_000), FightEnd::CaptureStopped)).is_empty());
    }
}

#[test]
fn late_reentry_or_death_cancels_until_run_end() {
    for dies in [false, true] {
        let mut e = remote();
        apply(&mut e, 5000, update(vec![], vec![3]));
        assert!(instances(e.advance_to(Timestamp(10_000_000))).is_empty());
        if dies {
            let rows = instances(apply(
                &mut e,
                11_000_000,
                player_death("SynthBravo", 20, "Synthetic Boss", 9),
            ));
            assert_eq!(rows.len(), 1);
            assert!(matches!(
                rows[0].kind,
                InstanceEventKind::PlayerDeath {
                    object_id: Some(3),
                    ..
                }
            ));
        } else {
            apply(
                &mut e,
                11_000_000,
                update(vec![player_obj(3, 100, "SynthBravo")], vec![]),
            );
        }
        assert!(instances(apply(&mut e, 12_000_000, map("Nexus"))).is_empty());
    }
}

#[test]
fn victory_keeps_pending_until_actual_run_end() {
    let mut e = remote();
    apply(&mut e, 5000, update(vec![], vec![3]));
    assert!(instances(apply(&mut e, 10_000_000, victory())).is_empty());
    assert!(e.snapshot().run.unwrap().completed_at.is_some());
    let rows = instances(apply(&mut e, 11_000_000, map("Nexus")));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at, Timestamp(5000));
}

#[test]
fn departures_at_or_after_victory_are_dropped_at_every_run_ending() {
    for leave_at in [10_000_000, 10_001_000, 11_000_000] {
        for ending in 0..3 {
            let mut e = remote();
            apply(&mut e, 10_000_000, victory());
            assert!(instances(apply(&mut e, leave_at, update(vec![], vec![3]))).is_empty());
            let events = match ending {
                0 => apply(&mut e, 12_000_000, map("Nexus")),
                1 => apply(&mut e, 12_000_000, death()),
                _ => e.finish(Timestamp(12_000_000), FightEnd::CaptureStopped),
            };
            assert!(
                instances(events).is_empty(),
                "leave_at={leave_at}, ending={ending}"
            );
        }
    }
}

#[test]
fn reentry_after_victory_cancels_a_pre_victory_departure() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 4000, update(vec![player_obj(3, 100, "")], vec![]));
    apply(&mut e, 5000, update(vec![], vec![3]));
    apply(&mut e, 10_000_000, victory());
    apply(
        &mut e,
        11_000_000,
        update(vec![player_obj(3, 100, "")], vec![]),
    );
    assert!(instances(apply(&mut e, 12_000_000, map("Nexus"))).is_empty());
}

#[test]
fn death_after_victory_cancels_a_pre_victory_departure() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 4000, update(vec![player_obj(3, 100, "")], vec![]));
    apply(&mut e, 5000, update(vec![], vec![3]));
    apply(&mut e, 10_000_000, victory());
    let rows = instances(apply(
        &mut e,
        11_000_000,
        player_death("", 20, "Synthetic boss", 9),
    ));
    assert!(matches!(
        rows[0].kind,
        InstanceEventKind::PlayerDeath {
            object_id: Some(3),
            ..
        }
    ));
    assert!(instances(apply(&mut e, 12_000_000, map("Nexus"))).is_empty());
}

#[test]
fn departure_finalized_before_later_victory_keeps_its_original_time() {
    let mut e = remote();
    apply(&mut e, 5000, update(vec![], vec![3]));
    let rows = instances(apply(&mut e, 6000, death()));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at, Timestamp(5000));
    assert!(instances(apply(&mut e, 7000, victory())).is_empty());
    assert_eq!(
        e.snapshot().run.unwrap().completed_at,
        Some(Timestamp(7000))
    );
}

#[test]
fn local_death_does_not_count_local_removal_as_left() {
    let mut e = remote();
    apply(&mut e, 5000, update(vec![], vec![1, 3]));
    let rows = instances(apply(&mut e, 10_000_000, death()));
    assert_eq!(rows.len(), 1);
    assert!(matches!(
        rows[0].kind,
        InstanceEventKind::PlayerLeft { object_id: 3, .. }
    ));
}

#[test]
fn second_removal_after_reentry_uses_new_time() {
    let mut e = remote();
    apply(&mut e, 5000, update(vec![], vec![3]));
    apply(
        &mut e,
        10_000_000,
        update(vec![player_obj(3, 100, "SynthBravo")], vec![]),
    );
    apply(&mut e, 11_000_000, update(vec![], vec![3]));
    let rows = instances(apply(&mut e, 12_000_000, map("Nexus")));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at, Timestamp(11_000_000));
}

#[test]
fn transport_change_preserves_departures_until_old_run_ends() {
    let mut e = remote();
    apply(&mut e, 5000, update(vec![], vec![3]));
    let rows = instances(connection(&mut e, 2, 10_000_000, map("Nexus")));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at, Timestamp(5000));
    assert_eq!(rows[0].map.as_deref(), Some("The Void"));
}

#[test]
fn pending_departures_remain_bounded_after_entity_eviction() {
    let mut cfg = config();
    cfg.max_entities = 4;
    let mut e = maxed_engine::Engine::new(cfg, std::sync::Arc::new(catalog())).unwrap();
    setup(&mut e);
    for id in 3..10 {
        let at = u64::try_from(id).unwrap() * 40_000_000;
        apply(
            &mut e,
            at,
            update(vec![player_obj(id, 100, "Synthetic")], vec![]),
        );
        apply(&mut e, at + 1000, update(vec![], vec![id]));
    }
    let rows = instances(apply(&mut e, 400_000_000, map("Nexus")));
    assert_eq!(rows.len(), 4);
    for (row, id) in rows.iter().zip(3..7) {
        assert!(
            matches!(row.kind, InstanceEventKind::PlayerLeft { object_id, .. } if object_id == id)
        );
    }
}
