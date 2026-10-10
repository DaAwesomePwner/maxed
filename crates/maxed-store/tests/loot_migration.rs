mod common;
use maxed_types::*;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

#[test]
fn fresh_database_has_loot_indexes() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let v: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 12);
    assert_loot_indexes(&db);
}

fn assert_loot_indexes(db: &Connection) {
    for (index, columns) in [
        ("drops_source", vec!["source_type", "at", "id"]),
        ("runs_dungeon_ended", vec!["dungeon", "ended", "id"]),
        ("drops_id_at", vec!["id", "at"]),
        (
            "drops_stats",
            vec!["at", "id", "run_id", "bag_rank", "source_type"],
        ),
    ] {
        let mut statement = db
            .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
            .unwrap();
        let actual: Vec<String> = statement
            .query_map([index], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(actual, columns, "{index}");
    }
}

#[test]
fn v7_database_upgrades_and_backfills_source_type() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v7");
    let mut db = Connection::open(&path).unwrap();
    let scripts = [
        include_str!("../migrations/001_initial.sql"),
        include_str!("../migrations/002_fight_summaries.sql"),
        include_str!("../migrations/003_loot_and_sessions.sql"),
        include_str!("../migrations/004_alerts.sql"),
        include_str!("../migrations/005_instance.sql"),
        include_str!("../migrations/006_summary_target_kinds.sql"),
        include_str!("../migrations/007_pb_lookup_indexes.sql"),
    ];
    for (index, script) in scripts.iter().enumerate() {
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
    db.pragma_update(None, "user_version", 7).unwrap();
    db.execute("INSERT INTO play_sessions VALUES('s',1,2,NULL,'{}')", [])
        .unwrap();
    let cases = [
        ("known", r#"{"source_enemy":1234}"#, Some(1234)),
        ("unknown", r#"{"source_enemy":null}"#, None),
        ("missing", "{}", None),
        ("fractional", r#"{"source_enemy":12.5}"#, None),
        ("string", r#"{"source_enemy":"1234"}"#, None),
        ("too_large", r#"{"source_enemy":70000}"#, None),
        ("negative", r#"{"source_enemy":-1}"#, None),
        ("malformed", "broken", None),
    ];
    // The v7 drops table has no JSON-validity constraint, so malformed JSON can be stored.
    for (id, json, _) in cases {
        db.execute(
            "INSERT INTO drops VALUES(?1,'s',NULL,1,400,NULL,?2)",
            params![id, json],
        )
        .unwrap();
    }
    assert!(matches!(
        maxed_store::open_reader(&path),
        Err(maxed_store::StoreError::Migration)
    ));
    maxed_store::migrate(&mut db).unwrap();
    assert_loot_indexes(&db);
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
    for (id, json, expected) in cases {
        let row: (Option<i64>, String) = db
            .query_row(
                "SELECT source_type,json FROM drops WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, (expected, json.to_owned()), "{id}");
    }
    for (index, script) in scripts.iter().enumerate() {
        assert_eq!(
            db.query_row(
                "SELECT checksum FROM schema_migrations WHERE version=?1",
                [index as i64 + 1],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
            format!("{:x}", Sha256::digest(script.as_bytes()))
        );
    }
    maxed_store::migrate(&mut db).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        12
    );
    maxed_store::open_reader(&path).unwrap();
}

#[test]
fn projector_keeps_source_type_in_sync_with_json() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let events = common::story_events();
    maxed_store::apply_batch(&mut db, &events).unwrap();
    let mut drop = events
        .iter()
        .find_map(|event| match &event.event {
            EngineEvent::BagDropped(drop) => Some(drop.clone()),
            _ => None,
        })
        .unwrap();
    let source: (Option<u16>, Option<u16>) = db
        .query_row(
            "SELECT source_type,json_extract(json,'$.source_enemy') FROM drops WHERE id=?1",
            [&drop.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(source, (drop.source_enemy, drop.source_enemy));
    let original_at = drop.at;
    for source in [Some(77), None] {
        drop.source_enemy = source;
        drop.at = Timestamp(original_at.0 + 100);
        let head: i64 = db
            .query_row(
                "SELECT sequence FROM stream_heads WHERE stream_id='test-stream'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        maxed_store::apply_batch(
            &mut db,
            &[common::envelope(
                u64::try_from(head).unwrap() + 1,
                EngineEvent::DropUpdated(drop.clone()),
            )],
        )
        .unwrap();
        let row: (Option<u16>, Option<u16>, i64, String) = db.query_row(
            "SELECT source_type,json_extract(json,'$.source_enemy'),at,json_extract(json,'$.at') FROM drops WHERE id=?1",
            [&drop.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).unwrap();
        assert_eq!((row.0, row.1), (source, source));
        assert_eq!(row.2, i64::try_from(original_at.0).unwrap());
        assert_eq!(row.3, original_at.0.to_string());
    }
}

#[test]
fn migration_008_is_lf_only() {
    let bytes = include_bytes!("../migrations/008_loot_browsing.sql");
    assert!(!bytes.contains(&b'\r'));
}
