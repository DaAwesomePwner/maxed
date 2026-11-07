// Compile the actual private implementation here without exposing a public test API.
#[path = "../src/journey.rs"]
mod implementation;

use implementation::{chain, chain_range, in_a_row, session_visits, VisitRow};
use maxed_store::StoreError;
use maxed_types::{AreaContext, ChainStepKind, Timestamp};
use rusqlite::{params, Connection};
use std::cell::RefCell;

const CASTLE: &str = "Oryx's Castle";
const CHAMBER: &str = "Oryx's Chamber";
const SANCTUARY: &str = "Oryx's Sanctuary";

fn db() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    for id in ["synthetic-session", "other-session"] {
        db.execute(
            "INSERT INTO play_sessions VALUES(?1,0,10000,10000,'{}')",
            [id],
        )
        .unwrap();
    }
    db
}

fn visit(db: &Connection, id: &str, kind: &str, dungeon: Option<&str>, started: i64) {
    let run = dungeon.map(|_| format!("run-{id}"));
    if let Some(dungeon) = dungeon {
        db.execute(
            "INSERT INTO runs(id,session_id,dungeon,started,ended,complete_capture,json)
             VALUES(?1,'synthetic-session',?2,?3,?3+100,1,'{}')",
            params![run, dungeon, started],
        )
        .unwrap();
    }
    db.execute(
        "INSERT INTO map_visits(id,stream_id,session_id,run_id,map,kind,realm_instance_key,started,ended,source,json)
         VALUES(?1,'synthetic-stream','synthetic-session',?2,'Synthetic Map',?3,'synthetic-realm',?4,?4+100,'live','{}')",
        params![id, run, kind, started],
    ).unwrap();
    crate::common::account_scope_fixture::attribute(db);
}

fn sequence(entries: &[(&str, Option<&str>)]) -> (Connection, Vec<VisitRow>) {
    let db = db();
    for (index, (kind, dungeon)) in entries.iter().enumerate() {
        visit(
            &db,
            &format!("v{index:04}"),
            kind,
            *dungeon,
            index as i64 * 100,
        );
    }
    crate::common::account_scope_fixture::attribute(&db);
    let visits = session_visits(&db, "synthetic-session").unwrap();
    (db, visits)
}

#[test]
fn realm_castle_chamber_sanctuary_is_one_chain() {
    let (db, visits) = sequence(&[
        ("realm", None),
        ("dungeon", Some(CASTLE)),
        ("dungeon", Some(CHAMBER)),
        ("dungeon", Some(SANCTUARY)),
    ]);
    for index in 0..4 {
        assert_eq!(chain_range(&visits, index), Some(0..4));
    }
    let result = chain(&db, "synthetic-session", &visits, 0..4).unwrap();
    assert_eq!(
        result.steps.iter().map(|s| s.kind).collect::<Vec<_>>(),
        [
            ChainStepKind::Realm,
            ChainStepKind::Castle,
            ChainStepKind::Chamber,
            ChainStepKind::Sanctuary
        ]
    );
    assert_eq!(result.steps[1].name, CASTLE);
    assert_eq!(result.steps[1].run_id.as_deref(), Some("run-v0001"));
    assert_eq!(result.steps[0].visit_id, "v0000");
    assert_eq!(result.steps[0].started, Timestamp(0));
    assert_eq!(result.steps[0].ended, Some(Timestamp(100)));
}

#[test]
fn castle_without_realm_is_not_a_chain() {
    let (_, visits) = sequence(&[("dungeon", Some(CASTLE)), ("dungeon", Some(CHAMBER))]);
    assert_eq!(chain_range(&visits, 0), None);
    assert_eq!(chain_range(&visits, 1), None);
}

#[test]
fn variant_dungeons_join_the_chain() {
    for (dungeon, kind) in [
        ("Wine Cellar Short", ChainStepKind::Chamber),
        ("Oryx Mania Sanctuary", ChainStepKind::Sanctuary),
    ] {
        let (db, visits) = sequence(&[
            ("realm", None),
            ("dungeon", Some(CASTLE)),
            ("dungeon", Some(dungeon)),
        ]);
        assert_eq!(chain_range(&visits, 2), Some(0..3));
        let result = chain(&db, "synthetic-session", &visits, 0..3).unwrap();
        assert_eq!(result.steps[2].kind, kind);
        assert_eq!(result.steps[2].name, dungeon);
    }
}

#[test]
fn nexus_between_realm_and_castle_breaks_chain() {
    let (_, visits) = sequence(&[("realm", None), ("nexus", None), ("dungeon", Some(CASTLE))]);
    for index in 0..3 {
        assert_eq!(chain_range(&visits, index), None);
    }
}

#[test]
fn chamber_and_sanctuary_join_castle() {
    for chamber in [CHAMBER, "Wine Cellar"] {
        let (db, visits) = sequence(&[
            ("realm", None),
            ("dungeon", Some(CASTLE)),
            ("dungeon", Some(chamber)),
            ("dungeon", Some(SANCTUARY)),
        ]);
        assert_eq!(chain_range(&visits, 3), Some(0..4));
        let result = chain(&db, "synthetic-session", &visits, 0..4).unwrap();
        assert_eq!(result.steps[2].kind, ChainStepKind::Chamber);
        assert_eq!(result.steps[2].name, chamber);
    }
    let (_, visits) = sequence(&[
        ("realm", None),
        ("dungeon", Some(CASTLE)),
        ("dungeon", Some(SANCTUARY)),
    ]);
    assert_eq!(chain_range(&visits, 2), Some(0..3));
}

#[test]
fn nexus_after_castle_ends_chain() {
    for breaker in [
        ("nexus", None),
        ("other_hub", None),
        ("unknown", None),
        ("dungeon", Some("The Shatters")),
    ] {
        let (_, visits) = sequence(&[
            ("realm", None),
            ("dungeon", Some(CASTLE)),
            breaker,
            ("dungeon", Some(CHAMBER)),
        ]);
        assert_eq!(chain_range(&visits, 1), Some(0..2));
        assert_eq!(chain_range(&visits, 2), None);
        assert_eq!(chain_range(&visits, 3), None);
    }
}

#[test]
fn hub_visits_keep_in_a_row() {
    let (_, visits) = sequence(&[
        ("dungeon", Some("The Shatters")),
        ("nexus", None),
        ("other_hub", None),
        ("dungeon", Some("The Shatters")),
        ("nexus", None),
        ("dungeon", Some("The Shatters")),
    ]);
    assert_eq!(in_a_row(&visits, 0), 1);
    assert_eq!(in_a_row(&visits, 3), 2);
    assert_eq!(in_a_row(&visits, 5), 3);
}

#[test]
fn other_dungeon_resets_in_a_row() {
    let (_, visits) = sequence(&[
        ("dungeon", Some("The Shatters")),
        ("dungeon", Some(CASTLE)),
        ("dungeon", Some("The Shatters")),
    ]);
    assert_eq!(in_a_row(&visits, 2), 1);
}

#[test]
fn realm_between_runs_resets_in_a_row() {
    for kind in ["realm", "unknown"] {
        let (_, visits) = sequence(&[
            ("dungeon", Some("The Shatters")),
            (kind, None),
            ("dungeon", Some("The Shatters")),
        ]);
        assert_eq!(in_a_row(&visits, 2), 1);
    }
}

#[test]
fn missing_dungeons_and_invalid_indices_do_not_infer() {
    let (_, mut visits) = sequence(&[
        ("realm", None),
        ("dungeon", None),
        ("dungeon", Some(CASTLE)),
    ]);
    visits[1].map = CASTLE.into();
    assert_eq!(chain_range(&visits, 1), None);
    assert_eq!(in_a_row(&visits, 1), 0);
    assert_eq!(in_a_row(&visits, 0), 0);
    assert_eq!(chain_range(&visits, 99), None);
    assert_eq!(in_a_row(&visits, 99), 0);
    assert_eq!(chain_range(&[], 0), None);
    assert_eq!(in_a_row(&[], 0), 0);
}

fn fight(
    db: &Connection,
    id: &str,
    run: Option<&str>,
    session: &str,
    started: i64,
    local: Option<f64>,
    span: i64,
) {
    db.execute(
        "INSERT INTO fights(id,run_id,session_id,started,ended,local_dps,complete_capture,json)
         VALUES(?1,?2,?3,?4,?4+90,?5,1,json_object('first_hit',CAST(?4+1 AS TEXT),'last_hit',CAST(?4+1+?6 AS TEXT)))",
        params![id,run,session,started,local,span],
    ).unwrap();
    crate::common::account_scope_fixture::attribute(db);
    for (player, local, dps) in [
        ("local", true, Some(99999.0)),
        ("one", false, Some(1000.0)),
        ("two", false, Some(3000.0)),
        ("unknown", false, None),
    ] {
        db.execute(
            "INSERT INTO fight_players VALUES(?1,?2,1,0,?3,NULL,json_object('is_local',json(?4),'hits',0,'deaths',0))",
            params![id, player, dps, if local { "true" } else { "false" }],
        )
        .unwrap();
    }
}

#[test]
fn chain_step_aggregates_weight_by_hit_span() {
    let (db, visits) = sequence(&[("realm", None), ("dungeon", Some(CASTLE))]);
    for run in [None, Some("run-v0001")] {
        let prefix = run.unwrap_or("realm");
        let start = if run.is_some() { 100 } else { 0 };
        for (suffix, local, span) in [
            ("short", Some(4000.0), 10),
            ("long", Some(2000.0), 30),
            ("solo", Some(10000.0), 10),
            ("unknown", None, 10),
            ("zero", Some(99000.0), 0),
            ("invalid", Some(99000.0), 100),
        ] {
            fight(
                &db,
                &format!("{prefix}-{suffix}"),
                run,
                "synthetic-session",
                start,
                local,
                span,
            );
        }
        db.execute(
            "DELETE FROM fight_players WHERE fight_id=?1",
            [format!("{prefix}-solo")],
        )
        .unwrap();
        db.execute("UPDATE fight_players SET dps=4000 WHERE fight_id=?1 AND player_id!='local' AND dps IS NOT NULL", [format!("{prefix}-long")]).unwrap();
    }
    // Realm attribution excludes another session, run-linked fights and the end boundary.
    fight(
        &db,
        "other-session",
        None,
        "other-session",
        0,
        Some(99000.0),
        10,
    );
    fight(
        &db,
        "at-end",
        None,
        "synthetic-session",
        100,
        Some(99000.0),
        10,
    );
    let result = chain(&db, "synthetic-session", &visits, 0..2).unwrap();
    for step in &result.steps {
        assert_eq!(step.local_dps, Some(4000.0)); // (4000*10 + 2000*30 + 10000*10)/50
        assert_eq!(step.party_dps, Some(3500.0)); // (2000*10 + 4000*30)/40
    }
}

fn bag(db: &Connection, id: &str, run: Option<&str>, at: i64, rank: Option<i64>, items: &[u16]) {
    db.execute("INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json) VALUES(?1,'synthetic-session',?2,?3,5000,?4,'{}')",
        params![id,run,at,rank]).unwrap();
    crate::common::account_scope_fixture::attribute(db);
    for (slot, item) in items.iter().enumerate() {
        db.execute(
            "INSERT INTO drop_items(drop_id,slot,item_type,quantity,json) VALUES(?1,?2,?3,1,'{}')",
            params![id, slot as i64, item],
        )
        .unwrap();
    }
}

#[test]
fn chain_step_whites_match_streak_predicate() {
    let (db, visits) = sequence(&[
        ("realm", None),
        ("dungeon", Some(CASTLE)),
        ("dungeon", Some(CHAMBER)),
        ("dungeon", Some(SANCTUARY)),
    ]);
    for (index, rank) in [Some(255), Some(255), Some(254), Some(0), None]
        .into_iter()
        .enumerate()
    {
        bag(
            &db,
            &format!("castle-{index}"),
            Some("run-v0001"),
            110 + index as i64,
            rank,
            &[],
        );
    }
    bag(&db, "chamber", Some("run-v0002"), 210, Some(254), &[]);
    bag(&db, "sanctuary", Some("run-v0003"), 310, Some(255), &[]);
    bag(&db, "realm", None, 10, Some(255), &[]);
    let result = chain(&db, "synthetic-session", &visits, 0..4).unwrap();
    assert_eq!((result.steps[1].bags, result.steps[1].whites), (5, 2));
    for step in &result.steps[1..] {
        let (last_white, average): (Option<i64>, Option<f64>) = db
            .query_row(
                include_str!("../src/home_streak.sql"),
                params![step.name, crate::common::account_scope_fixture::ACCOUNT],
                |row| Ok((row.get(1)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(step.whites > 0, last_white.is_some());
        assert_eq!(average, (step.whites > 0).then_some(1.0));
    }
}

#[test]
fn realm_drop_scope_and_notable_items_are_bounded_and_distinct() {
    let (db, visits) = sequence(&[("realm", None), ("dungeon", Some(CASTLE))]);
    bag(&db, "ordinary", None, 0, Some(10), &[9]);
    bag(&db, "white-early", None, 10, Some(255), &[4, 3, 4]);
    bag(&db, "white-late", None, 20, Some(255), &[3, 2, 1, 0]);
    bag(&db, "boundary", None, 100, Some(255), &[99]);
    bag(&db, "other", None, 30, Some(255), &[99]);
    db.execute(
        "UPDATE drops SET session_id='other-session' WHERE id='other'",
        [],
    )
    .unwrap();
    bag(
        &db,
        "run-outside-visit",
        Some("run-v0001"),
        900,
        Some(255),
        &[8],
    );
    let result = chain(&db, "synthetic-session", &visits, 0..2).unwrap();
    assert_eq!((result.steps[0].bags, result.steps[0].whites), (3, 2));
    assert_eq!(result.steps[0].item_types, [4, 3, 2, 1]);
    assert_eq!(result.steps[1].item_types, [8]);
}

#[test]
fn realm_name_uses_context_with_map_fallback() {
    let (db, visits) = sequence(&[
        ("nexus", None),
        ("realm", None),
        ("dungeon", Some(CASTLE)),
        ("dungeon", Some(CHAMBER)),
    ]);
    let mut run = crate::common::stats::run("run-v0002", "synthetic-session", CASTLE, 200);
    run.ended = Some(Timestamp(300));
    for (context, name) in [
        (Some((true, Some("Synthetic Realm"))), "Synthetic Realm"),
        (Some((false, Some("Synthetic Realm"))), "Synthetic Map"),
        (Some((true, None)), "Synthetic Map"),
        (None, "Synthetic Map"),
    ] {
        run.context = context.map(|(entered_from_realm, realm_name)| AreaContext {
            instance_cap: None,
            realm_name: realm_name.map(str::to_owned),
            entered_from_realm,
            difficulty: None,
            game_opened_epoch_s: None,
            instance_age_micros: None,
            modifiers: vec![],
            grade: None,
        });
        db.execute(
            "UPDATE runs SET json=?1 WHERE id=?2",
            params![serde_json::to_string(&run).unwrap(), run.id],
        )
        .unwrap();
        assert_eq!(
            chain(&db, "synthetic-session", &visits, 1..4)
                .unwrap()
                .steps[0]
                .name,
            name
        );
    }
    db.execute("UPDATE runs SET json='invalid-json' WHERE id=?1", [&run.id])
        .unwrap();
    assert_eq!(
        chain(&db, "synthetic-session", &visits, 1..4)
            .unwrap()
            .steps[0]
            .name,
        "Synthetic Map"
    );
}

#[test]
fn non_finite_local_dps_preserves_chain_and_other_aggregates() {
    let (db, visits) = sequence(&[("realm", None), ("dungeon", Some(CASTLE))]);
    for (id, run, started, dps) in [
        ("realm", None, 0, f64::INFINITY),
        ("castle", Some("run-v0001"), 100, f64::NEG_INFINITY),
    ] {
        fight(&db, id, run, "synthetic-session", started, Some(dps), 10);
        bag(&db, id, run, started, Some(255), &[42]);
    }
    let result = chain(&db, "synthetic-session", &visits, 0..2).unwrap();
    assert_eq!(result.steps.len(), 2);
    for step in result.steps {
        assert_eq!(step.local_dps, None);
        assert_eq!(step.party_dps, Some(2000.0));
        assert_eq!((step.bags, step.whites), (1, 1));
        assert_eq!(step.item_types, [42]);
    }
}

#[test]
fn open_realm_and_empty_aggregates_preserve_unknowns() {
    let (db, _) = sequence(&[("realm", None), ("dungeon", Some(CASTLE))]);
    db.execute("UPDATE map_visits SET ended=NULL WHERE id='v0000'", [])
        .unwrap();
    crate::common::account_scope_fixture::attribute(&db);
    let visits = session_visits(&db, "synthetic-session").unwrap();
    bag(&db, "open-realm", None, 1000, None, &[]);
    let result = chain(&db, "synthetic-session", &visits, 0..2).unwrap();
    assert_eq!(result.steps[0].ended, None);
    assert_eq!(result.steps[0].bags, 1);
    assert_eq!(result.steps[1].bags, 0);
    assert!(result
        .steps
        .iter()
        .all(|s| s.local_dps.is_none() && s.party_dps.is_none()));
    assert!(chain(&db, "synthetic-session", &visits, 0..99).is_err());
}

#[test]
fn session_visits_orders_ties_scopes_and_caps() {
    let db = db();
    for index in (0..2002).rev() {
        visit(&db, &format!("v{index:04}"), "realm", None, 0);
    }
    visit(&db, "another-session", "realm", None, 0);
    db.execute(
        "UPDATE map_visits SET session_id='other-session' WHERE id='another-session'",
        [],
    )
    .unwrap();
    crate::common::account_scope_fixture::attribute(&db);
    let visits = session_visits(&db, "synthetic-session").unwrap();
    assert_eq!(visits.len(), 2000);
    assert_eq!(visits.first().unwrap().id, "v0000");
    assert_eq!(visits.last().unwrap().id, "v1999");
}

thread_local! { static STATEMENTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) }; }

fn trace(event: rusqlite::trace::TraceEvent<'_>) {
    if let rusqlite::trace::TraceEvent::Stmt(statement, _) = event {
        STATEMENTS.with(|s| s.borrow_mut().push(statement.expanded_sql().unwrap()));
    }
}

#[test]
fn chain_reuses_the_known_session() {
    let (db, visits) = sequence(&[("realm", None), ("dungeon", Some(CASTLE))]);
    db.trace_v2(
        rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
        Some(trace),
    );
    STATEMENTS.with(|s| s.borrow_mut().clear());
    let result = chain(&db, "synthetic-session", &visits, 0..2).unwrap();
    assert_eq!(result.steps.len(), 2);
    let statements = STATEMENTS.with(|s| s.take());
    assert!(
        !statements
            .iter()
            .any(|sql| sql.contains("SELECT session_id FROM map_visits")),
        "{statements:#?}"
    );
}

#[test]
fn session_visits_plan_uses_index() {
    let db = db();
    db.trace_v2(
        rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
        Some(trace),
    );
    STATEMENTS.with(|s| s.borrow_mut().clear());
    session_visits(&db, "synthetic-session").unwrap();
    let statements = STATEMENTS.with(|s| s.take());
    assert_eq!(statements.len(), 1);
    let plan = db
        .prepare(&format!("EXPLAIN QUERY PLAN {}", statements[0]))
        .unwrap()
        .query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("SEARCH map_visits USING INDEX map_visits_session (session_id=?)"),
        "{plan}"
    );
    assert!(!plan.contains("SCAN map_visits"), "{plan}");
    assert!(!plan.contains("USE TEMP B-TREE FOR ORDER BY"), "{plan}");
}

#[test]
fn chain_step_plans_use_indexes() {
    let (db, mut visits) = sequence(&[("realm", None), ("dungeon", Some(CASTLE))]);
    for ended in [Some(100), None] {
        visits[0].ended = ended;
        db.trace_v2(
            rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
            Some(trace),
        );
        STATEMENTS.with(|s| s.borrow_mut().clear());
        chain(&db, "synthetic-session", &visits, 0..2).unwrap();
        db.trace_v2(rusqlite::trace::TraceEventCodes::empty(), None);
        let statements = STATEMENTS.with(|s| s.take());
        let aggregates: Vec<_> = statements
            .iter()
            .filter(|sql| {
                sql.starts_with("WITH scoped_fights") || sql.starts_with("WITH scoped_drops")
            })
            .collect();
        assert_eq!(aggregates.len(), 6, "{statements:#?}");
        for (index, sql) in aggregates.iter().enumerate() {
            let realm = index < 3;
            let (table, time) = if index % 3 == 0 {
                ("fights", "started")
            } else {
                ("drops", "at")
            };
            let scope = if realm { "session" } else { "run" };
            let plan = db
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map([], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n");
            assert!(
                plan.lines()
                    .any(|line| line.contains(&format!("SEARCH {table} "))
                        && line.contains(&format!("INDEX {table}_{scope} ("))),
                "{plan}"
            );
            assert!(!plan.contains("SCAN fights"), "{plan}");
            assert!(!plan.contains("SCAN drops"), "{plan}");
            if realm {
                assert!(
                    plan.contains(&format!("session_id=? AND {time}>? AND {time}<?")),
                    "{plan}"
                );
                assert!(
                    sql.contains(&format!("INDEXED BY {table}_session")),
                    "{sql}"
                );
                assert!(
                    sql.contains(&format!("{time}<{}", ended.unwrap_or(i64::MAX))),
                    "{sql}"
                );
            }
        }
    }
}

#[test]
fn account_scope_journey_keeps_foreign_visits_as_boundaries() {
    for entries in [
        vec![
            ("realm", None),
            ("dungeon", Some(CASTLE)),
            ("dungeon", Some(CHAMBER)),
        ],
        vec![
            ("dungeon", Some(CASTLE)),
            ("nexus", None),
            ("dungeon", Some(CASTLE)),
        ],
    ] {
        let (db, _) = sequence(&entries);
        let b = "account-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        db.execute("INSERT INTO accounts VALUES(?1)", [b]).unwrap();
        db.execute("INSERT INTO characters(id,observed_at,json,account_id) VALUES('synthetic-b',0,'{}',?1)",[b]).unwrap();
        db.execute(
            "UPDATE map_visits SET character_id='synthetic-b' WHERE id='v0001'",
            [],
        )
        .unwrap();
        db.execute(
            "UPDATE runs SET character_id='synthetic-b' WHERE id='run-v0001'",
            [],
        )
        .unwrap();
        let visits = session_visits(&db, "synthetic-session").unwrap();
        assert_eq!(visits.len(), 3);
        assert!(chain_range(&visits, 2).is_none());
        assert_eq!(in_a_row(&visits, 2), 1);
    }
}
