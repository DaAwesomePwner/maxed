use crossbeam_channel::{bounded, Receiver, Sender};
use maxed_app::{
    bridge::{Publication, Publisher},
    demo::{DemoClock, DemoResources},
    runtime::CaptureRuntime,
};
use maxed_types::{CaptureState, EngineEvent, FightEnd, Outcome, RunFilter, RunSort};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Default)]
struct FastClock {
    offsets: Mutex<Vec<Duration>>,
}
impl DemoClock for FastClock {
    fn wait_until(&self, offset: Duration, stop: &Receiver<()>) -> bool {
        self.offsets.lock().unwrap().push(offset);
        stop.try_recv().is_err()
    }
    fn elapsed(&self) -> Duration {
        self.offsets
            .lock()
            .unwrap()
            .last()
            .copied()
            .unwrap_or_default()
    }
}
struct PausedClock {
    reached: Sender<()>,
    at: Duration,
    elapsed: Mutex<Duration>,
}
impl DemoClock for PausedClock {
    fn wait_until(&self, offset: Duration, stop: &Receiver<()>) -> bool {
        if offset >= self.at {
            self.reached.send(()).unwrap();
            let _ = stop.recv();
            false
        } else {
            *self.elapsed.lock().unwrap() = offset;
            true
        }
    }
    fn elapsed(&self) -> Duration {
        *self.elapsed.lock().unwrap()
    }
}
fn resources() -> DemoResources {
    let root = DemoResources::development_root();
    DemoResources::load(&root.join("story.pcap"), &root.join("catalog.json")).unwrap()
}
fn observer() -> (Publisher, Arc<Mutex<Vec<Publication>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let output = seen.clone();
    (
        Publisher::new(
            move |value| {
                output.lock().unwrap().push(value);
                Ok(())
            },
            Arc::new(|| panic!("delivery failed")),
        )
        .unwrap(),
        seen,
    )
}
fn eof(runtime: &CaptureRuntime) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.is_running() {
        assert!(Instant::now() < deadline, "demo failed to finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    runtime.flush_bridge().unwrap();
}
fn filter() -> RunFilter {
    RunFilter {
        character_id: None,
        dungeon: None,
        outcome: None,
        range: None,
        sort: RunSort::Newest,
        cursor: None,
        limit: 30,
    }
}

#[test]
fn full_pipeline_story_pacing_sequence_and_eof_are_durable() {
    let root = tempfile::tempdir().unwrap();
    let runtime = CaptureRuntime::default();
    let clock = Arc::new(FastClock::default());
    let (publisher, seen) = observer();
    runtime
        .start_demo(root.path().into(), resources(), clock.clone(), publisher)
        .unwrap();
    eof(&runtime);
    assert_eq!(runtime.status().state, CaptureState::Stopped);
    assert!(runtime.snapshot().session.as_ref().unwrap().ended.is_some());
    assert_eq!(
        runtime.store_status().durable,
        Some(runtime.snapshot().sequence)
    );
    runtime.stop_and_flush().unwrap();
    let seen = seen.lock().unwrap();
    let mut sequence = 0;
    let mut bags = 0;
    let mut ended = 0;
    let mut fight = None;
    let stream = runtime.snapshot().stream_id;
    assert!(stream.starts_with("demo-"));
    assert!(!runtime.external_delivery_allowed(&stream));
    assert!(!runtime.external_delivery_allowed("live-retired"));
    for publication in seen.iter() {
        match publication {
            Publication::Event(event) => {
                sequence += 1;
                assert_eq!(event.sequence.0 .0, sequence);
                assert_eq!(event.stream_id, stream);
                match &event.event {
                    EngineEvent::FightEnded(value) if value.total_damage > 0.0 => {
                        fight = Some(value)
                    }
                    EngineEvent::BagDropped(_) => bags += 1,
                    EngineEvent::RunEnded(value) if value.dungeon == "Snake Pit" => {
                        assert_eq!(value.outcome, Some(Outcome::Cleared));
                        ended += 1;
                    }
                    _ => {}
                }
            }
            Publication::Snapshot(snapshot) => {
                assert_eq!(snapshot.stream_id, stream);
                assert_eq!(snapshot.sequence.0 .0, sequence);
            }
            _ => {}
        }
    }
    let fight = fight.unwrap();
    assert_eq!(fight.total_damage, 300.0);
    let local = fight.players.iter().find(|p| p.is_local).unwrap();
    assert_eq!(local.damage, 300.0);
    assert_eq!(local.dps, None);
    assert_eq!((bags, ended), (1, 1));
    assert_eq!(runtime.store_status().durable.unwrap().0 .0, sequence);
    assert!(seen
        .iter()
        .any(|p| matches!(p, Publication::Capture(s) if s.state == CaptureState::Live)));
    assert!(seen.iter().any(|p| matches!(p, Publication::Snapshot(s) if s.fights.iter().any(|f| f.total_damage == 300.0))));
    let reader = maxed_store::open_reader(&root.path().join("demo.sqlite3")).unwrap();
    let runs = reader.list_runs(filter()).unwrap();
    assert_eq!(runs.rows.len(), 1);
    let run = reader.get_run(&runs.rows[0].id).unwrap().unwrap();
    assert_eq!(run.run.outcome, Some(Outcome::Cleared));
    let offsets = clock.offsets.lock().unwrap();
    assert_eq!(offsets.first(), Some(&Duration::ZERO));
    assert_eq!(offsets.last(), Some(&Duration::from_secs(12)));
    assert!(offsets.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn demo_with_corrupt_settings_uses_ephemeral_policy_and_restores_live_history() {
    let root = tempfile::tempdir().unwrap();
    let settings_path = root.path().join("settings.json");
    let corrupt = b"{corrupt settings preserved for repair}";
    std::fs::write(&settings_path, corrupt).unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime
        .configure_alert_settings(Arc::new(
            maxed_app::settings::SettingsStore::new(root.path().into()).unwrap(),
        ))
        .unwrap();
    runtime.initialize_home(root.path().into()).unwrap();
    let (publisher, _) = observer();
    runtime
        .start_demo(
            root.path().into(),
            resources(),
            Arc::new(FastClock::default()),
            publisher,
        )
        .unwrap();
    eof(&runtime);
    assert!(runtime.snapshot().stream_id.starts_with("demo-"));
    assert_eq!(
        runtime
            .update_alert_preferences(Default::default())
            .unwrap_err(),
        "Live alert preferences cannot be changed during preview or replay."
    );
    runtime.stop_and_flush().unwrap();
    runtime.reset_preview().unwrap();
    let result = tauri::async_runtime::block_on(runtime.list_runs(filter())).unwrap();
    assert!(matches!(result, maxed_types::HistoryReply::Ready { value } if value.rows.is_empty()));
    assert_eq!(
        runtime
            .update_alert_preferences(Default::default())
            .unwrap_err(),
        maxed_app::settings::SETTINGS_ERROR
    );
    assert_eq!(std::fs::read(settings_path).unwrap(), corrupt);
}

struct PacedClock {
    elapsed: Mutex<Duration>,
    waits: Mutex<Vec<Duration>>,
    publisher: Publisher,
    seen: Arc<Mutex<Vec<Publication>>>,
    hundred_before_eight: Mutex<Option<bool>>,
}
impl DemoClock for PacedClock {
    fn wait_until(&self, offset: Duration, stop: &Receiver<()>) -> bool {
        self.publisher.flush().unwrap();
        if offset >= Duration::from_secs(8) {
            self.hundred_before_eight
                .lock()
                .unwrap()
                .get_or_insert_with(|| {
                    self.seen.lock().unwrap().iter().any(|p| {
                        matches!(p,
                    Publication::Snapshot(s) if s.fights.iter().any(|f|
                        f.run_id.is_some() && f.total_damage == 100.0))
                    })
                });
        }
        self.waits.lock().unwrap().push(offset);
        *self.elapsed.lock().unwrap() = offset;
        stop.try_recv().is_err()
    }
    fn elapsed(&self) -> Duration {
        *self.elapsed.lock().unwrap()
    }
}

#[test]
fn paced_waits_publish_the_first_hit_before_the_second_hit_is_due() {
    let root = tempfile::tempdir().unwrap();
    let runtime = CaptureRuntime::default();
    let (publisher, seen) = observer();
    let clock = Arc::new(PacedClock {
        elapsed: Mutex::new(Duration::ZERO),
        waits: Mutex::new(vec![]),
        publisher: publisher.clone(),
        seen,
        hundred_before_eight: Mutex::new(None),
    });
    runtime
        .start_demo(root.path().into(), resources(), clock.clone(), publisher)
        .unwrap();
    eof(&runtime);
    runtime.stop_and_flush().unwrap();
    assert_eq!(
        *clock.hundred_before_eight.lock().unwrap(),
        Some(true),
        "100 damage must be published before the wait for the 8-second story beat"
    );
    let waits = clock.waits.lock().unwrap();
    assert!(
        waits
            .windows(2)
            .all(|pair| pair[1] - pair[0] <= Duration::from_millis(250)),
        "presentation must get a tick at least every 250 ms between frames"
    );
}

#[test]
fn stop_mid_replay_joins_finalizes_once_and_never_delivers_late_frames() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime
        .configure_alert_settings(Arc::new(
            maxed_app::settings::SettingsStore::new(root.path().into()).unwrap(),
        ))
        .unwrap();
    let (reached, wait) = bounded(1);
    let clock = Arc::new(PausedClock {
        reached,
        at: Duration::from_secs(7),
        elapsed: Default::default(),
    });
    let (publisher, seen) = observer();
    runtime
        .start_demo(root.path().into(), resources(), clock, publisher)
        .unwrap();
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(runtime.is_running());
    let maxed_types::HistoryReply::Ready { value: party } =
        tauri::async_runtime::block_on(runtime.get_party_live()).unwrap()
    else {
        panic!("demo Party is not ready");
    };
    assert!(party.assets_available);
    runtime.stop_and_flush().unwrap();
    assert!(!runtime.is_running());
    assert_eq!(runtime.status().state, CaptureState::Stopped);
    let count = seen.lock().unwrap().len();
    runtime.stop_and_flush().unwrap();
    assert_eq!(count, seen.lock().unwrap().len());
    let seen = seen.lock().unwrap();
    let ends: Vec<_> = seen
        .iter()
        .filter_map(|p| match p {
            Publication::Event(e) => match &e.event {
                EngineEvent::FightEnded(f) if f.run_id.is_some() => Some(f),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(ends.len(), 1, "{ends:?}");
    assert_eq!(ends[0].end_reason, Some(FightEnd::CaptureStopped));
    assert_eq!(ends[0].total_damage, 100.0);
    assert!(!ends[0]
        .issues
        .contains(&maxed_types::CoverageIssue::StreamGap));
    let reader = maxed_store::open_reader(&root.path().join("demo.sqlite3")).unwrap();
    let run = reader.list_runs(filter()).unwrap().rows.remove(0);
    assert_eq!(run.outcome, Some(Outcome::Unknown));
    assert!(!run.issues.contains(&maxed_types::CoverageIssue::StreamGap));
    assert!(!run
        .evidence
        .iter()
        .any(|e| e.evidence == maxed_types::Evidence::CaptureGap));
    assert!(!seen.iter().any(
        |p| matches!(p, Publication::Event(e) if matches!(e.event, EngineEvent::BagDropped(_)))
    ));
}

#[test]
fn repeated_replay_uses_new_epochs_and_never_touches_live_database() {
    let root = tempfile::tempdir().unwrap();
    let live = root.path().join("live.sqlite3");
    maxed_store::start_writer(live.clone(), Default::default())
        .unwrap()
        .shutdown()
        .unwrap();
    let before = std::fs::read(&live).unwrap();
    let runtime = CaptureRuntime::default();
    let mut streams = std::collections::HashSet::new();
    for _ in 0..3 {
        let (publisher, _) = observer();
        runtime
            .start_demo(
                root.path().into(),
                resources(),
                Arc::new(FastClock::default()),
                publisher,
            )
            .unwrap();
        eof(&runtime);
        runtime.stop_and_flush().unwrap();
        assert!(streams.insert(runtime.snapshot().stream_id));
        let reset = runtime.reset_preview().unwrap();
        assert!(reset.stream_id.starts_with("stopped-"));
        assert!(reset.character.is_none() && reset.drops.is_empty() && reset.fights.is_empty());
        assert_eq!(std::fs::read(&live).unwrap(), before);
    }
    assert!(!root.path().join("live.lock").exists());
    assert!(!root.path().join("identity.key").exists());
}

#[test]
fn missing_unsupported_and_truncated_resources_are_safe_errors() {
    let root = tempfile::tempdir().unwrap();
    let catalog = DemoResources::development_root().join("catalog.json");
    assert!(DemoResources::load(&root.path().join("missing.pcap"), &catalog).is_err());
    assert!(DemoResources::load(
        &DemoResources::development_root().join("story.pcap"),
        Path::new("missing-catalog.json")
    )
    .is_err());
    let invalid = root.path().join("unsupported.pcap");
    std::fs::write(&invalid, b"unsupported synthetic fixture").unwrap();
    let error = DemoResources::load(&invalid, &catalog).err().unwrap();
    assert_eq!(error, "Synthetic demo format is unsupported");
    let mut story = std::fs::read(DemoResources::development_root().join("story.pcap")).unwrap();
    story.pop();
    std::fs::write(&invalid, story).unwrap();
    assert_eq!(
        DemoResources::load(&invalid, &catalog).err().unwrap(),
        "Synthetic demo data is invalid"
    );
}

#[derive(Default)]
struct MissingCapture(std::sync::atomic::AtomicUsize);
impl maxed_app::setup::CaptureBackend for MissingCapture {
    fn probe(&self) -> Result<(), maxed_capture::CaptureError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(maxed_capture::CaptureError::Missing)
    }
    fn adapters(&self) -> Result<Vec<maxed_capture::AdapterInfo>, maxed_capture::CaptureError> {
        panic!("demo must not enumerate adapters")
    }
    fn start(
        &self,
        _: maxed_capture::CaptureConfig,
    ) -> Result<maxed_capture::CaptureHandle, maxed_capture::CaptureError> {
        panic!("demo must not open capture")
    }
}
struct DemoLifecycle {
    runtime: Arc<CaptureRuntime>,
    root: std::path::PathBuf,
    publisher: Publisher,
    clock: Arc<dyn DemoClock>,
    fail_after_start: std::sync::atomic::AtomicBool,
}
impl maxed_app::setup::Lifecycle for DemoLifecycle {
    fn running(&self) -> bool {
        self.runtime.is_running()
    }
    fn live(&self) -> bool {
        self.runtime.status().state == CaptureState::Live
    }
    fn stop(&self) -> Result<(), String> {
        self.runtime.stop_and_flush()
    }
    fn start(
        &self,
        _: Option<maxed_assets::CachedCatalog>,
        _: maxed_capture::CaptureHandle,
    ) -> Result<(), String> {
        panic!("missing Npcap cannot start live")
    }
    fn demo(&self, enabled: bool) -> Result<(), String> {
        if enabled {
            self.runtime.start_demo(
                self.root.clone(),
                resources(),
                self.clock.clone(),
                self.publisher.clone(),
            )?;
            if self
                .fail_after_start
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                return Err("Synthetic publication failure after launch".into());
            }
        } else {
            self.publisher.publish(Publication::Snapshot(Box::new(
                self.runtime.reset_preview()?,
            )));
        }
        self.publisher.flush()
    }
}

#[test]
fn setup_transition_keeps_physical_checks_settings_and_sources_separate() {
    use std::sync::atomic::Ordering;
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let backend = Arc::new(MissingCapture::default());
    let (publisher, seen) = observer();
    let (reached, wait) = bounded(1);
    let lifecycle = Arc::new(DemoLifecycle {
        runtime: runtime.clone(),
        root: root.path().into(),
        publisher,
        clock: Arc::new(PausedClock {
            reached,
            at: Duration::from_secs(7),
            elapsed: Default::default(),
        }),
        fail_after_start: Default::default(),
    });
    let setup = Arc::new(
        maxed_app::setup::SetupService::with_discovery_roots(
            root.path().into(),
            backend.clone(),
            lifecycle,
            Arc::new(maxed_app::assets_bridge::AssetBridge::default()),
            vec![],
        )
        .unwrap(),
    );
    setup.set_demo(true).unwrap();
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(runtime.status().state, CaptureState::Live);
    let status = setup.status();
    assert!(status.demo);
    assert!(!status.live_confirmed);
    assert_eq!(status.npcap, maxed_types::NpcapCheck::Unchecked);
    assert_eq!(status.access, maxed_types::CaptureAccess::Unknown);
    let demo_stream = runtime.snapshot().stream_id;
    assert!(!runtime.external_delivery_allowed(&demo_stream));
    assert_eq!(
        setup
            .set_adapter(Some("saved-live-adapter".into()))
            .unwrap_err(),
        "Leave demo before changing the live capture adapter."
    );
    let settings = setup.settings.get().unwrap();
    assert_eq!(settings.adapter_id, None);
    assert_eq!(runtime.snapshot().stream_id, demo_stream);
    setup.query_with_retry(true);
    assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    setup.set_demo(false).unwrap();
    assert!(!setup.status().demo);
    assert!(!setup.status().live_confirmed);
    assert_eq!(runtime.status().state, CaptureState::Stopped);
    assert_eq!(setup.settings.get().unwrap(), settings);
    assert_eq!(backend.0.load(Ordering::SeqCst), 0);
    let seen = seen.lock().unwrap();
    assert!(
        matches!(seen.iter().rev().find(|p| !matches!(p, Publication::PartyChanged(_))), Some(Publication::Snapshot(s)) if s.stream_id.starts_with("stopped-") && s.character.is_none())
    );
    drop(seen);
    // Explicit Start is the only operation here that probes the physical source.
    assert!(setup.start().unwrap_err().contains("Npcap is missing"));
    assert_eq!(backend.0.load(Ordering::SeqCst), 1);
    assert!(!runtime.is_running());
    assert_eq!(setup.status().npcap, maxed_types::NpcapCheck::Missing);
    // Explicit Start while demo is active must join and clear it before probing.
    setup.set_demo(true).unwrap();
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(setup.start().is_err());
    assert!(!setup.status().demo);
    assert!(!runtime.is_running());
    assert!(runtime.snapshot().stream_id.starts_with("stopped-"));
    assert_eq!(setup.settings.get().unwrap(), settings);
}

#[test]
fn failure_after_launch_joins_worker_before_clearing_demo_marker() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let (publisher, _) = observer();
    let (reached, _wait) = bounded(1);
    let lifecycle = Arc::new(DemoLifecycle {
        runtime: runtime.clone(),
        root: root.path().into(),
        publisher,
        clock: Arc::new(PausedClock {
            reached,
            at: Duration::ZERO,
            elapsed: Default::default(),
        }),
        fail_after_start: std::sync::atomic::AtomicBool::new(true),
    });
    let setup = maxed_app::setup::SetupService::with_discovery_roots(
        root.path().into(),
        Arc::new(MissingCapture::default()),
        lifecycle,
        Arc::new(maxed_app::assets_bridge::AssetBridge::default()),
        vec![],
    )
    .unwrap();
    assert_eq!(
        setup.set_demo(true).unwrap_err(),
        "Synthetic publication failure after launch"
    );
    assert!(!runtime.is_running());
    assert!(!setup.status().demo);
    assert!(!setup.status().live_confirmed);
    assert!(runtime.snapshot().stream_id.starts_with("stopped-"));
    assert_eq!(runtime.status().state, CaptureState::Stopped);
}

#[test]
fn leaving_demo_after_failed_startup_upgrade_restores_live() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("live.sqlite3"), b"invalid history").unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    let scope = maxed_types::HomeScope::Today {
        range: maxed_types::TimeRange {
            start: maxed_types::Timestamp(0),
            end: maxed_types::Timestamp(10_000_000),
        },
    };
    let error = tauri::async_runtime::block_on(runtime.home_summary(scope.clone())).unwrap_err();
    assert_eq!(error, "History schema upgrade failed; close other Maxed instances or resolve local storage access, then Retry.");
    let (publisher, seen) = observer();
    let (reached, wait) = bounded(1);
    let setup = maxed_app::setup::SetupService::with_discovery_roots(
        root.path().into(),
        Arc::new(MissingCapture::default()),
        Arc::new(DemoLifecycle {
            runtime: runtime.clone(),
            root: root.path().into(),
            publisher,
            clock: Arc::new(PausedClock {
                reached,
                at: Duration::from_secs(7),
                elapsed: Default::default(),
            }),
            fail_after_start: Default::default(),
        }),
        Arc::new(maxed_app::assets_bridge::AssetBridge::default()),
        vec![],
    )
    .unwrap();
    setup.set_demo(true).unwrap();
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(setup.status().demo);
    let preview = runtime.snapshot();
    assert!(preview.character.is_some());
    assert!(matches!(
        tauri::async_runtime::block_on(runtime.home_summary(scope.clone())).unwrap(),
        maxed_types::HistoryReply::Ready { .. }
    ));

    setup.set_demo(false).unwrap();
    assert!(!setup.status().demo);
    assert!(!runtime.is_running());
    let live = runtime.snapshot();
    assert_ne!(live.stream_id, preview.stream_id);
    assert!(live.stream_id.starts_with("stopped-"));
    assert!(live.character.is_none());
    assert!(live.run.is_none());
    assert!(live.fights.is_empty());
    assert!(
        matches!(seen.lock().unwrap().iter().rev().find(|p| !matches!(p, Publication::PartyChanged(_))), Some(Publication::Snapshot(s)) if **s == live)
    );
    // The failed live history gate is restored instead of retaining demo rows.
    assert_eq!(
        tauri::async_runtime::block_on(runtime.home_summary(scope)).unwrap_err(),
        error
    );
    setup.stop().unwrap();
}

#[test]
fn recording_defaults_off_and_no_files_for_demo() {
    for opted_in in [false, true] {
        let root = tempfile::tempdir().unwrap();
        if opted_in {
            let settings = maxed_types::AppSettings {
                save_recordings: true,
                ..Default::default()
            };
            maxed_app::settings::SettingsStore::new(root.path().into())
                .unwrap()
                .put(&settings)
                .unwrap();
        }
        let runtime = CaptureRuntime::default();
        let (publisher, _) = observer();
        runtime
            .start_demo(
                root.path().into(),
                resources(),
                Arc::new(FastClock::default()),
                publisher,
            )
            .unwrap();
        eof(&runtime);
        runtime.stop_and_flush().unwrap();
        assert!(runtime.snapshot().sequence.0 .0 > 0);
        assert!(!root.path().join("recordings").exists());
    }
}
