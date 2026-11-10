//! Bounded additions to the Runs page, evaluated in its read snapshot.
use crate::{
    project::tag,
    queries::{identifier, range},
    StoreError,
};
use maxed_types::{RunFilter, RunListSummary, RunRowDetail, RunView};
use rusqlite::{params_from_iter, types::Value, Connection};

const METRICS: &str = include_str!("run_metrics.sql");

fn detail_sql() -> String {
    crate::account_sql::with_accounts(&format!(
        "WITH selected AS MATERIALIZED (SELECT r.*,j.key AS ordinal
        FROM json_each(?1) j CROSS JOIN runs r ON r.id=j.value)\n{METRICS}\n{}",
        include_str!("run_rows.sql")
    ))
}

pub(crate) fn details(
    conn: &Connection,
    runs: &[RunView],
) -> Result<Vec<RunRowDetail>, StoreError> {
    if runs.is_empty() {
        return Ok(Vec::new());
    }
    let ids = runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>();
    for id in &ids {
        identifier(id)?;
    }
    let json: Option<String> =
        conn.query_row(&detail_sql(), [serde_json::to_string(&ids)?], |r| r.get(0))?;
    Ok(serde_json::from_str(&json.ok_or(StoreError::Limit)?)?)
}

fn summary_sql(filter: &RunFilter) -> Result<(String, Vec<Value>), StoreError> {
    let mut args = Vec::<Value>::new();
    let mut common = vec!["1=1".to_owned()];
    if let Some(dungeon) = &filter.dungeon {
        identifier(dungeon)?;
        args.push(dungeon.clone().into());
        common.push(format!("dungeon=?{}", args.len()));
    }
    if let Some(time) = &filter.range {
        let (start, end) = range(time)?;
        args.push(start.into());
        common.push(format!("started>=?{}", args.len()));
        args.push(end.into());
        common.push(format!("started<?{}", args.len()));
    }
    let mut counts = common.clone();
    let mut selected = common.clone();
    if let Some(character) = &filter.character_id {
        identifier(character)?;
        args.push(character.clone().into());
        let clause = format!("character_id=?{}", args.len());
        counts.push(clause.clone());
        selected.push(clause);
    }
    if let Some(outcome) = &filter.outcome {
        args.push(tag(outcome)?.into());
        let clause = format!("outcome=?{}", args.len());
        common.push(clause.clone());
        selected.push(clause);
    }
    let tail = include_str!("run_list_summary.sql")
        .replace("/*counts*/", &counts.join(" AND "))
        .replace("/*characters*/", &common.join(" AND "));
    Ok((
        crate::account_sql::with_accounts(&format!(
            "WITH selected AS MATERIALIZED (SELECT id,session_id,character_id,dungeon,started,ended,completed_at FROM runs WHERE {})\n{METRICS}\n{tail}",
            selected.join(" AND ")
        )),
        args,
    ))
}

pub(crate) fn summary(
    conn: &Connection,
    filter: &RunFilter,
) -> Result<Option<RunListSummary>, StoreError> {
    if filter.cursor.is_some() {
        return Ok(None);
    }
    let (sql, args) = summary_sql(filter)?;
    let json: Option<String> = conn.query_row(&sql, params_from_iter(args), |r| r.get(0))?;
    Ok(Some(serde_json::from_str(&json.ok_or(StoreError::Limit)?)?))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use maxed_types::{RunSort, TimeRange, Timestamp};

    fn plan(conn: &Connection, sql: &str, args: Vec<Value>) -> String {
        conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map(params_from_iter(args), |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
    }

    fn has_child_scan(explained: &str) -> bool {
        explained.lines().any(|line| {
            line.split("SCAN ").skip(1).any(|scan| {
                [
                    "fights",
                    "fight_players",
                    "fame_samples",
                    "drops",
                    "fight",
                    "drop_row",
                    "p",
                    "fp",
                    "characters",
                    "character_aliases",
                    "account_source",
                    "account_target",
                    "run_character",
                    "ch",
                    "run_evidence",
                    "e",
                ]
                .contains(&scan.split_whitespace().next().unwrap_or_default())
            })
        })
    }

    #[test]
    fn child_scan_guard_rejects_prefixed_plan_lines() {
        for table in [
            "fights",
            "fight_players",
            "fame_samples",
            "drops",
            "fight",
            "drop_row",
            "p",
            "fp",
            "characters",
            "character_aliases",
            "account_source",
            "account_target",
            "run_character",
            "ch",
            "run_evidence",
            "e",
        ] {
            for prefix in ["", "0 12 0 ", "QUERY PLAN: ", "|--"] {
                assert!(has_child_scan(&format!(
                    "{prefix}SCAN {table} USING INDEX example"
                )));
            }
        }
        assert!(!has_child_scan(
            "SCAN fame\nSCAN page_fights\nSEARCH fame_samples USING INDEX fame_character"
        ));
    }

    pub(crate) fn assert_plans(conn: &Connection, statements: &[String]) {
        for (index, sql) in statements.iter().enumerate() {
            let explained = plan(conn, sql, vec![]);
            assert!(
                explained.contains("SEARCH fame_samples USING INDEX fame_character"),
                "{explained}"
            );
            assert!(!has_child_scan(&explained), "{explained}");
            assert!(
                explained
                    .contains("SEARCH r USING INDEX pd_runs_comparison (dungeon=? AND outcome=?)"),
                "all-time clears must seek the selected dungeons: {explained}"
            );
            assert!(
                explained.contains("SEARCH e EXISTS USING COVERING INDEX pd_run_evidence_kind"),
                "clear evidence must use the run-scoped index: {explained}"
            );
            if index == 0 {
                // Page details seek physical runs; only the summary aggregates
                // may scan the full run history for unfiltered totals.
                assert!(!explained.contains("SCAN runs"), "{explained}");
                assert_eq!(explained.matches("MATERIALIZE all_time_clears").count(), 1);
                for search in [
                    "SEARCH fight USING INDEX fights_run",
                    "SEARCH drop_row USING INDEX drops_run",
                    "SEARCH p USING INDEX",
                    "SEARCH previous_clear USING AUTOMATIC COVERING INDEX (dungeon=? AND account_id=?)",
                ] {
                    assert!(explained.contains(search), "missing {search}:\n{explained}");
                }
            } else {
                assert!(
                    !sql.contains("FROM fights")
                        && !sql.contains("FROM drops")
                        && !sql.contains("fight_players")
                );
            }
        }
    }

    pub(crate) fn seed_identity_history(db: &Connection) {
        // Keep ownership tables large enough that EXPLAIN tests real lookup costs,
        // rather than SQLite's deliberate scans of a two-row character fixture.
        db.execute_batch("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<1000)
            INSERT INTO characters(id,observed_at,json) SELECT 'history-character-'||i,0,'{}' FROM n;
            INSERT INTO character_aliases(provisional_id,verified_id)
                SELECT id,'history-character-1000' FROM characters
                WHERE id LIKE 'history-character-%' AND id<>'history-character-1000';
            ANALYZE characters;
            ANALYZE character_aliases;").unwrap();
    }

    #[test]
    fn fifty_thousand_run_summary_and_page_details_use_child_indexes() {
        use std::{cell::RefCell, time::Instant};
        thread_local! {
            static SQL: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        }
        fn capture(event: rusqlite::trace::TraceEvent<'_>) {
            if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
                if statement.sql().starts_with("SELECT") || statement.sql().starts_with("WITH") {
                    SQL.with(|sql| sql.borrow_mut().push(statement.expanded_sql().unwrap()));
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.db");
        let mut db = Connection::open(&path).unwrap();
        crate::migrate(&mut db).unwrap();
        db.execute_batch("INSERT INTO play_sessions(id,started,last_traffic,json) VALUES('synthetic',0,3000060000000,'{}');
            INSERT INTO accounts VALUES('synthetic-a'),('synthetic-b');
            INSERT INTO characters(id,observed_at,json,account_id) VALUES('test-mage',0,'{}','synthetic-a'),('demo-ally',0,'{}','synthetic-b');").unwrap();
        seed_identity_history(&db);
        // One run each minute over ~35 days, two characters, ten dungeons.
        // Populate children too: an empty child table can hide a bad query plan.
        db.execute("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<50000)
            INSERT INTO runs(id,session_id,character_id,dungeon,started,ended,completed_at,outcome,complete_capture,json)
            SELECT 'run-'||i,'synthetic',CASE WHEN i%2=1 THEN 'test-mage' ELSE 'demo-ally' END,
                'Dungeon '||(i%10),i*60000000,i*60000000+30000000,i*60000000+30000000,'cleared',1,
                json_set(?1,'$.id','run-'||i,'$.session_id','synthetic',
                    '$.character_id',CASE WHEN i%2=1 THEN 'test-mage' ELSE 'demo-ally' END,
                    '$.dungeon','Dungeon '||(i%10),'$.started',CAST(i*60000000 AS TEXT),
                    '$.ended',CAST(i*60000000+30000000 AS TEXT),
                    '$.completed_at',CAST(i*60000000+30000000 AS TEXT),'$.issues',json('[]')) FROM n",
            [serde_json::to_string(&maxed_types::demo::demo_run_story().run).unwrap()]).unwrap();
        db.execute_batch(r#"
            INSERT INTO run_evidence(run_id,ordinal,at,kind,json) SELECT id,0,completed_at,'victory','{}' FROM runs;
            INSERT INTO fights(id,run_id,session_id,started,ended,complete_capture,json)
                SELECT 'fight-'||id,id,session_id,started,ended,1,'{"issues":[],"total_damage":100.0}' FROM runs;
            INSERT INTO fight_players(fight_id,player_id,object_id,damage,json)
                SELECT id,'test-mage',1,40.0,'{"is_local":true,"share":0.4,"hits":1,"deaths":0,"sources":[]}' FROM fights;
            INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json)
                SELECT 'drop-'||id,session_id,id,ended,1,1,'{"issues":[]}' FROM runs;
            INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame)
                SELECT 'synthetic',rowid*2,character_id,started,100 FROM runs
                UNION ALL SELECT 'synthetic',rowid*2+1,character_id,ended,110 FROM runs;
            ANALYZE;
        "#).unwrap();
        let reader = crate::open_reader(&path).unwrap();
        reader.conn.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
            Some(capture),
        );
        let all = RunFilter {
            character_id: None,
            dungeon: None,
            range: None,
            outcome: None,
            sort: RunSort::Newest,
            cursor: None,
            limit: 100,
        };
        let week = TimeRange {
            start: Timestamp((50_001 - 10_080) * 60_000_000),
            end: Timestamp(50_001 * 60_000_000),
        };
        for (label, query, expected) in [
            ("no filter", all.clone(), 50_000),
            (
                "character only",
                RunFilter {
                    character_id: Some("test-mage".into()),
                    ..all.clone()
                },
                25_000,
            ),
            (
                "7-day range",
                RunFilter {
                    range: Some(week.clone()),
                    ..all.clone()
                },
                10_080,
            ),
            (
                "character and 7-day range",
                RunFilter {
                    character_id: Some("test-mage".into()),
                    range: Some(week),
                    ..all.clone()
                },
                5_040,
            ),
            (
                "combined",
                RunFilter {
                    character_id: Some("test-mage".into()),
                    dungeon: Some("Dungeon 1".into()),
                    outcome: Some(maxed_types::Outcome::Cleared),
                    ..all
                },
                5_000,
            ),
        ] {
            let started = Instant::now();
            let page = reader.list_runs(query.clone()).unwrap();
            eprintln!("50k list_runs {label}: {:?}", started.elapsed());
            assert_eq!(page.summary.as_ref().unwrap().runs, expected);
            assert_eq!(page.rows.len(), 100);
            assert_eq!(page.details.as_ref().unwrap().len(), 100);
            assert!(page
                .details
                .as_ref()
                .unwrap()
                .iter()
                .all(|row| row.best_clear_micros == Some(30_000_000)));
            if label == "no filter" {
                assert_eq!(page.summary.as_ref().unwrap().personal_bests, 10);
            }
            assert!(serde_json::to_vec(&page).unwrap().len() < 1024 * 1024);
            let statements = SQL.with(|sql| std::mem::take(&mut *sql.borrow_mut()));
            assert_eq!(statements.len(), 3);
            assert_plans(&db, &statements[1..]);
            if label == "no filter" {
                let mut next = query.clone();
                next.cursor = page.next.clone();
                assert!(next.cursor.is_some());
                let started = Instant::now();
                let second = reader.list_runs(next).unwrap();
                eprintln!("50k list_runs cursor second page: {:?}", started.elapsed());
                assert!(second.summary.is_none());
                assert_eq!(second.rows.len(), 100);
                assert_eq!(second.details.as_ref().unwrap().len(), 100);
                assert_ne!(second.rows[0].id, page.rows[0].id);
                let statements = SQL.with(|sql| std::mem::take(&mut *sql.borrow_mut()));
                assert_eq!(statements.len(), 2);
                assert_plans(&db, &statements[1..]);
            }
            if query.range.is_some() {
                for sql in [&statements[0], &statements[2]] {
                    let explained = plan(&db, sql, vec![]);
                    assert!(
                        explained.contains(
                            "SEARCH runs USING INDEX runs_started (started>? AND started<?)"
                        ),
                        "{explained}"
                    );
                }
            }
        }
    }
}
