use crate::common;
use common::*;
use maxed_engine::*;
use maxed_types::*;
use std::sync::Arc;
#[test]
fn fight_eviction_flushes_partial_once_and_tombstone_blocks_reopen() {
    let mut c = config();
    c.max_active_fights = 1;
    let mut e = Engine::new(c, Arc::new(catalog())).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 10));
    apply(&mut e, 4000, update(vec![object(3, 301, vec![])], vec![]));
    let maxed_protocol::Packet::Damage(mut d) = damage(1, 2, 20) else {
        unreachable!()
    };
    d.targetId = 3;
    let events = apply(&mut e, 5000, maxed_protocol::Packet::Damage(d));
    assert!(events.iter().any(|v|matches!(v,EngineEvent::FightEnded(f) if f.end_reason==Some(FightEnd::ResourceLimit) && !f.complete_capture && f.issues.contains(&CoverageIssue::ResourceLimit) && f.total_damage==10.)));
    assert!(apply(&mut e, 6000, damage(1, 3, 30))
        .iter()
        .all(|v| !matches!(v, EngineEvent::FightStarted(_))));
}
#[test]
fn bucket_merges_preserve_exact_totals_and_bounded_size() {
    let mut e = engine();
    setup(&mut e);
    for second in 1..=3601 {
        let at = second * 1_000_000;
        apply(&mut e, at, tick(vec![]));
        apply(&mut e, at + 1000, damage(1, second as u16, 1));
    }
    let f = &e.snapshot().fights[0];
    assert_eq!(f.total_damage, 3601.);
    assert_eq!(f.buckets.iter().map(|b| b.total).sum::<f64>(), 3601.);
    assert!(f.buckets.len() <= 1800);
}
#[test]
fn shot_cap_marks_coverage_and_expired_source_does_not_leak() {
    let mut c = config();
    c.max_shots = 1;
    let mut e = Engine::new(c, Arc::new(catalog())).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, shoot(1));
    apply(&mut e, 4000, shoot(2));
    assert!(e.snapshot().issues.contains(&CoverageIssue::ResourceLimit));
    apply(&mut e, 5000, hit(1));
    assert_eq!(e.snapshot().fights[0].total_damage, 0.);
}
#[test]
fn hostile_catalog_and_config_are_rejected() {
    let mut c = catalog();
    c.items[0].projectiles[0].min_damage = -1;
    assert!(matches!(
        Engine::new(config(), Arc::new(c)),
        Err(EngineError::InvalidCatalog)
    ));
    let mut c = catalog();
    c.items[1].scaling.as_mut().unwrap().damage_per_stat = f32::NAN;
    assert!(Engine::new(config(), Arc::new(c)).is_err());
    let mut c = config();
    c.max_entities = 0;
    assert!(matches!(
        Engine::new(c, Arc::new(catalog())),
        Err(EngineError::InvalidConfig)
    ));
}
#[test]
fn three_hundred_targets_have_summaries_without_unbounded_hit_history() {
    let mut e = engine();
    setup(&mut e);
    for id in 2..302 {
        let at = id as u64 * 10_000_000;
        apply(&mut e, at, update(vec![object(id, 300, vec![])], vec![]));
        for bullet in 0..3 {
            let maxed_protocol::Packet::Damage(mut d) = damage(1, bullet, 100) else {
                unreachable!()
            };
            d.targetId = id;
            apply(
                &mut e,
                at + u64::from(bullet) * 1000,
                maxed_protocol::Packet::Damage(d),
            );
        }
    }
    let s = e.snapshot();
    assert_eq!(s.fights.len(), 300);
    assert!(s.fights.iter().all(|f| f.total_damage == 300.));
    assert!(serde_json::to_vec(&s).unwrap().len() < 1_000_000);
}

#[test]
fn settled_kills_leave_room_without_tainting_open_fights_or_run() {
    use maxed_engine::alerts::personal_best_candidate;
    let mut e = engine();
    setup(&mut e);
    // Keep an older open fight: eviction must prefer settled kills, not age alone.
    let target_damage = |id, bullet| {
        let maxed_protocol::Packet::Damage(mut d) = damage(1, bullet, 100) else {
            unreachable!()
        };
        d.targetId = id;
        maxed_protocol::Packet::Damage(d)
    };
    apply(&mut e, 3000, update(vec![object(3, 301, vec![])], vec![]));
    apply(&mut e, 4000, tick(vec![]));
    apply(&mut e, 4000, target_damage(3, 0));
    let mut ended = vec![];
    for id in 4..2054 {
        let at = (id as u64 - 3) * 12_000_000;
        apply(&mut e, at, update(vec![object(id, 300, vec![])], vec![]));
        apply(&mut e, at, tick(vec![]));
        ended.extend(apply(&mut e, at, target_damage(id, 1)));
        apply(&mut e, at + 4_000_000, tick(vec![]));
        apply(&mut e, at + 4_000_000, target_damage(id, 2));
        apply(
            &mut e,
            at + 4_001_000,
            tick(vec![status(id, vec![stat(1, 0)])]),
        );
        apply(&mut e, at + 4_002_000, update(vec![], vec![id]));
    }
    let snapshot = e.snapshot();
    assert_eq!(snapshot.fights.len(), 2048);
    assert!(snapshot.issues.is_empty(), "{:?}", snapshot.issues);
    assert!(snapshot.run.as_ref().unwrap().complete_capture);
    for f in &snapshot.fights {
        assert!(
            f.complete_capture && f.issues.is_empty(),
            "{}: {:?}",
            f.target_id,
            f.issues
        );
    }
    assert!(snapshot.fights.iter().any(|f| f.target_id == 3));
    let retired: Vec<_> = ended
        .iter()
        .filter_map(|event| match event {
            EngineEvent::FightEnded(f) => Some(f),
            _ => None,
        })
        .collect();
    assert_eq!(retired.len(), 3);
    for f in retired {
        assert_eq!(f.end_reason, Some(FightEnd::Killed));
        assert!(f.complete_capture && f.issues.is_empty());
        assert!(personal_best_candidate(f, snapshot.character.as_ref().unwrap()).is_some());
    }
}

#[test]
fn cap_preserves_kills_until_pairs_and_settlement_window_expire() {
    for (at, can_retire) in [(6_000_000, false), (10_002_000, false), (10_003_000, true)] {
        let mut c = config();
        c.max_active_fights = 1;
        let mut e = Engine::new(c, Arc::new(catalog())).unwrap();
        setup(&mut e);
        apply(&mut e, 1_000_000, tick(vec![]));
        apply(&mut e, 1_000_000, damage(1, 1, 100));
        apply(&mut e, 5_000_000, tick(vec![]));
        apply(&mut e, 5_000_000, shoot(2));
        apply(&mut e, 5_000_000, hit(2));
        apply(&mut e, 5_001_000, tick(vec![status(2, vec![stat(1, 0)])]));
        apply(&mut e, 5_002_000, update(vec![], vec![2]));
        assert!(e.snapshot().fights[0].pending_local_reconciliation);
        apply(&mut e, at, update(vec![object(3, 301, vec![])], vec![]));
        apply(&mut e, at, tick(vec![]));
        let maxed_protocol::Packet::Damage(mut d) = damage(1, 3, 20) else {
            unreachable!()
        };
        d.targetId = 3;
        let events = apply(&mut e, at, maxed_protocol::Packet::Damage(d));
        assert_eq!(
            events
                .iter()
                .any(|v| matches!(v, EngineEvent::FightEnded(_))),
            can_retire
        );
        let snapshot = e.snapshot();
        assert_eq!(snapshot.fights.len(), 1);
        assert_eq!(snapshot.fights[0].target_id, if can_retire { 3 } else { 2 });
        if !can_retire {
            assert!(snapshot.fights[0].complete_capture);
            assert!(snapshot.fights[0].issues.is_empty());
        }
    }
}

#[test]
fn provisional_kill_is_not_a_safe_eviction() {
    let mut c = config();
    c.max_active_fights = 1;
    let mut e = Engine::new(c, Arc::new(catalog())).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 10));
    apply(&mut e, 4000, victory());
    assert!(e.snapshot().fights[0].kill.as_ref().unwrap().provisional);
    apply(
        &mut e,
        10_000_000,
        update(vec![object(3, 301, vec![])], vec![]),
    );
    let maxed_protocol::Packet::Damage(mut d) = damage(1, 2, 20) else {
        unreachable!()
    };
    d.targetId = 3;
    let events = apply(&mut e, 10_000_000, maxed_protocol::Packet::Damage(d));
    assert!(events.iter().any(|v| matches!(v, EngineEvent::FightEnded(f)
        if f.end_reason == Some(FightEnd::ResourceLimit)
            && f.issues.contains(&CoverageIssue::ResourceLimit)
            && !f.complete_capture)));
}
