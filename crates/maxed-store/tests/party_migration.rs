use maxed_store::{migrate, open_reader};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const LEGACY_SCRIPTS: [&str; 13] = [
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
    include_str!("../migrations/011_exclude_junk_bags.sql"),
    include_str!("../migrations/012_event_fts.sql"),
    include_str!("../migrations/013_player_profiles.sql"),
];

fn install_013(db: &Connection) {
    db.pragma_update(None, "foreign_keys", true).unwrap();
    for (i, script) in LEGACY_SCRIPTS.iter().enumerate() {
        db.execute_batch(script).unwrap();
        db.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2)",
            params![
                i as i64 + 1,
                format!("{:x}", Sha256::digest(script.as_bytes()))
            ],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", 13).unwrap();
}

fn legacy() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    install_013(&db);
    db.execute(
        "INSERT INTO play_sessions(id,started,last_traffic,json) VALUES('session',0,5000,'{}')",
        [],
    )
    .unwrap();
    db
}

fn run(db: &Connection, id: &str, started: i64, dungeon: &str) {
    db.execute(
        "INSERT INTO runs(id,session_id,dungeon,started,complete_capture,json)
         VALUES(?1,'session',?2,?3,1,'{}')",
        params![id, dungeon, started],
    )
    .unwrap();
}

fn fight(db: &Connection, id: &str, run_id: &str, kind: &str, boss_type: Option<i64>) {
    db.execute(
        "INSERT INTO fights(id,run_id,session_id,started,boss_type,complete_capture,json)
         VALUES(?1,?2,'session',150,?3,1,'{}')",
        params![id, run_id, boss_type],
    )
    .unwrap();
    db.execute(
        "INSERT INTO fight_summaries VALUES(?1,?2,150,?3)",
        params![id, run_id, json!({"target_kind": kind}).to_string()],
    )
    .unwrap();
}

fn player(db: &Connection, fight_id: &str, id: &str, object_id: i32, damage: f64, details: Value) {
    db.execute(
        "INSERT INTO fight_players(fight_id,player_id,object_id,damage,json) VALUES(?1,?2,?3,?4,?5)",
        params![fight_id, id, object_id, damage, details.to_string()],
    )
    .unwrap();
}

fn details(name: Option<&str>, is_local: bool, deaths: i64) -> Value {
    json!({"name": name, "class_id": 782, "is_local": is_local, "hits": 1, "deaths": deaths})
}

fn event(db: &Connection, id: &str, run_id: &str, at: i64, kind: &str, payload: Value) {
    db.execute(
        "INSERT INTO instance_events VALUES(?1,'session',?2,?3,?4,?5)",
        params![id, run_id, at, kind, json!({"kind": payload}).to_string()],
    )
    .unwrap();
}

fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn members(db: &Connection, run_id: &str) -> Vec<Value> {
    let projection =
        "SELECT json_object('key',key,'run_id',run_id,'player_id',player_id,'class_id',class_id,
            'joined',joined,'departed_at',departed_at,'left_early',left_early,'died',died,
            'damage',damage,'share',share,'boss_share',boss_share) FROM";
    let read = |source: &str| -> Vec<Value> {
        db.prepare(&format!("{projection} {source} ORDER BY key,player_id"))
            .unwrap()
            .query_map([run_id], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
            .collect()
    };
    let stored = read("party_run_members WHERE run_id=?1");
    let rebuilt = read(&format!("({})", maxed_store::party::RUN_MEMBERS_SQL));
    assert_eq!(rebuilt, stored, "reusable query must match the backfill");
    stored
}

#[test]
fn migration_014_from_013_and_fresh() {
    for legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        let mut db = Connection::open(&path).unwrap();
        if legacy {
            install_013(&db);
        }
        migrate(&mut db).unwrap();
        assert_eq!(
            open_reader(&path).unwrap().user_version().unwrap(),
            maxed_store::SCHEMA_VERSION
        );
        for table in ["party_players", "party_run_members"] {
            assert_eq!(count(&db, table), 0);
        }
        for (index, table, column) in [
            ("party_players_last_seen", "party_players", "last_seen"),
            ("party_run_members_run", "party_run_members", "run_id"),
        ] {
            let indexed: (String, String) = db
                .query_row(
                    "SELECT s.tbl_name,i.name FROM sqlite_schema s,pragma_index_info(s.name) i
                     WHERE s.type='index' AND s.name=?1",
                    [index],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(indexed, (table.into(), column.into()));
        }
        // Read at runtime so the RED test can compile before migration 014 exists.
        let script = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/014_party.sql"
        ))
        .unwrap();
        assert!(
            !script.contains('\r'),
            "migration checksums require LF bytes"
        );
        for (i, script) in LEGACY_SCRIPTS
            .iter()
            .copied()
            .chain([script.as_str()])
            .enumerate()
        {
            let checksum: String = db
                .query_row(
                    "SELECT checksum FROM schema_migrations WHERE version=?1",
                    [i as i64 + 1],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(checksum, format!("{:x}", Sha256::digest(script.as_bytes())));
        }
        let changes = db.total_changes();
        migrate(&mut db).unwrap();
        assert_eq!(db.total_changes(), changes);
        assert_eq!(count(&db, "schema_migrations"), maxed_store::SCHEMA_VERSION);
    }
}

#[test]
fn backfill_builds_members_and_players() {
    // Each arm of the existing boss predicate must contribute to boss share.
    for (kind, boss_type) in [("boss", None), ("event_boss", None), ("enemy", Some(900))] {
        let mut db = legacy();
        run(&db, "run-2", 2000, "Synthetic Later Dungeon");
        run(&db, "run-1", 100, "Synthetic Dungeon");
        fight(&db, "boss", "run-1", kind, boss_type);
        fight(&db, "trash", "run-1", "enemy", None);
        fight(&db, "later", "run-2", "enemy", None);
        player(
            &db,
            "boss",
            "alpha",
            101,
            60.,
            details(Some("SynthAlpha,fe14"), false, 0),
        );
        player(
            &db,
            "trash",
            "alpha",
            101,
            20.,
            details(Some("SynthAlpha,fe14"), false, 0),
        );
        player(
            &db,
            "later",
            "alpha-later",
            201,
            0.,
            details(Some("synthalpha"), false, 0),
        );
        player(
            &db,
            "trash",
            "bravo",
            102,
            100.,
            details(Some("SynthBravo"), false, 0),
        );
        player(
            &db,
            "boss",
            "charlie",
            103,
            50.,
            details(Some("SynthCharlie"), false, 1),
        );
        for fight_id in ["boss", "trash"] {
            player(
                &db,
                fight_id,
                "local",
                104,
                0.,
                details(Some("SynthLocal"), true, 0),
            );
        }
        for (id, object_id, name) in [
            ("unnamed", 105, None),
            ("empty", 106, Some("")),
            ("suffix", 107, Some(",fe14")),
        ] {
            player(&db, "trash", id, object_id, 0., details(name, false, 0));
        }
        db.execute(
            "UPDATE pd_run_players SET damage_correction=20 WHERE player_id='alpha'",
            [],
        )
        .unwrap();
        for (id, at) in [("late-left", 500), ("early-left", 300)] {
            event(
                &db,
                id,
                "run-1",
                at,
                "player_left",
                json!({"object_id": 102}),
            );
        }
        event(
            &db,
            "other-run",
            "run-2",
            200,
            "player_left",
            json!({"object_id": 102}),
        );
        event(
            &db,
            "other-kind",
            "run-1",
            200,
            "player_entered",
            json!({"object_id": 102}),
        );
        migrate(&mut db).unwrap();

        let rows = members(&db, "run-1");
        for (row, expected) in rows.iter().zip([60. / 110., 0., 50. / 110.]) {
            assert!((row["boss_share"].as_f64().unwrap() - expected).abs() < 1e-9);
        }
        let mut without_boss_share = rows.clone();
        for row in &mut without_boss_share {
            row.as_object_mut().unwrap().remove("boss_share");
        }
        assert_eq!(
            without_boss_share,
            vec![
                json!({"key":"synthalpha","run_id":"run-1","player_id":"alpha","class_id":782,
                "joined":100,"departed_at":null,"left_early":0,"died":0,"damage":100.,"share":0.4}),
                json!({"key":"synthbravo","run_id":"run-1","player_id":"bravo","class_id":782,
                "joined":100,"departed_at":300,"left_early":1,"died":0,"damage":100.,"share":0.4}),
                json!({"key":"synthcharlie","run_id":"run-1","player_id":"charlie","class_id":782,
                "joined":100,"departed_at":null,"left_early":0,"died":1,"damage":50.,"share":0.2}),
            ]
        );
        assert!(
            (rows
                .iter()
                .map(|r| r["share"].as_f64().unwrap())
                .sum::<f64>()
                - 1.)
                .abs()
                < 1e-9
        );
        assert_eq!(
            members(&db, "run-2"),
            vec![json!({
                "key":"synthalpha","run_id":"run-2","player_id":"alpha-later","class_id":782,
                "joined":2000,"departed_at":null,"left_early":0,"died":0,"damage":0.,"share":null,"boss_share":null
            })]
        );
        assert_eq!(count(&db, "party_run_members"), 4);
        assert_eq!(count(&db, "party_players"), 3);
        let alpha: String = db.query_row(
            "SELECT json_array(name,first_seen,last_seen,last_map,updated,last_scope_id,last_profile_json,mark,note)
             FROM party_players WHERE key='synthalpha'", [], |r| r.get(0)).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&alpha).unwrap(),
            json!([
                "synthalpha",
                100,
                2000,
                "Synthetic Later Dungeon",
                2000,
                null,
                null,
                null,
                null
            ])
        );
        let changes = db.total_changes();
        migrate(&mut db).unwrap();
        assert_eq!(db.total_changes(), changes);
        assert_eq!(members(&db, "run-1"), rows);
    }
}

#[test]
fn backfill_departure_matches_by_name_when_object_id_missing() {
    let mut db = legacy();
    run(&db, "run-1", 100, "Synthetic Dungeon");
    run(&db, "run-2", 2000, "Synthetic Later Dungeon");
    fight(&db, "trash", "run-1", "enemy", None);
    player(
        &db,
        "trash",
        "alpha",
        101,
        10.,
        details(Some("SynthAlpha,fe14"), false, 0),
    );
    player(
        &db,
        "trash",
        "bravo",
        102,
        10.,
        details(Some("SynthBravo"), false, 0),
    );
    for (id, at) in [("late", 500), ("early", 300)] {
        event(
            &db,
            id,
            "run-1",
            at,
            "player_left",
            json!({"player_name": "SYNTHALPHA,ab,tail"}),
        );
    }
    event(
        &db,
        "wrong-run",
        "run-2",
        200,
        "player_left",
        json!({"player_name": "synthalpha"}),
    );
    event(
        &db,
        "wrong-kind",
        "run-1",
        200,
        "player_entered",
        json!({"player_name": "synthalpha"}),
    );
    migrate(&mut db).unwrap();
    let rows = members(&db, "run-1");
    assert_eq!(rows[0]["departed_at"], 300);
    assert_eq!(rows[0]["left_early"], 1);
    assert_eq!(rows[1]["departed_at"], Value::Null);
    assert_eq!(rows[1]["left_early"], 0);
}

#[test]
fn backfill_without_boss_fights_leaves_boss_share_null() {
    for zero_damage_boss in [false, true] {
        let mut db = legacy();
        run(&db, "run-1", 100, "Synthetic Dungeon");
        fight(&db, "trash", "run-1", "enemy", None);
        player(
            &db,
            "trash",
            "alpha",
            101,
            10.,
            details(Some("SynthAlpha,fe14"), false, 0),
        );
        player(
            &db,
            "trash",
            "local",
            102,
            30.,
            details(Some("SynthLocal"), true, 0),
        );
        if zero_damage_boss {
            fight(&db, "boss", "run-1", "boss", None);
            player(
                &db,
                "boss",
                "alpha",
                101,
                0.,
                details(Some("SynthAlpha,fe14"), false, 0),
            );
        }
        migrate(&mut db).unwrap();
        let rows = members(&db, "run-1");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["boss_share"], Value::Null);
        assert_eq!(rows[0]["share"], 0.25);
        let name: String = db
            .query_row("SELECT name FROM party_players", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "SynthAlpha");
        assert_eq!(
            rows[0]["key"],
            maxed_types::party::name_key("SynthAlpha,fe14")
        );
    }
}

#[test]
fn backfill_rejoin_only_departs_the_matching_object() {
    for first_fought in [false, true] {
        let mut db = legacy();
        run(&db, "run-1", 100, "Synthetic Dungeon");
        fight(&db, "fight", "run-1", "boss", None);
        if first_fought {
            player(
                &db,
                "fight",
                "alpha",
                101,
                10.,
                details(Some("SynthAlpha,first"), false, 0),
            );
        } else {
            db.execute(
                "INSERT INTO player_profiles VALUES('run-1','alpha',101,0,'run-1',100,200,'{}')",
                [],
            )
            .unwrap();
        }
        player(
            &db,
            "fight",
            "alpha-rejoin",
            202,
            20.,
            details(Some("SYNTHALPHA,rejoin"), false, 0),
        );
        event(
            &db,
            "alpha-left",
            "run-1",
            300,
            "player_left",
            json!({"object_id":101,"player_name":"SynthAlpha"}),
        );
        migrate(&mut db).unwrap();
        let rows = members(&db, "run-1");
        let rejoin = rows
            .iter()
            .find(|r| r["player_id"] == "alpha-rejoin")
            .unwrap();
        assert_eq!(rejoin["departed_at"], Value::Null);
        assert_eq!(rejoin["left_early"], 0);
        if first_fought {
            assert_eq!(rows[0]["departed_at"], 300);
            assert_eq!(rows[0]["left_early"], 1);
        }
    }
}
