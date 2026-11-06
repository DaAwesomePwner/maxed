use crate::{Engine, EngineConfig};
use maxed_protocol::{defs::AllyShoot, Packet};
use maxed_types::*;
use std::sync::Arc;

fn engine() -> Engine {
    Engine::new(
        EngineConfig::default(),
        Arc::new(Catalog {
            format_version: 1,
            client_fingerprint: "synthetic-maintenance".into(),
            items: vec![],
            objects: vec![],
            classes: vec![],
            enchants: vec![],
            dungeons: vec![],
            bags: vec![],
            sprites: vec![],
            tiles: vec![],
            crucible_bonuses: vec![],
        }),
    )
    .unwrap()
}
fn shot(e: &mut Engine, owner: i32, bullet: u16) {
    e.apply_combat(
        &Packet::AllyShoot(AllyShoot {
            bulletId: bullet,
            ownerId: owner,
            containerType: 200,
            angle: 0.,
            inspiredBuff: false,
        }),
        &mut vec![],
    );
}

#[test]
fn replacement_and_expiration_keep_exact_bounded_shot_index() {
    let mut e = engine();
    for at in 0..10_000 {
        e.now = Timestamp(at);
        shot(&mut e, 1, 1);
    }
    assert_eq!(e.shots.len(), 1);
    assert_eq!(e.shot_expiry.len(), 1);
    e.advance_to(Timestamp(5_009_999));
    assert_eq!(e.shots.len(), 1);
    e.advance_to(Timestamp(5_010_000));
    assert!(e.shots.is_empty());
    assert!(e.shot_expiry.is_empty());
    e.config.max_shots = 2;
    shot(&mut e, 1, 1);
    shot(&mut e, 2, 1);
    shot(&mut e, 3, 1);
    assert!(!e.shots.contains_key(&(1, 1)));
    assert_eq!(e.shot_expiry.len(), 2);
    e.remove_owner_shots(2);
    assert_eq!(e.shots.len(), 1);
    assert_eq!(e.shot_expiry.len(), 1);
    assert!(e.shots.contains_key(&(3, 1)));
}

#[test]
fn removed_shot_diagnostics_are_bounded_replaced_and_cleared_with_the_world() {
    let mut e = engine();
    e.config.max_shots = 2;
    for owner in 0..1000 {
        shot(&mut e, owner, 1);
        assert!(e.removed_shots.len() <= 2);
        assert_eq!(e.removed_shot_order.len(), e.removed_shots.len());
    }
    let (&key, _) = e.removed_shots.last_key_value().unwrap();
    shot(&mut e, key.0, key.1);
    assert!(!e.removed_shots.contains_key(&key));
    assert_eq!(e.removed_shot_order.len(), e.removed_shots.len());
    e.clear_world();
    assert!(e.removed_shots.is_empty());
    assert!(e.removed_shot_order.is_empty());
}

#[test]
fn removed_shot_diagnostics_evict_oldest_insertion_not_lowest_owner() {
    let mut e = engine();
    e.config.max_shots = 2;
    // All removals share a timestamp; insertion order must break the tie.
    for owner in [30, 10, 20] {
        shot(&mut e, owner, 1);
        e.remove_owner_shots(owner);
    }
    assert!(!e.removed_shots.contains_key(&(30, 1)));
    assert!(e.removed_shots.contains_key(&(10, 1)));
    assert!(e.removed_shots.contains_key(&(20, 1)));
    // Reusing a bullet removes its old index entry and starts a new lifetime.
    shot(&mut e, 10, 1);
    assert_eq!(e.removed_shot_order.len(), 1);
    e.remove_owner_shots(10);
    shot(&mut e, 30, 1);
    e.remove_owner_shots(30);
    assert!(!e.removed_shots.contains_key(&(20, 1)));
    assert!(e.removed_shots.contains_key(&(10, 1)));
    assert!(e.removed_shots.contains_key(&(30, 1)));
    assert_eq!(e.removed_shot_order.len(), 2);
    for (&key, &(sequence, reason)) in &e.removed_shots {
        assert!(e.removed_shot_order.contains(&(sequence, key)));
        assert_eq!(reason, crate::ReconstructionReason::OwnerLeftView);
    }
}

#[test]
fn pending_expiry_settles_once_and_zero_reports_use_no_pair_capacity() {
    let mut e = engine();
    let mut events = vec![];
    e.report(2, 1, 1, false, Ok(100), &mut events);
    e.now = Timestamp(4_000_000);
    e.report(2, 1, 2, false, Ok(200), &mut events);
    let key = *e.fights.keys().next().unwrap();
    assert_eq!(e.pair_expiry.len(), 2);
    e.advance_to(Timestamp(5_000_000));
    assert_eq!(e.pair_expiry.len(), 2);
    e.advance_to(Timestamp(5_000_001));
    assert_eq!(e.pair_expiry.len(), 1);
    assert_eq!(e.fights[&key].base.total_damage, 100.);
    assert_eq!(e.snapshot().fights[0].total_damage, 300.);
    e.report(2, 1, 3, false, Ok(0), &mut events);
    assert_eq!(e.pair_expiry.len(), 1);
    e.advance_to(Timestamp(9_000_001));
    assert!(e.pair_expiry.is_empty());
    assert_eq!(e.fights[&key].base.total_damage, 300.);
    e.report(2, 1, 4, false, Ok(50), &mut events);
    e.retire_fight(key, FightEnd::AppClosed, &mut events);
    assert!(e.pair_expiry.is_empty());
    assert!(e.dirty_fights.is_empty());
    assert!(events
        .iter()
        .any(|v| matches!(v,EngineEvent::FightEnded(f) if f.total_damage==350.)));
}

#[test]
fn grace_period_prunes_only_unreferenced_entities_and_preserves_reentry_generation() {
    let mut e = engine();
    let mut events = vec![];
    for id in 1..=3 {
        e.ensure_entity(id, &mut events);
    }
    let generation = e.entities[&1].generation;
    e.protect_entity(2); // A fight or bag references this identity.
    e.remove_entity(1, &mut events);
    e.remove_entity(2, &mut events);
    e.advance_to(Timestamp(29_999_999));
    assert!(e.entities.contains_key(&1));
    e.advance_to(Timestamp(30_000_000));
    assert!(!e.entities.contains_key(&1));
    assert!(e.entities.contains_key(&2));
    assert!(e.entities.contains_key(&3));
    assert!(e.inactive_entities.is_empty());
    e.ensure_entity(1, &mut events);
    assert_eq!(e.entities[&1].generation, generation);
}

#[test]
fn reverse_summon_index_cleans_only_related_mappings() {
    let mut e = engine();
    let mut events = vec![];
    for id in 1..=5 {
        e.ensure_entity(id, &mut events);
    }
    e.set_summoner(2, 1);
    e.set_summoner(3, 1);
    e.set_summoner(5, 4);
    e.remove_entity(2, &mut events);
    assert_eq!(e.summons[&1].len(), 1);
    e.remove_entity(1, &mut events);
    assert!(e.entities[&3].summoner.is_none());
    assert!(e.entities[&5].summoner.is_some());
    assert_eq!(e.summons.len(), 1);
    e.set_summoner(5, 3);
    assert!(!e.summons.contains_key(&4));
    assert!(e.summons.contains_key(&3));
    e.clear_world();
    assert!(e.summons.is_empty());
    assert!(e.inactive_entities.is_empty());
}

#[test]
fn type_replacement_and_boundaries_cannot_collide_with_existing_generations() {
    use maxed_protocol::defs::{ObjectStatusData, WorldPosData};
    let mut e = engine();
    let mut events = vec![];
    let status = |id| ObjectStatusData {
        objectId: id,
        pos: WorldPosData { x: 0., y: 0. },
        stats: vec![],
    };
    e.merge_entity(status(1), Some(300), &mut events);
    e.ensure_entity(2, &mut events);
    let initial = e.entities[&2].generation;
    e.remove_entity(2, &mut events);
    e.merge_entity(status(1), Some(301), &mut events);
    let replacement = e.entities[&1].generation;
    assert_ne!(initial, replacement);
    e.advance_to(Timestamp(30_000_000));
    e.ensure_entity(2, &mut events);
    assert_eq!(e.entities[&2].generation, initial);
    e.clear_entities();
    e.ensure_entity(1, &mut events);
    assert_ne!(e.entities[&1].generation, replacement);
}

#[test]
fn retired_targets_release_capacity_and_keep_bounded_tombstones() {
    let mut e = engine();
    e.config.max_entities = 2;
    e.config.max_active_fights = 1;
    let mut events = vec![];
    for id in 1..=100 {
        e.report(id, 0, 1, false, Ok(10), &mut events);
        assert!(e.entities.contains_key(&id), "target {id} was rejected");
        assert_eq!(e.fights.len(), 1);
        let key = *e.fights.keys().next().unwrap();
        assert_eq!(key.0, id);
        e.remove_entity(id, &mut events);
        assert!(e.entities.len() <= 2);
        assert!(e.retired_entities.len() <= 2);
        assert!(e.retired_targets.len() <= 2);
        assert_eq!(e.referenced_entities.len(), 1);
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event,
        EngineEvent::FightEnded(f) if f.end_reason == Some(FightEnd::ResourceLimit)))
            .count(),
        99
    );
    // All inactive, unreferenced entities now represent retired targets.
    assert!(e
        .inactive_entities
        .iter()
        .all(|(_, id)| e.entities[id].fight_retired));
    e.advance_to(Timestamp(30_000_000));
    assert!(!e.entities.contains_key(&99));
    events.clear();
    e.report(99, 0, 2, false, Ok(20), &mut events);
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::FightStarted(_))));
    assert_eq!(e.fights.keys().next().unwrap().0, 100);
    let key = *e.fights.keys().next().unwrap();
    e.retire_fight(key, FightEnd::ResourceLimit, &mut events);
    e.remove_entity(99, &mut events);
    assert_eq!(e.entities.len(), e.config.max_entities);
    assert!(e
        .entities
        .values()
        .all(|entity| !entity.present && entity.fight_retired));
    assert!(e.ensure_entity(101, &mut events));
}

#[test]
fn all_fight_retirements_release_nonplayer_targets() {
    for reason in [
        FightEnd::AppClosed,
        FightEnd::MapChanged,
        FightEnd::SessionIdle,
        FightEnd::ResourceLimit,
    ] {
        let mut e = engine();
        let mut events = vec![];
        e.report(2, 1, 1, false, Ok(100), &mut events);
        let key = *e.fights.keys().next().unwrap();
        e.remove_entity(2, &mut events);
        e.retire_fight(key, reason, &mut events);
        assert!(!e.referenced_entities.contains(&2));
        assert_eq!(e.inactive_entities.len(), 1);
        e.advance_to(Timestamp(30_000_000));
        assert!(!e.entities.contains_key(&2));
        events.clear();
        e.report(2, 1, 2, false, Ok(100), &mut events);
        assert!(e.fights.is_empty());
    }
}

#[test]
fn retired_bags_release_capacity_and_flush_releases_protection() {
    use maxed_protocol::defs::{ObjectStatusData, WorldPosData};
    let mut e = engine();
    e.config.max_entities = 2;
    e.config.max_active_bags = 1;
    let mut events = vec![];
    let status = |id| ObjectStatusData {
        objectId: id,
        pos: WorldPosData { x: 0., y: 0. },
        stats: vec![],
    };
    for id in 1..=20 {
        e.merge_entity(status(id), Some(1292), &mut events);
        assert!(e.entities.contains_key(&id));
        assert_eq!(e.drops.len(), 1);
        e.remove_entity(id, &mut events);
        assert_eq!(e.referenced_entities.len(), 1);
        assert!(e.retired_entities.len() <= 2);
    }
    e.flush_bags(&mut events);
    assert!(e.referenced_entities.is_empty());
    assert!(e.drops.is_empty());
    e.advance_to(Timestamp(30_000_000));
    assert!(e.entities.is_empty());
    events.clear();
    for id in [19, 20] {
        e.merge_entity(status(id), Some(1292), &mut events);
    }
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::BagDropped(_))));
    // Changed type is still evidence of a new generation, including after prune.
    e.remove_entity(19, &mut events);
    e.advance_to(Timestamp(60_000_000));
    e.merge_entity(status(19), Some(1296), &mut events);
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::BagDropped(b) if b.bag_type == 1296)));
}

#[test]
fn full_tombstone_table_preserves_returning_bag_during_entity_eviction() {
    use maxed_protocol::defs::{ObjectStatusData, WorldPosData};
    let mut e = engine();
    e.config.max_entities = 2;
    e.config.max_active_bags = 1;
    let mut events = vec![];
    let status = |id| ObjectStatusData {
        objectId: id,
        pos: WorldPosData { x: 0., y: 0. },
        stats: vec![],
    };
    for id in 1..=4 {
        e.merge_entity(status(id), Some(1292), &mut events);
        e.remove_entity(id, &mut events);
    }
    assert_eq!(e.entities.len(), 2);
    assert_eq!(e.retired_entities.len(), 2);
    assert!(e.retired_entities.contains_key(&1));
    events.clear();
    e.merge_entity(status(1), Some(1292), &mut events);
    assert!(e.entities[&1].bag_retired);
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::BagDropped(_))));
}
