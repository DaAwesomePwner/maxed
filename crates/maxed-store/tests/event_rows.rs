use crate::common;
use common::*;
use maxed_store::{apply_batch, migrate, StoreError};
use maxed_types::*;
use rusqlite::Connection;

#[test]
fn run_identity_assignment_and_changes_refresh_only_the_matching_entry_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let mut events = story_events();
    if let EngineEvent::RunStarted(run) = &mut events[2].event {
        run.character_id = None;
    }
    apply_batch(&mut db, &events).unwrap();
    let reader = maxed_store::open_reader(&path).unwrap();
    assert!(reader
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .character
        .is_none());
    let mut run = demo::demo_run_story().run;
    let first = demo::demo_snapshot().character.unwrap();
    apply_batch(
        &mut db,
        &[envelope(8, EngineEvent::RunUpdated(run.clone()))],
    )
    .unwrap();
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().character,
        Some(first.clone())
    );
    let mut second = first.clone();
    second.id = "second".into();
    second.name = Some("Second Knight".into());
    second.equipment.clear();
    run.character_id = Some(second.id.clone());
    apply_batch(
        &mut db,
        &[
            envelope(9, EngineEvent::CharacterUpdated(second.clone())),
            envelope(10, EngineEvent::RunUpdated(run.clone())),
        ],
    )
    .unwrap();
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().character,
        Some(second.clone())
    );
    let mut later = second.clone();
    later.name = Some("Later gear and name".into());
    later.equipment = first.equipment;
    apply_batch(
        &mut db,
        &[
            envelope(11, EngineEvent::CharacterUpdated(later)),
            envelope(12, EngineEvent::RunUpdated(run.clone())),
        ],
    )
    .unwrap();
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().character,
        Some(second)
    );
    run.character_id = None;
    apply_batch(&mut db, &[envelope(13, EngineEvent::RunUpdated(run))]).unwrap();
    assert!(reader
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .character
        .is_none());
}

#[test]
fn receipts_projection_and_head_commit_together_and_retries_are_exact() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let events = story_events();
    apply_batch(&mut db, &events).unwrap();
    apply_batch(&mut db, &events).unwrap();
    assert_eq!(common::receipts::count(&db), 7);
    for (table, count) in [
        ("runs", 1),
        ("fights", 1),
        ("fight_players", 1),
        ("damage_sources", 1),
        ("drops", 1),
        ("drop_items", 1),
        ("fame_samples", 2),
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            count
        );
    }
    let mut changed = events[0].clone();
    changed.at = Timestamp(123);
    assert!(matches!(
        apply_batch(&mut db, &[changed]),
        Err(StoreError::InvalidEvent)
    ));
    assert!(matches!(
        apply_batch(
            &mut db,
            &[envelope(
                9,
                EngineEvent::MapEntered {
                    realm_instance_key: None,
                    at: Timestamp(1),
                    name: "map".into(),
                    kind: AreaKind::Realm
                }
            )]
        ),
        Err(StoreError::SequenceGap)
    ));
    let mut bad = events[4].clone();
    bad.sequence = Sequence(Timestamp(9));
    if let EngineEvent::BagDropped(drop) = &mut bad.event {
        drop.run_id = Some("missing".into());
    }
    assert!(apply_batch(
        &mut db,
        &[
            envelope(
                8,
                EngineEvent::CaptureStateChanged(CaptureStatus::default())
            ),
            bad
        ]
    )
    .is_err());
    assert_eq!(common::receipts::head(&db, "test-stream").unwrap(), 7);
    assert!(!db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
}

#[test]
fn revisions_replace_children_and_stable_completion_timeline() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    let mut story = demo::demo_run_story();
    story.run.evidence[0].evidence = Evidence::CompletionCounter {
        before: 1,
        after: 2,
        delay_micros: 1,
    };
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.players.clear();
    fight.buckets.clear();
    story.drops[0].contents.clear();
    story.drops[0].items.clear();
    let updates = [
        EngineEvent::RunUpdated(story.run.clone()),
        EngineEvent::RunEnded(story.run),
        EngineEvent::FightUpdated(fight.clone()),
        EngineEvent::FightEnded(fight.clone()),
        EngineEvent::DropUpdated(story.drops[0].clone()),
    ];
    let events: Vec<_> = updates
        .into_iter()
        .enumerate()
        .map(|(i, e)| envelope(8 + i as u64, e))
        .collect();
    apply_batch(&mut db, &events).unwrap();
    for (table, expected) in [
        ("fight_players", 0),
        ("damage_sources", 0),
        ("drop_items", 1),
    ] {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            expected
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM timeline WHERE kind='completion'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn merges_rewrite_ids_but_preserve_entry_gear_and_names() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    let mut verified = demo::demo_snapshot().character.unwrap();
    verified.id = "verified".into();
    verified.name = Some("Later name".into());
    verified.equipment.clear();
    verified.identity_verified = true;
    apply_batch(
        &mut db,
        &[
            envelope(
                8,
                EngineEvent::PlayerDied {
                    at: Timestamp(7_000_000),
                    character_id: Some("demo-character".into()),
                    run_id: Some("demo-run".into()),
                },
            ),
            envelope(9, EngineEvent::CharacterUpdated(verified)),
            envelope(
                10,
                EngineEvent::CharacterMerged {
                    provisional_id: "demo-character".into(),
                    verified_id: "verified".into(),
                },
            ),
            envelope(
                11,
                EngineEvent::FameChanged {
                    at: Timestamp(8_000_000),
                    character_id: "demo-character".into(),
                    fame: 1270,
                },
            ),
        ],
    )
    .unwrap();
    let (id, json): (String, String) = db
        .query_row(
            "SELECT character_id,entry_character_json FROM runs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let character: CharacterView = serde_json::from_str(&json).unwrap();
    assert_eq!(id, "verified");
    assert_eq!(character.id, "verified");
    assert_eq!(character.name.as_deref(), Some("Demo Knight"));
    assert_eq!(character.equipment.len(), 1);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM fame_samples WHERE character_id='verified'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    let fight: FightView = serde_json::from_str(
        &db.query_row("SELECT json FROM fights", [], |r| r.get::<_, String>(0))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        fight.players[0].player_id, "demo-character",
        "combat identity is not joined by character aliases"
    );
}

#[test]
fn unsafe_numeric_values_and_zero_quantities_are_rejected_atomically() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let mut events = story_events();
    if let EngineEvent::FightStarted(fight) = &mut events[3].event {
        fight.players[0].damage = f64::NAN;
    }
    assert!(apply_batch(&mut db, &events).is_err());
    assert_eq!(common::receipts::count(&db), 0);
    let mut events = story_events();
    events[0].at = Timestamp(u64::MAX);
    assert!(apply_batch(&mut db, &events).is_err());
    let mut events = story_events();
    if let EngineEvent::BagDropped(drop) = &mut events[4].event {
        drop.contents[0].quantity = 0;
    }
    assert!(apply_batch(&mut db, &events).is_err());
}

#[test]
fn transport_events_only_receipt_and_alerts_are_typed_summaries() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    let alert = AlertView {
        details: None,
        presentation: None,
        id: "alert".into(),
        at: Timestamp(6_000_000),
        kind: AlertKind::WhiteBag,
        run_id: Some("demo-run".into()),
        explanation: "Observed synthetic bag".into(),
    };
    let capture = CaptureStatus {
        message: "synthetic-sensitive-transport-value".into(),
        ..CaptureStatus::default()
    };
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::CaptureStateChanged(capture)),
            envelope(
                9,
                EngineEvent::MapEntered {
                    realm_instance_key: None,
                    at: Timestamp(6_000_000),
                    name: "synthetic-map-value".into(),
                    kind: AreaKind::Realm,
                },
            ),
            envelope(10, EngineEvent::AlertFired(alert.clone())),
        ],
    )
    .unwrap();
    let persisted: AlertView = serde_json::from_str(
        &db.query_row("SELECT json FROM alerts", [], |r| r.get::<_, String>(0))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(persisted, alert);
    let digest = common::receipts::lookup(&db, "test-stream", 8).unwrap();
    assert_eq!(digest.len(), 32);
    assert!(!digest
        .windows(b"synthetic-sensitive".len())
        .any(|part| part == b"synthetic-sensitive"));
}

#[test]
fn real_sqlite_full_and_readonly_do_not_commit_a_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let pages: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
    db.pragma_update(None, "max_page_count", pages).unwrap();
    let mut event = story_events().remove(0);
    if let EngineEvent::SessionUpdated(s) = &mut event.event {
        s.id = "large-synthetic-session".repeat(10_000);
    }
    assert!(matches!(
        apply_batch(&mut db, &[event]),
        Err(StoreError::Full)
    ));
    assert_eq!(common::receipts::count(&db), 0);
    drop(db);
    let mut readonly =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert!(matches!(
        apply_batch(&mut readonly, &story_events()[..1]),
        Err(StoreError::Io)
    ));
}

#[test]
fn death_with_missing_run_is_invalid_and_rolls_back_the_batch() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    let death = |sequence, run_id| {
        envelope(
            sequence,
            EngineEvent::PlayerDied {
                at: Timestamp(7_000_000),
                character_id: None,
                run_id,
            },
        )
    };
    assert!(matches!(
        apply_batch(
            &mut db,
            &[death(8, None), death(9, Some("missing-run".into()))]
        ),
        Err(StoreError::InvalidEvent)
    ));
    assert_eq!(
        db.query_row("SELECT count(*) FROM deaths", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(common::receipts::head(&db, "test-stream").unwrap(), 7);
    assert_eq!(common::receipts::count(&db), 7);
    apply_batch(&mut db, &[death(8, Some("demo-run".into()))]).unwrap();
    assert_eq!(
        db.query_row("SELECT session_id FROM deaths", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "demo-session"
    );
}
