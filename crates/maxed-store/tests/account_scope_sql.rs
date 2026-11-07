use crate::common::account_sql::with_accounts;
use maxed_types::{HomeScope, TimeRange, Timestamp};
use rusqlite::{params, Connection};

fn db(path: &std::path::Path) -> Connection {
    let mut db = Connection::open(path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    db.execute_batch("INSERT INTO accounts VALUES('synthetic-a'),('synthetic-b');
        INSERT INTO characters(id,observed_at,json,account_id) VALUES
        ('a',0,'{}','synthetic-a'),('b',0,'{}','synthetic-b'),('unknown',0,'{}',NULL),('alias',0,'{}',NULL);
        INSERT INTO character_aliases VALUES('alias','a'),('missing','a');
        INSERT INTO play_sessions VALUES('s',0,1000,1000,'{}');").unwrap();
    db
}

#[test]
fn account_scope_sql_slots_match_017_views_and_missing_provisional_row() {
    let dir = tempfile::tempdir().unwrap();
    let db = db(&dir.path().join("history"));
    for (index, character) in [Some("a"), Some("b"), Some("unknown"), Some("alias"), None]
        .into_iter()
        .enumerate()
    {
        db.execute("INSERT INTO runs(id,session_id,character_id,dungeon,started,complete_capture,json) VALUES(?1,'s',?2,'D',0,1,'{}')", params![index.to_string(),character]).unwrap();
    }
    for (index, character) in [
        Some("a"),
        Some("b"),
        Some("unknown"),
        Some("alias"),
        Some("missing"),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        for run in [None, Some("0"), Some("2")] {
            let id = format!("{index}-{run:?}");
            db.execute("INSERT INTO fights(id,session_id,run_id,started,complete_capture,json) VALUES(?1,'s',?2,100,1,'{}')", params![id,run]).unwrap();
            db.execute("INSERT INTO drops(id,session_id,run_id,at,bag_type,json) VALUES(?1,'s',?2,100,1,'{}')", params![id,run]).unwrap();
            for kind in ["fight", "drop"] {
                db.execute(
                    "INSERT INTO observation_characters VALUES(?1,?2,?3)",
                    params![kind, id, character],
                )
                .unwrap();
            }
        }
    }
    for (table, view, slot) in [
        ("characters", "character_accounts", "character:t.id"),
        ("runs", "run_accounts", "character:t.character_id"),
        ("fights", "fight_accounts", "fight:t"),
        ("drops", "drop_accounts", "drop:t"),
    ] {
        let sql = with_accounts(&format!("SELECT t.id FROM {table} t JOIN {view} v ON v.id=t.id WHERE v.account_id IS NOT /*account:{slot}*/"));
        assert!(!db.prepare(&sql).unwrap().exists([]).unwrap(), "{table}");
    }
    assert_eq!(
        db.query_row(
            "SELECT account_id FROM fight_accounts WHERE id=?1",
            ["4-None"],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        None
    );
    let notice = with_accounts("SELECT /*unattributed_history*/");
    let check = || {
        let expected: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM run_accounts WHERE account_id IS NULL) OR EXISTS(SELECT 1 FROM fight_accounts WHERE account_id IS NULL) OR EXISTS(SELECT 1 FROM drop_accounts WHERE account_id IS NULL)", [], |r| r.get(0)).unwrap();
        assert_eq!(
            db.query_row(&notice, [], |r| r.get::<_, bool>(0)).unwrap(),
            expected
        );
    };
    check();
    db.execute_batch("UPDATE characters SET account_id='synthetic-a' WHERE account_id IS NULL; UPDATE runs SET character_id='a' WHERE character_id IS NULL; UPDATE observation_characters SET character_id='a';").unwrap();
    check();
    assert!(!db.query_row(&notice, [], |r| r.get::<_, bool>(0)).unwrap());
    for mutation in [
        "UPDATE observation_characters SET observation_id='future' WHERE kind='fight' AND observation_id='4-None'",
        "UPDATE observation_characters SET observation_id='4-None' WHERE kind='fight' AND observation_id='future'",
        "DELETE FROM observation_characters WHERE kind='fight' AND observation_id='4-None'",
        "INSERT INTO observation_characters VALUES('fight','4-None','missing')",
        "INSERT INTO characters(id,observed_at,json) VALUES('missing',0,'{}')",
        "DELETE FROM characters WHERE id='missing'",
        "DELETE FROM fights WHERE id='4-None'",
        "INSERT INTO drops(id,session_id,at,bag_type,json) VALUES('legacy','s',100,1,'{}')",
        "UPDATE drops SET run_id='0' WHERE id='legacy'",
        "UPDATE drops SET run_id=NULL WHERE id='legacy'",
        "DELETE FROM drops WHERE id='legacy'",
    ] {
        db.execute_batch(mutation).unwrap();
        check();
    }
}

#[test]
fn account_scope_notice_fully_attributed_large_history_seeks() {
    notice_large_history(false);
}

#[test]
#[ignore = "release Home 50ms gate; run serially with --ignored --nocapture"]
fn account_scope_notice_fully_attributed_large_history_warm_gate() {
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    notice_large_history(true);
}

fn notice_large_history(timing: bool) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history");
    let db = db(&path);
    db.execute_batch("BEGIN;
        CREATE TEMP TABLE n(i INTEGER PRIMARY KEY);
        WITH RECURSIVE seq(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM seq WHERE i<200000) INSERT INTO n SELECT i FROM seq;
        INSERT INTO observation_characters SELECT 'fight',CAST(i AS TEXT),'a' FROM n WHERE i<=50000;
        INSERT INTO observation_characters SELECT 'drop',CAST(i AS TEXT),'a' FROM n;
        INSERT INTO fights(id,session_id,started,complete_capture,json) SELECT CAST(i AS TEXT),'s',100,1,'{}' FROM n WHERE i<=50000;
        INSERT INTO drops(id,session_id,at,bag_type,json) SELECT CAST(i AS TEXT),'s',100,1,'{}' FROM n;
        DROP TABLE n; COMMIT; ANALYZE;").unwrap();
    let sql = with_accounts("EXPLAIN QUERY PLAN SELECT /*unattributed_history*/");
    let plan = db
        .prepare(&sql)
        .unwrap()
        .query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    eprintln!("account_scope notice 250k observations:\n{plan}");
    for alias in [
        "fights",
        "drops",
        "observation_characters",
        "characters",
        "character_aliases",
        "f",
        "d",
        "o",
        "unknown_character",
        "unknown_alias",
        "unknown_observation",
        "alias_source",
    ] {
        assert!(
            !plan.lines().any(|line| line == format!("SCAN {alias}")
                || line.starts_with(&format!("SCAN {alias} "))),
            "{plan}"
        );
    }
    let reader = maxed_store::open_reader(&path).unwrap();
    let scope = HomeScope::Today {
        range: TimeRange {
            start: Timestamp(0),
            end: Timestamp(10),
        },
    };
    assert!(
        !reader
            .home_summary(scope.clone(), Some("synthetic-a"))
            .unwrap()
            .unattributed_history
    );
    if !timing {
        return;
    }
    let mut samples = Vec::new();
    for _ in 0..30 {
        let start = std::time::Instant::now();
        assert!(
            !reader
                .home_summary(scope.clone(), Some("synthetic-a"))
                .unwrap()
                .unattributed_history
        );
        samples.push(start.elapsed());
    }
    samples.sort();
    eprintln!("account_scope fully attributed Home p95={:?}", samples[28]);
    assert!(samples[28] < std::time::Duration::from_millis(50));
}

#[test]
fn account_scope_notice_migration_backfills_existing_runless_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = db(&dir.path().join("history"));
    let triggers: Vec<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='trigger' AND name LIKE 'unresolved_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for trigger in triggers {
        db.execute_batch(&format!("DROP TRIGGER {trigger}"))
            .unwrap();
    }
    db.execute_batch("DROP TABLE unresolved_observations; DELETE FROM schema_migrations WHERE version=19; PRAGMA user_version=18;
        INSERT INTO fights(id,session_id,started,complete_capture,json) VALUES('legacy','s',100,1,'{}');
        INSERT INTO drops(id,session_id,at,bag_type,json) VALUES('legacy','s',100,1,'{}');").unwrap();
    maxed_store::migrate(&mut db).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM unresolved_observations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let notice = with_accounts("SELECT /*unattributed_history*/");
    assert!(db.query_row(&notice, [], |r| r.get::<_, bool>(0)).unwrap());
    db.execute_batch(
        "INSERT INTO observation_characters VALUES('fight','legacy','a'),('drop','legacy','a');",
    )
    .unwrap();
    assert!(!db.query_row(&notice, [], |r| r.get::<_, bool>(0)).unwrap());
}
