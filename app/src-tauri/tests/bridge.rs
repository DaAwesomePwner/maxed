use maxed_app::{
    bridge::{Publication, Publisher, SnapshotGate},
    runtime::CaptureRuntime,
};
use maxed_types::{CaptureStatus, EngineEvent, EventEnvelope, Sequence, Timestamp};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

#[test]
fn payload_encodes_upserts_removals_and_resets_on_stream_change() {
    use maxed_app::bridge::payload::SnapshotEncoder;
    use maxed_types::{demo::demo_snapshot, LiveSnapshotUpdate};
    let mut encoder = SnapshotEncoder::default();
    let mut initial = demo_snapshot();
    initial.sequence = Sequence(Timestamp(10));
    assert!(matches!(
        encoder.encode(initial.clone()),
        LiveSnapshotUpdate::Full(_)
    ));
    let mut next = initial.clone();
    next.sequence = Sequence(Timestamp(11));
    let LiveSnapshotUpdate::Delta(delta) = encoder.encode(next.clone()) else {
        panic!("unchanged rows must be omitted")
    };
    assert_eq!(delta.base_sequence, initial.sequence);
    assert!(delta.snapshot.fights.is_empty());
    assert!(delta.snapshot.drops.is_empty());
    next.sequence = Sequence(Timestamp(12));
    next.fights[0].total_damage += 1.;
    let LiveSnapshotUpdate::Delta(delta) = encoder.encode(next.clone()) else {
        panic!("expected delta")
    };
    assert_eq!(delta.snapshot.fights, next.fights);
    next.sequence = Sequence(Timestamp(13));
    next.fights.clear();
    next.drops.clear();
    let LiveSnapshotUpdate::Delta(delta) = encoder.encode(next.clone()) else {
        panic!("expected removals")
    };
    assert_eq!(
        delta.removed_fights,
        initial
            .fights
            .iter()
            .map(|f| f.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        delta.removed_drops,
        initial
            .drops
            .iter()
            .map(|d| d.id.clone())
            .collect::<Vec<_>>()
    );
    next.stream_id = "new-generation".into();
    next.sequence = Sequence(Timestamp(0));
    assert!(matches!(encoder.encode(next), LiveSnapshotUpdate::Full(_)));
}

#[test]
fn presentation_events_omit_ordinary_series_without_changing_domain_events() {
    use maxed_app::bridge::payload::presentation_event;
    let mut fight = maxed_types::demo::demo_snapshot().fights.remove(0);
    fight.target_kind = Some(maxed_types::TargetKind::Ordinary);
    let event = EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "synthetic".into(),
        sequence: Sequence(Timestamp(1)),
        at: Timestamp(1),
        event: EngineEvent::FightUpdated(fight.clone()),
    };
    let original = event.clone();
    let view = presentation_event(event);
    let EngineEvent::FightUpdated(projected) = view.event else {
        panic!("event kind changed")
    };
    assert!(projected.buckets.is_empty());
    assert!(projected.player_series.is_none());
    assert_eq!(projected.total_damage, fight.total_damage);
    assert_eq!(original.event, EngineEvent::FightUpdated(fight));
    assert_eq!(view.sequence, original.sequence);
}

#[test]
fn first_query_is_a_stopped_consistent_snapshot() {
    let runtime = CaptureRuntime::default();
    let snapshot = runtime.snapshot();
    assert_eq!(snapshot.capture, runtime.status());
    assert_eq!(snapshot.sequence, Sequence(Timestamp(0)));
    assert!(snapshot.character.is_none());
    runtime.stop().unwrap();
    assert_eq!(runtime.snapshot(), snapshot);
}

#[test]
fn leaving_preview_always_allocates_a_fresh_stopped_stream() {
    let runtime = CaptureRuntime::default();
    let first = runtime.reset_preview().unwrap();
    let second = runtime.reset_preview().unwrap();
    assert!(first.stream_id.starts_with("stopped-"));
    assert_ne!(first.stream_id, second.stream_id);
    assert_eq!(second.sequence, Sequence(Timestamp(0)));
    assert_eq!(runtime.snapshot(), second);
}

#[test]
fn snapshots_are_due_only_when_dirty_and_after_250_monotonic_ms() {
    let mut gate = SnapshotGate::default();
    assert!(!gate.take_due(Duration::from_secs(10)));
    gate.dirty = true;
    assert!(!gate.take_due(Duration::from_millis(249)));
    assert!(gate.take_due(Duration::from_millis(250)));
    assert!(!gate.take_due(Duration::from_millis(500)));
    gate.dirty = true;
    assert!(!gate.take_due(Duration::from_millis(100)));
    assert!(gate.take_due(Duration::from_millis(500)));
}

#[test]
fn every_event_bypasses_the_gate_and_shutdown_flush_preserves_order() {
    let delivered = Arc::new(Mutex::new(vec![]));
    let values = delivered.clone();
    let publisher = Publisher::new(
        move |event| {
            let value = match event {
                Publication::Event(value) => value.sequence.0 .0,
                Publication::Events(_) => unreachable!("legacy subscriber receives envelopes"),
                Publication::Snapshot(_) => 1001,
                Publication::Capture(_) => 1002,
            };
            values.lock().unwrap().push(value);
            Ok(())
        },
        Arc::new(|| panic!("delivery failed")),
    )
    .unwrap();
    for sequence in 1..=1000 {
        publisher.publish(Publication::Event(Box::new(EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "test".into(),
            sequence: Sequence(Timestamp(sequence)),
            at: Timestamp(sequence),
            event: EngineEvent::CaptureStateChanged(CaptureStatus::default()),
        })));
    }
    publisher.publish(Publication::Snapshot(Box::new(
        CaptureRuntime::default().snapshot(),
    )));
    publisher.publish(Publication::Capture(CaptureStatus::default()));
    publisher.flush().unwrap();
    assert_eq!(*delivered.lock().unwrap(), (1..=1002).collect::<Vec<_>>());
}

#[test]
fn emission_failure_is_reported_and_following_events_are_attempted() {
    let failures = Arc::new(AtomicUsize::new(0));
    let reported = failures.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let publisher = Publisher::new(
        move |_| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(())
            } else {
                Ok(())
            }
        },
        Arc::new(move || {
            reported.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .unwrap();
    publisher.publish(Publication::Capture(CaptureStatus::default()));
    publisher.publish(Publication::Capture(CaptureStatus::default()));
    assert!(publisher.flush().is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(failures.load(Ordering::SeqCst), 1);
}

#[test]
fn event_batches_preserve_ranges_and_split_at_gaps_and_stream_resets() {
    use maxed_app::bridge::payload::event_batches;
    let event = |stream: &str, sequence| EventEnvelope {
        fight_chart_omitted: false,
        stream_id: stream.into(),
        sequence: Sequence(Timestamp(sequence)),
        at: Timestamp(100),
        event: EngineEvent::CaptureStateChanged(CaptureStatus::default()),
    };
    let events = vec![
        event("a", 10),
        event("a", 11),
        event("a", 13),
        event("b", 0),
        event("b", 1),
    ];
    let batches = event_batches(events.clone());
    assert_eq!(batches.len(), 3);
    assert_eq!(
        (batches[0].first_sequence, batches[0].last_sequence),
        (events[0].sequence, events[1].sequence)
    );
    assert_eq!(batches[1].first_sequence, events[2].sequence);
    assert_eq!(batches[2].stream_id, "b");
    assert_eq!(
        batches
            .into_iter()
            .flat_map(|batch| batch.events)
            .collect::<Vec<_>>(),
        events
    );
}

#[test]
fn mass_end_splits_into_bounded_presentation_messages_with_all_events() {
    use maxed_app::bridge::payload::{event_batches, presentation_event};
    let events: Vec<_> = (1..=2048)
        .map(|sequence| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic".into(),
            sequence: Sequence(Timestamp(sequence)),
            at: Timestamp(100),
            event: EngineEvent::FightEnded(maxed_types::demo::demo_snapshot().fights.remove(0)),
        })
        .collect();
    let batches = event_batches(events.iter().cloned().map(presentation_event).collect());
    assert_eq!(batches.len(), 2);
    assert!(batches.iter().all(|batch| batch.events.len() == 1024));
    assert_eq!(
        batches
            .iter()
            .flat_map(|batch| batch.events.iter().cloned())
            .collect::<Vec<_>>(),
        events
            .into_iter()
            .map(presentation_event)
            .collect::<Vec<_>>()
    );
    eprintln!(
        "2048 presentation events: {} bytes in two messages",
        batches
            .iter()
            .map(|batch| serde_json::to_vec(batch).unwrap().len())
            .sum::<usize>()
    );
}

#[test]
fn oversized_transactions_are_bounded_and_flush_after_the_final_range() {
    use maxed_app::bridge::payload::{event_batches, EVENT_BATCH_LIMIT};
    let received = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    let publisher = Publisher::new_batched(
        move |publication| {
            match publication {
                Publication::Events(events) => {
                    assert!(events.len() <= EVENT_BATCH_LIMIT);
                    sink.lock().unwrap().extend(event_batches(events));
                }
                _ => panic!("expected events"),
            }
            Ok(())
        },
        Arc::new(|| panic!("delivery failed")),
    )
    .unwrap();
    publisher.publish(Publication::Events(
        (1..=2049)
            .map(|sequence| EventEnvelope {
                fight_chart_omitted: false,
                stream_id: "bounded".into(),
                sequence: Sequence(Timestamp(sequence)),
                at: Timestamp(1),
                event: EngineEvent::CaptureStateChanged(CaptureStatus::default()),
            })
            .collect(),
    ));
    publisher.flush().unwrap();
    let batches = received.lock().unwrap();
    assert_eq!(batches.len(), 3);
    assert_eq!(
        batches
            .iter()
            .map(|batch| (batch.first_sequence.0 .0, batch.last_sequence.0 .0))
            .collect::<Vec<_>>(),
        [(1, 1024), (1025, 2048), (2049, 2049)]
    );
}
