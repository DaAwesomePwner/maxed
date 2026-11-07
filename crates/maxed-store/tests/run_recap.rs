//! Synthetic history: stable recap ownership and the existing PB evidence rule.
use maxed_store::{open_reader, StoreError};
use maxed_types::*;
use rusqlite::{params, Connection};

const DUNGEON: &str = "Synthetic Sanctum";

struct Fixture {
    db: Connection,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Connection::open(dir.path().join("recap.sqlite")).unwrap();
        maxed_store::migrate(&mut db).unwrap();
        db.execute(
            "INSERT INTO play_sessions VALUES('session',0,100000,100000,'{}')",
            [],
        )
        .unwrap();
        for id in ["self", "other"] {
            let mut character = demo::demo_run_story().character.unwrap();
            character.id = id.into();
            character.identity_verified = true;
            db.execute(
                "INSERT INTO characters(id,observed_at,json) VALUES(?1,0,?2)",
                params![id, serde_json::to_string(&character).unwrap()],
            )
            .unwrap();
        }
        Self { db, dir }
    }

    fn run(&self, id: &str, started: i64, outcome: Option<&str>, duration: i64) {
        self.db.execute(
            "INSERT INTO runs(id,session_id,character_id,dungeon,started,ended,completed_at,outcome,complete_capture,json)
             VALUES(?1,'session','self',?2,?3,?4,?5,?6,1,'{\"area_kind\":\"dungeon\"}')",
            params![id, DUNGEON, started, outcome.map(|_| started + duration),
                (outcome == Some("cleared")).then_some(started + duration), outcome],
        ).unwrap();
        if outcome == Some("cleared") {
            self.db
                .execute(
                    "INSERT INTO run_evidence VALUES(?1,0,?2,'victory','{}')",
                    params![id, started + duration],
                )
                .unwrap();
        }
    }

    fn drop(&self, id: &str, run: Option<&str>, at: i64, rank: u8, items: &[u16]) {
        self.db.execute("INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json) VALUES(?1,'session',?2,?3,1,?4,'{}')", params![id, run, at, rank]).unwrap();
        for (slot, item) in items.iter().enumerate() {
            self.db.execute("INSERT INTO drop_items(drop_id,slot,item_type,quantity,json) VALUES(?1,?2,?3,1,'{}')", params![id, slot as i64, item]).unwrap();
        }
    }

    fn fight(&self, id: &str, run: &str, started: u64, dps: f64, character: &str, eligible: bool) {
        let mut f = demo::demo_snapshot().fights.remove(0);
        f.id = id.into();
        f.run_id = Some(run.into());
        f.started = Timestamp(started);
        f.ended = Some(Timestamp(started + 10));
        f.first_hit = Some(Timestamp(started + 1));
        f.last_hit = Some(Timestamp(started + 9));
        f.end_reason = Some(FightEnd::Killed);
        f.boss_type = Some(42);
        f.dps_basis = DpsBasis::PlayerActiveWindowV1;
        f.complete_capture = eligible;
        f.pending_local_reconciliation = false;
        f.issues.clear();
        f.players.retain(|p| p.is_local);
        f.players[0].player_id = character.into();
        f.players[0].damage = dps;
        f.players[0].dps = Some(dps);
        f.players[0].sources.clear();
        self.db
            .execute(
                "INSERT INTO fights VALUES(?1,?2,'session',?3,?4,42,?5,?6,?7)",
                params![
                    id,
                    run,
                    started as i64,
                    started as i64 + 10,
                    dps,
                    eligible,
                    serde_json::to_string(&f).unwrap()
                ],
            )
            .unwrap();
        let p = &f.players[0];
        self.db.execute("INSERT INTO fight_players(fight_id,player_id,object_id,damage,dps,json) VALUES(?1,?2,?3,?4,?4,?5)", params![id, character, p.object_id, dps, serde_json::to_string(p).unwrap()]).unwrap();
    }

    fn visit(&self, id: &str, run: Option<&str>, kind: &str, started: i64) {
        self.db.execute("INSERT INTO map_visits(id,stream_id,session_id,run_id,map,kind,started,ended,source,json) VALUES(?1,'stream','session',?2,'Synthetic Map',?3,?4,?4+10,'live','{}')", params![id, run, kind, started]).unwrap();
    }

    fn recap(&self, id: &str, chase: &[u16]) -> RunRecap {
        open_reader(&self.dir.path().join("recap.sqlite"))
            .unwrap()
            .run_recap(&RunRecapRequest { run_id: id.into() }, chase)
            .unwrap()
            .unwrap()
    }
}

#[test]
fn clear_number_counts_cleared_runs_before_and_including() {
    let t = Fixture::new();
    for (id, start, outcome) in [
        ("a", 10, "cleared"),
        ("b", 20, "left"),
        ("c", 30, "cleared"),
        ("d", 30, "cleared"),
        ("e", 40, "cleared"),
    ] {
        t.run(id, start, Some(outcome), 10);
    }
    t.db.execute("UPDATE runs SET character_id='other' WHERE id='a'", [])
        .unwrap();
    assert_eq!(t.recap("c", &[]).clear_number, Some(2));
    assert_eq!(t.recap("d", &[]).clear_number, Some(3));
}

#[test]
fn non_cleared_run_has_no_clear_number() {
    let t = Fixture::new();
    for (id, outcome) in [
        ("left", Some("left")),
        ("died", Some("died")),
        ("unknown", Some("unknown")),
        ("open", None),
    ] {
        t.run(id, 10, outcome, 10);
        assert_eq!(t.recap(id, &[]).clear_number, None);
        assert_eq!(t.recap(id, &[]).pb_clear, None);
    }
}

#[test]
fn pb_clear_only_when_strictly_faster_than_previous() {
    let t = Fixture::new();
    for (id, start, duration) in [
        ("a", 10, 100),
        ("b", 20, 100),
        ("c", 20, 90),
        ("d", 30, 110),
        ("e", 40, 50),
    ] {
        t.run(id, start, Some("cleared"), duration);
    }
    assert_eq!(t.recap("b", &[]).pb_clear, None);
    assert_eq!(
        t.recap("c", &[]).pb_clear,
        Some(PbClear {
            clear_micros: 90,
            previous_best_micros: 100
        })
    );
    assert_eq!(t.recap("d", &[]).pb_clear, None);
}

#[test]
fn pb_clear_uses_dungeon_best_eligibility() {
    let t = Fixture::new();
    for (id, start, duration) in [
        ("no-evidence", 1, 1),
        ("realm", 2, 1),
        ("zero", 3, 0),
        ("baseline", 10, 100),
        ("current", 20, 90),
    ] {
        t.run(id, start, Some("cleared"), duration);
    }
    t.db.execute("DELETE FROM run_evidence WHERE run_id='no-evidence'", [])
        .unwrap();
    t.db.execute(
        "UPDATE runs SET json='{\"area_kind\":\"realm\"}' WHERE id='realm'",
        [],
    )
    .unwrap();
    t.db.execute(
        "UPDATE run_evidence SET kind='completion_dialogue' WHERE run_id='baseline'",
        [],
    )
    .unwrap();
    assert_eq!(
        t.recap("current", &[])
            .pb_clear
            .unwrap()
            .previous_best_micros,
        100
    );
    t.db.execute("DELETE FROM run_evidence WHERE run_id='current'", [])
        .unwrap();
    assert_eq!(t.recap("current", &[]).pb_clear, None);
}

#[test]
fn first_clear_is_milestone_not_pb() {
    let t = Fixture::new();
    t.run("first", 10, Some("cleared"), 10);
    let recap = t.recap("first", &[]);
    assert_eq!(recap.pb_clear, None);
    assert_eq!(recap.milestones, vec![RecapMilestone::FirstClear]);
}

#[test]
fn dry_streak_end_counts_ended_runs_since_previous_white() {
    let t = Fixture::new();
    for (id, started, outcome) in [
        ("old", 1, Some("left")),
        ("white", 10, None),
        ("a", 20, Some("left")),
        ("b", 20, Some("died")),
        ("open", 25, None),
        ("current", 30, Some("cleared")),
        ("later", 40, Some("cleared")),
    ] {
        t.run(id, started, outcome, 10);
    }
    t.drop("previous", Some("white"), 22, 255, &[]);
    t.drop("current-drop", Some("current"), 35, 255, &[]);
    t.drop("later-drop", Some("later"), 45, 255, &[]);
    assert_eq!(
        t.recap("current", &[]).dry_streak_end,
        Some(DryStreakEnd {
            runs: 2,
            had_previous_white: true
        })
    );
    assert!(!t
        .recap("current", &[])
        .milestones
        .contains(&RecapMilestone::FirstWhite));
    assert_eq!(t.recap("a", &[]).dry_streak_end, None);
}

#[test]
fn dry_streak_end_without_previous_white() {
    let t = Fixture::new();
    t.run("first", 1, Some("left"), 1);
    t.run("second", 2, Some("died"), 1);
    t.run("current", 3, Some("cleared"), 1);
    t.drop("ordinary", Some("first"), 1, 254, &[]);
    t.drop("white", Some("current"), 3, 255, &[]);
    let recap = t.recap("current", &[]);
    assert_eq!(
        recap.dry_streak_end,
        Some(DryStreakEnd {
            runs: 2,
            had_previous_white: false
        })
    );
    assert_eq!(
        recap.milestones,
        vec![RecapMilestone::FirstClear, RecapMilestone::FirstWhite]
    );
}

#[test]
fn other_dungeon_runs_do_not_count() {
    let t = Fixture::new();
    t.run("first", 10, Some("cleared"), 1);
    t.run("current", 50, Some("cleared"), 1);
    t.drop("current-white", Some("current"), 50, 255, &[]);
    let expected = t.recap("current", &[]);
    assert_eq!(expected.clear_number, Some(2));
    assert_eq!(
        expected.dry_streak_end,
        Some(DryStreakEnd {
            runs: 1,
            had_previous_white: false
        })
    );

    for (id, started) in [("other-white", 20), ("other-dry", 30)] {
        t.run(id, started, Some("cleared"), 1);
        t.db.execute(
            "UPDATE runs SET dungeon='Synthetic Cavern' WHERE id=?1",
            [id],
        )
        .unwrap();
    }
    // Another dungeon's ended runs neither increment the counts nor reset them.
    assert_eq!(t.recap("current", &[]), expected);
    t.drop("other-drop", Some("other-white"), 20, 255, &[]);
    assert_eq!(t.recap("current", &[]), expected);
}

#[test]
fn another_characters_white_ends_the_streak() {
    let t = Fixture::new();
    for (id, started) in [("old", 10), ("other", 20), ("dry", 30), ("current", 40)] {
        t.run(id, started, Some("cleared"), 1);
    }
    t.db.execute("UPDATE runs SET character_id='other' WHERE id='other'", [])
        .unwrap();
    t.drop("current-white", Some("current"), 40, 255, &[]);
    assert_eq!(
        t.recap("current", &[]).dry_streak_end,
        Some(DryStreakEnd {
            runs: 3,
            had_previous_white: false
        })
    );

    t.drop("other-white", Some("other"), 20, 255, &[]);
    let recap = t.recap("current", &[]);
    assert_eq!(
        recap.dry_streak_end,
        Some(DryStreakEnd {
            runs: 1,
            had_previous_white: true
        })
    );
    assert!(!recap.milestones.contains(&RecapMilestone::FirstWhite));
}

#[test]
fn several_whites_in_one_run_end_one_streak() {
    let t = Fixture::new();
    t.run("current", 1, Some("cleared"), 10);
    t.drop("a", Some("current"), 2, 255, &[]);
    t.drop("b", Some("current"), 3, 255, &[]);
    assert_eq!(
        t.recap("current", &[]).dry_streak_end,
        Some(DryStreakEnd {
            runs: 0,
            had_previous_white: false
        })
    );
    assert_eq!(
        t.recap("current", &[]).milestones,
        vec![RecapMilestone::FirstClear, RecapMilestone::FirstWhite]
    );
}

#[test]
fn best_dps_needs_an_earlier_eligible_fight() {
    let t = Fixture::new();
    t.run("run", 1, Some("cleared"), 100);
    t.fight("first", "run", 10, 100., "self", true);
    assert!(t.recap("run", &[]).best_dps.is_empty());
    t.fight("second", "run", 20, 200., "self", true);
    t.fight("tie", "run", 30, 200., "self", true);
    assert_eq!(
        t.recap("run", &[]).best_dps,
        vec![BossDpsRecord {
            fight_id: "second".into(),
            boss_type: 42,
            dps: 200.,
            previous_best_dps: 100.
        }]
    );
}

#[test]
fn best_dps_ignores_other_characters_and_ineligible_fights() {
    let t = Fixture::new();
    t.run("before", 1, Some("left"), 100);
    t.run("current", 100, Some("cleared"), 100);
    t.fight("baseline", "before", 10, 100., "self", true);
    t.fight("other", "before", 20, 1000., "other", true);
    t.fight("partial", "before", 30, 900., "self", false);
    t.fight("record", "current", 110, 200., "self", true);
    t.fight("ineligible-current", "current", 120, 500., "self", false);
    assert_eq!(
        t.recap("current", &[]).best_dps,
        vec![BossDpsRecord {
            fight_id: "record".into(),
            boss_type: 42,
            dps: 200.,
            previous_best_dps: 100.
        }]
    );
}

#[test]
fn best_dps_resolves_historical_aliases_and_excludes_same_start() {
    let t = Fixture::new();
    t.run("before", 1, Some("left"), 100);
    t.run("current", 100, Some("cleared"), 100);
    t.db.execute("INSERT INTO character_aliases VALUES('alias','self')", [])
        .unwrap();
    t.fight("baseline", "before", 10, 100., "alias", true);
    t.fight("same-start", "before", 110, 1000., "self", true);
    t.fight("record", "current", 110, 200., "self", true);
    assert_eq!(t.recap("current", &[]).best_dps[0].previous_best_dps, 100.);
}

#[test]
fn best_dps_is_as_of_the_run() {
    let t = Fixture::new();
    t.run("before", 1, Some("cleared"), 10);
    t.run("current", 20, Some("cleared"), 10);
    t.fight("baseline", "before", 1, 100., "self", true);
    t.fight("record", "current", 20, 200., "self", true);
    let recap = t.recap("current", &[]);
    assert_eq!(recap.best_dps.len(), 1);
    t.run("later", 40, Some("cleared"), 10);
    t.fight("later-best", "later", 40, 300., "self", true);
    assert_eq!(t.recap("current", &[]), recap);
}

#[test]
fn clear_count_milestone_thresholds() {
    let t = Fixture::new();
    for count in 1..=1001 {
        let id = format!("run-{count:04}");
        t.run(&id, count, Some("cleared"), 10);
        let expected = match count {
            1 => vec![RecapMilestone::FirstClear],
            10 | 25 | 50 | 100 | 250 | 500 | 1000 => vec![RecapMilestone::ClearCount {
                count: count as u32,
            }],
            _ => vec![],
        };
        assert_eq!(t.recap(&id, &[]).milestones, expected, "clear {count}");
    }
}

#[test]
fn new_items_first_appearance_only() {
    let t = Fixture::new();
    t.run("first", 10, Some("left"), 10);
    t.run("current", 30, Some("cleared"), 10);
    t.run("later", 50, Some("cleared"), 10);
    t.drop("realm", None, 1, 10, &[1]);
    t.drop("first-bag", Some("first"), 11, 10, &[2]);
    t.drop("a", Some("current"), 31, 255, &[12, 3, 1, 2]);
    t.drop("b", Some("current"), 31, 10, &[4, 5, 6, 7, 8, 9, 10, 11]);
    t.drop("c", Some("current"), 32, 10, &[12]);
    t.drop("later-bag", Some("later"), 51, 255, &[3, 12]);
    let chase: Vec<u16> = (1..=12).rev().chain([3]).collect();
    assert_eq!(
        t.recap("current", &chase).milestones,
        vec![
            RecapMilestone::FirstClear,
            RecapMilestone::FirstWhite,
            RecapMilestone::NewItems {
                item_types: vec![12, 3, 4, 5, 6, 7, 8, 9],
                more: 2
            }
        ]
    );
    assert!(t.recap("later", &chase).milestones.is_empty());
    assert_eq!(
        t.recap("current", &[]).milestones,
        vec![RecapMilestone::FirstClear, RecapMilestone::FirstWhite]
    );
}

#[test]
fn chain_only_on_last_step() {
    let t = Fixture::new();
    t.visit("realm", None, "realm", 0);
    for (id, start, dungeon) in [
        ("castle", 10, "Oryx's Castle"),
        ("chamber", 20, "Oryx's Chamber"),
        ("sanctuary", 30, "Oryx's Sanctuary"),
    ] {
        t.run(id, start, Some("cleared"), 10);
        t.db.execute(
            "UPDATE runs SET dungeon=?2 WHERE id=?1",
            params![id, dungeon],
        )
        .unwrap();
        t.visit(id, Some(id), "dungeon", start);
    }
    assert!(t.recap("castle", &[]).chain.is_none());
    assert!(t.recap("chamber", &[]).chain.is_none());
    let chain = t.recap("sanctuary", &[]).chain.unwrap();
    assert_eq!(
        chain.steps.iter().map(|s| s.kind).collect::<Vec<_>>(),
        vec![
            ChainStepKind::Realm,
            ChainStepKind::Castle,
            ChainStepKind::Chamber,
            ChainStepKind::Sanctuary
        ]
    );
}

#[test]
fn repeat_ordinal_uses_visits_and_defaults_when_missing_or_capped() {
    let t = Fixture::new();
    for (id, start) in [("a", 10), ("b", 30), ("c", 50)] {
        t.run(id, start, Some("cleared"), 10);
        t.visit(id, Some(id), "dungeon", start);
    }
    t.visit("hub", None, "nexus", 20);
    assert_eq!(t.recap("c", &[]).in_a_row, 3);
    t.run("missing", 60, Some("cleared"), 10);
    assert_eq!(
        (
            t.recap("missing", &[]).in_a_row,
            t.recap("missing", &[]).chain
        ),
        (1, None)
    );
    t.db.execute_batch("BEGIN").unwrap();
    for i in 0..2000 {
        t.visit(&format!("hub-{i}"), None, "nexus", 100 + i);
    }
    t.db.execute_batch("COMMIT").unwrap();
    t.run("capped", 3000, Some("cleared"), 10);
    t.visit("capped", Some("capped"), "dungeon", 3000);
    assert_eq!(
        (
            t.recap("capped", &[]).in_a_row,
            t.recap("capped", &[]).chain
        ),
        (1, None)
    );
}

#[test]
fn unknown_run_is_none() {
    let t = Fixture::new();
    assert_eq!(
        open_reader(&t.dir.path().join("recap.sqlite"))
            .unwrap()
            .run_recap(
                &RunRecapRequest {
                    run_id: "missing".into()
                },
                &[]
            )
            .unwrap(),
        None
    );
}

#[test]
fn run_recap_rejects_invalid_id() {
    let t = Fixture::new();
    for run_id in [String::new(), "x".repeat(257), "bad\0id".into()] {
        assert!(matches!(
            open_reader(&t.dir.path().join("recap.sqlite"))
                .unwrap()
                .run_recap(&RunRecapRequest { run_id }, &[]),
            Err(StoreError::InvalidEvent)
        ));
    }
}

#[test]
fn previous_dps_candidates_select_only_scalar_fields() {
    let t = Fixture::new();
    t.run("before", 1, Some("cleared"), 100);
    for (id, started, dps) in [
        ("low", 10, 100.),
        ("tie-b", 20, 200.),
        ("tie-a", 20, 200.),
        ("tie-later", 25, 200.),
        ("same-start", 30, 300.),
    ] {
        t.fight(id, "before", started, dps, "self", true);
    }
    let mut statement =
        t.db.prepare(include_str!("../src/records_previous_dps.sql"))
            .unwrap();
    assert_eq!(statement.column_names(), ["id", "local_dps", "started"]);
    let candidates = statement
        .query_map(params!["self", 42, 30], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        candidates,
        vec![
            ("tie-a".into(), 200., 20),
            ("tie-b".into(), 200., 20),
            ("tie-later".into(), 200., 25),
            ("low".into(), 100., 10),
        ]
    );
}

#[test]
fn run_recap_plans_avoid_base_scans() {
    let t = Fixture::new();
    // Exercise every new statement with its real parameter layout. Journey's
    // shared statements are covered by journey::chain_step_plans_use_indexes.
    for (file, values) in [
        ("run_recap.sql", vec!["current"]),
        ("records_runs.sql", vec!["current", DUNGEON, "30"]),
        ("records_fights.sql", vec!["current"]),
        ("records_previous_dps.sql", vec!["self", "42", "30"]),
        ("records_items.sql", vec!["current", "[1,2]"]),
    ] {
        let sql = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join(file),
        )
        .unwrap();
        if file == "records_previous_dps.sql" {
            let statement = t.db.prepare(&sql).unwrap();
            assert!(
                !statement.column_names().contains(&"json"),
                "the ordered candidate statement must not carry full fight JSON"
            );
        }
        let plan =
            t.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map(rusqlite::params_from_iter(values), |r| {
                    r.get::<_, String>(3)
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
        for line in &plan {
            if let Some(scanned) = line
                .strip_prefix("SCAN ")
                .and_then(|s| s.split_whitespace().next())
            {
                assert!(
                    ![
                        "runs",
                        "r",
                        "fights",
                        "f",
                        "drops",
                        "d",
                        "prior_drop",
                        "drop_items",
                        "i",
                        "prior_item"
                    ]
                    .contains(&scanned),
                    "{file}: {plan:?}"
                );
            }
        }
        assert!(
            plan.iter().any(|s| s.starts_with("SEARCH ")),
            "{file}: {plan:?}"
        );
    }
}
