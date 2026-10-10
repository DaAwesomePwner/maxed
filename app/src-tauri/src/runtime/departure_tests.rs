//! Cheap deterministic close/receipt regression; no perf-live workload or timing gate.
use super::*;
use crate::bridge::{payload::presentation_event, Publication, Publisher};
use crate::runtime::history_replay_tests::reconstruction as packets;

#[test]
fn mass_close_preserves_full_ordered_storage_before_bounded_presentation_batches() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("actual.sqlite3");
    let config = EngineConfig {
        stream_id: "departure-test".into(),
        ..EngineConfig::default()
    };
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let publisher = Publisher::new_batched(
        move |publication| {
            sink.lock().unwrap().push(publication);
            Ok(())
        },
        Arc::new(|| panic!("presentation failed")),
    )
    .unwrap();
    let bridge = crate::runtime::CaptureRuntime::domain_bridge(publisher.clone());
    let mut adapter = Adapter::open(
        path.clone(),
        Arc::new(packets::catalog()),
        config.clone(),
        WriterConfig {
            stream_id: config.stream_id.clone(),
            capacity: 16,
            batch_size: 16,
            ..WriterConfig::default()
        },
        false,
        Arc::new(Views::default()),
        bridge,
    )
    .unwrap();
    let mut expected = Vec::new();
    let mut feed = |adapter: &mut Adapter, at, packet| {
        let events = packets::apply(&mut adapter.engine, at, packet);
        let envelopes: Vec<_> = events
            .iter()
            .enumerate()
            .map(|(index, event)| EventEnvelope {
                fight_chart_omitted: false,
                stream_id: config.stream_id.clone(),
                sequence: Sequence(Timestamp(expected.len() as u64 + index as u64 + 1)),
                at: Timestamp(at),
                event: event.clone(),
            })
            .collect();
        adapter.events_at(events, Some(Timestamp(at)));
        assert_eq!(
            adapter
                .pending
                .iter()
                .map(|row| row.event.clone())
                .collect::<Vec<_>>(),
            envelopes,
            "storage must retain every original before projection"
        );
        expected.extend(envelopes);
        adapter.submit_pending_for_test();
    };
    feed(&mut adapter, 0, packets::map("Realm"));
    feed(&mut adapter, 1000, packets::create(1, 1));
    let mut objects = vec![packets::player()];
    objects.extend((100..2148).map(|id| {
        packets::object(
            id,
            if id == 100 { 300 } else { 301 },
            vec![packets::stat(0, 10000), packets::stat(1, 10000)],
        )
    }));
    feed(&mut adapter, 2000, packets::update(objects, vec![]));
    for id in 100..2148 {
        let maxed_protocol::Packet::Damage(mut damage) = packets::damage(1, 0, 100) else {
            unreachable!()
        };
        damage.targetId = id;
        feed(&mut adapter, 3000, maxed_protocol::Packet::Damage(damage));
    }
    assert_eq!(adapter.engine.snapshot().fights.len(), 2048);
    let maxed_protocol::Packet::Damage(mut damage) = packets::damage(1, 1, 100) else {
        unreachable!()
    };
    damage.targetId = 100;
    feed(&mut adapter, 4000, maxed_protocol::Packet::Damage(damage));
    feed(&mut adapter, 300_000, packets::tick(vec![]));
    // Keep the original full boss update in the writer's pending/receipt path,
    // while the publisher queues a marked chart-free presentation copy.
    publisher.flush().unwrap();
    let projected = {
        let publications = received.lock().unwrap();
        publications
            .iter()
            .find_map(|publication| {
                let Publication::Events(events) = publication else {
                    return None;
                };
                events
                    .iter()
                    .find(|event| event.fight_chart_omitted)
                    .cloned()
            })
            .expect("open boss update must be marked")
    };
    received.lock().unwrap().clear();
    feed(&mut adapter, 400_000, packets::map("Nexus"));
    let original = expected
        .iter()
        .find(|event| event.sequence == projected.sequence)
        .unwrap();
    assert!(!original.fight_chart_omitted);
    let EngineEvent::FightUpdated(full) = &original.event else {
        panic!("boss update required")
    };
    assert!(!full.buckets.is_empty());
    assert!(full
        .player_series
        .as_ref()
        .is_some_and(|series| !series.is_empty()));
    let EngineEvent::FightUpdated(compact) = &projected.event else {
        panic!("boss update required")
    };
    assert!(compact.buckets.is_empty());
    assert!(compact.player_series.is_none());
    adapter.writer_flush_for_test();
    publisher.flush().unwrap();
    let publications = received.lock().unwrap();
    assert_eq!(publications.len(), 3);
    let events: Vec<_> = publications
        .iter()
        .flat_map(|publication| {
            let Publication::Events(events) = publication else {
                panic!("close batches required")
            };
            assert!(events.len() <= maxed_types::EVENT_BATCH_LIMIT);
            events.iter().cloned()
        })
        .collect();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.event, EngineEvent::FightEnded(_)))
            .count(),
        2048
    );
    let original = &expected[expected.len() - events.len()..];
    assert_eq!(
        events,
        original
            .iter()
            .cloned()
            .map(presentation_event)
            .collect::<Vec<_>>(),
        "the queued publications must already be compact"
    );
    let batches = crate::bridge::payload::event_batches(events.clone());
    assert_eq!(batches.len(), 3);
    assert_eq!(
        batches
            .iter()
            .flat_map(|batch| batch.events.iter())
            .collect::<Vec<_>>(),
        events.iter().collect::<Vec<_>>()
    );
    let old_bytes: usize = events
        .iter()
        .map(|event| serde_json::to_vec(event).unwrap().len())
        .sum();
    let sizes: Vec<_> = batches
        .iter()
        .map(|batch| serde_json::to_vec(batch).unwrap().len())
        .collect();
    eprintln!(
        "2048-fight close: {} -> {} messages, {} -> {} JSON bytes; largest message {} bytes",
        events.len(),
        batches.len(),
        old_bytes,
        sizes.iter().sum::<usize>(),
        sizes.iter().max().unwrap()
    );
    // The writer's ordered receipt digests must match applying the untouched
    // engine events directly, including full fight series and alert eligibility.
    let mut oracle = rusqlite::Connection::open(temporary.path().join("oracle.sqlite3")).unwrap();
    maxed_store::migrate(&mut oracle).unwrap();
    maxed_store::apply_batch(&mut oracle, &expected).unwrap();
    let actual = rusqlite::Connection::open(&path).unwrap();
    let receipts = |db: &rusqlite::Connection| {
        db.prepare("SELECT sequence, digest FROM event_receipts ORDER BY sequence")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    assert_eq!(receipts(&actual), receipts(&oracle));
    drop(publications);
    adapter.close().unwrap();
}

#[test]
fn native_bridge_single_fallback_delivers_a_compact_batch() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let publisher = Publisher::new_batched(
        move |value| {
            sink.lock().unwrap().push(value);
            Ok(())
        },
        Arc::new(|| panic!("delivery failed")),
    )
    .unwrap();
    let bridge = crate::runtime::CaptureRuntime::domain_bridge(publisher.clone());
    let original = EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "single".into(),
        sequence: Sequence(Timestamp(1)),
        at: Timestamp(1),
        event: EngineEvent::FightEnded(maxed_types::demo::demo_snapshot().fights.remove(0)),
    };
    (bridge.event)(original.clone());
    publisher.flush().unwrap();
    let received = received.lock().unwrap();
    assert_eq!(received.len(), 1);
    let Publication::Events(events) = &received[0] else {
        panic!("single must become batch")
    };
    assert_eq!(events, &[presentation_event(original)]);
}
