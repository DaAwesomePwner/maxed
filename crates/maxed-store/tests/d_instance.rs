use crate::common;
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;

fn setup() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()).unwrap();
    (dir, path, db)
}
fn event(kind: InstanceEventKind) -> InstanceEventView {
    InstanceEventView {
        id: "instance-1".into(),
        session_id: "demo-session".into(),
        run_id: Some("demo-run".into()),
        map: Some("Synthetic dungeon".into()),
        at: Timestamp(4_000_000),
        kind,
        provisional: true,
    }
}
fn kinds() -> Vec<(InstanceEventKind, TimelineKind)> {
    vec![
        (
            InstanceEventKind::PlayerDeath {
                player_id: None,
                player_name: "SynthAlpha".into(),
                level: Some(1),
                killed_by: Some("Snomipede".into()),
                grave_type: None,
                object_id: Some(42),
                class_id: None,
                is_local: false,
            },
            TimelineKind::PartyDeath,
        ),
        (
            InstanceEventKind::PlayerLeft {
                object_id: 43,
                player_name: Some("SynthBravo".into()),
                class_id: None,
            },
            TimelineKind::PlayerLeft,
        ),
        (
            InstanceEventKind::PortalOpened {
                opener_is_local: None,
                opener: Some("SynthAlpha".into()),
                portal_type: Some(44),
                dungeon: Some("Synthetic dungeon".into()),
            },
            TimelineKind::PortalOpened,
        ),
        (
            InstanceEventKind::RealmEventKilled {
                boss_key: "Synthetic boss".into(),
                object_type: None,
                killer: None,
            },
            TimelineKind::Kill,
        ),
    ]
}
#[test]
fn instance_event_projects_row_and_timeline_kind() {
    for (kind, timeline_kind) in kinds() {
        let (_dir, path, mut db) = setup();
        let e = event(kind);
        apply_batch(
            &mut db,
            &[common::envelope(8, EngineEvent::InstanceEvent(e.clone()))],
        )
        .unwrap();
        let story = open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap();
        let row = story
            .timeline
            .iter()
            .find(|r| r.related_id.as_deref() == Some(&e.id))
            .unwrap();
        assert_eq!(row.kind, timeline_kind);
        assert!(row.explanation.contains("provisional"));
        let mut realm = e;
        realm.id = "realm-event".into();
        realm.run_id = None;
        apply_batch(
            &mut db,
            &[common::envelope(9, EngineEvent::InstanceEvent(realm))],
        )
        .unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM instance_events", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM timeline WHERE related_id='realm-event'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}
#[test]
fn provisional_flags_survive_store_round_trip() {
    let (_dir, path, mut db) = setup();
    let e = event(kinds().remove(0).0);
    apply_batch(
        &mut db,
        &[common::envelope(8, EngineEvent::InstanceEvent(e.clone()))],
    )
    .unwrap();
    let stored: String = db
        .query_row("SELECT json FROM instance_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<InstanceEventView>(&stored).unwrap(),
        e
    );
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.kill = Some(KillView {
        at: Timestamp(4_000_000),
        evidence: KillEvidence::VictorySoleBoss,
        provisional: true,
    });
    apply_batch(
        &mut db,
        &[common::envelope(
            9,
            EngineEvent::FightUpdated(fight.clone()),
        )],
    )
    .unwrap();
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .get_fight(&fight.id)
            .unwrap()
            .unwrap()
            .kill,
        fight.kill
    );
}

#[test]
fn historical_names_are_normalized_in_logs_and_run_events_without_rewriting_json() {
    for (mut kind, _) in kinds() {
        if let InstanceEventKind::RealmEventKilled { killer, .. } = &mut kind {
            *killer = Some("SynthAlpha".into());
        }
        if let InstanceEventKind::PortalOpened {
            opener_is_local, ..
        } = &mut kind
        {
            // Historical attribution is deliberately not repaired by display normalization.
            *opener_is_local = Some(false);
        }
        let expected = event(kind);
        let mut historical = expected.clone();
        match &mut historical.kind {
            InstanceEventKind::PlayerDeath { player_name, .. } => player_name.push_str(",fe14"),
            InstanceEventKind::PlayerLeft { player_name, .. } => {
                player_name.as_mut().unwrap().push_str(",fe14")
            }
            InstanceEventKind::PortalOpened { opener, .. } => {
                opener.as_mut().unwrap().push_str(",fe14")
            }
            InstanceEventKind::RealmEventKilled { killer, .. } => {
                killer.as_mut().unwrap().push_str(",fe14")
            }
        }
        let (_dir, path, mut db) = setup();
        apply_batch(
            &mut db,
            &[common::envelope(
                8,
                EngineEvent::InstanceEvent(historical.clone()),
            )],
        )
        .unwrap();
        let reader = open_reader(&path).unwrap();
        let page = reader
            .list_events(EventFilter {
                kinds: vec![],
                query: String::new(),
                run_id: None,
                dungeon: None,
                range: None,
                opener_local: None,
                cursor: None,
                limit: 50,
            })
            .unwrap();
        assert_eq!(page.rows.as_slice(), std::slice::from_ref(&expected));
        let story = reader.get_run("demo-run").unwrap().unwrap();
        assert_eq!(
            story.events.as_deref().unwrap(),
            std::slice::from_ref(&expected)
        );
        let summary = story
            .timeline
            .iter()
            .find(|row| row.related_id.as_deref() == Some(&expected.id))
            .unwrap();
        let explanation = match expected.kind {
            InstanceEventKind::PlayerDeath { .. } => {
                "SynthAlpha died to Snomipede (level 1) (provisional)"
            }
            InstanceEventKind::PlayerLeft { .. } => {
                "SynthBravo left (cause unconfirmed) (provisional)"
            }
            InstanceEventKind::PortalOpened { .. } => {
                "SynthAlpha opened Synthetic dungeon (provisional)"
            }
            InstanceEventKind::RealmEventKilled { .. } => {
                "Synthetic boss killed by SynthAlpha (provisional)"
            }
        };
        assert_eq!(summary.explanation, explanation);
        let json: String = db
            .query_row("SELECT json FROM instance_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<InstanceEventView>(&json).unwrap(),
            historical
        );
    }
}

#[test]
fn historical_unknown_event_names_stay_unknown() {
    for kind in [
        InstanceEventKind::PortalOpened {
            opener: None,
            opener_is_local: None,
            portal_type: None,
            dungeon: None,
        },
        InstanceEventKind::PlayerLeft {
            object_id: 43,
            player_name: None,
            class_id: None,
        },
        InstanceEventKind::RealmEventKilled {
            boss_key: "Synthetic_Boss".into(),
            object_type: None,
            killer: None,
        },
    ] {
        let (_dir, path, mut db) = setup();
        let expected = event(kind);
        apply_batch(
            &mut db,
            &[common::envelope(
                8,
                EngineEvent::InstanceEvent(expected.clone()),
            )],
        )
        .unwrap();
        assert_eq!(
            open_reader(&path)
                .unwrap()
                .get_run("demo-run")
                .unwrap()
                .unwrap()
                .events
                .unwrap(),
            [expected]
        );
    }
}
#[test]
fn event_upsert_is_idempotent() {
    let (_dir, _path, mut db) = setup();
    let e = event(kinds().remove(1).0);
    let envelope = common::envelope(8, EngineEvent::InstanceEvent(e.clone()));
    apply_batch(&mut db, &[envelope.clone(), envelope]).unwrap();
    apply_batch(
        &mut db,
        &[common::envelope(9, EngineEvent::InstanceEvent(e.clone()))],
    )
    .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM instance_events", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM timeline WHERE related_id=?1",
            [&e.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let mut realm = e;
    realm.run_id = None;
    apply_batch(
        &mut db,
        &[common::envelope(10, EngineEvent::InstanceEvent(realm))],
    )
    .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM timeline WHERE kind='player_left'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn saved_departures_are_filtered_at_read_time_by_completed_at() {
    for completed_at in [None, Some(Timestamp(5_000_000))] {
        let (_dir, path, mut db) = setup();
        let mut run = demo::demo_run_story().run;
        run.completed_at = completed_at;
        let mut events = vec![common::envelope(8, EngineEvent::RunUpdated(run.clone()))];
        for (index, at) in [4_999_000, 5_000_000, 5_001_000].into_iter().enumerate() {
            let mut row = event(InstanceEventKind::PlayerLeft {
                object_id: index as i32,
                player_name: None,
                class_id: None,
            });
            row.id = format!("departure-{index}");
            row.at = Timestamp(at);
            events.push(common::envelope(
                9 + index as u64,
                EngineEvent::InstanceEvent(row),
            ));
        }
        apply_batch(&mut db, &events).unwrap();
        let reader = open_reader(&path).unwrap();
        let story = reader.get_run(&run.id).unwrap().unwrap();
        let left: Vec<_> = story
            .timeline
            .iter()
            .filter(|r| r.kind == TimelineKind::PlayerLeft)
            .collect();
        assert_eq!(left.len(), if completed_at.is_some() { 1 } else { 3 });
        assert_eq!(left[0].at, Timestamp(4_999_000));
        assert_eq!(
            reader
                .list_runs(common::filter(RunSort::Newest, 50))
                .unwrap()
                .rows[0]
                .completed_at,
            completed_at
        );
        // Reading history never rewrites or deletes the original facts.
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM timeline WHERE kind='player_left'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            3
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM instance_events WHERE kind='player_left'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            3
        );
    }
}
#[test]
fn kill_timeline_only_for_boss_event_miniboss() {
    for kind in [
        TargetKind::Boss,
        TargetKind::EventBoss,
        TargetKind::Miniboss,
        TargetKind::Quest,
        TargetKind::Ordinary,
    ] {
        let (_dir, path, mut db) = setup();
        let mut f = demo::demo_snapshot().fights.remove(0);
        f.target_kind = Some(kind.clone());
        f.kill = Some(KillView {
            at: Timestamp(4_000_000),
            evidence: KillEvidence::HpZeroRemoved,
            provisional: false,
        });
        apply_batch(
            &mut db,
            &[
                common::envelope(8, EngineEvent::FightUpdated(f.clone())),
                common::envelope(9, EngineEvent::FightEnded(f.clone())),
            ],
        )
        .unwrap();
        let story = open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap();
        let kills: Vec<_> = story
            .timeline
            .iter()
            .filter(|t| t.kind == TimelineKind::Kill)
            .collect();
        assert_eq!(
            kills.len(),
            usize::from(matches!(
                kind,
                TargetKind::Boss | TargetKind::EventBoss | TargetKind::Miniboss
            ))
        );
        if let Some(kill) = kills.first() {
            assert_eq!(kill.related_id.as_deref(), Some(f.id.as_str()));
        }
        f.kill = None;
        apply_batch(
            &mut db,
            &[common::envelope(10, EngineEvent::FightUpdated(f))],
        )
        .unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM timeline WHERE kind='kill'", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
