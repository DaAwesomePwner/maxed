mod common;
use common::*;
use maxed_store::*;
use maxed_types::*;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

const BACKFILL: &str = include_str!("../migrations/006_summary_target_kinds.sql");

fn legacy(path: &std::path::Path) -> Connection {
    let mut db = Connection::open(path).unwrap();
    for (index, script) in [
        include_str!("../migrations/001_initial.sql"),
        include_str!("../migrations/002_fight_summaries.sql"),
        include_str!("../migrations/003_loot_and_sessions.sql"),
        include_str!("../migrations/004_alerts.sql"),
        include_str!("../migrations/005_instance.sql"),
    ]
    .iter()
    .enumerate()
    {
        db.execute_batch(script).unwrap();
        db.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2)",
            params![
                index as i64 + 1,
                format!("{:x}", Sha256::digest(script.as_bytes()))
            ],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", 5).unwrap();
    // This legacy fight fixture predates the current drop writer.
    let events: Vec<_> = story_events()
        .into_iter()
        .filter(|e| !matches!(e.event, EngineEvent::BagDropped(_)))
        .enumerate()
        .map(|(i, e)| envelope(i as u64 + 1, e.event))
        .collect();
    // The current projector writes map visits; drop the scratch table so the stored schema stays v5.
    db.execute_batch(
        "CREATE TABLE map_visits(
    id TEXT PRIMARY KEY,
    stream_id TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES play_sessions(id),
    character_id TEXT REFERENCES characters(id),
    run_id TEXT REFERENCES runs(id),
    map TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('nexus','realm','dungeon','other_hub','unknown')),
    realm_instance_key TEXT,
    started INTEGER NOT NULL CHECK(started>=0),
    ended INTEGER CHECK(ended>=started),
    source TEXT NOT NULL CHECK(source IN ('live','backfill')),
    json TEXT NOT NULL
);",
    )
    .unwrap();
    apply_batch(&mut db, &events).unwrap();
    db.execute_batch("DROP TABLE map_visits").unwrap();
    db.execute("UPDATE fights SET boss_type=NULL,json=json_set(json,'$.boss_type',NULL,'$.target_type',2227,'$.target_kind','miniboss')", []).unwrap();
    db.execute("UPDATE fight_summaries SET json=json_remove(json_set(json,'$.boss_type',NULL,'$.target_type',2227),'$.target_kind')", []).unwrap();
    db
}

fn summary_json(db: &Connection) -> String {
    db.query_row(
        "SELECT json FROM fight_summaries WHERE fight_id='demo-fight'",
        [],
        |row| row.get(0),
    )
    .unwrap()
}

#[test]
fn v5_miniboss_is_backfilled_once_and_story_stays_summary_only() {
    assert!(
        !BACKFILL.contains('\r'),
        "migration checksums require LF bytes"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v5.sqlite");
    let db = legacy(&path);
    let before: serde_json::Value = serde_json::from_str(&summary_json(&db)).unwrap();
    assert!(before.get("target_kind").is_none());
    assert!(matches!(open_reader(&path), Err(StoreError::Migration)));
    upgrade_history(&path).unwrap();
    let mut expected = before;
    expected["target_kind"] = serde_json::json!("miniboss");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&summary_json(&db)).unwrap(),
        expected
    );
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        db.query_row(
            "SELECT checksum FROM schema_migrations WHERE version=6",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        format!("{:x}", Sha256::digest(BACKFILL.as_bytes()))
    );
    let saved = summary_json(&db);
    upgrade_history(&path).unwrap();
    assert_eq!(summary_json(&db), saved);
    db.execute_batch(BACKFILL).unwrap();
    assert_eq!(db.changes(), 0);
    // A backfilled story must no longer depend on the full fight record.
    db.execute("UPDATE fights SET json='broken'", []).unwrap();
    let reader = open_reader(&path).unwrap();
    let fight = reader
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .fights
        .remove(0);
    assert_eq!(fight.target_kind, Some(TargetKind::Miniboss));
    assert_eq!(fight.target_type, Some(2227));
    assert_eq!(fight.boss_type, None);
    assert!(reader.get_fight("demo-fight").is_err());
}

#[test]
fn v6_preserves_missing_null_corrupt_and_existing_classifications() {
    for (detail, summary_kind) in [
        ("{}", None),
        (r#"{"target_kind":null}"#, None),
        ("broken", None),
        (r#"{"target_kind":"miniboss"}"#, Some("null")),
        (r#"{"target_kind":"miniboss"}"#, Some(r#""quest""#)),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v5.sqlite");
        let mut db = legacy(&path);
        db.execute("UPDATE fights SET json=?1", [detail]).unwrap();
        if let Some(kind) = summary_kind {
            db.execute(
                "UPDATE fight_summaries SET json=json_set(json,'$.target_kind',json(?1))",
                [kind],
            )
            .unwrap();
        }
        let before = summary_json(&db);
        migrate(&mut db).unwrap();
        assert_eq!(summary_json(&db), before);
        if summary_kind.is_none() {
            assert!(serde_json::from_str::<serde_json::Value>(&before)
                .unwrap()
                .get("target_kind")
                .is_none());
        }
    }
}

#[test]
fn v6_failure_rolls_back_backfill_and_version_then_retries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v5.sqlite");
    let mut db = legacy(&path);
    let before = summary_json(&db);
    db.execute_batch("CREATE TRIGGER fail_backfill BEFORE UPDATE ON fight_summaries BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(matches!(migrate(&mut db), Err(StoreError::Migration)));
    assert_eq!(summary_json(&db), before);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM schema_migrations WHERE version=6",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute_batch("DROP TRIGGER fail_backfill").unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT json_extract(json,'$.target_kind') FROM fight_summaries",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "miniboss"
    );
}

#[test]
fn v6_backfills_large_history_in_one_update() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v5.sqlite");
    let mut db = legacy(&path);
    db.execute_batch("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<10000)
        INSERT INTO fights SELECT 'extra-'||i,run_id,session_id,started,ended,boss_type,local_dps,complete_capture,json
        FROM fights CROSS JOIN n WHERE id='demo-fight';
        INSERT INTO fight_summaries SELECT f.id,s.run_id,s.started,json_set(s.json,'$.id',f.id)
        FROM fights f CROSS JOIN fight_summaries s WHERE f.id<>'demo-fight' AND s.fight_id='demo-fight';").unwrap();
    assert_eq!(BACKFILL.matches("UPDATE ").count(), 1);
    migrate(&mut db).unwrap();
    assert_eq!(db.query_row("SELECT count(*) FROM fight_summaries WHERE json_extract(json,'$.target_kind')='miniboss'", [], |r| r.get::<_, i64>(0)).unwrap(), 10001);
}
