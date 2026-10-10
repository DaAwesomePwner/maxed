//! Invented run-history fixtures, derived from maxed-types' admitted synthetic demo.
mod common;
use common::*;
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;

fn database() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    (dir, path, db)
}

#[test]
fn invalid_filters_and_identifiers_fail_without_sql() {
    let (_dir, path, db) = database();
    let reader = open_reader(&path).unwrap();
    let boundary = "x".repeat(256);
    assert!(reader.get_run(&boundary).unwrap().is_none());
    assert!(reader.get_fight(&boundary).unwrap().is_none());
    assert!(reader.get_player_breakdown(&boundary, 1).unwrap().is_none());
    let mut sentinel = filter(RunSort::DurationDescending, 50);
    sentinel.cursor = Some(RunCursor {
        sort_value: "-1".into(),
        id: "demo-run".into(),
    });
    assert!(reader.list_runs(sentinel).unwrap().rows.is_empty());
    // A dropped table is a positive control: invalid arguments must fail first.
    db.pragma_update(None, "foreign_keys", false).unwrap();
    db.execute_batch("DROP TABLE fight_summaries; DROP TABLE fight_players; DROP TABLE runs;")
        .unwrap();
    for id in ["".to_owned(), "bad\nidentifier".into(), "x".repeat(257)] {
        assert!(matches!(reader.get_run(&id), Err(StoreError::InvalidEvent)));
        assert!(matches!(
            reader.get_fight(&id),
            Err(StoreError::InvalidEvent)
        ));
        assert!(matches!(
            reader.get_player_breakdown(&id, 1),
            Err(StoreError::InvalidEvent)
        ));
    }
    assert!(matches!(
        reader.list_runs(filter(RunSort::Newest, 50)),
        Err(StoreError::Io)
    ));
    for sort in [
        RunSort::Newest,
        RunSort::Oldest,
        RunSort::DurationAscending,
        RunSort::DurationDescending,
    ] {
        for value in [
            "01",
            "+1",
            "-0",
            "-2",
            "9223372036854775808",
            "-9223372036854775809",
        ]
        .into_iter()
        .chain((sort != RunSort::DurationDescending).then_some("-1"))
        {
            let mut query = filter(sort.clone(), 50);
            query.cursor = Some(RunCursor {
                sort_value: value.into(),
                id: "demo-run".into(),
            });
            assert!(
                matches!(reader.list_runs(query), Err(StoreError::InvalidEvent)),
                "{sort:?}: {value}"
            );
        }
    }
    let outside = i64::MAX as u64 + 1;
    for (start, end) in [(5, 5), (6, 5), (0, outside), (outside, outside + 1)] {
        let mut query = filter(RunSort::Newest, 50);
        query.range = Some(TimeRange {
            start: Timestamp(start),
            end: Timestamp(end),
        });
        assert!(
            matches!(reader.list_runs(query), Err(StoreError::InvalidEvent)),
            "{start}..{end}"
        );
    }
    for limit in [0, 101] {
        assert!(matches!(
            reader.list_runs(filter(RunSort::Newest, limit)),
            Err(StoreError::InvalidEvent)
        ));
    }
    for dungeon in ["".to_owned(), "x".repeat(257)] {
        let mut query = filter(RunSort::Newest, 50);
        query.dungeon = Some(dungeon);
        assert!(matches!(
            reader.list_runs(query),
            Err(StoreError::InvalidEvent)
        ));
    }
}

#[test]
fn summary_projection_is_independent_of_detail_size() {
    for boss in [false, true] {
        let (_dir, path, mut db) = database();
        let reader = open_reader(&path).unwrap();
        let mut fight = demo::demo_snapshot().fights.remove(0);
        if !boss {
            fight.boss_type = None;
            fight.target_kind = Some(TargetKind::Ordinary);
        }
        apply_batch(
            &mut db,
            &[envelope(8, EngineEvent::FightUpdated(fight.clone()))],
        )
        .unwrap();
        let mut before = reader.get_run("demo-run").unwrap().unwrap();
        assert_eq!(before.boss_fights.take().unwrap().len(), usize::from(boss));
        fight.buckets = vec![fight.buckets[0].clone(); 1800];
        fight.players[0].sources = vec![fight.players[0].sources[0].clone(); 3];
        apply_batch(
            &mut db,
            &[envelope(9, EngineEvent::FightUpdated(fight.clone()))],
        )
        .unwrap();
        let mut after = reader.get_run("demo-run").unwrap().unwrap();
        assert_eq!(
            after.boss_fights.take().unwrap(),
            if boss { vec![fight.clone()] } else { vec![] }
        );
        // Existing fields stay independent of details; only boss_fights expands.
        assert_eq!(
            serde_json::to_vec(&before).unwrap(),
            serde_json::to_vec(&after).unwrap()
        );
        assert_eq!(reader.get_fight("demo-fight").unwrap().unwrap(), fight);
        db.execute("UPDATE fights SET json='invalid detail'", [])
            .unwrap();
        // Corrupt boss details are omitted; ordinary details remain unread.
        // Neither can prevent the original summary fields from loading.
        let mut after = reader.get_run("demo-run").unwrap().unwrap();
        assert_eq!(after.boss_fights.take(), Some(vec![]));
        assert_eq!(after.boss_fights_limited, Some(boss));
        before.boss_fights_limited = Some(boss);
        assert_eq!(before, after);
        assert!(reader.get_fight("demo-fight").is_err());
    }
}

#[test]
fn reused_object_id_is_ambiguous() {
    let (_dir, path, mut db) = database();
    let reader = open_reader(&path).unwrap();
    let mut sequence = 8;
    for object_id in [i32::MIN, -7, 0, i32::MAX] {
        let mut fight = demo::demo_snapshot().fights.remove(0);
        fight.players[0].object_id = object_id;
        for copies in [0, 1, 2] {
            let mut updated = fight.clone();
            updated.players = (0..copies)
                .map(|i| {
                    let mut player = fight.players[0].clone();
                    player.player_id = format!("player-{i}");
                    player
                })
                .collect();
            apply_batch(
                &mut db,
                &[envelope(
                    sequence,
                    EngineEvent::FightUpdated(updated.clone()),
                )],
            )
            .unwrap();
            sequence += 1;
            assert_eq!(
                reader.get_player_breakdown(&fight.id, object_id).unwrap(),
                if copies == 1 {
                    Some(updated.players.remove(0))
                } else {
                    None
                }
            );
        }
    }
}

#[test]
fn source_rows_survive_projection_without_join_multiplication() {
    let (_dir, path, mut db) = database();
    let reader = open_reader(&path).unwrap();
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.issues.clear();
    fight.players[0].sources = vec![fight.players[0].sources[0].clone(); 3];
    fight.players[0].sources[1].kind = SourceKind::Ability;
    fight.players[0].sources[1].estimated = true;
    fight.players[0].sources[1].reasons = vec!["invented estimate".into()];
    fight.players[0].sources[2].item_type = None;
    let mut other = fight.players[0].clone();
    other.player_id = "other".into();
    other.is_local = false;
    fight.players.push(other);
    let before = reader.get_run("demo-run").unwrap().unwrap();
    let mut invalid = fight.clone();
    invalid.total_damage = 999.0;
    invalid.players.push(invalid.players[0].clone());
    assert!(apply_batch(&mut db, &[envelope(8, EngineEvent::FightUpdated(invalid))]).is_err());
    assert_eq!(reader.get_run("demo-run").unwrap().unwrap(), before);
    assert_eq!(
        reader.get_fight("demo-fight").unwrap().unwrap(),
        demo::demo_snapshot().fights[0]
    );
    let revision = envelope(8, EngineEvent::FightUpdated(fight.clone()));
    apply_batch(&mut db, std::slice::from_ref(&revision)).unwrap();
    apply_batch(&mut db, &[revision]).unwrap();
    let story = reader.get_run("demo-run").unwrap().unwrap();
    assert_eq!(story.fights.len(), 1);
    assert_eq!(story.drops.len(), 1);
    assert_eq!(story.fights[0].player_count, 2);
    assert!(story.fights[0].estimated);
    assert_eq!(story.fights[0].local_damage, Some(300.0));
    assert_eq!(reader.get_fight(&fight.id).unwrap().unwrap(), fight);
    assert_eq!(
        reader
            .get_player_by_id(&fight.id, "other")
            .unwrap()
            .unwrap(),
        fight.players[1]
    );
    fight.players.remove(1);
    fight.players[0].sources.remove(1);
    fight.run_id = None;
    apply_batch(
        &mut db,
        &[envelope(9, EngineEvent::FightUpdated(fight.clone()))],
    )
    .unwrap();
    assert!(reader
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .fights
        .is_empty());
    assert_eq!(reader.get_fight(&fight.id).unwrap().unwrap(), fight);
    assert!(reader
        .get_player_by_id(&fight.id, "other")
        .unwrap()
        .is_none());
    assert_eq!(
        db.query_row("SELECT count(*) FROM damage_sources", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn local_metrics_require_one_local_and_no_missing_identity() {
    let (_dir, path, mut db) = database();
    let reader = open_reader(&path).unwrap();
    for (sequence, variant) in (8..).zip(0..5) {
        let mut fight = demo::demo_snapshot().fights.remove(0);
        match variant {
            0 => {}
            1 => {
                fight.issues.clear();
                fight.players[0].is_local = false;
            }
            2 => {
                fight.issues.clear();
                let mut p = fight.players[0].clone();
                p.player_id = "duplicate-local".into();
                fight.players.push(p);
            }
            3 => {
                fight.issues.clear();
                fight.players[0].damage = 0.0;
                fight.players[0].dps = Some(0.0);
            }
            _ => {
                fight.issues.clear();
                fight.first_hit = fight.last_hit;
                fight.players[0].dps = None;
            }
        }
        apply_batch(
            &mut db,
            &[envelope(sequence, EngineEvent::FightUpdated(fight))],
        )
        .unwrap();
        let row = reader
            .get_run("demo-run")
            .unwrap()
            .unwrap()
            .fights
            .remove(0);
        if variant < 3 {
            assert_eq!(
                (row.local_damage, row.local_dps, row.local_rank),
                (None, None, None)
            );
        } else if variant == 3 {
            assert_eq!(
                (row.local_damage, row.local_dps, row.local_rank),
                (Some(0.0), Some(0.0), Some(1))
            );
        } else {
            assert_eq!((row.local_damage, row.local_dps), (Some(300.0), None));
        }
    }
}

#[test]
fn exact_serialized_byte_limits_and_oversize_errors_are_distinct_from_absence() {
    let (_dir, path, db) = database();
    let reader = open_reader(&path).unwrap();
    let mut run = demo::demo_run_story().run;
    let mut page = reader.list_runs(filter(RunSort::Newest, 50)).unwrap();
    let page_bytes = serde_json::to_vec(&page).unwrap().len();
    run.dungeon.push_str(&"x".repeat(1024 * 1024 - page_bytes));
    page.rows[0] = run.clone();
    assert_eq!(serde_json::to_vec(&page).unwrap().len(), 1024 * 1024);
    db.execute(
        "UPDATE runs SET json=?1",
        [serde_json::to_string(&run).unwrap()],
    )
    .unwrap();
    assert_eq!(reader.list_runs(filter(RunSort::Newest, 50)).unwrap(), page);
    run.dungeon.push('x');
    db.execute(
        "UPDATE runs SET json=?1",
        [serde_json::to_string(&run).unwrap()],
    )
    .unwrap();
    assert!(matches!(
        reader.list_runs(filter(RunSort::Newest, 50)),
        Err(StoreError::Limit)
    ));
    for (table, ceiling) in [("fights", 8 * 1024 * 1024), ("fight_players", 1024 * 1024)] {
        let mut value = if table == "fights" {
            serde_json::to_value(demo::demo_snapshot().fights.remove(0)).unwrap()
        } else {
            serde_json::to_value(demo::demo_snapshot().fights.remove(0).players.remove(0)).unwrap()
        };
        // JSON strings make the exact serialized boundary measurable, not an estimated size.
        let name = if table == "fights" {
            "session_id"
        } else {
            "name"
        };
        value[name] = serde_json::json!("");
        let overhead = serde_json::to_vec(&value).unwrap().len();
        value[name] = serde_json::json!("x".repeat(ceiling - overhead));
        db.execute(
            &format!("UPDATE {table} SET json=?1"),
            [serde_json::to_string(&value).unwrap()],
        )
        .unwrap();
        if table == "fights" {
            assert!(reader.get_fight("demo-fight").unwrap().is_some());
        } else {
            assert!(reader
                .get_player_breakdown("demo-fight", 1)
                .unwrap()
                .is_some());
        }
        value[name] = serde_json::json!("x".repeat(ceiling - overhead + 1));
        db.execute(
            &format!("UPDATE {table} SET json=?1"),
            [serde_json::to_string(&value).unwrap()],
        )
        .unwrap();
        let error = if table == "fights" {
            reader.get_fight("demo-fight").unwrap_err()
        } else {
            reader.get_player_breakdown("demo-fight", 1).unwrap_err()
        };
        assert!(matches!(error, StoreError::Limit));
    }
    assert!(reader.get_run("absent").unwrap().is_none());
    db.execute("UPDATE runs SET json='bad'", []).unwrap();
    assert!(matches!(
        reader.get_run("demo-run"),
        Err(StoreError::InvalidEvent)
    ));
}

#[test]
fn story_row_ceilings_have_headroom_and_never_truncate_success() {
    for (table, limit) in [
        ("fight_summaries", 12000),
        ("timeline", 20000),
        ("drops", 2000),
    ] {
        let (_dir, path, mut db) = database();
        let reader = open_reader(&path).unwrap();
        db.pragma_update(None, "foreign_keys", false).unwrap();
        let tx = db.transaction().unwrap();
        tx.execute(&format!("DELETE FROM {table}"), []).unwrap();
        for i in 0..limit {
            match table {
                "fight_summaries" => {
                    let mut row = demo::demo_run_story().fights.remove(0);
                    row.id = format!("f-{i:05}");
                    tx.execute(
                        "INSERT INTO fight_summaries VALUES(?1,'demo-run',2,?2)",
                        rusqlite::params![row.id, serde_json::to_string(&row).unwrap()],
                    )
                    .unwrap();
                }
                "timeline" => {
                    tx.execute("INSERT INTO timeline VALUES(?1,'demo-run',2,'fight','demo-fight','observed')", [format!("t-{i:05}")]).unwrap();
                }
                _ => {
                    let mut row = demo::demo_snapshot().drops.remove(0);
                    row.id = format!("d-{i:05}");
                    tx.execute(
                        "INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json) VALUES(?1,'demo-session','demo-run',2,400,NULL,?2)",
                        rusqlite::params![row.id, serde_json::to_string(&row).unwrap()],
                    )
                    .unwrap();
                }
            }
        }
        tx.commit().unwrap();
        let story = reader.get_run("demo-run").unwrap().unwrap();
        assert_eq!(
            match table {
                "fight_summaries" => story.fights.len(),
                "timeline" => story.timeline.len(),
                _ => story.drops.len(),
            },
            limit
        );
        let sql = match table {
            "fight_summaries" => "INSERT INTO fight_summaries SELECT 'extra',run_id,started,json FROM fight_summaries LIMIT 1",
            "timeline" => "INSERT INTO timeline SELECT 'extra',run_id,at,kind,related_id,explanation FROM timeline LIMIT 1",
            _ => "INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json) SELECT 'extra',session_id,run_id,at,bag_type,bag_rank,json FROM drops LIMIT 1",
        };
        db.execute(sql, []).unwrap();
        assert!(matches!(reader.get_run("demo-run"), Err(StoreError::Limit)));
    }
}

#[test]
fn story_exact_aggregate_byte_boundary_includes_timeline_escaping() {
    let (_dir, path, db) = database();
    let reader = open_reader(&path).unwrap();
    let mut baseline = reader.get_run("demo-run").unwrap().unwrap();
    // At the exact boundary, optional boss details yield to summary data.
    baseline.boss_fights = Some(vec![]);
    baseline.boss_fights_limited = Some(true);
    let row = &baseline.timeline[0];
    let bytes = serde_json::to_vec(&baseline).unwrap().len();
    let mut text = row.explanation.clone();
    text.push_str(&"x".repeat(32 * 1024 * 1024 - bytes));
    db.execute(
        "UPDATE timeline SET explanation=?1 WHERE id=?2",
        rusqlite::params![text, row.id],
    )
    .unwrap();
    let story = reader.get_run("demo-run").unwrap().unwrap();
    assert_eq!(story.boss_fights, Some(vec![]));
    assert_eq!(story.boss_fights_limited, Some(true));
    assert_eq!(serde_json::to_vec(&story).unwrap().len(), 32 * 1024 * 1024);
    text.push('x');
    db.execute(
        "UPDATE timeline SET explanation=?1 WHERE id=?2",
        rusqlite::params![text, row.id],
    )
    .unwrap();
    assert!(matches!(reader.get_run("demo-run"), Err(StoreError::Limit)));
    // Refuse before materializing a row, including UTF-8 byte accounting.
    db.execute(
        "UPDATE timeline SET explanation=?1 WHERE id=?2",
        rusqlite::params!["\u{e9}".repeat(16 * 1024 * 1024 + 1), row.id],
    )
    .unwrap();
    assert!(matches!(reader.get_run("demo-run"), Err(StoreError::Limit)));
}

#[test]
fn detail_expansion_at_eight_and_eighty_five_players_only_expands_boss_views() {
    for players in [8, 85] {
        let (_dir, path, mut db) = database();
        let reader = open_reader(&path).unwrap();
        let mut fight = demo::demo_snapshot().fights.remove(0);
        fight.players = (0..players)
            .map(|i| {
                let mut player = fight.players[0].clone();
                player.player_id = format!("p-{i}");
                player.is_local = i == 0;
                player.object_id = i;
                player.sources = vec![player.sources[0].clone(); 3];
                player
            })
            .collect();
        fight.buckets = vec![fight.buckets[0].clone(); 120];
        apply_batch(
            &mut db,
            &[envelope(8, EngineEvent::FightUpdated(fight.clone()))],
        )
        .unwrap();
        let mut before = reader.get_run("demo-run").unwrap().unwrap();
        assert_eq!(before.boss_fights.take().unwrap()[0], fight);
        fight.buckets = vec![fight.buckets[0].clone(); 1800];
        apply_batch(
            &mut db,
            &[envelope(9, EngineEvent::FightUpdated(fight.clone()))],
        )
        .unwrap();
        let mut after = reader.get_run("demo-run").unwrap().unwrap();
        assert_eq!(after.boss_fights.take().unwrap()[0], fight);
        assert_eq!(
            serde_json::to_vec(&before).unwrap(),
            serde_json::to_vec(&after).unwrap()
        );
        assert_eq!(
            reader.get_run("demo-run").unwrap().unwrap().fights[0].player_count,
            players as u32
        );
    }
}

#[test]
fn comparison_requires_three_earlier_observed_clears() {
    let (_dir, path, mut db) = database();
    let reader = open_reader(&path).unwrap();
    let mut sequence = 8;
    let mut current = demo::demo_run_story().run;
    current.id = "current".into();
    current.dungeon = "Comparison fixture".into();
    current.started = Timestamp(100_000_000);
    current.completed_at = Some(Timestamp(108_000_000));
    current.ended = Some(Timestamp(130_000_000));
    apply_batch(
        &mut db,
        &[envelope(sequence, EngineEvent::RunStarted(current.clone()))],
    )
    .unwrap();
    sequence += 1;
    assert!(reader
        .get_run("current")
        .unwrap()
        .unwrap()
        .comparison
        .is_none());
    for i in 0..3 {
        let mut prior = current.clone();
        prior.id = format!("prior-{i}");
        prior.character_id = None;
        prior.started = Timestamp(10_000_000 + i * 10_000_000);
        prior.completed_at = Some(Timestamp(prior.started.0 + 10_000_000));
        apply_batch(
            &mut db,
            &[envelope(sequence, EngineEvent::RunStarted(prior))],
        )
        .unwrap();
        sequence += 1;
        let comparison = reader.get_run("current").unwrap().unwrap().comparison;
        if i < 2 {
            assert!(comparison.is_none());
        } else {
            let comparison = comparison.unwrap();
            assert_eq!(comparison.previous_clears, 3);
            assert_eq!(comparison.mean_micros, 10_000_000.0);
            assert_eq!(comparison.change_fraction, -0.2);
        }
    }
    for variant in 0..8 {
        let mut excluded = current.clone();
        excluded.id = format!("excluded-{variant}");
        excluded.started = Timestamp(20_000_000);
        excluded.completed_at = Some(Timestamp(25_000_000));
        match variant {
            0 => excluded.started = current.started,
            1 => excluded.started = Timestamp(200_000_000),
            2 => excluded.outcome = Some(Outcome::Left),
            3 => excluded.outcome = Some(Outcome::Died),
            4 => excluded.outcome = None,
            5 => excluded.completed_at = Some(excluded.started),
            6 => excluded.completed_at = Some(Timestamp(1)),
            _ => {
                excluded.evidence = vec![EvidenceAt {
                    at: Timestamp(25_000_000),
                    evidence: Evidence::CompletionCounter {
                        before: 0,
                        after: 1,
                        delay_micros: 0,
                    },
                }]
            }
        }
        apply_batch(
            &mut db,
            &[envelope(sequence, EngineEvent::RunStarted(excluded))],
        )
        .unwrap();
        sequence += 1;
    }
    assert_eq!(
        reader
            .get_run("current")
            .unwrap()
            .unwrap()
            .comparison
            .unwrap()
            .previous_clears,
        3
    );
    current.died_at = Some(Timestamp(120_000_000));
    apply_batch(
        &mut db,
        &[envelope(sequence, EngineEvent::RunUpdated(current.clone()))],
    )
    .unwrap();
    sequence += 1;
    let story = reader.get_run("current").unwrap().unwrap();
    assert!(story.timeline.iter().any(|t| t.kind == TimelineKind::Death));
    assert!(story
        .timeline
        .iter()
        .any(|t| t.kind == TimelineKind::Completion));
    assert!(story.comparison.is_some());
    assert!(!story.run.complete_capture);
    current.evidence = vec![EvidenceAt {
        at: Timestamp(108_000_000),
        evidence: Evidence::CompletionCounter {
            before: 0,
            after: 1,
            delay_micros: 0,
        },
    }];
    apply_batch(
        &mut db,
        &[envelope(sequence, EngineEvent::RunUpdated(current))],
    )
    .unwrap();
    assert!(reader
        .get_run("current")
        .unwrap()
        .unwrap()
        .comparison
        .is_none());
}

#[path = "common/runs_workload.rs"]
mod runs_workload;

#[test]
fn fixed_runs_workloads_counts_bytes_and_plans() {
    for shape in [runs_workload::REALISTIC, runs_workload::WORST] {
        runs_workload::build(shape).validate();
    }
}

#[test]
#[ignore = "release performance gate; run serially with --ignored --nocapture"]
fn realistic_runs_queries_thirty_warm_reads() {
    benchmark_runs(runs_workload::REALISTIC);
}

#[test]
#[ignore = "release performance gate; run serially with --ignored --nocapture"]
fn worst_case_runs_queries_thirty_warm_reads() {
    benchmark_runs(runs_workload::WORST);
}

fn benchmark_runs(shape: runs_workload::Shape) {
    use runs_workload::{bytes, cases, FIGHT, RUN};
    use std::time::{Duration, Instant};
    if cfg!(debug_assertions) {
        panic!("performance gates require --release");
    }
    let fixture = runs_workload::build(shape);
    let reader = fixture.validate();
    let before = Instant::now();
    let setup_reader = open_reader(&fixture.path).unwrap();
    let setup = before.elapsed();
    drop(setup_reader);
    eprintln!("{} reader open/config/schema: {setup:?}", shape.name);
    fn measure<T: serde::Serialize>(label: &str, limit: u64, mut read: impl FnMut() -> T) {
        let expected_bytes = bytes(&read()); // One warm-up, separate from all 30 samples.
        let mut samples = Vec::with_capacity(30);
        for _ in 0..30 {
            let before = Instant::now();
            let result = read(); // Includes production's capped serialization check.
            samples.push(before.elapsed());
            assert_eq!(bytes(&result), expected_bytes); // Reporting serialization is outside timing.
        }
        samples.sort();
        eprintln!(
            "{label}: n=30 median={:?} p95={:?} max={:?} bytes={expected_bytes} limit={limit}ms",
            samples[14], samples[28], samples[29]
        );
        assert!(
            samples[28] <= Duration::from_millis(limit),
            "{label}: p95 {:?} > {limit}ms",
            samples[28]
        );
    }
    for (label, filter) in cases() {
        measure(&format!("{} list {label}", shape.name), 50, || {
            reader.list_runs(filter.clone()).unwrap()
        });
    }
    let worst = shape.name == "worst-case";
    measure(
        &format!("{} story", shape.name),
        if worst { 1500 } else { 100 },
        || reader.get_run(RUN).unwrap().unwrap(),
    );
    measure(
        &format!("{} fight", shape.name),
        if worst { 1500 } else { 50 },
        || reader.get_fight(FIGHT).unwrap().unwrap(),
    );
    measure(
        &format!("{} player", shape.name),
        if worst { 1500 } else { 50 },
        || reader.get_player_breakdown(FIGHT, 1).unwrap().unwrap(),
    );
}

#[test]
fn story_summary_preserves_target_kind_and_accepts_legacy_rows() {
    let (_dir, path, mut db) = database();
    let reader = open_reader(&path).unwrap();
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.boss_type = None;
    fight.target_kind = Some(TargetKind::Miniboss);
    apply_batch(&mut db, &[envelope(8, EngineEvent::FightUpdated(fight))]).unwrap();
    let summary = reader
        .get_run("demo-run")
        .unwrap()
        .unwrap()
        .fights
        .remove(0);
    assert_eq!(summary.target_kind, Some(TargetKind::Miniboss));
    assert_eq!(summary.boss_type, None);
    db.execute(
        "UPDATE fight_summaries SET json=json_remove(json,'$.target_kind')",
        [],
    )
    .unwrap();
    assert_eq!(
        reader.get_run("demo-run").unwrap().unwrap().fights[0].target_kind,
        None
    );
}
