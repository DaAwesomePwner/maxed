//! Invented summaries derived from the admitted demo; no gameplay recordings.
use maxed_store::*;
use maxed_types::*;
use rusqlite::{params, Connection};
mod common;

#[path = "common/alerts_workload.rs"]
mod alerts_workload;

#[test]
fn alerts_workload_closes_database_before_removing_windows_fixture() {
    let fixture = alerts_workload::query_fixture(100);
    let directory = fixture.path.parent().unwrap().to_owned();
    assert!(fixture.path.exists());
    std::mem::drop(fixture);
    assert!(
        !directory.exists(),
        "temporary SQLite fixture must be removed"
    );
}

#[test]
fn alerts_realistic_and_stress_queries() {
    let mut fixture = alerts_workload::query_fixture(alerts_workload::ORDINARY);
    assert_eq!(
        alerts_workload::count(&fixture.db, "SELECT count(*) FROM alerts"),
        alerts_workload::ORDINARY as i64
    );
    fixture.validate_pages();
    fixture.add_alerts(alerts_workload::STRESS);
    assert_eq!(
        alerts_workload::count(&fixture.db, "SELECT count(*) FROM alerts"),
        alerts_workload::STRESS as i64
    );
    fixture.validate_pages();
}

#[test]
#[ignore = "release performance gate; run serially with --ignored --nocapture"]
fn alerts_realistic_queries_thirty_warm_reads() {
    alerts_workload::benchmark_queries(alerts_workload::ORDINARY);
}

#[test]
#[ignore = "release performance gate; run serially with --ignored --nocapture"]
fn alerts_stress_queries_thirty_warm_reads() {
    alerts_workload::benchmark_queries(alerts_workload::STRESS);
}

#[test]
#[ignore = "release summary performance gate; run serially with --ignored --nocapture"]
fn alerts_summaries_thirty_warm_reads() {
    for n in [alerts_workload::ORDINARY, alerts_workload::STRESS] {
        alerts_workload::benchmark_summaries(n);
    }
}

#[test]
#[ignore = "release performance gate; run serially with --ignored --nocapture"]
fn alert_retirement_burst() {
    alerts_workload::benchmark_burst();
}

#[test]
#[ignore = "release performance gate; run serially with --ignored --nocapture"]
fn alerts_cold_upgrade_budget() {
    alerts_workload::benchmark_cold_upgrade();
}

#[test]
fn alerts_burst_shapes_have_expected_counts_and_event_bytes() {
    for (stress, fights, players, sources, buckets) in [
        (false, 400, 3200, 9600, 5440),
        (true, 3000, 24770, 74310, 41920),
    ] {
        let (mut actual_players, mut actual_sources, mut actual_buckets, mut bosses) = (0, 0, 0, 0);
        let mut max_event = 0;
        for i in 0..fights {
            let f = alerts_workload::burst_fight(i, stress);
            actual_players += f.players.len();
            actual_sources += f.players.iter().map(|p| p.sources.len()).sum::<usize>();
            actual_buckets += f.buckets.len();
            bosses += usize::from(f.boss_type.is_some());
            max_event = max_event.max(alerts_workload::bytes(&alerts_workload::envelope(
                i as u64 + 1,
                EngineEvent::FightEnded(f),
            )));
        }
        assert_eq!(
            (actual_players, actual_sources, actual_buckets, bosses),
            (players, sources, buckets, 20)
        );
        assert!(max_event <= 8 * 1024 * 1024);
    }
}

fn fight(id: &str, dps: f64, character: &str) -> FightView {
    let mut f = demo::demo_snapshot().fights.remove(0);
    f.id = id.into();
    f.run_id = None;
    f.boss_type = Some(42);
    f.complete_capture = true;
    f.issues.clear();
    f.ended = Some(Timestamp(10_000_000));
    f.end_reason = Some(FightEnd::MapChanged);
    f.first_hit = Some(Timestamp(1_000_000));
    f.last_hit = Some(Timestamp(2_000_000));
    f.players.retain(|p| p.is_local);
    f.players[0].player_id = character.into();
    f.players[0].damage = dps;
    f.players[0].dps = Some(dps);
    for s in &mut f.players[0].sources {
        s.estimated = false;
    }
    f
}

#[test]
fn flawless_remains_unsupported_for_clear_complete_capture_and_zero_deaths() {
    let mut h = Harness::new();
    let mut run = demo::demo_run_story().run;
    run.character_id = Some("self".into());
    run.complete_capture = true;
    run.issues.clear();
    run.outcome = Some(Outcome::Cleared);
    run.completed_at = Some(Timestamp(9_000_000));
    run.died_at = None;
    h.send(EngineEvent::RunEnded(run.clone()), AlertLoudness::Banner);
    // No incoming-damage evidence exists in this summary. Absence is not proof.
    for (id, dps) in [("empty-observation", 0.), ("complete-observation", 100.)] {
        let mut f = fight(id, dps, "self");
        f.run_id = Some(run.id.clone());
        f.total_damage = dps;
        f.unattributed_damage = 0.;
        f.buckets.clear();
        for player in &mut f.players {
            player.deaths = 0;
        }
        h.send(EngineEvent::FightEnded(f), AlertLoudness::Banner);
    }
    assert_eq!(h.count("deaths"), 0);
    assert!(h
        .logs()
        .iter()
        .all(|row| row.kind != AlertKind::FlawlessRun));
    assert_eq!(
        h.db()
            .query_row(
                "SELECT count(*) FROM alert_occurrences WHERE kind='flawless_run'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    // Positive controls: source work committed and PB eligibility was evaluated.
    assert_eq!(h.count("runs"), 1);
    assert_eq!(h.count("fights"), 2);
    assert_eq!(h.count("personal_bests"), 1);
    h.send(
        EngineEvent::BagDropped(drop("positive-white", Some(255))),
        AlertLoudness::Banner,
    );
    assert_eq!(h.logs().len(), 1);
    assert_eq!(h.logs()[0].kind, AlertKind::WhiteBag);
}
fn character(id: &str) -> CharacterView {
    let mut c = demo::demo_run_story().character.unwrap();
    c.id = id.into();
    c.identity_verified = true;
    c
}
fn drop(id: &str, rank: Option<u8>) -> DropView {
    let mut d = demo::demo_run_story().drops.remove(0);
    d.id = id.into();
    d.run_id = None;
    d.bag_rank = rank;
    d
}
fn context(level: AlertLoudness) -> AlertContext {
    AlertContext {
        chat: None,
        live: true,
        preferences: AlertPreferences {
            personal_best: level,
            white_bag: level,
            death: level,
            ..Default::default()
        },
        key_pop_dungeon: None,
        bag_window: Some(TimeRange {
            start: Timestamp(0),
            end: Timestamp(99_000_000),
        }),
    }
}
struct Harness {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    writer: StoreWriter,
    seq: u64,
}
impl Harness {
    fn send_window(&mut self, event: EngineEvent, window: Option<TimeRange>) {
        self.seq += 1;
        let mut ctx = context(AlertLoudness::Banner);
        ctx.bag_window = window;
        self.writer
            .try_submit_with_alert_context(common::envelope(self.seq, event), ctx)
            .unwrap();
        self.writer.flush().unwrap();
    }
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: "test-stream".into(),
                flush_millis: 60_000,
                ..Default::default()
            },
        )
        .unwrap();
        let mut h = Self {
            _dir: dir,
            path,
            writer,
            seq: 0,
        };
        h.send(
            EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
            AlertLoudness::Off,
        );
        h.send(
            EngineEvent::CharacterUpdated(character("self")),
            AlertLoudness::Off,
        );
        h
    }
    fn enqueue(&mut self, event: EngineEvent, level: AlertLoudness) -> EventEnvelope {
        self.seq += 1;
        let e = common::envelope(self.seq, event);
        self.submit(e.clone(), level);
        e
    }
    fn submit(&self, mut e: EventEnvelope, level: AlertLoudness) {
        loop {
            match self.writer.try_submit_with_alert_context(e, context(level)) {
                Ok(()) => break,
                Err(SubmitError::Full(v)) => {
                    e = v;
                    self.writer.flush().unwrap();
                }
                Err(e) => panic!("{e:?}"),
            }
        }
    }
    fn send(&mut self, e: EngineEvent, level: AlertLoudness) -> EventEnvelope {
        let e = self.enqueue(e, level);
        self.writer.flush().unwrap();
        e
    }
    fn db(&self) -> Connection {
        Connection::open(&self.path).unwrap()
    }
    fn count(&self, table: &str) -> i64 {
        self.db()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    fn logs(&self) -> Vec<AlertView> {
        open_reader(&self.path)
            .unwrap()
            .list_alerts(AlertFilter {
                kinds: vec![],
                exclude_kinds: vec![],
                kind: None,
                range: None,
                query: String::new(),
                cursor: None,
                limit: 100,
            })
            .unwrap()
            .rows
    }
}

fn bag_context(h: &Harness, id: &str) -> (Option<u32>, Option<TimeRange>) {
    h.logs()
        .into_iter()
        .find_map(|row| match row.details {
            Some(AlertDetails::WhiteBag {
                drop_id,
                ordinal,
                window,
                ..
            }) if drop_id == id => Some((ordinal, window)),
            _ => None,
        })
        .unwrap()
}

#[test]
fn seven_local_days_dst_and_tie_order() {
    const HOUR: u64 = 3_600_000_000;
    // Supplied UTC microseconds for local calendar windows straddling DST.
    for hours in [167, 169] {
        let mut h = Harness::new();
        let window = TimeRange {
            start: Timestamp(100 * HOUR),
            end: Timestamp((100 + hours) * HOUR),
        };
        let at = window.end.0 - 2;
        for (id, time) in [
            ("outside-start", window.start.0 - 1),
            ("inclusive-start", window.start.0),
            ("exclusive-end", window.end.0),
            ("outside-end", window.end.0 + 1),
            ("later", at + 1),
            ("z-same-time", at),
            ("a-same-time", at),
            ("m-current", at),
        ] {
            let mut d = drop(id, Some(255));
            d.at = Timestamp(time);
            h.send_window(EngineEvent::BagDropped(d), Some(window.clone()));
        }
        assert_eq!(
            bag_context(&h, "inclusive-start"),
            (Some(1), Some(window.clone()))
        );
        assert_eq!(
            bag_context(&h, "m-current"),
            (Some(3), Some(window.clone()))
        );
        assert_eq!(bag_context(&h, "exclusive-end"), (None, None));
        assert_eq!(bag_context(&h, "outside-start"), (None, None));
        h.writer.claim_alerts(32).unwrap();
        let mut earlier = drop("late-insertion", Some(255));
        earlier.at = Timestamp(window.start.0 + 1);
        h.send_window(EngineEvent::BagDropped(earlier), Some(window.clone()));
        let mut revised = drop("m-current", Some(255));
        revised.at = Timestamp(at + HOUR);
        revised.source_enemy = Some(42);
        h.send_window(EngineEvent::DropUpdated(revised), None);
        assert_eq!(
            bag_context(&h, "m-current"),
            (Some(3), Some(window.clone()))
        );
        let mut next = drop("n-new", Some(255));
        next.at = Timestamp(at);
        h.send_window(EngineEvent::BagDropped(next), Some(window.clone()));
        assert_eq!(bag_context(&h, "n-new"), (Some(5), Some(window)));
        let plan: String = h.db().query_row("EXPLAIN QUERY PLAN SELECT count(*) FROM drops WHERE bag_type=1 AND at>=2 AND at<9 AND (at<8 OR (at=8 AND id<='m'))", [], |r| r.get(3)).unwrap();
        assert!(plan.contains("COVERING INDEX drops_bag"), "{plan}");
    }
}

#[test]
fn same_bag_not_same_item_or_rank() {
    let mut h = Harness::new();
    let mut first = drop("a", Some(255));
    first.at = Timestamp(10);
    first.contents[0].quantity = 100;
    first.items = vec![first.contents[0].item_type; 20];
    h.send(
        EngineEvent::BagDropped(first.clone()),
        AlertLoudness::Banner,
    );
    h.send(
        EngineEvent::DropUpdated(first.clone()),
        AlertLoudness::Banner,
    );
    assert_eq!(bag_context(&h, "a").0, Some(1));
    let mut other_type = first.clone();
    other_type.id = "b".into();
    other_type.bag_type += 1;
    h.send(EngineEvent::BagDropped(other_type), AlertLoudness::Banner);
    // Nonwhite observations of the same type still contribute.
    let mut other_rank = first.clone();
    other_rank.id = "c".into();
    other_rank.bag_rank = Some(1);
    h.send(EngineEvent::BagDropped(other_rank), AlertLoudness::Banner);
    h.send(
        EngineEvent::CharacterUpdated(character("other")),
        AlertLoudness::Off,
    );
    let mut run = demo::demo_run_story().run;
    run.character_id = Some("other".into());
    h.send(EngineEvent::RunStarted(run.clone()), AlertLoudness::Off);
    let mut current = first.clone();
    current.id = "d".into();
    current.run_id = Some(run.id);
    current.contents.clear();
    current.items.clear();
    current.contents_known = false;
    current.source_enemy = None;
    h.send(EngineEvent::BagDropped(current), AlertLoudness::Banner);
    assert_eq!(bag_context(&h, "d").0, Some(3));
    assert_eq!(h.count("drops"), 4);
    assert_eq!(h.logs().len(), 3);
    let mut isolated = Harness::new();
    isolated.send(EngineEvent::BagDropped(first), AlertLoudness::Banner);
    assert_eq!(bag_context(&isolated, "a").0, Some(1));
}

fn alert_filter() -> AlertFilter {
    AlertFilter {
        kinds: vec![],
        exclude_kinds: vec![],
        kind: None,
        range: None,
        query: String::new(),
        cursor: None,
        limit: 100,
    }
}

#[test]
fn alert_pages_escape_literal_search_and_validate_unicode_bounds() {
    let mut h = Harness::new();
    for (id, at, text, kind) in [
        ("a", 1, "Legacy 100%_\\ literal", AlertKind::Death),
        ("b", 2, "Legacy 100xx literal", AlertKind::Death),
        ("c", 2, "Legacy 100%_\\ literal", AlertKind::WhiteBag),
    ] {
        h.send(
            EngineEvent::AlertFired(AlertView {
                id: id.into(),
                at: Timestamp(at),
                kind,
                run_id: None,
                explanation: text.into(),
                details: None,
                presentation: None,
            }),
            AlertLoudness::Off,
        );
    }
    let reader = open_reader(&h.path).unwrap();
    let mut filter = alert_filter();
    filter.query = "  100%_\\  ".into();
    assert_eq!(
        reader
            .list_alerts(filter.clone())
            .unwrap()
            .rows
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        ["c", "a"]
    );
    filter.kind = Some(AlertKind::Death);
    assert_eq!(reader.list_alerts(filter.clone()).unwrap().rows[0].id, "a");
    filter.query = "' OR 1=1 --".into();
    assert!(reader.list_alerts(filter).unwrap().rows.is_empty());
    for query in ["x".repeat(129), "😀".repeat(129), "a\0b".into()] {
        assert!(reader
            .list_alerts(AlertFilter {
                query,
                ..alert_filter()
            })
            .is_err());
    }
    assert!(reader
        .list_alerts(AlertFilter {
            query: "😀".repeat(128),
            ..alert_filter()
        })
        .is_ok());
    let mut filter = alert_filter();
    filter.limit = 1;
    let mut ids = Vec::new();
    loop {
        let page = reader.list_alerts(filter.clone()).unwrap();
        ids.extend(page.rows.into_iter().map(|r| r.id));
        filter.cursor = page.next;
        if filter.cursor.is_none() {
            break;
        }
    }
    assert_eq!(ids, ["c", "b", "a"]);
    let range = TimeRange {
        start: Timestamp(1),
        end: Timestamp(2),
    };
    assert_eq!(
        reader
            .list_alerts(AlertFilter {
                range: Some(range),
                ..alert_filter()
            })
            .unwrap()
            .rows[0]
            .id,
        "a"
    );
    for (start, end) in [(2, 2), (3, 2), (0, u64::MAX)] {
        assert!(reader
            .list_alerts(AlertFilter {
                range: Some(TimeRange {
                    start: Timestamp(start),
                    end: Timestamp(end)
                }),
                ..alert_filter()
            })
            .is_err());
    }
}

#[test]
fn alert_pages_bound_legacy_json_and_rows() {
    let h = Harness::new();
    let db = h.db();
    for n in 0..101 {
        let view = AlertView {
            id: format!("legacy-{n:03}"),
            at: Timestamp(1),
            kind: AlertKind::Death,
            run_id: None,
            explanation: "legacy".into(),
            details: None,
            presentation: None,
        };
        db.execute(
            "INSERT INTO alerts VALUES(?1,NULL,1,'death',?2)",
            params![view.id, serde_json::to_string(&view).unwrap()],
        )
        .unwrap();
    }
    let reader = open_reader(&h.path).unwrap();
    let page = reader.list_alerts(alert_filter()).unwrap();
    assert_eq!(page.rows.len(), 100);
    assert!(page.next.is_some());
    db.execute(
        "UPDATE alerts SET json='malformed' WHERE id='legacy-100'",
        [],
    )
    .unwrap();
    assert!(reader.list_alerts(alert_filter()).is_err());
    db.execute(
        "UPDATE alerts SET json=?1 WHERE id='legacy-100'",
        ["x".repeat(2 * 1024 * 1024 + 1)],
    )
    .unwrap();
    assert!(matches!(
        reader.list_alerts(alert_filter()),
        Err(StoreError::Limit)
    ));
}
fn legacy_seed() -> Harness {
    let mut h = Harness::new();
    for (id, dps) in [("old-100", 100.), ("old-250", 250.)] {
        h.send(
            EngineEvent::FightEnded(fight(id, dps, "self")),
            AlertLoudness::Off,
        );
    }
    let mut estimated = fight("estimated", 900., "self");
    estimated.players[0].sources[0].estimated = true;
    h.send(EngineEvent::FightEnded(estimated), AlertLoudness::Off);
    h.send(
        EngineEvent::CharacterUpdated(character("other")),
        AlertLoudness::Off,
    );
    h.send(
        EngineEvent::FightEnded(fight("other-800", 800., "other")),
        AlertLoudness::Off,
    );
    for (id, rank) in [("white", Some(255)), ("blue", Some(1)), ("unknown", None)] {
        h.send(EngineEvent::BagDropped(drop(id, rank)), AlertLoudness::Off);
    }
    h.send(
        EngineEvent::PlayerDied {
            at: Timestamp(2),
            character_id: None,
            run_id: None,
        },
        AlertLoudness::Off,
    );
    // Recreate the exact installed v3 schema for the cold-upgrade fixture.
    h.db().execute_batch("DROP TABLE map_visits; DROP INDEX play_sessions_started; DROP TABLE chat_fts; DROP TABLE chat_messages; DROP INDEX drops_source; DROP INDEX runs_dungeon_ended; DROP INDEX drops_id_at; DROP INDEX drops_stats; ALTER TABLE drops DROP COLUMN source_type; DROP TRIGGER pd_player_insert; DROP TRIGGER pd_player_delete; DROP TRIGGER pd_player_update; DROP TRIGGER pd_fight_association; DROP TABLE pd_player_contributions; DROP TABLE pd_run_players; DROP INDEX pd_home_session_runs; DROP INDEX pd_home_today_runs; DROP INDEX pd_run_evidence_kind; DROP INDEX pd_fight_players_totals; DROP INDEX pd_runs_comparison; DROP TABLE event_fts; DROP TABLE instance_events; DROP TABLE alert_occurrences; DROP TABLE personal_bests; DROP INDEX alerts_kind_at; DROP INDEX fights_final_boss; DROP INDEX fights_final_boss_key; DROP INDEX fight_players_identity; DROP INDEX character_aliases_verified; DELETE FROM schema_migrations WHERE version>=4; PRAGMA user_version=3;").unwrap();
    upgrade_history(&h.path).unwrap();
    h
}

#[test]
fn historical_dps_reset_and_same_batch_records() {
    let mut h = legacy_seed();
    assert_eq!(h.count("personal_bests"), 0);
    h.send(
        EngineEvent::FightEnded(fight("new-200", 200., "self")),
        AlertLoudness::Banner,
    );
    h.send(
        EngineEvent::FightEnded(fight("new-260", 260., "self")),
        AlertLoudness::Banner,
    );
    assert_eq!(h.logs().len(), 1);
    h.enqueue(
        EngineEvent::CharacterUpdated(character("batch")),
        AlertLoudness::Off,
    );
    h.enqueue(
        EngineEvent::FightEnded(fight("batch-100", 100., "batch")),
        AlertLoudness::Banner,
    );
    h.enqueue(
        EngineEvent::FightEnded(fight("batch-150", 150., "batch")),
        AlertLoudness::Banner,
    );
    h.writer.flush().unwrap();
    assert_eq!(h.logs().len(), 2);
}

#[test]
fn seeded_receipts_are_limited_and_historical_revisions_stay_silent() {
    let mut h = legacy_seed();
    assert_eq!(h.count("personal_bests"), 0);
    assert_eq!(h.count("alert_occurrences"), 4);
    assert_eq!(h.db().query_row("SELECT count(*) FROM alert_occurrences WHERE state='pending' OR committed_at IS NOT NULL",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    for (id, dps) in [("old-250", 250.), ("old-100", 100.), ("old-100", 1000.)] {
        send_timed(
            &mut h,
            EngineEvent::FightEnded(fight(id, dps, "self")),
            false,
        );
    }
    assert!(h.logs().is_empty());
    assert_eq!(h.count("alert_occurrences"), 5);
    h.send(
        EngineEvent::FightEnded(fight("fresh", 1001., "self")),
        AlertLoudness::Banner,
    );
    assert_eq!(h.logs().len(), 1);
}

#[test]
fn source_baseline_receipt_and_log_rollback_together() {
    for table in ["fights", "personal_bests", "stream_heads"] {
        let mut h = Harness::new();
        h.send(
            EngineEvent::FightEnded(fight("baseline", 100., "self")),
            AlertLoudness::Banner,
        );
        let head = h.seq;
        let db = h.db();
        db.execute_batch(&format!("CREATE TRIGGER fail_write BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT,'injected failure'); END;")).unwrap();
        h.enqueue(
            EngineEvent::FightEnded(fight("winner", 150., "self")),
            AlertLoudness::Banner,
        );
        assert!(h.writer.flush().is_err());
        assert_eq!(h.count("fights"), 1);
        assert_eq!(h.count("alert_occurrences"), 1);
        assert_eq!(h.count("alerts"), 0);
        assert_eq!(
            db.query_row("SELECT dps FROM personal_bests", [], |r| r.get::<_, f64>(0))
                .unwrap(),
            100.
        );
        assert_eq!(h.writer.status().durable.unwrap().0 .0, head);
        assert!(h.writer.claim_alerts(32).unwrap().is_empty());
        db.execute_batch("DROP TRIGGER fail_write").unwrap();
        h.writer.retry().unwrap();
        assert_eq!(h.count("fights"), 2);
        assert_eq!(h.count("alert_occurrences"), 2);
        assert_eq!(h.count("alerts"), 1);
    }
}

#[test]
fn redelivery_and_drop_updates_are_idempotent() {
    let mut h = Harness::new();
    let mut d = drop("bag", None);
    h.send(EngineEvent::BagDropped(d.clone()), AlertLoudness::Banner);
    assert!(h.logs().is_empty());
    let original = d.at;
    d.bag_rank = Some(255);
    d.contents_known = false;
    let event = h.send(EngineEvent::DropUpdated(d.clone()), AlertLoudness::Banner);
    h.submit(event.clone(), AlertLoudness::Off);
    h.writer.flush().unwrap();
    let first = h.logs().remove(0);
    d.at = Timestamp(20_000_000);
    d.contents_known = true;
    d.source_enemy = Some(123);
    h.send(EngineEvent::DropUpdated(d), AlertLoudness::Banner);
    let next = h.logs().remove(0);
    assert_eq!(next.id, first.id);
    assert_eq!(next.at, original);
    assert_eq!(h.count("alert_occurrences"), 1);
    match (first.details, next.details) {
        (
            Some(AlertDetails::WhiteBag {
                ordinal: a,
                window: w,
                ..
            }),
            Some(AlertDetails::WhiteBag {
                ordinal: b,
                window: x,
                source_enemy,
                contents_known,
                ..
            }),
        ) => {
            assert_eq!(a, b);
            assert_eq!(w, x);
            assert_eq!(source_enemy, Some(123));
            assert!(contents_known);
        }
        _ => panic!("white details"),
    }
    assert_eq!(h.writer.claim_alerts(32).unwrap().len(), 1);
    let mut altered = event;
    altered.at = Timestamp(999);
    h.submit(altered, AlertLoudness::Banner);
    assert!(h.writer.flush().is_err());
}

#[test]
fn death_data_survives_without_alert_occurrences() {
    let mut h = Harness::new();
    for level in [
        AlertLoudness::Off,
        AlertLoudness::QuietLogOnly,
        AlertLoudness::Banner,
    ] {
        h.send(
            EngineEvent::PlayerDied {
                at: Timestamp(2),
                character_id: None,
                run_id: None,
            },
            level,
        );
    }
    assert_eq!(h.count("alert_occurrences"), 0);
    assert_eq!(h.logs().len(), 0);
    assert_eq!(h.writer.claim_alerts(32).unwrap().len(), 0);
    assert_eq!(h.count("deaths"), 3);
    let mut f = fight("deaths-counter", 100., "self");
    f.players[0].deaths = 5;
    h.send(EngineEvent::FightEnded(f), AlertLoudness::Banner);
    assert_eq!(h.logs().len(), 0);
    h.send(
        EngineEvent::FightEnded(fight("off-record", 150., "self")),
        AlertLoudness::Off,
    );
    h.send(
        EngineEvent::FightEnded(fight("quiet-record", 200., "self")),
        AlertLoudness::QuietLogOnly,
    );
    assert_eq!(h.logs().len(), 1);
    assert!(h.writer.claim_alerts(32).unwrap().is_empty());
}

#[test]
fn claim_before_effect_crash_matrix() {
    let mut h = Harness::new();
    for i in 0..35 {
        h.send(
            EngineEvent::BagDropped(drop(&format!("claim-{i}"), Some(255))),
            AlertLoudness::Banner,
        );
    }
    assert!(h.writer.claim_alerts(0).is_err());
    assert!(h.writer.claim_alerts(33).is_err());
    let first = h.writer.claim_alerts(32).unwrap();
    assert_eq!(first.len(), 32);
    assert!(first
        .windows(2)
        .all(|w| w[0].source_sequence.0 < w[1].source_sequence.0));
    assert_eq!(
        h.db()
            .query_row(
                "SELECT count(*) FROM alert_occurrences WHERE state='attempted'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        32
    );
    assert_eq!(h.writer.claim_alerts(32).unwrap().len(), 3);
    assert!(h.writer.claim_alerts(32).unwrap().is_empty());
    h.send(
        EngineEvent::BagDropped(drop("claim-last", Some(255))),
        AlertLoudness::Banner,
    );
    h.writer.shutdown().unwrap();
    let writer = start_writer(
        h.path.clone(),
        WriterConfig {
            stream_id: "new-stream".into(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(writer.claim_alerts(32).unwrap().is_empty());
    assert_eq!(
        Connection::open(&h.path)
            .unwrap()
            .query_row("SELECT count(*) FROM alerts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        36
    );
    writer.shutdown().unwrap();
}

#[test]
fn pb_emission_time_and_bounds_are_preserved() {
    let mut h = Harness::new();
    h.send(
        EngineEvent::FightEnded(fight("baseline", 100., "self")),
        AlertLoudness::Banner,
    );
    let emitted = h.send(
        EngineEvent::FightEnded(fight("record", 101., "self")),
        AlertLoudness::Banner,
    );
    let claim = h.writer.claim_alerts(1).unwrap().remove(0);
    assert_eq!(claim.committed_at, emitted.at);
    assert_eq!(claim.view.at, Timestamp(2_000_000));
    let reader = open_reader(&h.path).unwrap();
    for limit in [0, 101] {
        assert!(reader
            .list_alerts(AlertFilter {
                kinds: vec![],
                exclude_kinds: vec![],
                kind: None,
                range: None,
                query: String::new(),
                cursor: None,
                limit
            })
            .is_err());
    }
    assert!(reader
        .list_alerts(AlertFilter {
            kinds: vec![],
            exclude_kinds: vec![],
            kind: None,
            range: None,
            query: "x".repeat(2049),
            cursor: None,
            limit: 1
        })
        .is_err());
    let db = h.db();
    let plan:String=db.query_row("EXPLAIN QUERY PLAN SELECT id FROM alert_occurrences WHERE state='pending' AND source_stream_id='test-stream' ORDER BY source_sequence,id LIMIT 32",[],|r|r.get(3)).unwrap();
    assert!(plan.contains("alert_occurrences_pending"));
    let plan:String=db.query_row("EXPLAIN QUERY PLAN SELECT json FROM alerts WHERE kind='death' ORDER BY at,id LIMIT 100",[],|r|r.get(3)).unwrap();
    assert!(plan.contains("alerts_kind_at"));
}

#[test]
fn malformed_upgrade_rolls_back_and_checksums_stay_installed() {
    let h = Harness::new();
    let db = h.db();
    let f = fight("bad", 100., "self");
    db.execute(
        "INSERT INTO fights VALUES(?1,NULL,?2,1,2,42,100,1,'broken')",
        params![f.id, f.session_id],
    )
    .unwrap();
    db.execute_batch("DROP TABLE map_visits; DROP INDEX play_sessions_started; DROP TABLE chat_fts; DROP TABLE chat_messages; DROP INDEX drops_source; DROP INDEX runs_dungeon_ended; DROP INDEX drops_id_at; DROP INDEX drops_stats; ALTER TABLE drops DROP COLUMN source_type; DROP TRIGGER pd_player_insert; DROP TRIGGER pd_player_delete; DROP TRIGGER pd_player_update; DROP TRIGGER pd_fight_association; DROP TABLE pd_player_contributions; DROP TABLE pd_run_players; DROP INDEX pd_home_session_runs; DROP INDEX pd_home_today_runs; DROP INDEX pd_run_evidence_kind; DROP INDEX pd_fight_players_totals; DROP INDEX pd_runs_comparison; DROP TABLE event_fts; DROP TABLE instance_events; DROP TABLE alert_occurrences; DROP TABLE personal_bests; DROP INDEX alerts_kind_at; DROP INDEX fights_final_boss; DROP INDEX fights_final_boss_key; DROP INDEX fight_players_identity; DROP INDEX character_aliases_verified; DELETE FROM schema_migrations WHERE version>=4; PRAGMA user_version=3;").unwrap();
    let sums: Vec<String> = db
        .prepare("SELECT checksum FROM schema_migrations ORDER BY version")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(upgrade_history(&h.path).is_err());
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='personal_bests'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    db.execute(
        "UPDATE fights SET json=?1",
        [serde_json::to_string(&f).unwrap()],
    )
    .unwrap();
    upgrade_history(&h.path).unwrap();
    let after: Vec<String> = db
        .prepare("SELECT checksum FROM schema_migrations WHERE version<=3 ORDER BY version")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(sums, after);
}

#[test]
fn delayed_identity_merge_seeds_silently_and_does_not_lower_verified_record() {
    let mut h = Harness::new();
    h.send(
        EngineEvent::FightEnded(fight("verified", 200., "self")),
        AlertLoudness::Banner,
    );
    let mut provisional = character("provisional");
    provisional.identity_verified = false;
    h.send(
        EngineEvent::CharacterUpdated(provisional),
        AlertLoudness::Off,
    );
    h.send(
        EngineEvent::FightEnded(fight("provisional-record", 250., "provisional")),
        AlertLoudness::Banner,
    );
    h.send(
        EngineEvent::CharacterMerged {
            provisional_id: "provisional".into(),
            verified_id: "self".into(),
        },
        AlertLoudness::Banner,
    );
    assert!(h.logs().is_empty());
    assert_eq!(
        h.db()
            .query_row(
                "SELECT dps FROM personal_bests WHERE character_id='self'",
                [],
                |r| r.get::<_, f64>(0)
            )
            .unwrap(),
        250.
    );
    h.send(
        EngineEvent::FightEnded(fight("new-record", 251., "self")),
        AlertLoudness::Banner,
    );
    assert_eq!(h.logs().len(), 1);
}

#[test]
fn cleared_then_died_preserves_both_facts_and_alertfired_cannot_replace_details() {
    let mut h = Harness::new();
    let mut run = demo::demo_run_story().run;
    run.character_id = Some("self".into());
    run.completed_at = Some(Timestamp(3_000_000));
    run.outcome = Some(Outcome::Cleared);
    h.send(EngineEvent::RunEnded(run.clone()), AlertLoudness::Banner);
    assert!(h.logs().is_empty());
    run.died_at = Some(Timestamp(4_000_000));
    run.outcome = Some(Outcome::Died);
    h.send(EngineEvent::RunUpdated(run.clone()), AlertLoudness::Banner);
    assert!(h.logs().is_empty());
    let event = h.send(
        EngineEvent::PlayerDied {
            at: Timestamp(4_000_000),
            character_id: Some("self".into()),
            run_id: Some(run.id.clone()),
        },
        AlertLoudness::Banner,
    );
    h.submit(event, AlertLoudness::Banner);
    h.writer.flush().unwrap();
    assert!(h.logs().is_empty());
    h.send(
        EngineEvent::BagDropped(drop("detail-source", Some(255))),
        AlertLoudness::Banner,
    );
    let original = h.logs().remove(0);
    let mut stale = original.clone();
    stale.details = None;
    stale.presentation = None;
    stale.explanation = "stale".into();
    h.send(EngineEvent::AlertFired(stale), AlertLoudness::Banner);
    assert_eq!(h.logs(), vec![original]);
    assert_eq!(h.count("deaths"), 1);
    let saved = open_reader(&h.path)
        .unwrap()
        .get_run(&run.id)
        .unwrap()
        .unwrap()
        .run;
    assert_eq!(saved.completed_at, run.completed_at);
    assert_eq!(saved.died_at, run.died_at);
}

#[test]
fn oversized_new_alerts_fail_atomically() {
    let mut h = Harness::new();
    let mut alert = AlertView {
        id: "bounded".into(),
        at: Timestamp(1),
        kind: AlertKind::Death,
        run_id: None,
        explanation: "x".repeat(2049),
        details: Some(AlertDetails::Death { character_id: None }),
        presentation: Some(AlertLoudness::Banner),
    };
    h.enqueue(
        EngineEvent::AlertFired(alert.clone()),
        AlertLoudness::Banner,
    );
    assert!(h.writer.flush().is_err());
    assert_eq!(h.count("alerts"), 0);
    alert.id = "x".repeat(257);
    alert.explanation = "bounded".into();
    // The direct batch API shares exactly the same validation/transaction path.
    let mut db = h.db();
    assert!(apply_batch(
        &mut db,
        &[common::envelope(h.seq, EngineEvent::AlertFired(alert))]
    )
    .is_err());
}

fn portal(id: &str) -> EngineEvent {
    EngineEvent::InstanceEvent(InstanceEventView {
        id: id.into(),
        session_id: demo::demo_snapshot().session.unwrap().id,
        run_id: None,
        map: Some("Nexus".into()),
        at: Timestamp(2),
        provisional: false,
        kind: InstanceEventKind::PortalOpened {
            opener_is_local: None,
            opener: None,
            portal_type: Some(301),
            dungeon: Some("The Void".into()),
        },
    })
}
#[test]
fn key_pop_occurrence_uses_instance_id_and_dungeon_filter() {
    let mut h = Harness::new();
    let mut value = serde_json::to_value(context(AlertLoudness::Banner)).unwrap();
    value["key_pop_dungeon"] = serde_json::json!("The Void");
    let ctx: AlertContext = serde_json::from_value(value).unwrap();
    for _ in 0..2 {
        h.seq += 1;
        h.writer
            .try_submit_with_alert_context(common::envelope(h.seq, portal("portal-1")), ctx.clone())
            .unwrap();
        h.writer.flush().unwrap();
    }
    assert_eq!(h.count("instance_events"), 1);
    assert_eq!(h.count("alert_occurrences"), 1);
    assert_eq!(h.logs()[0].explanation, "The Void key popped");
    assert_eq!(h.writer.claim_alerts(32).unwrap().len(), 1);
    let source: String = h
        .db()
        .query_row("SELECT source_key FROM alert_occurrences", [], |r| r.get(0))
        .unwrap();
    assert_eq!(source, "portal-1");
    let mut value = serde_json::to_value(ctx).unwrap();
    value["preferences"]["key_pop_dungeons"] = serde_json::json!(["Lost Halls"]);
    let ctx: AlertContext = serde_json::from_value(value).unwrap();
    h.seq += 1;
    h.writer
        .try_submit_with_alert_context(common::envelope(h.seq, portal("portal-2")), ctx)
        .unwrap();
    h.writer.flush().unwrap();
    assert_eq!(h.count("alert_occurrences"), 1);
    assert_eq!(h.logs().len(), 1);
    h.send(portal("no-live-identity"), AlertLoudness::Banner);
    assert_eq!(h.count("alert_occurrences"), 1);
}

fn timed_run(id: &str, duration: u64) -> RunView {
    let mut run = demo::demo_run_story().run;
    run.id = id.into();
    run.character_id = None;
    run.dungeon = "Lair of Shaitan".into();
    run.area_kind = AreaKind::Dungeon;
    run.started = Timestamp(1_000_000);
    run.completed_at = Some(Timestamp(1_000_000 + duration));
    run.ended = Some(Timestamp(150_000_000));
    run.outcome = Some(Outcome::Cleared);
    run.evidence = vec![EvidenceAt {
        at: run.completed_at.unwrap(),
        evidence: Evidence::Victory,
    }];
    run.complete_capture = false;
    run.issues = vec![CoverageIssue::MissingStats];
    run
}
fn send_timed(h: &mut Harness, event: EngineEvent, live: bool) {
    h.seq += 1;
    let mut ctx = context(AlertLoudness::Banner);
    ctx.live = live;
    h.writer
        .try_submit_with_alert_context(common::envelope(h.seq, event), ctx)
        .unwrap();
    h.writer.flush().unwrap();
}

#[test]
fn run_time_pb_matches_home_and_republish_is_silent() {
    let mut h = Harness::new();
    let baseline = timed_run("prior-clear", 87_000_000);
    send_timed(&mut h, EngineEvent::RunEnded(baseline), true);
    assert!(h.logs().is_empty());
    let mut faster = timed_run("faster-clear", 86_000_000);
    faster.character_id = Some("self".into());
    let mut updated = faster.clone();
    updated.ended = None;
    send_timed(&mut h, EngineEvent::RunUpdated(updated), true);
    send_timed(&mut h, EngineEvent::RunEnded(faster.clone()), true);
    assert_eq!(h.logs().len(), 1);
    let alert = h.logs().remove(0);
    assert_eq!(alert.kind, AlertKind::PersonalBest);
    assert_eq!(alert.run_id.as_deref(), Some("faster-clear"));
    let details = serde_json::to_value(alert.details.unwrap()).unwrap();
    assert_eq!(details["type"], "run_time_personal_best");
    assert_eq!(details["duration_micros"], 86_000_000);
    assert_eq!(details["previous_micros"], 87_000_000);
    let best = open_reader(&h.path).unwrap().dungeon_bests().unwrap();
    assert_eq!(best[0].best_clear_micros, 86_000_000);
    send_timed(&mut h, EngineEvent::RunEnded(faster), true);
    assert_eq!(h.logs().len(), 1);
    assert_eq!(
        h.db()
            .query_row(
                "SELECT count(*) FROM alert_occurrences WHERE source_key='faster-clear'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn run_time_pb_excludes_non_improvements_non_clears_and_non_live() {
    for case in [
        "first",
        "slower",
        "equal",
        "left",
        "died",
        "unknown",
        "no-victory",
        "history",
        "demo",
    ] {
        let mut h = Harness::new();
        if case != "first" {
            send_timed(
                &mut h,
                EngineEvent::RunEnded(timed_run("prior", 87_000_000)),
                true,
            );
        }
        let mut run = timed_run(
            "candidate",
            match case {
                "slower" => 88_000_000,
                "equal" => 87_000_000,
                _ => 86_000_000,
            },
        );
        run.outcome = Some(match case {
            "left" => Outcome::Left,
            "died" => Outcome::Died,
            "unknown" => Outcome::Unknown,
            _ => Outcome::Cleared,
        });
        if case == "no-victory" {
            run.evidence.clear();
        }
        send_timed(
            &mut h,
            EngineEvent::RunEnded(run.clone()),
            !matches!(case, "history" | "demo"),
        );
        assert!(h.logs().is_empty(), "{case}");
        // A historical/offline result must not celebrate when re-published live.
        if matches!(case, "history" | "demo") {
            send_timed(&mut h, EngineEvent::RunEnded(run), true);
            assert!(h.logs().is_empty(), "{case} revision");
        }
    }
}

#[test]
fn run_time_pb_obeys_personal_best_loudness() {
    for level in [
        AlertLoudness::Off,
        AlertLoudness::QuietLogOnly,
        AlertLoudness::Toast,
        AlertLoudness::Banner,
    ] {
        let mut h = Harness::new();
        send_timed(
            &mut h,
            EngineEvent::RunEnded(timed_run("prior", 87_000_000)),
            true,
        );
        h.seq += 1;
        let mut ctx = context(level);
        ctx.live = true;
        h.writer
            .try_submit_with_alert_context(
                common::envelope(
                    h.seq,
                    EngineEvent::RunEnded(timed_run("record", 86_000_000)),
                ),
                ctx,
            )
            .unwrap();
        h.writer.flush().unwrap();
        assert_eq!(h.logs().len(), usize::from(level != AlertLoudness::Off));
        if let Some(alert) = h.logs().first() {
            assert_eq!(alert.presentation, Some(level));
        }
    }
}

#[path = "../../maxed-engine/tests/common/mod.rs"]
mod engine_common;

#[test]
fn fully_observed_reconstructed_boss_records_and_beats_dps_pb() {
    use engine_common as combat;
    use maxed_protocol::{defs::QuestObjId, Packet};
    let mut h = Harness::new();
    for i in 0..2 {
        let mut cfg = combat::config();
        cfg.stream_id = format!("synthetic-pb-{i}");
        let mut engine =
            maxed_engine::Engine::new(cfg, std::sync::Arc::new(combat::catalog())).unwrap();
        let mut player = combat::player();
        player
            .status
            .stats
            .iter_mut()
            .find(|s| s.statTypeNum == 20)
            .unwrap()
            .statValue = if i == 0 { 25 } else { 75 };
        for (at, packet) in [
            (0, combat::map("The Void")),
            (1000, combat::create(1, 1)),
            (2000, combat::update(vec![player, combat::target()], vec![])),
            (
                3000,
                Packet::QuestObjId(QuestObjId {
                    objectId: 2,
                    list: vec![],
                }),
            ),
            (1_000_000, combat::tick(vec![])),
            (1_000_000, combat::shoot(1)),
            (1_000_000, combat::hit(1)),
            (5_000_000, combat::tick(vec![])),
            (5_000_000, combat::shoot(2)),
            (5_000_000, combat::hit(2)),
            (6_000_000, combat::victory()),
        ] {
            for event in combat::apply(&mut engine, at, packet) {
                h.send(event, AlertLoudness::Banner);
            }
        }
        for event in engine.finish(Timestamp(7_000_000), FightEnd::CaptureStopped) {
            h.send(event, AlertLoudness::Banner);
        }
        assert_eq!(h.count("personal_bests"), 1);
        assert_eq!(h.logs().len(), i);
    }
    let row = h.logs().remove(0);
    assert!(matches!(
        row.details,
        Some(AlertDetails::PersonalBest {
            dps: 100.,
            previous_dps: 50.,
            ..
        })
    ));
}

#[test]
fn review1_late_eligible_revision_alerts_once() {
    let mut h = Harness::new();
    send_timed(
        &mut h,
        EngineEvent::FightEnded(fight("prior-pb", 500., "self")),
        true,
    );
    let mut pending = fight("settling", 600., "self");
    pending.players[0].sources[0].estimated = true;
    send_timed(&mut h, EngineEvent::FightUpdated(pending), true);
    assert!(h.logs().is_empty());
    let settled = fight("settling", 600., "self");
    send_timed(&mut h, EngineEvent::FightUpdated(settled.clone()), true);
    assert_eq!(h.logs().len(), 1);
    send_timed(&mut h, EngineEvent::FightEnded(settled.clone()), true);
    send_timed(&mut h, EngineEvent::FightUpdated(settled), true);
    assert_eq!(h.logs().len(), 1);
}
#[test]
fn review1_owned_pb_revision_restores_next_eligible_best() {
    for ineligible in [false, true] {
        let mut h = Harness::new();
        send_timed(
            &mut h,
            EngineEvent::FightEnded(fight("prior-pb", 500., "self")),
            true,
        );
        send_timed(
            &mut h,
            EngineEvent::FightUpdated(fight("revised", 600., "self")),
            true,
        );
        let mut revision = fight("revised", 450., "self");
        if ineligible {
            revision.complete_capture = false;
        }
        send_timed(&mut h, EngineEvent::FightUpdated(revision), true);
        let best: f64 = h
            .db()
            .query_row(
                "SELECT dps FROM personal_bests WHERE character_id='self' AND boss_type=42",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(best, 500.);
        send_timed(
            &mut h,
            EngineEvent::FightEnded(fight("next", 550., "self")),
            true,
        );
        assert_eq!(h.logs().len(), 2);
    }
}
#[test]
fn review1_offline_dps_updates_baseline_without_alerts() {
    let mut h = Harness::new();
    for (id, dps) in [("offline-first", 500.), ("offline-best", 600.)] {
        send_timed(
            &mut h,
            EngineEvent::FightEnded(fight(id, dps, "self")),
            false,
        );
    }
    assert!(h.logs().is_empty());
    let best: f64 = h
        .db()
        .query_row("SELECT dps FROM personal_bests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(best, 600.);
    send_timed(
        &mut h,
        EngineEvent::FightEnded(fight("offline-best", 700., "self")),
        true,
    );
    assert!(h.logs().is_empty());
}
#[test]
fn review1_time_pb_fires_at_victory_before_departure() {
    let mut h = Harness::new();
    send_timed(
        &mut h,
        EngineEvent::RunEnded(timed_run("prior", 87_000_000)),
        true,
    );
    let mut pending = fight("pending-at-victory", 600., "self");
    pending.pending_local_reconciliation = true;
    send_timed(&mut h, EngineEvent::FightUpdated(pending), true);
    assert!(h.logs().is_empty());
    let mut run = timed_run("victory", 86_000_000);
    run.ended = None;
    send_timed(&mut h, EngineEvent::RunUpdated(run.clone()), true);
    assert_eq!(h.logs().len(), 1);
    run.ended = Some(Timestamp(999_000_000));
    send_timed(&mut h, EngineEvent::RunEnded(run.clone()), true);
    send_timed(&mut h, EngineEvent::RunUpdated(run), true);
    assert_eq!(h.logs().len(), 1);
}

fn pending_pb_fixture() -> (Harness, maxed_engine::Engine) {
    use engine_common as combat;
    let mut h = Harness::new();
    let mut engine = combat::engine();
    for (at, packet) in [
        (0, combat::map("The Void")),
        (1000, combat::create(1, 1)),
        (
            2000,
            combat::update(vec![combat::player(), combat::target()], vec![]),
        ),
        (
            3000,
            maxed_protocol::Packet::QuestObjId(maxed_protocol::defs::QuestObjId {
                objectId: 2,
                list: vec![],
            }),
        ),
        (1_000_000, combat::tick(vec![])),
        (1_000_000, combat::shoot(1)),
        (1_000_000, combat::hit(1)),
        (1_010_000, combat::damage(1, 1, 100)),
        (5_000_000, combat::tick(vec![])),
        (5_000_000, combat::shoot(2)),
        (5_000_000, combat::hit(2)),
    ] {
        for event in combat::apply(&mut engine, at, packet) {
            h.send(event, AlertLoudness::Banner);
        }
    }
    let mut previous = engine.snapshot().fights.remove(0);
    previous.id = "settled-prior-pb".into();
    previous.ended = Some(Timestamp(5_000_000));
    previous.end_reason = Some(FightEnd::MapChanged);
    previous.players[0].dps = Some(25.);
    // A previously settled fight supplies the baseline independently of live pairing.
    previous.pending_local_reconciliation = false;
    h.send(EngineEvent::FightEnded(previous), AlertLoudness::Banner);
    for event in combat::apply(&mut engine, 5_100_000, combat::victory()) {
        h.send(event, AlertLoudness::Banner);
    }
    assert!(h.logs().is_empty(), "pending kill must not alert");
    assert_eq!(pb_value(&h), 25.);
    (h, engine)
}

fn pb_value(h: &Harness) -> f64 {
    h.db()
        .query_row("SELECT dps FROM personal_bests", [], |r| r.get(0))
        .unwrap()
}

fn publish_pb_settlement(h: &mut Harness, events: Vec<EngineEvent>, alerts: usize, dps: f64) {
    assert!(
        events.iter().any(|e| matches!(e,
            EngineEvent::FightUpdated(f) | EngineEvent::FightEnded(f) if f.ended.is_some()
        )),
        "settlement must publish a terminal revision"
    );
    for event in events.iter().chain(events.iter()) {
        h.send(event.clone(), AlertLoudness::Banner);
    }
    assert_eq!(h.logs().len(), alerts);
    assert_eq!(
        h.db()
            .query_row(
                "SELECT count(*) FROM alert_occurrences WHERE kind='personal_best' AND state='pending'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        alerts as i64
    );
    assert_eq!(pb_value(h), dps);
}

#[test]
fn pending_local_pb_waits_for_matching_server() {
    for later_tick in [false, true] {
        let (mut h, mut engine) = pending_pb_fixture();
        if later_tick {
            for event in engine_common::apply(&mut engine, 5_150_000, engine_common::tick(vec![])) {
                h.send(event, AlertLoudness::Banner);
            }
        }
        for event in engine_common::apply(&mut engine, 5_200_000, engine_common::damage(1, 2, 100))
        {
            h.send(event, AlertLoudness::Banner);
        }
        publish_pb_settlement(&mut h, engine.advance_to(Timestamp(5_500_000)), 1, 50.);
        let events = engine.finish(Timestamp(5_600_000), FightEnd::AppClosed);
        publish_pb_settlement(&mut h, events, 1, 50.);
    }
}

#[test]
fn pending_local_pb_rejects_disagreeing_server() {
    for later_tick in [false, true] {
        let (mut h, mut engine) = pending_pb_fixture();
        if later_tick {
            for event in engine_common::apply(&mut engine, 5_150_000, engine_common::tick(vec![])) {
                h.send(event, AlertLoudness::Banner);
            }
        }
        for event in engine_common::apply(&mut engine, 5_200_000, engine_common::damage(1, 2, 90)) {
            h.send(event, AlertLoudness::Banner);
        }
        publish_pb_settlement(&mut h, engine.advance_to(Timestamp(5_500_000)), 0, 25.);
        let events = engine.finish(Timestamp(5_600_000), FightEnd::AppClosed);
        publish_pb_settlement(&mut h, events, 0, 25.);
    }
}

#[test]
fn pending_local_pb_alerts_when_estimate_expires() {
    let (mut h, mut engine) = pending_pb_fixture();
    for event in engine.advance_to(Timestamp(9_900_000)) {
        h.send(event, AlertLoudness::Banner);
    }
    assert!(h.logs().is_empty());
    publish_pb_settlement(&mut h, engine.advance_to(Timestamp(10_200_000)), 1, 50.);
}

#[test]
fn pending_local_pb_finalizes_before_retirement() {
    for reason in [
        FightEnd::MapChanged,
        FightEnd::CaptureStopped,
        FightEnd::AppClosed,
    ] {
        let (mut h, mut engine) = pending_pb_fixture();
        let events = if reason == FightEnd::MapChanged {
            engine_common::apply(&mut engine, 5_200_000, engine_common::map("Nexus"))
        } else {
            engine.finish(Timestamp(5_200_000), reason)
        };
        publish_pb_settlement(&mut h, events, 1, 50.);
    }
}

fn legacy_basis(fight: FightView) -> FightView {
    let mut value = serde_json::to_value(fight).unwrap();
    value.as_object_mut().unwrap().remove("dps_basis");
    serde_json::from_value(value).unwrap()
}

#[test]
fn legacy_dps_cannot_seed_a_pb_and_current_basis_alerts_once() {
    let mut h = Harness::new();
    let legacy = legacy_basis(fight("legacy-basis", 100., "self"));
    send_timed(&mut h, EngineEvent::FightEnded(legacy), true);
    assert_eq!(h.count("personal_bests"), 0);
    send_timed(
        &mut h,
        EngineEvent::FightEnded(fight("current-first", 500., "self")),
        true,
    );
    assert_eq!(pb_value(&h), 500.);
    assert!(h.logs().is_empty());
    let better = fight("current-best", 600., "self");
    send_timed(&mut h, EngineEvent::FightUpdated(better.clone()), true);
    send_timed(&mut h, EngineEvent::FightEnded(better.clone()), true);
    send_timed(&mut h, EngineEvent::FightUpdated(better), true);
    assert_eq!(pb_value(&h), 600.);
    assert_eq!(h.logs().len(), 1);
}

#[test]
fn legacy_dps_cannot_be_restored_by_revision_repair() {
    for explicit in [false, true] {
        let mut h = Harness::new();
        send_timed(
            &mut h,
            EngineEvent::FightEnded(fight("legacy-best", 900., "self")),
            false,
        );
        h.db()
            .execute(
                "UPDATE fights SET json=json_remove(json,'$.dps_basis') WHERE id='legacy-best'",
                [],
            )
            .unwrap();
        if explicit {
            let legacy = legacy_basis(fight("legacy-best", 900., "self"));
            h.db()
                .execute(
                    "UPDATE fights SET json=?1 WHERE id='legacy-best'",
                    [serde_json::to_string(&legacy).unwrap()],
                )
                .unwrap();
        }
        h.db().execute("DELETE FROM personal_bests", []).unwrap();
        send_timed(
            &mut h,
            EngineEvent::FightEnded(fight("current-first", 500., "self")),
            true,
        );
        send_timed(
            &mut h,
            EngineEvent::FightUpdated(fight("current-best", 600., "self")),
            true,
        );
        let mut revised = fight("current-best", 600., "self");
        revised.complete_capture = false;
        send_timed(&mut h, EngineEvent::FightUpdated(revised), true);
        assert_eq!(pb_value(&h), 500.);
        assert_eq!(h.logs().len(), 1);
    }
}

#[test]
fn alert_multi_kind_filters_and_legacy_decode() {
    let old: AlertFilter =
        serde_json::from_str(r#"{"kind":null,"range":null,"query":"","cursor":null,"limit":50}"#)
            .unwrap();
    assert!(old.kinds.is_empty() && old.exclude_kinds.is_empty());
    let mut h = Harness::new();
    for n in 0..180 {
        h.send(
            EngineEvent::AlertFired(AlertView {
                id: format!("multi-{n:03}"),
                at: Timestamp(n + 1),
                kind: [
                    AlertKind::Whisper,
                    AlertKind::GuildMessage,
                    AlertKind::KeyPopped,
                ][n as usize % 3]
                    .clone(),
                run_id: None,
                explanation: "Synthetic".into(),
                details: None,
                presentation: None,
            }),
            AlertLoudness::Off,
        );
    }
    let reader = open_reader(&h.path).unwrap();
    let mut filter = old;
    filter.kinds = vec![AlertKind::Whisper, AlertKind::GuildMessage];
    filter.exclude_kinds = vec![AlertKind::GuildMessage];
    let page = reader.list_alerts(filter.clone()).unwrap();
    assert_eq!(page.rows.len(), 50);
    assert!(page.rows.iter().all(|r| r.kind == AlertKind::Whisper));
    filter.cursor = page.next;
    assert_eq!(reader.list_alerts(filter.clone()).unwrap().rows.len(), 10);
    filter.cursor = None;
    filter.exclude_kinds.clear();
    assert!(reader
        .list_alerts(filter.clone())
        .unwrap()
        .rows
        .iter()
        .all(|r| r.kind != AlertKind::KeyPopped));
    filter.kind = Some(AlertKind::Whisper);
    assert!(matches!(
        reader.list_alerts(filter.clone()),
        Err(StoreError::Limit)
    ));
    filter.kind = None;
    filter.kinds = vec![AlertKind::Whisper; 9];
    assert!(matches!(
        reader.list_alerts(filter.clone()),
        Err(StoreError::Limit)
    ));
    filter.kinds.clear();
    filter.exclude_kinds = vec![AlertKind::KeyPopped];
    assert_eq!(reader.list_alerts(filter.clone()).unwrap().rows.len(), 50);
    let first = reader.list_alerts(filter.clone()).unwrap();
    filter.cursor = first.next;
    let second = reader.list_alerts(filter.clone()).unwrap();
    assert_eq!(second.rows.len(), 50);
    assert!(second
        .rows
        .iter()
        .all(|row| row.kind != AlertKind::KeyPopped));
    filter.cursor = None;
    filter.kind = Some(AlertKind::Whisper);
    assert!(reader
        .list_alerts(filter.clone())
        .unwrap()
        .rows
        .iter()
        .all(|row| row.kind == AlertKind::Whisper));
    filter.kind = None;
    for len in [2, 9] {
        filter.exclude_kinds = vec![AlertKind::KeyPopped; len];
        assert!(matches!(
            reader.list_alerts(filter.clone()),
            Err(StoreError::Limit)
        ));
        filter.exclude_kinds.clear();
        filter.kinds = vec![AlertKind::Whisper; len];
        assert!(matches!(
            reader.list_alerts(filter.clone()),
            Err(StoreError::Limit)
        ));
        filter.kinds.clear();
    }
}

#[test]
fn alert_summary_counts_range_and_personal_bests() {
    let mut h = Harness::new();
    let reader = open_reader(&h.path).unwrap();
    let request = AlertSummaryRequest::default;
    assert_eq!(reader.alert_summary(request()).unwrap().total, 0);
    let mut run = demo::demo_run_story().run;
    run.character_id = Some("self".into());
    run.dungeon = "Synthetic vault".into();
    h.send(EngineEvent::RunStarted(run.clone()), AlertLoudness::Off);
    for (n, kind, details) in [
        (1, AlertKind::WhiteBag, None),
        (
            2,
            AlertKind::PersonalBest,
            Some(AlertDetails::PersonalBest {
                fight_id: "synthetic-fight".into(),
                character_id: "synthetic-character".into(),
                boss_type: 42,
                dps: 2.,
                previous_dps: 1.,
            }),
        ),
        (
            3,
            AlertKind::PersonalBest,
            Some(AlertDetails::RunTimePersonalBest {
                dungeon: "Synthetic vault".into(),
                duration_micros: 1,
                previous_micros: 2,
            }),
        ),
        (4, AlertKind::WhiteBag, None),
        (5, AlertKind::Death, None),
    ] {
        h.send(
            EngineEvent::AlertFired(AlertView {
                id: format!("summary-{n}"),
                at: Timestamp(n),
                kind,
                run_id: (n == 1).then(|| run.id.clone()),
                explanation: "Synthetic %_\\".into(),
                details,
                presentation: None,
            }),
            AlertLoudness::Off,
        );
    }
    let all = reader.alert_summary(request()).unwrap();
    assert_eq!(all.total, 5);
    assert_eq!(all.first_at, Some(Timestamp(1)));
    assert_eq!(
        all.by_kind
            .iter()
            .map(|k| (&k.kind, k.count))
            .collect::<Vec<_>>(),
        vec![
            (&AlertKind::WhiteBag, 2),
            (&AlertKind::PersonalBest, 2),
            (&AlertKind::Death, 1)
        ]
    );
    assert_eq!(all.last_white.unwrap().run_id, None);
    let middle = reader
        .alert_summary(AlertSummaryRequest {
            range: Some(TimeRange {
                start: Timestamp(2),
                end: Timestamp(4),
            }),
        })
        .unwrap();
    assert_eq!(middle.total, 2);
    assert_eq!(middle.first_at, Some(Timestamp(2)));
    assert!(middle.last_white.is_none());
    assert_eq!(
        middle.by_kind,
        vec![AlertKindCount {
            kind: AlertKind::PersonalBest,
            count: 2
        }]
    );

    let scoped = reader
        .alert_summary(AlertSummaryRequest {
            range: Some(TimeRange {
                start: Timestamp(1),
                end: Timestamp(4),
            }),
        })
        .unwrap();
    assert_eq!(scoped.total, 3);
    assert_eq!(
        scoped
            .by_kind
            .iter()
            .map(|entry| entry.count)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let white = scoped.last_white.unwrap();
    assert_eq!(white.run_id, Some(run.id));
    assert_eq!(white.dungeon.as_deref(), Some("Synthetic vault"));
    assert!(serde_json::from_str::<AlertSummaryRequest>(r#"{"query":"Synthetic"}"#).is_err());
    assert_eq!(
        serde_json::from_str::<AlertSummaryRequest>("{}").unwrap(),
        request()
    );
}

#[test]
fn alert_summary_bounds_dates_and_legacy_last_white_text() {
    let mut h = Harness::new();
    let mut run = demo::demo_run_story().run;
    run.character_id = Some("self".into());
    h.send(EngineEvent::RunStarted(run.clone()), AlertLoudness::Off);
    h.send(
        EngineEvent::AlertFired(AlertView {
            id: "synthetic-white".into(),
            at: Timestamp(10),
            kind: AlertKind::WhiteBag,
            run_id: Some(run.id.clone()),
            explanation: "Synthetic".into(),
            details: None,
            presentation: None,
        }),
        AlertLoudness::Off,
    );
    let reader = open_reader(&h.path).unwrap();
    for (start, end) in [(1, 1), (2, 1), (0, u64::MAX)] {
        assert!(reader
            .alert_summary(AlertSummaryRequest {
                range: Some(TimeRange {
                    start: Timestamp(start),
                    end: Timestamp(end)
                }),
            })
            .is_err());
    }
    h.db()
        .execute(
            "UPDATE runs SET dungeon=?1 WHERE id=?2",
            params!["x".repeat(2 * 1024 * 1024 + 1), run.id],
        )
        .unwrap();
    assert!(matches!(
        reader.alert_summary(AlertSummaryRequest::default()),
        Err(StoreError::Limit)
    ));
}
