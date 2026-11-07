use crate::common;
use common::*;
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;

fn config() -> WriterConfig {
    WriterConfig {
        stream_id: "test-stream".into(),
        ..WriterConfig::default()
    }
}

#[test]
fn recovery_does_not_shorten_a_closed_session_with_an_interrupted_run() {
    for ended in [4_000_000, 10_000_000] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut db = Connection::open(&path).unwrap();
        migrate(&mut db).unwrap();
        let snapshot = demo::demo_snapshot();
        let mut session = snapshot.session.unwrap();
        session.ended = Some(Timestamp(ended));
        apply_batch(
            &mut db,
            &[
                envelope(1, EngineEvent::SessionUpdated(session)),
                envelope(
                    2,
                    EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
                ),
                envelope(3, EngineEvent::RunStarted(snapshot.run.unwrap())),
            ],
        )
        .unwrap();
        start_writer(path, config()).unwrap().shutdown().unwrap();
        let (scalar, json): (i64, String) = db
            .query_row(
                "SELECT ended,json FROM play_sessions WHERE id='demo-session'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let session: PlaySessionView = serde_json::from_str(&json).unwrap();
        assert_eq!(scalar as u64, ended.max(5_000_000));
        assert_eq!(session.ended, Some(Timestamp(ended.max(5_000_000))));
    }
}

#[test]
fn flush_reopen_resume_and_invalid_events_keep_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let writer = start_writer(path.clone(), config()).unwrap();
    assert_eq!(writer.flush().unwrap().sequence.0 .0, 0);
    let events = story_events();
    for event in &events {
        submit(&writer, event.clone());
    }
    assert_eq!(writer.flush().unwrap().sequence.0 .0, 7);
    assert_eq!(writer.status().durable.unwrap().0 .0, 7);
    writer.shutdown().unwrap();
    let writer = start_writer(path.clone(), config()).unwrap();
    assert_eq!(writer.status().durable.unwrap().0 .0, 7);
    for event in &events {
        submit(&writer, event.clone());
    }
    assert_eq!(writer.flush().unwrap().sequence.0 .0, 7);
    let mut wrong = events[0].clone();
    wrong.stream_id = "other".into();
    assert!(matches!(writer.try_submit(wrong.clone()),Err(SubmitError::Invalid(e)) if e==wrong));
    let mut huge = events[0].clone();
    if let EngineEvent::SessionUpdated(s) = &mut huge.event {
        s.id = "x".repeat(8 * 1024 * 1024);
    }
    assert!(matches!(
        writer.try_submit(huge),
        Err(SubmitError::Invalid(_))
    ));
    writer.shutdown().unwrap();
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap()
            .fights
            .len(),
        1
    );
}

#[test]
fn busy_batch_is_retained_retry_is_ordered_and_full_returns_event() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let writer = start_writer(
        path.clone(),
        WriterConfig {
            capacity: 1,
            batch_size: 1,
            ..config()
        },
    )
    .unwrap();
    let lock = Connection::open(&path).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let event = story_events().remove(0);
    submit(&writer, event.clone());
    assert!(matches!(writer.try_submit(event.clone()),Err(SubmitError::Full(e)) if e==event));
    assert!(writer.flush().is_err());
    assert_eq!(writer.status().state, StoreState::Failed);
    assert_eq!(writer.status().queued, 1);
    assert!(writer.status().durable.is_none());
    // Contention can return Full before the worker failure is checked.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut pending = event.clone();
    let result = loop {
        match writer.try_submit(pending) {
            Err(SubmitError::Full(returned)) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "waiting for Failed timed out after 10s on Full: {returned:?}"
                );
                pending = returned;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            result => break result,
        }
    };
    assert!(matches!(result,Err(SubmitError::Failed(e)) if e==event));
    lock.execute_batch("ROLLBACK").unwrap();
    writer.retry().unwrap();
    assert_eq!(writer.flush().unwrap().sequence.0 .0, 1);
    writer.shutdown().unwrap();
}

#[test]
fn failed_shutdown_returns_all_unsaved_events_to_the_caller() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let writer = start_writer(path.clone(), config()).unwrap();
    let lock = Connection::open(&path).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let event = story_events().remove(0);
    submit(&writer, event.clone());
    assert!(writer.flush().is_err());
    match writer.shutdown().unwrap_err() {
        StoreError::WorkerFailed { pending } => assert_eq!(pending, vec![event]),
        e => panic!("wrong error: {e}"),
    }
    lock.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn reopening_interrupted_run_preserves_completion_and_never_invents_a_death() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let writer = start_writer(path.clone(), config()).unwrap();
    let snapshot = demo::demo_snapshot();
    for (i, event) in [
        EngineEvent::SessionUpdated(snapshot.session.unwrap()),
        EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
        EngineEvent::RunStarted(snapshot.run.unwrap()),
        EngineEvent::FightStarted(snapshot.fights[0].clone()),
    ]
    .into_iter()
    .enumerate()
    {
        submit(&writer, envelope(i as u64 + 1, event));
    }
    writer.shutdown().unwrap();
    let writer = start_writer(path.clone(), config()).unwrap();
    writer.shutdown().unwrap();
    let story = open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap();
    assert_eq!(story.run.outcome, Some(Outcome::Cleared));
    assert!(story
        .run
        .evidence
        .iter()
        .any(|e| e.evidence == Evidence::AppClosed));
    assert!(story.run.ended.is_some());
    assert!(!story.run.complete_capture);
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .get_fight(&story.fights[0].id)
            .unwrap()
            .unwrap()
            .end_reason,
        Some(FightEnd::AppClosed)
    );
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM deaths", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    let heads: i64 = common::receipts::heads(&db).len() as i64;
    start_writer(path, config()).unwrap().shutdown().unwrap();
    assert_eq!(common::receipts::heads(&db).len() as i64, heads);
}

#[test]
fn duplicate_writer_cannot_recover_an_active_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let writer = start_writer(path.clone(), config()).unwrap();
    let snapshot = demo::demo_snapshot();
    submit(
        &writer,
        envelope(1, EngineEvent::SessionUpdated(snapshot.session.unwrap())),
    );
    submit(
        &writer,
        envelope(
            2,
            EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
        ),
    );
    submit(
        &writer,
        envelope(3, EngineEvent::RunStarted(snapshot.run.unwrap())),
    );
    writer.flush().unwrap();
    assert!(matches!(
        start_writer(path.clone(), config()),
        Err(StoreError::Busy)
    ));
    assert!(open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .run
        .ended
        .is_none());
    writer.shutdown().unwrap();
}

#[test]
fn old_summaries_are_retained_and_unknown_interruptions_stay_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let writer = start_writer(path.clone(), config()).unwrap();
    for event in story_events() {
        submit(&writer, event);
    }
    let mut open_run = demo::demo_snapshot().run.unwrap();
    open_run.id = "interrupted".into();
    open_run.outcome = None;
    open_run.completed_at = None;
    open_run.evidence.clear();
    submit(&writer, envelope(8, EngineEvent::RunStarted(open_run)));
    writer.shutdown().unwrap();
    let db = Connection::open(&path).unwrap();
    let tables = [
        "characters",
        "runs",
        "run_evidence",
        "fights",
        "fight_players",
        "damage_sources",
        "drops",
        "drop_items",
        "fame_samples",
    ];
    let before: Vec<i64> = tables
        .iter()
        .map(|table| {
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect();
    start_writer(path.clone(), config())
        .unwrap()
        .shutdown()
        .unwrap();
    let after: Vec<i64> = tables
        .iter()
        .map(|table| {
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect();
    for (i, table) in tables.iter().enumerate() {
        if *table == "run_evidence" {
            assert!(after[i] > before[i]);
        } else {
            assert_eq!(before[i], after[i], "{table}");
        }
    }
    let story = open_reader(&path)
        .unwrap()
        .get_run("interrupted")
        .unwrap()
        .unwrap();
    assert_eq!(story.run.outcome, Some(Outcome::Unknown));
    assert!(story.run.completed_at.is_none());
    assert!(story
        .run
        .evidence
        .iter()
        .any(|e| e.evidence == Evidence::AppClosed));
}
