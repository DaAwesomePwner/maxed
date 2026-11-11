use crate::common::{self, installed_schema::*};
use maxed_store::{migrate, open_reader, upgrade_history, StoreError};
use rusqlite::{params, types::Value, Connection};
use std::collections::BTreeSet;

const NEW_TABLES: [&str; 10] = [
    "crucible_definitions",
    "maintenance_jobs",
    "backup_generations",
    "event_streams",
    "event_receipts_v2",
    "accounts",
    "current_account",
    "observation_characters",
    "identity_spans",
    "account_recovery",
];

fn fixture(db: &mut Connection, version: usize) {
    install(db, version);
    assert_installed(db, version);
    common::apply_legacy_story(db, &common::story_events());
    if version >= 14 {
        let profile: maxed_types::PlayerProfileView = serde_json::from_value(serde_json::json!({
            "scope_id": "demo-run", "run_id": "demo-run", "player_id": "synthetic-player",
            "object_id": 101, "is_local": false, "stats": [], "equipment": [],
            "first_seen": maxed_types::Timestamp(100), "last_seen": maxed_types::Timestamp(200)
        }))
        .unwrap();
        db.execute("INSERT INTO player_profiles VALUES('demo-run','synthetic-player',101,0,'demo-run',100,200,?1)",
            [serde_json::to_string(&profile).unwrap()]).unwrap();
        db.execute_batch(
            "INSERT INTO party_players(key,name,first_seen,last_seen,mark,note,updated)
            VALUES('synthetic-player','Synthetic Player',100,200,'watched','Synthetic note',200);",
        )
        .unwrap();
    }
    assert_integrity(db);
}

fn assert_empty_additions(db: &Connection) {
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account_id IS NOT NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    for table in NEW_TABLES {
        assert_eq!(
            db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn exact_v12_v14_and_v15_upgrade_preserves_history_and_adds_empty_maintenance_schema() {
    for version in [12, 14, 15] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        let mut db = Connection::open(&path).unwrap();
        fixture(&mut db, version);
        let objects = schema(&db);
        let saved = history(&db);
        let original_checksums = checksums(&db);
        let (result, trace) = trace_sql(&mut db, migrate);
        result.unwrap();
        assert_eq!(user_version(&db), maxed_store::SCHEMA_VERSION);
        assert_eq!(maxed_store::SCHEMA_VERSION, 20);
        let attributed = assert_additive_upgrade(&trace, version, 20, &objects);
        if version == 12 {
            assert!(attributed[&13].iter().all(|sql| sql.starts_with("CREATE ")));
            assert!(attributed[&14]
                .iter()
                .any(|sql| sql.starts_with("INSERT INTO PARTY_RUN_MEMBERS")));
            assert!(attributed[&14]
                .iter()
                .any(|sql| sql.starts_with("INSERT INTO PARTY_PLAYERS")));
        }
        assert_eq!(
            attributed[&16]
                .iter()
                .filter(|sql| sql.starts_with("CREATE TABLE "))
                .count(),
            4
        );
        let upgraded_objects = schema(&db);
        if version >= 14 {
            let mut fresh = Connection::open_in_memory().unwrap();
            migrate(&mut fresh).unwrap();
            assert_eq!(upgraded_objects, schema(&fresh));
        }
        assert!(
            objects
                .iter()
                .all(|object| account_schema_preserves(object, &upgraded_objects)),
            "installed objects changed"
        );
        let upgraded = history(&db);
        assert!(
            saved
                .iter()
                .all(|(table, rows)| upgraded.get(table) == Some(rows)),
            "installed history changed"
        );
        assert_eq!(&checksums(&db)[..version], original_checksums);
        let script = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/016_maintenance_receipts.sql"
        ))
        .unwrap();
        assert!(
            !script.contains('\r'),
            "migration checksum requires LF bytes"
        );
        assert_eq!(checksums(&db)[15], (16, hash(&script)));
        assert_empty_additions(&db);
        assert_integrity(&db);
        let changes = db.total_changes();
        migrate(&mut db).unwrap();
        assert_eq!(db.total_changes(), changes);
        assert_eq!(schema(&db), upgraded_objects);
        assert!(
            history(&db) == upgraded,
            "idempotent migration changed history"
        );
        drop(db);
        assert_eq!(
            open_reader(&path).unwrap().user_version().unwrap(),
            maxed_store::SCHEMA_VERSION
        );
    }
}

#[test]
fn history_entry_point_upgrades_exact_installs_without_changing_history() {
    for version in [12, 14, 15] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        let mut db = Connection::open(&path).unwrap();
        fixture(&mut db, version);
        let saved = history(&db);
        drop(db);
        upgrade_history(&path).unwrap();
        upgrade_history(&path).unwrap();
        let db = Connection::open(&path).unwrap();
        assert_eq!(user_version(&db), maxed_store::SCHEMA_VERSION);
        assert_empty_additions(&db);
        let upgraded = history(&db);
        assert!(saved
            .iter()
            .all(|(table, rows)| upgraded.get(table) == Some(rows)));
        assert_integrity(&db);
        let story = open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap();
        assert_eq!(story.fights.len(), 1);
        assert_eq!(story.drops.len(), 1);
    }
}

#[test]
fn late_ddl_fault_rolls_back_all_upgrade_work() {
    for version in [12, 14, 15] {
        let mut db = Connection::open_in_memory().unwrap();
        fixture(&mut db, version);
        // Fail the last CREATE in 016 after earlier scripts and DDL have run.
        db.execute_batch("CREATE TABLE event_receipts_v2(sentinel TEXT);")
            .unwrap();
        let objects = schema(&db);
        let saved = history(&db);
        let sums = checksums(&db);
        let (result, trace) = trace_sql(&mut db, migrate);
        assert!(matches!(result, Err(StoreError::Migration)));
        assert!(
            trace
                .iter()
                .any(|sql| sql.contains("CREATE TABLE event_streams")),
            "fault must follow successful 016 DDL"
        );
        assert_eq!(user_version(&db), version as i64);
        assert_eq!(schema(&db), objects);
        assert_eq!(checksums(&db), sums);
        assert!(history(&db) == saved);
        assert_integrity(&db);
        db.execute_batch("DROP TABLE event_receipts_v2").unwrap();
        migrate(&mut db).unwrap();
        assert_eq!(user_version(&db), maxed_store::SCHEMA_VERSION);
    }
}

#[test]
fn installed_registry_rejects_unknown_future_and_tampered_versions() {
    for version in [12, 14, 15] {
        for mutation in [
            "PRAGMA user_version=21",
            "PRAGMA user_version=0",
            "PRAGMA user_version=-1",
            "DELETE FROM schema_migrations WHERE version=12",
            "UPDATE schema_migrations SET checksum='tampered' WHERE version=12",
            "INSERT INTO schema_migrations VALUES(99,'unknown')",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("history.sqlite3");
            let mut db = Connection::open(&path).unwrap();
            fixture(&mut db, version);
            db.execute_batch(mutation).unwrap();
            let objects = schema(&db);
            let saved = history(&db);
            let sums = checksums(&db);
            let before_version = user_version(&db);
            let result = migrate(&mut db);
            if before_version == 21 {
                assert!(matches!(result, Err(StoreError::NewerSchema)));
            } else {
                assert!(matches!(result, Err(StoreError::Migration)));
            }
            assert!(upgrade_history(&path).is_err());
            assert!(open_reader(&path).is_err());
            assert_eq!(user_version(&db), before_version);
            assert_eq!(checksums(&db), sums);
            assert_eq!(schema(&db), objects);
            assert!(history(&db) == saved);
        }
    }
}

#[test]
fn receipt_registry_and_binary_keys_enforce_the_storage_contract() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(user_version(&db), maxed_store::SCHEMA_VERSION);
    assert_empty_additions(&db);
    db.execute(
        "INSERT INTO event_streams(stream_id) VALUES('synthetic-stream')",
        [],
    )
    .unwrap();
    let key = db.last_insert_rowid();
    let defaults: String = db.query_row(
        "SELECT json_array(durable_sequence,lifecycle,final_sequence,sealed_at,receipt_floor,owner_kind,epoch) FROM event_streams",
        [], |r| r.get(0)).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&defaults).unwrap(),
        serde_json::json!([null, "resumable", null, null, 0, "unknown", 0])
    );
    assert!(db
        .execute(
            "INSERT INTO event_streams(stream_id) VALUES('synthetic-stream')",
            []
        )
        .is_err());
    assert!(db
        .execute("INSERT INTO event_streams(stream_id) VALUES(NULL)", [])
        .is_err());
    assert!(db
        .execute("UPDATE event_streams SET durable_sequence=0", [])
        .is_err());
    let digest = vec![0x5a_u8; 32];
    db.execute(
        "INSERT INTO event_receipts_v2 VALUES(?1,1,?2)",
        params![key, digest],
    )
    .unwrap();
    for (stream, sequence, digest) in [
        (
            Value::Integer(key),
            Value::Integer(1),
            Value::Blob(digest.clone()),
        ),
        (
            Value::Integer(key + 1),
            Value::Integer(2),
            Value::Blob(digest.clone()),
        ),
        (Value::Null, Value::Integer(2), Value::Blob(digest.clone())),
        (
            Value::Integer(key),
            Value::Null,
            Value::Blob(digest.clone()),
        ),
        (
            Value::Integer(key),
            Value::Integer(0),
            Value::Blob(digest.clone()),
        ),
        (
            Value::Integer(key),
            Value::Integer(-1),
            Value::Blob(digest.clone()),
        ),
        (Value::Integer(key), Value::Integer(2), Value::Null),
        (
            Value::Integer(key),
            Value::Integer(2),
            Value::Text("a".repeat(32)),
        ),
        (
            Value::Integer(key),
            Value::Integer(2),
            Value::Text("a".repeat(64)),
        ),
        (
            Value::Integer(key),
            Value::Integer(2),
            Value::Blob(vec![0; 31]),
        ),
        (
            Value::Integer(key),
            Value::Integer(2),
            Value::Blob(vec![0; 33]),
        ),
    ] {
        assert!(db
            .execute(
                "INSERT INTO event_receipts_v2 VALUES(?1,?2,?3)",
                params![stream, sequence, digest]
            )
            .is_err());
    }
    assert!(db.execute("DELETE FROM event_streams", []).is_err());
    let stored: Vec<u8> = db
        .query_row("SELECT digest FROM event_receipts_v2", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, digest);
    assert_eq!(
        db.query_row(
            "SELECT wr FROM pragma_table_list WHERE name='event_receipts_v2'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let pk: Vec<String> = db
        .prepare("SELECT name FROM pragma_table_info('event_receipts_v2') WHERE pk>0 ORDER BY pk")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(pk, ["stream_key", "sequence"]);
    assert_integrity(&db);
}

#[test]
fn jobs_are_requested_explicitly_and_reference_managed_verified_backups() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(user_version(&db), maxed_store::SCHEMA_VERSION);
    assert_empty_additions(&db);
    db.execute_batch(
        "INSERT INTO maintenance_jobs(kind,job_key,format_version,input_generation,input_revision)
        VALUES('receipt_conversion','synthetic-stream',1,'synthetic-generation',7);",
    )
    .unwrap();
    assert!(db.execute("INSERT INTO maintenance_jobs(kind,job_key,format_version,input_generation,input_revision) VALUES('receipt_conversion','synthetic-stream',1,'synthetic-generation',7)", []).is_err());
    let initial: (String, Option<String>, Option<String>, Option<String>) = db
        .query_row(
            "SELECT phase,keyset_cursor,safe_error,backup_id FROM maintenance_jobs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(initial, ("awaiting_backup".into(), None, None, None));
    assert!(db
        .execute("UPDATE maintenance_jobs SET backup_id='missing'", [])
        .is_err());
    db.execute_batch("INSERT INTO backup_generations(backup_id,database_generation,verified_at) VALUES('managed-backup','synthetic-generation',100);
        UPDATE maintenance_jobs SET backup_id='managed-backup',keyset_cursor='42',phase='copying_receipts',safe_error='busy';").unwrap();
    assert!(db.execute("DELETE FROM backup_generations", []).is_err());
    let saved = history(&db);
    migrate(&mut db).unwrap();
    assert!(history(&db) == saved);
    assert_integrity(&db);
}

#[test]
fn schema_only_guard_rejects_scans_backfills_and_implicit_constraint_work() {
    let existing = BTreeSet::from(["EVENT_RECEIPTS".into()]);
    let mut created = BTreeSet::new();
    assert!(schema_only("CREATE TABLE fresh(id INTEGER PRIMARY KEY, parent INTEGER REFERENCES fresh(id), value BLOB CHECK(length(value)=32))", &existing, &mut created));
    assert!(schema_only(
        "CREATE INDEX fresh_parent ON fresh(parent)",
        &existing,
        &mut created
    ));
    for sql in [
        "SELECT * FROM event_receipts",
        "INSERT INTO fresh SELECT * FROM event_receipts",
        "UPDATE event_receipts SET digest='changed'",
        "DELETE FROM event_receipts",
        "CREATE INDEX old_receipts ON event_receipts(sequence)",
        "CREATE TABLE copied AS SELECT * FROM event_receipts",
        "CREATE TABLE event_receipts(id INTEGER)",
        "ALTER TABLE event_receipts ADD COLUMN checked INTEGER CHECK(checked>0)",
        "ALTER TABLE event_receipts ADD COLUMN parent INTEGER REFERENCES fresh(id)",
        "VACUUM",
        "PRAGMA writable_schema=ON",
    ] {
        assert!(
            !schema_only(sql, &existing, &mut created),
            "unsafe SQL accepted: {sql}"
        );
    }
}

#[test]
fn existing_writer_writes_only_v2_receipts() {
    for version in [12, 14, 15] {
        let mut db = Connection::open_in_memory().unwrap();
        install(&db, version);
        let mut events = common::story_events();
        // v12 predates Party's ended-run projector; keep this receipt fixture open.
        for event in &mut events {
            if let maxed_types::EngineEvent::RunStarted(run) = &mut event.event {
                run.ended = None;
            }
        }
        common::apply_legacy_batch(&mut db, &events);
        let saved = history(&db);
        migrate(&mut db).unwrap();
        assert_eq!(user_version(&db), maxed_store::SCHEMA_VERSION);
        let changes = db.total_changes();
        maxed_store::apply_batch(&mut db, &events).unwrap();
        assert_eq!(db.total_changes(), changes);
        let after_retry = history(&db);
        assert!(saved
            .iter()
            .all(|(table, rows)| after_retry.get(table) == Some(rows)));
        let mut conflict = events[0].clone();
        conflict.at = maxed_types::Timestamp(1);
        assert!(maxed_store::apply_batch(&mut db, &[conflict]).is_err());
        let mut next = events[0].clone();
        next.sequence = maxed_types::Sequence(maxed_types::Timestamp(events.len() as u64 + 2));
        assert!(maxed_store::apply_batch(&mut db, &[next.clone()]).is_err());
        next.sequence = maxed_types::Sequence(maxed_types::Timestamp(events.len() as u64 + 1));
        maxed_store::apply_batch(&mut db, &[next]).unwrap();
        assert_eq!(common::receipts::count(&db), events.len() as i64 + 1);
        assert_eq!(
            db.query_row("SELECT count(*) FROM event_receipts", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            events.len() as i64
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM event_receipts_v2", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            common::receipts::head(&db, "test-stream"),
            Some(events.len() as i64 + 1)
        );
        for table in ["maintenance_jobs", "backup_generations"] {
            assert_eq!(
                db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        assert_integrity(&db);
    }
}

#[test]
fn installed_015_and_016_checksums_reject_tampering_without_writes() {
    for version in [15, 16] {
        let mut db = Connection::open_in_memory().unwrap();
        install(&db, version);
        db.execute(
            "UPDATE schema_migrations SET checksum='tampered' WHERE version=?1",
            [version as i64],
        )
        .unwrap();
        let sums = checksums(&db);
        let objects = schema(&db);
        let changes = db.total_changes();
        assert!(matches!(migrate(&mut db), Err(StoreError::Migration)));
        assert_eq!(db.total_changes(), changes);
        assert_eq!(checksums(&db), sums);
        assert_eq!(schema(&db), objects);
        assert_eq!(user_version(&db), version as i64);
    }
}

#[test]
fn exact_v15_is_accepted_and_preserves_shipped_loot_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let db = Connection::open(&path).unwrap();
    install(&db, 15);
    assert_installed(&db, 15);
    let before = schema(&db);
    drop(db);
    assert!(
        maxed_store::history_preflight(&path).is_ok(),
        "shipped v15 must pass preflight"
    );
    upgrade_history(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(user_version(&db), maxed_store::SCHEMA_VERSION);
    let after = schema(&db);
    assert!(before
        .iter()
        .all(|object| account_schema_preserves(object, &after)));
    assert_empty_additions(&db);
}

#[test]
fn downgrade_helpers_preserve_exact_v15_then_remove_only_loot_indexes_for_v14() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    common::downgrade::remove_maintenance_schema(&db);
    db.execute_batch("DELETE FROM schema_migrations WHERE version=16; PRAGMA user_version=15;")
        .unwrap();
    assert_installed(&db, 15);
    common::downgrade::remove_loot_stats_indexes(&db);
    db.execute_batch("DELETE FROM schema_migrations WHERE version=15; PRAGMA user_version=14;")
        .unwrap();
    assert_installed(&db, 14);
}
