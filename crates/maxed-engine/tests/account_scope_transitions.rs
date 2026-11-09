use crate::common::*;
use maxed_types::*;

fn contexts(events: &[EngineEvent]) -> Vec<(Option<String>, Option<String>)> {
    events
        .iter()
        .filter_map(|event| match event {
            EngineEvent::LocalIdentityChanged {
                character_id,
                account_id,
                ..
            } => Some((character_id.clone(), account_id.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn account_scope_same_entity_account_replacement_emits_context() {
    let mut e = engine();
    setup(&mut e);
    let original = e.snapshot().character.unwrap();
    let events = apply(
        &mut e,
        3000,
        tick(vec![status(1, vec![string(38, "replacement-account")])]),
    );
    let next = e.snapshot().character.unwrap();
    assert_ne!(next.id, original.id);
    assert_ne!(next.account_id, original.account_id);
    assert_eq!(contexts(&events), [(Some(next.id), next.account_id)]);
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::CharacterMerged { .. })));
}

#[test]
fn account_scope_map_change_loses_then_acquires_provisional_and_verified() {
    let mut e = engine();
    setup(&mut e);
    let original = e.snapshot().character.unwrap();
    let events = apply(&mut e, 3000, map("Realm"));
    assert_eq!(contexts(&events), [(None, None)]);
    assert!(e.snapshot().character.is_none());
    let events = apply(&mut e, 4000, create(1, 1));
    let provisional = e.snapshot().character.unwrap();
    assert_eq!(contexts(&events), [(Some(provisional.id), None)]);
    let events = apply(&mut e, 5000, update(vec![player()], vec![]));
    assert_eq!(
        contexts(&events),
        [(Some(original.id), original.account_id)]
    );
}

#[test]
fn account_scope_idle_expiry_emits_loss_once() {
    let mut e = engine();
    setup(&mut e);
    assert!(contexts(&e.advance_to(Timestamp(1_800_001_999))).is_empty());
    let events = e.advance_to(Timestamp(1_800_002_000));
    assert_eq!(contexts(&events), [(None, None)]);
    assert!(e.snapshot().character.is_none());
    assert!(e.snapshot().session.unwrap().ended.is_some());
    assert!(contexts(&e.advance_to(Timestamp(1_800_003_000))).is_empty());
}

#[test]
fn account_scope_fame_and_stat_changes_do_not_emit_identity() {
    let mut e = engine();
    setup(&mut e);
    let events = apply(&mut e, 3000, tick(vec![status(1, vec![stat(57, 101)])]));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::FameChanged { fame: 101, .. })));
    assert!(contexts(&events).is_empty());
    let events = apply(&mut e, 4000, tick(vec![status(1, vec![stat(20, 26)])]));
    assert!(events.iter().any(|event| matches!(event, EngineEvent::CharacterUpdated(c) if c.stats.iter().any(|s| s.stat_id == 20 && s.value == 26))));
    assert!(contexts(&events).is_empty());
}
