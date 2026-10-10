mod common;
use common::*;
use maxed_engine::Engine;
use maxed_protocol::{defs::ServerPlayerShoot, Packet};
use maxed_types::*;
use std::sync::Arc;

#[test]
fn untyped_target_stats_default_independently_and_count_each_default_per_hit() {
    for (stats, expected, defaults) in [
        (vec![stat(29, 0), stat(96, 0)], 100., 1),
        (vec![stat(21, 20), stat(96, 0)], 80., 1),
        (vec![stat(21, 20), stat(29, 0)], 80., 1),
        (vec![], 100., 3),
        (vec![stat(29, 0x1000000)], 0., 2),
        (vec![stat(96, 0x40)], 125., 2),
        (vec![stat(21, 0), stat(29, 0), stat(96, 0)], 100., 0),
    ] {
        let mut e = engine();
        apply(&mut e, 0, map("The Void"));
        apply(&mut e, 1000, create(1, 1));
        apply(&mut e, 2000, update(vec![player()], vec![]));
        apply(&mut e, 2000, tick(vec![status(2, stats)]));
        for bullet in [1, 2] {
            apply(&mut e, 3000, shoot(bullet));
            apply(&mut e, 4000, hit(bullet));
        }
        let counts = e.reconstruction_coverage();
        assert_eq!(counts["attemptedHits"], 2);
        assert_eq!(counts["reconstructedHits"], 2);
        assert_eq!(counts["omittedHits"], 0);
        assert_eq!(counts["assumedTargetStatDefault"], defaults * 2);
        assert_eq!(
            counts["reconstructedZero"],
            if expected == 0. { 2 } else { 0 }
        );
        let snapshot = e.snapshot();
        assert_eq!(snapshot.fights[0].total_damage, expected * 2.);
        if expected > 0. {
            assert_eq!(
                snapshot.fights[0].players[0].sources[0].estimated,
                defaults > 0
            );
        }
        apply(&mut e, 5000, damage(1, 1, 321));
        assert_eq!(e.snapshot().fights[0].total_damage, 321. + expected);
        assert_eq!(e.reconstruction_coverage(), counts);
    }
}

#[test]
fn target_defaults_apply_to_ability_paths_but_absent_entity_remains_an_omission() {
    for server in [false, true] {
        for present in [false, true] {
            let mut e = engine();
            apply(&mut e, 0, map("The Void"));
            apply(&mut e, 1000, create(1, 1));
            let mut objects = vec![player()];
            if present {
                objects.push(object(2, 300, vec![]));
            }
            apply(&mut e, 2000, update(objects, vec![]));
            apply(&mut e, 3000, ability_shot(server));
            apply(&mut e, 4000, hit(1));
            let counts = e.reconstruction_coverage();
            assert_eq!(counts["missingTarget"], u64::from(!present));
            assert_eq!(counts["omittedHits"], u64::from(!present));
            assert_eq!(counts["reconstructedHits"], u64::from(present));
            assert_eq!(counts["assumedTargetStatDefault"], 2 * u64::from(present));
            assert_eq!(
                e.snapshot().fights[0].total_damage,
                if present { 70. } else { 0. }
            );
        }
    }
}

#[test]
fn missing_weapon_stats_are_separate_omissions_without_invented_defaults() {
    for (stat_id, reason) in [
        (29, "missingCondition"),
        (20, "missingAttack"),
        (113, "missingExaltation"),
    ] {
        let mut e = engine();
        apply(&mut e, 0, map("The Void"));
        apply(&mut e, 1000, create(1, 1));
        let mut p = player();
        p.status.stats.retain(|s| s.statTypeNum != stat_id);
        apply(&mut e, 2000, update(vec![p, target()], vec![]));
        apply(&mut e, 3000, shoot(1));
        assert_eq!(e.reconstruction_coverage()["attemptedHits"], 0);
        apply(&mut e, 4000, hit(1));
        let counts = e.reconstruction_coverage();
        assert_eq!(counts[reason], 1);
        assert_eq!(counts["attemptedHits"], 1);
        assert_eq!(counts["reconstructedHits"], 0);
        assert_eq!(counts["omittedHits"], 1);
        assert_eq!(e.snapshot().fights[0].total_damage, 0.);
        apply(&mut e, 5000, damage(1, 1, 321));
        assert_eq!(e.snapshot().fights[0].total_damage, 321.);
        assert_eq!(e.reconstruction_coverage(), counts);
    }
}

#[test]
fn default_counts_are_per_hit_survive_finish_and_known_bonuses_multiply() {
    for known in [false, true] {
        let mut cat = catalog();
        cat.crucible_bonuses = vec![CrucibleDef {
            id: "bonus".into(),
            damage_multiplier: 1.5,
        }];
        let mut e = Engine::new(config(), Arc::new(cat)).unwrap();
        setup(&mut e);
        apply(
            &mut e,
            3000,
            tick(vec![status(
                1,
                vec![
                    string(128, if known { "bonus" } else { "unknown" }),
                    string(155, "bonus"),
                ],
            )]),
        );
        for bullet in [1, 2] {
            apply(&mut e, 4000, shoot(bullet));
            apply(&mut e, 5000, hit(bullet));
        }
        assert_eq!(
            e.snapshot().fights[0].total_damage,
            if known { 450. } else { 300. }
        );
        assert_eq!(
            e.snapshot().fights[0].players[0].sources[0].estimated,
            !known
        );
        let before = e.reconstruction_coverage();
        assert_eq!(before["assumedCrucibleDefault"], if known { 0 } else { 2 });
        assert_eq!(before["assumedBloodRitualDefault"], 0);
        e.finish(Timestamp(6000), FightEnd::CaptureStopped);
        assert_eq!(e.reconstruction_coverage(), before);
    }
}

fn ability_shot(server: bool) -> Packet {
    if server {
        Packet::ServerPlayerShoot(ServerPlayerShoot {
            bulletId: 1,
            ownerId: 1,
            containerType: 201,
            startingPos: pos(),
            angle: 0.,
            damage: 50,
            summonerId: 0,
            bulletType: 0,
            bulletCount: 1,
            anglesBetweenBullets: 0.,
        })
    } else {
        let Packet::PlayerShoot(mut p) = shoot(1) else {
            unreachable!()
        };
        p.weaponId = 201;
        Packet::PlayerShoot(p)
    }
}

#[test]
fn ability_enchant_products_are_applied_and_unknown_ids_or_legacy_metadata_are_counted() {
    for server in [false, true] {
        for mode in 0..3 {
            let mut cat = catalog();
            cat.enchants.push(EnchantDef {
                id: 123,
                name: "Synthetic".into(),
                description: String::new(),
                stat_damage_multiplier: Some(1.25),
            });
            if mode != 1 {
                cat.enchants.push(EnchantDef {
                    id: 124,
                    name: "Synthetic".into(),
                    description: String::new(),
                    stat_damage_multiplier: (mode == 0).then_some(1.5),
                });
            }
            let mut e = Engine::new(config(), Arc::new(cat)).unwrap();
            setup(&mut e);
            apply(
                &mut e,
                3000,
                tick(vec![status(1, vec![string(80, ",AAIEewB8AA==,,")])]),
            );
            apply(&mut e, 4000, ability_shot(server));
            apply(&mut e, 5000, hit(1));
            let counts = e.reconstruction_coverage();
            assert_eq!(
                e.snapshot().fights[0].total_damage,
                if mode == 0 { 87. } else { 0. }
            );
            assert_eq!(counts["unknownEnchantMultiplier"], u64::from(mode == 1));
            assert_eq!(counts["missingEnchantMetadata"], u64::from(mode == 2));
            assert_eq!(counts["reconstructedHits"], u64::from(mode == 0));
            if mode == 0 {
                assert!(!e.snapshot().fights[0].players[0].sources[0].estimated);
            }
        }
    }
}

#[test]
fn missing_ability_scaling_stat_defaults_bonus_to_zero_and_counts_it() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let mut p = player();
    p.status.stats.retain(|s| s.statTypeNum != 27);
    apply(&mut e, 2000, update(vec![p, target()], vec![]));
    apply(&mut e, 3000, ability_shot(false));
    apply(&mut e, 4000, hit(1));
    assert_eq!(e.snapshot().fights[0].total_damage, 50.);
    assert_eq!(e.reconstruction_coverage()["assumedScalingStatDefault"], 1);
}

#[test]
fn absent_expired_evicted_and_changed_generation_shots_have_distinct_reasons() {
    for mode in 0..4 {
        let mut cfg = config();
        cfg.max_shots = 1;
        let mut e = Engine::new(cfg, Arc::new(catalog())).unwrap();
        setup(&mut e);
        if mode != 0 {
            apply(&mut e, 3000, shoot(1));
        }
        let (at, reason) = match mode {
            0 => (4000, "missingProjectile"),
            1 => (5_003_001, "shotExpired"),
            2 => {
                apply(&mut e, 3500, shoot(2));
                (4000, "shotEvicted")
            }
            _ => {
                apply(&mut e, 3500, update(vec![object(1, 301, vec![])], vec![]));
                (4000, "staleOwnerGeneration")
            }
        };
        apply(&mut e, at, hit(1));
        assert_eq!(e.reconstruction_coverage()[reason], 1, "{reason}");
        assert_eq!(e.reconstruction_coverage()["reconstructedHits"], 0);
        assert_eq!(e.snapshot().fights[0].total_damage, 0.);
    }
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, hit(1));
    assert_eq!(e.reconstruction_coverage()["missingIdentity"], 1);
}
