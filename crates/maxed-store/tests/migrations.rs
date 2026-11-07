use maxed_store::{migrate, StoreError, SCHEMA_VERSION};
use rusqlite::Connection;

#[test]
fn every_unconfirmed_file_writer_refuses_legacy_without_changing_bytes() {
    for version in [2, 6, 10] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite3");
        let db = Connection::open(&path).unwrap();
        crate::common::installed_schema::install(&db, version);
        drop(db);
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            maxed_store::upgrade_history(&path),
            Err(StoreError::HistoryUpgradeRequired)
        ));
        assert!(matches!(
            maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()),
            Err(StoreError::HistoryUpgradeRequired)
        ));
        assert!(matches!(
            maxed_store::maintenance::StoppedMaintenance::open(&path, Default::default()),
            Err(StoreError::HistoryUpgradeRequired)
        ));
        assert!(matches!(
            maxed_store::maintain_chat_file(&path, maxed_store::ChatMaintenance::Clear),
            Err(StoreError::HistoryUpgradeRequired)
        ));
        assert!(matches!(
            maxed_store::party_write_file(
                &path,
                &maxed_store::PartyWrite::Mark {
                    key: "synthetic".into(),
                    name: "Synthetic".into(),
                    mark: None,
                    note: None
                }
            ),
            Err(StoreError::HistoryUpgradeRequired)
        ));
        assert!(std::fs::read(&path).unwrap() == before);
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            crate::common::installed_schema::user_version(&db),
            version as i64
        );
    }
}

#[test]
fn schema_creation_is_idempotent_and_integral() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    assert_eq!(
        db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert!(!db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
    assert!(db.execute("INSERT INTO runs(id,session_id,dungeon,started,complete_capture,json) VALUES('r','missing','d',1,1,'{}')", []).is_err());
}

#[test]
fn future_and_tampered_schemas_are_rejected() {
    let mut db = Connection::open_in_memory().unwrap();
    db.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    assert!(matches!(migrate(&mut db), Err(StoreError::NewerSchema)));
    db.pragma_update(None, "user_version", 0).unwrap();
    migrate(&mut db).unwrap();
    db.execute("UPDATE schema_migrations SET checksum='tampered'", [])
        .unwrap();
    assert!(matches!(migrate(&mut db), Err(StoreError::Migration)));
}

#[test]
fn unrelated_database_is_untouched() {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('keep');")
        .unwrap();
    assert!(matches!(migrate(&mut db), Err(StoreError::Migration)));
    assert_eq!(
        db.query_row("SELECT value FROM unrelated", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

fn legacy(path: &std::path::Path) -> Connection {
    let mut db = Connection::open(path).unwrap();
    crate::common::installed_schema::install(&db, 14);
    let snapshot = maxed_types::demo::demo_snapshot();
    let events: Vec<_> = [
        maxed_types::EngineEvent::SessionUpdated(snapshot.session.unwrap()),
        maxed_types::EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
        maxed_types::EngineEvent::RunStarted(snapshot.run.unwrap()),
        maxed_types::EngineEvent::FightStarted(snapshot.fights[0].clone()),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, event)| maxed_types::EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "migration-fixture".into(),
        sequence: maxed_types::Sequence(maxed_types::Timestamp(i as u64 + 1)),
        at: maxed_types::Timestamp(5_000_000),
        event,
    })
    .collect();
    crate::common::apply_legacy_batch(&mut db, &events);
    db.execute_batch("DROP TABLE party_run_members; DROP TABLE party_players; DROP TABLE player_profiles; DROP TABLE player_gear_log; DROP TABLE scope_timelines; DROP TABLE player_timelines; DROP TABLE map_visits; DROP INDEX play_sessions_started; DROP TABLE chat_fts; DROP TABLE chat_messages; DROP INDEX drops_source; DROP INDEX runs_dungeon_ended; DROP INDEX drops_id_at; DROP INDEX drops_stats; ALTER TABLE drops DROP COLUMN source_type; DROP TRIGGER pd_player_insert; DROP TRIGGER pd_player_delete; DROP TRIGGER pd_player_update; DROP TRIGGER pd_fight_association; DROP TABLE pd_player_contributions; DROP TABLE pd_run_players; DROP INDEX pd_home_session_runs; DROP INDEX pd_home_today_runs; DROP INDEX pd_run_evidence_kind; DROP INDEX pd_fight_players_totals; DROP INDEX pd_runs_comparison; DROP TABLE event_fts; DROP TABLE instance_events; DROP TABLE alert_occurrences; DROP TABLE personal_bests; DROP INDEX alerts_kind_at; DROP INDEX fights_final_boss; DROP INDEX fights_final_boss_key; DROP INDEX fight_players_identity; DROP INDEX character_aliases_verified; DROP TABLE stream_sessions; DROP INDEX fame_session; DROP INDEX deaths_session;
        ALTER TABLE fame_samples DROP COLUMN session_id; ALTER TABLE deaths DROP COLUMN session_id;
        ALTER TABLE drop_items RENAME TO v3_items; DROP INDEX drop_items_type;
        CREATE TABLE drop_items(drop_id TEXT NOT NULL REFERENCES drops(id),slot INTEGER NOT NULL,item_type INTEGER NOT NULL,quantity INTEGER NOT NULL CHECK(quantity>0),json TEXT NOT NULL,PRIMARY KEY(drop_id,slot));
        INSERT INTO drop_items SELECT drop_id,slot,item_type,quantity,json FROM v3_items;
        DROP TABLE v3_items; CREATE INDEX drop_items_type ON drop_items(item_type,drop_id);
        DROP TABLE fight_summaries; DELETE FROM schema_migrations WHERE version>=2; PRAGMA user_version=1;").unwrap();
    db
}

#[test]
fn v1_backfill_is_atomic_and_reopens_as_v8() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy");
    let db = legacy(&path);
    assert!(matches!(
        maxed_store::open_reader(&path),
        Err(StoreError::Migration)
    ));
    let old_checksum: String = db
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE version=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let original: String = db
        .query_row("SELECT json FROM fights", [], |r| r.get(0))
        .unwrap();
    db.execute("UPDATE fights SET json='corrupt'", []).unwrap();
    assert!(crate::common::installed_schema::upgrade_after_confirmation(&path).is_err());
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='fight_summaries'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute("UPDATE fights SET json=?1", [original]).unwrap();
    crate::common::installed_schema::upgrade_after_confirmation(&path).unwrap();
    crate::common::installed_schema::upgrade_after_confirmation(&path).unwrap();
    let reader = maxed_store::open_reader(&path).unwrap();
    let story = reader.get_run("demo-run").unwrap().unwrap();
    assert_eq!(story.fights.len(), 1);
    assert_eq!(story.fights[0].ordinal, 1);
    assert_eq!(
        reader.get_fight("demo-fight").unwrap().unwrap(),
        maxed_types::demo::demo_snapshot().fights[0]
    );
    assert_eq!(
        db.query_row(
            "SELECT checksum FROM schema_migrations WHERE version=1",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        old_checksum
    );
    db.execute(
        "UPDATE schema_migrations SET checksum='bad' WHERE version=2",
        [],
    )
    .unwrap();
    assert!(matches!(
        maxed_store::open_reader(&path),
        Err(StoreError::Migration)
    ));
}

#[test]
fn upgrade_never_creates_absent_and_initializes_only_empty_version_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent");
    assert!(maxed_store::upgrade_history(&path).is_err());
    assert!(!path.exists());
    drop(Connection::open(&path).unwrap());
    maxed_store::upgrade_history(&path).unwrap();
    assert_eq!(
        maxed_store::open_reader(&path)
            .unwrap()
            .user_version()
            .unwrap(),
        maxed_store::SCHEMA_VERSION
    );
    let unknown = dir.path().join("unknown");
    let db = Connection::open(&unknown).unwrap();
    db.execute_batch("CREATE TABLE unrecognized_history(value TEXT)")
        .unwrap();
    assert!(maxed_store::upgrade_history(&unknown).is_err());
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn large_legacy_backfill_is_measured_separately_from_summary_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy");
    let mut db = legacy(&path);
    let template = serde_json::to_string(&maxed_types::demo::demo_snapshot().fights[0]).unwrap();
    let tx = db.transaction().unwrap();
    tx.execute("DELETE FROM damage_sources", []).unwrap();
    tx.execute("DELETE FROM fight_players", []).unwrap();
    tx.execute("DELETE FROM fights", []).unwrap();
    for i in 0..3000 {
        let id = format!("legacy-{i:04}");
        tx.execute("INSERT INTO fights VALUES(?1,'demo-run','demo-session',2000000,NULL,300,150,0,json_set(?2,'$.id',?1))", rusqlite::params![id, template]).unwrap();
    }
    tx.commit().unwrap();
    let start = std::time::Instant::now();
    crate::common::installed_schema::upgrade_after_confirmation(&path).unwrap();
    eprintln!(
        "synthetic legacy backfill, 3000 fights: {:?}",
        start.elapsed()
    );
    let reader = maxed_store::open_reader(&path).unwrap();
    let start = std::time::Instant::now();
    let story = reader.get_run("demo-run").unwrap().unwrap();
    eprintln!(
        "separate post-backfill story read: {:?}; bytes: {}",
        start.elapsed(),
        serde_json::to_vec(&story).unwrap().len()
    );
    assert_eq!(story.fights.len(), 3000);
}

#[test]
fn cold_v2_upgrade_backfills_only_unambiguous_sessions_and_preserves_checksums() {
    use sha2::{Digest, Sha256};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v2");
    let db = legacy(&path);
    let v2 = include_str!("../migrations/002_fight_summaries.sql");
    db.execute_batch(v2).unwrap();
    db.execute(
        "INSERT INTO schema_migrations VALUES(2,?1)",
        [format!("{:x}", Sha256::digest(v2.as_bytes()))],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 2).unwrap();
    db.execute(
        "UPDATE play_sessions SET started=100,last_traffic=200,ended=200",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO play_sessions VALUES('overlap',150,250,250,'{}')",
        [],
    )
    .unwrap();
    for (i, at) in [125, 175, 225, 300].into_iter().enumerate() {
        db.execute(
            "INSERT INTO fame_samples VALUES('legacy',?1,'demo-character',?2,100)",
            rusqlite::params![i as i64, at],
        )
        .unwrap();
        db.execute(
            "INSERT INTO deaths VALUES('legacy',?1,NULL,NULL,?2)",
            rusqlite::params![i as i64, at],
        )
        .unwrap();
    }
    let bag = maxed_types::demo::demo_run_story().drops.remove(0);
    let mut old_bag = serde_json::to_value(&bag).unwrap();
    old_bag["contents"][0]
        .as_object_mut()
        .unwrap()
        .remove("taken_observed_at");
    let item = old_bag["contents"][0].to_string();
    db.execute(
        "INSERT INTO drops VALUES('demo-drop','demo-session','demo-run',4500000,400,255,?1)",
        [old_bag.to_string()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO drop_items VALUES('demo-drop',0,200,1,?1)",
        [&item],
    )
    .unwrap();
    // Corrupt legacy JSON must roll back the entire v3 DDL and retain v2.
    db.execute("UPDATE drop_items SET json='corrupt'", [])
        .unwrap();
    assert!(crate::common::installed_schema::upgrade_after_confirmation(&path).is_err());
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM pragma_table_info('fame_samples') WHERE name='session_id'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute("UPDATE drop_items SET json=?1", [&item])
        .unwrap();
    let checksum: String = db
        .query_row(
            "SELECT group_concat(checksum) FROM schema_migrations ORDER BY version",
            [],
            |r| r.get(0),
        )
        .unwrap();
    drop(db);
    assert!(matches!(
        maxed_store::open_reader(&path),
        Err(StoreError::Migration)
    ));
    crate::common::installed_schema::upgrade_after_confirmation(&path).unwrap();
    crate::common::installed_schema::upgrade_after_confirmation(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    assert_eq!(db.query_row("SELECT group_concat(checksum) FROM schema_migrations WHERE version<=2 ORDER BY version",[],|r|r.get::<_,String>(0)).unwrap(),checksum);
    for table in ["fame_samples", "deaths"] {
        let actual = db
            .prepare(&format!("SELECT session_id FROM {table} ORDER BY sequence"))
            .unwrap()
            .query_map([], |r| r.get::<_, Option<String>>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            actual,
            vec![
                Some("demo-session".into()),
                None,
                Some("overlap".into()),
                None
            ]
        );
    }
    let saved = maxed_store::open_reader(&path)
        .unwrap()
        .get_run("demo-run")
        .unwrap()
        .unwrap();
    assert_eq!(saved.drops[0].contents[0].taken_observed_at, None);
    assert_eq!(
        db.query_row("SELECT count(*) FROM drop_items", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(!db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
    db.execute(
        "UPDATE schema_migrations SET checksum='tampered' WHERE version=3",
        [],
    )
    .unwrap();
    assert!(matches!(
        maxed_store::open_reader(&path),
        Err(StoreError::Migration)
    ));
}

// Construct the actual installed v2 scripts, not a latest-schema version downgrade.
fn empty_v2(path: &std::path::Path) -> Connection {
    use sha2::{Digest, Sha256};
    let db = Connection::open(path).unwrap();
    db.pragma_update(None, "foreign_keys", true).unwrap();
    for (i, script) in [
        include_str!("../migrations/001_initial.sql"),
        include_str!("../migrations/002_fight_summaries.sql"),
    ]
    .into_iter()
    .enumerate()
    {
        db.execute_batch(script).unwrap();
        db.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2)",
            rusqlite::params![
                i as i64 + 1,
                format!("{:x}", Sha256::digest(script.as_bytes()))
            ],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", 2).unwrap();
    db
}

#[test]
#[ignore = "large synthetic v2 upgrade measurement; run serially with --ignored --nocapture"]
fn large_v2_backfill_is_measured_separately_from_home_reads() {
    use maxed_types::*;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large-v2");
    let mut db = empty_v2(&path);
    let snapshot = demo::demo_snapshot();
    let tx = db.transaction().unwrap();
    tx.execute(
        "INSERT INTO characters(id,observed_at,json) VALUES('demo-character',0,?1)",
        [serde_json::to_string(&snapshot.character.unwrap()).unwrap()],
    )
    .unwrap();
    tx.execute("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<3000)
        INSERT INTO play_sessions SELECT printf('session-%04d',i),(i-1)*100,i*100-1,i*100-1,
        json_set(?1,'$.id',printf('session-%04d',i),'$.started',CAST((i-1)*100 AS TEXT),'$.last_traffic',CAST(i*100-1 AS TEXT),'$.ended',CAST(i*100-1 AS TEXT)) FROM n",
        [serde_json::to_string(&snapshot.session.unwrap()).unwrap()]).unwrap();
    tx.execute("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<10000)
        INSERT INTO runs SELECT printf('run-%05d',i),printf('session-%04d',1+(i-1)%3000),'demo-character','Synthetic Sanctum',((i-1)%3000)*100,((i-1)%3000)*100+50,NULL,NULL,'left',1,NULL,
        json_set(?1,'$.id',printf('run-%05d',i),'$.session_id',printf('session-%04d',1+(i-1)%3000),'$.started',CAST(((i-1)%3000)*100 AS TEXT),'$.ended',CAST(((i-1)%3000)*100+50 AS TEXT)) FROM n",
        [serde_json::to_string(&snapshot.run.unwrap()).unwrap()]).unwrap();
    tx.execute("INSERT INTO drops SELECT r.id||'-'||n.i,r.session_id,r.id,r.started,400,NULL,
        json_set(?1,'$.id',r.id||'-'||n.i,'$.run_id',r.id,'$.session_id',r.session_id,'$.at',CAST(r.started AS TEXT))
        FROM runs r CROSS JOIN (SELECT 1 i UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4) n",
        [serde_json::to_string(&snapshot.drops[0]).unwrap()]).unwrap();
    tx.execute(
        "INSERT INTO drop_items SELECT id,0,200,1,?1 FROM drops",
        [serde_json::to_string(&snapshot.drops[0].contents[0]).unwrap()],
    )
    .unwrap();
    tx.execute_batch(
        "WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<299999)
        INSERT INTO fame_samples SELECT 'legacy',i,'demo-character',i,1000+i FROM n;
        WITH RECURSIVE n(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<19999)
        INSERT INTO deaths SELECT 'legacy',i,'demo-character',NULL,i*15 FROM n;",
    )
    .unwrap();
    tx.commit().unwrap();
    drop(db);
    let start = std::time::Instant::now();
    crate::common::installed_schema::upgrade_after_confirmation(&path).unwrap();
    let elapsed = start.elapsed();
    eprintln!("cold v2->v3: 3000 sessions, 10000 runs, 40000 drops/items, 300000 fame samples, 20000 deaths: {elapsed:?}");
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM fame_samples WHERE session_id IS NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM deaths WHERE session_id IS NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM fame_samples WHERE session_id != printf('session-%04d',1+at/100)",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM deaths WHERE session_id != printf('session-%04d',1+at/100)",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let start = std::time::Instant::now();
    let home = maxed_store::open_reader(&path)
        .unwrap()
        .home_summary(HomeScope::Session {
            id: "session-3000".into(),
        })
        .unwrap();
    eprintln!("separate post-upgrade Home read: {:?}", start.elapsed());
    assert_eq!(home.fame.len(), 100);
    assert_eq!(home.runs, 3);
    assert_eq!(
        db.query_row("SELECT count(*) FROM drop_items", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        40000
    );
    if !cfg!(debug_assertions) {
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "v2->v3 upgrade exceeded 2 s: {elapsed:?}"
        );
    }
}

#[test]
fn cold_v2_preserves_run_deaths_and_open_session_tails() {
    use maxed_types::*;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ownership-v2");
    let db = empty_v2(&path);
    let snapshot = demo::demo_snapshot();
    db.execute(
        "INSERT INTO characters(id,observed_at,json) VALUES('demo-character',0,?1)",
        [serde_json::to_string(&snapshot.character.unwrap()).unwrap()],
    )
    .unwrap();
    for (id, started, last, ended) in [
        ("demo-session", 100, 150, Some(200)),
        ("overlap", 120, 180, Some(180)),
        ("next", 300, 400, Some(400)),
        ("open", 500, 510, None),
        ("after-open", 800, 900, Some(900)),
        ("tail", 1000, 1010, None),
    ] {
        let session = PlaySessionView {
            id: id.into(),
            started: Timestamp(started),
            last_traffic: Timestamp(last),
            ended: ended.map(Timestamp),
            issues: vec![],
        };
        db.execute(
            "INSERT INTO play_sessions VALUES(?1,?2,?3,?4,?5)",
            rusqlite::params![
                id,
                started as i64,
                last as i64,
                ended.map(|value| value as i64),
                serde_json::to_string(&session).unwrap()
            ],
        )
        .unwrap();
    }
    db.execute("INSERT INTO runs VALUES('demo-run','demo-session','demo-character','Synthetic Sanctum',100,NULL,NULL,NULL,NULL,0,NULL,?1)",[serde_json::to_string(&snapshot.run.unwrap()).unwrap()]).unwrap();
    // Explicit run ownership wins both an overlapping interval and a later session.
    for (sequence, at) in [(1, 170), (2, 450)] {
        db.execute(
            "INSERT INTO deaths VALUES('run-deaths',?1,NULL,'demo-run',?2)",
            rusqlite::params![sequence, at],
        )
        .unwrap();
    }
    for (sequence, at) in [110, 170, 250, 300, 401, 700, 800, 900, 901, 1500]
        .into_iter()
        .enumerate()
    {
        db.execute(
            "INSERT INTO fame_samples VALUES('legacy',?1,'demo-character',?2,100)",
            rusqlite::params![sequence as i64, at],
        )
        .unwrap();
        db.execute(
            "INSERT INTO deaths VALUES('legacy',?1,NULL,NULL,?2)",
            rusqlite::params![sequence as i64, at],
        )
        .unwrap();
    }
    drop(db);
    crate::common::installed_schema::upgrade_after_confirmation(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    for table in ["fame_samples", "deaths"] {
        let actual = db
            .prepare(&format!(
                "SELECT session_id FROM {table} WHERE stream_id='legacy' ORDER BY sequence"
            ))
            .unwrap()
            .query_map([], |r| r.get::<_, Option<String>>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        // Closed ends are inclusive. Open bounds end exclusively at the next
        // distinct start, and an open tail without a successor is unbounded.
        assert_eq!(
            actual,
            vec![
                Some("demo-session".into()),
                None,
                None,
                Some("next".into()),
                None,
                Some("open".into()),
                Some("after-open".into()),
                Some("after-open".into()),
                None,
                Some("tail".into())
            ]
        );
    }
    assert_eq!(db.query_row("SELECT count(*) FROM deaths WHERE stream_id='run-deaths' AND session_id='demo-session'",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    let home = maxed_store::open_reader(&path)
        .unwrap()
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap();
    assert_eq!(home.deaths, 3);
}

#[test]
fn cold_v2_equal_start_open_sessions_remain_ambiguous() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("equal-start-v2");
    let db = empty_v2(&path);
    db.execute_batch("INSERT INTO play_sessions VALUES('a',100,110,NULL,'{}'),('b',100,120,NULL,'{}'),('next',300,310,NULL,'{}');
        INSERT INTO deaths VALUES('legacy',1,NULL,NULL,250),('legacy',2,NULL,NULL,300),('legacy',3,NULL,NULL,9223372036854775807);").unwrap();
    drop(db);
    crate::common::installed_schema::upgrade_after_confirmation(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    let actual = db
        .prepare("SELECT session_id FROM deaths ORDER BY sequence")
        .unwrap()
        .query_map([], |r| r.get::<_, Option<String>>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(actual, vec![None, Some("next".into()), Some("next".into())]);
}
