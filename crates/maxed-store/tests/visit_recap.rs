//! Synthetic realm visits: interval ownership, bounded detail and historical records.
use maxed_store::StoreError;
use maxed_types::*;
use rusqlite::{params, Connection};

struct Fixture {
    db: Connection,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut db = Connection::open(dir.path().join("visit.sqlite")).unwrap();
        maxed_store::migrate(&mut db).unwrap();
        for session in ["session", "other-session"] {
            db.execute(
                "INSERT INTO play_sessions VALUES(?1,0,100000,100000,'{}')",
                [session],
            )
            .unwrap();
        }
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

    fn visit(&self, id: &str, start: i64, end: Option<i64>, key: Option<&str>) {
        self.db.execute("INSERT INTO map_visits VALUES(?1,?1,'session','self',NULL,'Synthetic Realm','realm',?2,?3,?4,'live','{}')", params![id,key,start,end]).unwrap();
    }

    fn fight(&self, id: &str, start: u64, dps: f64) -> FightView {
        let mut fight = demo::demo_snapshot().fights.remove(0);
        fight.id = id.into();
        fight.session_id = "session".into();
        fight.run_id = None;
        fight.started = Timestamp(start);
        fight.ended = Some(Timestamp(start + 10));
        fight.first_hit = Some(Timestamp(start + 1));
        fight.last_hit = Some(Timestamp(start + 9));
        fight.end_reason = Some(FightEnd::Killed);
        fight.boss_type = Some(42);
        fight.target_kind = Some(TargetKind::EventBoss);
        fight.dps_basis = DpsBasis::PlayerActiveWindowV1;
        fight.complete_capture = true;
        fight.pending_local_reconciliation = false;
        fight.issues.clear();
        fight.players.retain(|p| p.is_local);
        fight.players[0].player_id = "self".into();
        fight.players[0].damage = dps;
        fight.players[0].dps = Some(dps);
        fight.players[0].sources.clear();
        let mut other = fight.players[0].clone();
        other.player_id = "other".into();
        other.is_local = false;
        other.dps = Some(123.);
        fight.players.push(other);
        self.db
            .execute(
                "INSERT INTO fights VALUES(?1,NULL,'session',?2,?2+10,42,?3,1,?4)",
                params![
                    id,
                    start as i64,
                    dps,
                    serde_json::to_string(&fight).unwrap()
                ],
            )
            .unwrap();
        for player in &fight.players {
            self.db.execute("INSERT INTO fight_players(fight_id,player_id,object_id,damage,dps,json) VALUES(?1,?2,?3,?4,?5,?6)", params![id,player.player_id,player.object_id,player.damage,player.dps,serde_json::to_string(player).unwrap()]).unwrap();
        }
        let mut summary = demo::demo_run_story().fights.remove(0);
        summary.id = id.into();
        summary.started = fight.started;
        summary.ended = fight.ended;
        summary.target_kind = fight.target_kind.clone();
        summary.boss_type = fight.boss_type;
        summary.local_dps = Some(dps);
        summary.party = None;
        self.db
            .execute(
                "INSERT INTO fight_summaries VALUES(?1,NULL,?2,?3)",
                params![id, start as i64, serde_json::to_string(&summary).unwrap()],
            )
            .unwrap();
        fight
    }

    fn contents(&self, id: &str, at: u64) {
        self.fight(id, at, 100.);
        let mut drop = demo::demo_run_story().drops.remove(0);
        drop.id = id.into();
        drop.session_id = "session".into();
        drop.run_id = None;
        drop.at = Timestamp(at);
        drop.bag_rank = Some(255);
        self.db.execute("INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json) VALUES(?1,'session',NULL,?2,1,255,?3)", params![id,at as i64,serde_json::to_string(&drop).unwrap()]).unwrap();
        let event = InstanceEventView {
            id: id.into(),
            session_id: "session".into(),
            run_id: None,
            map: None,
            at: Timestamp(at),
            provisional: false,
            kind: InstanceEventKind::RealmEventKilled {
                boss_key: "synthetic-boss".into(),
                object_type: Some(42),
                killer: None,
            },
        };
        self.db
            .execute(
                "INSERT INTO instance_events VALUES(?1,'session',NULL,?2,'realm_event_killed',?3)",
                params![id, at as i64, serde_json::to_string(&event).unwrap()],
            )
            .unwrap();
    }

    fn fame(&self, sequence: i64, at: i64, fame: i64, character: &str, session: &str) {
        self.db
            .execute(
                "INSERT INTO fame_samples VALUES('synthetic-stream',?1,?2,?3,?4,?5)",
                params![sequence, character, at, fame, session],
            )
            .unwrap();
    }

    fn castle(&self, start: i64, entered: bool) {
        self.db.execute("INSERT INTO runs(id,session_id,dungeon,started,ended,complete_capture,json) VALUES('castle','session',?1,?2,?2+10,1,?3)", params!["Oryx's Castle",start,serde_json::json!({"context":{"entered_from_realm":entered,"realm_name":"Synthetic Hydra"}}).to_string()]).unwrap();
        self.visit("castle-visit", start, Some(start + 10), None);
        self.db
            .execute(
                "UPDATE map_visits SET kind='dungeon',run_id='castle' WHERE id='castle-visit'",
                [],
            )
            .unwrap();
    }

    fn read(&self, id: &str) -> Result<Option<VisitRecap>, StoreError> {
        crate::common::account_scope_fixture::open_reader(&self.dir.path().join("visit.sqlite"))
            .unwrap()
            .visit_recap(&VisitRecapRequest {
                visit_id: id.into(),
            })
    }
    fn recap(&self, id: &str) -> VisitRecap {
        self.read(id).unwrap().unwrap()
    }
}

fn ids(recap: &VisitRecap, expected: &[&str]) {
    assert_eq!(
        recap
            .fights
            .iter()
            .map(|f| f.id.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        recap
            .boss_fights
            .iter()
            .map(|f| f.id.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        recap
            .drops
            .iter()
            .map(|d| d.id.as_str())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(
        recap
            .announcements
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn visit_contents_are_half_open_and_same_session() {
    let t = Fixture::new();
    t.visit("visit", 10, Some(20), None);
    for (id, at) in [
        ("before", 9),
        ("a-start", 10),
        ("b-start", 10),
        ("inside", 19),
        ("end", 20),
        ("foreign", 15),
    ] {
        t.contents(id, at);
    }
    for table in ["fights", "drops", "instance_events"] {
        t.db.execute(
            &format!("UPDATE {table} SET session_id='other-session' WHERE id='foreign'"),
            [],
        )
        .unwrap();
    }
    let recap = t.recap("visit");
    ids(&recap, &["a-start", "b-start", "inside"]);
    assert_eq!(recap.session_id, "session");
    assert_eq!(recap.character_id.as_deref(), Some("self"));
    assert_eq!(recap.title, "Synthetic Realm");
    assert_eq!(
        recap.fights.iter().map(|f| f.ordinal).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        recap.fights[0].party,
        Some(PartyAverage {
            avg_dps: 123.,
            players: 1
        })
    );
    assert!(!recap.boss_fights_limited && !recap.drops_limited && !recap.announcements_limited);
}

#[test]
fn run_fights_are_excluded() {
    let t = Fixture::new();
    t.visit("visit", 10, Some(20), None);
    t.castle(30, true);
    t.contents("run-owned", 15);
    for table in ["fights", "drops", "instance_events"] {
        t.db.execute(
            &format!("UPDATE {table} SET run_id='castle' WHERE id='run-owned'"),
            [],
        )
        .unwrap();
    }
    ids(&t.recap("visit"), &[]);
}

#[test]
fn same_instance_reentry_merges_periods() {
    let t = Fixture::new();
    t.visit("early", 10, Some(20), Some("instance"));
    t.visit("later", 30, Some(40), Some("instance"));
    t.visit("different", 50, Some(60), Some("elsewhere"));
    t.visit("foreign", 70, Some(80), Some("instance"));
    t.db.execute(
        "UPDATE map_visits SET session_id='other-session' WHERE id='foreign'",
        [],
    )
    .unwrap();
    for (id, at) in [
        ("early-fight", 10),
        ("gap", 25),
        ("late-fight", 30),
        ("other-realm", 50),
        ("other-visit", 70),
    ] {
        t.contents(id, at);
    }
    let recap = t.recap("later");
    ids(&recap, &["early-fight", "late-fight"]);
    assert_eq!(recap.visit_id, "later");
    assert_eq!(
        recap
            .periods
            .iter()
            .map(|p| p.visit_id.as_str())
            .collect::<Vec<_>>(),
        ["early", "later"]
    );
    assert_eq!(recap.periods[0].started, Timestamp(10));
    assert_eq!(recap.periods[1].ended, Some(Timestamp(40)));
}

#[test]
fn null_instance_key_is_single_period() {
    let t = Fixture::new();
    t.visit("early", 10, Some(20), None);
    t.visit("late", 30, Some(40), None);
    t.contents("early-fight", 10);
    t.contents("late-fight", 30);
    let recap = t.recap("late");
    ids(&recap, &["late-fight"]);
    assert_eq!(recap.periods.len(), 1);
}

#[test]
fn open_period_ends_at_session_last_traffic() {
    let t = Fixture::new();
    t.visit("open", 10, None, None);
    t.db.execute(
        "UPDATE play_sessions SET last_traffic=20 WHERE id='session'",
        [],
    )
    .unwrap();
    for (id, at) in [("inside", 19), ("at-last-traffic", 20), ("after", 21)] {
        t.contents(id, at);
    }
    let recap = t.recap("open");
    ids(&recap, &["inside"]);
    assert_eq!(recap.periods[0].ended, None);
}

#[test]
fn overlapping_periods_do_not_duplicate_contents() {
    let t = Fixture::new();
    t.visit("a", 10, Some(30), Some("instance"));
    t.visit("b", 20, Some(40), Some("instance"));
    t.contents("overlap", 25);
    ids(&t.recap("a"), &["overlap"]);
}

#[test]
fn announcements_are_realm_event_killed_only() {
    let t = Fixture::new();
    t.visit("visit", 10, Some(20), None);
    t.contents("kill", 11);
    t.contents("other", 12);
    t.db.execute("UPDATE instance_events SET kind='player_left',json='invalid excluded event' WHERE id='other'", []).unwrap();
    assert_eq!(
        t.recap("visit")
            .announcements
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["kill"]
    );
}

#[test]
fn limits_set_flags() {
    let t = Fixture::new();
    t.visit("visit", 0, Some(10000), None);
    t.db.execute_batch("BEGIN").unwrap();
    for i in 0..2001 {
        t.contents(&format!("item-{i:04}"), i);
    }
    // Exercise detail limits independently of the historical PB scan.
    t.db.execute("UPDATE fights SET boss_type=NULL", [])
        .unwrap();
    t.db.execute_batch("COMMIT").unwrap();
    let recap = t.recap("visit");
    assert_eq!(recap.fights.len(), 2001);
    assert_eq!(recap.boss_fights.len(), 64);
    assert_eq!(recap.announcements.len(), 2000);
    assert_eq!(recap.drops.len(), 2000);
    assert!(recap.boss_fights_limited && recap.announcements_limited && recap.drops_limited);
    assert_eq!(recap.drops.last().unwrap().id, "item-1999");
    t.db.execute(
        "UPDATE drops SET session_id='other-session' WHERE id='item-2000'",
        [],
    )
    .unwrap();
    t.db.execute(
        "UPDATE instance_events SET session_id='other-session' WHERE id='item-2000'",
        [],
    )
    .unwrap();
    t.db.execute(
        "UPDATE fights SET session_id='other-session' WHERE started>=64",
        [],
    )
    .unwrap();
    let exact = t.recap("visit");
    assert_eq!(exact.boss_fights.len(), 64);
    assert_eq!(exact.drops.len(), 2000);
    assert_eq!(exact.announcements.len(), 2000);
    assert!(!exact.boss_fights_limited && !exact.drops_limited && !exact.announcements_limited);
}

#[test]
fn boss_detail_budget_and_unreadable_details_preserve_summaries() {
    let t = Fixture::new();
    t.visit("visit", 0, Some(100), None);
    for i in 0..3 {
        t.fight(&format!("fight-{i}"), i, 100.);
    }
    // Event-boss classification still loads detail without catalog PB eligibility.
    t.db.execute(
        "UPDATE fights SET boss_type=NULL,json=json || substr(?1,1,?2-length(CAST(json AS BLOB)))",
        params![" ".repeat(8 * 1024 * 1024), 8 * 1024 * 1024],
    )
    .unwrap();
    let recap = t.recap("visit");
    assert_eq!(recap.fights.len(), 3);
    assert_eq!(recap.boss_fights.len(), 2);
    assert!(recap.boss_fights_limited);
    t.db.execute("UPDATE fights SET json='broken' WHERE id='fight-0'", [])
        .unwrap();
    let recap = t.recap("visit");
    assert_eq!(recap.fights.len(), 3);
    assert_eq!(recap.boss_fights.len(), 1);
    assert!(recap.boss_fights_limited);
}

#[test]
fn fame_delta_uses_samples_inside_periods() {
    let t = Fixture::new();
    t.visit("early", 10, Some(20), Some("instance"));
    t.visit("late", 30, Some(40), Some("instance"));
    assert_eq!(t.recap("early").fame_delta, None);
    t.fame(1, 9, 1, "self", "session");
    t.fame(2, 10, 100, "self", "session");
    assert_eq!(t.recap("early").fame_delta, None);
    for (sequence, at, fame, character, session) in [
        (3, 19, 110, "self", "session"),
        (4, 20, 150, "self", "session"),
        (5, 25, 200, "self", "session"),
        (6, 30, 200, "self", "session"),
        (10, 30, 220, "self", "session"),
        (7, 39, 999, "other", "session"),
        (8, 39, 999, "self", "other-session"),
        (9, 40, 999, "self", "session"),
    ] {
        t.fame(sequence, at, fame, character, session);
    }
    assert_eq!(t.recap("early").fame_delta, Some(30));
    t.db.execute("UPDATE map_visits SET character_id=NULL", [])
        .unwrap();
    assert_eq!(t.recap("early").fame_delta, None);
}

#[test]
fn fame_delta_none_on_regression() {
    let t = Fixture::new();
    t.visit("early", 10, Some(20), Some("instance"));
    t.visit("late", 30, Some(40), Some("instance"));
    t.fame(1, 10, 100, "self", "session");
    t.fame(2, 19, 90, "self", "session");
    t.fame(3, 30, 200, "self", "session");
    t.fame(4, 39, 220, "self", "session");
    assert_eq!(t.recap("early").fame_delta, None);
}

#[test]
fn fame_delta_none_without_a_usable_pair() {
    let t = Fixture::new();
    t.visit("early", 10, Some(20), Some("instance"));
    t.visit("late", 30, Some(40), Some("instance"));
    t.visit("empty", 50, Some(60), Some("instance"));
    t.fame(1, 10, 100, "self", "session");
    t.fame(2, 30, 200, "self", "session");
    assert_eq!(t.recap("early").fame_delta, None);
}

#[test]
fn single_sample_interval_contributes_nothing() {
    let t = Fixture::new();
    t.visit("early", 10, Some(20), Some("instance"));
    t.visit("late", 30, Some(40), Some("instance"));
    t.fame(1, 10, 100, "self", "session");
    t.fame(2, 19, 110, "self", "session");
    t.fame(3, 30, 200, "self", "session");
    assert_eq!(t.recap("early").fame_delta, Some(10));
}

#[test]
fn best_dps_uses_each_fight_and_earlier_eligible_baselines() {
    let t = Fixture::new();
    t.visit("visit", 10, Some(100), None);
    t.fight("baseline", 1, 100.);
    t.fight("record", 10, 200.);
    t.fight("tie", 20, 200.);
    let mut invalid = t.fight("ineligible", 30, 999.);
    invalid.complete_capture = false;
    t.db.execute(
        "UPDATE fights SET json=?1 WHERE id='ineligible'",
        [serde_json::to_string(&invalid).unwrap()],
    )
    .unwrap();
    t.fight("second-record", 40, 300.);
    t.fight("later", 100, 1000.);
    assert_eq!(
        t.recap("visit").best_dps,
        vec![
            BossDpsRecord {
                fight_id: "record".into(),
                boss_type: 42,
                dps: 200.,
                previous_best_dps: 100.
            },
            BossDpsRecord {
                fight_id: "second-record".into(),
                boss_type: 42,
                dps: 300.,
                previous_best_dps: 200.
            }
        ]
    );
}

#[test]
fn best_dps_records_extend_beyond_the_boss_detail_cap() {
    let t = Fixture::new();
    t.visit("visit", 10, Some(100), None);
    t.db.execute_batch("BEGIN").unwrap();
    for i in 0..65 {
        t.fight(&format!("fight-{i:02}"), 10 + i, 100.);
    }
    t.fight("record", 80, 200.);
    t.db.execute_batch("COMMIT").unwrap();
    let recap = t.recap("visit");
    assert_eq!(recap.boss_fights.len(), 64);
    assert!(recap.boss_fights_limited);
    assert_eq!(
        recap.best_dps,
        vec![BossDpsRecord {
            fight_id: "record".into(),
            boss_type: 42,
            dps: 200.,
            previous_best_dps: 100.
        }]
    );
}

#[test]
fn chain_when_visit_led_into_castle() {
    let t = Fixture::new();
    t.visit("early", 10, Some(20), Some("instance"));
    t.visit("late", 30, Some(40), Some("instance"));
    t.castle(40, true);
    let recap = t.recap("early");
    assert_eq!(recap.title, "Synthetic Hydra");
    let chain = recap.chain.unwrap();
    assert_eq!(chain.steps.len(), 2);
    assert_eq!(chain.steps[0].visit_id, "late");
    assert_eq!(chain.steps[0].name, "Synthetic Hydra");
}

#[test]
fn title_requires_direct_castle_and_entered_from_realm() {
    let t = Fixture::new();
    t.visit("visit", 10, Some(20), None);
    t.db.execute(
        "UPDATE map_visits SET json='{\"context\":{\"realm_name\":\"Wrong source\"}}'",
        [],
    )
    .unwrap();
    t.castle(30, false);
    assert_eq!(t.recap("visit").title, "Synthetic Realm");
    t.db.execute(
        "UPDATE runs SET json=json_set(json,'$.context.entered_from_realm',json('true'))",
        [],
    )
    .unwrap();
    assert_eq!(t.recap("visit").title, "Synthetic Hydra");
    t.visit("hub", 20, Some(30), None);
    t.db.execute("UPDATE map_visits SET kind='nexus' WHERE id='hub'", [])
        .unwrap();
    let recap = t.recap("visit");
    assert_eq!(recap.title, "Synthetic Realm");
    assert_eq!(recap.chain, None);
}

#[test]
fn visit_beyond_journey_cap_keeps_contents_and_title_without_chain() {
    let t = Fixture::new();
    t.db.execute_batch("BEGIN").unwrap();
    for i in 0..2000 {
        t.visit(&format!("hub-{i:04}"), i, Some(i + 1), None);
    }
    t.db.execute("UPDATE map_visits SET kind='nexus'", [])
        .unwrap();
    t.visit("visit", 3000, Some(3010), None);
    t.contents("inside", 3001);
    t.castle(3010, true);
    t.db.execute_batch("COMMIT").unwrap();
    let recap = t.recap("visit");
    ids(&recap, &["inside"]);
    assert_eq!(recap.title, "Synthetic Hydra");
    assert_eq!(recap.chain, None);
}

#[test]
fn non_realm_or_unknown_visit_is_none() {
    let t = Fixture::new();
    t.visit("hub", 10, Some(20), None);
    t.db.execute("UPDATE map_visits SET kind='nexus'", [])
        .unwrap();
    assert_eq!(t.read("hub").unwrap(), None);
    assert_eq!(t.read("unknown").unwrap(), None);
    for id in [String::new(), "x".repeat(257), "bad\nvisit".into()] {
        assert!(matches!(t.read(&id), Err(StoreError::InvalidEvent)));
    }
}

#[test]
fn live_visit_id_format() {
    let mut t = Fixture::new();
    let mut session = demo::demo_snapshot().session.unwrap();
    session.id = "synthetic-live-session".into();
    session.started = Timestamp(0);
    session.last_traffic = Timestamp(100);
    session.ended = None;
    let mut events = vec![EngineEvent::SessionUpdated(session); 41];
    events.push(EngineEvent::MapEntered {
        realm_instance_key: Some("instance".into()),
        at: Timestamp(10),
        name: "Synthetic Realm".into(),
        kind: AreaKind::Realm,
    });
    let envelopes = events
        .into_iter()
        .enumerate()
        .map(|(i, event)| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic-live".into(),
            sequence: Sequence(Timestamp(i as u64 + 1)),
            at: Timestamp(10),
            event,
        })
        .collect::<Vec<_>>();
    crate::common::account_scope_fixture::apply_batch(&mut t.db, &envelopes).unwrap();
    assert_eq!(
        t.recap("synthetic-live:visit:42").periods[0].visit_id,
        "synthetic-live:visit:42"
    );
}

#[test]
fn visit_recap_plans_avoid_base_scans() {
    let t = Fixture::new();
    for (file, args) in [
        ("visit_recap.sql", vec!["visit"]),
        (
            "visit_periods.sql",
            vec![
                "session",
                "instance",
                crate::common::account_scope_fixture::ACCOUNT,
                "visit",
            ],
        ),
        ("visit_castle.sql", vec!["session", "10", "visit"]),
        (
            "visit_fights.sql",
            vec![
                "session",
                "[[10,20],[30,40]]",
                crate::common::account_scope_fixture::ACCOUNT,
            ],
        ),
        (
            "visit_bosses.sql",
            vec![
                "session",
                "[[10,20],[30,40]]",
                crate::common::account_scope_fixture::ACCOUNT,
            ],
        ),
        ("visit_records.sql", vec!["session", "[[10,20],[30,40]]"]),
        (
            "visit_drops.sql",
            vec![
                "session",
                "[[10,20],[30,40]]",
                crate::common::account_scope_fixture::ACCOUNT,
            ],
        ),
        (
            "visit_announcements.sql",
            vec!["session", "[[10,20],[30,40]]"],
        ),
        (
            "visit_fame.sql",
            vec![
                "session",
                "[[10,20],[30,40]]",
                "self",
                crate::common::account_scope_fixture::ACCOUNT,
            ],
        ),
    ] {
        let sql = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join(file),
        )
        .unwrap();
        let plan =
            t.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
        for line in &plan {
            if let Some(table) = line
                .strip_prefix("SCAN ")
                .and_then(|s| s.split_whitespace().next())
            {
                assert!(
                    ![
                        "map_visits",
                        "runs",
                        "fights",
                        "fight_summaries",
                        "drops",
                        "instance_events",
                        "fame_samples",
                        "play_sessions"
                    ]
                    .contains(&table),
                    "{file}: {plan:?}"
                );
            }
        }
        assert!(
            plan.iter().any(|line| line.starts_with("SEARCH ")),
            "{file}: {plan:?}"
        );
        let interval_index = match file {
            "visit_fights.sql" | "visit_bosses.sql" | "visit_records.sql" => {
                Some(("fights_session", "started"))
            }
            "visit_drops.sql" => Some(("drops_session", "at")),
            "visit_announcements.sql" => Some(("instance_events_session", "at")),
            "visit_fame.sql" => Some(("fame_session", "at")),
            _ => None,
        };
        if let Some((index, time)) = interval_index {
            assert!(
                plan.iter().any(|line| line.contains(index)
                    && line.contains(&format!("session_id=? AND {time}>? AND {time}<?"))),
                "{file}: {plan:?}"
            );
        }
    }
}
