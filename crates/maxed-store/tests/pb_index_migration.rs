use maxed_store::migrate;
use maxed_types::*;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
mod common;

#[test]
fn v6_upgrade_adds_pb_indexes_without_rewriting_prior_checksums() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v6");
    let mut db = Connection::open(&path).unwrap();
    let scripts = [
        include_str!("../migrations/001_initial.sql"),
        include_str!("../migrations/002_fight_summaries.sql"),
        include_str!("../migrations/003_loot_and_sessions.sql"),
        include_str!("../migrations/004_alerts.sql"),
        include_str!("../migrations/005_instance.sql"),
        include_str!("../migrations/006_summary_target_kinds.sql"),
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
    db.pragma_update(None, "user_version", 6).unwrap();
    // This legacy fight fixture predates the current drop writer.
    let events: Vec<_> = common::story_events()
        .into_iter()
        .filter(|e| !matches!(e.event, EngineEvent::BagDropped(_)))
        .enumerate()
        .map(|(i, e)| common::envelope(i as u64 + 1, e.event))
        .collect();
    // The current projector writes map visits; drop the scratch table so the stored schema stays v6.
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
    maxed_store::apply_batch(&mut db, &events).unwrap();
    db.execute_batch("DROP TABLE map_visits").unwrap();
    db.execute("UPDATE fights SET json=json_remove(json,'$.dps_basis')", [])
        .unwrap();
    db.execute(
        "INSERT INTO personal_bests SELECT 'demo-character',42,id,100,1 FROM fights",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO alerts VALUES('legacy-alert',NULL,1,'personal_best','{}')",
        [],
    )
    .unwrap();
    let preserved: Vec<String> = ["fights", "runs", "alerts"]
        .iter()
        .map(|table| {
            db.query_row(&format!("SELECT json FROM {table}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect();
    migrate(&mut db).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM personal_bests", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    for (table, expected) in ["fights", "runs", "alerts"].iter().zip(preserved) {
        assert_eq!(
            db.query_row(&format!("SELECT json FROM {table}"), [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            expected
        );
    }

    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
    for name in ["personal_bests_fight", "fights_final_boss_key"] {
        assert!(db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='index' AND name=?1)",
                [name],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
    }
    for (index, script) in scripts.iter().enumerate() {
        assert_eq!(
            db.query_row(
                "SELECT checksum FROM schema_migrations WHERE version=?1",
                [index as i64 + 1],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            format!("{:x}", Sha256::digest(script.as_bytes()))
        );
    }
    let script = include_str!("../migrations/007_pb_lookup_indexes.sql");
    assert!(
        !script.contains('\r'),
        "migration checksums require LF bytes"
    );
    assert_eq!(
        db.query_row(
            "SELECT checksum FROM schema_migrations WHERE version=7",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        format!("{:x}", Sha256::digest(script.as_bytes()))
    );
    migrate(&mut db).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        12
    );
    drop(db);
    let writer = maxed_store::start_writer(
        path.clone(),
        maxed_store::WriterConfig {
            stream_id: "test-stream".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut character = demo::demo_run_story().character.unwrap();
    character.identity_verified = true;
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = "new-basis-first".into();
    fight.complete_capture = true;
    fight.issues.clear();
    fight.boss_type = Some(42);
    fight.ended = Some(Timestamp(10_000_000));
    fight.end_reason = Some(FightEnd::Killed);
    fight.first_hit = Some(Timestamp(1_000_000));
    fight.last_hit = Some(Timestamp(5_000_000));
    fight.players.retain(|p| p.is_local);
    fight.players[0].player_id = character.id.clone();
    fight.players[0].dps = Some(500.);
    for source in &mut fight.players[0].sources {
        source.estimated = false;
    }
    for (i, event) in [
        EngineEvent::CharacterUpdated(character),
        EngineEvent::FightEnded(fight),
    ]
    .into_iter()
    .enumerate()
    {
        writer
            .try_submit_with_alert_context(
                common::envelope(events.len() as u64 + 1 + i as u64, event),
                maxed_store::AlertContext {
                    live: true,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    writer.flush().unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row("SELECT dps FROM personal_bests", [], |r| r.get::<_, f64>(0))
            .unwrap(),
        500.
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM alerts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1,
        "first current-basis record stays silent"
    );
    writer.shutdown().unwrap();
}
