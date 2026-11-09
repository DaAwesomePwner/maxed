use crate::common::*;
use maxed_engine::character_id_for;
use maxed_types::*;

#[test]
fn account_scope_accounts_and_recomputation() {
    let mut e = engine();
    setup(&mut e);
    let first = e.snapshot().character.unwrap();
    for number in [1, 0, 65_535, -1, i32::MIN, i32::MAX] {
        apply(&mut e, 3000, create(1, number));
        apply(&mut e, 4000, update(vec![player()], vec![]));
        let c = e.snapshot().character.unwrap();
        assert_eq!(
            character_id_for(&[7; 32], c.account_id.as_ref().unwrap(), number),
            Some(c.id)
        );
    }
    apply(&mut e, 5000, create(1, 1));
    apply(
        &mut e,
        6000,
        update(
            vec![object(1, 100, vec![string(38, "second-account")])],
            vec![],
        ),
    );
    let second = e.snapshot().character.unwrap();
    assert_ne!(first.id, second.id);
    assert_ne!(first.account_id, second.account_id);
    for invalid in ["", "account-00", "account-", "character-0000"] {
        assert_eq!(character_id_for(&[7; 32], invalid, 1), None);
    }
    let account = first.account_id.unwrap();
    assert_eq!(character_id_for(&[7; 32], &account.to_uppercase(), 1), None);
    assert_eq!(character_id_for(&[7; 32], &format!("{account}0"), 1), None);
}

#[test]
fn account_scope_connection_switch_loss_acquisition_and_order() {
    let mut e = engine();
    setup(&mut e);
    let events = connection(&mut e, 2, 3000, create(1, 1));
    let loss = events
        .iter()
        .position(|event| {
            matches!(
                event,
                EngineEvent::LocalIdentityChanged {
                    character_id: None,
                    account_id: None,
                    ..
                }
            )
        })
        .unwrap();
    let materialized = events
        .iter()
        .position(|event| matches!(event, EngineEvent::CharacterUpdated(_)))
        .unwrap();
    let acquired = events
        .iter()
        .position(|event| {
            matches!(
                event,
                EngineEvent::LocalIdentityChanged {
                    character_id: Some(_),
                    account_id: None,
                    ..
                }
            )
        })
        .unwrap();
    assert!(loss < materialized && materialized < acquired);
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::SessionUpdated(s) if s.ended.is_some())));
    let mut next_player = player();
    next_player.status.stats.push(stat(57, 101));
    let events = connection(&mut e, 2, 4000, update(vec![next_player], vec![]));
    let kinds: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            EngineEvent::CharacterUpdated(_) => Some("character"),
            EngineEvent::CharacterMerged { .. } => Some("merge"),
            EngineEvent::FameChanged { .. } => Some("fame"),
            EngineEvent::LocalIdentityChanged {
                account_id: Some(_),
                ..
            } => Some("identity"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["character", "merge", "fame", "identity"]);
    let events = e.finish(Timestamp(5000), FightEnd::CaptureStopped);
    assert!(events.iter().any(|event| matches!(
        event,
        EngineEvent::LocalIdentityChanged {
            character_id: None,
            account_id: None,
            ..
        }
    )));
}

#[test]
fn account_scope_remote_accounts_never_project() {
    let mut remote = engine();
    setup(&mut remote);
    let events = apply(
        &mut remote,
        3000,
        update(
            vec![object(3, 100, vec![string(38, "remote-secret")])],
            vec![],
        ),
    );
    let mut local = engine();
    apply(&mut local, 0, map("Realm"));
    apply(&mut local, 1000, create(3, 1));
    apply(
        &mut local,
        2000,
        update(
            vec![object(3, 100, vec![string(38, "remote-secret")])],
            vec![],
        ),
    );
    let remote_account = local.snapshot().character.unwrap().account_id.unwrap();
    for json in [
        serde_json::to_string(&events).unwrap(),
        serde_json::to_string(&remote.snapshot()).unwrap(),
    ] {
        assert!(!json.contains("remote-secret"));
        assert!(!json.contains(&remote_account));
    }
}

#[test]
fn account_scope_original_owners_survive_switch_and_provisional_merge() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let owner = e.snapshot().character.unwrap().id;
    apply(
        &mut e,
        2000,
        update(vec![object(1, 100, vec![]), target()], vec![]),
    );
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, update(vec![object(4, 1292, vec![])], vec![]));
    let events = apply(
        &mut e,
        5000,
        tick(vec![status(1, vec![string(38, "first-account")])]),
    );
    let verified = e.snapshot().character.unwrap().id;
    assert!(events.iter().any(|event| matches!(event, EngineEvent::CharacterMerged { provisional_id, verified_id } if provisional_id == &owner && verified_id == &verified)));
    // Original owner is intentionally stable: consumers resolve it through the merge alias.
    assert_eq!(
        e.snapshot().fights[0].owner_character_id.as_ref(),
        Some(&owner)
    );
    assert_eq!(
        e.snapshot().drops[0].owner_character_id.as_ref(),
        Some(&owner)
    );
    apply(
        &mut e,
        6000,
        tick(vec![status(1, vec![string(38, "second-account")])]),
    );
    assert_ne!(e.snapshot().character.unwrap().id, verified);
    let mut events = apply(&mut e, 7000, damage(1, 2, 100));
    events.extend(apply(
        &mut e,
        8000,
        tick(vec![status(4, (8..16).map(|n| stat(n, -1)).collect())]),
    ));
    events.extend(e.finish(Timestamp(9000), FightEnd::CaptureStopped));
    let mut fights = 0;
    let mut drops = 0;
    for event in events {
        match event {
            EngineEvent::FightUpdated(f) | EngineEvent::FightEnded(f) => {
                assert_eq!(f.owner_character_id.as_ref(), Some(&owner));
                fights += 1;
            }
            EngineEvent::DropUpdated(d) => {
                assert_eq!(d.owner_character_id.as_ref(), Some(&owner));
                drops += 1;
            }
            _ => {}
        }
    }
    assert!(fights > 0 && drops > 0);
}

#[test]
fn account_scope_legacy_json_and_events_deserialize() {
    let snapshot = maxed_types::demo::demo_snapshot();
    let mut character = serde_json::to_value(snapshot.character.unwrap()).unwrap();
    character.as_object_mut().unwrap().remove("account_id");
    assert!(serde_json::from_value::<CharacterView>(character.clone())
        .unwrap()
        .account_id
        .is_none());
    let event: EngineEvent =
        serde_json::from_value(serde_json::json!({"character_updated": character})).unwrap();
    assert!(matches!(event, EngineEvent::CharacterUpdated(_)));
    let mut fight = serde_json::to_value(&snapshot.fights[0]).unwrap();
    fight.as_object_mut().unwrap().remove("owner_character_id");
    assert!(serde_json::from_value::<FightView>(fight)
        .unwrap()
        .owner_character_id
        .is_none());
    let mut drop = serde_json::to_value(&snapshot.drops[0]).unwrap();
    drop.as_object_mut().unwrap().remove("owner_character_id");
    assert!(serde_json::from_value::<DropView>(drop)
        .unwrap()
        .owner_character_id
        .is_none());
}
