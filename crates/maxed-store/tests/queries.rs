use crate::common;
use common::*;
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;

fn database() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    (dir, path, db)
}

#[test]
fn clear_comparison_needs_three_prior_observed_clears() {
    assert!(clear_comparison(8.0, &[10.0, 10.0]).is_none());
    let value = clear_comparison(8.0, &[10.0, 10.0, 10.0]).unwrap();
    assert_eq!(value.previous_clears, 3);
    assert!((value.change_fraction + 0.2).abs() < 1e-12);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(clear_comparison(bad, &[10.0; 3]).is_none());
        assert!(clear_comparison(8.0, &[10.0, 10.0, bad]).is_none());
    }
}

#[test]
fn story_drilldown_nulls_and_exact_bag_counts_survive_reopen() {
    let (_dir, path, db) = database();
    drop(db);
    let reader = open_reader(&path).unwrap();
    let story = reader.get_run("demo-run").unwrap().unwrap();
    assert_eq!(
        story.character.unwrap().name.as_deref(),
        Some("Demo Knight")
    );
    assert_eq!(
        reader
            .get_fight(&story.fights[0].id)
            .unwrap()
            .unwrap()
            .players[0]
            .sources
            .len(),
        1
    );
    assert_eq!(
        reader
            .get_player_breakdown("demo-fight", 1)
            .unwrap()
            .unwrap()
            .sources[0]
            .item_type,
        Some(200)
    );
    assert!(reader.get_run("absent").unwrap().is_none());
    assert!(reader
        .get_player_by_id("demo-fight", "absent")
        .unwrap()
        .is_none());
    assert_eq!(
        reader
            .bag_count_since(
                400,
                TimeRange {
                    start: Timestamp(5_000_000),
                    end: Timestamp(5_000_001)
                }
            )
            .unwrap(),
        1
    );
    assert_eq!(
        reader
            .bag_count_since(
                401,
                TimeRange {
                    start: Timestamp(0),
                    end: Timestamp(6_000_000)
                }
            )
            .unwrap(),
        0
    );
    assert_eq!(
        reader
            .bag_count_since(
                400,
                TimeRange {
                    start: Timestamp(0),
                    end: Timestamp(5_000_000)
                }
            )
            .unwrap(),
        0
    );
}

#[test]
fn object_id_reuse_does_not_select_a_generation_by_accident() {
    let (_dir, path, mut db) = database();
    let mut fight = demo::demo_snapshot().fights.remove(0);
    let mut second = fight.players[0].clone();
    second.player_id = "another-generation".into();
    fight.players.push(second);
    apply_batch(&mut db, &[envelope(8, EngineEvent::FightUpdated(fight))]).unwrap();
    let reader = open_reader(&path).unwrap();
    assert!(reader
        .get_player_breakdown("demo-fight", 1)
        .unwrap()
        .is_none());
    assert!(reader
        .get_player_by_id("demo-fight", "another-generation")
        .unwrap()
        .is_some());
    assert!(reader
        .home_summary(HomeScope::Session {
            id: "demo-session".into()
        })
        .unwrap()
        .top_fight
        .is_none());
}

#[test]
fn home_counts_death_after_clear_and_preserves_fame_and_unknown_bag_rank() {
    let (_dir, path, mut db) = database();
    apply_batch(
        &mut db,
        &[envelope(
            8,
            EngineEvent::PlayerDied {
                at: Timestamp(5_000_000),
                character_id: Some("demo-character".into()),
                run_id: Some("demo-run".into()),
            },
        )],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let home = reader
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap();
    assert_eq!((home.runs, home.clears, home.deaths), (1, 1, 1));
    assert_eq!(
        home.fame_delta,
        Some(10),
        "counts from the first in-session sample"
    );
    assert!(home.best_drop.is_none());
    assert!(home.top_fight.is_none());
    assert!(!home.issues.is_empty());
    let home = reader
        .home_summary(HomeScope::Today {
            range: TimeRange {
                start: Timestamp(0),
                end: Timestamp(1_000_000),
            },
        })
        .unwrap();
    assert_eq!(home.runs, 0);
    assert!(home.fame_delta.is_none());
}

#[test]
fn fame_delta_counts_from_first_in_scope_sample_for_today_and_session() {
    for with_baseline in [false, true] {
        let (_dir, path, mut db) = database();
        let mut sequence = 8;
        if with_baseline {
            for (at, fame) in [(100, 1000), (900_000, 1200)] {
                apply_batch(
                    &mut db,
                    &[envelope(
                        sequence,
                        EngineEvent::FameChanged {
                            at: Timestamp(at),
                            character_id: "demo-character".into(),
                            fame,
                        },
                    )],
                )
                .unwrap();
                sequence += 1;
            }
        }
        let reader = open_reader(&path).unwrap();
        for scope in [
            HomeScope::Session {
                id: "demo-session".into(),
            },
            HomeScope::Today {
                range: TimeRange {
                    start: Timestamp(1_000_000),
                    end: Timestamp(6_000_000),
                },
            },
        ] {
            let owned_baselines = with_baseline && matches!(scope, HomeScope::Session { .. });
            let home = reader.home_summary(scope.clone()).unwrap();
            // The delta always equals the chart's last point minus its first.
            assert_eq!(
                home.fame_delta,
                Some(home.fame.last().unwrap().fame - home.fame.first().unwrap().fame)
            );
            assert_eq!(
                home.fame_delta,
                Some(if owned_baselines { 260 } else { 10 })
            );
            assert_eq!(home.fame_missing_baselines, 0);
            if owned_baselines {
                assert_eq!(home.fame.first().unwrap().fame, 1000);
                assert_eq!(home.fame.last().unwrap().fame, 1260);
                // Fame from a previous session is not credited to this one.
                db.execute("INSERT INTO fame_samples VALUES('previous',1,'demo-character',50,990,NULL),('previous',2,'demo-character',500000,1100,NULL)",[]).unwrap();
                let with_external = reader.home_summary(scope).unwrap();
                assert_eq!(with_external.fame, home.fame);
                assert_eq!(with_external.fame_delta, home.fame_delta);
                assert_eq!(with_external.fame_missing_baselines, 0);
            }
            assert_eq!(
                home.fame.len(),
                if owned_baselines { 4 } else { 2 },
                "Session membership follows ownership; Today excludes pre-midnight points"
            );
        }
    }
}

#[test]
fn fame_delta_sums_each_character_from_its_first_in_scope_sample() {
    let (_dir, path, mut db) = database();
    let mut other = demo::demo_snapshot().character.unwrap();
    other.id = "second-character".into();
    let mut unknown = other.clone();
    unknown.id = "no-baseline".into();
    let events = [
        EngineEvent::CharacterUpdated(other),
        EngineEvent::CharacterUpdated(unknown),
        EngineEvent::FameChanged {
            at: Timestamp(500_000),
            character_id: "demo-character".into(),
            fame: 1200,
        },
        EngineEvent::FameChanged {
            at: Timestamp(500_000),
            character_id: "second-character".into(),
            fame: 50,
        },
        EngineEvent::FameChanged {
            at: Timestamp(2_000_000),
            character_id: "second-character".into(),
            fame: 70,
        },
        EngineEvent::FameChanged {
            at: Timestamp(5_000_000),
            character_id: "second-character".into(),
            fame: 90,
        },
        EngineEvent::FameChanged {
            at: Timestamp(5_000_000),
            character_id: "no-baseline".into(),
            fame: 999,
        },
    ];
    apply_batch(
        &mut db,
        &events
            .into_iter()
            .enumerate()
            .map(|(i, e)| envelope(8 + i as u64, e))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let session = reader
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap();
    assert_eq!(session.fame_delta, Some(100)); // (1260-1200) + (90-50) + (999-999).
    db.execute(
        "UPDATE fame_samples SET session_id=NULL WHERE at=500000",
        [],
    )
    .unwrap();
    let session = reader
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap();
    assert_eq!(session.fame_delta, Some(30)); // (1260-1250) + (90-70).
    let today = reader
        .home_summary(HomeScope::Today {
            range: TimeRange {
                start: Timestamp(3_000_000),
                end: Timestamp(6_000_000),
            },
        })
        .unwrap();
    assert_eq!(today.fame_delta, Some(0)); // Pre-midnight samples are not baselines.
    assert_eq!(today.fame.len(), 3);
    apply_batch(
        &mut db,
        &[envelope(
            15,
            EngineEvent::FameChanged {
                at: Timestamp(5_500_000),
                character_id: "second-character".into(),
                fame: i32::MAX,
            },
        )],
    )
    .unwrap();
    apply_batch(
        &mut db,
        &[envelope(
            16,
            EngineEvent::FameChanged {
                at: Timestamp(5_500_000),
                character_id: "demo-character".into(),
                fame: i32::MAX,
            },
        )],
    )
    .unwrap();
    assert!(reader
        .home_summary(HomeScope::Session {
            id: "demo-session".into()
        })
        .unwrap()
        .fame_delta
        .is_none());
}

#[test]
fn session_scope_counts_past_stale_last_traffic_and_includes_closed_end() {
    let (_dir, path, mut db) = database();
    apply_batch(
        &mut db,
        &[
            envelope(
                8,
                EngineEvent::FameChanged {
                    at: Timestamp(7_000_000),
                    character_id: "demo-character".into(),
                    fame: 1270,
                },
            ),
            envelope(
                9,
                EngineEvent::PlayerDied {
                    at: Timestamp(7_000_000),
                    character_id: None,
                    run_id: None,
                },
            ),
        ],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let scope = HomeScope::Session {
        id: "demo-session".into(),
    };
    let open = reader.home_summary(scope.clone()).unwrap();
    assert_eq!(open.fame.last().unwrap().at, Timestamp(7_000_000));
    assert_eq!(open.deaths, 1);
    let mut session = demo::demo_snapshot().session.unwrap();
    session.ended = Some(Timestamp(7_000_000));
    apply_batch(
        &mut db,
        &[envelope(10, EngineEvent::SessionUpdated(session.clone()))],
    )
    .unwrap();
    let closed = reader.home_summary(scope.clone()).unwrap();
    assert_eq!(closed.fame.len(), 3);
    assert_eq!(closed.deaths, 1);
    apply_batch(
        &mut db,
        &[
            envelope(
                11,
                EngineEvent::FameChanged {
                    at: Timestamp(7_000_001),
                    character_id: "demo-character".into(),
                    fame: 1280,
                },
            ),
            envelope(
                12,
                EngineEvent::PlayerDied {
                    at: Timestamp(7_000_001),
                    character_id: None,
                    run_id: None,
                },
            ),
        ],
    )
    .unwrap();
    let closed = reader.home_summary(scope.clone()).unwrap();
    assert_eq!(
        closed.fame.len(),
        4,
        "explicitly owned observations survive a corrected session end"
    );
    assert_eq!(closed.deaths, 2);
    session.ended = Some(Timestamp(8_000_000));
    apply_batch(
        &mut db,
        &[envelope(13, EngineEvent::SessionUpdated(session))],
    )
    .unwrap();
    let closed = reader.home_summary(scope).unwrap();
    assert_eq!(closed.fame.len(), 4);
    assert_eq!(closed.deaths, 2);
}

#[test]
fn closed_session_ending_at_its_start_keeps_explicit_summaries() {
    let (_dir, path, mut db) = database();
    let mut session = demo::demo_snapshot().session.unwrap();
    session.last_traffic = session.started;
    session.ended = Some(session.started);
    let mut drop = demo::demo_run_story().drops.remove(0);
    drop.bag_rank = Some(1);
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::SessionUpdated(session)),
            envelope(9, EngineEvent::DropUpdated(drop)),
            envelope(
                10,
                EngineEvent::PlayerDied {
                    at: Timestamp(1_000_000),
                    character_id: None,
                    run_id: None,
                },
            ),
        ],
    )
    .unwrap();
    let reader = open_reader(&path).unwrap();
    let home = reader
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap();
    assert_eq!(home.runs, 1);
    assert_eq!(home.recent_runs[0].id, "demo-run");
    assert!(home.top_fight.is_none());
    assert_eq!(home.best_drop.unwrap().id, "demo-drop");
    assert_eq!(home.fame.len(), 2);
    assert_eq!(home.deaths, 1);

    let missing = reader
        .home_summary(HomeScope::Session {
            id: "missing-session".into(),
        })
        .unwrap();
    assert_eq!((missing.runs, missing.clears, missing.deaths), (0, 0, 0));
    assert!(missing.fame.is_empty());
    assert!(missing.fame_delta.is_none());
    assert!(missing.recent_runs.is_empty());
    assert!(missing.top_fight.is_none());
    assert!(missing.best_drop.is_none());
    assert!(missing.issues.is_empty());
}

#[test]
fn list_keysets_filter_ties_and_null_durations() {
    let (_dir, path, mut db) = database();
    let base = demo::demo_run_story().run;
    let events: Vec<_> = (0..8)
        .map(|i| {
            let mut run = base.clone();
            run.id = format!("run-{i}");
            run.started = Timestamp(10_000_000 + (i / 2) * 100);
            run.ended = (i != 7).then_some(Timestamp(run.started.0 + (i % 3) * 10));
            run.dungeon = if i % 2 == 0 { "A" } else { "B" }.into();
            run.outcome = Some(if i % 2 == 0 {
                Outcome::Cleared
            } else {
                Outcome::Left
            });
            envelope(8 + i, EngineEvent::RunStarted(run))
        })
        .collect();
    apply_batch(&mut db, &events).unwrap();
    let all_runs: Vec<_> = std::iter::once(base)
        .chain(events.iter().filter_map(|event| match &event.event {
            EngineEvent::RunStarted(run) => Some(run.clone()),
            _ => None,
        }))
        .collect();
    let reader = open_reader(&path).unwrap();
    for sort in [
        RunSort::Newest,
        RunSort::Oldest,
        RunSort::DurationAscending,
        RunSort::DurationDescending,
    ] {
        for dungeon in [None, Some("A".to_owned()), Some("B".to_owned())] {
            for outcome in [None, Some(Outcome::Cleared), Some(Outcome::Left)] {
                for range in [
                    None,
                    Some(TimeRange {
                        start: Timestamp(10_000_100),
                        end: Timestamp(10_000_301),
                    }),
                ] {
                    let mut full = filter(sort.clone(), 100);
                    full.dungeon = dungeon.clone();
                    full.outcome = outcome.clone();
                    full.range = range;
                    let mut expected: Vec<_> = all_runs
                        .iter()
                        .filter(|run| {
                            full.dungeon.as_ref().is_none_or(|d| *d == run.dungeon)
                                && full
                                    .outcome
                                    .as_ref()
                                    .is_none_or(|o| Some(o) == run.outcome.as_ref())
                                && full
                                    .range
                                    .as_ref()
                                    .is_none_or(|r| run.started >= r.start && run.started < r.end)
                        })
                        .cloned()
                        .collect();
                    expected.sort_by(|a, b| {
                        let duration =
                            |r: &RunView| r.ended.and_then(|end| end.0.checked_sub(r.started.0));
                        match sort {
                            RunSort::Newest => (b.started, &b.id).cmp(&(a.started, &a.id)),
                            RunSort::Oldest => (a.started, &a.id).cmp(&(b.started, &b.id)),
                            RunSort::DurationAscending => (duration(a).unwrap_or(u64::MAX), &a.id)
                                .cmp(&(duration(b).unwrap_or(u64::MAX), &b.id)),
                            RunSort::DurationDescending => {
                                (duration(b), &b.id).cmp(&(duration(a), &a.id))
                            }
                        }
                    });
                    assert_eq!(reader.list_runs(full.clone()).unwrap().rows, expected);
                    for limit in [1, 50, 100] {
                        full.limit = limit;
                        full.cursor = None;
                        let mut actual = vec![];
                        loop {
                            let page = reader.list_runs(full.clone()).unwrap();
                            actual.extend(page.rows);
                            if page.next.is_none() {
                                break;
                            }
                            full.cursor = page.next;
                            assert!(actual.len() < 20);
                        }
                        assert_eq!(actual, expected);
                    }
                }
            }
        }
    }
    for limit in [0, 101, u16::MAX] {
        assert!(reader.list_runs(filter(RunSort::Newest, limit)).is_err());
    }
    let mut invalid = filter(RunSort::Newest, 30);
    invalid.cursor = Some(RunCursor {
        sort_value: "not-a-number".into(),
        id: "x".into(),
    });
    assert!(reader.list_runs(invalid).is_err());
}

#[test]
fn input_ranges_handle_short_and_long_dst_days_without_assuming_24_hours() {
    let (_dir, path, _db) = database();
    let reader = open_reader(&path).unwrap();
    for hours in [23, 25] {
        let summary = reader
            .home_summary(HomeScope::Today {
                range: TimeRange {
                    start: Timestamp(0),
                    end: Timestamp(hours * 60 * 60 * 1_000_000),
                },
            })
            .unwrap();
        assert_eq!(summary.runs, 1);
    }
    assert!(reader
        .home_summary(HomeScope::Today {
            range: TimeRange {
                start: Timestamp(2),
                end: Timestamp(1)
            }
        })
        .is_err());
}

#[test]
fn late_revision_adds_one_clear_and_comparison_requires_observed_completions() {
    let (_dir, path, mut db) = database();
    let mut current = demo::demo_run_story().run;
    current.id = "current".into();
    current.started = Timestamp(100_000_000);
    current.completed_at = None;
    current.outcome = Some(Outcome::Unknown);
    let mut sequence = 8;
    for i in 0..3 {
        let mut run = current.clone();
        run.id = format!("prior-{i}");
        run.started = Timestamp(20_000_000 + i * 10_000_000);
        run.completed_at = Some(Timestamp(run.started.0 + 10_000_000));
        run.outcome = Some(Outcome::Cleared);
        run.evidence = vec![EvidenceAt {
            at: run.completed_at.unwrap(),
            evidence: Evidence::Victory,
        }];
        apply_batch(&mut db, &[envelope(sequence, EngineEvent::RunStarted(run))]).unwrap();
        sequence += 1;
    }
    apply_batch(
        &mut db,
        &[envelope(sequence, EngineEvent::RunStarted(current.clone()))],
    )
    .unwrap();
    sequence += 1;
    let reader = open_reader(&path).unwrap();
    assert!(reader
        .get_run("current")
        .unwrap()
        .unwrap()
        .comparison
        .is_none());
    current.completed_at = Some(Timestamp(108_000_000));
    current.outcome = Some(Outcome::Cleared);
    current.evidence = vec![EvidenceAt {
        at: Timestamp(108_000_000),
        evidence: Evidence::CompletionCounter {
            before: 10,
            after: 11,
            delay_micros: 1,
        },
    }];
    apply_batch(
        &mut db,
        &[envelope(sequence, EngineEvent::RunUpdated(current.clone()))],
    )
    .unwrap();
    let story = reader.get_run("current").unwrap().unwrap();
    assert!(
        story.comparison.is_none(),
        "counter fallback cannot establish an observed clear duration"
    );
    assert_eq!(
        story
            .timeline
            .iter()
            .filter(|t| t.kind == TimelineKind::Completion)
            .count(),
        1
    );
    current.evidence.push(EvidenceAt {
        at: Timestamp(108_000_000),
        evidence: Evidence::Victory,
    });
    apply_batch(
        &mut db,
        &[envelope(sequence + 1, EngineEvent::RunUpdated(current))],
    )
    .unwrap();
    assert!(reader
        .get_run("current")
        .unwrap()
        .unwrap()
        .comparison
        .is_some());
}

#[test]
fn ten_thousand_runs_use_the_history_index_and_return_a_bounded_page() {
    let (_dir, path, mut db) = database();
    let base = demo::demo_run_story().run;
    let events: Vec<_> = (0..10_000)
        .map(|i| {
            let mut run = base.clone();
            run.id = format!("load-{i:05}");
            run.started = Timestamp(10_000_000 + i);
            envelope(8 + i, EngineEvent::RunStarted(run))
        })
        .collect();
    apply_batch(&mut db, &events).unwrap();
    let plan:String=db.query_row("EXPLAIN QUERY PLAN SELECT json FROM runs WHERE (started,id)<(?1,?2) ORDER BY started DESC,id DESC LIMIT 31",rusqlite::params![20_000_000,"z"],|r| r.get(3)).unwrap();
    assert!(plan.contains("runs_started"), "{plan}");
    for (sql,index) in [
        ("SELECT json FROM runs WHERE dungeon='Synthetic Sanctum' AND outcome='cleared' ORDER BY started,id LIMIT 31","runs_dungeon_outcome"),
        ("SELECT json FROM runs WHERE session_id='demo-session' ORDER BY started,id LIMIT 31","runs_session"),
        ("SELECT json FROM runs ORDER BY coalesce((CASE WHEN ended>=started THEN ended-started END),9223372036854775807),id LIMIT 31","runs_duration_ascending"),
        ("SELECT json FROM runs ORDER BY coalesce((CASE WHEN ended>=started THEN ended-started END),-1) DESC,id DESC LIMIT 31","runs_duration_descending"),
    ] {
        let details:String=db.query_row(&format!("EXPLAIN QUERY PLAN {sql}"),[],|r| r.get(3)).unwrap();
        assert!(details.contains(index),"{details}");
        eprintln!("query index: {details}");
    }
    let reader = open_reader(&path).unwrap();
    let before = std::time::Instant::now();
    let page = reader.list_runs(filter(RunSort::Newest, 30)).unwrap();
    eprintln!(
        "10,000-run history page: {:?}; plan: {plan}",
        before.elapsed()
    );
    assert_eq!(page.rows.len(), 30);
    assert!(page.next.is_some());
}

#[test]
fn home_prefers_highest_observed_local_dps_and_ranks_bags_without_inventing_identity() {
    let (_dir, path, mut db) = database();
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = "eligible".into();
    fight.first_hit = Some(fight.started);
    fight.last_hit = Some(Timestamp(fight.started.0 + 20_000_000));
    fight.local_active_micros = Some(20_000_000);
    fight.ended = fight.last_hit;
    fight.complete_capture = true;
    fight.issues.clear();
    fight.players[0].dps = Some(100.0);
    let mut estimated = fight.clone();
    estimated.id = "estimated".into();
    estimated.players[0].dps = Some(999.0);
    estimated.players[0].sources[0].estimated = true;
    let mut remote = fight.clone();
    remote.id = "remote".into();
    remote.players[0].is_local = false;
    remote.players[0].dps = Some(10000.0);
    let mut drop = demo::demo_run_story().drops.remove(0);
    drop.id = "ranked-old".into();
    drop.bag_rank = Some(5);
    let mut newer = drop.clone();
    newer.id = "ranked-new".into();
    newer.at = Timestamp(5_000_001);
    let mut lower = newer.clone();
    lower.id = "lower".into();
    lower.at = Timestamp(5_000_002);
    lower.bag_rank = Some(4);
    let events: Vec<_> = [
        EngineEvent::FightStarted(fight),
        EngineEvent::FightStarted(estimated),
        EngineEvent::FightStarted(remote),
        EngineEvent::BagDropped(drop),
        EngineEvent::BagDropped(newer),
        EngineEvent::BagDropped(lower),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, event)| envelope(8 + i as u64, event))
    .collect();
    apply_batch(&mut db, &events).unwrap();
    let home = open_reader(&path)
        .unwrap()
        .home_summary(HomeScope::Session {
            id: "demo-session".into(),
        })
        .unwrap();
    let top = home.top_fight.unwrap();
    assert_eq!(top.id, "estimated");
    assert!(top.players[0].sources[0].estimated);
    assert_eq!(home.best_drop.unwrap().id, "ranked-new");
}

#[test]
fn seven_day_counts_keep_boosted_bags_distinct_and_exclude_end_boundary() {
    let (_dir, path, mut db) = database();
    const DAY: u64 = 86_400_000_000;
    let events: Vec<_> = [(0, 400), (DAY, 400), (7 * DAY, 400), (DAY, 401)]
        .into_iter()
        .enumerate()
        .map(|(i, (at, bag_type))| {
            let mut drop = demo::demo_run_story().drops.remove(0);
            drop.id = format!("week-{i}");
            drop.at = Timestamp(at);
            drop.bag_type = bag_type;
            envelope(8 + i as u64, EngineEvent::BagDropped(drop))
        })
        .collect();
    apply_batch(&mut db, &events).unwrap();
    let reader = open_reader(&path).unwrap();
    let week = TimeRange {
        start: Timestamp(0),
        end: Timestamp(7 * DAY),
    };
    assert_eq!(reader.bag_count_since(400, week.clone()).unwrap(), 3); // Includes the original synthetic bag.
    assert_eq!(reader.bag_count_since(401, week).unwrap(), 1);
}
