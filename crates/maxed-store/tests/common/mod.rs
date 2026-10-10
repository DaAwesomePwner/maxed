#![allow(dead_code)]
pub mod chat;
pub mod loot;
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
pub fn submit(writer: &maxed_store::StoreWriter, mut event: EventEnvelope) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match writer.try_submit(event) {
            Ok(()) => return,
            Err(maxed_store::SubmitError::Full(returned))
                if std::time::Instant::now() < deadline =>
            {
                event = returned;
                std::thread::yield_now();
            }
            Err(error) => panic!("synthetic submission failed: {error:?}"),
        }
    }
}
