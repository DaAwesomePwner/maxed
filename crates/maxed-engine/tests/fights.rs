mod common;
use common::*;
use maxed_types::*;
#[test]
fn fight_display_names_strip_suffixes_without_changing_player_identity() {
    let mut e = engine();
    setup(&mut e);
    apply(
        &mut e,
        3000,
        tick(vec![status(1, vec![string(31, "Local,xxxx")])]),
    );
    apply(
        &mut e,
        4000,
        update(
            vec![object(3, 100, vec![string(31, "Remote,yyyy,extra")])],
            vec![],
        ),
    );
    apply(&mut e, 5000, damage(1, 1, 100));
    apply(&mut e, 6000, damage(3, 2, 100));
    let snapshot = e.snapshot();
    let rows = &snapshot.fights[0].players;
    let local = rows.iter().find(|p| p.is_local).unwrap();
    let remote = rows.iter().find(|p| !p.is_local).unwrap();
    assert_eq!(local.name.as_deref(), Some("Local"));
    assert_eq!(remote.name.as_deref(), Some("Remote"));
    assert_eq!(local.player_id, snapshot.character.as_ref().unwrap().id);
    let identities: Vec<_> = rows
        .iter()
        .map(|p| (p.object_id, p.player_id.clone()))
        .collect();
    apply(
        &mut e,
        7000,
        tick(vec![
            status(1, vec![string(31, "Local,changed")]),
            status(3, vec![string(31, "Remote,changed")]),
        ]),
    );
    apply(&mut e, 8000, damage(3, 3, 100));
    let events = e.finish(Timestamp(9000), FightEnd::CaptureStopped);
    let ended = events
        .iter()
        .find_map(|event| match event {
            EngineEvent::FightEnded(fight) => Some(fight),
            _ => None,
        })
        .unwrap();
    for row in &ended.players {
        assert!(!row.name.as_deref().unwrap().contains(','));
        assert!(identities.contains(&(row.object_id, row.player_id.clone())));
    }
}
#[test]
fn damage_ties_and_generation_boundaries() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(3, 100, vec![])], vec![]));
    apply(&mut e, 1_000_000, tick(vec![]));
    apply(&mut e, 1_001_000, damage(1, 1, 300));
    apply(&mut e, 3_000_000, tick(vec![]));
    apply(&mut e, 3_001_000, damage(3, 1, 300));
    let f = &e.snapshot().fights[0];
    assert_eq!(f.total_damage, 600.);
    assert!(f
        .players
        .iter()
        .all(|p| p.rank == Some(1) && p.dps.is_none()));
    apply(&mut e, 4_000_000, update(vec![], vec![2]));
    apply(&mut e, 4_001_000, damage(1, 2, 10));
    assert_eq!(e.snapshot().fights.len(), 1);
    apply(&mut e, 4_002_000, update(vec![target()], vec![]));
    apply(&mut e, 4_003_000, damage(1, 3, 10));
    assert_eq!(e.snapshot().fights.len(), 1);
    let events = e.finish(Timestamp(5_000_000), FightEnd::CaptureStopped);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, EngineEvent::FightEnded(_)))
            .count(),
        1
    );
    assert!(e
        .finish(Timestamp(5_001_000), FightEnd::CaptureStopped)
        .iter()
        .all(|e| !matches!(e, EngineEvent::FightEnded(_))));
}

#[test]
fn independent_bosses_and_realm_fights_without_runs() {
    let mut e = engine();
    apply(&mut e, 0, map("Realm"));
    apply(&mut e, 1000, create(1, 1));
    apply(
        &mut e,
        2000,
        update(vec![player(), target(), object(3, 300, vec![])], vec![]),
    );
    apply(&mut e, 3000, damage(1, 1, 100));
    let maxed_protocol::Packet::Damage(mut d) = damage(1, 2, 200) else {
        unreachable!()
    };
    d.targetId = 3;
    apply(&mut e, 4000, maxed_protocol::Packet::Damage(d));
    let s = e.snapshot();
    assert!(s.run.is_none());
    assert_eq!(s.fights.len(), 2);
    assert!(s
        .fights
        .iter()
        .all(|f| f.run_id.is_none() && f.boss_type == Some(300)));
    assert_eq!(s.fights[0].total_damage, 100.);
    assert_eq!(s.fights[1].total_damage, 200.);
}

#[test]
fn missing_map_or_local_identity_remains_explicit() {
    for with_map in [false, true] {
        let mut e = engine();
        if with_map {
            apply(&mut e, 0, map("Realm"));
        }
        apply(
            &mut e,
            1000,
            update(vec![object(3, 100, vec![]), target()], vec![]),
        );
        apply(&mut e, 2000, damage(3, 1, 100));
        let s = e.snapshot();
        let f = &s.fights[0];
        assert!(f.issues.contains(&CoverageIssue::MissingIdentity));
        assert!(!f.complete_capture);
        assert_eq!(f.players.len(), 1);
        assert!(!f.players[0].is_local);
        assert!(f.run_id.is_none());
        if !with_map {
            assert!(s.map.is_none());
            assert!(f.issues.contains(&CoverageIssue::Midstream));
        }
    }
}

#[test]
fn observed_local_zero_is_distinct_from_unobserved_remote_damage() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, update(vec![object(3, 100, vec![])], vec![]));
    apply(&mut e, 4000, damage(99, 1, 100));
    let s = e.snapshot();
    let f = &s.fights[0];
    assert_eq!(f.players.len(), 1);
    let local = &f.players[0];
    assert!(local.is_local);
    assert_eq!(local.damage, 0.);
    assert_eq!(local.hits, 0);
    assert_eq!(local.rank, None);
    assert_eq!(local.share, Some(0.));
    assert_eq!(f.unattributed_damage, 100.);
    assert_eq!(f.buckets[0].local, Some(0.));
}
