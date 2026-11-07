//! Independent marker transitions, including the writer's owner-before-observation order.
use crate::common::account_sql::with_accounts;
use rusqlite::Connection;

struct Step {
    sql: String,
    unresolved: bool,
    writes: Option<u64>,
}
impl Step {
    fn new(sql: impl Into<String>, unresolved: bool) -> Self {
        Self {
            sql: sql.into(),
            unresolved,
            writes: None,
        }
    }
}
struct Case {
    trigger: String,
    steps: Vec<Step>,
}
#[derive(Debug, PartialEq)]
struct State {
    markers: Vec<(String, String)>,
    notice: bool,
    writes: Option<u64>,
}
fn database() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut db).unwrap();
    db.execute_batch("INSERT INTO accounts VALUES('synthetic-a');
        INSERT INTO characters(id,observed_at,json,account_id) VALUES('a',0,'{}','synthetic-a');
        INSERT INTO play_sessions VALUES('s',0,100,100,'{}');
        INSERT INTO runs(id,session_id,character_id,dungeon,started,complete_capture,json) VALUES('r','s','a','Synthetic',0,1,'{}');").unwrap();
    db
}
fn state(db: &Connection, writes: Option<u64>) -> State {
    State {
        markers: db.prepare("SELECT kind,observation_id FROM unresolved_observations ORDER BY kind,observation_id").unwrap().query_map([], |r|Ok((r.get(0)?,r.get(1)?))).unwrap().collect::<Result<_,_>>().unwrap(),
        notice: db.query_row(&with_accounts("SELECT /*unattributed_history*/"), [], |r|r.get(0)).unwrap(),
        writes,
    }
}
fn expected(kind: &str, unresolved: bool, writes: Option<u64>) -> State {
    State {
        markers: if unresolved {
            vec![(kind.into(), "target".into())]
        } else {
            vec![]
        },
        notice: unresolved,
        writes,
    }
}
fn cases(kind: &str) -> Vec<Case> {
    let table = if kind == "fight" { "fights" } else { "drops" };
    let insert = |run| {
        if kind == "fight" {
            format!("INSERT INTO fights(id,session_id,run_id,started,complete_capture,json) VALUES('target','s',{run},0,1,'{{}}')")
        } else {
            format!("INSERT INTO drops(id,session_id,run_id,at,bag_type,json) VALUES('target','s',{run},0,1,'{{}}')")
        }
    };
    let owner = |id, character| {
        format!("INSERT INTO observation_characters VALUES('{kind}','{id}',{character})")
    };
    let character = "INSERT INTO characters(id,observed_at,json,account_id) VALUES('later',0,'{}','synthetic-a')";
    let owned = || {
        vec![
            Step::new(owner("target", "'a'"), false),
            Step::new(insert("NULL"), false),
        ]
    };
    let mut owner_old = owned();
    owner_old.push(Step::new(format!("UPDATE observation_characters SET observation_id='spare' WHERE kind='{kind}' AND observation_id='target'"),true));
    let mut owner_delete = owned();
    owner_delete.push(Step::new(
        format!(
            "DELETE FROM observation_characters WHERE kind='{kind}' AND observation_id='target'"
        ),
        true,
    ));
    vec![
        Case { trigger: format!("unresolved_{table}_insert"), steps: vec![
            Step::new(owner("target","NULL"),false),Step::new(insert("NULL"),true)] },
        Case { trigger: format!("unresolved_{table}_update"), steps: vec![
            Step::new(owner("target","NULL"),false),Step::new(insert("'r'"),false),
            Step::new(format!("UPDATE {table} SET run_id=NULL WHERE id='target'"),true),
            Step::new(format!("UPDATE {table} SET run_id='r' WHERE id='target'"),false),
            Step::new(format!("UPDATE {table} SET run_id=NULL WHERE id='target'"),true),
            Step { sql:format!("UPDATE {table} SET json='{{\"updated\":true}}' WHERE id='target'"), unresolved:true, writes:Some(1) },
            Step { sql:format!("UPDATE {table} SET run_id=run_id WHERE id='target'"), unresolved:true, writes:Some(1) }] },
        Case { trigger: format!("unresolved_{table}_delete"), steps: vec![
            Step::new(owner("target","NULL"),false),Step::new(insert("NULL"),true),
            Step::new(format!("DELETE FROM {table} WHERE id='target'"),false)] },
        Case { trigger: format!("unresolved_{kind}_owner_insert"), steps: vec![
            Step::new(insert("NULL"),true),Step::new(owner("target","'a'"),false)] },
        Case { trigger: format!("unresolved_{kind}_owner_update"), steps: vec![
            Step::new(owner("spare","'a'"),false),Step::new(insert("NULL"),true),
            Step::new(format!("UPDATE observation_characters SET observation_id='target' WHERE kind='{kind}' AND observation_id='spare'"),false)] },
        Case { trigger: format!("unresolved_{kind}_owner_update_old"), steps:owner_old },
        Case { trigger: format!("unresolved_{kind}_owner_delete"), steps:owner_delete },
        Case { trigger: format!("unresolved_{kind}_character_insert"), steps: vec![
            Step::new(owner("target","'later'"),false),Step::new(insert("NULL"),true),Step::new(character,false)] },
        Case { trigger: format!("unresolved_{kind}_character_delete"), steps: vec![
            Step::new(character,false),Step::new(owner("target","'later'"),false),Step::new(insert("NULL"),false),
            Step::new("DELETE FROM characters WHERE id='later'",true)] },
    ]
}
fn exercise(db: &Connection, kind: &str, case: &Case, drop_trigger: bool) -> Vec<State> {
    // DDL and data rollback together: each run begins clean and restores its trigger.
    db.execute_batch("SAVEPOINT trigger_case").unwrap();
    if drop_trigger {
        db.execute_batch(&format!("DROP TRIGGER {}", case.trigger))
            .unwrap();
    }
    assert_eq!(state(db, None), expected(kind, false, None));
    let actual = case
        .steps
        .iter()
        .map(|step| {
            let before = db.total_changes();
            db.execute_batch(&step.sql).unwrap();
            state(db, step.writes.map(|_| db.total_changes() - before))
        })
        .collect();
    db.execute_batch("ROLLBACK TO trigger_case; RELEASE trigger_case")
        .unwrap();
    actual
}

#[test]
fn account_scope_notice_trigger_paths_start_clean() {
    let db = database();
    for kind in ["fight", "drop"] {
        for case in cases(kind) {
            let expected: Vec<_> = case
                .steps
                .iter()
                .map(|step| expected(kind, step.unresolved, step.writes))
                .collect();
            assert_eq!(
                exercise(&db, kind, &case, false),
                expected,
                "{}",
                case.trigger
            );
        }
    }
}

#[test]
fn account_scope_notice_trigger_drop_mutations_are_detected_and_restored() {
    let db = database();
    for kind in ["fight", "drop"] {
        for case in cases(kind) {
            let expected: Vec<_> = case
                .steps
                .iter()
                .map(|step| expected(kind, step.unresolved, step.writes))
                .collect();
            assert_eq!(
                exercise(&db, kind, &case, false),
                expected,
                "baseline {}",
                case.trigger
            );
            assert_ne!(
                exercise(&db, kind, &case, true),
                expected,
                "surviving mutation: {}",
                case.trigger
            );
            assert_eq!(
                exercise(&db, kind, &case, false),
                expected,
                "restored {}",
                case.trigger
            );
            eprintln!(
                "account_scope mutation {}: detected; restored path passes",
                case.trigger
            );
        }
    }
}
