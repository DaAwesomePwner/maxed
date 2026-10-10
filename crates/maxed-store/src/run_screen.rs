//! Bounded, transaction-consistent additions for saved run screens.
use crate::{
    project::micros,
    queries::{add_bytes, bounded, identifier, row_json, MAX_RUN_STORY_BYTES},
    StoreError,
};
use maxed_types::{FightView, InstanceEventView, RunStory, RunView};
use rusqlite::{params, Transaction};

const MAX_STORY_BOSS_FIGHTS: usize = 64;
const MAX_STORY_BOSS_BYTES: usize = 16 * 1024 * 1024;
const MAX_STORY_EVENTS: usize = 2000;
// Leave room for field names, array separators, and fixed metadata. The final
// serialization check also handles expansion from escaping and aggregate DTOs.
const STORY_SERIALIZATION_RESERVE: usize = 4096;
const BOSSES: &str = "SELECT length(CAST(fights.json AS BLOB)),fights.json
    FROM fights LEFT JOIN fight_summaries ON fight_summaries.fight_id=fights.id
    WHERE fights.run_id=?1 AND (fights.boss_type IS NOT NULL
        OR json_extract(fight_summaries.json,'$.target_kind') IN ('boss','event_boss'))
    ORDER BY fights.started,fights.id LIMIT ?2";
const EVENTS: &str = "SELECT length(CAST(json AS BLOB)),json FROM instance_events
    WHERE run_id=?1 ORDER BY at,id LIMIT ?2";
const OLDER: &str = "SELECT length(CAST(json_quote(id) AS BLOB)),json_quote(id) FROM runs
    WHERE (started,id)<(?1,?2) ORDER BY started DESC,id DESC LIMIT 1";
const NEWER: &str = "SELECT length(CAST(json_quote(id) AS BLOB)),json_quote(id) FROM runs
    WHERE (started,id)>(?1,?2) ORDER BY started,id LIMIT 1";

pub(crate) struct RunScreen {
    pub boss_fights: Vec<FightView>,
    pub boss_fights_limited: bool,
    pub events: Vec<InstanceEventView>,
    pub events_limited: bool,
    pub older_run_id: Option<String>,
    pub newer_run_id: Option<String>,
}

pub(crate) fn load(
    tx: &Transaction<'_>,
    run: &RunView,
    bytes: &mut usize,
) -> Result<RunScreen, StoreError> {
    identifier(&run.id)?;
    let mut neighbor = |sql| -> Result<Option<String>, StoreError> {
        let mut statement = tx.prepare(sql)?;
        let mut rows = statement.query(params![micros(run.started)?, run.id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_json(row, 0, bytes, MAX_RUN_STORY_BYTES)?)),
            None => {
                add_bytes(bytes, 4, MAX_RUN_STORY_BYTES)?;
                Ok(None)
            }
        }
    };
    let older_run_id = neighbor(OLDER)?;
    let newer_run_id = neighbor(NEWER)?;
    // Both limit flags are at most five bytes each.
    add_bytes(bytes, 10, MAX_RUN_STORY_BYTES)?;
    let detail_limit = MAX_RUN_STORY_BYTES - STORY_SERIALIZATION_RESERVE;
    let mut statement = tx.prepare(EVENTS)?;
    let mut rows = statement.query(params![run.id, MAX_STORY_EVENTS as i64 + 1])?;
    let mut events = Vec::new();
    let mut events_limited = false;
    while let Some(row) = rows.next()? {
        if events.len() == MAX_STORY_EVENTS {
            events_limited = true;
            break;
        }
        let size = usize::try_from(row.get::<_, i64>(0)?).map_err(|_| StoreError::InvalidEvent)?;
        if *bytes > detail_limit || size > detail_limit - *bytes {
            events_limited = true;
            break;
        }
        // Count unreadable input too, while preserving SQL conversion errors.
        add_bytes(bytes, size, detail_limit)?;
        let json = row.get::<_, String>(1)?;
        match serde_json::from_str(&json) {
            Ok(event) => events.push(event),
            Err(_) => events_limited = true,
        }
    }
    let mut statement = tx.prepare(BOSSES)?;
    let mut rows = statement.query(params![run.id, MAX_STORY_BOSS_FIGHTS as i64 + 1])?;
    let mut boss_fights = Vec::new();
    let mut boss_fights_limited = false;
    let mut boss_bytes = 0;
    while let Some(row) = rows.next()? {
        if boss_fights.len() == MAX_STORY_BOSS_FIGHTS {
            boss_fights_limited = true;
            break;
        }
        let size = usize::try_from(row.get::<_, i64>(0)?).map_err(|_| StoreError::InvalidEvent)?;
        if *bytes > detail_limit
            || size > MAX_STORY_BOSS_BYTES - boss_bytes
            || size > detail_limit - *bytes
        {
            boss_fights_limited = true;
            break;
        }
        // Bound input before allocation/decoding, including unreadable rows.
        boss_bytes += size;
        add_bytes(bytes, size, detail_limit)?;
        let json = row.get::<_, String>(1)?;
        match serde_json::from_str(&json) {
            Ok(fight) => boss_fights.push(fight),
            Err(_) => boss_fights_limited = true,
        }
    }
    Ok(RunScreen {
        boss_fights,
        boss_fights_limited,
        events,
        events_limited,
        older_run_id,
        newer_run_id,
    })
}

/// Raw row sizes cannot predict all escaping/aggregate expansion. If the final
/// response exceeds the cap, discard optional boss details first, then events;
/// never discard the original summary data or swallow non-limit errors.
pub(crate) fn bound_story(mut story: RunStory, limit: usize) -> Result<RunStory, StoreError> {
    loop {
        match bounded(&story, limit) {
            Ok(_) => return Ok(story),
            Err(StoreError::Limit) => {
                if let Some(bosses) = story.boss_fights.as_mut().filter(|rows| !rows.is_empty()) {
                    bosses.clear();
                    story.boss_fights_limited = Some(true);
                } else if let Some(events) = story.events.as_mut().filter(|rows| !rows.is_empty()) {
                    events.clear();
                    story.events_limited = Some(true);
                } else {
                    return Err(StoreError::Limit);
                }
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rusqlite::Connection;

    fn event(id: &str) -> InstanceEventView {
        InstanceEventView {
            id: id.into(),
            session_id: "demo-session".into(),
            run_id: Some("demo-run".into()),
            map: None,
            at: maxed_types::Timestamp(3_000_000),
            provisional: false,
            kind: maxed_types::InstanceEventKind::PlayerLeft {
                object_id: 42,
                player_name: Some("TestMage".into()),
                class_id: None,
            },
        }
    }

    #[test]
    fn final_serialization_drops_bosses_then_events_without_trimming_summaries() {
        let mut story = maxed_types::demo::demo_run_story();
        story.boss_fights = Some(maxed_types::demo::demo_snapshot().fights);
        story.boss_fights_limited = Some(false);
        story.events = Some(vec![event("left")]);
        story.events_limited = Some(false);
        let mut expected = story.clone();
        expected.boss_fights = Some(vec![]);
        expected.boss_fights_limited = Some(true);
        let limit = serde_json::to_vec(&expected).unwrap().len();
        assert_eq!(bound_story(story.clone(), limit).unwrap(), expected);
        expected.events = Some(vec![]);
        expected.events_limited = Some(true);
        let limit = serde_json::to_vec(&expected).unwrap().len();
        assert_eq!(bound_story(story, limit).unwrap(), expected);
        assert!(matches!(
            bound_story(expected, limit - 1),
            Err(StoreError::Limit)
        ));
    }

    #[test]
    fn remaining_summary_budget_caps_events_before_bosses() {
        use maxed_types::{demo, EngineEvent, EventEnvelope, Sequence, Timestamp};
        let mut db = Connection::open_in_memory().unwrap();
        crate::migrate(&mut db).unwrap();
        let snapshot = demo::demo_snapshot();
        let run = snapshot.run.unwrap();
        let first = event("a-left");
        let events = [
            EngineEvent::SessionUpdated(snapshot.session.unwrap()),
            EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
            EngineEvent::RunStarted(run.clone()),
            EngineEvent::FightStarted(snapshot.fights[0].clone()),
            EngineEvent::InstanceEvent(first.clone()),
            EngineEvent::InstanceEvent(event("b-left")),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, event)| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "budget-test".into(),
            sequence: Sequence(Timestamp(i as u64 + 1)),
            at: Timestamp(4_000_000),
            event,
        })
        .collect::<Vec<_>>();
        crate::apply_batch(&mut db, &events).unwrap();
        let tx = db.transaction().unwrap();
        // Simulate large already-read summaries without writing them to disk.
        let mut bytes = MAX_RUN_STORY_BYTES
            - STORY_SERIALIZATION_RESERVE
            - serde_json::to_vec(&first).unwrap().len()
            - 32;
        let screen = load(&tx, &run, &mut bytes).unwrap();
        assert_eq!(screen.events, vec![first]);
        assert!(screen.events_limited);
        assert!(screen.boss_fights.is_empty());
        assert!(screen.boss_fights_limited);
        assert!(bytes <= MAX_RUN_STORY_BYTES - STORY_SERIALIZATION_RESERVE);

        // Empty unreadable rows also yield when summaries consume the reserve.
        tx.execute("DELETE FROM instance_events", []).unwrap();
        tx.execute("UPDATE fights SET json=''", []).unwrap();
        let mut bytes = MAX_RUN_STORY_BYTES - STORY_SERIALIZATION_RESERVE + 1;
        let screen = load(&tx, &run, &mut bytes).unwrap();
        assert!(screen.boss_fights.is_empty());
        assert!(screen.boss_fights_limited);
    }

    pub(crate) fn assert_plans(db: &Connection, statements: &[String]) {
        assert_eq!(statements.len(), 4);
        for (sql, index) in statements.iter().zip([
            "runs_started",
            "runs_started",
            "instance_events_run",
            "fights_run",
        ]) {
            let plan = db
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map([], |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("; ");
            assert!(plan.contains(index), "{plan}");
            assert!(!plan.contains("SCAN"), "{plan}");
            assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        }
    }

    #[test]
    fn screen_reads_use_existing_indexes() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::migrate(&mut db).unwrap();
        for (sql, args, index, table) in [
            (BOSSES, params!["demo-run", 65], "fights_run", "fights"),
            (
                EVENTS,
                params!["demo-run", 2001],
                "instance_events_run",
                "instance_events",
            ),
            (
                OLDER,
                params![1_000_000, "demo-run"],
                "runs_started",
                "runs",
            ),
            (
                NEWER,
                params![1_000_000, "demo-run"],
                "runs_started",
                "runs",
            ),
        ] {
            let mut statement = db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
            let plan = statement
                .query_map(args, |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("; ");
            println!("{table}: {plan}");
            assert!(plan.contains(index), "{plan}");
            assert!(!plan.contains(&format!("SCAN {table}")), "{plan}");
        }
    }
}
