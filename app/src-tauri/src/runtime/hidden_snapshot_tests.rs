//! Hidden live workers defer projections, but keep ordered events and fresh reads.
use super::*;
use crate::{bridge::Publication, runtime::CaptureRuntime};
use std::time::Duration;

fn open(
    path: PathBuf,
) -> (
    Adapter,
    crate::bridge::Publisher,
    crossbeam_channel::Receiver<Publication>,
) {
    let (send, received) = crossbeam_channel::unbounded();
    let publisher = crate::bridge::Publisher::new_batched(
        move |publication| send.send(publication).map_err(|_| ()),
        Arc::new(|| panic!("publication failed")),
    )
    .unwrap();
    let adapter = Adapter::open(
        path,
        unavailable_catalog(),
        EngineConfig::default(),
        WriterConfig {
            stream_id: "engine".into(),
            ..Default::default()
        },
        true,
        Arc::new(Views::default()),
        CaptureRuntime::domain_bridge(publisher.clone()),
    )
    .unwrap();
    publisher.flush().unwrap();
    received.try_iter().for_each(drop);
    (adapter, publisher, received)
}

#[test]
fn hidden_live_builds_wait_for_show_while_events_and_direct_reads_continue() {
    for restore_read in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut adapter, publisher, received) = open(dir.path().join("hidden"));
        adapter.live = true;
        adapter.live_traffic(Timestamp(1)).unwrap();
        adapter.publish_due(Duration::from_secs(1));
        publisher.flush().unwrap();
        received.try_iter().for_each(drop);
        let visibility = publisher.visibility();
        visibility.set_visible(false);
        let builds = adapter.snapshot_builds;

        adapter.live_traffic(Timestamp(32_000_000)).unwrap();
        for second in 2..=10 {
            adapter.publish_due(Duration::from_secs(second));
            assert_eq!(
                adapter.snapshot_builds, builds,
                "hidden ticks do not project"
            );
        }
        publisher.flush().unwrap();
        let hidden: Vec<_> = received.try_iter().collect();
        assert!(hidden.iter().any(|publication| matches!(publication,
            Publication::Events(events) if events.iter().any(|event|
                matches!(event.event, EngineEvent::SessionUpdated(_))))));
        assert!(!hidden.iter().any(|p| matches!(p, Publication::Snapshot(_))));
        assert!(adapter.gate.dirty);
        assert_eq!(adapter.gate.last, Duration::from_secs(1));

        let direct = adapter.read_snapshot();
        assert_eq!(direct.session.unwrap().last_traffic, Timestamp(32_000_000));
        assert_eq!(adapter.snapshot_builds, builds + 1);
        assert!(
            adapter.gate.dirty,
            "direct reads do not consume publication"
        );
        publisher.flush().unwrap();
        assert!(received.try_recv().is_err(), "direct reads do not publish");

        // This metadata-only update must be included in the restored baseline.
        adapter.live_traffic(Timestamp(33_000_000)).unwrap();
        visibility.set_visible(true);
        if restore_read {
            let snapshot = adapter.read_snapshot();
            publisher.publish(Publication::Snapshot(Box::new(snapshot)));
        } else {
            adapter.publish_due(Duration::from_secs(10));
        }
        publisher.flush().unwrap();
        let restored = received
            .try_iter()
            .find_map(|p| match p {
                Publication::Snapshot(snapshot) => Some(snapshot),
                _ => None,
            })
            .expect("show delivers a fresh full snapshot");
        assert_eq!(
            restored.session.unwrap().last_traffic,
            Timestamp(33_000_000)
        );
        assert_eq!(adapter.snapshot_builds, builds + 2);
        // The restore read must leave the pending scheduled publication intact.
        if restore_read {
            assert!(adapter.gate.dirty);
            adapter.publish_due(Duration::from_secs(10));
            publisher.flush().unwrap();
            assert!(received
                .try_iter()
                .any(|p| matches!(p, Publication::Snapshot(_))));
            assert_eq!(adapter.snapshot_builds, builds + 2);
        }
        assert!(!adapter.gate.dirty);
        adapter
            .finish(adapter.at(), FightEnd::CaptureStopped)
            .unwrap();
        adapter.close().unwrap();
        publisher.flush().unwrap();
    }
}

#[test]
fn hidden_preview_keeps_its_gate_paced_cache_fresh() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, publisher, received) = open(dir.path().join("preview"));
    assert!(!adapter.live);
    publisher.visibility().set_visible(false);
    adapter.traffic(Timestamp(1)).unwrap();
    let builds = adapter.snapshot_builds;
    adapter.publish_due(Duration::from_secs(1));
    assert_eq!(adapter.snapshot_builds, builds + 1);
    assert_eq!(
        adapter
            .views
            .snapshot
            .lock()
            .unwrap()
            .session
            .as_ref()
            .unwrap()
            .last_traffic,
        Timestamp(1)
    );
    publisher.flush().unwrap();
    assert!(!received
        .try_iter()
        .any(|p| matches!(p, Publication::Snapshot(_))));
    adapter
        .finish(adapter.at(), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
    publisher.flush().unwrap();
}
