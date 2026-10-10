mod common;
use common::*;
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;

fn database(count: usize) -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    db.execute("DELETE FROM damage_sources", []).unwrap();
    db.execute("DELETE FROM fight_players", []).unwrap();
    db.execute("DELETE FROM fight_summaries", []).unwrap();
    db.execute("DELETE FROM fights", []).unwrap();
    for index in 0..count {
        let mut fight = demo::demo_snapshot().fights.remove(0);
        fight.id = format!("boss-{index:03}");
        fight.started = Timestamp(index as u64);
        fight.boss_type = Some(1);
        fight.target_kind = Some(TargetKind::Boss);
        fight
            .issues
            .retain(|issue| *issue != CoverageIssue::MissingIdentity);
        fight.players.truncate(1);
        fight.players[0].player_id = "TestMage".into();
        fight.players[0].is_local = true;
        fight.players[0].rank = Some(1);
        fight.players[0].share = Some(0.75);
        apply_batch(
            &mut db,
            &[envelope(index as u64 + 8, EngineEvent::FightStarted(fight))],
        )
        .unwrap();
    }
    (dir, path, db)
}

#[test]
fn local_default_and_explicit_missing_player_keep_unknowns() {
    let (_dir, path, db) = database(2);
    let reader = open_reader(&path).unwrap();
    let rows = reader.run_fight_strip("demo-run", None).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].fight_id, "boss-000");
    assert_eq!(rows[1].fight_id, "boss-001");
    for row in rows {
        assert!(row.damage.unwrap() > 0.0);
        assert_eq!(row.rank, Some(1));
        assert_eq!(row.share, Some(0.75));
    }
    db.execute("DELETE FROM damage_sources WHERE fight_id='boss-001'", [])
        .unwrap();
    db.execute(
        "UPDATE fight_players SET player_id='DemoAlly' WHERE fight_id='boss-001'",
        [],
    )
    .unwrap();
    let rows = reader
        .run_fight_strip("demo-run", Some("TestMage"))
        .unwrap();
    assert!(rows[0].damage.is_some());
    assert_eq!(
        (rows[1].damage, rows[1].rank, rows[1].share),
        (None, None, None)
    );
}

#[test]
fn missing_identity_suppresses_only_the_local_default() {
    let (_dir, path, db) = database(2);
    let issues = serde_json::to_string(&vec![CoverageIssue::MissingIdentity]).unwrap();
    db.execute(
        "UPDATE fights SET json=json_set(json,'$.issues',json(?1)) WHERE id='boss-000'",
        [issues],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let rows = reader.run_fight_strip("demo-run", None).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].fight_id, "boss-000");
    assert_eq!(
        (rows[0].damage, rows[0].rank, rows[0].share),
        (None, None, None)
    );
    assert!(rows[1].damage.unwrap() > 0.0);
    assert_eq!(rows[1].rank, Some(1));
    assert_eq!(rows[1].share, Some(0.75));
    let explicit = reader
        .run_fight_strip("demo-run", Some("TestMage"))
        .unwrap();
    for row in explicit {
        assert!(row.damage.unwrap() > 0.0);
        assert_eq!(row.rank, Some(1));
        assert_eq!(row.share, Some(0.75));
    }
}

#[test]
fn zero_or_two_locals_are_ambiguous() {
    let (_dir, path, db) = database(2);
    db.execute("INSERT INTO fight_players SELECT fight_id,'DemoAlly',2,damage,dps,rank,json FROM fight_players WHERE fight_id='boss-000'", []).unwrap();
    db.execute("UPDATE fight_players SET json=json_set(json,'$.is_local',json('false')) WHERE fight_id='boss-001'", []).unwrap();
    for row in open_reader(&path)
        .unwrap()
        .run_fight_strip("demo-run", None)
        .unwrap()
    {
        assert_eq!((row.damage, row.rank, row.share), (None, None, None));
    }
}

#[test]
fn validates_both_identifiers_and_caps_bosses() {
    let (_dir, path, _db) = database(65);
    let reader = open_reader(&path).unwrap();
    for id in ["", "bad\n", &"x".repeat(257)] {
        assert!(matches!(
            reader.run_fight_strip(id, None),
            Err(StoreError::InvalidEvent)
        ));
        assert!(matches!(
            reader.run_fight_strip("demo-run", Some(id)),
            Err(StoreError::InvalidEvent)
        ));
    }
    assert_eq!(reader.run_fight_strip("demo-run", None).unwrap().len(), 64);
    assert!(reader.run_fight_strip("absent", None).unwrap().is_empty());
}

#[test]
fn boss_tiers_and_kill_evidence_are_preserved() {
    let (_dir, path, db) = database(4);
    db.execute("UPDATE fights SET boss_type=NULL", []).unwrap();
    db.execute("UPDATE fight_summaries SET json=json_set(json,'$.boss_type',NULL,'$.target_kind','ordinary')", []).unwrap();
    db.execute("UPDATE fight_summaries SET json=json_set(json,'$.target_kind','event_boss') WHERE fight_id='boss-001'", []).unwrap();
    db.execute("UPDATE fight_summaries SET json=json_set(json,'$.target_kind','boss') WHERE fight_id='boss-002'", []).unwrap();
    db.execute("UPDATE fights SET boss_type=1 WHERE id='boss-003'", [])
        .unwrap();
    db.execute(
        "UPDATE fights SET json=json_set(json,'$.end_reason',NULL,'$.kill',NULL)",
        [],
    )
    .unwrap();
    db.execute("UPDATE fights SET json=json_set(json,'$.kill',json('{\"provisional\":true}')) WHERE id='boss-001'", []).unwrap();
    db.execute("UPDATE fights SET json=json_set(json,'$.kill',json('{\"provisional\":false}')) WHERE id='boss-002'", []).unwrap();
    db.execute(
        "UPDATE fights SET json=json_set(json,'$.end_reason','killed') WHERE id='boss-003'",
        [],
    )
    .unwrap();
    let rows = open_reader(&path)
        .unwrap()
        .run_fight_strip("demo-run", None)
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| (r.fight_id.as_str(), r.killed))
            .collect::<Vec<_>>(),
        vec![("boss-001", false), ("boss-002", true), ("boss-003", true)]
    );
}

#[test]
fn rejects_oversized_strip() {
    let (_dir, path, db) = database(1);
    db.execute(
        "UPDATE fight_summaries SET json=json_set(json,'$.target_name',?1)",
        ["x".repeat(256 * 1024)],
    )
    .unwrap();
    assert!(matches!(
        open_reader(&path)
            .unwrap()
            .run_fight_strip("demo-run", None),
        Err(StoreError::Limit)
    ));
}
