mod common;
use common::*;

#[test]
fn late_joiner_uses_own_window_and_short_glimpse_keeps_damage_rank() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        update(vec![object(3, 100, vec![]), object(4, 100, vec![])], vec![]),
    );
    for (at, owner, bullet, amount) in [
        (1_000_000, 1, 1, 1500),
        (21_000_000, 3, 2, 1000),
        (30_600_000, 4, 3, 6000),
        (31_000_000, 4, 4, 6000),
        (31_000_000, 3, 5, 1000),
        (31_000_000, 1, 6, 1500),
    ] {
        apply(&mut e, at, tick(vec![]));
        apply(&mut e, at, damage(owner, bullet, amount));
    }
    let snapshot = e.snapshot();
    let fight = &snapshot.fights[0];
    let row = |id| fight.players.iter().find(|p| p.object_id == id).unwrap();
    assert_eq!(row(1).dps, Some(100.));
    assert_eq!(row(3).dps, Some(200.));
    assert_eq!(row(4).dps, None);
    assert_eq!(row(4).rank, Some(1));
    assert_eq!(row(1).rank, Some(2));
    assert_eq!(row(3).rank, Some(3));
    assert_eq!(fight.total_damage, 17000.);
    assert_eq!(row(3).share, Some(2000. / 17000.));
}

#[test]
fn three_seconds_is_the_inclusive_minimum() {
    for (elapsed, expected) in [(2_999_999, None), (3_000_000, Some(100.))] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 1_000_000, damage(1, 1, 150));
        apply(&mut e, 1_000_000 + elapsed, damage(1, 2, 150));
        assert_eq!(e.snapshot().fights[0].players[0].dps, expected);
    }
}

#[test]
fn pending_local_window_is_clipped_at_confirmed_kill_and_cannot_reopen() {
    use maxed_protocol::{defs::QuestObjId, Packet};
    use maxed_types::*;
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        Packet::QuestObjId(QuestObjId {
            objectId: 2,
            list: vec![],
        }),
    );
    for (at, bullet) in [(1_000_123, 1), (5_000_999, 2)] {
        apply(&mut e, at - 1, tick(vec![]));
        apply(&mut e, at - 1, shoot(bullet));
        apply(&mut e, at, hit(bullet));
    }
    apply(&mut e, 5_000_999, victory());
    let fight = e.snapshot().fights.remove(0);
    assert_eq!(fight.ended, Some(Timestamp(5_000_000)));
    assert_eq!(fight.players[0].damage, 200.);
    assert_eq!(fight.players[0].dps, Some(200_000_000. / 3_999_877.));
    apply(&mut e, 6_000_000, shoot(3));
    apply(&mut e, 6_100_000, hit(3));
    assert_eq!(e.snapshot().fights[0].players[0].dps, fight.players[0].dps);
}

#[test]
fn pb_candidate_uses_player_rate_and_requires_a_value() {
    use maxed_engine::alerts::personal_best_candidate;
    use maxed_types::*;
    for (elapsed, expected) in [(400_000, None), (10_000_000, Some(200.))] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 3000, update(vec![object(3, 100, vec![])], vec![]));
        apply(&mut e, 1_000_000, tick(vec![]));
        apply(&mut e, 1_000_000, damage(3, 1, 100));
        apply(&mut e, 21_000_000, tick(vec![]));
        apply(&mut e, 21_000_000, damage(1, 2, 1000));
        apply(&mut e, 21_000_000 + elapsed, tick(vec![]));
        apply(&mut e, 21_000_000 + elapsed, damage(1, 3, 1000));
        let character = e.snapshot().character.unwrap();
        let events = apply(&mut e, 32_000_000, map("Nexus"));
        let fight = events
            .iter()
            .find_map(|e| match e {
                EngineEvent::FightEnded(f) => Some(f),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            personal_best_candidate(fight, &character).map(|c| c.dps),
            expected
        );
    }
}

#[test]
fn verified_identity_keeps_the_players_accepted_window() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let mut local = player();
    local.status.stats.retain(|s| s.statTypeNum != 38);
    apply(&mut e, 2000, update(vec![local, target()], vec![]));
    for (at, bullet) in [(1_000_000, 1), (5_000_000, 2)] {
        apply(&mut e, at, tick(vec![]));
        apply(&mut e, at, damage(1, bullet, 300));
    }
    let before = e.snapshot().fights.remove(0).players.remove(0);
    assert_eq!(before.dps, Some(150.));
    apply(
        &mut e,
        5_100_000,
        tick(vec![status(1, vec![string(38, "synthetic-account")])]),
    );
    let after = e.snapshot().fights.remove(0).players.remove(0);
    assert_ne!(before.player_id, after.player_id);
    assert_eq!(after.dps, before.dps);
}
