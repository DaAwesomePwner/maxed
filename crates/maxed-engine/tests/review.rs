use crate::common;
use common::*;
use maxed_types::*;

#[test]
fn both_crucible_channels_use_counted_neutral_defaults() {
    for missing in [128, 155] {
        let mut e = engine();
        apply(&mut e, 0, map("The Void"));
        apply(&mut e, 1000, create(1, 1));
        let mut p = player();
        p.status.stats.retain(|s| s.statTypeNum != missing);
        apply(&mut e, 2000, update(vec![p, target()], vec![]));
        apply(&mut e, 3000, shoot(1));
        apply(&mut e, 4000, hit(1));
        assert_eq!(e.snapshot().fights[0].total_damage, 100.);
        assert_eq!(
            e.reconstruction_coverage()[if missing == 128 {
                "assumedCrucibleDefault"
            } else {
                "assumedBloodRitualDefault"
            }],
            1
        );
        assert!(e.snapshot().fights[0].players[0].sources[0].estimated);
    }
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        tick(vec![status(
            1,
            vec![string(128, ""), string(155, "unknown-bonus")],
        )]),
    );
    apply(&mut e, 4000, shoot(1));
    apply(&mut e, 5000, hit(1));
    assert_eq!(e.snapshot().fights[0].total_damage, 100.);
    assert_eq!(e.reconstruction_coverage()["assumedBloodRitualDefault"], 1);
    assert_eq!(e.reconstruction_coverage()["assumedCrucibleDefault"], 0);
    assert!(e.snapshot().fights[0]
        .issues
        .contains(&CoverageIssue::MissingStats));
}

#[test]
fn zero_damage_reports_extend_window_without_contributing() {
    let mut e = engine();
    setup(&mut e);
    let events = apply(&mut e, 1_000_000, damage(1, 1, 0));
    assert!(events
        .iter()
        .any(|v| matches!(v, EngineEvent::FightStarted(_))));
    apply(&mut e, 2_000_000, damage(1, 2, 100));
    apply(&mut e, 3_000_000, damage(1, 3, 100));
    let s = e.snapshot();
    let f = &s.fights[0];
    assert_eq!(f.first_hit, Some(Timestamp(1_000_000)));
    assert_eq!(f.players[0].dps, None);
    assert_eq!(f.players[0].hits, 2);
    assert_eq!(f.total_damage, 200.);
}

#[test]
fn returning_bag_and_player_keep_their_identity() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(
            vec![object(3, 100, vec![]), object(4, 1292, vec![])],
            vec![],
        ),
    );
    apply(&mut e, 4000, damage(3, 1, 100));
    let before = e.snapshot();
    apply(&mut e, 5000, update(vec![], vec![3, 4]));
    let events = apply(
        &mut e,
        6000,
        update(
            vec![object(3, 100, vec![]), object(4, 1292, vec![])],
            vec![],
        ),
    );
    assert!(!events
        .iter()
        .any(|v| matches!(v, EngineEvent::BagDropped(_))));
    apply(&mut e, 7000, damage(3, 2, 100));
    let after = e.snapshot();
    assert_eq!(after.drops.len(), 1);
    assert_eq!(after.drops[0].id, before.drops[0].id);
    let rows: Vec<_> = after.fights[0]
        .players
        .iter()
        .filter(|p| p.object_id == 3)
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].damage, 200.);
}

#[test]
fn returning_retired_target_cannot_bypass_resource_tombstone() {
    use maxed_engine::Engine;
    use std::sync::Arc;
    let mut c = config();
    c.max_active_fights = 1;
    let mut e = Engine::new(c, Arc::new(catalog())).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, update(vec![object(3, 301, vec![])], vec![]));
    let maxed_protocol::Packet::Damage(mut d) = damage(1, 2, 100) else {
        unreachable!()
    };
    d.targetId = 3;
    apply(&mut e, 5000, maxed_protocol::Packet::Damage(d));
    apply(&mut e, 6000, update(vec![], vec![2]));
    apply(&mut e, 7000, update(vec![target()], vec![]));
    assert!(!apply(&mut e, 8000, damage(1, 3, 100))
        .iter()
        .any(|v| matches!(v, EngineEvent::FightStarted(_))));
}

#[test]
fn changed_type_is_actual_new_generation() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, update(vec![object(2, 301, vec![])], vec![]));
    apply(&mut e, 5000, damage(1, 2, 100));
    let s = e.snapshot();
    assert_eq!(s.fights.len(), 2);
    assert_ne!(s.fights[0].target_generation, s.fights[1].target_generation);
}

fn server_shot(owner: i32, summoner: i32, item: i32) -> maxed_protocol::Packet {
    use maxed_protocol::{defs::ServerPlayerShoot, Packet};
    Packet::ServerPlayerShoot(ServerPlayerShoot {
        bulletId: 1,
        ownerId: owner,
        containerType: item,
        startingPos: pos(),
        angle: 0.,
        damage: 50,
        summonerId: summoner,
        bulletType: 0,
        bulletCount: 1,
        anglesBetweenBullets: 0.,
    })
}

#[test]
fn source_defaults_are_caller_specific_and_server_item_is_container() {
    use maxed_engine::Engine;
    use maxed_protocol::{defs::AllyShoot, Packet};
    use std::sync::Arc;
    for slot in [None, Some(0)] {
        for caller in 0..3 {
            let mut cat = catalog();
            cat.items[0].slot_type = slot;
            let mut e = Engine::new(config(), Arc::new(cat)).unwrap();
            setup(&mut e);
            let packet = match caller {
                0 => Packet::AllyShoot(AllyShoot {
                    bulletId: 1,
                    ownerId: 1,
                    containerType: 200,
                    angle: 0.,
                    inspiredBuff: false,
                }),
                1 => server_shot(1, 1, 200),
                _ => shoot(1),
            };
            apply(&mut e, 3000, packet);
            apply(&mut e, 4000, damage(1, 1, 100));
            assert_eq!(
                e.snapshot().fights[0].players[0].sources[0].kind,
                [
                    SourceKind::Weapon,
                    SourceKind::ItemEffect,
                    SourceKind::Unknown
                ][caller]
            );
        }
    }
    let mut cat = catalog();
    cat.items[1].scaling.as_mut().unwrap().origin_item = Some(200);
    let mut e = Engine::new(config(), Arc::new(cat)).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, server_shot(1, 1, 201));
    apply(&mut e, 4000, damage(1, 1, 50));
    assert_eq!(
        e.snapshot().fights[0].players[0].sources[0].item_type,
        Some(201)
    );
}

#[test]
fn resolved_summon_keeps_source_without_a_remembered_shot() {
    for (bullet, at) in [(2, 5000), (1, 5_004_001)] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 3000, update(vec![object(3, 301, vec![])], vec![]));
        apply(&mut e, 4000, server_shot(3, 1, 201));
        apply(&mut e, at, damage(3, bullet, 100));
        let s = e.snapshot();
        let source = &s.fights[0].players[0].sources[0];
        assert_eq!(source.kind, SourceKind::Summon);
        assert_eq!(source.item_type, Some(301));
    }
}

#[test]
fn scaling_only_requires_enchant_multiplier_for_matching_ability() {
    use maxed_protocol::Packet;
    for (ability, enchants, expected) in [
        (201, Some("AAIEewA=,,,"), 70.), // weapon enchant is irrelevant
        (201, Some(",AAIEewA=,,"), 0.),
        (200, Some(",AAIEewA=,,"), 70.), // different ability is irrelevant
        (201, Some("AAIE"), 70.),        // missing ability evidence defaults to 1
        (201, Some(",invalid,,"), 70.),  // malformed ability evidence defaults to 1
        (201, Some(",,,"), 70.),
        (200, None, 70.),
        (201, None, 70.),
    ] {
        for server in [false, true] {
            let mut e = engine();
            apply(&mut e, 0, map("The Void"));
            apply(&mut e, 1000, create(1, 1));
            let mut p = player();
            p.status
                .stats
                .retain(|s| s.statTypeNum != 80 && s.statTypeNum != 9);
            p.status.stats.push(stat(9, ability));
            if let Some(v) = enchants {
                p.status.stats.push(string(80, v));
            }
            apply(&mut e, 2000, update(vec![p, target()], vec![]));
            let shot = if server {
                server_shot(1, 1, 201)
            } else {
                let Packet::PlayerShoot(mut p) = shoot(1) else {
                    unreachable!()
                };
                p.weaponId = 201;
                Packet::PlayerShoot(p)
            };
            apply(&mut e, 3000, shot);
            apply(&mut e, 4000, hit(1));
            let counts = e.reconstruction_coverage();
            assert_eq!(
                counts["assumedAbilityEnchantDefault"],
                u64::from(ability == 201 && matches!(enchants, None | Some("AAIE" | ",invalid,,")))
            );
            assert_eq!(
                counts["unknownEnchantMultiplier"],
                u64::from(ability == 201 && enchants == Some(",AAIEewA=,,"))
            );
            assert_eq!(
                counts["omittedHits"],
                u64::from(ability == 201 && enchants == Some(",AAIEewA=,,"))
            );
            if enchants == Some(",invalid,,") {
                let snapshot = e.snapshot();
                let source = &snapshot.fights[0].players[0].sources[0];
                assert!(source.estimated);
                assert_eq!(source.enchant.state, EnchantState::Invalid);
            }
            assert_eq!(
                e.snapshot().fights[0].total_damage,
                expected,
                "ability={ability} enchant={enchants:?} server={server}"
            );
        }
    }
}

#[test]
fn unchanged_fame_is_not_reemitted_across_maps_or_reconnects() {
    let mut e = engine();
    setup(&mut e);
    for conn in 1..=6 {
        connection(&mut e, conn, conn * 10_000, map("The Void"));
        connection(&mut e, conn, conn * 10_000 + 1000, create(1, 1));
        let events = connection(
            &mut e,
            conn,
            conn * 10_000 + 2000,
            update(vec![player()], vec![]),
        );
        assert!(!events
            .iter()
            .any(|v| matches!(v, EngineEvent::FameChanged { .. })));
    }
    let events = connection(
        &mut e,
        6,
        70_000,
        tick(vec![status(1, vec![stat(57, 101)])]),
    );
    assert_eq!(
        events
            .iter()
            .filter(|v| matches!(v, EngineEvent::FameChanged { fame: 101, .. }))
            .count(),
        1
    );
}

#[test]
fn fight_updates_are_coalesced_and_end_is_immediate() {
    let mut e = engine();
    setup(&mut e);
    assert!(apply(&mut e, 3000, damage(1, 1, 100))
        .iter()
        .any(|v| matches!(v, EngineEvent::FightStarted(_))));
    for at in 4000..5000 {
        assert!(!apply(&mut e, at, damage(1, 2, 1))
            .iter()
            .any(|v| matches!(v, EngineEvent::FightUpdated(_))));
    }
    assert_eq!(e.snapshot().fights[0].total_damage, 1100.);
    assert!(!e
        .advance_to(Timestamp(249_999))
        .iter()
        .any(|v| matches!(v, EngineEvent::FightUpdated(_))));
    let events = e.advance_to(Timestamp(250_000));
    assert_eq!(
        events
            .iter()
            .filter(|v| matches!(v,EngineEvent::FightUpdated(f) if f.total_damage==1100.))
            .count(),
        1
    );
    apply(&mut e, 251_000, damage(1, 3, 100));
    let events = e.finish(Timestamp(252_000), FightEnd::AppClosed);
    assert!(events
        .iter()
        .any(|v| matches!(v,EngineEvent::FightEnded(f) if f.total_damage==1200.)));
    assert!(!e
        .advance_to(Timestamp(500_000))
        .iter()
        .any(|v| matches!(v, EngineEvent::FightUpdated(_))));
}

#[test]
fn long_realm_prunes_unreferenced_view_exits_without_resource_limit() {
    use maxed_engine::Engine;
    use std::sync::Arc;
    let mut cfg = config();
    cfg.max_entities = 4;
    let mut e = Engine::new(cfg, Arc::new(catalog())).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    apply(&mut e, 4000, update(vec![], vec![2])); // fight reference must survive every prune
    for i in 0..20 {
        let at = 31_000_000 * (i as u64 + 1);
        apply(
            &mut e,
            at,
            update(vec![object(100 + i, 301, vec![])], vec![]),
        );
        apply(&mut e, at + 1000, update(vec![], vec![100 + i]));
    }
    apply(&mut e, 630_000_000, update(vec![target()], vec![]));
    apply(&mut e, 630_001_000, damage(1, 2, 100));
    let s = e.snapshot();
    assert_eq!(s.fights.len(), 1);
    assert_eq!(s.fights[0].total_damage, 200.);
    assert!(!s.issues.contains(&CoverageIssue::ResourceLimit));
}

#[test]
fn known_crucible_bonuses_multiply_and_deltas_preserve_other_channel() {
    use maxed_engine::Engine;
    use std::sync::Arc;
    let mut cat = catalog();
    cat.crucible_bonuses = vec![
        CrucibleDef {
            id: "a".into(),
            damage_multiplier: 1.5,
        },
        CrucibleDef {
            id: "b".into(),
            damage_multiplier: 2.,
        },
    ];
    let mut e = Engine::new(config(), Arc::new(cat)).unwrap();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        tick(vec![status(1, vec![string(128, "a"), string(155, "b")])]),
    );
    apply(&mut e, 4000, shoot(1));
    apply(&mut e, 5000, hit(1));
    assert_eq!(e.snapshot().fights[0].total_damage, 300.);
    apply(&mut e, 6000, tick(vec![status(1, vec![string(128, "")])]));
    apply(&mut e, 7000, shoot(2));
    apply(&mut e, 8000, hit(2));
    assert_eq!(e.snapshot().fights[0].total_damage, 500.);
}
