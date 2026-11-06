use crate::common;
use common::*;
use maxed_engine::alerts::personal_best_candidate;
use maxed_protocol::{defs::QuestObjId, Packet};
use maxed_types::*;

#[test]
fn observed_reconstruction_with_neutral_stats_is_pb_eligible_after_settlement() {
    for sparse_target in [false, true] {
        let mut e = engine();
        setup(&mut e);
        if sparse_target {
            // The typed target explicitly supplies DEFENSE and primary condition.
            apply(&mut e, 2500, update(vec![object(2, 301, vec![])], vec![]));
            apply(
                &mut e,
                2600,
                update(vec![object(2, 300, vec![stat(21, 0), stat(29, 0)])], vec![]),
            );
        }
        apply(
            &mut e,
            3000,
            Packet::QuestObjId(QuestObjId {
                objectId: 2,
                list: vec![],
            }),
        );
        for (at, bullet) in [(1_000_000, 1), (5_000_000, 2)] {
            apply(&mut e, at, tick(vec![]));
            apply(&mut e, at, shoot(bullet));
            apply(&mut e, at, hit(bullet));
        }
        apply(&mut e, 6_000_000, victory());
        let snapshot = e.snapshot();
        let fight = &snapshot.fights[0];
        assert!(fight.complete_capture, "{:?}", fight.issues);
        assert!(fight.issues.is_empty());
        assert!(fight.players[0].sources.iter().all(|s| !s.estimated));
        assert!(fight.pending_local_reconciliation);
        assert!(personal_best_candidate(fight, snapshot.character.as_ref().unwrap()).is_none());
        let events = e.finish(Timestamp(7_000_000), FightEnd::CaptureStopped);
        let ended = events
            .iter()
            .find_map(|event| match event {
                EngineEvent::FightEnded(fight) => Some(fight),
                _ => None,
            })
            .unwrap();
        assert!(ended.complete_capture);
        assert!(!ended.pending_local_reconciliation);
        assert_eq!(
            personal_best_candidate(ended, snapshot.character.as_ref().unwrap())
                .unwrap()
                .dps,
            50.
        );
    }
}

#[test]
fn missing_or_unknown_multiplier_and_untyped_target_stay_uncertain() {
    for problem in [0, 1, 2, 3] {
        let mut e = engine();
        apply(&mut e, 0, map("The Void"));
        apply(&mut e, 1000, create(1, 1));
        let mut p = player();
        if problem == 0 {
            p.status.stats.retain(|s| s.statTypeNum != 128);
        }
        if problem == 1 {
            p.status.stats.retain(|s| s.statTypeNum != 155);
        }
        apply(&mut e, 2000, update(vec![p], vec![]));
        if problem == 3 {
            apply(&mut e, 3000, tick(vec![status(2, vec![])]));
        } else {
            apply(&mut e, 3000, update(vec![target()], vec![]));
        }
        if problem == 2 {
            apply(
                &mut e,
                4000,
                tick(vec![status(1, vec![string(128, "unknown-active")])]),
            );
        }
        apply(&mut e, 1_000_000, shoot(1));
        apply(&mut e, 1_100_000, hit(1));
        let snapshot = e.snapshot();
        let fight = &snapshot.fights[0];
        assert!(fight.issues.contains(&CoverageIssue::MissingStats));
        assert!(fight.players[0].sources.iter().all(|s| s.estimated));
        assert!(personal_best_candidate(fight, snapshot.character.as_ref().unwrap()).is_none());
    }
}
