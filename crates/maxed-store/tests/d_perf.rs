//! Synthetic release gates. They need same-machine baseline inputs via
//! environment variables; see docs/release/performance.md#synthetic-storeengine-gates.
use crate::common::account_scope_fixture::ACCOUNT;
use maxed_store::{apply_batch, migrate, open_reader, StoreError, StoreReader};
use maxed_types::*;
use std::time::Duration;
mod common;
#[path = "common/d_legacy.rs"]
mod d_legacy;
#[path = "common/d_workload.rs"]
mod d_workload;
#[path = "common/home_workload.rs"]
mod home_workload;
use common::{downgrade, receipts};
#[path = "common/runs_audit.rs"]
mod runs_audit;
#[path = "common/runs_workload.rs"]
mod runs_workload;

#[test]
#[ignore = "release report only; distributed 180-day Home history, 30 warm reads"]
fn home_history_thirty_warm_reads() {
    if cfg!(debug_assertions) {
        panic!("timing reports require --release");
    }
    let fixture = home_workload::Fixture::build(180);
    let bytes: i64 = fixture
        .db
        .query_row(
            "SELECT sum(length(CAST(json AS BLOB))) FROM fights",
            [],
            |r| r.get(0),
        )
        .unwrap();
    eprintln!("Home synthetic: 25200 fights, 176400 players, 180 days, 3 characters, 4 dungeons, 48 observed targets (64 requested), 720 realm visits; fight JSON bytes={bytes}");
    let reader = fixture.reader();
    for targets in [1, 16, 64] {
        let request = fixture.boss_request(targets);
        report_home_read(&format!("boss_history/90d/{targets}"), || {
            reader.boss_history(request.clone()).unwrap()
        });
        report_home_read(&format!("home_baselines/90d/{targets}"), || {
            reader
                .home_boss_baselines(request.clone(), Some(ACCOUNT))
                .unwrap()
        });
    }
    report_home_read("dungeon_profile/90d", || {
        reader
            .dungeon_profile(fixture.profile_request(), Some(ACCOUNT))
            .unwrap()
    });
    report_home_read("boss_history/all-time", || {
        reader
            .boss_history(BossHistoryRequest {
                range: None,
                character_id: Some(home_workload::CHARACTER.into()),
                target_types: None,
            })
            .unwrap()
    });
    report_profile_materialization(&fixture);
}

fn report_home_read<T: serde::Serialize>(label: &str, mut read: impl FnMut() -> T) {
    let start = std::time::Instant::now();
    let first = read();
    eprintln!("{label}: first={:?}", start.elapsed());
    d_workload::warm(label, &mut read);
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(read()).unwrap()
    );
}

fn report_profile_materialization(fixture: &home_workload::Fixture) {
    let columns =
        &crate::common::account_sql::with_accounts(include_str!("../src/dungeon_profile.sql"))
            .replace(
                "/*fight_character*/",
                include_str!("../src/fight_character.sql"),
            );
    let mut full_json = columns.clone();
    // Controlled alternative: materialize valid full fight JSON before extracting
    // the same scalar facts. Keep this benchmark tied to the production scopes.
    for (name, source) in [
        ("learning", "FROM cleared_runs cleared CROSS JOIN fights ON fights.run_id=cleared.id\n  WHERE /*account:fight:fights*/=?5 AND json_valid(fights.json)"),
        ("performance", "FROM fights\n  WHERE /*account:fight:fights*/=?5 AND fights.started>=?1 AND fights.started<?2\n    AND fights.ended IS NOT NULL AND fights.local_dps IS NOT NULL AND json_valid(fights.json)"),
    ] {
        let source=crate::common::account_sql::with_accounts(source);
        assert_eq!(full_json.matches(source.as_str()).count(), 1, "review {name} materialization benchmark when the scope changes");
        full_json = full_json.replace(source.as_str(), &format!("FROM {name}_valid fights"))
            .replace(&format!("{name}_facts AS MATERIALIZED ("), &format!("{name}_valid AS MATERIALIZED (SELECT fights.* {source}),\n{name}_facts AS MATERIALIZED ("));
    }
    let read = |sql: &str| {
        let request = fixture.profile_request();
        fixture
            .db
            .prepare_cached(sql)
            .unwrap()
            .query_map(
                rusqlite::params![
                    request.range.start.0 as i64,
                    request.range.end.0 as i64,
                    request.character_id,
                    request.dungeon,
                    ACCOUNT
                ],
                |r| {
                    let mut offsets: Vec<u64> =
                        serde_json::from_str(&r.get::<_, String>(3)?).unwrap();
                    let mut durations: Vec<u64> =
                        serde_json::from_str(&r.get::<_, String>(4)?).unwrap();
                    offsets.sort_unstable();
                    durations.sort_unstable();
                    Ok((
                        r.get::<_, u16>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, u32>(2)?,
                        offsets,
                        durations,
                        r.get::<_, Option<f64>>(5)?,
                    ))
                },
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(read(columns), read(&full_json));
    report_home_read("profile_boss_sql/extracted_columns", || read(columns));
    report_home_read("profile_boss_sql/full_json", || read(&full_json));
}

#[test]
#[ignore = "focused synthetic Runs query timing; debug builds supported"]
fn runs_scalar_share_query_timing() {
    let dir = tempfile::tempdir().unwrap();
    let mut db = rusqlite::Connection::open(dir.path().join("synthetic.sqlite")).unwrap();
    let ids = runs_audit::seed(&mut db, 3_138, 23_534);
    let old = runs_audit::query(true);
    let new = runs_audit::query(false);
    let expected = runs_audit::read(&db, &old, &ids);
    assert_eq!(runs_audit::read(&db, &new, &ids), expected);
    let bytes: i64 = db
        .query_row(
            "SELECT (SELECT sum(length(CAST(json AS BLOB))) FROM fights)
        +(SELECT sum(length(CAST(json AS BLOB))) FROM fight_players)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut before = Vec::new();
    let mut after = Vec::new();
    // Alternate order to avoid giving one query all the warm-cache advantage.
    for round in 0..5 {
        for legacy in if round % 2 == 0 {
            [true, false]
        } else {
            [false, true]
        } {
            let start = std::time::Instant::now();
            let result = runs_audit::read(&db, if legacy { &old } else { &new }, &ids);
            let elapsed = start.elapsed();
            assert_eq!(result, expected);
            if legacy {
                before.push(elapsed);
            } else {
                after.push(elapsed);
            }
        }
    }
    before.sort();
    after.sort();
    eprintln!("Runs synthetic: 12 runs, 3138 fights, 23534 players, {bytes} JSON bytes; five warm samples: before={before:?}; after={after:?}");
    assert!(
        after[2] < before[2].mul_f64(0.8),
        "scalar median must improve at least 20%"
    );
    assert!(
        after[4] < Duration::from_secs(1),
        "each warm Runs detail read must stay below 1 s"
    );
}

#[test]
#[ignore = "release report only; upgraded v4 history with 3000 fights x 85 players"]
fn upgraded_worst_story_thirty_warm_reads() {
    // Gate exception: legacy fallback; no released schema predates 005.
    report_upgraded_story(runs_workload::WORST, true);
}

#[test]
#[ignore = "release report only; upgraded v4 realistic history"]
fn upgraded_realistic_story_thirty_warm_reads() {
    // Gate exception: legacy fallback; no released schema predates 005.
    report_upgraded_story(runs_workload::REALISTIC, false);
}

fn report_upgraded_story(shape: runs_workload::Shape, dense: bool) {
    if cfg!(debug_assertions) {
        panic!("performance reports require --release");
    }
    let seed = runs_workload::build(shape);
    let path = seed._dir.path().join("v4.sqlite");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    d_legacy::copy_v4(&seed.path, &db);
    // The baseline worst shape has only ten 85-player fights. Stress every fight's
    // player projection at that density while retaining its timeline/bag shape.
    if dense {
        db.execute_batch(
            "BEGIN;
        INSERT OR IGNORE INTO fight_players
        SELECT f.id,p.player_id,p.object_id,p.damage,p.dps,p.rank,p.json
        FROM fights f CROSS JOIN fight_players p WHERE p.fight_id='fight-00000';
        UPDATE fight_summaries SET json=json_set(json,'$.player_count',85,'$.total_damage',25500);
        COMMIT;",
        )
        .unwrap();
    }
    let player_rows = if dense {
        shape.fights * shape.large_players
    } else {
        shape.players
    };
    assert_eq!(
        db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM fight_players", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        player_rows as i64
    );
    let start = std::time::Instant::now();
    migrate(&mut db).unwrap();
    eprintln!("{} v4 -> v5 upgrade: {:?}", shape.name, start.elapsed());
    assert_eq!(
        db.query_row("SELECT count(*) FROM pd_run_players", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let start = std::time::Instant::now();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    eprintln!("upgraded {} reader open: {:?}", shape.name, start.elapsed());
    let start = std::time::Instant::now();
    let first = reader.get_run(runs_workload::RUN).unwrap().unwrap();
    eprintln!(
        "upgraded {} first story: {:?}; bytes={}",
        shape.name,
        start.elapsed(),
        runs_workload::bytes(&first)
    );
    let totals = first.totals.as_ref().unwrap();
    assert_eq!(totals.fights, shape.fights as u32);
    assert_eq!(totals.players.len(), shape.large_players.min(64));
    assert_eq!(totals.total_damage, player_rows as f64 * 300.);
    assert_eq!(
        totals.local.as_ref().unwrap().damage,
        shape.fights as f64 * 300.
    );
    d_workload::warm(
        &format!(
            "upgraded {} story ({player_rows} player rows; report only)",
            shape.name
        ),
        || reader.get_run(runs_workload::RUN).unwrap().unwrap(),
    );
    assert_eq!(reader.get_run(runs_workload::RUN).unwrap().unwrap(), first);
}

#[test]
#[ignore = "release gate; 30 warm reads, requires same-machine P4 baselines"]
fn dungeon_stats_and_story_totals_thirty_warm_reads() {
    let baselines = [
        d_workload::baseline("MAXED_PD_P4_REALISTIC_STORY_P95_US"),
        d_workload::baseline("MAXED_PD_P4_WORST_STORY_P95_US"),
    ];
    let mut home = d_workload::Fixture::new();
    home.runs();
    let reader = home.reader();
    for scope in [
        HomeScope::Session {
            id: "demo-session".into(),
        },
        HomeScope::Today {
            range: TimeRange {
                start: Timestamp(0),
                end: Timestamp(10_000_000),
            },
        },
    ] {
        let (kind, session, start, end) = match &scope {
            HomeScope::Session { id } => (0, Some(id.as_str()), None, None),
            HomeScope::Today { range } => (
                1,
                None,
                Some(range.start.0 as i64),
                Some(range.end.0 as i64),
            ),
        };
        let mut plan = home
            .db
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                crate::common::account_sql::with_accounts(include_str!("../src/home.sql"))
                    .replace(
                        "/*home_scope*/",
                        &crate::common::account_sql::with_accounts(include_str!(
                            "../src/home_scope.sql"
                        ))
                    )
                    .replace(
                        "/*dungeon_stats*/",
                        include_str!("../src/dungeon_stats.sql")
                    )
            ))
            .unwrap();
        let details = plan
            .query_map(rusqlite::params![kind, session, start, end, ACCOUNT], |r| {
                r.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        eprintln!("Home scope {kind} plan:\n{}", details.join("\n"));
        let summary = reader.home_summary(scope.clone(), Some(ACCOUNT)).unwrap();
        assert_eq!(summary.runs, 10_000);
        assert_eq!(summary.dungeon_stats.unwrap().len(), 20);
        assert!(
            d_workload::warm(&format!("Home dungeon stats scope={kind}"), || reader
                .home_summary(scope.clone(), Some(ACCOUNT))
                .unwrap())
                < Duration::from_millis(50)
        );
    }
    for (shape, baseline) in [runs_workload::REALISTIC, runs_workload::WORST]
        .into_iter()
        .zip(baselines)
    {
        let fixture = runs_workload::build(shape);
        let reader = fixture.validate();
        let start = std::time::Instant::now();
        let setup = crate::common::account_scope_fixture::open_reader(&fixture.path).unwrap();
        eprintln!(
            "{} reader open/config/schema: {:?}",
            shape.name,
            start.elapsed()
        );
        drop(setup);
        let p95 = d_workload::warm(shape.name, || {
            reader.get_run(runs_workload::RUN).unwrap().unwrap()
        });
        assert!(
            p95 <= baseline.mul_f64(1.1),
            "{}: {p95:?} exceeds P4 {baseline:?} +10%",
            shape.name
        );
    }
}
#[test]
#[ignore = "release byte gate"]
fn fight_json_growth_six_series_by_1800_buckets() {
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    let fixture = runs_workload::build(runs_workload::WORST);
    let reader = fixture.validate();
    let mut fight = reader.get_fight(runs_workload::FIGHT).unwrap().unwrap();
    let baseline = runs_workload::bytes(&fight);
    assert_eq!(fight.buckets.len(), 1800);
    fight.player_series = Some(
        fight
            .players
            .iter()
            .take(6)
            .map(|p| PlayerSeries {
                player_id: p.player_id.clone(),
                object_id: p.object_id,
                is_local: p.is_local,
                points: (0..1800)
                    .map(|i| if i % 3 == 0 { 12345.6789012345 } else { 0.0 })
                    .collect(),
            })
            .collect(),
    );
    let bytes = runs_workload::bytes(&fight);
    eprintln!("FightView series growth: {baseline} -> {bytes} bytes");
    assert!(bytes * 100 <= baseline * 135);
    assert!(bytes < 8 * 1024 * 1024);
}
#[test]
#[ignore = "release volume gate; projects 1.2 million synthetic events"]
fn instance_event_volume_projection_batches() {
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    let mut legacy = d_workload::Fixture::new();
    legacy.runs();
    let baseline = d_workload::volume(&mut legacy, false);
    drop(legacy);
    let mut fixture = d_workload::Fixture::new();
    fixture.runs();
    let p95 = d_workload::volume(&mut fixture, true);
    report_event_reads(&fixture);
    let story = fixture.reader().get_run("demo-run").unwrap().unwrap();
    assert!(story.timeline.len() < 20_000);
    assert_eq!(
        story
            .timeline
            .iter()
            .filter(|e| e.kind == TimelineKind::PlayerLeft)
            .count(),
        60
    );
    assert!(
        p95 <= baseline.mul_f64(1.1),
        "projection p95 {p95:?} exceeds P4 batch-100 {baseline:?} +10%"
    );
}
#[test]
#[ignore = "release captured-bag volume gate"]
fn captured_bags_fourfold_and_overflow_returns_limit() {
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    let mut fixture = d_workload::Fixture::new();
    fixture.runs();
    for count in [
        runs_workload::REALISTIC.bags * 4,
        runs_workload::WORST.bags * 4,
    ] {
        fixture.add_bags(count);
        assert_eq!(
            fixture
                .reader()
                .get_run("demo-run")
                .unwrap()
                .unwrap()
                .drops
                .len(),
            count
        );
    }
    // The baseline worst case is 200 bags: fourfold is 800, still under the 2,000 cap.
    // A separate 501-bag burst expanded fourfold must fail instead of truncating.
    fixture.add_bags(501 * 4);
    assert!(matches!(
        fixture.reader().get_run("demo-run"),
        Err(StoreError::Limit)
    ));
}

#[test]
#[ignore = "release writer growth gate; 3000 fights and 200 repeated writes per phase"]
fn worst_run_repeated_fight_updates_do_not_grow_with_history() {
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    const EARLY: usize = 20;
    const SAMPLES: usize = 200;
    let mut fixture = runs_workload::build(runs_workload::Shape {
        fights: EARLY,
        ..runs_workload::WORST
    });
    let reader = crate::common::account_scope_fixture::open_reader(&fixture.path).unwrap();
    let mut updated = reader.get_fight(runs_workload::FIGHT).unwrap().unwrap();
    let small = reader.get_fight("fight-00010").unwrap().unwrap();
    assert_eq!(updated.players.len(), 85);
    assert_eq!(updated.buckets.len(), 1800);
    let mut sequence = receipts::head(&fixture.db, "synthetic-b3").unwrap() as u64;
    let envelope = |sequence: &mut u64, fight: FightView| {
        *sequence += 1;
        EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic-b3".into(),
            sequence: Sequence(Timestamp(*sequence)),
            at: Timestamp(runs_workload::START + 5_000_000),
            event: EngineEvent::FightUpdated(fight),
        }
    };
    // Preserve an exact early snapshot, then grow the same run. Paired AB/BA
    // sampling exposes both sizes to the same host load and thermal conditions.
    let early_path = fixture._dir.path().join("early.sqlite");
    fixture
        .db
        .execute("VACUUM INTO ?1", [early_path.to_str().unwrap()])
        .unwrap();
    let mut early_db = rusqlite::Connection::open(early_path).unwrap();
    early_db.pragma_update(None, "journal_mode", "WAL").unwrap();
    early_db.pragma_update(None, "synchronous", "FULL").unwrap();
    fixture
        .db
        .pragma_update(None, "synchronous", "FULL")
        .unwrap();
    let mut early_sequence = sequence;
    for chunk in (EARLY..runs_workload::WORST.fights)
        .collect::<Vec<_>>()
        .chunks(100)
    {
        let events: Vec<_> = chunk
            .iter()
            .map(|i| {
                let mut fight = small.clone();
                fight.id = format!("fight-{i:05}");
                fight.target_id = *i as i32 + 2;
                envelope(&mut sequence, fight)
            })
            .collect();
        maxed_store::apply_batch(&mut fixture.db, &events).unwrap();
    }
    assert_eq!(
        fixture
            .db
            .query_row(
                "SELECT count(*) FROM pd_player_contributions WHERE run_id=?1 AND is_local=1",
                [runs_workload::RUN],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        3000
    );
    assert_eq!(
        fixture
            .db
            .query_row(
                "SELECT count(*) FROM fights WHERE run_id=?1",
                [runs_workload::RUN],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        3000
    );
    let mut early_samples = Vec::new();
    let mut late_samples = Vec::new();
    for i in 0..SAMPLES + 20 {
        updated.players[0].damage = 300. + (i % 2) as f64;
        updated.total_damage = updated.players.iter().map(|p| p.damage).sum();
        let order = if i % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for late in order {
            let (db, sequence, samples) = if late {
                (&mut fixture.db, &mut sequence, &mut late_samples)
            } else {
                (&mut early_db, &mut early_sequence, &mut early_samples)
            };
            let event = envelope(sequence, updated.clone());
            let start = std::time::Instant::now();
            maxed_store::apply_batch(db, &[event]).unwrap();
            if i >= 20 {
                samples.push(start.elapsed());
            }
        }
    }
    let report = |label: &str, mut samples: Vec<Duration>| {
        samples.sort();
        let p95 = samples[(SAMPLES * 95).div_ceil(100) - 1];
        eprintln!(
            "{label}: n={SAMPLES} median={:?} p95={p95:?} max={:?}",
            samples[SAMPLES / 2 - 1],
            samples[SAMPLES - 1]
        );
        p95
    };
    let early = report("FightUpdated early (20 fights)", early_samples);
    let late = report("FightUpdated late (3000 fights)", late_samples);
    assert!(
        late <= early.mul_f64(1.1),
        "late {late:?} exceeds early {early:?} +10%"
    );
}

#[test]
#[ignore = "release report only; event reads over 1.2 million synthetic events"]
fn event_history_volume_reads() {
    if cfg!(debug_assertions) {
        panic!("performance reports require --release");
    }
    let mut fixture = d_workload::Fixture::new();
    fixture.runs();
    // Populate the v11 source without maintaining an index, then measure the actual upgrade.
    downgrade::remove_maintenance_schema(&fixture.db);
    downgrade::remove_loot_stats_indexes(&fixture.db);
    fixture.db.execute_batch("DROP TABLE party_run_members; DROP TABLE party_players; DROP TABLE player_profiles; DROP TABLE player_gear_log; DROP TABLE scope_timelines; DROP TABLE player_timelines; DROP TRIGGER instance_events_fts_ai; DROP TRIGGER instance_events_fts_ad; DROP TRIGGER instance_events_fts_au; DROP TABLE event_fts; DELETE FROM schema_migrations WHERE version>=12; PRAGMA user_version=11;").unwrap();
    d_workload::volume(&mut fixture, true);
    // Five spread-out observations share one rare player, including two key pops.
    for (index, id) in [0, 250_000, 500_000, 750_000, 1_000_000]
        .into_iter()
        .enumerate()
    {
        let kind = if index < 2 {
            InstanceEventKind::PortalOpened {
                opener: Some("RareSyntheticPlayer".into()),
                opener_is_local: Some(false),
                portal_type: None,
                dungeon: None,
            }
        } else {
            InstanceEventKind::PlayerLeft {
                object_id: 1,
                player_name: Some("RareSyntheticPlayer".into()),
                class_id: None,
            }
        };
        let changed = fixture.db.execute(
            "UPDATE instance_events SET kind=?1,json=json_set(json,'$.kind',json(?2)) WHERE id=?3",
            rusqlite::params![if index < 2 { "portal_opened" } else { "player_left" }, serde_json::to_string(&kind).unwrap(), format!("pd-event-{id:07}")],
        ).unwrap();
        assert_eq!(changed, 1);
    }
    let before = event_database_bytes(&fixture);
    let started = std::time::Instant::now();
    migrate(&mut fixture.db).unwrap();
    let backfill = started.elapsed();
    eprintln!("1.2M event FTS migration backfill: {backfill:?}");
    assert!(
        backfill <= Duration::from_secs(30),
        "STOP: backfill exceeded 30 s"
    );
    let after = event_database_bytes(&fixture);
    eprintln!("1.2M event FTS storage: before={} bytes ({} allocated), after={} bytes ({} allocated), file delta={} bytes, allocated delta={} bytes", before.0, before.1, after.0, after.1, after.0-before.0, after.1-before.1);
    report_few_hit_pages(&fixture);
    report_fts_pages(&fixture, "departures-heavy 1.2M", "SynthAl");
    report_event_reads(&fixture);
}

// Checkpoint both snapshots so WAL bytes are included in the main-file size.
fn event_database_bytes(fixture: &d_workload::Fixture) -> (u64, u64) {
    fixture
        .db
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    let allocated = fixture.db.query_row(
        "SELECT (page_count-freelist_count)*page_size FROM pragma_page_count,pragma_freelist_count,pragma_page_size",
        [], |row| row.get::<_, i64>(0),
    ).unwrap();
    (
        std::fs::metadata(&fixture.path).unwrap().len(),
        u64::try_from(allocated).unwrap(),
    )
}

fn report_few_hit_pages(fixture: &d_workload::Fixture) {
    let reader = fixture.reader();
    for (label, kinds, expected) in [
        ("default kinds", vec![], 5),
        ("Key pops", vec![InstanceEventTag::PortalOpened], 2),
    ] {
        let filter = EventFilter {
            query: "RareSyntheticPlayer".into(),
            kinds,
            run_id: None,
            dungeon: None,
            cursor: None,
            limit: 50,
            range: None,
            opener_local: None,
        };
        let (sql, args) = portal_page_plan(&filter);
        report_portal_plan(&fixture.db, &format!("1.2M few-hit {label}"), &sql, &args);
        let started = std::time::Instant::now();
        let page = reader.list_events(filter).unwrap();
        let elapsed = started.elapsed();
        eprintln!(
            "1.2M few-hit {label} first page: {elapsed:?}; rows={}",
            page.rows.len()
        );
        assert_eq!(page.rows.len(), expected);
        assert!(page.next.is_none());
        assert!(
            elapsed <= Duration::from_secs(1),
            "STOP: few-hit first page exceeded 1 s; coordinator decision required"
        );
    }
}

fn report_event_reads(fixture: &d_workload::Fixture) {
    let reader = fixture.reader();
    let base = EventFilter {
        range: None,
        query: String::new(),
        opener_local: None,
        kinds: vec![],
        run_id: None,
        dungeon: None,
        cursor: None,
        limit: 100,
    };
    let range = TimeRange {
        start: Timestamp(0),
        end: Timestamp(30 * 86_400_000_000),
    };
    report_no_match_pages(fixture, "departures-heavy", range.clone(), false);
    for (label, range) in [
        ("30-day event summary", Some(range.clone())),
        ("all-time event summary", None),
    ] {
        let started = std::time::Instant::now();
        let summary = reader
            .event_summary(EventSummaryRequest {
                filter: EventFilter {
                    range,
                    ..base.clone()
                },
            })
            .unwrap();
        eprintln!(
            "{label}: {:?}; total={}; target <=250ms, coordinator stop threshold >1s",
            started.elapsed(),
            summary.total
        );
        assert_eq!(summary.total, 1_200_000);
    }
    let started = std::time::Instant::now();
    let page = reader
        .list_events(EventFilter {
            range: Some(range),
            query: "Vexlin".into(),
            opener_local: Some(true),
            ..base
        })
        .unwrap();
    eprintln!(
        "filtered event first page: {:?}; rows={}; target <=250ms, coordinator stop threshold >1s",
        started.elapsed(),
        page.rows.len()
    );
    let pops_sql = include_str!("../src/event_summary_pops.sql").replace("/* scope */", "e.kind IN ('player_death','player_left','portal_opened','realm_event_killed') AND e.at>=0 AND e.at<2592000000000");
    for sql in [
        "SELECT kind,count(*),min(at) FROM instance_events WHERE kind IN ('player_death','player_left','portal_opened','realm_event_killed') AND at>=0 AND at<2592000000000 GROUP BY kind",
        "SELECT kind,count(*),min(at) FROM instance_events WHERE kind IN ('player_death','player_left','portal_opened','realm_event_killed') GROUP BY kind",
        pops_sql.as_str(),
    ] {
        let mut statement = fixture.db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let plan = statement.query_map([], |row| row.get::<_, String>(3)).unwrap().collect::<Result<Vec<_>, _>>().unwrap().join("\n");
        eprintln!("{sql}\n{plan}");
    }
}

/// Read workload only: bulk inserts keep setup separate from projection timing.
/// Each group has three competing portals and one run entered after the last pop.
#[test]
#[ignore = "release report only; 200k mixed events, 60k portals and 20k runs"]
fn portal_heavy_event_reads() {
    if cfg!(debug_assertions) {
        panic!("performance reports require --release");
    }
    const GROUPS: u64 = 20_000;
    const STEP: u64 = 240_000_000;
    let mut fixture = d_workload::Fixture::new();
    let tx = fixture.db.transaction().unwrap();
    for n in 0..200 {
        let mut session = demo::demo_snapshot().session.unwrap();
        session.id = format!("portal-session-{n:03}");
        tx.execute(
            "INSERT INTO play_sessions(id,started,last_traffic,json) VALUES (?1,0,?2,?3)",
            rusqlite::params![
                session.id,
                (GROUPS * STEP) as i64,
                serde_json::to_string(&session).unwrap()
            ],
        )
        .unwrap();
    }
    {
        let mut events = tx
            .prepare("INSERT INTO instance_events VALUES (?1,?2,NULL,?3,?4,?5)")
            .unwrap();
        let mut runs = tx.prepare("INSERT INTO runs(id,session_id,dungeon,started,ended,outcome,complete_capture,json) VALUES (?1,?2,?3,?4,?5,'cleared',1,?6)").unwrap();
        for group in 0..GROUPS {
            let session = format!("portal-session-{:03}", group % 200);
            let dungeon = format!("Synthetic dungeon {:03}", group % 100);
            let mut run = demo::demo_run_story().run;
            run.id = format!("portal-run-{group:05}");
            run.session_id = session.clone();
            run.character_id = None;
            run.dungeon = dungeon.clone();
            run.started = Timestamp(group * STEP + 180_000_000);
            run.ended = Some(Timestamp(group * STEP + 210_000_000));
            run.completed_at = run.ended;
            run.died_at = None;
            run.outcome = Some(Outcome::Cleared);
            run.evidence.clear();
            runs.execute(rusqlite::params![
                run.id,
                session,
                dungeon,
                run.started.0 as i64,
                run.ended.unwrap().0 as i64,
                serde_json::to_string(&run).unwrap()
            ])
            .unwrap();
            for offset in 0..10 {
                let (tag, kind) = if offset < 3 {
                    let local = [Some(true), Some(false), None][((group + offset) % 3) as usize];
                    (
                        "portal_opened",
                        InstanceEventKind::PortalOpened {
                            opener: Some(
                                if local == Some(true) {
                                    "Vexlin"
                                } else {
                                    "Quenby"
                                }
                                .into(),
                            ),
                            opener_is_local: local,
                            portal_type: None,
                            dungeon: Some(dungeon.clone()),
                        },
                    )
                } else {
                    match offset % 3 {
                        0 => (
                            "player_left",
                            InstanceEventKind::PlayerLeft {
                                object_id: 1,
                                player_name: Some("Vexlin".into()),
                                class_id: None,
                            },
                        ),
                        1 => (
                            "player_death",
                            InstanceEventKind::PlayerDeath {
                                player_id: None,
                                player_name: "Quenby".into(),
                                level: None,
                                killed_by: Some("Synthetic sentinel".into()),
                                grave_type: None,
                                object_id: None,
                                class_id: None,
                                is_local: false,
                            },
                        ),
                        _ => (
                            "realm_event_killed",
                            InstanceEventKind::RealmEventKilled {
                                boss_key: "Synthetic sentinel".into(),
                                object_type: None,
                                killer: Some("Vexlin".into()),
                            },
                        ),
                    }
                };
                let event = InstanceEventView {
                    id: format!("portal-event-{group:05}-{offset}"),
                    session_id: session.clone(),
                    run_id: None,
                    map: Some("Synthetic realm".into()),
                    at: Timestamp(group * STEP + offset * 10_000_000),
                    kind,
                    provisional: group % 5 == 0,
                };
                events
                    .execute(rusqlite::params![
                        event.id,
                        session,
                        event.at.0 as i64,
                        tag,
                        serde_json::to_string(&event).unwrap()
                    ])
                    .unwrap();
            }
        }
    }
    tx.commit().unwrap();
    let shape: (i64, i64, i64, i64) = fixture.db.query_row(
        "SELECT count(*),sum(kind='portal_opened'),count(DISTINCT CASE WHEN kind='portal_opened' THEN json_extract(json,'$.kind.dungeon') END),(SELECT count(*) FROM runs) FROM instance_events",
        [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).unwrap();
    assert_eq!(shape, (200_000, 60_000, 100, 20_000));
    eprintln!("Portal-heavy shape: events={}, portals={}, dungeons={}, runs={}, sessions=200; local flags true/false/null=20000 each", shape.0,shape.1,shape.2,shape.3);
    report_fts_pages(&fixture, "portal-heavy 200k", "Vex");
    report_scoped_fts_miss(&fixture);
    let reader = fixture.reader();
    let base = EventFilter {
        kinds: vec![],
        run_id: None,
        dungeon: None,
        cursor: None,
        limit: 50,
        range: None,
        query: String::new(),
        opener_local: None,
    };
    let end = GROUPS * STEP;
    if !report_no_match_pages(
        &fixture,
        "portal-heavy",
        TimeRange {
            start: Timestamp(end - 30 * 86_400_000_000),
            end: Timestamp(end),
        },
        true,
    ) {
        return;
    }
    for (label, range) in [
        ("portal-heavy all-time summary", None),
        (
            "portal-heavy 30-day summary",
            Some(TimeRange {
                start: Timestamp(end - 30 * 86_400_000_000),
                end: Timestamp(end),
            }),
        ),
    ] {
        let (sqls, args) = portal_summary_plans(range.as_ref());
        for (part, sql) in &sqls {
            report_portal_plan(
                &fixture.db,
                &format!("{label}: {part}"),
                sql,
                if *part == "flagged since" { &[] } else { &args },
            );
        }
        let start = std::time::Instant::now();
        let summary = reader
            .event_summary(EventSummaryRequest {
                filter: EventFilter {
                    range,
                    ..base.clone()
                },
            })
            .unwrap();
        let elapsed = start.elapsed();
        eprintln!(
            "{label}: {elapsed:?}; total={}, portals={}, dungeons={}, local={}",
            summary.total,
            summary.by_kind.portal_opened,
            summary.dungeons.len(),
            summary.local_pops
        );
        assert_eq!(summary.dungeons.len(), 100);
        if elapsed > Duration::from_secs(1) {
            eprintln!("STOP: read exceeded 1 s");
            return;
        }
        if elapsed > Duration::from_millis(250) {
            for (part, sql) in &sqls {
                if !report_portal_component(
                    &fixture.db,
                    part,
                    sql,
                    if *part == "flagged since" { &[] } else { &args },
                ) {
                    return;
                }
            }
        }
    }
    for (label, filter) in [
        (
            "portal-heavy key pops",
            EventFilter {
                kinds: vec![InstanceEventTag::PortalOpened],
                ..base.clone()
            },
        ),
        (
            "portal-heavy local key pops",
            EventFilter {
                kinds: vec![InstanceEventTag::PortalOpened],
                opener_local: Some(true),
                ..base.clone()
            },
        ),
        (
            "portal-heavy dungeon key pops",
            EventFilter {
                kinds: vec![InstanceEventTag::PortalOpened],
                dungeon: Some("Synthetic dungeon 099".into()),
                ..base.clone()
            },
        ),
        (
            "portal-heavy events query",
            EventFilter {
                query: "Vexlin".into(),
                ..base
            },
        ),
    ] {
        let (sql, args) = portal_page_plan(&filter);
        report_portal_plan(&fixture.db, label, &sql, &args);
        let start = std::time::Instant::now();
        let page = reader.list_events(filter).unwrap();
        let elapsed = start.elapsed();
        eprintln!(
            "{label}: {elapsed:?}; rows={}, entered_runs={}",
            page.rows.len(),
            page.entered_runs.len()
        );
        assert_eq!(page.rows.len(), 50);
        assert!(!page.entered_runs.is_empty());
        let ids: Vec<_> = page
            .rows
            .iter()
            .filter(|row| {
                matches!(
                    row.kind,
                    InstanceEventKind::PortalOpened {
                        dungeon: Some(_),
                        ..
                    }
                )
            })
            .map(|row| rusqlite::types::Value::Text(row.id.clone()))
            .collect();
        let matched = portal_matches_plan(ids.len());
        report_portal_plan(
            &fixture.db,
            &format!("{label}: entered_runs"),
            &matched,
            &ids,
        );
        if elapsed > Duration::from_secs(1) {
            eprintln!("STOP: read exceeded 1 s");
            return;
        }
        if elapsed > Duration::from_millis(250)
            && (!report_portal_component(&fixture.db, "page selection", &sql, &args)
                || !report_portal_component(&fixture.db, "entered_runs", &matched, &ids))
        {
            return;
        }
    }
}

fn report_portal_plan(
    db: &rusqlite::Connection,
    label: &str,
    sql: &str,
    args: &[rusqlite::types::Value],
) {
    let mut statement = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let details = statement
        .query_map(rusqlite::params_from_iter(args), |row| {
            row.get::<_, String>(3)
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    eprintln!("{label} plan:\n{}", details.join("\n"));
}

fn report_portal_component(
    db: &rusqlite::Connection,
    label: &str,
    sql: &str,
    args: &[rusqlite::types::Value],
) -> bool {
    let start = std::time::Instant::now();
    let mut statement = db.prepare(sql).unwrap();
    let mut rows = statement.query(rusqlite::params_from_iter(args)).unwrap();
    while rows.next().unwrap().is_some() {}
    let elapsed = start.elapsed();
    eprintln!("Component {label}: {elapsed:?}");
    if elapsed > Duration::from_secs(1) {
        eprintln!("STOP: component read exceeded 1 s");
        return false;
    }
    true
}

// These diagnostic statements mirror chat.rs, including its bound parameters
// and the four-kind predicate, so plans describe the actual production reads.
fn portal_summary_plans(
    range: Option<&TimeRange>,
) -> (Vec<(&'static str, String)>, Vec<rusqlite::types::Value>) {
    let mut scope =
        "e.kind IN ('player_death','player_left','portal_opened','realm_event_killed')".to_owned();
    let mut args = vec![];
    if let Some(range) = range {
        scope.push_str(" AND e.at>=?1 AND e.at<?2");
        args.extend([
            rusqlite::types::Value::Integer(range.start.0 as i64),
            rusqlite::types::Value::Integer(range.end.0 as i64),
        ]);
    }
    (vec![
        ("kind counts", format!("SELECT e.kind,count(*),min(e.at) FROM instance_events e WHERE {scope} GROUP BY e.kind")),
        ("combined dungeon and local aggregates", include_str!("../src/event_summary_pops.sql").replace("/* scope */", &scope)),
        ("flagged since", "SELECT at FROM instance_events WHERE kind='portal_opened' AND json_extract(json,'$.kind.opener_is_local') IS NOT NULL ORDER BY at,id LIMIT 1".into()),
    ], args)
}

fn portal_page_plan(filter: &EventFilter) -> (String, Vec<rusqlite::types::Value>) {
    maxed_store::event_page_sql_for_tests(filter).unwrap()
}

fn portal_matches_plan(count: usize) -> String {
    let marks = (1..=count)
        .map(|n| format!("?{n}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("SELECT e.id,r.id,r.dungeon,r.started,r.outcome
        FROM instance_events e JOIN runs r ON r.id=(
            SELECT candidate.id FROM runs candidate
            WHERE candidate.session_id=e.session_id
              AND candidate.dungeon=json_extract(e.json,'$.kind.dungeon')
              AND candidate.started>=e.at AND candidate.started<=e.at+min(600000000,9223372036854775807-e.at)
              AND NOT EXISTS (SELECT 1 FROM instance_events later
                WHERE later.kind='portal_opened' AND later.session_id=e.session_id
                  AND later.at>e.at AND later.at<candidate.started
                  AND json_extract(later.json,'$.kind.dungeon')=candidate.dungeon)
            ORDER BY candidate.started,candidate.id LIMIT 1)
        WHERE e.id IN ({marks}) ORDER BY e.at DESC,e.id DESC LIMIT 100")
}

fn report_no_match_pages(
    fixture: &d_workload::Fixture,
    label: &str,
    range: TimeRange,
    stop_over_two_seconds: bool,
) -> bool {
    if cfg!(debug_assertions) {
        panic!("performance reports require --release");
    }
    let reader = fixture.reader();
    for (scope, range) in [("all-time", None), ("30-day", Some(range))] {
        let start = std::time::Instant::now();
        let page = reader
            .list_events(EventFilter {
                range,
                query: "SyntheticNoMatchingIdentity".into(),
                kinds: vec![],
                run_id: None,
                dungeon: None,
                opener_local: None,
                cursor: None,
                limit: 50,
            })
            .unwrap();
        let elapsed = start.elapsed();
        eprintln!(
            "{label} {scope} no-match first page: {elapsed:?}; rows={}",
            page.rows.len()
        );
        assert!(page.rows.is_empty());
        assert!(page.next.is_none());
        assert!(page.entered_runs.is_empty());
        if stop_over_two_seconds && scope == "all-time" && elapsed > Duration::from_secs(2) {
            eprintln!("STOP: 200k all-time no-match page exceeded 2 s");
            return false;
        }
    }
    true
}

fn report_fts_pages(fixture: &d_workload::Fixture, label: &str, matching: &str) {
    let reader = fixture.reader();
    for query in ["SyntheticNoMatchingIdentity", matching] {
        let filter = EventFilter {
            query: query.into(),
            kinds: vec![],
            run_id: None,
            dungeon: None,
            cursor: None,
            limit: 50,
            range: None,
            opener_local: None,
        };
        let (sql, args) = portal_page_plan(&filter);
        let (probe_sql, probe_binding) = maxed_store::event_fts_probe_for_tests(query).unwrap();
        report_portal_plan(
            &fixture.db,
            &format!("{label} {query}: existence probe"),
            probe_sql,
            &[probe_binding.into()],
        );
        report_portal_plan(
            &fixture.db,
            &format!("{label} {query}: page (only when probe matches)"),
            &sql,
            &args,
        );
        let started = std::time::Instant::now();
        let page = reader.list_events(filter).unwrap();
        eprintln!(
            "{label} {query} first page: {:?}; rows={}",
            started.elapsed(),
            page.rows.len()
        );
        assert_eq!(page.rows.len(), if query == matching { 50 } else { 0 });
    }
}

// The name exists in death/boss fields, but never in a portal opener field.
fn report_scoped_fts_miss(fixture: &d_workload::Fixture) {
    let query = "Synthetic";
    let (probe_sql, probe_binding) = maxed_store::event_fts_probe_for_tests(query).unwrap();
    let found: bool = fixture
        .db
        .query_row(probe_sql, [probe_binding], |row| row.get(0))
        .unwrap();
    assert!(
        found,
        "the global probe must match for this scoped-miss report"
    );
    let filter = EventFilter {
        query: query.into(),
        kinds: vec![InstanceEventTag::PortalOpened],
        run_id: None,
        dungeon: None,
        cursor: None,
        limit: 50,
        range: None,
        opener_local: None,
    };
    let (sql, args) = portal_page_plan(&filter);
    report_portal_plan(
        &fixture.db,
        "portal-heavy 200k present name excluded by Key pops scope",
        &sql,
        &args,
    );
    let reader = fixture.reader();
    let started = std::time::Instant::now();
    let page = reader.list_events(filter).unwrap();
    eprintln!(
        "portal-heavy 200k present name excluded by Key pops scope first page: {:?}; rows={}",
        started.elapsed(),
        page.rows.len()
    );
    assert!(page.rows.is_empty());
    assert!(page.next.is_none());
    assert!(page.entered_runs.is_empty());
}

#[test]
#[ignore = "release story baseline report; run serially before the dungeon/Home gate"]
fn account_scope_story_baselines_thirty_warm_reads() {
    // Measure the exact two story shapes used by the gate independently of
    // the unrelated Runs-list timing gates on the same workload.
    for shape in [runs_workload::REALISTIC, runs_workload::WORST] {
        let fixture = runs_workload::build(shape);
        fixture.validate();
        let reader = crate::common::account_scope_fixture::open_reader(&fixture.path).unwrap();
        d_workload::warm(&format!("{} story baseline", shape.name), || {
            reader.get_run(runs_workload::RUN).unwrap().unwrap()
        });
    }
}
