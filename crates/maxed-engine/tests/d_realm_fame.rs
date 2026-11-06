use crate::common;
use common::*;
use maxed_engine::Engine;
use maxed_types::*;

fn delta(e: &Engine) -> Option<i32> {
    e.snapshot().instance.and_then(|i| i.visit_fame_delta)
}
fn enter(e: &mut Engine, at: u64, name: &str, fame: Option<i32>) {
    apply(e, at, map(name));
    apply(e, at + 1000, create(1, 1));
    let mut p = player();
    p.status.stats.retain(|s| s.statTypeNum != 57);
    if let Some(value) = fame {
        p.status.stats.push(stat(57, value));
    }
    apply(e, at + 2000, update(vec![p], vec![]));
}

#[test]
fn visit_fame_uses_entry_observation_and_survives_snapshot_reads() {
    let mut e = engine();
    apply(&mut e, 0, map("Realm"));
    assert_eq!(delta(&e), None);
    apply(&mut e, 1000, create(1, 1));
    assert_eq!(delta(&e), None);
    apply(&mut e, 2000, update(vec![player()], vec![]));
    assert_eq!(delta(&e), Some(0)); // player() supplies fame 100
    apply(&mut e, 3000, tick(vec![status(1, vec![stat(57, 137)])]));
    assert_eq!(delta(&e), Some(37));
    assert_eq!(delta(&e), Some(37));
    apply(&mut e, 4000, tick(vec![status(1, vec![stat(20, 70)])]));
    assert_eq!(delta(&e), Some(37));
}

#[test]
fn missing_entry_fame_is_not_invented_from_a_later_sample() {
    let mut e = engine();
    enter(&mut e, 0, "Realm", None);
    assert_eq!(delta(&e), None);
    apply(&mut e, 3000, tick(vec![status(1, vec![stat(57, 137)])]));
    assert_eq!(delta(&e), None);
}

#[test]
fn dungeon_hops_and_repeated_realm_names_start_new_baselines() {
    let mut e = engine();
    enter(&mut e, 0, "Realm", Some(100));
    apply(&mut e, 3000, tick(vec![status(1, vec![stat(57, 125)])]));
    assert_eq!(delta(&e), Some(25));
    enter(&mut e, 4000, "The Void", Some(125));
    apply(&mut e, 7000, tick(vec![status(1, vec![stat(57, 200)])]));
    assert_eq!(delta(&e), None);
    enter(&mut e, 8000, "Realm", Some(200));
    assert_eq!(delta(&e), Some(0));
    apply(&mut e, 11000, tick(vec![status(1, vec![stat(57, 207)])]));
    assert_eq!(delta(&e), Some(7));
    enter(&mut e, 12000, "Realm", Some(207));
    assert_eq!(delta(&e), Some(0));
}

#[test]
fn identity_upgrade_keeps_entry_but_a_new_local_incarnation_cannot_claim_it() {
    let mut e = engine();
    apply(&mut e, 0, map("Realm"));
    apply(&mut e, 1000, create(1, 1));
    apply(
        &mut e,
        2000,
        update(vec![object(1, 100, vec![stat(57, 100)])], vec![]),
    );
    assert!(!e.snapshot().character.unwrap().identity_verified);
    assert_eq!(delta(&e), Some(0));
    apply(&mut e, 3000, update(vec![player()], vec![]));
    assert!(e.snapshot().character.unwrap().identity_verified);
    apply(&mut e, 4000, tick(vec![status(1, vec![stat(57, 120)])]));
    assert_eq!(delta(&e), Some(20));
    apply(&mut e, 5000, create(1, 2));
    apply(&mut e, 6000, update(vec![player()], vec![]));
    assert_eq!(delta(&e), None);
}

#[test]
fn fame_regression_is_unknown_instead_of_a_negative_gain() {
    let mut e = engine();
    enter(&mut e, 0, "Realm", Some(100));
    apply(&mut e, 3000, tick(vec![status(1, vec![stat(57, 90)])]));
    assert_eq!(delta(&e), None);
    apply(&mut e, 4000, tick(vec![status(1, vec![stat(57, 130)])]));
    assert_eq!(delta(&e), Some(30));
}

#[test]
fn legacy_instance_without_visit_fame_deserializes_as_unknown() {
    let mut e = engine();
    enter(&mut e, 0, "Realm", Some(100));
    let instance = e.snapshot().instance.unwrap();
    let mut json = serde_json::to_value(instance).unwrap();
    json.as_object_mut().unwrap().remove("visit_fame_delta");
    let legacy: InstanceLive = serde_json::from_value(json).unwrap();
    assert_eq!(legacy.visit_fame_delta, None);
}
