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
    assert!(events.iter().any(|v|matches!(v,EngineEvent::FightEnded(f) if f.end_reason==Some(FightEnd::ResourceLimit) && !f.complete_capture && f.total_damage==10.)));
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
