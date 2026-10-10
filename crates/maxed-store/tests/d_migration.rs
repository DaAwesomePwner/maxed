// Compile the actual catalog rule builder in this test only; no copied rank table.
#[allow(dead_code)]
#[path = "../../maxed-assets/src/rules.rs"]
mod asset_rules;
use maxed_store::*;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
const SCRIPTS: [&str; 10] = [
    include_str!("../migrations/001_initial.sql"),
    include_str!("../migrations/002_fight_summaries.sql"),
    include_str!("../migrations/003_loot_and_sessions.sql"),
    include_str!("../migrations/004_alerts.sql"),
    include_str!("../migrations/005_instance.sql"),
    include_str!("../migrations/006_summary_target_kinds.sql"),
    include_str!("../migrations/007_pb_lookup_indexes.sql"),
    include_str!("../migrations/008_loot_browsing.sql"),
    include_str!("../migrations/009_chat.sql"),
    include_str!("../migrations/010_map_visits.sql"),
];
fn legacy(db: &Connection, version: usize) {
    for (i, script) in SCRIPTS.iter().take(version).enumerate() {
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
    db.pragma_update(None, "user_version", version as i64)
        .unwrap();
}
#[test]
fn fresh_install_retains_v5_checksum_at_v8() {
    let script = include_str!("../migrations/005_instance.sql");
    assert!(
        !script.contains('\r'),
        "migration checksum must use LF bytes"
    );
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let installed: String = db
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE version=5",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        installed,
        format!("{:x}", Sha256::digest(script.as_bytes()))
    );
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
}
#[test]
fn v6_reader_rejects_newer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future");
    let db = Connection::open(&path).unwrap();
    db.pragma_update(None, "user_version", 13).unwrap();
    assert!(matches!(open_reader(&path), Err(StoreError::NewerSchema)));
}
#[test]
fn v4_to_v5_preserves_rows_and_backfills_ranks() {
    for version in 1..=4 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy");
        let db = Connection::open(&path).unwrap();
        legacy(&db, version);
        db.execute("INSERT INTO play_sessions VALUES('s',1,2,NULL,'{}')", [])
            .unwrap();
        db.execute(
            "INSERT INTO drops VALUES('d','s',NULL,1,1708,NULL,'{\"bag_rank\":null}')",
            [],
        )
        .unwrap();
        upgrade_history(&path).unwrap();
        open_reader(&path).unwrap();
        let row: (i64, i64) = db
            .query_row(
                "SELECT bag_rank,json_extract(json,'$.bag_rank') FROM drops",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, (90, 90));
        assert_eq!(
            db.query_row("SELECT count(*) FROM schema_migrations", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            12
        );
        assert!(!db
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap());
    }
}

#[test]
fn backfill_matches_rules_table() {
    let bags = asset_rules::bags();
    let mut db = Connection::open_in_memory().unwrap();
    legacy(&db, 4);
    db.execute("INSERT INTO play_sessions VALUES('s',1,2,NULL,'{}')", [])
        .unwrap();
    for b in &bags {
        // Legacy producers excluded base brown/pink and already stamped whites.
        if matches!(b.object_type, 1280 | 1286 | 1709 | 1710) {
            continue;
        }
        let rank = b.is_white.then_some(255);
        db.execute(
            "INSERT INTO drops VALUES(?1,'s',NULL,1,?2,?3,json_object('bag_rank',?3))",
            rusqlite::params![b.object_type.to_string(), b.object_type, rank],
        )
        .unwrap();
    }
    db.execute(
        "INSERT INTO drops VALUES('supplied','s',NULL,1,1708,42,'{\"bag_rank\":42}')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO drops VALUES('unknown','s',NULL,1,9999,NULL,'{\"bag_rank\":null}')",
        [],
    )
    .unwrap();
    migrate(&mut db).unwrap();
    for b in &bags {
        if matches!(b.object_type, 1280 | 1286 | 1709 | 1710) {
            continue;
        }
        let row: (Option<u8>, Option<u8>) = db
            .query_row(
                "SELECT bag_rank,json_extract(json,'$.bag_rank') FROM drops WHERE id=?1",
                [b.object_type.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, (b.rank, b.rank), "bag {}", b.object_type);
    }
    assert_eq!(
        db.query_row("SELECT bag_rank FROM drops WHERE id='supplied'", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        42
    );
    assert_eq!(
        db.query_row("SELECT bag_rank FROM drops WHERE id='unknown'", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .unwrap(),
        None
    );
}

#[test]
fn v5_backfill_failure_rolls_back_schema_and_rows() {
    let mut db = Connection::open_in_memory().unwrap();
    legacy(&db, 4);
    db.execute("INSERT INTO play_sessions VALUES('s',1,2,NULL,'{}')", [])
        .unwrap();
    db.execute(
        "INSERT INTO drops VALUES('d','s',NULL,1,1708,NULL,'broken')",
        [],
    )
    .unwrap();
    assert!(matches!(migrate(&mut db), Err(StoreError::Migration)));
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='instance_events'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT bag_rank FROM drops", [], |r| r
            .get::<_, Option<i64>>(0))
            .unwrap(),
        None
    );
}

#[test]
fn v4_upgrade_leaves_corrupt_fight_details_and_legacy_summaries_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("corrupt-detail");
    let mut db = Connection::open(&path).unwrap();
    legacy(&db, 4);
    let snapshot = maxed_types::demo::demo_snapshot();
    let session = snapshot.session.unwrap();
    let run = snapshot.run.unwrap();
    let fight = &snapshot.fights[0];
    db.execute(
        "INSERT INTO play_sessions VALUES(?1,1,9,NULL,?2)",
        rusqlite::params![session.id, serde_json::to_string(&session).unwrap()],
    )
    .unwrap();
    let mut run = run;
    run.character_id = None;
    db.execute(
        "INSERT INTO runs VALUES(?1,?2,NULL,?3,1,NULL,NULL,NULL,NULL,0,NULL,?4)",
        rusqlite::params![
            run.id,
            run.session_id,
            run.dungeon,
            serde_json::to_string(&run).unwrap()
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO fights VALUES(?1,?2,?3,2,NULL,300,150,0,'broken')",
        rusqlite::params![fight.id, run.id, session.id],
    )
    .unwrap();
    let mut summary = maxed_types::demo::demo_run_story().fights.remove(0);
    summary.partial = false;
    summary.estimated = false;
    let json = serde_json::to_string(&summary).unwrap();
    db.execute(
        "INSERT INTO fight_summaries VALUES(?1,?2,2,?3)",
        rusqlite::params![fight.id, run.id, json],
    )
    .unwrap();
    migrate(&mut db).unwrap();
    assert_eq!(
        db.query_row("SELECT json FROM fight_summaries", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        json
    );
    assert_eq!(
        db.query_row("SELECT json FROM fights", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "broken"
    );
    let reader = open_reader(&path).unwrap();
    let story = reader.get_run(&run.id).unwrap().unwrap();
    assert_eq!(story.boss_fights, Some(vec![]));
    assert_eq!(story.boss_fights_limited, Some(true));
    let totals = reader.get_run(&run.id).unwrap().unwrap().totals.unwrap();
    assert_eq!(totals.kills, 0);
    assert_eq!(totals.partial, summary.partial || summary.estimated);
    assert_eq!(totals.unattributed_damage, summary.total_damage);
    for (partial, estimated) in [(true, false), (false, true), (true, true)] {
        db.execute("UPDATE fight_summaries SET json=json_set(json,'$.partial',json(?1),'$.estimated',json(?2))",
            rusqlite::params![partial.to_string(), estimated.to_string()]).unwrap();
        assert!(
            reader
                .get_run(&run.id)
                .unwrap()
                .unwrap()
                .totals
                .unwrap()
                .partial
        );
    }
    assert!(reader.get_fight(&fight.id).is_err());
}

#[test]
fn v10_purges_excluded_drops_items_and_alerts() {
    let mut db = Connection::open_in_memory().unwrap();
    legacy(&db, 10);
    db.execute_batch(
        "INSERT INTO play_sessions VALUES('s',0,1000,NULL,'{}'),('other',0,1000,NULL,'{}');
        INSERT INTO runs(id,session_id,dungeon,started,complete_capture,json) VALUES('run','s','Synthetic',0,1,'{}');
        INSERT INTO map_visits VALUES
        ('n','n','s',NULL,NULL,'Nexus','nexus',NULL,100,200,'live','{}'),
        ('h','h','s',NULL,NULL,'Vault','other_hub',NULL,300,400,'live','{}'),
        ('r','r','s',NULL,NULL,'Realm','realm',NULL,400,500,'live','{}'),
        ('open','open','s',NULL,NULL,'Vault','other_hub',NULL,900,NULL,'live','{}');",
    )
    .unwrap();
    for (id, ty, at, session) in [
        ("brown", 1280, 0, "s"),
        ("pink", 1286, 0, "s"),
        ("boosted-brown", 1709, 0, "s"),
        ("boosted-pink", 1710, 0, "s"),
        ("nexus", 1287, 100, "s"),
        ("hub", 1287, 350, "s"),
        ("realm", 1287, 450, "s"),
        ("unmatched", 1287, 600, "s"),
        ("end-boundary", 1287, 200, "s"),
        ("before", 1287, 99, "s"),
        ("other-session", 1287, 150, "other"),
        ("open-hub", 1287, 950, "s"),
        ("hub-white", 1292, 150, "s"),
        ("kept-white", 1292, 600, "s"),
    ] {
        db.execute(
            "INSERT INTO drops(id,session_id,at,bag_type,json) VALUES(?1,?2,?3,?4,'{}')",
            rusqlite::params![id, session, at, ty],
        )
        .unwrap();
        db.execute("INSERT INTO drop_items VALUES(?1,0,200,1,'{}',NULL)", [id])
            .unwrap();
        db.execute(
            "INSERT INTO timeline VALUES(?1,'run',?2,'drop',?1,'Observed bag')",
            rusqlite::params![id, at],
        )
        .unwrap();
    }
    for (id, drop_id, at, ordinal) in [
        ("removed", "hub-white", 150, 1),
        ("kept", "kept-white", 600, 2),
    ] {
        let json = serde_json::json!({"details":{"type":"white_bag","drop_id":drop_id,"bag_type":1292,"ordinal":ordinal,"window":{"start":"0","end":"1000"}}});
        db.execute(
            "INSERT INTO alerts VALUES(?1,NULL,?2,'white_bag',?3)",
            rusqlite::params![id, at, json.to_string()],
        )
        .unwrap();
        db.execute("INSERT INTO alert_occurrences VALUES(?1,'white_bag',?2,'stream',1,0,'banner','pending',?1)",rusqlite::params![id,drop_id]).unwrap();
    }
    db.execute("INSERT INTO alert_occurrences VALUES('suppressed','white_bag','brown',NULL,NULL,NULL,'off','suppressed',NULL)",[]).unwrap();
    migrate(&mut db).unwrap();
    let ids = |db: &Connection, table: &str, column: &str| -> Vec<String> {
        db.prepare(&format!("SELECT {column} FROM {table} ORDER BY {column}"))
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    let expected = vec![
        "before",
        "end-boundary",
        "kept-white",
        "other-session",
        "realm",
        "unmatched",
    ];
    assert_eq!(ids(&db, "drops", "id"), expected);
    assert_eq!(ids(&db, "drop_items", "drop_id"), expected);
    assert_eq!(ids(&db, "timeline", "related_id"), expected);
    assert_eq!(ids(&db, "alerts", "id"), ["kept"]);
    assert_eq!(ids(&db, "alert_occurrences", "id"), ["kept"]);
    assert_eq!(
        db.query_row(
            "SELECT json_extract(json,'$.details.ordinal') FROM alerts",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert!(!db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        12
    );
    let checksum: String = db
        .query_row(
            "SELECT checksum FROM schema_migrations WHERE version=11",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        checksum,
        format!(
            "{:x}",
            Sha256::digest(include_bytes!("../migrations/011_exclude_junk_bags.sql"))
        )
    );
    migrate(&mut db).unwrap();
    assert_eq!(ids(&db, "drops", "id"), expected);
}
