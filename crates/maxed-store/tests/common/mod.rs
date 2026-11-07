#![allow(dead_code)]
pub mod chat;
pub mod installed_schema;
pub mod loot;
mod receipt_support;
pub use receipt_support::{downgrade, receipts};
pub mod stats;
use maxed_types::*;

pub fn envelope(sequence: u64, event: EngineEvent) -> EventEnvelope {
    EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "test-stream".into(),
        sequence: Sequence(Timestamp(sequence)),
        at: Timestamp(6_000_000),
        event,
    }
}

pub fn story_events() -> Vec<EventEnvelope> {
    let snapshot = demo::demo_snapshot();
    let story = demo::demo_run_story();
    [
        EngineEvent::SessionUpdated(snapshot.session.unwrap()),
        EngineEvent::CharacterUpdated(story.character.unwrap()),
        EngineEvent::RunStarted(story.run),
        EngineEvent::FightStarted(snapshot.fights[0].clone()),
        EngineEvent::BagDropped(story.drops[0].clone()),
        EngineEvent::FameChanged {
            at: Timestamp(1_000_000),
            character_id: "demo-character".into(),
            fame: 1250,
        },
        EngineEvent::FameChanged {
            at: Timestamp(5_000_000),
            character_id: "demo-character".into(),
            fame: 1260,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, e)| envelope(i as u64 + 1, e))
    .collect()
}

/// Seed a pre-Party schema without asking the current projector to rebuild
/// ended-run members. Restore the legacy run's exact end time before migrating.
pub fn apply_legacy_story(db: &mut rusqlite::Connection, events: &[EventEnvelope]) {
    let mut events = events.to_vec();
    let mut endings = vec![];
    for envelope in &mut events {
        if let EngineEvent::RunStarted(run) = &mut envelope.event {
            if let Some(ended) = run.ended.take() {
                endings.push((run.id.clone(), ended));
            }
        }
    }
    apply_legacy_batch(db, &events);
    for (id, ended) in endings {
        db.execute(
            "UPDATE runs SET ended=?2,json=json_set(json,'$.ended',?3) WHERE id=?1",
            rusqlite::params![id, i64::try_from(ended.0).unwrap(), ended.0.to_string()],
        )
        .unwrap();
    }
}

pub fn apply_legacy_batch(db: &mut rusqlite::Connection, events: &[EventEnvelope]) {
    db.execute_batch(include_str!(
        "../../migrations/016_maintenance_receipts.sql"
    ))
    .unwrap();
    db.execute_batch(include_str!("../../migrations/017_accounts.sql"))
        .unwrap();
    maxed_store::apply_batch(db, events).unwrap();
    downgrade::remove_maintenance_schema(db);
}

pub fn filter(sort: RunSort, limit: u16) -> RunFilter {
    RunFilter {
        character_id: None,
        dungeon: None,
        outcome: None,
        range: None,
        sort,
        cursor: None,
        limit,
    }
}

/// Model the adapter's ownership-preserving backpressure behavior. Contention
/// may legitimately return Full even before the queue reaches its capacity.
pub fn submit(writer: &maxed_store::StoreWriter, event: EventEnvelope) {
    submit_with_context_retrying(writer, event, maxed_store::AlertContext::default());
}

/// Retry only transient backpressure, preserving the event and alert context.
/// Do not flush here: callers own their fixture's commit boundaries.
pub fn submit_with_context_retrying(
    writer: &maxed_store::StoreWriter,
    mut event: EventEnvelope,
    context: maxed_store::AlertContext,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match writer.try_submit_with_alert_context(event, context.clone()) {
            Ok(()) => return,
            Err(maxed_store::SubmitError::Full(returned)) => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "synthetic submission timed out after 10s on Full: {returned:?}"
                );
                event = returned;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => panic!("synthetic submission failed: {error:?}"),
        }
    }
}

pub mod account_scope_fixture;

#[path = "../../src/account_sql.rs"]
pub mod account_sql;
