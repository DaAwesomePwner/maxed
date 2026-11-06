use maxed_store::{migrate, open_reader};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

const LEGACY_SCRIPTS: [&str; 14] = [
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
    include_str!("../migrations/014_party.sql"),
];

#[test]
fn fresh_and_v14_databases_get_indexes_without_rewriting_history() {
    for legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        let mut db = Connection::open(&path).unwrap();
        if legacy {
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
            db.pragma_update(None, "user_version", 14).unwrap();
            db.execute_batch(
                "INSERT INTO play_sessions VALUES('session',0,100,NULL,'{}');
                INSERT INTO runs(id,session_id,dungeon,started,ended,outcome,complete_capture,json)
                    VALUES('run','session','Synthetic Dungeon',0,100,'left',1,'preserved run');
                INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json)
                    VALUES('white','session','run',50,400,255,'preserved white'),
                          ('other','session','run',60,400,10,'preserved other');",
            )
            .unwrap();
        }
        let changes = db.total_changes();
        migrate(&mut db).unwrap();
        assert_eq!(
            open_reader(&path).unwrap().user_version().unwrap(),
            maxed_store::SCHEMA_VERSION
        );
        if legacy {
            assert_eq!(
                db.total_changes() - changes,
                2,
                "only the migration ledger changes"
            );
            assert_eq!(
                db.query_row("SELECT json FROM runs", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "preserved run"
            );
            let drops: Vec<String> = db
                .prepare("SELECT json FROM drops ORDER BY at")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(drops, ["preserved white", "preserved other"]);
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM drops INDEXED BY drops_white_runs WHERE bag_rank=255",
                    [],
                    |r| r.get::<_, u32>(0)
                )
                .unwrap(),
                1
            );
        }
        for (index, expected) in [
            (
                "runs_loot_stats",
                vec!["dungeon", "started", "id", "ended", "outcome"],
            ),
            ("runs_id_dungeon", vec!["id", "dungeon"]),
            ("drops_white_runs", vec!["run_id", "at"]),
        ] {
            let columns: Vec<String> = db
                .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                .unwrap()
                .query_map([index], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(columns, expected);
        }
        assert!(db
            .query_row(
                "SELECT partial FROM pragma_index_list('drops') WHERE name='drops_white_runs'",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
        let script = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations/015_loot_stats_indexes.sql"
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
        assert_eq!(
            db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
}
