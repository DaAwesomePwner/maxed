use crate::common;
use common::*;
use maxed_protocol::{defs::*, Packet};
use maxed_types::*;

fn inferred_source_engine() -> maxed_engine::Engine {
    let mut catalog = catalog();
    let mut second_weapon = catalog.items[0].clone();
    second_weapon.object_type = 202;
    second_weapon.name = "Synthetic second sword".into();
    catalog.items.push(second_weapon);
    let mut projectile_less = catalog.items[1].clone();
    projectile_less.object_type = 203;
    projectile_less.name = "Synthetic projectile-less item".into();
    projectile_less.projectiles.clear();
    catalog.items.push(projectile_less);
    maxed_engine::Engine::new(config(), std::sync::Arc::new(catalog)).unwrap()
}

#[test]
fn remote_shotless_damage_credits_equipped_weapon() {
    let mut e = inferred_source_engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            vec![object(
                3,
                100,
                vec![stat(8, 200), string(80, "AAIEewA=,,,")],
            )],
            vec![],
        ),
    );
    apply(&mut e, 4000, damage(3, 7, 100));
    let snapshot = e.snapshot();
    let player = snapshot.fights[0]
        .players
        .iter()
        .find(|p| p.object_id == 3)
        .unwrap();
    assert_eq!(player.sources.len(), 1);
    let source = &player.sources[0];
    assert_eq!(source.kind, SourceKind::Weapon);
    assert_eq!(source.item_type, Some(200));
    assert!(source.estimated);
    assert_eq!(source.reasons, vec!["inferred-equipped-weapon"]);
    assert_eq!(
        source.enchant,
        maxed_engine::parse_enchant(Some("AAIEewA="))
    );
    assert_eq!(source.damage, 100.);
}

#[test]
fn remote_shotless_damage_without_weapon_stays_other() {
    // Item 201 has projectiles; 203 is the projectile-less catalog item.
    for stats in [
        vec![],
        vec![stat(8, 203)],
        vec![stat(8, 999)],
        vec![stat(8, -1)],
    ] {
        let mut e = inferred_source_engine();
        setup(&mut e);
        apply(&mut e, 3000, update(vec![object(3, 100, stats)], vec![]));
        apply(&mut e, 4000, damage(3, 7, 100));
        let snapshot = e.snapshot();
        let source = &snapshot.fights[0]
            .players
            .iter()
            .find(|p| p.object_id == 3)
            .unwrap()
            .sources[0];
        assert_eq!(source.kind, SourceKind::Other);
        assert_eq!(source.item_type, None);
        assert!(!source.estimated);
        assert!(source.reasons.is_empty());
    }
}

#[test]
fn local_and_summon_sources_are_not_inferred() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 7, 100));
    let snapshot = e.snapshot();
    let source = &snapshot.fights[0].players[0].sources[0];
    assert_eq!(source.kind, SourceKind::Other);
    assert_eq!(source.item_type, None);
    assert!(!source.estimated);
    assert!(source.reasons.is_empty());

    apply(
        &mut e,
        4000,
        update(
            vec![object(3, 100, vec![stat(8, 200)]), object(4, 301, vec![])],
            vec![],
        ),
    );
    apply(
        &mut e,
        5000,
        Packet::ServerPlayerShoot(ServerPlayerShoot {
            bulletId: 1,
            ownerId: 4,
            containerType: 301,
            startingPos: pos(),
            angle: 0.,
            damage: 100,
            summonerId: 3,
            bulletType: 0,
            bulletCount: 1,
            anglesBetweenBullets: 0.,
        }),
    );
    // A different bullet has no known shot, but retains explicit summon ownership.
    apply(&mut e, 6000, damage(4, 7, 100));
    let snapshot = e.snapshot();
    let source = &snapshot.fights[0]
        .players
        .iter()
        .find(|p| p.object_id == 3)
        .unwrap()
        .sources[0];
    assert_eq!(source.kind, SourceKind::Summon);
    assert_eq!(source.item_type, Some(301));
    assert!(!source.estimated);
    assert!(source.reasons.is_empty());
}

#[test]
fn weapon_swap_mid_fight_splits_inferred_damage() {
    let mut e = inferred_source_engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(vec![object(3, 100, vec![stat(8, 200)])], vec![]),
    );
    apply(&mut e, 4000, damage(3, 7, 100));
    apply(&mut e, 5000, tick(vec![status(3, vec![stat(8, 202)])]));
    apply(&mut e, 6000, damage(3, 8, 50));
    let snapshot = e.snapshot();
    let sources = &snapshot.fights[0]
        .players
        .iter()
        .find(|p| p.object_id == 3)
        .unwrap()
        .sources;
    assert_eq!(sources.len(), 2);
    for (item, damage) in [(200, 100.), (202, 50.)] {
        let source = sources.iter().find(|s| s.item_type == Some(item)).unwrap();
        assert_eq!(source.kind, SourceKind::Weapon);
        assert!(source.estimated);
        assert_eq!(source.damage, damage);
        assert_eq!(source.hits, 1);
        assert_eq!(source.reasons, vec!["inferred-equipped-weapon"]);
    }
}

#[test]
fn explicit_summoner_without_summon_metadata_is_attributed_with_unknown_item() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        Packet::ServerPlayerShoot(ServerPlayerShoot {
            bulletId: 1,
            ownerId: 3,
            containerType: 201,
            startingPos: pos(),
            angle: 0.,
            damage: 100,
            summonerId: 1,
            bulletType: 0,
            bulletCount: 1,
            anglesBetweenBullets: 0.,
        }),
    );
    apply(&mut e, 4000, damage(3, 1, 100));
    let snapshot = e.snapshot();
    let f = &snapshot.fights[0];
    assert_eq!(f.total_damage, 100.);
    assert_eq!(f.unattributed_damage, 0.);
    assert_eq!(f.players[0].damage, 100.);
    assert_eq!(f.players[0].hits, 1);
    let source = &f.players[0].sources[0];
    assert_eq!(source.kind, SourceKind::Summon);
    assert_eq!(source.item_type, None);
    assert!(!source.estimated);
}

#[test]
fn server_multishot_tracks_every_unsigned_bullet_through_wrap() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        Packet::ServerPlayerShoot(ServerPlayerShoot {
            bulletId: -1,
            ownerId: 1,
            containerType: 201,
            startingPos: pos(),
            angle: 0.,
            damage: 50,
            summonerId: 1,
            bulletType: 0,
            bulletCount: 3,
            anglesBetweenBullets: 0.,
        }),
    );
    for bullet in [65535, 0, 1] {
        apply(&mut e, 4000, damage(1, bullet, 50));
    }
    let s = e.snapshot();
    let sources = &s.fights[0].players[0].sources;
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].item_type, Some(201));
    assert_eq!(sources[0].hits, 3);
}
#[test]
fn source_snapshots_survive_gear_swaps() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        Packet::AllyShoot(AllyShoot {
            bulletId: 1,
            ownerId: 1,
            containerType: 200,
            angle: 0.,
            inspiredBuff: false,
        }),
    );
    apply(&mut e, 4000, damage(1, 1, 100));
    apply(&mut e, 5000, tick(vec![status(1, vec![stat(8, 201)])]));
    assert_eq!(
        e.snapshot().fights[0].players[0].sources[0].item_type,
        Some(200)
    );
}
#[test]
fn summon_owner_is_explicit_and_removed_mapping_expires() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(3, 301, vec![])], vec![]));
    apply(
        &mut e,
        4000,
        Packet::ServerPlayerShoot(ServerPlayerShoot {
            bulletId: 1,
            ownerId: 3,
            containerType: 301,
            startingPos: pos(),
            angle: 0.,
            damage: 100,
            summonerId: 1,
            bulletType: 0,
            bulletCount: 1,
            anglesBetweenBullets: 0.,
        }),
    );
    apply(&mut e, 5000, damage(3, 1, 100));
    assert_eq!(
        e.snapshot().fights[0].players[0].sources[0].kind,
        SourceKind::Summon
    );
    apply(&mut e, 6000, update(vec![], vec![3]));
    apply(&mut e, 7000, damage(3, 1, 100));
    assert_eq!(e.snapshot().fights[0].unattributed_damage, 100.);
}
#[test]
fn source_expiration_and_owner_scoping() {
    for delay in [5_000_000, 5_000_001] {
        let mut e = engine();
        setup(&mut e);
        apply(
            &mut e,
            10_000,
            Packet::AllyShoot(AllyShoot {
                bulletId: 65535,
                ownerId: 1,
                containerType: 200,
                angle: 0.,
                inspiredBuff: false,
            }),
        );
        apply(&mut e, 10_000 + delay, damage(1, 65535, 100));
        assert_eq!(
            e.snapshot().fights[0].players[0].sources[0].item_type,
            if delay == 5_000_000 { Some(200) } else { None }
        );
    }
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(3, 100, vec![])], vec![]));
    apply(
        &mut e,
        4000,
        Packet::AllyShoot(AllyShoot {
            bulletId: 1,
            ownerId: 1,
            containerType: 200,
            angle: 0.,
            inspiredBuff: false,
        }),
    );
    apply(&mut e, 5000, damage(3, 1, 100));
    assert_eq!(
        e.snapshot().fights[0]
            .players
            .iter()
            .find(|p| p.object_id == 3)
            .unwrap()
            .sources[0]
            .item_type,
        None
    );
}
#[test]
fn multiple_summons_may_reuse_bullet_ids_without_cross_credit() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            vec![
                object(3, 301, vec![]),
                object(4, 301, vec![]),
                object(5, 100, vec![]),
            ],
            vec![],
        ),
    );
    for (owner, summoner) in [(3, 1), (4, 5)] {
        apply(
            &mut e,
            4000,
            Packet::ServerPlayerShoot(ServerPlayerShoot {
                bulletId: -1,
                ownerId: owner,
                containerType: 301,
                startingPos: pos(),
                angle: 0.,
                damage: 100,
                summonerId: summoner,
                bulletType: 0,
                bulletCount: 1,
                anglesBetweenBullets: 0.,
            }),
        );
        apply(&mut e, 5000, damage(owner, 65535, 100));
    }
    let f = &e.snapshot().fights[0];
    assert_eq!(f.players.len(), 2);
    assert!(f.players.iter().all(|p| p.damage == 100.));
}
#[test]
fn swapping_item_without_new_enchant_data_clears_only_that_slot_evidence() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        tick(vec![status(1, vec![string(80, "AAIEewA=,,,")])]),
    );
    assert_eq!(
        e.snapshot().character.unwrap().equipment[0].enchant.ids,
        vec![123]
    );
    apply(&mut e, 4000, tick(vec![status(1, vec![stat(8, 201)])]));
    let c = e.snapshot().character.unwrap();
    assert_eq!(c.equipment[0].enchant.state, EnchantState::Missing);
    assert_eq!(c.equipment[1].enchant.state, EnchantState::RecordedEmpty);
}

#[test]
fn inferred_weapon_labels_do_not_make_run_damage_partial() {
    let mut e = inferred_source_engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(vec![object(3, 100, vec![stat(8, 200)])], vec![]),
    );
    apply(&mut e, 3500, tick(vec![]));
    apply(&mut e, 4000, damage(3, 7, 100));
    let snapshot = e.snapshot();
    assert!(!snapshot.run_totals.as_ref().unwrap().partial);
    assert!(!maxed_engine::run_totals(&snapshot.fights, None).partial);
    assert!(snapshot.fights[0].players[0].sources[0].estimated);
    apply(&mut e, 5000, shoot(1));
    apply(&mut e, 6000, hit(1));
    let snapshot = e.snapshot();
    assert!(snapshot.run_totals.as_ref().unwrap().partial);
    assert!(maxed_engine::run_totals(&snapshot.fights, None).partial);
}
