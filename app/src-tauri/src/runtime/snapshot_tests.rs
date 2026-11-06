//! Synthetic packet and injected-clock coverage for live snapshot scheduling.
use super::*;
use crate::runtime::{history_replay_tests::reconstruction as packets, CaptureClock};
use maxed_protocol::{DecodedPacket, Packet};
use std::time::{Duration, Instant};

const CAPTURE_START: u64 = 1_700_000_000_000_000;

fn feed(adapter: &mut Adapter, micros: u64, packet: Packet) {
    adapter
        .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(DecodedPacket {
            conn: ConnId(1),
            dir: Direction::ServerToClient,
            at: Timestamp(CAPTURE_START + micros),
            id: 0,
            len: 0,
            alignment_verified: true,
            packet,
        })))
        .unwrap();
}

fn open(
    path: PathBuf,
) -> (
    Adapter,
    crossbeam_channel::Receiver<LiveSnapshot>,
    crossbeam_channel::Receiver<EventEnvelope>,
) {
    let (snapshot_send, snapshots) = crossbeam_channel::unbounded();
    let (event_send, events) = crossbeam_channel::unbounded();
    let adapter = Adapter::open(
        path,
        Arc::new(packets::catalog()),
        EngineConfig::default(),
        WriterConfig {
            stream_id: "engine".into(),
            ..Default::default()
        },
        false,
        Arc::new(Views::default()),
        Bridge {
            snapshot: Arc::new(move |snapshot| snapshot_send.send(snapshot).unwrap()),
            batch: Some(Arc::new(move |batch| {
                for event in batch {
                    event_send.send(event).unwrap();
                }
            })),
            ..Default::default()
        },
    )
    .unwrap();
    snapshots.try_recv().unwrap(); // Initial baseline.
    (adapter, snapshots, events)
}

#[test]
fn live_clock_expires_recent_hits_without_packets_and_resumes_on_new_damage() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, snapshots, _events) = open(dir.path().join("cadence"));
    feed(&mut adapter, 0, packets::map("Realm"));
    feed(
        &mut adapter,
        0,
        packets::update(vec![packets::target()], vec![]),
    );
    feed(&mut adapter, 0, packets::damage(1, 1, 100));
    let arrival = Instant::now();
    let mut clock = CaptureClock::default();
    clock.frame(adapter.at(), arrival);

    for millis in [250, 4750] {
        // Exercise cadence with a pending snapshot, independently of tick events.
        adapter.gate.dirty = true;
        let elapsed = Duration::from_millis(millis);
        clock.service_tick(&mut adapter, arrival + elapsed).unwrap();
        clock.publish_due(&mut adapter, elapsed, arrival + elapsed);
        snapshots.try_recv().unwrap();
    }
    adapter.gate.dirty = true;
    for millis in [5000, 5250, 5500] {
        let elapsed = Duration::from_millis(millis);
        clock.service_tick(&mut adapter, arrival + elapsed).unwrap();
        clock.publish_due(&mut adapter, elapsed, arrival + elapsed);
        assert!(snapshots.try_recv().is_err(), "idle at {millis} ms");
    }
    assert!(adapter.engine.has_active_fight(), "the fight remains open");
    let elapsed = Duration::from_millis(5750);
    clock.service_tick(&mut adapter, arrival + elapsed).unwrap();
    clock.publish_due(&mut adapter, elapsed, arrival + elapsed);
    snapshots.try_recv().unwrap();

    // A pending non-damage update must resume the fast cadence after a new hit.
    adapter.gate.dirty = true;
    feed(&mut adapter, 5_800_000, packets::tick(vec![]));
    feed(&mut adapter, 5_800_000, packets::damage(1, 2, 100));
    clock.frame(adapter.at(), arrival + Duration::from_millis(5800));
    let elapsed = Duration::from_secs(6);
    clock.service_tick(&mut adapter, arrival + elapsed).unwrap();
    clock.publish_due(&mut adapter, elapsed, arrival + elapsed);
    snapshots.try_recv().unwrap();
    adapter
        .finish(adapter.at(), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
}

#[test]
fn empty_live_ticks_do_not_build_or_publish_snapshots() {
    for in_nexus in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut adapter, snapshots, events) = open(dir.path().join("idle"));
        if in_nexus {
            feed(&mut adapter, 0, packets::map("Nexus"));
            adapter.publish_snapshot();
            snapshots.try_recv().unwrap();
        }
        events.try_iter().for_each(drop);
        let builds = adapter.snapshot_builds;
        let arrival = Instant::now();
        let mut clock = CaptureClock::default();
        clock.frame(adapter.at(), arrival);
        for tick in 1..=40 {
            let elapsed = Duration::from_millis(tick * 250);
            clock.service_tick(&mut adapter, arrival + elapsed).unwrap();
            assert!(events.try_recv().is_err(), "the tick must be event-free");
            clock.publish_due(&mut adapter, elapsed, arrival + elapsed);
            assert!(snapshots.try_recv().is_err(), "idle tick {tick}");
            assert_eq!(adapter.snapshot_builds, builds, "no idle projection work");
        }
        adapter
            .finish(adapter.at(), FightEnd::CaptureStopped)
            .unwrap();
        adapter.close().unwrap();
    }
}

#[test]
fn loot_and_fight_end_bypass_a_waiting_snapshot_gate() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, snapshots, events) = open(dir.path().join("events"));
    feed(&mut adapter, 0, packets::map("Realm"));
    feed(
        &mut adapter,
        0,
        packets::update(vec![packets::target()], vec![]),
    );
    feed(&mut adapter, 0, packets::damage(1, 1, 100));
    adapter.publish_due(Duration::from_millis(250));
    snapshots.try_recv().unwrap();
    events.try_iter().for_each(drop);

    feed(
        &mut adapter,
        1000,
        packets::update(vec![packets::object(3, 1292, vec![])], vec![]),
    );
    assert!(events
        .try_iter()
        .any(|event| matches!(event.event, EngineEvent::BagDropped(_))));
    adapter.publish_due(Duration::from_millis(251));
    assert!(snapshots.try_recv().is_err());

    feed(&mut adapter, 2000, packets::map("Nexus"));
    assert!(events
        .try_iter()
        .any(|event| matches!(event.event, EngineEvent::FightEnded(_))));
    adapter.publish_due(Duration::from_millis(252));
    assert!(snapshots.try_recv().is_err());
    adapter
        .finish(adapter.at(), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
}
