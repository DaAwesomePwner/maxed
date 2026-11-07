use crate::common;
use common::stats::v9_database as v9;
use rusqlite::{params, Connection};

fn version(db: &Connection) -> i64 {
    db.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn fresh_database_uses_current_schema_with_map_visits() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    assert_eq!(version(&db), maxed_store::SCHEMA_VERSION);
    db.execute("INSERT INTO play_sessions VALUES('s',0,0,NULL,'{}')", [])
        .unwrap();
    let insert = "INSERT INTO map_visits VALUES(?1,'stream','s',NULL,NULL,'Nexus',?2,NULL,?3,?4,'live','{}')";
    db.execute(insert, params!["a", "nexus", 10, Option::<i64>::None])
        .unwrap();
    // One open visit per stream.
    assert!(db
        .execute(insert, params!["b", "realm", 20, Option::<i64>::None])
        .is_err());
    // Closed visits may share the stream; kinds and order are checked.
    db.execute(insert, params!["c", "other_hub", 20, Some(30)])
        .unwrap();
    assert!(db
        .execute(insert, params!["d", "vault", 20, Some(30)])
        .is_err());
    assert!(db
        .execute(insert, params!["e", "realm", 20, Some(19)])
        .is_err());
}

#[test]
fn v9_database_upgrades_to_current_schema_and_backfills_ended_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v9");
    let mut db = v9(&path);
    db.execute_batch(
        "INSERT INTO play_sessions VALUES('s1',100,900,900,'{}');
         INSERT INTO characters(id,observed_at,json) VALUES('synth-character',100,'{}');
         INSERT INTO runs(id,session_id,character_id,dungeon,started,ended,completed_at,died_at,outcome,complete_capture,entry_character_json,json)
           VALUES('run-ended','s1','synth-character','Synthetic Halls',200,500,450,NULL,'cleared',1,NULL,'{}'),
                 ('run-clock','s1',NULL,'Synthetic Halls',600,550,NULL,NULL,'unknown',1,NULL,'{}'),
                 ('run-open','s1',NULL,'Synthetic Halls',700,NULL,NULL,NULL,NULL,1,NULL,'{}');",
    )
    .unwrap();
    maxed_store::migrate(&mut db).unwrap();
    assert_eq!(version(&db), maxed_store::SCHEMA_VERSION);
    let visits = common::stats::visits(&db);
    assert_eq!(
        visits,
        vec![
            maxed_types::MapVisitView {
                id: "backfill:run-ended".into(),
                stream_id: "backfill".into(),
                session_id: "s1".into(),
                character_id: Some("synth-character".into()),
                run_id: Some("run-ended".into()),
                map: "Synthetic Halls".into(),
                kind: maxed_types::AreaKind::Dungeon,
                realm_instance_key: None,
                started: maxed_types::Timestamp(200),
                ended: Some(maxed_types::Timestamp(500)),
                source: maxed_types::VisitSource::Backfill,
            },
            // An end before the start (clock regression) is clamped to the start.
            maxed_types::MapVisitView {
                id: "backfill:run-clock".into(),
                stream_id: "backfill".into(),
                session_id: "s1".into(),
                character_id: None,
                run_id: Some("run-clock".into()),
                map: "Synthetic Halls".into(),
                kind: maxed_types::AreaKind::Dungeon,
                realm_instance_key: None,
                started: maxed_types::Timestamp(600),
                ended: Some(maxed_types::Timestamp(600)),
                source: maxed_types::VisitSource::Backfill,
            },
        ]
    );
    // Migrating again changes nothing.
    maxed_store::migrate(&mut db).unwrap();
    assert_eq!(common::stats::visits(&db).len(), 2);
}

#[test]
fn newer_schema_is_refused() {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    db.pragma_update(None, "user_version", maxed_store::SCHEMA_VERSION + 1)
        .unwrap();
    assert!(matches!(
        maxed_store::migrate(&mut db),
        Err(maxed_store::StoreError::NewerSchema)
    ));
}

#[test]
fn backfill_json_survives_names_with_quotes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v9");
    let mut db = v9(&path);
    db.execute_batch(
        r#"INSERT INTO play_sessions VALUES('s1',100,900,900,'{}');
           INSERT INTO runs(id,session_id,character_id,dungeon,started,ended,completed_at,died_at,outcome,complete_capture,entry_character_json,json)
             VALUES('r"1','s1',NULL,'Synth "Quoted" \ Halls',200,300,NULL,NULL,'left',1,NULL,'{}');"#,
    )
    .unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let visits = common::stats::visits(&db);
    assert_eq!(visits[0].map, r#"Synth "Quoted" \ Halls"#);
    assert_eq!(visits[0].run_id.as_deref(), Some(r#"r"1"#));
}

#[test]
fn run_open_at_upgrade_is_recovered_without_a_visit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let db = v9(&path);
    let session = maxed_types::PlaySessionView {
        id: "s1".into(),
        started: maxed_types::Timestamp(100),
        last_traffic: maxed_types::Timestamp(400),
        ended: None,
        issues: vec![],
    };
    let run = common::stats::run("run-open", "s1", "Synthetic Halls", 200);
    db.execute(
        "INSERT INTO play_sessions VALUES('s1',100,400,NULL,?1)",
        [serde_json::to_string(&session).unwrap()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO runs(id,session_id,character_id,dungeon,started,ended,completed_at,died_at,outcome,complete_capture,entry_character_json,json)
         VALUES('run-open','s1',NULL,'Synthetic Halls',200,NULL,NULL,NULL,NULL,1,NULL,?1)",
        [serde_json::to_string(&run).unwrap()],
    )
    .unwrap();
    drop(db);
    // Confirm the stopped legacy upgrade before the writer recovers the open run.
    common::installed_schema::upgrade_after_confirmation(&path).unwrap();
    let writer =
        maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()).unwrap();
    writer.shutdown().unwrap();
    let db = Connection::open(&path).unwrap();
    let ended: Option<i64> = db
        .query_row("SELECT ended FROM runs WHERE id='run-open'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(ended.is_some());
    assert!(common::stats::visits(&db).is_empty());
}
