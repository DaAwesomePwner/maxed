use crate::common;
use common::*;

#[test]
fn remote_server_damage_before_target_metadata_keeps_sources_counts_and_denominator() {
    use maxed_protocol::{defs::AllyShoot, Packet};
    use maxed_types::{CoverageIssue, SourceKind, Timestamp};

    for observed_type in [None, Some(999)] {
        let mut e = engine();
        apply(&mut e, 0, map("The Void"));
        apply(&mut e, 1000, create(1, 1));
        let mut objects = vec![player(), object(3, 100, vec![])];
        if let Some(ty) = observed_type {
            objects.push(object(2, ty, vec![]));
        }
        apply(&mut e, 2000, update(objects, vec![]));
        apply(&mut e, 1_000_000, tick(vec![]));
        apply(&mut e, 1_001_000, damage(3, 10, 40));
        apply(&mut e, 1_002_000, damage(3, 11, 60));
        apply(
            &mut e,
            1_003_000,
            Packet::AllyShoot(AllyShoot {
                bulletId: 12,
                ownerId: 3,
                containerType: 200,
                angle: 0.,
                inspiredBuff: false,
            }),
        );
        apply(&mut e, 3_000_000, tick(vec![]));
        apply(&mut e, 3_001_000, damage(3, 12, 100));
        apply(&mut e, 3_002_000, damage(99, 13, 50));
        apply(&mut e, 3_003_000, damage(3, 14, 0));
        let snapshot = e.snapshot();
        let f = &snapshot.fights[0];
        assert_eq!(f.target_type, observed_type);
        assert_eq!(f.total_damage, 250.);
        assert_eq!(f.unattributed_damage, 50.);
        assert_eq!(f.first_hit, Some(Timestamp(1_000_000)));
        assert_eq!(f.last_hit, Some(Timestamp(3_000_000)));
        assert!(!f.complete_capture);
        assert!(f.issues.contains(&CoverageIssue::MissingCatalog));
        let remote = f.players.iter().find(|p| p.object_id == 3).unwrap();
        assert_eq!(remote.damage, 200.);
        assert_eq!(remote.hits, 3);
        assert_eq!(remote.biggest_hit, Some(100.));
        assert_eq!(remote.dps, None);
        assert_eq!(remote.share, Some(0.8));
        assert_eq!(remote.sources.len(), 2);
        let other = remote
            .sources
            .iter()
            .find(|s| s.kind == SourceKind::Other)
            .unwrap();
        assert_eq!(
            (other.item_type, other.damage, other.hits, other.biggest_hit),
            (None, 100., 2, Some(60.))
        );
        let weapon = remote
            .sources
            .iter()
            .find(|s| s.kind == SourceKind::Weapon)
            .unwrap();
        assert_eq!(
            (weapon.item_type, weapon.damage, weapon.hits),
            (Some(200), 100., 1)
        );
        assert!(remote.sources.iter().all(|s| !s.estimated));

        // Filling an unknown type enriches the same fight; amounts observed
        // before UPDATE must not disappear or count twice.
        if observed_type.is_none() {
            let original_id = f.id.clone();
            apply(&mut e, 3_004_000, update(vec![target()], vec![]));
            apply(&mut e, 3_005_000, damage(3, 15, 25));
            let snapshot = e.snapshot();
            assert_eq!(snapshot.fights.len(), 1);
            assert_eq!(snapshot.fights[0].id, original_id);
            assert_eq!(snapshot.fights[0].target_type, Some(300));
            assert_eq!(snapshot.fights[0].total_damage, 275.);
        }
    }
}

#[test]
fn known_remote_and_local_player_targets_do_not_create_outgoing_fights() {
    use maxed_protocol::Packet;
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(3, 100, vec![])], vec![]));
    for target in [1, 3] {
        let Packet::Damage(mut d) = damage(99, 1, 100) else {
            unreachable!()
        };
        d.targetId = target;
        apply(&mut e, 4000, Packet::Damage(d));
    }
    assert!(e.snapshot().fights.is_empty());
}

#[test]
fn unclassified_server_target_retains_reported_damage_and_missing_catalog_evidence() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    apply(&mut e, 2000, update(vec![player()], vec![]));
    apply(&mut e, 3000, damage(999, 1, 100));
    let f = &e.snapshot().fights[0];
    assert_eq!(f.total_damage, 100.);
    assert_eq!(f.unattributed_damage, 100.);
    assert!(f
        .issues
        .contains(&maxed_types::CoverageIssue::MissingCatalog));
}
#[test]
fn paired_reports_use_server_once_in_either_order() {
    for server_first in [true, false] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 1_000_000, tick(vec![]));
        apply(&mut e, 1_010_000, shoot(-1));
        apply(
            &mut e,
            1_020_000,
            if server_first {
                damage(1, 65535, 120)
            } else {
                hit(-1)
            },
        );
        apply(
            &mut e,
            1_030_000,
            if server_first {
                hit(-1)
            } else {
                damage(1, 65535, 120)
            },
        );
        let f = &e.snapshot().fights[0];
        assert_eq!(f.total_damage, 120.);
        assert_eq!(f.players[0].hits, 1);
        assert_eq!(f.players[0].biggest_hit, Some(120.));
        assert_eq!(f.buckets.iter().map(|b| b.total).sum::<f64>(), 120.);
        assert!(!f.players[0].sources[0].estimated);
        apply(&mut e, 1_040_000, damage(1, 65535, 120));
        assert_eq!(e.snapshot().fights[0].total_damage, 240.);
    }
}
#[test]
fn paired_reports_keep_local_activity_in_either_order_at_dps_cutoff() {
    for (last_hit, expected_dps) in [(4_190_000, None), (4_210_000, Some(200. / 3.01))] {
        for server_first in [true, false] {
            let mut e = engine();
            setup(&mut e);
            for (bullet, tick_at, hit_at) in [(1, 1_000_000, 1_200_000), (2, 4_000_000, last_hit)] {
                apply(&mut e, tick_at, tick(vec![]));
                apply(&mut e, tick_at + 1_000, shoot(bullet));
                if server_first {
                    apply(&mut e, hit_at - 1_000, damage(1, bullet as u16, 100));
                }
                apply(&mut e, hit_at, hit(bullet));
                if !server_first {
                    apply(&mut e, hit_at + 1_000, damage(1, bullet as u16, 100));
                }
            }
            for settled in [false, true] {
                if settled {
                    apply(&mut e, 10_000_000, tick(vec![]));
                }
                let snapshot = e.snapshot();
                let f = &snapshot.fights[0];
                assert_eq!(f.total_damage, 200.);
                assert_eq!(f.players[0].hits, 2);
                assert_eq!(f.buckets.iter().map(|b| b.total).sum::<f64>(), 200.);
                assert_eq!(
                    f.players[0].dps, expected_dps,
                    "server_first={server_first}, settled={settled}"
                );
            }
            let character = e.snapshot().character.unwrap();
            let ended = apply(&mut e, 11_000_000, map("Nexus"));
            let fight = ended
                .iter()
                .find_map(|event| match event {
                    maxed_types::EngineEvent::FightEnded(f) => Some(f),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                maxed_engine::alerts::personal_best_candidate(fight, &character).map(|pb| pb.dps),
                expected_dps
            );
        }
    }
}
#[test]
fn missing_owner_is_in_denominator_and_incoming_is_excluded() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 1_000_000, tick(vec![]));
    apply(&mut e, 1_001_000, damage(1, 1, 100));
    apply(&mut e, 3_000_000, tick(vec![]));
    apply(&mut e, 3_001_000, damage(999, 1, 200));
    let f = &e.snapshot().fights[0];
    assert_eq!(f.total_damage, 300.);
    assert_eq!(f.unattributed_damage, 200.);
    assert_eq!(f.players[0].dps, None);
    assert_eq!(f.players[0].share, Some(1. / 3.));
}
#[test]
fn missing_projectile_is_unquantified_report() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 1_000_000, hit(5));
    let f = &e.snapshot().fights[0];
    assert_eq!(f.total_damage, 0.);
    assert!(f.first_hit.is_some());
    assert!(!f.complete_capture);
}
#[test]
fn local_unknown_class_is_still_an_incoming_target() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(2, 1));
    apply(&mut e, 2000, damage(99, 1, 100));
    assert!(e.snapshot().fights.is_empty());
}
#[test]
fn ambiguity_does_not_publish_combined_estimates() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 1_000_000, tick(vec![]));
    apply(&mut e, 1_001_000, shoot(1));
    apply(&mut e, 1_002_000, hit(1));
    assert_eq!(e.snapshot().fights[0].total_damage, 100.);
    apply(&mut e, 1_003_000, hit(1));
    assert_eq!(e.snapshot().fights[0].total_damage, 0.);
    apply(&mut e, 1_004_000, damage(1, 1, 120));
    let f = &e.snapshot().fights[0];
    assert_eq!(f.total_damage, 120.);
    assert!(f.issues.contains(&maxed_types::CoverageIssue::AmbiguousHit));
}
#[test]
fn scaling_uses_shot_time_for_local_and_hit_time_for_server_proc() {
    use maxed_protocol::{defs::*, Packet};
    for server in [false, true] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 1_000_000, tick(vec![]));
        let packet = if server {
            Packet::ServerPlayerShoot(ServerPlayerShoot {
                bulletId: 1,
                ownerId: 1,
                containerType: 201,
                startingPos: pos(),
                angle: 0.,
                damage: 50,
                summonerId: 0,
                bulletType: 0,
                bulletCount: 3,
                anglesBetweenBullets: 0.,
            })
        } else {
            let Packet::PlayerShoot(mut p) = shoot(1) else {
                unreachable!()
            };
            p.weaponId = 201;
            Packet::PlayerShoot(p)
        };
        apply(&mut e, 1_001_000, packet);
        apply(&mut e, 2_000_000, tick(vec![status(1, vec![stat(27, 80)])]));
        apply(&mut e, 2_001_000, hit(1));
        assert_eq!(
            e.snapshot().fights[0].total_damage,
            if server { 110. } else { 70. }
        );
    }
}
#[test]
fn missing_crucible_evidence_defaults_estimates_without_changing_server_damage() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        tick(vec![status(1, vec![string(128, "unknown-active")])]),
    );
    apply(&mut e, 4000, shoot(1));
    apply(&mut e, 5000, hit(1));
    assert_eq!(e.snapshot().fights[0].total_damage, 100.);
    assert_eq!(e.reconstruction_coverage()["assumedCrucibleDefault"], 1);
    assert!(e.snapshot().fights[0].players[0].sources[0].estimated);
    apply(&mut e, 6000, damage(1, 1, 123));
    assert_eq!(e.snapshot().fights[0].total_damage, 123.);
    assert_eq!(e.reconstruction_coverage()["assumedCrucibleDefault"], 1);
    assert!(!e.snapshot().fights[0].players[0].sources[0].estimated);
}
#[test]
fn fixed_damage_and_invalid_projectiles_do_not_consume_rng() {
    use maxed_engine::Engine;
    use maxed_protocol::Packet;
    use std::sync::Arc;
    let mut cat = catalog();
    let mut ranged = cat.items[0].clone();
    ranged.object_type = 202;
    ranged.projectiles[0].max_damage = 200;
    cat.items.push(ranged);
    let mut e = Engine::new(config(), Arc::new(cat)).unwrap();
    setup(&mut e);
    apply(&mut e, 3000, shoot(1));
    let Packet::PlayerShoot(mut p) = shoot(2) else {
        unreachable!()
    };
    p.weaponId = 202;
    p.projectileId = 42;
    apply(&mut e, 4000, Packet::PlayerShoot(p));
    let Packet::PlayerShoot(mut p) = shoot(3) else {
        unreachable!()
    };
    p.weaponId = 202;
    apply(&mut e, 5000, Packet::PlayerShoot(p));
    apply(&mut e, 6000, hit(3));
    assert_eq!(e.snapshot().fights[0].total_damage, 107.);
}

#[test]
fn unmatched_local_server_reports_stay_pending_until_expiry() {
    for reports in [1, 2] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 1_000_000, tick(vec![]));
        apply(&mut e, 1_001_000, shoot(1));
        for i in 0..reports {
            apply(&mut e, 1_010_000 + i, damage(1, 1, 100));
        }
        let snapshot = e.snapshot();
        assert!(
            snapshot.fights[0].pending_local_reconciliation,
            "reports={reports}"
        );
        assert_eq!(snapshot.fights[0].total_damage, reports as f64 * 100.);
        let events = e.advance_to(maxed_types::Timestamp(6_100_000));
        assert!(events.iter().any(|event| matches!(event,
            maxed_types::EngineEvent::FightUpdated(f) if !f.pending_local_reconciliation
        )));
        assert_eq!(e.snapshot().fights[0].total_damage, reports as f64 * 100.);
    }
}

#[test]
fn realm_local_recency_uses_receipt_time_in_both_pair_orders_and_on_expiry() {
    use maxed_protocol::Packet;
    use maxed_types::Timestamp;
    for order in ["estimate-first", "server-first", "estimate-only"] {
        let mut e = engine();
        let mut other = target();
        other.status.objectId = 3;
        apply(&mut e, 0, map("Realm"));
        apply(&mut e, 1000, create(1, 1));
        apply(
            &mut e,
            2000,
            update(vec![player(), target(), other], vec![]),
        );
        apply(&mut e, 1_000_000, tick(vec![]));
        for (target_id, bullet, hit_at) in [(2, 1, 1_123_456), (3, 2, 1_234_567)] {
            apply(&mut e, hit_at - 10_000, shoot(bullet));
            let Packet::EnemyHit(mut local) = hit(bullet) else {
                unreachable!()
            };
            local.targetId = target_id;
            let server = || {
                let Packet::Damage(mut server) = damage(1, bullet as u16, 100) else {
                    unreachable!()
                };
                server.targetId = target_id;
                Packet::Damage(server)
            };
            if order == "server-first" {
                apply(&mut e, hit_at - 1, server());
                let snapshot = e.snapshot();
                let fight = snapshot
                    .fights
                    .iter()
                    .find(|f| f.target_id == target_id)
                    .unwrap();
                assert_eq!(fight.local_last_hit, Some(Timestamp(1_000_000)));
            }
            apply(&mut e, hit_at, Packet::EnemyHit(local));
            let snapshot = e.snapshot();
            let fight = snapshot
                .fights
                .iter()
                .find(|f| f.target_id == target_id)
                .unwrap();
            assert_eq!(fight.local_last_hit, Some(Timestamp(hit_at)), "{order}");
            if order == "estimate-first" {
                apply(&mut e, hit_at + 1, server());
            }
        }
        for expired in [false, true] {
            if expired {
                e.advance_to(Timestamp(6_500_000));
            }
            let snapshot = e.snapshot();
            let earlier = snapshot.fights.iter().find(|f| f.target_id == 2).unwrap();
            let later = snapshot.fights.iter().find(|f| f.target_id == 3).unwrap();
            assert_eq!(
                earlier.local_last_hit,
                Some(Timestamp(1_123_456)),
                "{order}, expired={expired}"
            );
            assert_eq!(
                later.local_last_hit,
                Some(Timestamp(1_234_567)),
                "{order}, expired={expired}"
            );
            assert!(later.local_last_hit > earlier.local_last_hit);
            for fight in [earlier, later] {
                assert_eq!(fight.first_hit, Some(Timestamp(1_000_000)));
                assert_eq!(fight.last_hit, Some(Timestamp(1_000_000)));
                assert_eq!(fight.total_damage, 100.);
                assert_eq!(fight.players[0].hits, 1);
            }
        }
    }
}

#[test]
fn subattack_wire_indices_select_projectiles_without_fallback() {
    use maxed_engine::Engine;
    use maxed_protocol::Packet;
    use std::sync::Arc;
    for (mapping, cases) in [
        (
            serde_json::json!([2, 0, 1]),
            vec![(0, Some(300.)), (1, Some(100.)), (2, Some(200.)), (3, None)],
        ),
        (
            serde_json::json!([0, 0, 0, 0, 0]),
            vec![
                (0, Some(100.)),
                (1, Some(100.)),
                (2, Some(100.)),
                (3, Some(100.)),
                (4, Some(100.)),
                (5, None),
            ],
        ),
        (
            serde_json::Value::Null,
            vec![
                (-1, Some(100.)),
                (0, Some(100.)),
                (1, Some(200.)),
                (2, Some(300.)),
            ],
        ),
        (
            serde_json::json!([]),
            vec![(-1, Some(100.)), (2, Some(300.))],
        ),
        (
            serde_json::json!([null, 2]),
            vec![(0, None), (1, Some(300.))],
        ),
    ] {
        for (wire, expected) in cases {
            let mut c = catalog();
            let item = &mut c.items[0];
            let prototype = item.projectiles[0].clone();
            item.projectiles = (0..3)
                .map(|index| maxed_types::ProjectileDef {
                    index,
                    min_damage: 100 * (i32::from(index) + 1),
                    max_damage: 100 * (i32::from(index) + 1),
                    ..prototype.clone()
                })
                .collect();
            if mapping == serde_json::json!([0, 0, 0, 0, 0]) {
                item.projectiles.truncate(1);
            }
            item.weapon_firing = Some(
                serde_json::from_value(serde_json::json!({
                    "support": "unsupported", "groups": [], "omitted_projectile_indices": [],
                    "reasons": [], "subattack_projectile_indices": mapping
                }))
                .unwrap(),
            );
            let mut e = Engine::new(config(), Arc::new(c)).unwrap();
            setup(&mut e);
            let Packet::PlayerShoot(mut p) = shoot(1) else {
                unreachable!()
            };
            p.projectileId = wire;
            apply(&mut e, 3000, Packet::PlayerShoot(p));
            apply(&mut e, 4000, hit(1));
            if let Some(damage) = expected {
                assert_eq!(
                    e.snapshot().fights[0].total_damage,
                    damage,
                    "wire {wire}, mapping {mapping}"
                );
            } else {
                assert_eq!(
                    e.reconstruction_coverage()["missingProjectileDefinition"],
                    1
                );
            }
        }
    }
}

#[test]
fn subattack_damage_ranges_consume_rng_in_received_shot_order() {
    use maxed_engine::Engine;
    use maxed_protocol::Packet;
    use std::sync::Arc;
    let mut c = catalog();
    let item = &mut c.items[0];
    let prototype = item.projectiles[0].clone();
    item.projectiles = (0..3)
        .map(|index| maxed_types::ProjectileDef {
            index,
            min_damage: 100 * (i32::from(index) + 1),
            max_damage: 100 * (i32::from(index) + 1) + if index == 1 { 0 } else { 10 },
            ..prototype.clone()
        })
        .collect();
    item.weapon_firing = Some(
        serde_json::from_value(serde_json::json!({
            "support": "unsupported", "groups": [], "omitted_projectile_indices": [],
            "reasons": [], "subattack_projectile_indices": [2, 0, 1]
        }))
        .unwrap(),
    );
    let mut e = Engine::new(config(), Arc::new(c)).unwrap();
    setup(&mut e);
    // Seed 1 yields 16807, 282475249, 1622650073. The fixed 200 consumes none.
    for (bullet, wire, total) in [(1, 0, 307.), (2, 2, 507.), (3, 1, 616.), (4, 0, 919.)] {
        let Packet::PlayerShoot(mut p) = shoot(bullet) else {
            unreachable!()
        };
        p.projectileId = wire;
        apply(&mut e, 3000 + bullet as u64 * 100, Packet::PlayerShoot(p));
        apply(&mut e, 3050 + bullet as u64 * 100, hit(bullet));
        assert_eq!(e.snapshot().fights[0].total_damage, total);
    }
}
