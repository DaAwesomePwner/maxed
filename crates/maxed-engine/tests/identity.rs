use crate::common;
use common::*;
use maxed_types::*;
#[test]
fn verification_merges_without_exposing_account() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let provisional = e.snapshot().character.unwrap();
    assert!(!provisional.identity_verified);
    let events = apply(&mut e, 2000, update(vec![player()], vec![]));
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::CharacterMerged { .. })));
    assert!(e.snapshot().character.unwrap().identity_verified);
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("synthetic-account"));
}
#[test]
fn account_switch_and_pre_map_create() {
    let mut e = engine();
    apply(&mut e, 0, create(1, 1));
    apply(&mut e, 1000, map("The Void"));
    apply(&mut e, 2000, update(vec![player()], vec![]));
    let first = e.snapshot().character.unwrap().id;
    connection(&mut e, 2, 3000, create(1, 1));
    connection(&mut e, 2, 4000, map("Nexus"));
    connection(
        &mut e,
        2,
        5000,
        update(
            vec![object(1, 100, vec![string(38, "different-account")])],
            vec![],
        ),
    );
    assert_ne!(first, e.snapshot().character.unwrap().id);
}
#[test]
fn new_connection_create_cannot_reuse_previous_accounts_stats() {
    let mut e = engine();
    setup(&mut e);
    let events = connection(&mut e, 2, 3000, create(1, 2));
    assert!(events
        .iter()
        .all(|v| !matches!(v,EngineEvent::CharacterUpdated(c) if c.identity_verified)));
    assert!(events
        .iter()
        .all(|v| !matches!(v, EngineEvent::RunUpdated(_))));
}
#[test]
fn provisional_ids_are_unique_without_spawns() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let first = e.snapshot().character.unwrap().id;
    apply(&mut e, 2000, create(1, 2));
    assert_ne!(first, e.snapshot().character.unwrap().id);
}
#[test]
fn create_and_local_spawn_before_map_are_buffered_together() {
    let mut e = engine();
    apply(&mut e, 0, create(1, 1));
    apply(&mut e, 1000, update(vec![player()], vec![]));
    let id = e.snapshot().character.unwrap().id;
    apply(&mut e, 2000, map("The Void"));
    let c = e.snapshot().character.unwrap();
    assert_eq!(c.id, id);
    assert!(c.identity_verified);
}
