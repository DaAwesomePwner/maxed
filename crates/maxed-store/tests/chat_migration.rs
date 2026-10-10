use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

const SCRIPTS: [&str; 8] = [
    include_str!("../migrations/001_initial.sql"),
    include_str!("../migrations/002_fight_summaries.sql"),
    include_str!("../migrations/003_loot_and_sessions.sql"),
    include_str!("../migrations/004_alerts.sql"),
    include_str!("../migrations/005_instance.sql"),
    include_str!("../migrations/006_summary_target_kinds.sql"),
    include_str!("../migrations/007_pb_lookup_indexes.sql"),
    include_str!("../migrations/008_loot_browsing.sql"),
];

fn version(db: &Connection) -> i64 {
    db.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn fresh_database_is_version_12_with_chat_search() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    assert_eq!(version(&db), 12);
    db.execute("INSERT INTO play_sessions VALUES('s',0,0,NULL,'{}')", [])
        .unwrap();
    db.execute(
        "INSERT INTO chat_messages VALUES('c1','s',NULL,5,'public',NULL,'player','SynthAlpha',NULL,0,'cheap gold here','{}')",
        [],
    )
    .unwrap();
    let hits: i64 = db
        .query_row(
            "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH '\"gold\"*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hits, 1);
    db.execute("DELETE FROM chat_messages WHERE id='c1'", [])
        .unwrap();
    let hits: i64 = db
        .query_row(
            "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH '\"gold\"*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hits, 0);
    let nocase: i64 = db
        .query_row("SELECT 'SynthAlpha'='synthalpha' COLLATE NOCASE", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(nocase, 1);
}

#[test]
fn v8_database_upgrades_to_12() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v8");
    let mut db = Connection::open(&path).unwrap();
    for (index, script) in SCRIPTS.iter().enumerate() {
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
    db.pragma_update(None, "user_version", 8).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    assert_eq!(version(&db), 12);
    let indexed: i64 = db
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='instance_events_kind'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(indexed, 1);
}

#[test]
fn updating_chat_replaces_search_terms() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    db.execute("INSERT INTO play_sessions VALUES('s',0,0,NULL,'{}')", [])
        .unwrap();
    db.execute(
        "INSERT INTO chat_messages VALUES('c1','s',NULL,5,'public',NULL,'player','SynthAlpha',NULL,0,'gold','{}')",
        [],
    ).unwrap();
    db.execute(
        "UPDATE chat_messages SET sender='SynthBravo', text='silver' WHERE id='c1'",
        [],
    )
    .unwrap();
    for (term, expected) in [
        ("gold", 0),
        ("SynthAlpha", 0),
        ("silver", 1),
        ("SynthBravo", 1),
    ] {
        let hits: i64 = db
            .query_row(
                "SELECT count(*) FROM chat_fts WHERE chat_fts MATCH ?1",
                [term],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hits, expected);
    }
    let hits: i64 = db
        .query_row(
            "SELECT count(*) FROM chat_messages WHERE sender='synthbravo'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hits, 1);
}
