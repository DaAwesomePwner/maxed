use crate::common;
use crate::common::account_scope_fixture::ACCOUNT;
use common::*;
use maxed_store::*;
use maxed_types::*;
use rusqlite::{params, Connection};

#[path = "home/active_window.rs"]
mod active_window;

fn database() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home.sqlite");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    crate::common::account_scope_fixture::apply_batch(&mut db, &story_events()).unwrap();
    (dir, path, db)
}

fn session() -> HomeScope {
    HomeScope::Session {
        id: "demo-session".into(),
    }
}

fn today(start: u64, end: u64) -> HomeScope {
    HomeScope::Today {
        range: TimeRange {
            start: Timestamp(start),
            end: Timestamp(end),
        },
    }
}

fn add(db: &mut Connection, events: Vec<EngineEvent>) {
    let head: i64 = common::receipts::head(db, "test-stream").unwrap();
    let events: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(i, e)| envelope(head as u64 + i as u64 + 1, e))
        .collect();
    crate::common::account_scope_fixture::apply_batch(db, &events).unwrap();
}

// Synthetic completed encounter eligible for Home's 20-second combat window.
fn eligible_fight(id: &str, started: u64) -> FightView {
    let mut fight = demo::demo_snapshot().fights.remove(0);
    fight.id = id.into();
    fight.started = Timestamp(started);
    fight.first_hit = Some(Timestamp(started));
    fight.last_hit = Some(Timestamp(started + 20_000_000));
    fight.local_active_micros = Some(20_000_000);
    fight.ended = fight.last_hit;
    fight.complete_capture = true;
    fight.issues.clear();
    fight.players[0].dps = Some(100.0);
    fight
}

#[test]
fn home_top_dps_requires_complete_ended_twenty_second_combat() {
    let (_dir, path, mut db) = database();
    let complete = eligible_fight("complete", 2_000_000);
    let mut candidates = vec![complete.clone()];
    for id in [
        "short-spike",
        "just-short",
        "open",
        "partial",
        "issues",
        "unknown-window",
        "unknown-dps",
    ] {
        let mut fight = complete.clone();
        fight.id = id.into();
        fight.players[0].dps = Some(9999.0);
        match id {
            // A long idle tail must not turn a ten-second burst into a best.
            "short-spike" => fight.local_active_micros = Some(10_000_000),
            "just-short" => fight.local_active_micros = Some(19_999_999),
            "open" => fight.ended = None,
            "partial" => fight.complete_capture = false,
            "issues" => fight.issues = vec![CoverageIssue::StreamGap],
            "unknown-window" => fight.local_active_micros = None,
            "unknown-dps" => fight.players[0].dps = None,
            _ => unreachable!(),
        }
        candidates.push(fight);
    }
    add(
        &mut db,
        candidates
            .into_iter()
            .map(EngineEvent::FightUpdated)
            .collect(),
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    for scope in [session(), today(0, 30_000_000)] {
        assert_eq!(
            reader.home_summary(scope, Some(ACCOUNT)).unwrap().top_fight,
            Some(complete.clone())
        );
    }
    let mut no_longer_eligible = complete;
    no_longer_eligible.complete_capture = false;
    add(&mut db, vec![EngineEvent::FightUpdated(no_longer_eligible)]);
    assert!(reader
        .home_summary(session(), Some(ACCOUNT))
        .unwrap()
        .top_fight
        .is_none());
}

#[test]
fn home_top_dps_excludes_legacy_basis_and_missing_basis_json() {
    for explicit in [false, true] {
        let (_dir, path, mut db) = database();
        let current = eligible_fight("current", 2_000_000);
        let mut legacy = eligible_fight("legacy", 2_000_000);
        legacy.dps_basis = DpsBasis::WholeTargetV0;
        legacy.players[0].dps = Some(9999.0);
        add(&mut db, vec![EngineEvent::FightUpdated(legacy)]);
        if !explicit {
            db.execute(
                "UPDATE fights SET json=json_remove(json,'$.dps_basis') WHERE id='legacy'",
                [],
            )
            .unwrap();
        }
        let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
        for scope in [session(), today(0, 30_000_000)] {
            assert!(reader
                .home_summary(scope, Some(ACCOUNT))
                .unwrap()
                .top_fight
                .is_none());
        }
        add(&mut db, vec![EngineEvent::FightUpdated(current.clone())]);
        for scope in [session(), today(0, 30_000_000)] {
            assert_eq!(
                reader.home_summary(scope, Some(ACCOUNT)).unwrap().top_fight,
                Some(current.clone())
            );
        }
    }
}

#[test]
fn home_top_dps_ties_use_latest_start_then_id() {
    let (_dir, path, mut db) = database();
    let fights = vec![
        eligible_fight("older-z", 1_000_000),
        eligible_fight("latest-a", 2_000_000),
        eligible_fight("latest-z", 2_000_000),
    ];
    add(
        &mut db,
        fights.into_iter().map(EngineEvent::FightUpdated).collect(),
    );
    assert_eq!(
        crate::common::account_scope_fixture::open_reader(&path)
            .unwrap()
            .home_summary(session(), Some(ACCOUNT))
            .unwrap()
            .top_fight
            .unwrap()
            .id,
        "latest-z"
    );
}

#[test]
fn home_json_decodes_fame_timestamps_and_overflowing_delta() {
    let (_dir, path, db) = database();
    db.execute("DELETE FROM fame_samples", []).unwrap();
    let at = 9_007_199_254_740_993_i64;
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    for (baseline, latest, expected) in [
        (i32::MIN, i32::MAX, None),
        (i32::MAX, i32::MIN, None),
        (0, i32::MAX, Some(i32::MAX)),
        (0, i32::MIN, Some(i32::MIN)),
    ] {
        db.execute("DELETE FROM fame_samples", []).unwrap();
        db.execute(
            "INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame) VALUES('fame',1,'demo-character',?1,?2)",
            params![at - 1, baseline],
        )
        .unwrap();
        db.execute(
            "INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame) VALUES('fame',2,'demo-character',?1,?2)",
            params![at, latest],
        )
        .unwrap();
        let home = reader
            .home_summary(today(at as u64 - 1, at as u64 + 1), Some(ACCOUNT))
            .unwrap();
        assert_eq!(home.fame_delta, expected);
        assert_eq!(home.fame[0].at, Timestamp(at as u64 - 1));
        assert_eq!(home.fame[1].at, Timestamp(at as u64));
        assert_eq!(home.fame[1].fame, latest);
        assert_eq!(home.fame_missing_baselines, 0);
        assert!(!home.fame_sampled);
    }
    // A lone sample is its own baseline.
    db.execute("DELETE FROM fame_samples WHERE sequence=1", [])
        .unwrap();
    let home = reader
        .home_summary(today(at as u64 - 1, at as u64 + 1), Some(ACCOUNT))
        .unwrap();
    assert_eq!(home.fame_delta, Some(0));
    assert_eq!(home.fame_missing_baselines, 0);
}

#[test]
fn scope_ranking_and_recent_projection() {
    let (_dir, path, mut db) = database();
    let story = demo::demo_run_story();
    let mut events = Vec::new();
    for i in 0..12 {
        let mut run = story.run.clone();
        run.id = format!("run-{i:02}");
        run.started = Timestamp(10_000_000 + i);
        events.push(EngineEvent::RunStarted(run));
    }
    for (id, at, rank) in [
        ("white-old", 11_000_000, Some(255)),
        ("lower-new", 12_000_000, Some(254)),
        ("unranked", 13_000_000, None),
        ("white-new-a", 14_000_000, Some(255)),
        ("white-new-z", 14_000_000, Some(255)),
    ] {
        let mut bag = story.drops[0].clone();
        bag.id = id.into();
        bag.at = Timestamp(at);
        bag.bag_rank = rank;
        bag.run_id = Some("run-11".into());
        bag.contents_known = false;
        bag.contents.clear();
        bag.items.clear();
        bag.issues = vec![CoverageIssue::MissingCatalog];
        events.push(EngineEvent::BagDropped(bag));
    }
    add(&mut db, events);
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let home = reader.home_summary(session(), Some(ACCOUNT)).unwrap();
    assert_eq!(home.runs, 13);
    let best = home.best_drop.unwrap();
    assert_eq!(best.id, "white-new-z");
    assert!(!best.contents_known);
    assert_eq!(home.unranked_drop_count, 2);
    assert_eq!(home.recent_runs.len(), 10);
    assert_eq!(home.recent_details.len(), 10);
    for (run, detail) in home.recent_runs.iter().zip(&home.recent_details) {
        assert_eq!(run.id, detail.run_id);
    }
    let detail = &home.recent_details[0];
    assert_eq!(detail.run_id, "run-11");
    assert_eq!(detail.observed_drop_count, 5);
    assert!(detail.loot_preview_limited);
    assert_eq!(
        detail
            .loot
            .iter()
            .map(|d| d.id.as_str())
            .collect::<Vec<_>>(),
        ["white-new-z", "white-new-a", "unranked"]
    );
    assert!(detail.issues.contains(&CoverageIssue::MissingCatalog));
    // Tile drops obey the day window; recent details preserve the whole selected run.
    let day = reader
        .home_summary(today(10_000_000, 11_000_000), Some(ACCOUNT))
        .unwrap();
    assert!(day.best_drop.is_none());
    assert_eq!(day.unranked_drop_count, 0);
    assert_eq!(day.recent_details[0].observed_drop_count, 5);
    assert!(day.issues.contains(&CoverageIssue::MissingCatalog));
    let old_white = reader
        .home_summary(today(11_000_000, 14_000_000), Some(ACCOUNT))
        .unwrap();
    assert_eq!(old_white.best_drop.unwrap().id, "white-old");
}

#[test]
fn weighted_share_requires_every_fights_known_unambiguous_local() {
    let (_dir, path, mut db) = database();
    let mut story = demo::demo_run_story();
    // Engine total_damage already includes unattributed damage; do not add it twice.
    assert_eq!(story.fights[0].total_damage, 300.0);
    story.run.issues.clear();
    story.run.complete_capture = true;
    let mut small = demo::demo_snapshot().fights.remove(0);
    small.issues.clear();
    small.complete_capture = true;
    small.total_damage = 100.0;
    small.unattributed_damage = 80.0;
    small.players[0].damage = 20.0;
    small.players[0].share = Some(0.2);
    let mut large = small.clone();
    large.id = "large".into();
    large.total_damage = 900.0;
    large.unattributed_damage = 180.0;
    large.players[0].damage = 720.0;
    large.players[0].share = Some(0.8);
    add(
        &mut db,
        vec![
            EngineEvent::RunUpdated(story.run),
            EngineEvent::FightUpdated(small),
            EngineEvent::FightUpdated(large.clone()),
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let detail = reader
        .home_summary(session(), Some(ACCOUNT))
        .unwrap()
        .recent_details
        .remove(0);
    assert_eq!(detail.local_share, Some(0.74));
    assert!(!detail.share_partial);
    large.complete_capture = false;
    large.issues = vec![CoverageIssue::StreamGap];
    large.players[0].sources[0].estimated = true;
    add(&mut db, vec![EngineEvent::FightUpdated(large.clone())]);
    let detail = reader
        .home_summary(session(), Some(ACCOUNT))
        .unwrap()
        .recent_details
        .remove(0);
    assert_eq!(detail.local_share, Some(0.74));
    assert!(detail.share_partial);
    assert!(detail.issues.contains(&CoverageIssue::StreamGap));
    for variant in 0..7 {
        let mut invalid = large.clone();
        match variant {
            0 => invalid.players[0].share = None,
            1 => invalid.players[0].is_local = false,
            2 => {
                let mut p = invalid.players[0].clone();
                p.player_id = "ambiguous".into();
                invalid.players.push(p);
            }
            3 => invalid.total_damage = 0.0,
            4 => invalid.issues.push(CoverageIssue::MissingIdentity),
            5 => invalid.players[0].damage = -1.0,
            _ => invalid.total_damage = 1.0,
        }
        add(&mut db, vec![EngineEvent::FightUpdated(invalid)]);
        assert_eq!(
            reader
                .home_summary(session(), Some(ACCOUNT))
                .unwrap()
                .recent_details[0]
                .local_share,
            None,
            "variant {variant}"
        );
    }
    // Known local zero is distinct from absent identity/share.
    large.players[0].damage = 0.0;
    large.players[0].share = Some(0.0);
    add(&mut db, vec![EngineEvent::FightUpdated(large)]);
    assert_eq!(
        reader
            .home_summary(session(), Some(ACCOUNT))
            .unwrap()
            .recent_details[0]
            .local_share,
        Some(0.02)
    );
}

#[test]
fn unknown_sessions_and_invalid_scopes_never_borrow_nearby_events() {
    let (_dir, path, mut db) = database();
    add(
        &mut db,
        vec![EngineEvent::PlayerDied {
            at: Timestamp(0),
            character_id: None,
            run_id: None,
        }],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let empty = reader
        .home_summary(
            HomeScope::Session {
                id: "absent".into(),
            },
            Some(ACCOUNT),
        )
        .unwrap();
    assert_eq!(
        (empty.runs, empty.deaths, empty.unranked_drop_count),
        (0, 0, 0)
    );
    assert!(empty.fame.is_empty() && empty.recent_details.is_empty() && empty.issues.is_empty());
    assert!(empty.fame_delta.is_none() && empty.top_fight.is_none() && empty.best_drop.is_none());
    for scope in [
        HomeScope::Session { id: "".into() },
        HomeScope::Session {
            id: "x".repeat(257),
        },
        today(1, 1),
        today(2, 1),
        today(0, u64::MAX),
    ] {
        assert!(matches!(
            reader.home_summary(scope, Some(ACCOUNT)),
            Err(StoreError::InvalidEvent)
        ));
    }
    assert!(reader
        .home_summary(
            HomeScope::Session {
                id: "x".repeat(256)
            },
            Some(ACCOUNT)
        )
        .is_ok());
}

#[test]
fn session_ids_exclude_other_sessions_at_the_same_time() {
    let (_dir, path, mut db) = database();
    let mut session_view = demo::demo_snapshot().session.unwrap();
    session_view.id = "other-session".into();
    let mut story = demo::demo_run_story();
    story.run.id = "other-run".into();
    story.run.session_id = session_view.id.clone();
    let mut fight = eligible_fight("other-fight", 2_000_000);
    fight.run_id = Some(story.run.id.clone());
    fight.session_id = session_view.id.clone();
    fight.players[0].dps = Some(9999.0);
    story.drops[0].id = "other-bag".into();
    story.drops[0].run_id = Some(story.run.id.clone());
    story.drops[0].session_id = session_view.id.clone();
    story.drops[0].bag_rank = Some(255);
    add(
        &mut db,
        vec![
            EngineEvent::SessionUpdated(session_view),
            EngineEvent::RunStarted(story.run),
            EngineEvent::FightStarted(fight),
            EngineEvent::BagDropped(story.drops.remove(0)),
            EngineEvent::PlayerDied {
                at: Timestamp(5_000_000),
                character_id: None,
                run_id: Some("other-run".into()),
            },
        ],
    );
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let home = reader.home_summary(session(), Some(ACCOUNT)).unwrap();
    assert_eq!((home.runs, home.deaths), (1, 0));
    assert!(home.top_fight.is_none());
    assert!(home.best_drop.is_none());
    let day = reader
        .home_summary(today(0, 6_000_000), Some(ACCOUNT))
        .unwrap();
    assert_eq!((day.runs, day.deaths), (2, 1));
    assert_eq!(day.top_fight.unwrap().id, "other-fight");
    assert_eq!(day.best_drop.unwrap().id, "other-bag");
}

#[test]
fn fame_ties_use_stream_then_sequence_for_chart_and_baseline() {
    let (_dir, path, db) = database();
    db.execute("DELETE FROM fame_samples", []).unwrap();
    for (stream, sequence, at, fame) in [
        ("z", 2, 0, 20),
        ("a", 9, 0, 90),
        ("z", 1, 0, 10),
        ("z", 2, 1, 24),
        ("a", 9, 1, 91),
        ("z", 1, 1, 13),
    ] {
        // Distinct sequence pairs within each stream across the baseline and scope.
        db.execute(
            "INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame) VALUES(?1,?2,'demo-character',?3,?4)",
            params![stream, sequence + at * 100, at, fame],
        )
        .unwrap();
    }
    let home = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .home_summary(today(1, 2), Some(ACCOUNT))
        .unwrap();
    // The baseline is the first point in the chart's tie order, 91, not a pre-scope sample.
    assert_eq!(home.fame_delta, Some(24 - 91));
    assert_eq!(
        home.fame.iter().map(|p| p.fame).collect::<Vec<_>>(),
        [91, 13, 24]
    );
}

#[test]
fn late_revision_updates_one_recent_run() {
    let (_dir, path, mut db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let mut run = demo::demo_run_story().run;
    run.outcome = Some(Outcome::Left);
    run.completed_at = None;
    add(&mut db, vec![EngineEvent::RunUpdated(run.clone())]);
    assert_eq!(
        reader
            .home_summary(session(), Some(ACCOUNT))
            .unwrap()
            .clears,
        0
    );
    run.completed_at = Some(Timestamp(5_000_000));
    run.outcome = Some(Outcome::Cleared);
    run.died_at = Some(Timestamp(5_500_000));
    add(
        &mut db,
        vec![
            EngineEvent::RunUpdated(run.clone()),
            EngineEvent::PlayerDied {
                at: Timestamp(5_500_000),
                character_id: run.character_id.clone(),
                run_id: Some(run.id.clone()),
            },
        ],
    );
    drop(reader);
    let home = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .home_summary(session(), Some(ACCOUNT))
        .unwrap();
    assert_eq!((home.runs, home.clears, home.deaths), (1, 1, 1));
    assert_eq!(home.recent_runs, vec![run]);
    assert_eq!(home.recent_details.len(), 1);
}

#[test]
fn fame_sampling_keeps_endpoints_exact_delta_and_character_identity() {
    let (_dir, path, mut db) = database();
    let mut other = demo::demo_snapshot().character.unwrap();
    other.id = "second".into();
    add(&mut db, vec![EngineEvent::CharacterUpdated(other)]);
    db.execute("DELETE FROM fame_samples", []).unwrap();
    db.execute(
        "INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame) VALUES('chart',1,'demo-character',0,0)",
        [],
    )
    .unwrap();
    db.execute_batch("BEGIN").unwrap();
    for i in 1..=301 {
        let character = if i % 2 == 0 {
            "second"
        } else {
            "demo-character"
        };
        db.execute(
            "INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame) VALUES('chart',?1,?2,?3,?3)",
            params![i + 1, character, i],
        )
        .unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let home = reader.home_summary(today(1, 302), Some(ACCOUNT)).unwrap();
    assert_eq!(home.fame.len(), 301);
    assert!(!home.fame_sampled);
    // Each character counts from its first in-scope sample: (301-1) + (300-2).
    assert_eq!(home.fame_delta, Some(598));
    assert_eq!(home.fame_missing_baselines, 0);
    assert_eq!(home.fame.first().unwrap().at, Timestamp(1));
    assert_eq!(home.fame.last().unwrap().at, Timestamp(301));
    assert!(home.fame.windows(2).all(|p| p[0].at.0 < p[1].at.0));
    for point in &home.fame {
        assert_eq!(
            point.character_id,
            if point.at.0 % 2 == 0 {
                "second"
            } else {
                "demo-character"
            }
        );
    }
    let exact = reader.home_summary(today(1, 121), Some(ACCOUNT)).unwrap();
    assert_eq!(exact.fame.len(), 120);
    assert!(!exact.fame_sampled);
    // Two individually representable deltas can overflow their aggregate.
    db.execute("DELETE FROM fame_samples", []).unwrap();
    for (seq, character) in [(1, "demo-character"), (3, "second")] {
        db.execute(
            "INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame) VALUES('sum',?1,?2,0,0)",
            params![seq, character],
        )
        .unwrap();
        db.execute(
            "INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame) VALUES('sum',?1,?2,1,2147483647)",
            params![seq + 1, character],
        )
        .unwrap();
    }
    let overflow = reader.home_summary(today(0, 2), Some(ACCOUNT)).unwrap();
    assert_eq!(overflow.fame_delta, None);
    assert_eq!(overflow.fame.len(), 4);
}

#[test]
fn corrupt_and_oversized_home_rows_fail_instead_of_returning_empty_history() {
    let (_dir, path, db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let mut run = demo::demo_run_story().run;
    run.dungeon = "x".repeat(2 * 1024 * 1024);
    db.execute(
        "UPDATE runs SET json=?1",
        [serde_json::to_string(&run).unwrap()],
    )
    .unwrap();
    assert!(matches!(
        reader.home_summary(session(), Some(ACCOUNT)),
        Err(StoreError::InvalidEvent)
    ));
    for corrupt in ["{", "{}", "null"] {
        db.execute("UPDATE runs SET json=?1", [corrupt]).unwrap();
        assert!(
            matches!(
                reader.home_summary(session(), Some(ACCOUNT)),
                Err(StoreError::InvalidEvent)
            ),
            "{corrupt}"
        );
    }
}

#[test]
fn local_day_bounds_and_session_membership_remain_independent() {
    // Caller-provided Chicago spring/fall day lengths: store never adds 24 hours.
    for hours in [23, 25] {
        let (_dir, path, mut db) = database();
        let lo = 20_000_000;
        let hi = lo + hours * 3_600_000_000;
        let story = demo::demo_run_story();
        let mut events = Vec::new();
        for (id, at) in [
            ("before", lo - 1),
            ("start", lo),
            ("last", hi - 1),
            ("end", hi),
        ] {
            let mut run = story.run.clone();
            run.id = id.into();
            run.started = Timestamp(at);
            let mut fight = eligible_fight(id, at);
            fight.run_id = Some(id.into());
            fight.started = Timestamp(at);
            fight.players[0].dps = Some(at as f64);
            let mut bag = story.drops[0].clone();
            bag.id = id.into();
            bag.run_id = Some(id.into());
            bag.at = Timestamp(at);
            bag.bag_rank = Some(1);
            events.extend([
                EngineEvent::RunStarted(run),
                EngineEvent::FightStarted(fight),
                EngineEvent::BagDropped(bag),
                EngineEvent::PlayerDied {
                    at: Timestamp(at),
                    character_id: None,
                    run_id: Some(id.into()),
                },
                EngineEvent::FameChanged {
                    at: Timestamp(at),
                    character_id: "demo-character".into(),
                    fame: 1300,
                },
            ]);
        }
        add(&mut db, events);
        let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
        let day = reader.home_summary(today(lo, hi), Some(ACCOUNT)).unwrap();
        assert_eq!((day.runs, day.deaths), (2, 2));
        assert_eq!(day.best_drop.unwrap().id, "last");
        assert_eq!(day.top_fight.unwrap().id, "last");
        assert_eq!(
            day.fame.iter().map(|p| p.at.0).collect::<Vec<_>>(),
            [lo, hi - 1]
        );
        let all = reader.home_summary(session(), Some(ACCOUNT)).unwrap();
        assert_eq!((all.runs, all.deaths), (5, 4));
        // Run-associated deaths use the explicit session even after its closed end.
        let mut ended = demo::demo_snapshot().session.unwrap();
        ended.ended = Some(Timestamp(lo));
        add(&mut db, vec![EngineEvent::SessionUpdated(ended)]);
        let closed = reader.home_summary(session(), Some(ACCOUNT)).unwrap();
        assert_eq!((closed.runs, closed.deaths), (5, 4));
        assert_eq!(closed.fame.last().unwrap().at, Timestamp(hi));
    }
}

const BENCH_DAY: i64 = 86_400_000_000;
const BENCH_START: i64 = 100 * BENCH_DAY;

/// Both shapes have exactly 10k runs/20k fights/40k drops/100k fame samples.
/// Older history belongs to another session and lies strictly before the day.
fn benchmark_database(
    scoped_runs: i64,
    scoped_fame: i64,
) -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("home-benchmark.sqlite");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let story = demo::demo_run_story();
    let tx = db.transaction().unwrap();
    let character = story.character.as_ref().unwrap();
    tx.execute(
        "INSERT INTO characters(id,observed_at,json) VALUES(?1,?2,?3)",
        params![
            character.id,
            character.observed_at.0 as i64,
            serde_json::to_string(character).unwrap(),
        ],
    )
    .unwrap();
    for (id, start, end) in [
        ("archive-session", 0, BENCH_START - 1),
        ("demo-session", BENCH_START, BENCH_START + BENCH_DAY - 1),
    ] {
        let view = PlaySessionView {
            issues: vec![],
            id: id.into(),
            started: Timestamp(start as u64),
            last_traffic: Timestamp(end as u64),
            ended: Some(Timestamp(end as u64)),
        };
        tx.execute(
            "INSERT INTO play_sessions VALUES(?1,?2,?3,?3,?4)",
            params![id, start, end, serde_json::to_string(&view).unwrap()],
        )
        .unwrap();
    }
    // Preserve the synthetic DTO sizes, updating IDs and time/session membership.
    tx.execute(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<10000),
        timed AS (SELECT i,
            CASE WHEN i<=10000-?2 THEN 'archive-session' ELSE 'demo-session' END AS session_id,
            CASE WHEN i<=10000-?2 THEN ?3-(10000-?2-i+1)*(?4/300)
                ELSE ?3+(i-(10000-?2)-1)*(?4/?2) END AS at FROM n)
        INSERT INTO runs SELECT 'load-'||printf('%05d',i),session_id,'demo-character',
        'Synthetic Sanctum',at,at+1000000,at+1000000,NULL,'cleared',0,NULL,
        json_set(?1,'$.id','load-'||printf('%05d',i),'$.session_id',session_id,
            '$.started',CAST(at AS TEXT),'$.ended',CAST(at+1000000 AS TEXT),
            '$.completed_at',CAST(at+1000000 AS TEXT)) FROM timed",
        params![
            serde_json::to_string(&story.run).unwrap(),
            scoped_runs,
            BENCH_START,
            BENCH_DAY
        ],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO fights SELECT r.id||'-'||n.i,r.id,r.session_id,r.started,r.ended,300,150,0,
        json_set(?1,'$.id',r.id||'-'||n.i,'$.run_id',r.id,'$.session_id',r.session_id,
            '$.started',CAST(r.started AS TEXT),'$.ended',CAST(r.ended AS TEXT))
        FROM runs r CROSS JOIN (SELECT 1 i UNION ALL SELECT 2) n",
        [serde_json::to_string(&demo::demo_snapshot().fights[0]).unwrap()],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO fight_players SELECT id,'demo-character',1,300,150,1,?1 FROM fights",
        [serde_json::to_string(&demo::demo_snapshot().fights[0].players[0]).unwrap()],
    )
    .unwrap();
    tx.execute("INSERT INTO drops(id,session_id,run_id,at,bag_type,bag_rank,json) SELECT r.id||'-'||n.i,r.session_id,r.id,r.started,400,NULL,
        json_set(?1,'$.id',r.id||'-'||n.i,'$.run_id',r.id,'$.session_id',r.session_id,
            '$.at',CAST(r.started AS TEXT))
        FROM runs r CROSS JOIN (SELECT 1 i UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4) n",
        [serde_json::to_string(&story.drops[0]).unwrap()]).unwrap();
    tx.execute(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<100000)
        INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame,session_id) SELECT 'load',i,'demo-character',
            CASE WHEN i<=100000-?1 THEN ?2-(100000-?1-i+1)*(?3/3000)
                ELSE ?2+(i-(100000-?1)-1)*(?3/?1) END,1250+i,CASE WHEN i<=100000-?1 THEN 'archive-session' ELSE 'demo-session' END FROM n",
        params![scoped_fame, BENCH_START, BENCH_DAY],
    )
    .unwrap();
    tx.commit().unwrap();
    // Pin both total and selected cardinalities outside the timed reader calls.
    let totals: (i64, i64, i64, i64) = db
        .query_row(
            "SELECT (SELECT count(*) FROM runs),(SELECT count(*) FROM fights),
            (SELECT count(*) FROM drops),(SELECT count(*) FROM fame_samples)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(totals, (10_000, 20_000, 40_000, 100_000));
    for kind in [0, 1] {
        let counts: (i64, i64, i64, i64) = db.query_row(
            "SELECT
            (SELECT count(*) FROM runs WHERE (?1=0 AND session_id='demo-session') OR (?1=1 AND started>=?2 AND started<?3)),
            (SELECT count(*) FROM fights WHERE (?1=0 AND session_id='demo-session') OR (?1=1 AND started>=?2 AND started<?3)),
            (SELECT count(*) FROM drops WHERE (?1=0 AND session_id='demo-session') OR (?1=1 AND at>=?2 AND at<?3)),
            (SELECT count(*) FROM fame_samples WHERE at>=?2 AND at<?3)",
            params![kind, BENCH_START, BENCH_START + BENCH_DAY],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ).unwrap();
        assert_eq!(
            counts,
            (scoped_runs, scoped_runs * 2, scoped_runs * 4, scoped_fame)
        );
    }
    (dir, path, db)
}

/// Explicit opt-in; run benchmarks serially to avoid measuring their contention.
#[test]
#[ignore = "large synthetic history measurement; run with --ignored --nocapture"]
fn large_history_query_plans_and_thirty_warm_reads() {
    benchmark_home("worst-case", 10_000, 100_000, 1500);
}

#[test]
#[ignore = "realistic synthetic history measurement; run with --ignored --nocapture"]
fn realistic_history_query_plans_and_thirty_warm_reads() {
    benchmark_home("realistic", 300, 3_000, 50);
}

fn benchmark_home(shape: &str, scoped_runs: i64, scoped_fame: i64, release_limit_ms: u64) {
    let (_dir, path, db) = benchmark_database(scoped_runs, scoped_fame);
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let setup = std::time::Instant::now();
    drop(crate::common::account_scope_fixture::open_reader(&path).unwrap());
    eprintln!("Reader open/configure/schema: {:?}", setup.elapsed());
    for (kind, scope) in [
        (0, session()),
        (
            1,
            today(BENCH_START as u64, (BENCH_START + BENCH_DAY) as u64),
        ),
    ] {
        let mut explain = db
            .prepare(&format!(
                "EXPLAIN QUERY PLAN {}",
                include_str!("../src/home.sql")
                    .replace("/*home_scope*/", include_str!("../src/home_scope.sql"))
                    .replace(
                        "/*dungeon_stats*/",
                        include_str!("../src/dungeon_stats.sql")
                    )
            ))
            .unwrap();
        let plan: Vec<String> = explain
            .query_map(
                params![
                    kind,
                    "demo-session",
                    BENCH_START,
                    BENCH_START + BENCH_DAY,
                    ACCOUNT
                ],
                |r| r.get(3),
            )
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        eprintln!(
            "Home shape={shape} kind={kind} query plan:\n{}",
            plan.join("\n")
        );
        assert!(plan.iter().any(|s| s.contains("fights_run")));
        assert!(plan.iter().any(|s| s.contains("drops_run")));
        assert!(plan.iter().any(|s| s.contains("fame_character")));
        reader.home_summary(scope.clone(), Some(ACCOUNT)).unwrap();
        let mut durations = Vec::new();
        for _ in 0..30 {
            let before = std::time::Instant::now();
            let home = reader.home_summary(scope.clone(), Some(ACCOUNT)).unwrap();
            durations.push(before.elapsed());
            assert_eq!(home.runs, scoped_runs as u32);
            assert_eq!(home.recent_runs.len(), 10);
            assert_eq!(home.recent_details.len(), 10);
            assert_eq!(home.fame.len(), 120);
            assert!(home.fame_sampled);
            assert!(home
                .recent_details
                .iter()
                .all(|d| d.loot.len() == 3 && d.observed_drop_count == 4));
        }
        durations.sort();
        eprintln!(
            "Home shape={shape} kind={kind}: 30 warm reads, median={:?}, p95={:?}, max={:?}",
            durations[14], durations[28], durations[29]
        );
        if !cfg!(debug_assertions) {
            let limit = std::time::Duration::from_millis(release_limit_ms);
            assert!(
                durations[28] < limit,
                "{shape} kind={kind} release p95 {:?} must be < {limit:?}",
                durations[28]
            );
        }
    }
}

#[test]
fn saved_session_coverage_survives_catalog_repair_without_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("coverage");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let mut view = demo::demo_snapshot().session.unwrap();
    view.issues.push(CoverageIssue::MissingCatalog);
    crate::common::account_scope_fixture::apply_batch(
        &mut db,
        &[envelope(1, EngineEvent::SessionUpdated(view.clone()))],
    )
    .unwrap();
    view.issues.clear();
    crate::common::account_scope_fixture::apply_batch(
        &mut db,
        &[envelope(2, EngineEvent::SessionUpdated(view))],
    )
    .unwrap();
    drop(db);
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    for scope in [session(), today(0, 10_000_000)] {
        let home = reader.home_summary(scope, Some(ACCOUNT)).unwrap();
        assert_eq!((home.runs, home.clears), (0, 0));
        assert_eq!(home.issues, vec![CoverageIssue::MissingCatalog]);
    }
    assert!(reader
        .home_summary(today(10_000_000, 20_000_000), Some(ACCOUNT))
        .unwrap()
        .issues
        .is_empty());
}

#[test]
fn future_dated_observations_from_old_stream_do_not_leak_into_new_session() {
    let (_dir, path, mut db) = database();
    add(
        &mut db,
        vec![
            EngineEvent::FameChanged {
                at: Timestamp(90_000_000),
                character_id: "demo-character".into(),
                fame: 9999,
            },
            EngineEvent::PlayerDied {
                at: Timestamp(90_000_000),
                character_id: None,
                run_id: None,
            },
        ],
    );
    let mut view = demo::demo_snapshot().session.unwrap();
    view.id = "new-session".into();
    view.started = Timestamp(10_000_000);
    view.last_traffic = view.started;
    view.ended = None;
    let mut event = envelope(1, EngineEvent::SessionUpdated(view));
    event.stream_id = "new-stream".into();
    crate::common::account_scope_fixture::apply_batch(&mut db, &[event]).unwrap();
    drop(db);
    let mut db = Connection::open(&path).unwrap();
    let mut event = envelope(
        2,
        EngineEvent::FameChanged {
            at: Timestamp(11_000_000),
            character_id: "demo-character".into(),
            fame: 1300,
        },
    );
    event.stream_id = "new-stream".into();
    crate::common::account_scope_fixture::apply_batch(&mut db, &[event]).unwrap();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let home = reader
        .home_summary(
            HomeScope::Session {
                id: "new-session".into(),
            },
            Some(ACCOUNT),
        )
        .unwrap();
    assert_eq!(home.deaths, 0);
    assert_eq!(home.fame.len(), 1);
    assert_eq!(home.fame[0].fame, 1300);
    // Neither the old session's 1260 nor its future-dated 9999 is a baseline.
    assert_eq!(home.fame_delta, Some(0));
    assert_eq!(
        reader
            .home_summary(session(), Some(ACCOUNT))
            .unwrap()
            .deaths,
        1
    );
    assert_eq!(
        reader
            .home_summary(today(89_000_000, 91_000_000), Some(ACCOUNT))
            .unwrap()
            .deaths,
        1
    );
}

#[test]
fn fame_sampling_preserves_short_character_segment_and_all_boundaries_under_cap() {
    let (_dir, path, mut db) = database();
    let mut other = demo::demo_snapshot().character.unwrap();
    other.id = "B".into();
    add(&mut db, vec![EngineEvent::CharacterUpdated(other)]);
    db.execute("DELETE FROM fame_samples", []).unwrap();
    db.execute_batch("BEGIN").unwrap();
    for i in 1..=600 {
        let character = if (302..=303).contains(&i) {
            "B"
        } else {
            "demo-character"
        };
        db.execute(
            "INSERT INTO fame_samples VALUES('sample',?1,?2,?1,?1,'demo-session')",
            params![i, character],
        )
        .unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    let home = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .home_summary(session(), Some(ACCOUNT))
        .unwrap();
    assert!(home.fame.len() <= 120);
    assert!(home.fame_sampled);
    for boundary in [1, 301, 302, 303, 304, 600] {
        assert!(
            home.fame.iter().any(|p| p.at == Timestamp(boundary)),
            "missing boundary {boundary}"
        );
    }
    assert_eq!(
        home.fame.iter().filter(|p| p.character_id == "B").count(),
        2
    );
}

#[test]
fn fame_endpoint_overflow_drops_oldest_whole_segments() {
    let (_dir, path, mut db) = database();
    let mut other = demo::demo_snapshot().character.unwrap();
    other.id = "B".into();
    add(&mut db, vec![EngineEvent::CharacterUpdated(other)]);
    db.execute("DELETE FROM fame_samples", []).unwrap();
    // 300 segments of three points: 600 mandatory endpoints. Keep newest 256.
    db.execute_batch("BEGIN").unwrap();
    for i in 0..900 {
        let character = if i / 3 % 2 == 0 {
            "B"
        } else {
            "demo-character"
        };
        db.execute(
            "INSERT INTO fame_samples VALUES('overflow',?1,?2,?1,?1,'demo-session')",
            params![i, character],
        )
        .unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    let home = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .home_summary(session(), Some(ACCOUNT))
        .unwrap();
    assert_eq!(home.fame.len(), 512);
    assert_eq!(home.fame.first().unwrap().at, Timestamp(132));
    assert_eq!(home.fame.last().unwrap().at, Timestamp(899));
    assert!(home.fame_sampled);
    // Per-character totals ignore chart sampling: both start at their first scoped sample.
    let totals = |id: &str, first_fame, last_fame| CharacterFame {
        character_id: id.into(),
        first_fame,
        last_fame,
        last_at: Timestamp(last_fame as u64),
    };
    assert_eq!(
        home.fame_characters,
        vec![totals("B", 0, 896), totals("demo-character", 3, 899)]
    );
    for pair in home.fame.as_chunks::<2>().0 {
        assert_eq!(pair[0].character_id, pair[1].character_id);
        assert_eq!(pair[1].at.0 - pair[0].at.0, 2);
    }
}

#[test]
fn single_character_fame_sampling_keeps_exact_budget_and_endpoints() {
    let (_dir, path, db) = database();
    db.execute("DELETE FROM fame_samples", []).unwrap();
    db.execute_batch("BEGIN").unwrap();
    for i in 1..=600 {
        db.execute(
            "INSERT INTO fame_samples VALUES('single',?1,'demo-character',?1,?1,'demo-session')",
            [i],
        )
        .unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    let home = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .home_summary(session(), Some(ACCOUNT))
        .unwrap();
    assert_eq!(home.fame.len(), 120);
    assert!(home.fame_sampled);
    assert_eq!(home.fame.first().unwrap().at, Timestamp(1));
    assert_eq!(home.fame.last().unwrap().at, Timestamp(600));
    assert!(home.fame.windows(2).all(|pair| pair[0].at < pair[1].at));
    assert!(home
        .fame
        .iter()
        .all(|point| point.character_id == "demo-character" && point.fame as u64 == point.at.0));
}
