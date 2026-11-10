//! Synthetic Runs list contracts; no captured identities or traffic.
use crate::common;
use crate::common::account_scope_fixture::ACCOUNT;
use common::*;
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;
use serde_json::{json, Value};

#[path = "common/runs_audit.rs"]
mod runs_audit;

#[test]
fn scalar_share_query_is_byte_identical_across_multiple_runs() {
    for players in [30, 360] {
        let mut db = Connection::open_in_memory().unwrap();
        let ids = runs_audit::seed(&mut db, 48, players);
        // Old-style shares must describe the same damage/total as the new reader.
        // Stale legacy values are covered by every_share_reader_ignores_legacy_player_share.
        db.execute_batch("UPDATE fight_players SET json=json_set(json,'$.share',
            damage/(SELECT json_extract(f.json,'$.total_damage') FROM fights f WHERE f.id=fight_id));").unwrap();
        let legacy = runs_audit::query(true);
        let current = runs_audit::query(false);
        assert_eq!(
            runs_audit::read(&db, &current, &ids),
            runs_audit::read(&db, &legacy, &ids)
        );
        // Positive control: preserve non-null, weighted shares as well as unknowns.
        db.execute_batch("UPDATE fights SET json=json_set(json,'$.issues',json('[]'),'$.total_damage',1000.0);
            UPDATE fight_players SET json=json_set(json,'$.is_local',json(CASE WHEN object_id=0 THEN 'true' ELSE 'false' END),'$.share',0.1);").unwrap();
        let actual = runs_audit::read(&db, &current, &ids);
        assert_eq!(actual, runs_audit::read(&db, &legacy, &ids));
        if players == 360 {
            let rows: Vec<RunRowDetail> = serde_json::from_str(&actual).unwrap();
            assert!(rows.iter().all(|row| row.local_share == Some(0.1)));
        }
    }
}

#[test]
fn run_identity_backfill_and_legacy_fallback_preserve_entry_observations() {
    for mode in ["update", "merge", "legacy"] {
        for known in ["neither", "name", "class"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("synthetic.sqlite");
            let mut db = Connection::open(&path).unwrap();
            migrate(&mut db).unwrap();
            let mut entry = demo::demo_run_story().character.unwrap();
            if known != "name" {
                entry.name = None;
            }
            if known != "class" {
                entry.class_id = None;
            }
            let mut events = story_events();
            events[1].event = EngineEvent::CharacterUpdated(entry.clone());
            crate::common::account_scope_fixture::apply_batch(&mut db, &events).unwrap();
            let mut later = entry.clone();
            later.name = Some("Synthetic Mage".into());
            later.class_id = Some(777);
            later.observed_at.0 += 1;
            later.equipment.clear();
            later.stats.clear();
            if mode == "merge" {
                later.id = "synthetic-verified".into();
            }
            crate::common::account_scope_fixture::apply_batch(
                &mut db,
                &[envelope(8, EngineEvent::CharacterUpdated(later.clone()))],
            )
            .unwrap();
            if mode == "merge" {
                crate::common::account_scope_fixture::apply_batch(
                    &mut db,
                    &[envelope(
                        9,
                        EngineEvent::CharacterMerged {
                            provisional_id: entry.id.clone(),
                            verified_id: later.id.clone(),
                        },
                    )],
                )
                .unwrap();
                entry.id = later.id.clone();
            }
            if mode == "legacy" {
                db.execute(
                    "UPDATE runs SET entry_character_json=?1 WHERE id='demo-run'",
                    [serde_json::to_string(&entry).unwrap()],
                )
                .unwrap();
            }
            let expected_name = entry.name.clone().or(later.name);
            let expected_class = entry.class_id.or(later.class_id);
            let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
            let details = reader
                .list_runs(filter(RunSort::Newest, 100))
                .unwrap()
                .details
                .unwrap();
            assert_eq!(details[0].character_name, expected_name, "{mode}/{known}");
            assert_eq!(details[0].class_id, expected_class, "{mode}/{known}");
            if mode != "legacy" {
                let stored: String = db
                    .query_row(
                        "SELECT entry_character_json FROM runs WHERE id='demo-run'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                let stored: CharacterView = serde_json::from_str(&stored).unwrap();
                assert_eq!(stored.name, expected_name);
                assert_eq!(stored.class_id, expected_class);
            }
            entry.name = expected_name;
            entry.class_id = expected_class;
            // Ownership follows the relational column even for a frozen legacy entry.
            entry.account_id = Some(crate::common::account_scope_fixture::ACCOUNT.into());
            assert_eq!(
                reader.get_run("demo-run").unwrap().unwrap().character,
                Some(entry)
            );
            // Even rows with no entry snapshot can display identity in the list,
            // without fabricating historical gear or stats for the story.
            db.execute("UPDATE runs SET entry_character_json=NULL", [])
                .unwrap();
            let details = reader
                .list_runs(filter(RunSort::Newest, 100))
                .unwrap()
                .details
                .unwrap();
            assert_eq!(details[0].character_name.as_deref(), Some("Synthetic Mage"));
            assert_eq!(details[0].class_id, Some(777));
            assert!(reader
                .get_run("demo-run")
                .unwrap()
                .unwrap()
                .character
                .is_none());
        }
    }
}

fn database() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    crate::common::account_scope_fixture::apply_batch(&mut db, &story_events()).unwrap();
    let original = demo::demo_run_story().run;
    let mut events = Vec::new();
    for (id, start, duration, outcome) in [
        ("fast", 10_000_000, 2_000_000, Some(Outcome::Cleared)),
        ("death", 20_000_000, 3_000_000, Some(Outcome::Died)),
        ("left", 30_000_000, 3_000_000, Some(Outcome::Left)),
        ("unknown", 40_000_000, 3_000_000, Some(Outcome::Unknown)),
    ] {
        let mut run = original.clone();
        run.id = id.into();
        run.started = Timestamp(start);
        run.ended = Some(Timestamp(start + duration));
        run.completed_at =
            (outcome == Some(Outcome::Cleared)).then_some(Timestamp(start + duration));
        run.died_at = (outcome == Some(Outcome::Died)).then_some(Timestamp(start + duration));
        run.outcome = outcome;
        run.evidence = run
            .completed_at
            .map(|at| EvidenceAt {
                at,
                evidence: Evidence::Victory,
            })
            .into_iter()
            .collect();
        events.push(EngineEvent::RunStarted(run));
    }
    let mut other_character = demo::demo_run_story().character.unwrap();
    other_character.id = "other-character".into();
    other_character.name = Some("TestMage".into());
    events.push(EngineEvent::CharacterUpdated(other_character));
    let mut other = original;
    other.id = "other".into();
    other.character_id = Some("other-character".into());
    other.dungeon = "Other synthetic dungeon".into();
    events.push(EngineEvent::RunStarted(other));
    crate::common::account_scope_fixture::apply_batch(
        &mut db,
        &events
            .into_iter()
            .enumerate()
            .map(|(i, e)| envelope(i as u64 + 8, e))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    (dir, path, db)
}

fn page(reader: &StoreReader, filter: RunFilter) -> Value {
    serde_json::to_value(reader.list_runs(filter).unwrap()).unwrap()
}

#[test]
fn row_details_follow_page_order_and_preserve_observed_values() {
    let (_dir, path, _db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let result = page(&reader, filter(RunSort::Oldest, 100));
    let details = result["details"].as_array().expect("per-row details");
    assert_eq!(details.len(), result["rows"].as_array().unwrap().len());
    for (detail, row) in details.iter().zip(result["rows"].as_array().unwrap()) {
        assert_eq!(detail["run_id"], row["id"]);
    }
    let slow = details.iter().find(|d| d["run_id"] == "demo-run").unwrap();
    let fast = details.iter().find(|d| d["run_id"] == "fast").unwrap();
    assert_eq!(slow["local_share"], Value::Null);
    assert_eq!(slow["share_partial"], true);
    assert_eq!(slow["fame_delta"], 10);
    assert_eq!(slow["observed_drop_count"], 1);
    assert_eq!(slow["loot_preview"][0]["drop_id"], "demo-drop");
    assert_eq!(slow["clear_micros"], 4_000_000);
    assert_eq!(fast["clear_micros"], 2_000_000);
    assert_eq!(slow["best_clear_micros"], fast["best_clear_micros"]);
    assert_eq!(slow["is_personal_best"], false);
    assert_eq!(fast["is_personal_best"], true);
    assert_eq!(slow["previous_best_clear_micros"], Value::Null);
    assert_eq!(fast["previous_best_clear_micros"], 4_000_000);
    assert_eq!(fast["fame_delta"], Value::Null);
    assert!(serde_json::to_vec(&result).unwrap().len() < 1024 * 1024);
}

#[test]
fn summary_counts_ignore_outcome_but_respect_other_filters() {
    let (_dir, path, _db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let mut query = filter(RunSort::Newest, 100);
    query.dungeon = Some("Synthetic Sanctum".into());
    query.outcome = Some(Outcome::Cleared);
    let result = page(&reader, query.clone());
    let summary = &result["summary"];
    assert_eq!(summary["runs"], 5);
    assert_eq!(summary["cleared"], 2);
    assert_eq!(summary["died"], 1);
    assert_eq!(summary["left"], 1);
    assert_eq!(summary["unknown"], 1);
    assert_eq!(summary["personal_bests"], 1);
    assert_eq!(summary["fame_delta"], 10);
    assert_eq!(summary["fame_observed_runs"], 1);
    assert_eq!(summary["time_in_runs_micros"], 7_000_000);
    assert_eq!(summary["total_runs_all_time"], 6);
    assert_eq!(summary["first_started"], "1000000");
    assert_eq!(summary["last_death"]["run_id"], "death");
    query.range = Some(TimeRange {
        start: Timestamp(10_000_000),
        end: Timestamp(40_000_000),
    });
    query.outcome = None;
    let result = page(&reader, query);
    assert_eq!(result["summary"]["runs"], 3);
    assert_eq!(result["summary"]["unknown"], 0);
    assert_eq!(result["summary"]["last_death"]["run_id"], "death");
    assert_eq!(result["summary"]["last_death"]["at"], "23000000");
}

#[test]
fn character_filter_and_cursor_summary_are_independent() {
    let (_dir, path, _db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let mut value = serde_json::to_value(filter(RunSort::Newest, 1)).unwrap();
    value["character_id"] = json!("demo-character");
    let query: RunFilter = serde_json::from_value(value.clone()).unwrap();
    let first = reader.list_runs(query.clone()).unwrap();
    let result = serde_json::to_value(&first).unwrap();
    assert_eq!(result["summary"]["runs"], 5);
    assert_eq!(result["summary"]["cleared"], 2);
    assert_eq!(result["summary"]["died"], 1);
    assert_eq!(result["summary"]["left"], 1);
    assert_eq!(result["summary"]["unknown"], 1);
    assert_eq!(result["summary"]["characters"].as_array().unwrap().len(), 2);
    let mut next = query;
    next.cursor = first.next;
    let second = page(&reader, next);
    assert_eq!(second["summary"], Value::Null);
    assert_eq!(second["details"].as_array().unwrap().len(), 1);
    value["character_id"] = json!("other-character");
    let other = page(&reader, serde_json::from_value(value.clone()).unwrap());
    assert_eq!(other["rows"][0]["id"], "other");
    assert_eq!(other["summary"]["runs"], 1);
    for invalid in ["", "bad\nidentifier", &"x".repeat(257)] {
        value["character_id"] = json!(invalid);
        assert!(matches!(
            reader.list_runs(serde_json::from_value(value.clone()).unwrap()),
            Err(StoreError::InvalidEvent)
        ));
    }
}

#[test]
fn legacy_filter_json_deserializes() {
    let mut legacy = serde_json::to_value(filter(RunSort::Newest, 50)).unwrap();
    legacy.as_object_mut().unwrap().remove("character_id");
    let restored: RunFilter = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap().get("character_id"),
        Some(&Value::Null)
    );
}

#[test]
fn hundred_row_page_stays_under_one_mib_and_loot_is_ranked_then_newest() {
    let (_dir, path, mut db) = database();
    let mut events = Vec::new();
    for i in 0..100 {
        let mut run = demo::demo_run_story().run;
        run.id = format!("page-{i:03}");
        run.started = Timestamp(100_000_000 + i * 10_000_000);
        run.ended = Some(Timestamp(run.started.0 + 5_000_000));
        run.completed_at = Some(Timestamp(run.started.0 + 4_000_000));
        run.evidence[0].at = run.completed_at.unwrap();
        events.push(EngineEvent::RunStarted(run.clone()));
        for bag in 0..3 {
            let mut drop = demo::demo_run_story().drops.remove(0);
            drop.id = format!("page-{i:03}-bag-{bag}");
            drop.run_id = Some(run.id.clone());
            drop.at = run.completed_at.unwrap();
            events.push(EngineEvent::BagDropped(drop));
        }
    }
    for (id, rank, at) in [
        ("rank-low", Some(1), 5_000_000),
        ("rank-old", Some(8), 2_000_000),
        ("rank-new", Some(8), 3_000_000),
        ("rank-unknown", None, 6_000_000),
    ] {
        let mut drop = demo::demo_run_story().drops.remove(0);
        drop.id = id.into();
        drop.bag_rank = rank;
        drop.at = Timestamp(at);
        events.push(EngineEvent::BagDropped(drop));
    }
    crate::common::account_scope_fixture::apply_batch(
        &mut db,
        &events
            .into_iter()
            .enumerate()
            .map(|(i, e)| envelope(i as u64 + 14, e))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let page = reader.list_runs(filter(RunSort::Newest, 100)).unwrap();
    assert_eq!(page.rows.len(), 100);
    assert_eq!(page.details.as_ref().unwrap().len(), 100);
    assert!(page
        .details
        .as_ref()
        .unwrap()
        .iter()
        .all(|detail| detail.loot_preview.len() == 3 && detail.observed_drop_count == 3));
    let bytes = serde_json::to_vec(&page).unwrap().len();
    eprintln!("100-row page with three bags per row: {bytes} bytes");
    assert!(bytes < 1024 * 1024);
    let first = reader.list_runs(filter(RunSort::Oldest, 1)).unwrap();
    let detail = &first.details.unwrap()[0];
    assert_eq!(detail.observed_drop_count, 5);
    assert_eq!(
        detail
            .loot_preview
            .iter()
            .map(|d| d.drop_id.as_str())
            .collect::<Vec<_>>(),
        ["rank-new", "rank-old", "rank-low"]
    );
}

#[test]
fn valid_share_unknown_identity_and_pb_ties_follow_home_rules() {
    let (_dir, path, db) = database();
    db.execute_batch("UPDATE runs SET complete_capture=1,json=json_set(json,'$.issues',json('[]'));
        UPDATE fights SET complete_capture=1,json=json_set(json,'$.issues',json('[]'),'$.total_damage',100.0);
        UPDATE fight_players SET damage=40.0,json=json_set(json,'$.is_local',json('true'),'$.share',0.4,'$.sources',json('[]'));
        UPDATE drops SET json=json_set(json,'$.issues',json('[]'));").unwrap();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let query = filter(RunSort::Oldest, 100);
    let detail = reader
        .list_runs(query.clone())
        .unwrap()
        .details
        .unwrap()
        .remove(0);
    assert_eq!(detail.local_share, Some(0.4));
    assert!(!detail.share_partial);
    db.execute(
        "UPDATE drops SET json=json_set(json,'$.issues',json('[\"missing_identity\"]'))",
        [],
    )
    .unwrap();
    let detail = reader
        .list_runs(query.clone())
        .unwrap()
        .details
        .unwrap()
        .remove(0);
    assert_eq!(detail.local_share, None);
    assert!(detail.share_partial);
    // Equal clear durations choose the earlier start, then id, not every tied row.
    db.execute(
        "UPDATE runs SET completed_at=started+2000000 WHERE id='demo-run'",
        [],
    )
    .unwrap();
    let details = reader.list_runs(query).unwrap().details.unwrap();
    assert!(
        details
            .iter()
            .find(|d| d.run_id == "demo-run")
            .unwrap()
            .is_personal_best
    );
    assert!(
        !details
            .iter()
            .find(|d| d.run_id == "fast")
            .unwrap()
            .is_personal_best
    );
    db.execute("DELETE FROM run_evidence WHERE run_id='demo-run'", [])
        .unwrap();
    let details = reader
        .list_runs(filter(RunSort::Oldest, 100))
        .unwrap()
        .details
        .unwrap();
    assert_eq!(details[0].clear_micros, None);
    assert!(
        details
            .iter()
            .find(|d| d.run_id == "fast")
            .unwrap()
            .is_personal_best
    );
}

#[test]
fn oversized_details_and_summary_are_rejected_before_returning_the_page() {
    let (_dir, path, db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let oversized = "TestMage".repeat(1024 * 1024 / 8);
    db.execute(
        "UPDATE runs SET entry_character_json=json_object('name',?1) WHERE id='demo-run'",
        [&oversized],
    )
    .unwrap();
    assert!(matches!(
        reader.list_runs(filter(RunSort::Oldest, 1)),
        Err(StoreError::Limit)
    ));
    db.execute("UPDATE runs SET entry_character_json=NULL", [])
        .unwrap();
    db.execute(
        "UPDATE characters SET json=json_set(json,'$.name',?1)",
        [&oversized],
    )
    .unwrap();
    assert!(matches!(
        reader.list_runs(filter(RunSort::Newest, 1)),
        Err(StoreError::Limit)
    ));
}

#[test]
fn unknown_segment_matches_filtered_rows_and_excludes_in_progress_runs() {
    let (_dir, path, mut db) = database();
    let mut open = demo::demo_run_story().run;
    open.id = "open-run".into();
    open.started = Timestamp(50_000_000);
    open.ended = None;
    open.completed_at = None;
    open.died_at = None;
    open.outcome = None;
    open.evidence.clear();
    crate::common::account_scope_fixture::apply_batch(
        &mut db,
        &[envelope(14, EngineEvent::RunStarted(open))],
    )
    .unwrap();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let mut query = filter(RunSort::Newest, 100);
    query.dungeon = Some("Synthetic Sanctum".into());
    query.outcome = Some(Outcome::Unknown);
    let page = reader.list_runs(query).unwrap();
    let summary = page.summary.unwrap();
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].id, "unknown");
    assert_eq!(summary.unknown as usize, page.rows.len());
    assert_eq!(summary.runs, 6);
    assert_eq!(
        summary.cleared + summary.died + summary.left + summary.unknown,
        5
    );
}

#[test]
fn last_death_respects_dungeon_range_and_character_even_with_cleared_filter() {
    let (_dir, path, _db) = database();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let mut query = filter(RunSort::Newest, 100);
    query.outcome = Some(Outcome::Cleared);
    assert_eq!(
        reader
            .list_runs(query.clone())
            .unwrap()
            .summary
            .unwrap()
            .last_death
            .unwrap()
            .run_id,
        "death"
    );
    for scoped in [
        RunFilter {
            dungeon: Some("Other synthetic dungeon".into()),
            ..query.clone()
        },
        RunFilter {
            character_id: Some("other-character".into()),
            ..query.clone()
        },
        RunFilter {
            range: Some(TimeRange {
                start: Timestamp(30_000_000),
                end: Timestamp(50_000_000),
            }),
            ..query
        },
    ] {
        let summary = reader.list_runs(scoped).unwrap().summary.unwrap();
        assert_eq!(summary.died, 0);
        assert_eq!(summary.last_death, None);
    }
}

#[test]
fn fame_windows_preserve_boundaries_ties_overlaps_and_unknown_samples() {
    let (_dir, path, db) = database();
    db.execute_batch(
        "DELETE FROM fame_samples;
        UPDATE runs SET character_id='other-character' WHERE id='other';
        UPDATE runs SET started=1000000,ended=6000000 WHERE id='fast';
        INSERT INTO fame_samples(stream_id,sequence,character_id,at,fame) VALUES
            ('a',1,'demo-character',999999,999),
            ('a',2,'demo-character',1000000,100),
            ('b',1,'demo-character',1000000,150),
            ('b',2,'demo-character',6000000,170),
            ('b',3,'demo-character',6000000,180),
            ('z',1,'demo-character',6000001,200),
            ('a',3,'demo-character',20000000,300),
            ('a',4,'other-character',1000000,10),
            ('a',5,'other-character',6000000,30);",
    )
    .unwrap();
    let page = crate::common::account_scope_fixture::open_reader(&path)
        .unwrap()
        .list_runs(filter(RunSort::Oldest, 100))
        .unwrap();
    let details = page.details.unwrap();
    for id in ["demo-run", "fast"] {
        assert_eq!(
            details.iter().find(|d| d.run_id == id).unwrap().fame_delta,
            Some(80)
        );
    }
    assert_eq!(
        details
            .iter()
            .find(|d| d.run_id == "other")
            .unwrap()
            .fame_delta,
        Some(20)
    );
    for id in ["death", "left", "unknown"] {
        assert_eq!(
            details.iter().find(|d| d.run_id == id).unwrap().fame_delta,
            None
        );
    }
    let summary = page.summary.unwrap();
    assert_eq!(summary.fame_delta, Some(180));
    assert_eq!(summary.fame_observed_runs, 3);
}

#[test]
fn previous_best_uses_earlier_qualifying_clears_and_started_id_order() {
    let (_dir, path, mut db) = database();
    let cases = [
        ("no-proof", 1, 1, false, false, "Previous synthetic dungeon"),
        (
            "partial-no-proof",
            2,
            1,
            false,
            true,
            "Previous synthetic dungeon",
        ),
        ("other-dungeon", 3, 1, true, false, "Other prior dungeon"),
        (
            "first-clear",
            10,
            90,
            true,
            false,
            "Previous synthetic dungeon",
        ),
        (
            "faster-clear",
            20,
            62,
            true,
            false,
            "Previous synthetic dungeon",
        ),
        (
            "slower-clear",
            30,
            70,
            true,
            false,
            "Previous synthetic dungeon",
        ),
        ("tie-c", 40, 80, true, false, "Previous synthetic dungeon"),
        ("tie-b", 40, 50, true, false, "Previous synthetic dungeon"),
        ("tie-a", 40, 60, true, false, "Previous synthetic dungeon"),
        (
            "partial-with-proof",
            50,
            40,
            true,
            true,
            "Previous synthetic dungeon",
        ),
        (
            "after-partial",
            60,
            70,
            true,
            false,
            "Previous synthetic dungeon",
        ),
    ];
    let events = cases
        .into_iter()
        .enumerate()
        .map(|(i, (id, start, seconds, proof, partial, dungeon))| {
            let mut run = demo::demo_run_story().run;
            run.id = id.into();
            run.dungeon = dungeon.into();
            run.started = Timestamp(start * 1_000_000);
            run.completed_at = Some(Timestamp((start + seconds) * 1_000_000));
            run.ended = run.completed_at;
            run.complete_capture = !partial;
            run.issues = if partial {
                vec![CoverageIssue::StreamGap]
            } else {
                vec![]
            };
            run.evidence = if proof {
                vec![EvidenceAt {
                    at: run.completed_at.unwrap(),
                    evidence: Evidence::CompletionDialogue {
                        rule: "synthetic-completion".into(),
                    },
                }]
            } else {
                vec![]
            };
            envelope(i as u64 + 14, EngineEvent::RunStarted(run))
        })
        .collect::<Vec<_>>();
    crate::common::account_scope_fixture::apply_batch(&mut db, &events).unwrap();
    let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
    let result = page(&reader, filter(RunSort::Newest, 100));
    let details = result["details"].as_array().unwrap();
    for (id, expected) in [
        ("first-clear", None),
        ("faster-clear", Some(90_000_000)),
        ("slower-clear", Some(62_000_000)),
        ("tie-a", Some(62_000_000)),
        ("tie-b", Some(60_000_000)),
        ("tie-c", Some(50_000_000)),
        ("partial-with-proof", Some(50_000_000)),
        ("after-partial", Some(40_000_000)),
    ] {
        let row = details.iter().find(|d| d["run_id"] == id).unwrap();
        assert_eq!(
            row.get("previous_best_clear_micros"),
            Some(&json!(expected)),
            "{id}"
        );
    }
    // Earlier history must remain visible even outside the page and date filter.
    let mut query = filter(RunSort::Newest, 1);
    query.dungeon = Some("Previous synthetic dungeon".into());
    query.range = Some(TimeRange {
        start: Timestamp(20_000_000),
        end: Timestamp(21_000_000),
    });
    let result = page(&reader, query);
    assert_eq!(result["details"][0]["run_id"], "faster-clear");
    assert_eq!(
        result["details"][0]["previous_best_clear_micros"],
        90_000_000
    );
}

#[test]
fn inferred_weapon_labels_do_not_make_saved_damage_partial() {
    for (estimated, reasons, uncertain) in [
        (false, vec![], false),
        (true, vec![], true),
        (true, vec!["inferred-equipped-weapon"], false),
        (
            false,
            vec!["inferred-equipped-weapon", "inferred-equipped-weapon"],
            false,
        ),
        (
            true,
            vec![
                "inferred-equipped-weapon",
                "client-projectile-reconstruction",
            ],
            true,
        ),
        (false, vec!["source-not-observed"], true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inferred.db");
        let mut db = Connection::open(&path).unwrap();
        migrate(&mut db).unwrap();
        crate::common::account_scope_fixture::apply_batch(&mut db, &story_events()[..4]).unwrap();
        let mut fight = demo::demo_snapshot().fights.remove(0);
        fight.complete_capture = true;
        fight.issues.clear();
        let mut remote = fight.players[0].clone();
        remote.player_id = "synthetic-remote".into();
        remote.object_id = 3;
        remote.is_local = false;
        for source in &mut fight.players[0].sources {
            source.estimated = false;
            source.reasons.clear();
        }
        remote.sources.truncate(1);
        remote.sources[0].estimated = estimated;
        remote.sources[0].reasons = reasons.iter().map(|r| (*r).into()).collect();
        fight.players.push(remote);
        let mut run = demo::demo_run_story().run;
        run.complete_capture = true;
        run.issues.clear();
        crate::common::account_scope_fixture::apply_batch(
            &mut db,
            &[
                envelope(5, EngineEvent::RunUpdated(run)),
                envelope(6, EngineEvent::FightUpdated(fight)),
            ],
        )
        .unwrap();
        let reader = crate::common::account_scope_fixture::open_reader(&path).unwrap();
        assert_eq!(
            reader.get_run("demo-run").unwrap().unwrap().fights[0].estimated,
            uncertain,
            "{reasons:?}"
        );
        assert_eq!(
            reader
                .list_runs(filter(RunSort::Newest, 100))
                .unwrap()
                .details
                .unwrap()
                .iter()
                .find(|d| d.run_id == "demo-run")
                .unwrap()
                .share_partial,
            uncertain,
            "{reasons:?}"
        );
        assert_eq!(
            reader
                .home_summary(
                    HomeScope::Session {
                        id: "demo-session".into()
                    },
                    Some(ACCOUNT)
                )
                .unwrap()
                .recent_details[0]
                .share_partial,
            uncertain,
            "{reasons:?}"
        );
    }
}

#[test]
fn personal_bests_use_each_runs_account_without_hiding_legacy_runs() {
    let (_dir, path, db) = database();
    db.execute_batch(
        "INSERT INTO accounts VALUES('synthetic-b');
        UPDATE characters SET account_id='synthetic-b' WHERE id='other-character';
        UPDATE runs SET dungeon='Synthetic Sanctum',started=500000,completed_at=1500000,
            character_id='other-character' WHERE id='other';
        UPDATE runs SET character_id=NULL,completed_at=started+1,outcome='cleared' WHERE id='left';
        INSERT INTO run_evidence VALUES('left',0,30000001,'victory','{}');",
    )
    .unwrap();
    let reader = maxed_store::open_reader(&path).unwrap();
    let result = page(&reader, filter(RunSort::Oldest, 100));
    assert_eq!(result["rows"].as_array().unwrap().len(), 6);
    assert_eq!(result["summary"]["personal_bests"], 2);
    let details = result["details"].as_array().unwrap();
    for (id, best, previous, pb) in [
        ("demo-run", Some(2_000_000), None, false),
        ("fast", Some(2_000_000), Some(4_000_000), true),
        ("other", Some(1_000_000), None, true),
        ("left", None, None, false),
    ] {
        let row = details.iter().find(|d| d["run_id"] == id).unwrap();
        assert_eq!(row["best_clear_micros"], json!(best), "{id}");
        assert_eq!(row["previous_best_clear_micros"], json!(previous), "{id}");
        assert_eq!(row["is_personal_best"], pb, "{id}");
    }
    // Unattributed clear timing is still observed, but cannot seed or beat a PB.
    assert_eq!(
        details.iter().find(|d| d["run_id"] == "left").unwrap()["clear_micros"],
        1
    );
    for (account, best) in [(ACCOUNT, 2_000_000), ("synthetic-b", 1_000_000)] {
        assert_eq!(
            reader.dungeon_bests(Some(account)).unwrap()[0].best_clear_micros,
            best
        );
    }
}

#[test]
fn account_scoped_rows_comparison_and_recap_agree() {
    let (_dir, path, mut db) = database();
    db.execute_batch(
        "INSERT INTO accounts VALUES('synthetic-b');
        UPDATE characters SET account_id='synthetic-b' WHERE id='other-character';",
    )
    .unwrap();
    let mut events = Vec::new();
    for (owner, prefix, duration) in [
        (Some("demo-character"), "a", 4_000_000),
        (Some("other-character"), "b", 1_000_000),
        (None, "legacy", 1),
    ] {
        for i in 0..3 {
            let mut run = demo::demo_run_story().run;
            run.id = format!("{prefix}-{i}");
            run.character_id = owner.map(Into::into);
            run.started = Timestamp(2_000_000 + i * 1_000_000);
            run.completed_at = Some(Timestamp(run.started.0 + duration));
            run.ended = run.completed_at;
            run.evidence = vec![EvidenceAt {
                at: run.completed_at.unwrap(),
                evidence: Evidence::Victory,
            }];
            events.push(run);
        }
    }
    let mut current = demo::demo_run_story().run;
    current.id = "b-current".into();
    current.character_id = Some("other-character".into());
    current.started = Timestamp(12_000_000);
    current.completed_at = Some(Timestamp(12_500_000));
    current.ended = current.completed_at;
    current.evidence = vec![EvidenceAt {
        at: current.completed_at.unwrap(),
        evidence: Evidence::Victory,
    }];
    events.push(current);
    maxed_store::apply_batch(
        &mut db,
        &events
            .into_iter()
            .enumerate()
            .map(|(i, run)| envelope(14 + i as u64, EngineEvent::RunStarted(run)))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let reader = maxed_store::open_reader(&path).unwrap();
    let rows = reader
        .list_runs(filter(RunSort::Newest, 100))
        .unwrap()
        .details
        .unwrap();
    for (id, account, best, previous, count) in [
        ("fast", ACCOUNT, 2_000_000, 4_000_000, 4),
        ("b-current", "synthetic-b", 500_000, 1_000_000, 3),
    ] {
        let row = rows.iter().find(|r| r.run_id == id).unwrap();
        assert_eq!(row.best_clear_micros, Some(best));
        assert_eq!(row.previous_best_clear_micros, Some(previous));
        assert!(row.is_personal_best);
        let comparison = reader.get_run(id).unwrap().unwrap().comparison.unwrap();
        assert_eq!(comparison.previous_clears, count);
        assert_eq!(comparison.mean_micros, previous as f64);
        let pb = reader
            .run_recap(&RunRecapRequest { run_id: id.into() }, &[])
            .unwrap()
            .unwrap()
            .pb_clear
            .unwrap();
        assert_eq!((pb.clear_micros, pb.previous_best_micros), (best, previous));
        let dungeon = reader
            .dungeon_bests(Some(account))
            .unwrap()
            .into_iter()
            .find(|d| d.dungeon == "Synthetic Sanctum")
            .unwrap();
        assert_eq!(dungeon.best_clear_micros, best);
    }
    assert!(reader
        .get_run("legacy-2")
        .unwrap()
        .unwrap()
        .comparison
        .is_none());
    assert!(reader
        .run_recap(
            &RunRecapRequest {
                run_id: "legacy-2".into()
            },
            &[]
        )
        .unwrap()
        .unwrap()
        .pb_clear
        .is_none());
}
