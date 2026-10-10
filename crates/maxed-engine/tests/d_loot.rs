mod common;
mod d_support;
use d_support::*;
use maxed_types::*;
#[test]
fn delayed_death_keeps_removal_loot_tick_and_can_update_an_evaluated_bag() {
    for delay in [100_000, 500_000, 500_001] {
        let mut e = started();
        apply(&mut e, 5000, tick(vec![status(2, vec![stat(1, 428)])]));
        apply(&mut e, 10_000, update(vec![], vec![2]));
        apply(
            &mut e,
            20_000,
            update(vec![object(10, 1292, vec![])], vec![]),
        );
        apply(&mut e, 30_000, tick(vec![]));
        // A replacement type evicts the original entity; its removal evidence
        // must retain the old generation's type and position independently.
        let mut replacement = object(2, 301, vec![]);
        replacement.status.pos.x = 100.;
        apply(&mut e, 35_000, update(vec![replacement], vec![]));
        if delay >= 500_000 {
            apply(&mut e, 40_000, tick(vec![]));
            assert_eq!(e.snapshot().drops[0].source_enemy, None);
        }
        let maxed_protocol::Packet::Text(mut text) =
            oryx_killed("Synthetic_300", "SyntheticKiller")
        else {
            unreachable!()
        };
        text.text = r#"{"k":"stringlist.Synthetic_300.death.0"}"#.into();
        let mut events = apply(&mut e, 10_000 + delay, maxed_protocol::Packet::Text(text));
        events.extend(apply(&mut e, 20_000 + delay, tick(vec![])));
        let bag = e.snapshot().drops.remove(0);
        if delay <= 500_000 {
            assert_eq!(bag.source_enemy, Some(300), "delay {delay}");
            assert_eq!(
                bag.source_reason.as_deref(),
                Some("killed-target-same-tick")
            );
            assert!(bag.source_estimated);
            assert!(events.iter().any(
                |event| matches!(event, EngineEvent::DropUpdated(b) if b.source_enemy == Some(300))
            ));
        } else {
            assert_eq!(bag.source_enemy, None);
        }
    }
}
#[test]
fn brown_pink_bags_are_excluded_even_when_catalog_requests_capture() {
    for with_catalog in [false, true] {
        let mut c = catalog();
        let types = [
            (1280, 1280),
            (1286, 1286),
            (1709, 1280),
            (1710, 1286),
            (60000, 1280),
            (60001, 1286),
        ];
        if with_catalog {
            for (ty, base_type) in types {
                c.bags.push(BagDef {
                    object_type: ty,
                    name: "Synthetic".into(),
                    base_type,
                    is_white: true,
                    rank: Some(255),
                    capture: true,
                });
            }
        }
        let mut e = maxed_engine::Engine::new(config(), std::sync::Arc::new(c)).unwrap();
        setup(&mut e);
        for (i, (ty, _)) in types.into_iter().enumerate() {
            let id = 10 + i as i32;
            let mut events = apply(&mut e, 3000, update(vec![object(id, ty, vec![])], vec![]));
            events.extend(apply(
                &mut e,
                4000,
                tick(vec![status(id, vec![stat(8, 200)])]),
            ));
            assert!(!events.iter().any(|event| matches!(
                event,
                EngineEvent::BagDropped(_) | EngineEvent::DropUpdated(_)
            )));
        }
        assert!(e.snapshot().drops.is_empty());
    }
}
#[test]
fn bags_are_excluded_only_in_hubs() {
    for kind in [
        AreaKind::Nexus,
        AreaKind::OtherHub,
        AreaKind::Realm,
        AreaKind::Dungeon,
        AreaKind::Unknown,
    ] {
        for ty in [1287, 1292] {
            let mut c = catalog();
            c.dungeons[0].kind = kind.clone();
            let mut e = maxed_engine::Engine::new(config(), std::sync::Arc::new(c)).unwrap();
            apply(&mut e, 0, map("Nexus"));
            apply(&mut e, 1000, create(1, 1));
            let mut events = apply(&mut e, 2000, update(vec![object(10, ty, vec![])], vec![]));
            events.extend(apply(
                &mut e,
                3000,
                tick(vec![status(10, vec![stat(8, 200)])]),
            ));
            if matches!(kind, AreaKind::Nexus | AreaKind::OtherHub) {
                assert!(!events.iter().any(|event| matches!(
                    event,
                    EngineEvent::BagDropped(_) | EngineEvent::DropUpdated(_)
                )));
                assert!(e.snapshot().drops.is_empty());
            } else {
                assert!(events.iter().any(|event| matches!(event, EngineEvent::BagDropped(b) if b.capture_scope == Some(CaptureScope::RealmShark))));
            }
        }
    }
}
#[test]
fn realmshark_scope_bags_unchanged() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(10, 1292, vec![])], vec![]));
    let b = e.snapshot().drops.remove(0);
    assert_eq!(b.capture_scope, Some(CaptureScope::RealmShark));
    assert_eq!(b.bag_rank, Some(255));
}
#[test]
fn soulbound_1283_still_excluded() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(10, 1283, vec![])], vec![]));
    assert!(e.snapshot().drops.is_empty());
}
#[test]
fn ambiguous_kill_loot_stays_unattributed_after_candidates_expire() {
    let mut e = started();
    apply(&mut e, 4100, update(vec![object(4, 300, vec![])], vec![]));
    let maxed_protocol::Packet::Damage(mut p) = damage(1, 2, 10) else {
        unreachable!()
    };
    p.targetId = 4;
    apply(&mut e, 4200, maxed_protocol::Packet::Damage(p));
    apply(&mut e, 5000, tick(vec![status(2, vec![stat(1, 0)])]));
    apply(&mut e, 6000, update(vec![], vec![2]));
    apply(&mut e, 7000, tick(vec![status(4, vec![stat(1, 0)])]));
    apply(
        &mut e,
        8000,
        update(vec![object(10, 1292, vec![])], vec![4]),
    );
    for at in [9000, 10_000, 11_000, 12_000, 13_000] {
        apply(&mut e, at, tick(vec![]));
        assert_eq!(e.snapshot().drops[0].source_enemy, None, "at {at}");
    }
    assert_eq!(
        e.snapshot().drops[0].source_reason.as_deref(),
        Some("ambiguous-nearby-killed-targets")
    );
}

#[test]
fn boss_bag_same_tick_as_kill_gets_killed_target_source() {
    let mut e = started();
    apply(&mut e, 5000, tick(vec![status(2, vec![stat(1, 0)])]));
    apply(
        &mut e,
        6000,
        update(vec![object(10, 1292, vec![])], vec![2]),
    );
    apply(&mut e, 7000, tick(vec![]));
    apply(&mut e, 8000, tick(vec![]));
    let b = e.snapshot().drops.remove(0);
    assert_eq!(b.source_enemy, Some(300));
    assert!(b.source_estimated);
    assert_eq!(b.source_reason.as_deref(), Some("killed-target-same-tick"));
}
#[test]
fn existing_rs_attribution_wins_over_kill_rule() {
    let mut e = started();
    apply(&mut e, 5000, tick(vec![status(2, vec![stat(1, 0)])]));
    apply(
        &mut e,
        6000,
        update(vec![object(10, 1292, vec![])], vec![2]),
    );
    apply(&mut e, 7000, tick(vec![]));
    apply(&mut e, 7500, update(vec![object(4, 301, vec![])], vec![]));
    let maxed_protocol::Packet::Damage(mut p) = damage(1, 2, 10) else {
        unreachable!()
    };
    p.targetId = 4;
    apply(&mut e, 7600, maxed_protocol::Packet::Damage(p));
    apply(&mut e, 7700, update(vec![], vec![4]));
    apply(&mut e, 8000, tick(vec![]));
    assert_eq!(e.snapshot().drops[0].source_enemy, Some(301));
    assert_eq!(
        e.snapshot().drops[0].source_reason.as_deref(),
        Some("nearest-removed-hit-target")
    );
}

#[test]
fn kill_loot_requires_unique_nearby_candidate_in_tick_window() {
    for (tick_delay, distance, second, expected) in [
        (1, 1.5, false, true),
        (2, 0., false, false),
        (0, 1.51, false, false),
        (0, 0., true, false),
    ] {
        let mut e = started();
        if second {
            apply(&mut e, 4100, update(vec![object(4, 300, vec![])], vec![]));
            let maxed_protocol::Packet::Damage(mut p) = damage(1, 2, 10) else {
                unreachable!()
            };
            p.targetId = 4;
            apply(&mut e, 4200, maxed_protocol::Packet::Damage(p));
        }
        apply(
            &mut e,
            5000,
            tick(vec![
                status(2, vec![stat(1, 0)]),
                status(4, vec![stat(1, 0)]),
            ]),
        );
        apply(
            &mut e,
            6000,
            update(vec![], if second { vec![2, 4] } else { vec![2] }),
        );
        for i in 0..tick_delay {
            apply(&mut e, 7000 + i, tick(vec![]));
        }
        let mut bag = object(10, 1292, vec![]);
        bag.status.pos.x = distance;
        apply(&mut e, 8000, update(vec![bag], vec![]));
        apply(&mut e, 9000, tick(vec![]));
        apply(&mut e, 10000, tick(vec![]));
        assert_eq!(e.snapshot().drops[0].source_enemy.is_some(), expected);
    }
}

fn excluded_bag_engine(limit: usize) -> maxed_engine::Engine {
    let mut c = catalog();
    for ty in [1280, 1286] {
        c.bags.push(BagDef {
            object_type: ty,
            name: "Synthetic excluded".into(),
            base_type: ty,
            is_white: false,
            rank: Some(0),
            capture: true,
        });
    }
    let mut cfg = config();
    cfg.max_active_bags = limit;
    let mut e = maxed_engine::Engine::new(cfg, std::sync::Arc::new(c)).unwrap();
    setup(&mut e);
    e
}
#[test]
fn excluded_bags_do_not_consume_dialogue_window() {
    fn run(excluded: Option<u16>) -> Vec<(Option<u16>, Option<String>)> {
        let mut e = excluded_bag_engine(1024);
        let mut dialogue = oryx_killed("unused", "unused");
        if let maxed_protocol::Packet::Text(p) = &mut dialogue {
            p.name = "#Void Entity".into();
            p.text =
                "You fools... You can never truly defeat me! I am in all of you! I AM all of you!"
                    .into();
        }
        apply(&mut e, 3000, dialogue);
        if let Some(ty) = excluded {
            apply(&mut e, 4000, update(vec![object(10, ty, vec![])], vec![]));
        }
        apply(&mut e, 5000, tick(vec![]));
        apply(&mut e, 6000, update(vec![object(11, 1292, vec![])], vec![]));
        apply(&mut e, 7000, tick(vec![]));
        apply(&mut e, 8000, update(vec![object(12, 1292, vec![])], vec![]));
        apply(&mut e, 9000, tick(vec![]));
        apply(&mut e, 10000, tick(vec![]));
        e.snapshot()
            .drops
            .into_iter()
            .filter(|b| b.bag_type == 1292)
            .map(|b| (b.source_enemy, b.source_reason))
            .collect()
    }
    let expected = vec![(Some(45076), Some("dialogue-void".into())); 2];
    assert_eq!(run(None), expected);
    for ty in [1280, 1286] {
        assert_eq!(run(Some(ty)), expected);
    }
}
#[test]
fn excluded_bags_do_not_consume_capacity_or_mark_fights() {
    let mut e = excluded_bag_engine(4);
    apply(&mut e, 3000, tick(vec![]));
    apply(&mut e, 4000, damage(1, 1, 100));
    apply(&mut e, 5000, update(vec![object(10, 1292, vec![])], vec![]));
    let before = e.snapshot();
    assert!(before.fights[0].complete_capture);
    for id in 100..1200 {
        apply(
            &mut e,
            6000,
            update(
                vec![object(id, if id % 2 == 0 { 1280 } else { 1286 }, vec![])],
                vec![],
            ),
        );
    }
    let after = e.snapshot();
    assert_eq!(after.issues, before.issues);
    assert_eq!(after.fights[0].issues, before.fights[0].issues);
    assert!(after.fights[0].complete_capture);
    assert_eq!(
        after.run.unwrap().complete_capture,
        before.run.unwrap().complete_capture
    );
    assert!(after.drops.iter().any(|b| b.id == before.drops[0].id));
    assert_eq!(after.drops.len(), 1);
}
