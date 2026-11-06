//! Synthetic saved-run screen contracts.
use crate::common;
use common::{envelope, story_events};
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;
use serde_json::{json, Value};

fn database() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    apply_batch(&mut db, &story_events()).unwrap();
    (dir, path, db)
}

fn story(path: &std::path::Path, id: &str) -> Value {
    serde_json::to_value(open_reader(path).unwrap().get_run(id).unwrap().unwrap()).unwrap()
}

fn fights(db: &mut Connection, kinds: &[Option<TargetKind>]) -> Vec<FightView> {
    let values: Vec<_> = kinds
        .iter()
        .enumerate()
        .map(|(i, kind)| {
            let mut fight = demo::demo_snapshot().fights.remove(0);
            fight.id = if i == 0 {
                "demo-fight".into()
            } else {
                format!("fight-{i:03}")
            };
            fight.started = Timestamp(2_000_000 + (kinds.len() - i) as u64);
            fight.target_kind = kind.clone();
            fight.boss_type = None;
            fight
        })
        .collect();
    let events: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(i, f)| envelope(8 + i as u64, EngineEvent::FightUpdated(f.clone())))
        .collect();
    apply_batch(db, &events).unwrap();
    values
}

#[test]
fn boss_views_are_full_filtered_and_in_start_order() {
    let (_dir, path, mut db) = database();
    let values = fights(
        &mut db,
        &[
            Some(TargetKind::Boss),
            Some(TargetKind::Ordinary),
            Some(TargetKind::EventBoss),
            Some(TargetKind::Ordinary),
            Some(TargetKind::Ordinary),
        ],
    );
    let result = story(&path, "demo-run");
    assert_eq!(result["boss_fights"], json!([values[2], values[0]]));
    assert_eq!(result["boss_fights_limited"], false);
}

#[test]
fn miniboss_and_unknown_targets_do_not_create_boss_views() {
    let (_dir, path, mut db) = database();
    fights(&mut db, &[Some(TargetKind::Miniboss), None]);
    let result = story(&path, "demo-run");
    assert_eq!(result["boss_fights"], json!([]));
    assert_eq!(result["boss_fights_limited"], false);
}

#[test]
fn boss_type_preserves_legacy_boss_and_zero_bosses_are_explicit() {
    let (_dir, path, mut db) = database();
    let mut values = fights(&mut db, &[None]);
    assert_eq!(story(&path, "demo-run")["boss_fights"], json!([]));
    values[0].boss_type = Some(123);
    apply_batch(
        &mut db,
        &[envelope(9, EngineEvent::FightUpdated(values[0].clone()))],
    )
    .unwrap();
    assert_eq!(story(&path, "demo-run")["boss_fights"], json!(values));
}

#[test]
fn boss_views_are_capped_and_report_truncation() {
    let (_dir, path, mut db) = database();
    let mut values = fights(&mut db, &vec![Some(TargetKind::Boss); 65]);
    values.reverse();
    values.truncate(64);
    let result = story(&path, "demo-run");
    assert_eq!(result["boss_fights"], json!(values));
    assert_eq!(result["boss_fights_limited"], true);
}

#[test]
fn unreadable_boss_details_are_skipped_without_losing_later_bosses() {
    let (_dir, path, mut db) = database();
    let values = fights(&mut db, &vec![Some(TargetKind::Boss); 3]);
    for invalid in ["broken", "{}"] {
        db.execute(
            "UPDATE fights SET json=?1 WHERE id=?2",
            rusqlite::params![invalid, values[1].id],
        )
        .unwrap();
        let result = story(&path, "demo-run");
        assert_eq!(result["boss_fights"], json!([values[2], values[0]]));
        assert_eq!(result["boss_fights_limited"], true);
        assert_eq!(result["fights"].as_array().unwrap().len(), 3);
    }
}

#[test]
fn boss_detail_sql_type_errors_still_propagate() {
    let (_dir, path, mut db) = database();
    fights(&mut db, &[Some(TargetKind::Boss)]);
    // A SQLite column conversion error is not an unreadable JSON document.
    db.execute("UPDATE fights SET json=x'80'", []).unwrap();
    assert!(matches!(
        open_reader(&path).unwrap().get_run("demo-run"),
        Err(StoreError::Io)
    ));
}

#[test]
fn boss_details_stop_at_sixteen_mib_without_losing_the_story() {
    let (_dir, path, mut db) = database();
    let mut values = fights(&mut db, &vec![Some(TargetKind::Boss); 3]);
    values.reverse();
    let reader = open_reader(&path).unwrap();
    let mut expected = reader.get_run("demo-run").unwrap().unwrap();
    // Exactly two 8 MiB input rows fit. Whitespace exercises the pre-decode
    // byte guard without constructing millions of nested synthetic objects.
    db.execute(
        "UPDATE fights SET json=json || substr(?1,1,?2-length(CAST(json AS BLOB)))",
        rusqlite::params![" ".repeat(8 * 1024 * 1024), 8 * 1024 * 1024],
    )
    .unwrap();
    expected.boss_fights = Some(values[..2].to_vec());
    expected.boss_fights_limited = Some(true);
    assert_eq!(reader.get_run("demo-run").unwrap(), Some(expected.clone()));

    // One additional byte on the second row crosses the sub-budget.
    db.execute(
        "UPDATE fights SET json=json || ' ' WHERE id=?1",
        [&values[1].id],
    )
    .unwrap();
    expected.boss_fights = Some(values[..1].to_vec());
    assert_eq!(reader.get_run("demo-run").unwrap(), Some(expected));
}

fn event(id: &str, at: u64) -> InstanceEventView {
    InstanceEventView {
        id: id.into(),
        session_id: "demo-session".into(),
        run_id: Some("demo-run".into()),
        map: None,
        at: Timestamp(at),
        provisional: true,
        kind: InstanceEventKind::PlayerLeft {
            object_id: 42,
            player_name: Some("TestMage".into()),
            class_id: None,
        },
    }
}

#[test]
fn events_are_run_scoped_and_ordered_with_names() {
    let (_dir, path, mut db) = database();
    let mut other_run = demo::demo_run_story().run;
    other_run.id = "other-run".into();
    let left = event("z-left", 4_000_000);
    let mut death = event("a-death", 4_000_000);
    death.kind = InstanceEventKind::PlayerDeath {
        player_id: None,
        player_name: "DemoAlly".into(),
        level: None,
        killed_by: None,
        grave_type: None,
        object_id: Some(43),
        class_id: None,
        is_local: false,
    };
    let mut other = event("other", 2_000_000);
    other.run_id = Some(other_run.id.clone());
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::RunStarted(other_run)),
            envelope(9, EngineEvent::InstanceEvent(left.clone())),
            envelope(10, EngineEvent::InstanceEvent(death.clone())),
            envelope(11, EngineEvent::InstanceEvent(other)),
        ],
    )
    .unwrap();
    assert_eq!(story(&path, "demo-run")["events"], json!([death, left]));
    assert_eq!(story(&path, "demo-run")["events_limited"], false);
}

#[test]
fn unreadable_events_are_skipped_without_losing_later_events() {
    let (_dir, path, mut db) = database();
    let first = event("first", 2_000_000);
    let middle = event("middle", 3_000_000);
    let last = event("last", 4_000_000);
    apply_batch(
        &mut db,
        &[
            envelope(8, EngineEvent::InstanceEvent(last.clone())),
            envelope(9, EngineEvent::InstanceEvent(middle.clone())),
            envelope(10, EngineEvent::InstanceEvent(first.clone())),
        ],
    )
    .unwrap();
    let mut unknown = serde_json::to_value(&middle).unwrap();
    unknown["kind"] = json!({"type": "unknown_event"});
    for invalid in ["broken".to_owned(), unknown.to_string()] {
        db.execute(
            "UPDATE instance_events SET json=?1 WHERE id=?2",
            rusqlite::params![invalid, middle.id],
        )
        .unwrap();
        let result = open_reader(&path)
            .unwrap()
            .get_run("demo-run")
            .unwrap()
            .unwrap();
        assert_eq!(result.events, Some(vec![first.clone(), last.clone()]));
        assert_eq!(result.events_limited, Some(true));
    }

    // SQL column conversion failures remain errors, rather than omitted events.
    db.execute(
        "UPDATE instance_events SET json=x'80' WHERE id='middle'",
        [],
    )
    .unwrap();
    assert!(matches!(
        open_reader(&path).unwrap().get_run("demo-run"),
        Err(StoreError::Io)
    ));
}

#[test]
fn events_are_capped_at_two_thousand_in_time_order() {
    let (_dir, path, mut db) = database();
    let events: Vec<_> = (0..2001)
        .map(|i| {
            envelope(
                8 + i,
                EngineEvent::InstanceEvent(event(&format!("event-{i:04}"), 2_000_000 + 2000 - i)),
            )
        })
        .collect();
    apply_batch(&mut db, &events).unwrap();
    let result = story(&path, "demo-run");
    let rows = result["events"].as_array().unwrap();
    assert_eq!(rows.len(), 2000);
    assert_eq!(rows[0]["id"], "event-2000");
    assert_eq!(rows[1999]["id"], "event-0001");
    assert_eq!(result["events_limited"], true);
}

#[test]
fn neighbors_cross_dungeons_and_break_start_ties_by_id() {
    let (_dir, path, mut db) = database();
    let events: Vec<_> = [("a", 10), ("b", 20), ("c", 20)]
        .into_iter()
        .enumerate()
        .map(|(i, (id, at))| {
            let mut run = demo::demo_run_story().run;
            run.id = id.into();
            run.started = Timestamp(at);
            run.dungeon = format!("Synthetic {id}");
            envelope(8 + i as u64, EngineEvent::RunStarted(run))
        })
        .collect();
    apply_batch(&mut db, &events).unwrap();
    let middle = story(&path, "b");
    assert_eq!(middle["older_run_id"], "a");
    assert_eq!(middle["newer_run_id"], "c");
    assert!(story(&path, "a")["older_run_id"].is_null());
    assert!(story(&path, "demo-run")["newer_run_id"].is_null());
}

#[test]
fn legacy_story_missing_screen_fields_deserializes() {
    let mut value = json!(demo::demo_run_story());
    let fields = [
        "boss_fights",
        "boss_fights_limited",
        "events",
        "events_limited",
        "older_run_id",
        "newer_run_id",
    ];
    for field in fields {
        value.as_object_mut().unwrap().remove(field);
    }
    let decoded: RunStory = serde_json::from_value(value).unwrap();
    let restored = json!(decoded);
    for field in fields {
        assert_eq!(restored.get(field), Some(&Value::Null));
    }
}

#[test]
fn oversized_boss_details_and_events_degrade_with_explicit_flags() {
    for table in ["fights", "instance_events"] {
        let (_dir, path, mut db) = database();
        fights(&mut db, &[Some(TargetKind::Boss)]);
        apply_batch(
            &mut db,
            &[envelope(
                9,
                EngineEvent::InstanceEvent(event("left", 3_000_000)),
            )],
        )
        .unwrap();
        // Valid trailing whitespace counts toward the input budget even though
        // it would disappear from the serialized response after decoding.
        db.execute(
            &format!("UPDATE {table} SET json=json || ?1"),
            [" ".repeat(32 * 1024 * 1024)],
        )
        .unwrap();
        let result = open_reader(&path).unwrap().get_run("demo-run");
        if table == "fights" {
            let result = result.unwrap().unwrap();
            assert_eq!(result.boss_fights, Some(vec![]));
            assert_eq!(result.boss_fights_limited, Some(true));
            assert_eq!(result.events.unwrap().len(), 1);
            assert_eq!(result.fights.len(), 1);
        } else {
            let result = serde_json::to_value(result.unwrap().unwrap()).unwrap();
            assert_eq!(result["events"], json!([]));
            assert_eq!(result["events_limited"], true);
            assert_eq!(result["boss_fights"].as_array().unwrap().len(), 1);
            assert_eq!(result["boss_fights_limited"], false);
        }
    }
}

#[test]
fn events_keep_priority_when_boss_details_exceed_the_shared_story_budget() {
    let (_dir, path, mut db) = database();
    fights(&mut db, &[Some(TargetKind::Boss)]);
    apply_batch(
        &mut db,
        &[envelope(
            9,
            EngineEvent::InstanceEvent(event("left", 3_000_000)),
        )],
    )
    .unwrap();
    // Each query fits its own cap, but their combined input exceeds 32 MiB.
    db.execute(
        "UPDATE fights SET json=json || ?1",
        [" ".repeat(8 * 1024 * 1024)],
    )
    .unwrap();
    db.execute(
        "UPDATE instance_events SET json=json || ?1",
        [" ".repeat(25 * 1024 * 1024)],
    )
    .unwrap();
    let result = story(&path, "demo-run");
    assert_eq!(result["boss_fights"], json!([]));
    assert_eq!(result["boss_fights_limited"], true);
    assert_eq!(result["events"].as_array().unwrap().len(), 1);
    assert_eq!(result["events_limited"], false);
}

#[test]
fn large_summary_leaves_too_little_headroom_for_boss_details() {
    let (_dir, path, mut db) = database();
    fights(&mut db, &[Some(TargetKind::Boss)]);
    let reader = open_reader(&path).unwrap();
    let mut expected = reader.get_run("demo-run").unwrap().unwrap();
    // Two rows exercise the production 32 MiB cap without large DTO vectors.
    db.execute(
        "UPDATE fight_summaries SET json=json || ?1",
        [" ".repeat(25 * 1024 * 1024)],
    )
    .unwrap();
    db.execute(
        "UPDATE fights SET json=json || ?1",
        [" ".repeat(8 * 1024 * 1024)],
    )
    .unwrap();
    expected.boss_fights = Some(vec![]);
    expected.boss_fights_limited = Some(true);
    assert_eq!(reader.get_run("demo-run").unwrap(), Some(expected));
}

#[test]
fn serialized_summary_escaping_cannot_make_boss_details_break_the_story() {
    let (_dir, path, mut db) = database();
    fights(&mut db, &[Some(TargetKind::Boss)]);
    let reader = open_reader(&path).unwrap();
    let mut expected = reader.get_run("demo-run").unwrap().unwrap();
    expected.boss_fights = Some(vec![]);
    expected.boss_fights_limited = Some(true);
    let base_bytes = serde_json::to_vec(&expected).unwrap().len();
    // JSON escaping doubles these input bytes; the raw-row budget still has
    // ample room for a boss, but the final response has less than 100 bytes.
    expected.timeline[0]
        .explanation
        .push_str(&"\n".repeat((32 * 1024 * 1024 - base_bytes - 100) / 2));
    db.execute(
        "UPDATE timeline SET explanation=?1 WHERE id=?2",
        rusqlite::params![expected.timeline[0].explanation, expected.timeline[0].id],
    )
    .unwrap();
    assert_eq!(reader.get_run("demo-run").unwrap(), Some(expected));
}

#[test]
fn boss_selection_uses_summary_kind_and_stays_within_the_run() {
    let (_dir, path, mut db) = database();
    let values = fights(&mut db, &[Some(TargetKind::Ordinary)]);
    // The summary is the classification source; full JSON remains unchanged.
    db.execute(
        "UPDATE fight_summaries SET json=json_set(json,'$.target_kind','event_boss')",
        [],
    )
    .unwrap();
    assert_eq!(story(&path, "demo-run")["boss_fights"], json!(values));
    let mut other_run = demo::demo_run_story().run;
    other_run.id = "other-run".into();
    let mut other_fight = values[0].clone();
    other_fight.id = "other-fight".into();
    other_fight.run_id = Some(other_run.id.clone());
    other_fight.target_kind = Some(TargetKind::Boss);
    apply_batch(
        &mut db,
        &[
            envelope(9, EngineEvent::RunStarted(other_run)),
            envelope(10, EngineEvent::FightUpdated(other_fight)),
        ],
    )
    .unwrap();
    assert_eq!(story(&path, "demo-run")["boss_fights"], json!(values));
}
