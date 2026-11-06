use crate::common;
use common::*;
use maxed_engine::alerts::personal_best_candidate;
use maxed_protocol::{Diagnostic, Packet, PacketDiagnostic};
use maxed_types::*;

fn boss(e: &mut maxed_engine::Engine, start: u64) -> FightView {
    for (at, bullet) in [(start, 10), (start + 4_000_000, 11)] {
        apply(e, at, tick(vec![]));
        apply(e, at, shoot(bullet));
        apply(e, at + 123, hit(bullet));
    }
    apply(e, start + 5_000_000, map("Nexus"))
        .into_iter()
        .find_map(|e| match e {
            EngineEvent::FightEnded(f) => Some(f),
            _ => None,
        })
        .unwrap()
}
#[test]
fn rng_fault_before_any_fight_blocks_later_pb_until_map_reseed() {
    for problem in ["gap", "decode", "item", "projectile"] {
        let mut e = engine();
        setup(&mut e);
        match problem {
            "gap" => {
                e.diagnostic(
                    ConnId(1),
                    Direction::ClientToServer,
                    Timestamp(3000),
                    Diagnostic::Recovering,
                );
            }
            "decode" => {
                e.diagnostic(
                    ConnId(1),
                    Direction::ClientToServer,
                    Timestamp(3000),
                    Diagnostic::Packet {
                        id: 30,
                        problem: PacketDiagnostic::BadFields,
                        alignment_verified: true,
                    },
                );
            }
            _ => {
                let Packet::PlayerShoot(mut p) = shoot(1) else {
                    unreachable!()
                };
                if problem == "item" {
                    p.weaponId = 65535;
                } else {
                    p.projectileId = 42;
                }
                apply(&mut e, 3000, Packet::PlayerShoot(p));
            }
        }
        assert!(e.snapshot().fights.is_empty());
        let character = e.snapshot().character.unwrap();
        let f = boss(&mut e, 1_000_000);
        assert!(
            personal_best_candidate(&f, &character).is_none(),
            "{problem}"
        );
        assert!(
            f.players[0].sources.iter().any(|s| s.estimated),
            "{problem}"
        );
        assert!(f.issues.contains(&CoverageIssue::MissingStats));
        assert!(e.reconstruction_coverage()["untrustedRng"] > 0);
        apply(&mut e, 7_000_000, map("The Void"));
        apply(&mut e, 7_001_000, create(1, 1));
        apply(&mut e, 7_002_000, update(vec![player(), target()], vec![]));
        let character = e.snapshot().character.unwrap();
        let clean = boss(&mut e, 8_000_000);
        assert!(
            personal_best_candidate(&clean, &character).is_some(),
            "{problem}"
        );
    }
}
#[test]
fn mismatch_taints_fight_and_reconciliation_keeps_local_time() {
    for mismatch in [false, true] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 1_000_000, tick(vec![]));
        apply(&mut e, 1_100_123, shoot(1));
        apply(&mut e, 1_200_123, hit(1));
        apply(
            &mut e,
            1_300_000,
            damage(1, 1, if mismatch { 90 } else { 100 }),
        );
        apply(&mut e, 5_000_000, tick(vec![]));
        apply(&mut e, 5_100_456, shoot(2));
        apply(&mut e, 5_200_456, hit(2));
        let f = e.snapshot().fights.remove(0);
        if mismatch {
            assert!(f.issues.contains(&CoverageIssue::AmbiguousHit));
            assert!(!f.complete_capture);
        } else {
            assert_eq!(f.players[0].dps, Some(200_000_000. / 4_000_333.));
            assert!(f.complete_capture);
        }
    }
}
#[test]
fn typed_reentry_clears_omitted_stats_and_only_secondary_condition_defaults_known() {
    for omitted in [21, 29, 96] {
        let mut e = engine();
        setup(&mut e);
        apply(
            &mut e,
            3000,
            tick(vec![status(
                2,
                vec![stat(21, 50), stat(29, 0x2000000), stat(96, 0x40)],
            )]),
        );
        apply(&mut e, 4000, update(vec![], vec![2]));
        let stats = [21, 29, 96]
            .into_iter()
            .filter(|id| *id != omitted)
            .map(|id| stat(id, 0))
            .collect();
        apply(&mut e, 5000, update(vec![object(2, 300, stats)], vec![]));
        apply(&mut e, 1_000_000, tick(vec![]));
        apply(&mut e, 1_000_000, shoot(1));
        apply(&mut e, 1_100_000, hit(1));
        let f = e.snapshot().fights.remove(0);
        assert_eq!(f.players[0].damage, 100., "{omitted}");
        assert_eq!(
            f.players[0].sources[0].estimated,
            omitted != 96,
            "{omitted}"
        );
    }
}

#[test]
fn non_shoot_decode_failure_preserves_rng_for_later_boss() {
    for id in [67, 10, 42] {
        let mut e = engine();
        setup(&mut e);
        e.diagnostic(
            ConnId(1),
            Direction::ServerToClient,
            Timestamp(3000),
            Diagnostic::Packet {
                id,
                problem: PacketDiagnostic::BadFields,
                alignment_verified: true,
            },
        );
        let character = e.snapshot().character.unwrap();
        let fight = boss(&mut e, 1_000_000);
        assert!(
            personal_best_candidate(&fight, &character).is_some(),
            "{id}"
        );
    }
}

#[test]
fn damage_mismatch_taints_only_its_fight_on_the_same_map() {
    for reported in [0, 90, 110] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 1_000_000, tick(vec![]));
        apply(&mut e, 1_100_000, shoot(1));
        apply(&mut e, 1_200_000, hit(1));
        apply(&mut e, 1_300_000, damage(1, 1, reported));
        let mut other = target();
        other.status.objectId = 3;
        apply(&mut e, 2_000_000, update(vec![other], vec![]));
        for (at, bullet) in [(3_000_000, 2), (7_000_000, 3)] {
            apply(&mut e, at, tick(vec![]));
            apply(&mut e, at, shoot(bullet));
            let Packet::EnemyHit(mut packet) = hit(bullet) else {
                unreachable!()
            };
            packet.targetId = 3;
            apply(&mut e, at + 123, Packet::EnemyHit(packet));
        }
        let character = e.snapshot().character.unwrap();
        let ended = apply(&mut e, 8_000_000, map("Nexus"));
        let fights: Vec<_> = ended
            .iter()
            .filter_map(|event| match event {
                EngineEvent::FightEnded(f) => Some(f),
                _ => None,
            })
            .collect();
        let affected = fights.iter().find(|f| f.target_id == 2).unwrap();
        assert!(affected.issues.contains(&CoverageIssue::AmbiguousHit));
        assert!(personal_best_candidate(affected, &character).is_none());
        let clean = fights.iter().find(|f| f.target_id == 3).unwrap();
        assert!(
            personal_best_candidate(clean, &character).is_some(),
            "{reported}"
        );
    }
}
