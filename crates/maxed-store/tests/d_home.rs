use crate::common;
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;
#[test]
fn dungeon_stats_session_and_today_scopes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()).unwrap();
    let reader = open_reader(&path).unwrap();
    let session = reader
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap()
        .dungeon_stats
        .unwrap();
    let today = reader
        .home_summary(HomeScope::Today {
            range: TimeRange {
                start: Timestamp(0),
                end: Timestamp(10_000_000),
            },
        })
        .unwrap()
        .dungeon_stats
        .unwrap();
    assert_eq!(session, today);
    assert_eq!(session.len(), 1);
    assert_eq!(session[0].runs, 1);
    assert!(reader
        .home_summary(HomeScope::Today {
            range: TimeRange {
                start: Timestamp(10_000_000),
                end: Timestamp(20_000_000)
            }
        })
        .unwrap()
        .dungeon_stats
        .unwrap()
        .is_empty());
}
#[test]
fn observed_clear_qualification_matches_comparison() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()[..2]).unwrap();
    for i in 0..7 {
        let mut r = demo::demo_run_story().run;
        r.id = format!("r-{i}");
        if i == 6 {
            r.area_kind = AreaKind::Realm;
        }
        r.started = Timestamp(10_000_000 * i);
        r.completed_at = Some(Timestamp(r.started.0 + if i == 4 { 0 } else { 1_000_000 }));
        r.outcome = Some(if i == 5 {
            Outcome::Died
        } else {
            Outcome::Cleared
        });
        r.evidence.clear();
        if i != 3 {
            r.evidence.push(EvidenceAt {
                at: r.completed_at.unwrap(),
                evidence: Evidence::Victory,
            });
        }
        apply_batch(
            &mut db,
            &[common::envelope(i + 3, EngineEvent::RunEnded(r))],
        )
        .unwrap();
    }
    let mut current = demo::demo_run_story().run;
    current.id = "comparison-current".into();
    current.started = Timestamp(100_000_000);
    current.completed_at = Some(Timestamp(101_000_000));
    current.ended = Some(Timestamp(102_000_000));
    current.outcome = Some(Outcome::Cleared);
    current.evidence = vec![EvidenceAt {
        at: Timestamp(101_000_000),
        evidence: Evidence::Victory,
    }];
    apply_batch(
        &mut db,
        &[common::envelope(10, EngineEvent::RunEnded(current))],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let comparison = reader
        .get_run("comparison-current")
        .unwrap()
        .unwrap()
        .comparison
        .unwrap();
    let stats = reader
        .home_summary(HomeScope::Today {
            range: TimeRange {
                start: Timestamp(0),
                end: Timestamp(100_000_000),
            },
        })
        .unwrap()
        .dungeon_stats
        .unwrap();
    assert_eq!(stats[0].runs, 7);
    assert_eq!(stats[0].clears, 6);
    assert_eq!(stats[0].deaths, 1);
    assert_eq!(stats[0].observed_clears, comparison.previous_clears);
    assert_eq!(stats[0].mean_clear_micros, Some(comparison.mean_micros));
    assert_eq!(stats[0].observed_clears, 3);
    assert_eq!(stats[0].mean_clear_micros, Some(1_000_000.));
    assert_eq!(stats[0].best_clear_micros, Some(1_000_000));
}
#[test]
fn dungeon_stats_limit_20() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()[..2]).unwrap();
    for i in 0..60 {
        let mut r = demo::demo_run_story().run;
        r.id = format!("r-{i}");
        r.dungeon = format!("Dungeon-{i:02}");
        apply_batch(
            &mut db,
            &[common::envelope(i + 3, EngineEvent::RunEnded(r))],
        )
        .unwrap();
    }
    let stats = open_reader(&path)
        .unwrap()
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap()
        .dungeon_stats
        .unwrap();
    assert_eq!(stats.len(), 20);
    assert_eq!(stats[0].dungeon, "Dungeon-00");
    assert_eq!(stats[19].dungeon, "Dungeon-19");
}

#[test]
fn dungeon_bests_are_all_time_and_tied_scoped_clears_qualify() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("all-time-bests");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()[..2]).unwrap();
    for (i, duration) in [9_000_000, 5_000_000, 5_000_000, 7_000_000, 1_000_000]
        .into_iter()
        .enumerate()
    {
        let mut run = demo::demo_run_story().run;
        run.id = format!("pb-{i}");
        run.dungeon = "PB Dungeon".into();
        run.started = Timestamp(10_000_000 * (i as u64 + 1));
        run.completed_at = Some(Timestamp(run.started.0 + duration));
        run.ended = run.completed_at;
        run.outcome = Some(Outcome::Cleared);
        run.evidence = if i == 4 {
            vec![]
        } else {
            vec![EvidenceAt {
                at: run.completed_at.unwrap(),
                evidence: Evidence::Victory,
            }]
        };
        apply_batch(
            &mut db,
            &[common::envelope(i as u64 + 3, EngineEvent::RunEnded(run))],
        )
        .unwrap();
    }
    let reader = open_reader(&path).unwrap();
    assert_eq!(
        reader.dungeon_bests().unwrap(),
        vec![DungeonBest {
            dungeon: "PB Dungeon".into(),
            best_clear_micros: 5_000_000,
        }]
    );
    let session = reader
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap();
    assert_eq!(
        session.dungeon_stats.unwrap()[0].all_time_best_clear_micros,
        Some(5_000_000)
    );
    for (start, expected_pb) in [
        (10_000_000, false),
        (20_000_000, true),
        (30_000_000, true),
        (40_000_000, false),
        (50_000_000, false),
    ] {
        let summary = reader
            .home_summary(HomeScope::Today {
                range: TimeRange {
                    start: Timestamp(start),
                    end: Timestamp(start + 10_000_000),
                },
            })
            .unwrap();
        let rows = summary.dungeon_stats.unwrap();
        let row = &rows[0];
        assert_eq!(row.all_time_best_clear_micros, Some(5_000_000));
        assert_eq!(
            row.best_clear_micros.is_some()
                && row.best_clear_micros == row.all_time_best_clear_micros,
            expected_pb
        );
    }
}
#[test]
fn dungeon_bests_exclude_unqualified_clears_and_keep_unknown_null() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("no-bests");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &common::story_events()[..2]).unwrap();
    assert!(open_reader(&path)
        .unwrap()
        .dungeon_bests()
        .unwrap()
        .is_empty());
    for i in 0..5 {
        let mut run = demo::demo_run_story().run;
        run.id = format!("invalid-pb-{i}");
        run.dungeon = format!("Invalid {i}");
        run.started = Timestamp(10_000_000 * (i + 1));
        run.completed_at = Some(Timestamp(run.started.0 + 1_000_000));
        run.ended = run.completed_at;
        run.outcome = Some(Outcome::Cleared);
        run.evidence = vec![EvidenceAt {
            at: run.completed_at.unwrap(),
            evidence: Evidence::Victory,
        }];
        match i {
            0 => run.evidence.clear(),
            1 => run.area_kind = AreaKind::Realm,
            2 => run.completed_at = Some(run.started),
            3 => run.outcome = Some(Outcome::Died),
            _ => run.completed_at = None,
        }
        apply_batch(
            &mut db,
            &[common::envelope(i + 3, EngineEvent::RunEnded(run))],
        )
        .unwrap();
    }
    let reader = open_reader(&path).unwrap();
    assert!(reader.dungeon_bests().unwrap().is_empty());
    let rows = reader
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap()
        .dungeon_stats
        .unwrap();
    assert_eq!(rows.len(), 5);
    assert!(rows
        .iter()
        .all(|row| row.all_time_best_clear_micros.is_none()));
}
