//! Trace the production reader so plan assertions cannot drift from executed SQL.
use maxed_types::*;
use std::cell::RefCell;

thread_local! {
    static STATEMENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn trace(event: rusqlite::trace::TraceEvent<'_>) {
    if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
        STATEMENTS.with(|s| s.borrow_mut().push(statement.expanded_sql().unwrap()));
    }
}

fn reader() -> (tempfile::TempDir, crate::StoreReader) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic-plans.sqlite");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    crate::migrate(&mut db).unwrap();
    let reader = crate::open_reader(&path).unwrap();
    reader.conn.trace_v2(
        rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
        Some(trace),
    );
    STATEMENTS.with(|s| s.borrow_mut().clear());
    (dir, reader)
}

fn plan(reader: &crate::StoreReader, sql: &str) -> String {
    reader
        .conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap()
        .query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
}

fn seek(plan: &str, index: &str) {
    assert!(
        plan.lines()
            .any(|s| s.contains("SEARCH") && s.contains(index)),
        "requires {index}:\n{plan}"
    );
}

fn no_base_scan(plan: &str) {
    for table in ["fights", "fight_players", "runs", "f", "r"] {
        assert!(
            !plan
                .lines()
                .any(|s| s == format!("SCAN {table}") || s.starts_with(&format!("SCAN {table} "))),
            "base scan {table}:\n{plan}"
        );
    }
}

#[test]
fn bounded_boss_history_seeks() {
    let (_dir, reader) = reader();
    for target_types in [None, Some(vec![42]), Some(vec![])] {
        reader
            .boss_history(BossHistoryRequest {
                range: Some(TimeRange {
                    start: Timestamp(10),
                    end: Timestamp(100),
                }),
                character_id: Some("synthetic-character".into()),
                target_types,
            })
            .unwrap();
        let statements = STATEMENTS.with(|s| s.take());
        assert_eq!(statements.len(), 1);
        let plan = plan(&reader, &statements[0]);
        seek(&plan, "fights_started");
        assert!(
            plan.lines()
                .any(|s| s.contains("SEARCH fight_players") && s.contains("fight_id=?")),
            "{plan}"
        );
        no_base_scan(&plan);
    }
}

#[test]
fn bounded_dungeon_profile_seeks_both_scopes() {
    let (_dir, reader) = reader();
    reader
        .dungeon_profile(
            DungeonProfileRequest {
                dungeon: "Synthetic Dungeon".into(),
                character_id: Some("synthetic-character".into()),
                range: TimeRange {
                    start: Timestamp(10),
                    end: Timestamp(100),
                },
            },
            Some("account-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        )
        .unwrap();
    let statements = STATEMENTS.with(|s| s.take());
    let sql = statements
        .iter()
        .find(|s| s.contains("cleared_runs AS"))
        .unwrap();
    let plan = plan(&reader, sql);
    for index in ["runs_dungeon_outcome", "fights_run", "fights_started"] {
        seek(&plan, index);
    }
    no_base_scan(&plan);
}

#[test]
fn bounded_home_boss_baselines_seek_without_full_history_work() {
    let (_dir, reader) = reader();
    for target_types in [None, Some(vec![42]), Some((0..64).collect()), Some(vec![])] {
        reader
            .home_boss_baselines(
                BossHistoryRequest {
                    range: Some(TimeRange {
                        start: Timestamp(10),
                        end: Timestamp(100),
                    }),
                    character_id: Some("synthetic-character".into()),
                    target_types,
                },
                Some("account-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            )
            .unwrap();
        let statements = STATEMENTS.with(|s| s.take());
        assert_eq!(statements.len(), 1);
        for heavy in [
            "fight_players",
            "fight_summaries",
            "ROW_NUMBER",
            "party_avg",
        ] {
            assert!(
                !statements[0].contains(heavy),
                "lean baseline includes {heavy}"
            );
        }
        let plan = plan(&reader, &statements[0]);
        seek(&plan, "fights_started");
        no_base_scan(&plan);
    }
}
