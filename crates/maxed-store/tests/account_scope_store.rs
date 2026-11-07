use crate::common::envelope;
use maxed_store::{accounts::*, apply_batch, migrate};
use maxed_types::*;
use rusqlite::Connection;

fn account(c: char) -> String {
    format!("account-{}", c.to_string().repeat(64))
}
fn character(id: &str, account_id: Option<String>) -> EngineEvent {
    let mut c = demo::demo_snapshot().character.unwrap();
    c.id = id.into();
    c.account_id = account_id;
    EngineEvent::CharacterUpdated(c)
}
fn identity(at: u64, id: Option<&str>, a: Option<String>) -> EngineEvent {
    EngineEvent::LocalIdentityChanged {
        at: Timestamp(at),
        character_id: id.map(Into::into),
        account_id: a,
    }
}
fn seed(db: &mut Connection, mut events: Vec<EngineEvent>) -> Vec<EventEnvelope> {
    if !matches!(events.first(), Some(EngineEvent::SessionUpdated(_))) {
        events.insert(
            0,
            EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
        );
    }
    let events: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(i, e)| envelope(i as u64 + 1, e))
        .collect();
    apply_batch(db, &events).unwrap();
    events
}
fn owner(db: &Connection, view: &str, id: &str) -> Option<String> {
    db.query_row(
        &format!("SELECT account_id FROM {view} WHERE id=?1"),
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn account_scope_interleaving_revisions_merge_conflict_and_replay() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let snap = demo::demo_snapshot();
    let mut f = snap.fights[0].clone();
    f.owner_character_id = Some("provisional".into());
    let mut d = snap.drops[0].clone();
    d.owner_character_id = Some("provisional".into());
    let mut revised_f = f.clone();
    revised_f.owner_character_id = Some("b-7".into());
    let mut revised_d = d.clone();
    revised_d.owner_character_id = Some("b-7".into());
    let mut run = snap.run.unwrap();
    run.character_id = Some("a-7".into());
    let events = seed(
        &mut db,
        vec![
            EngineEvent::SessionUpdated(snap.session.unwrap()),
            character("provisional", None),
            character("a-7", Some(account('a'))),
            character("b-7", Some(account('b'))),
            identity(10, Some("provisional"), None),
            EngineEvent::RunStarted(run),
            EngineEvent::FightStarted(f.clone()),
            EngineEvent::BagDropped(d.clone()),
            EngineEvent::CharacterMerged {
                provisional_id: "provisional".into(),
                verified_id: "a-7".into(),
            },
            identity(20, Some("a-7"), Some(account('a'))),
            identity(30, None, None),
            identity(40, Some("b-7"), Some(account('b'))),
            EngineEvent::FightUpdated(revised_f),
            EngineEvent::DropUpdated(revised_d),
            character("a-7", Some(account('b'))),
            identity(50, None, None),
        ],
    );
    assert_eq!(owner(&db, "character_accounts", "a-7"), Some(account('a')));
    assert_eq!(owner(&db, "character_accounts", "b-7"), Some(account('b')));
    assert_eq!(owner(&db, "fight_accounts", &f.id), Some(account('a')));
    assert_eq!(owner(&db, "drop_accounts", &d.id), Some(account('a')));
    assert_eq!(current_account(&db).unwrap(), Some(account('b')));
    assert_eq!(
        db.query_row(
            "SELECT account_id FROM identity_spans WHERE started=10",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        account('a')
    );
    assert_eq!(
        db.query_row(
            "SELECT sum(ended-started) FROM identity_spans WHERE account_id=?1",
            [account('a')],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        20
    );
    assert_eq!(
        db.query_row(
            "SELECT sum(ended-started) FROM identity_spans WHERE account_id=?1",
            [account('b')],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        10
    );
    apply_batch(&mut db, &events).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM identity_spans", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        5
    );
}

#[test]
fn account_scope_legacy_attach_exact_idempotent_and_progress() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    seed(
        &mut db,
        vec![character("a-7", None), character("b-7", Some(account('b')))],
    );
    let ids = vec!["a-7".into(), "a-7".into(), "b-7".into(), "7".into()];
    assert_eq!(
        attach_legacy_characters(&mut db, &account('a'), &ids).unwrap(),
        AttachCounts {
            attached: 1,
            already_attached: 0,
            conflicting: 1,
            unmatched: 1
        }
    );
    assert_eq!(
        attach_legacy_characters(&mut db, &account('a'), &ids)
            .unwrap()
            .already_attached,
        1
    );
    assert_eq!(current_account(&db).unwrap(), None);
    assert_eq!(recovery_progress(&db, &account('a')).unwrap(), None);
    record_recovery_progress(&mut db, &account('a'), 65535).unwrap();
    record_recovery_progress(&mut db, &account('a'), 10).unwrap();
    assert_eq!(recovery_progress(&db, &account('a')).unwrap(), Some(65535));
}

#[test]
fn account_scope_current_verified_only_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("account.sqlite");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    seed(
        &mut db,
        vec![
            character("a-7", Some(account('a'))),
            identity(10, Some("a-7"), None),
        ],
    );
    assert_eq!(current_account(&db).unwrap(), None);
    apply_batch(
        &mut db,
        &[
            envelope(4, identity(20, Some("a-7"), Some(account('a')))),
            envelope(5, identity(30, None, None)),
            envelope(6, identity(40, Some("pending"), None)),
        ],
    )
    .unwrap();
    drop(db);
    assert_eq!(
        maxed_store::open_reader(&path)
            .unwrap()
            .current_account()
            .unwrap(),
        Some(account('a'))
    );
}

#[test]
fn account_scope_migration_016_populated_remains_unknown() {
    use sha2::{Digest, Sha256};
    let mut db = Connection::open_in_memory().unwrap();
    // Reconstruct installed schemas, never downgrade a current-schema database.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut paths: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "sql"))
        .collect();
    paths.sort();
    for (i, path) in paths.iter().take(16).enumerate() {
        let sql = std::fs::read_to_string(path).unwrap();
        db.execute_batch(&sql).unwrap();
        db.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2)",
            rusqlite::params![
                i as i64 + 1,
                format!("{:x}", Sha256::digest(sql.as_bytes()))
            ],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", 16).unwrap();
    db.execute(
        "INSERT INTO characters(id,observed_at,json) VALUES('legacy',0,'{}')",
        [],
    )
    .unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(owner(&db, "character_accounts", "legacy"), None);
    assert_eq!(current_account(&db).unwrap(), None);
}

#[test]
fn account_scope_reader_contract_unknown_and_conflicting_death() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let snap = demo::demo_snapshot();
    let mut run = snap.run.unwrap();
    run.character_id = Some("a-7".into());
    let run_id = run.id.clone();
    let mut fight = snap.fights[0].clone();
    fight.owner_character_id = None;
    seed(
        &mut db,
        vec![
            EngineEvent::SessionUpdated(snap.session.unwrap()),
            character("a-7", Some(account('a'))),
            character("b-7", Some(account('b'))),
            EngineEvent::RunStarted(run),
            EngineEvent::FightStarted(fight.clone()),
        ],
    );
    assert_eq!(owner(&db, "fight_accounts", &fight.id), Some(account('a')));
    fight.owner_character_id = Some("b-7".into());
    apply_batch(
        &mut db,
        &[envelope(6, EngineEvent::FightUpdated(fight.clone()))],
    )
    .unwrap();
    assert_eq!(owner(&db, "fight_accounts", &fight.id), Some(account('a')));
    db.execute("INSERT INTO deaths(stream_id,sequence,character_id,run_id,at) VALUES('synthetic',1,'b-7',?1,100),('synthetic',2,NULL,?1,100)",[run_id]).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT account_id FROM death_accounts WHERE sequence=1",
            [],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        None
    );
    assert_eq!(
        db.query_row(
            "SELECT account_id FROM death_accounts WHERE sequence=2",
            [],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        Some(account('a'))
    );
    let bad = envelope(7, identity(100, Some("a-7"), Some(account('b'))));
    apply_batch(&mut db, &[bad]).unwrap();
    assert_eq!(current_account(&db).unwrap(), None);
}

fn session(id: &str, started: u64, ended: Option<u64>) -> EngineEvent {
    EngineEvent::SessionUpdated(PlaySessionView {
        id: id.into(),
        started: Timestamp(started),
        last_traffic: Timestamp(ended.unwrap_or(started)),
        ended: ended.map(Timestamp),
        issues: vec![],
    })
}
fn span_end(db: &Connection, stream: &str, sequence: i64) -> Option<i64> {
    db.query_row(
        "SELECT ended FROM identity_spans WHERE stream_id=?1 AND sequence=?2",
        rusqlite::params![stream, sequence],
        |r| r.get(0),
    )
    .unwrap()
}
fn other_stream(sequence: u64, event: EngineEvent) -> EventEnvelope {
    let mut e = envelope(sequence, event);
    e.stream_id = "other-stream".into();
    e
}

#[test]
fn account_scope_session_end_loss_and_new_session_close_only_matching_spans() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    seed(
        &mut db,
        vec![
            session("s1", 0, None),
            character("a", Some(account('a'))),
            identity(10, Some("a"), Some(account('a'))),
        ],
    );
    apply_batch(
        &mut db,
        &[
            other_stream(1, session("other", 0, None)),
            other_stream(2, identity(15, Some("a"), Some(account('a')))),
        ],
    )
    .unwrap();
    apply_batch(&mut db, &[envelope(4, session("s1", 0, None))]).unwrap();
    assert_eq!(span_end(&db, "test-stream", 3), None); // Same-session heartbeat is not a switch.
    apply_batch(&mut db, &[envelope(5, session("s1", 0, Some(30)))]).unwrap();
    assert_eq!(span_end(&db, "test-stream", 3), Some(30));
    assert_eq!(span_end(&db, "other-stream", 2), None); // End predicate is session-specific.
    apply_batch(&mut db, &[envelope(6, identity(31, None, None))]).unwrap();
    assert_eq!(span_end(&db, "test-stream", 6), None); // Engine sends loss after end.
    assert_eq!(db.query_row("SELECT session_id,account_id FROM identity_spans WHERE stream_id='test-stream' AND sequence=6",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?))).unwrap(),("s1".into(),None));
    apply_batch(&mut db, &[envelope(7, session("s2", 50, None))]).unwrap();
    assert_eq!(span_end(&db, "test-stream", 6), Some(50));
    assert_eq!(span_end(&db, "test-stream", 3), Some(30)); // Already closed span is immutable.
    assert_eq!(span_end(&db, "other-stream", 2), None); // Switch predicate is stream-specific.
    apply_batch(
        &mut db,
        &[
            envelope(8, identity(60, Some("a"), Some(account('a')))),
            envelope(9, session("s2", 50, Some(90))),
        ],
    )
    .unwrap();
    assert_eq!(span_end(&db, "test-stream", 8), Some(90));
    apply_batch(&mut db, &[envelope(10, session("s2", 50, Some(100)))]).unwrap();
    assert_eq!(span_end(&db, "test-stream", 8), Some(90)); // Revisions cannot reopen/rewrite closed spans.
    assert_eq!(span_end(&db, "test-stream", 3), Some(30));
    assert_eq!(span_end(&db, "test-stream", 6), Some(50));
}

#[test]
fn account_scope_reconnect_new_stream_does_not_inherit_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    seed(
        &mut db,
        vec![
            session("s1", 0, None),
            character("a", Some(account('a'))),
            identity(10, Some("a"), Some(account('a'))),
            session("s1", 0, Some(30)),
            identity(31, None, None),
        ],
    );
    drop(db);
    let mut db = Connection::open(&path).unwrap();
    apply_batch(
        &mut db,
        &[
            other_stream(1, session("s2", 50, None)),
            other_stream(2, identity(60, Some("pending"), None)),
        ],
    )
    .unwrap();
    assert_eq!(span_end(&db, "test-stream", 3), Some(30));
    assert_eq!(span_end(&db, "test-stream", 5), None); // A new stream never extends old elapsed time.
    assert_eq!(current_account(&db).unwrap(), Some(account('a')));
    assert_eq!(
        db.query_row(
            "SELECT account_id FROM identity_spans WHERE stream_id='other-stream'",
            [],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        None
    );
    apply_batch(&mut db, &[other_stream(3, session("s2", 50, Some(80)))]).unwrap();
    assert_eq!(span_end(&db, "other-stream", 2), Some(80));
    assert_eq!(span_end(&db, "test-stream", 5), None);
}

#[test]
fn account_scope_stale_character_fills_membership_without_replacing_snapshot() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let EngineEvent::CharacterUpdated(mut recent) = character("a", None) else {
        unreachable!()
    };
    recent.observed_at = Timestamp(200);
    recent.name = Some("Synthetic Recent".into());
    let mut stale = recent.clone();
    stale.observed_at = Timestamp(100);
    stale.name = Some("Synthetic Old".into());
    stale.account_id = Some(account('a'));
    seed(
        &mut db,
        vec![
            session("s1", 0, None),
            EngineEvent::CharacterUpdated(recent.clone()),
            EngineEvent::CharacterUpdated(stale.clone()),
        ],
    );
    stale.account_id = Some(account('b'));
    apply_batch(
        &mut db,
        &[envelope(4, EngineEvent::CharacterUpdated(stale))],
    )
    .unwrap();
    let (at, json): (i64, String) = db
        .query_row(
            "SELECT observed_at,json FROM characters WHERE id='a'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    recent.account_id = Some(account('a'));
    assert_eq!(at, 200);
    assert_eq!(
        serde_json::from_str::<CharacterView>(&json).unwrap(),
        recent
    );
    assert_eq!(owner(&db, "character_accounts", "a"), Some(account('a')));
}

#[test]
fn account_scope_writer_conflict_continues_and_recovery_operations_flush() {
    use maxed_store::{start_writer, WriterConfig};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let writer = start_writer(
        path.clone(),
        WriterConfig {
            stream_id: "test-stream".into(),
            ..WriterConfig::default()
        },
    )
    .unwrap();
    for (i, e) in [
        session("s1", 0, None),
        character("a", Some(account('a'))),
        character("legacy", None),
        identity(10, Some("a"), Some(account('a'))),
        identity(20, Some("a"), Some(account('b'))),
    ]
    .into_iter()
    .enumerate()
    {
        crate::common::submit(&writer, envelope(i as u64 + 1, e));
    }
    writer.flush().unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(current_account(&db).unwrap(), Some(account('a')));
    assert_eq!(owner(&db, "character_accounts", "a"), Some(account('a')));
    assert_eq!(
        db.query_row(
            "SELECT character_id,account_id FROM identity_spans WHERE sequence=5",
            [],
            |r| Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?
            ))
        )
        .unwrap(),
        (None, None)
    );
    // Later live events remain accepted, and attach itself flushes this character.
    crate::common::submit(&writer, envelope(6, character("late", None)));
    let ids = vec!["legacy".into(), "late".into(), "a".into(), "missing".into()];
    assert_eq!(
        writer
            .attach_legacy_characters(&account('b'), &ids)
            .unwrap(),
        AttachCounts {
            attached: 2,
            already_attached: 0,
            conflicting: 1,
            unmatched: 1
        }
    );
    assert_eq!(
        writer
            .attach_legacy_characters(&account('b'), &ids)
            .unwrap()
            .already_attached,
        2
    );
    crate::common::submit(&writer, envelope(7, identity(30, None, None)));
    writer
        .record_recovery_progress(&account('b'), 65535)
        .unwrap();
    writer.record_recovery_progress(&account('b'), 10).unwrap();
    assert_eq!(writer.status().state, StoreState::Ready);
    assert_eq!(span_end(&db, "test-stream", 5), Some(30));
    assert_eq!(current_account(&db).unwrap(), Some(account('a')));
    writer.shutdown().unwrap();
    drop(db);
    let db = Connection::open(&path).unwrap();
    assert_eq!(recovery_progress(&db, &account('b')).unwrap(), Some(65535));
    assert_eq!(owner(&db, "character_accounts", "late"), Some(account('b')));
}

#[test]
fn account_scope_chained_merges_flatten_owners_spans_and_accounts() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let snap = demo::demo_snapshot();
    let mut f = snap.fights[0].clone();
    f.run_id = None;
    f.owner_character_id = Some("a".into());
    let mut d = snap.drops[0].clone();
    d.run_id = None;
    d.owner_character_id = Some("a".into());
    let events = seed(
        &mut db,
        vec![
            EngineEvent::SessionUpdated(snap.session.unwrap()),
            character("a", None),
            character("b", None),
            character("c", Some(account('c'))),
            identity(10, Some("a"), None),
            EngineEvent::FightStarted(f.clone()),
            EngineEvent::BagDropped(d.clone()),
            EngineEvent::CharacterMerged {
                provisional_id: "a".into(),
                verified_id: "b".into(),
            },
            EngineEvent::CharacterMerged {
                provisional_id: "b".into(),
                verified_id: "c".into(),
            },
        ],
    );
    for id in ["a", "b", "c"] {
        assert_eq!(
            db.query_row(
                "SELECT canonical_id,account_id FROM character_accounts WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            )
            .unwrap(),
            ("c".into(), Some(account('c')))
        );
    }
    assert_eq!(owner(&db, "fight_accounts", &f.id), Some(account('c')));
    assert_eq!(owner(&db, "drop_accounts", &d.id), Some(account('c')));
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM observation_characters WHERE character_id='c'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT character_id,account_id FROM identity_spans",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        ("c".into(), account('c'))
    );
    apply_batch(&mut db, &events).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM identity_spans", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn account_scope_regressed_session_boundaries_clamp_identity_span_ends() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    seed(
        &mut db,
        vec![
            session("s1", 0, None),
            character("a", Some(account('a'))),
            identity(100, Some("a"), Some(account('a'))),
        ],
    );
    // A regressed session end must not produce a negative identity interval.
    apply_batch(&mut db, &[envelope(4, session("s1", 0, Some(50)))]).unwrap();
    assert_eq!(span_end(&db, "test-stream", 3), Some(100));
    // Loss can arrive after session end. Switching sessions closes that span,
    // even when the next session's start regresses behind the loss timestamp.
    apply_batch(
        &mut db,
        &[
            envelope(5, identity(200, None, None)),
            envelope(6, session("s2", 150, None)),
        ],
    )
    .unwrap();
    assert_eq!(span_end(&db, "test-stream", 5), Some(200));
    assert_eq!(span_end(&db, "test-stream", 3), Some(100));
}

#[test]
fn account_scope_regressed_identity_change_clamps_open_span() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    seed(
        &mut db,
        vec![
            character("self", Some(account('a'))),
            identity(100, Some("self"), Some(account('a'))),
            identity(50, None, None),
        ],
    );
    let end: i64 = db
        .query_row(
            "SELECT ended FROM identity_spans WHERE started=100",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(end, 100);
}
