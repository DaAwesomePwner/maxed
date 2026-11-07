use crossbeam_channel::{unbounded, Receiver, Sender};
use maxed_app::{
    assets_bridge::{AssetBridge, AssetLoader},
    bridge::{Publication, Publisher},
    demo::DemoResources,
    runtime::{CaptureRuntime, RuntimeOptions},
    setup::{CaptureBackend, Lifecycle, SetupService},
};
use maxed_assets::{AssetError, AssetPaths, AssetProgress, CachedCatalog};
use maxed_capture::{
    backend::{start_with_backend, Backend, Device},
    CaptureConfig, CaptureError, CaptureHandle,
};
use maxed_protocol::{key_bytes, Rc4};
use maxed_types::*;
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use crate::fixture;

fn history_ready<T: std::fmt::Debug>(reply: Result<HistoryReply<T>, String>) -> T {
    match reply.unwrap() {
        HistoryReply::Ready { value } => value,
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[test]
fn history_commands_match_durable_live_story_after_stop() {
    let h = Harness::new(true);
    h.service.start().unwrap();
    h.story_to(12_000_000);
    h.runtime.stop_and_flush().unwrap();
    let before = h.runtime.store_status();
    let reader = h.reader();
    let page = history_ready(tauri::async_runtime::block_on(
        h.runtime.list_runs(filter()),
    ));
    assert_eq!(page, reader.list_runs(filter()).unwrap());
    assert!(!page.rows.is_empty());
    let story = history_ready(tauri::async_runtime::block_on(
        h.runtime.get_run(page.rows[0].id.clone()),
    ))
    .unwrap();
    assert_eq!(
        Some(story.clone()),
        reader.get_run(&page.rows[0].id).unwrap()
    );
    assert!(!story.fights.is_empty());
    let fight = history_ready(tauri::async_runtime::block_on(
        h.runtime.get_fight(story.fights[0].id.clone()),
    ))
    .unwrap();
    assert_eq!(Some(fight.clone()), reader.get_fight(&fight.id).unwrap());
    assert!(!fight.players.is_empty());
    let object_id = fight.players[0].object_id;
    assert_eq!(
        history_ready(tauri::async_runtime::block_on(
            h.runtime.get_player_breakdown(fight.id.clone(), object_id),
        )),
        reader.get_player_breakdown(&fight.id, object_id).unwrap(),
    );
    let player_id = fight.players[0].player_id.clone();
    let run_id = page.rows[0].id.clone();
    assert!(reader
        .get_player_context(&run_id, &player_id, None, None)
        .unwrap()
        .is_some());
    assert_eq!(
        history_ready(tauri::async_runtime::block_on(
            h.runtime.get_run_player(run_id.clone(), player_id.clone())
        )),
        reader.get_run_player(&run_id, &player_id).unwrap()
    );
    assert_eq!(
        history_ready(tauri::async_runtime::block_on(
            h.runtime
                .get_player_context(run_id.clone(), player_id.clone(), None, None)
        )),
        reader
            .get_player_context(&run_id, &player_id, None, None)
            .unwrap()
    );
    let scope = HomeScope::Today {
        range: TimeRange {
            start: Timestamp(0),
            end: Timestamp(i64::MAX as u64),
        },
    };
    assert_eq!(
        history_ready(tauri::async_runtime::block_on(
            h.runtime.home_summary(scope.clone())
        )),
        reader
            .home_summary(scope, reader.current_account().unwrap().as_deref())
            .unwrap(),
    );
    assert_eq!(h.runtime.store_status(), before);
}

struct DeviceFrames(Receiver<RawFrame>);
impl Device for DeviceFrames {
    fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError> {
        Ok(self.0.try_recv().ok())
    }
}
struct BackendFrames(Option<Receiver<RawFrame>>);
impl Backend for BackendFrames {
    fn discover(&mut self) -> Result<Vec<maxed_capture::AdapterInfo>, CaptureError> {
        Ok(vec![maxed_capture::AdapterInfo {
            id: "synthetic".into(),
            description: "Synthetic".into(),
        }])
    }
    fn open(&mut self, _: &maxed_capture::AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
        Ok(Box::new(DeviceFrames(self.0.take().unwrap())))
    }
}
#[derive(Default)]
struct Capture {
    sources: Mutex<Vec<Sender<RawFrame>>>,
}
impl CaptureBackend for Capture {
    fn probe(&self) -> Result<(), CaptureError> {
        Ok(())
    }
    fn adapters(&self) -> Result<Vec<maxed_capture::AdapterInfo>, CaptureError> {
        Ok(["one", "two"]
            .map(|id| maxed_capture::AdapterInfo {
                id: id.into(),
                description: "Synthetic".into(),
            })
            .to_vec())
    }
    fn start(&self, _: CaptureConfig) -> Result<CaptureHandle, CaptureError> {
        let (send, frames) = unbounded();
        self.sources.lock().unwrap().push(send);
        start_with_backend(CaptureConfig::default(), move || {
            Ok(BackendFrames(Some(frames)))
        })
    }
}
struct RuntimeLifecycle {
    runtime: Arc<CaptureRuntime>,
    root: PathBuf,
    publisher: Publisher,
}
impl Lifecycle for RuntimeLifecycle {
    fn recording_status(&self, limit: u32) -> maxed_types::RecordingStatus {
        self.runtime.recording_status(limit)
    }
    fn configure_recording(&self, enabled: bool, limit: u32, live: bool) -> Result<(), String> {
        self.runtime
            .configure_recording(self.root.clone(), enabled, limit, live)
    }
    fn running(&self) -> bool {
        self.runtime.is_running()
    }
    fn live(&self) -> bool {
        self.runtime.status().state == CaptureState::Live
    }
    fn stop(&self) -> Result<(), String> {
        self.runtime.stop_and_flush()
    }
    fn start(&self, cache: Option<CachedCatalog>, handle: CaptureHandle) -> Result<(), String> {
        let mut options = RuntimeOptions::live(self.root.clone(), None);
        options.prepared_catalog = cache;
        self.runtime
            .start_prepared_published(options, handle, self.publisher.clone())
    }
    fn demo(&self, _: bool) -> Result<(), String> {
        Err("Not a demo lifecycle".into())
    }
}
// The Unity container/cache are real synthetic assets. Substitute only the
// admitted story's synthetic catalog definitions, retaining the cache fingerprint.
struct StoryAssets {
    fail: AtomicBool,
}
impl AssetLoader for StoryAssets {
    fn load(
        &self,
        paths: &AssetPaths,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(AssetProgress),
    ) -> Result<CachedCatalog, AssetError> {
        if self.fail.load(Ordering::Acquire) {
            progress(AssetProgress::Reading);
            while !cancel.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            // Simulate an I/O failure racing the stop cancellation. The failed
            // generation must not prevent finalization or replace the good cache.
            return Err(AssetError::Io);
        }
        let mut cached = maxed_assets::load_or_extract(paths, cancel, progress)?;
        let mut catalog: Catalog = serde_json::from_slice(
            &fs::read(DemoResources::development_root().join("catalog.json")).unwrap(),
        )
        .unwrap();
        catalog
            .client_fingerprint
            .clone_from(&cached.catalog.client_fingerprint);
        cached.catalog = Arc::new(catalog);
        Ok(cached)
    }
}
struct Harness {
    root: tempfile::TempDir,
    runtime: Arc<CaptureRuntime>,
    capture: Arc<Capture>,
    service: Arc<SetupService>,
    loader: Arc<StoryAssets>,
    seen: Arc<Mutex<Vec<Publication>>>,
}
impl Harness {
    fn new(assets: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let runtime = Arc::new(CaptureRuntime::default());
        let seen = Arc::new(Mutex::new(vec![]));
        let output = seen.clone();
        let publisher = Publisher::new(
            move |p| {
                output.lock().unwrap().push(p);
                Ok(())
            },
            Arc::new(|| panic!("publication failed")),
        )
        .unwrap();
        let capture = Arc::new(Capture::default());
        let loader = Arc::new(StoryAssets {
            fail: AtomicBool::new(false),
        });
        let service = Arc::new(
            SetupService::with_discovery_roots(
                root.path().into(),
                capture.clone(),
                Arc::new(RuntimeLifecycle {
                    runtime: runtime.clone(),
                    root: root.path().into(),
                    publisher,
                }),
                Arc::new(AssetBridge::new(loader.clone())),
                vec![],
            )
            .unwrap(),
        );
        if assets {
            let path = root.path().join("resources.assets");
            fs::write(&path, fixture::fixture()).unwrap();
            service.set_assets(path.to_string_lossy().into()).unwrap();
        }
        Self {
            root,
            runtime,
            capture,
            service,
            loader,
            seen,
        }
    }
    fn send(&self, frames: &[RawFrame]) {
        let sources = self.capture.sources.lock().unwrap();
        let send = sources.last().unwrap();
        for frame in frames {
            send.send(frame.clone()).unwrap();
        }
    }
    fn story_to(&self, at: u64) {
        self.send(
            &frames()
                .into_iter()
                .filter(|f| f.at.0 <= at)
                .collect::<Vec<_>>(),
        );
        wait(|| self.runtime.snapshot().at.0 >= at);
    }
    fn reader(&self) -> maxed_store::StoreReader {
        maxed_store::open_reader(&self.root.path().join("live.sqlite3")).unwrap()
    }
}
fn frames() -> Vec<RawFrame> {
    let file = fs::File::open(DemoResources::development_root().join("story.pcap")).unwrap();
    let mut reader = maxed_recording::PcapReader::new(file).unwrap();
    let mut frames = vec![];
    while let Some(frame) = reader.next_frame().unwrap() {
        frames.push(frame);
    }
    frames
}
#[track_caller]
fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    while !condition() {
        assert!(Instant::now() < deadline, "runtime condition timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
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
fn assert_drained(h: &Harness) {
    let snapshot = h.runtime.snapshot();
    assert_eq!(h.runtime.store_status().durable, Some(snapshot.sequence));
    let seen = h.seen.lock().unwrap();
    let last = seen
        .iter()
        .rev()
        .find_map(|p| {
            if let Publication::Snapshot(s) = p {
                Some(s)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(**last, snapshot);
    let events: Vec<_> = seen
        .iter()
        .filter_map(|p| {
            if let Publication::Event(e) = p {
                Some(e)
            } else {
                None
            }
        })
        .filter(|e| e.stream_id == snapshot.stream_id)
        .collect();
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.sequence.0 .0, index as u64 + 1);
    }
    assert_eq!(events.last().unwrap().sequence, snapshot.sequence);
    let json = serde_json::to_string(&events).unwrap();
    for forbidden in [
        "DemoAccount",
        "DemoAllyAccount",
        "resources.assets",
        "192.0.2.",
        "payload",
        "raw_bytes",
    ] {
        assert!(!json.contains(forbidden), "bridge leaked {forbidden}");
    }
}

#[test]
fn parsed_traffic_and_catalog_are_not_live_but_a_decoded_packet_is() {
    let h = Harness::new(true);
    assert!(!h.service.status().live_confirmed);
    h.service.start().unwrap();
    let story = frames();
    let first = story.iter().position(|f| f.bytes.len() > 54).unwrap();
    h.send(&story[..=first]); // only the first fragment of MAPINFO
    wait(|| h.runtime.snapshot().session.is_some());
    assert_eq!(h.runtime.status().state, CaptureState::WaitingForGame);
    assert!(!h.service.status().live_confirmed);
    assert!(h.runtime.snapshot().character.is_none());
    h.send(
        &story[first + 1..]
            .iter()
            .filter(|f| f.at.0 <= 100_000)
            .cloned()
            .collect::<Vec<_>>(),
    );
    wait(|| h.runtime.status().state == CaptureState::Live);
    assert!(h.service.status().live_confirmed);
    assert!(h.runtime.snapshot().character.is_none());
    h.service.stop().unwrap();
    assert!(!h.service.status().live_confirmed);
    assert_drained(&h);
}

#[test]
fn realm_dungeon_nexus_and_shutdown_publish_the_engine_story_and_durable_terminal_rows() {
    let h = Harness::new(true);
    h.service.start().unwrap();
    let story = frames();
    let mut previous = 0;
    for (at, map) in [
        (2_010_000, "Realm"),
        (8_020_000, "Snake Pit"),
        (12_000_000, "Nexus"),
    ] {
        h.send(
            &story
                .iter()
                .filter(|f| f.at.0 >= previous && f.at.0 <= at)
                .cloned()
                .collect::<Vec<_>>(),
        );
        wait(|| {
            h.runtime.snapshot().map.as_deref() == Some(map) && h.runtime.snapshot().at.0 >= at
        });
        previous = at + 1;
    }
    h.runtime.shutdown().unwrap();
    assert_drained(&h);
    let reader = h.reader();
    let run = reader.list_runs(filter()).unwrap().rows.remove(0);
    let saved = reader.get_run(&run.id).unwrap().unwrap();
    assert_eq!(saved.run.outcome, Some(Outcome::Cleared));
    assert_eq!(saved.fights.last().unwrap().total_damage, 300.0);
    assert!(h
        .runtime
        .snapshot()
        .fights
        .iter()
        .all(|f| f.ended.is_some()));
    assert!(h.runtime.snapshot().session.unwrap().ended.is_some());
}

#[test]
fn adapter_switch_closes_partial_run_then_starts_a_new_stream_without_losing_history() {
    let h = Harness::new(true);
    h.service.start().unwrap();
    h.story_to(6_020_000);
    let before = h.runtime.snapshot();
    h.service.set_adapter(Some("two".into())).unwrap();
    assert_ne!(h.runtime.snapshot().stream_id, before.stream_id);
    assert_eq!(h.runtime.status().state, CaptureState::WaitingForGame);
    let reader = h.reader();
    let saved = reader.get_run(&before.run.unwrap().id).unwrap().unwrap();
    assert_ne!(saved.run.outcome, Some(Outcome::Cleared));
    assert!(!saved.run.complete_capture);
    assert!(saved
        .fights
        .iter()
        .any(|f| reader.get_fight(&f.id).unwrap().unwrap().end_reason
            == Some(FightEnd::CaptureStopped)));
    h.service.stop().unwrap();
    assert_drained(&h);
}

#[test]
fn unavailable_catalog_preserves_partial_evidence_and_shutdown_finishes_active_fight_once() {
    let h = Harness::new(false);
    h.service.start().unwrap();
    h.story_to(6_020_000);
    assert!(h
        .runtime
        .snapshot()
        .issues
        .contains(&CoverageIssue::MissingCatalog));
    assert!(h.service.visuals(&[100]).is_err());
    h.runtime.shutdown().unwrap();
    assert_drained(&h);
    let seen = h.seen.lock().unwrap();
    let ends: Vec<_> = seen
        .iter()
        .filter_map(|p| match p {
            Publication::Event(e) => match &e.event {
                EngineEvent::FightEnded(f) if f.end_reason == Some(FightEnd::AppClosed) => Some(f),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(ends.len(), 1);
    assert!(!ends[0].complete_capture);
    assert!(ends[0].issues.contains(&CoverageIssue::MissingCatalog));
}

#[test]
fn asset_fingerprint_replacement_keeps_history_and_failed_extraction_allows_stop() {
    let h = Harness::new(true);
    h.service.start().unwrap();
    h.story_to(6_020_000);
    let run_id = h.runtime.snapshot().run.unwrap().id;
    let before = h.service.visuals(&[256]).unwrap().client_fingerprint;
    let next = h.root.path().join("next");
    fs::create_dir(&next).unwrap();
    let path = next.join("resources.assets");
    let mut bytes = fixture::fixture();
    let pixel = bytes
        .windows(8)
        .position(|v| v == [255, 0, 0, 255, 0, 255, 0, 255])
        .unwrap();
    bytes[pixel] = 127;
    fs::write(&path, bytes).unwrap();
    h.service.set_assets(path.to_string_lossy().into()).unwrap();
    assert_ne!(
        h.service.visuals(&[256]).unwrap().client_fingerprint,
        before
    );
    assert!(h
        .reader()
        .get_run(&run_id)
        .unwrap()
        .unwrap()
        .run
        .ended
        .is_some());
    h.loader.fail.store(true, Ordering::Release);
    let service = h.service.clone();
    let worker = std::thread::spawn(move || service.set_assets(path.to_string_lossy().into()));
    wait(|| h.service.status().assets == AssetCheck::Extracting);
    h.service.cancel_pending(false); // same IPC entry-point cancellation as Stop
    h.service.stop().unwrap();
    assert!(worker.join().unwrap().is_err());
    assert_eq!(h.runtime.status().state, CaptureState::Stopped);
    assert!(h.service.assets.cache().is_some());
    assert_drained(&h);
}

// Re-encrypt only generated fixture packets; no owner data enters this harness.
fn swapped_story() -> Vec<RawFrame> {
    let mut story = frames();
    let server = |f: &RawFrame| u16::from_be_bytes([f.bytes[34], f.bytes[35]]) == 2050;
    let mut wire: Vec<u8> = story
        .iter()
        .filter(|f| server(f))
        .flat_map(|f| f.bytes[54..].iter().copied())
        .collect();
    let mut decode = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
    let mut encode = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
    let mut offset = 0;
    while offset < wire.len() {
        let length = u32::from_be_bytes(wire[offset..offset + 4].try_into().unwrap()) as usize;
        let id = wire[offset + 4];
        let body = &mut wire[offset + 5..offset + length];
        decode.apply(body);
        if id == 101 {
            body[4..8].copy_from_slice(&2_i32.to_be_bytes());
        }
        if let Some(at) = body.windows(8).position(|bytes| bytes == b"DemoHero") {
            body[at..at + 8].copy_from_slice(b"NextHero");
        }
        encode.apply(body);
        offset += length;
    }
    offset = 0;
    for frame in &mut story {
        let is_server = server(frame);
        if is_server {
            let length = frame.bytes.len() - 54;
            frame.bytes[54..].copy_from_slice(&wire[offset..offset + length]);
            offset += length;
        }
        let port = if is_server { 36 } else { 34 };
        frame.bytes[port..port + 2].copy_from_slice(&50001_u16.to_be_bytes());
        frame.at.0 += 13_000_000;
    }
    story
}

#[test]
fn recovery_transport_close_character_swap_and_restart_keep_order_and_identity_honest() {
    let h = Harness::new(true);
    h.service.start().unwrap();
    h.story_to(6_020_000);
    let first = h.runtime.snapshot().character.unwrap();
    let original: Vec<_> = frames()
        .into_iter()
        .filter(|f| f.at.0 <= 6_020_000)
        .collect();
    let mut gap = original.last().unwrap().clone();
    gap.index += 1;
    gap.at = Timestamp(6_030_000);
    let sequence =
        u32::from_be_bytes(gap.bytes[38..42].try_into().unwrap()) + (gap.bytes.len() - 54) as u32;
    gap.bytes.truncate(54);
    gap.bytes[38..42].copy_from_slice(&sequence.to_be_bytes());
    gap.bytes.extend_from_slice(&[0, 0, 0, 4, 105]); // impossible framed length
    gap.bytes[16..18].copy_from_slice(&45_u16.to_be_bytes());
    gap.bytes[47] = 0x18;
    gap.original_len = 59;
    h.send(&[gap.clone()]);
    wait(|| h.runtime.status().state == CaptureState::Recovering);
    assert!(!h.service.status().live_confirmed);
    let recovered = h.runtime.status();
    assert_eq!(recovered.decode_failures, 1);
    let mut next_index = gap.index + 1;
    for server in [true, false] {
        let mut close = original
            .iter()
            .rev()
            .find(|f| (u16::from_be_bytes([f.bytes[34], f.bytes[35]]) == 2050) == server)
            .unwrap()
            .clone();
        close.index = next_index;
        next_index += 1;
        close.at = Timestamp(6_040_000);
        close.bytes.truncate(54);
        close.bytes[16..18].copy_from_slice(&40_u16.to_be_bytes());
        close.bytes[47] = 0x04;
        close.original_len = 54;
        h.send(&[close]);
    }
    wait(|| h.runtime.status().state == CaptureState::WaitingForGame);
    let mut swapped = swapped_story();
    for frame in &mut swapped {
        frame.index = next_index;
        next_index += 1;
    }
    h.send(
        &swapped
            .into_iter()
            .filter(|f| f.at.0 <= 14_020_000)
            .collect::<Vec<_>>(),
    );
    wait(|| {
        h.runtime
            .snapshot()
            .character
            .as_ref()
            .is_some_and(|c| c.name.as_deref() == Some("NextHero"))
    });
    assert_ne!(h.runtime.snapshot().character.unwrap().id, first.id);
    assert_eq!(h.runtime.status().state, CaptureState::Live);
    h.service.stop().unwrap();
    let stopped = h.runtime.status();
    assert_eq!(stopped.unknown_packets, recovered.unknown_packets);
    assert_eq!(stopped.decode_failures, recovered.decode_failures);
    assert_eq!(
        h.runtime.snapshot().capture.decode_failures,
        recovered.decode_failures
    );
    assert_drained(&h);
    let stream = h.runtime.snapshot().stream_id;
    h.service.start().unwrap();
    assert_ne!(h.runtime.snapshot().stream_id, stream);
    assert_eq!(h.runtime.status().state, CaptureState::WaitingForGame);
    assert!(h.runtime.snapshot().character.is_none());
    h.service.stop().unwrap();
}

#[test]
fn failed_storage_flush_retains_final_events_and_retry_does_not_finalize_twice() {
    let h = Harness::new(true);
    h.service.start().unwrap();
    h.story_to(6_020_000);
    wait(|| h.runtime.store_status().durable == Some(h.runtime.snapshot().sequence));
    let database = rusqlite::Connection::open(h.root.path().join("live.sqlite3")).unwrap();
    database.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(h.runtime.shutdown().is_err());
    assert!(h.runtime.has_unsaved_summaries());
    assert_ne!(
        h.runtime.store_status().durable,
        Some(h.runtime.snapshot().sequence)
    );
    database.execute_batch("ROLLBACK").unwrap();
    h.runtime.retry_store().unwrap();
    h.runtime.shutdown().unwrap();
    assert_drained(&h);
    let seen = h.seen.lock().unwrap();
    assert_eq!(seen.iter().filter(|p| matches!(p, Publication::Event(e) if matches!(&e.event, EngineEvent::FightEnded(f) if f.end_reason == Some(FightEnd::AppClosed)))).count(), 1);
}

#[test]
fn shutdown_waits_for_delivery_and_reports_failed_flush_after_saving() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let blocked = Arc::new(AtomicBool::new(false));
    let block = blocked.clone();
    let (entered, waiting) = unbounded();
    let (release, gate) = unbounded();
    let publisher = Publisher::new(
        move |_| {
            if block.swap(false, Ordering::AcqRel) {
                entered.send(()).unwrap();
                gate.recv().unwrap();
                Err(())
            } else {
                Ok(())
            }
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let capture = Capture::default();
    let handle = capture.start(CaptureConfig::default()).unwrap();
    runtime
        .start_prepared_published(
            RuntimeOptions::live(root.path().into(), None),
            handle,
            publisher,
        )
        .unwrap();
    runtime.flush_bridge().unwrap();
    blocked.store(true, Ordering::Release);
    let closing = runtime.clone();
    let (done, completion) = unbounded();
    let worker = std::thread::spawn(move || done.send(closing.shutdown()).unwrap());
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(completion.recv_timeout(Duration::from_millis(50)).is_err());
    release.send(()).unwrap();
    assert!(completion
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .is_err());
    worker.join().unwrap();
    assert_eq!(runtime.status().message, maxed_app::bridge::DELIVERY_FAILED);
    assert!(!runtime.has_unsaved_summaries());
    assert_eq!(
        runtime.store_status().durable,
        Some(runtime.snapshot().sequence)
    );
}

#[test]
fn rejected_start_cannot_replace_the_running_sessions_publisher() {
    let root = tempfile::tempdir().unwrap();
    let runtime = CaptureRuntime::default();
    let capture = Capture::default();
    let original = Publisher::new(|_| Err(()), Arc::new(|| {})).unwrap();
    runtime
        .start_prepared_published(
            RuntimeOptions::live(root.path().into(), None),
            capture.start(CaptureConfig::default()).unwrap(),
            original,
        )
        .unwrap();
    assert_eq!(
        runtime.flush_bridge().unwrap_err(),
        maxed_app::bridge::DELIVERY_FAILED
    );
    // This unused sink has never failed. Installing it on a rejected start would
    // falsely make the running session's delivery barrier report success.
    let replacement = Publisher::new(|_| Ok(()), Arc::new(|| {})).unwrap();
    assert_eq!(
        runtime
            .start_prepared_published(
                RuntimeOptions::live(root.path().into(), None),
                capture.start(CaptureConfig::default()).unwrap(),
                replacement,
            )
            .unwrap_err(),
        "Capture is already running"
    );
    assert_eq!(
        runtime.flush_bridge().unwrap_err(),
        maxed_app::bridge::DELIVERY_FAILED
    );
    assert!(runtime.shutdown().is_err());
    assert!(!runtime.has_unsaved_summaries());
}

struct RecordingBarrier {
    entered: Sender<()>,
    release: Receiver<()>,
    block: AtomicBool,
}
impl maxed_recording::RecordingDisk for RecordingBarrier {
    fn write(&self, file: &mut fs::File, bytes: &[u8]) -> std::io::Result<usize> {
        if self.block.swap(false, Ordering::AcqRel) {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
        }
        std::io::Write::write(file, bytes)
    }
}

#[test]
fn recording_defaults_off_and_no_files() {
    let h = Harness::new(true);
    assert!(!h.root.path().join("recordings").exists());
    h.service.start().unwrap();
    h.story_to(12_000_000);
    h.service.stop().unwrap();
    assert!(!h.root.path().join("recordings").exists());
    assert!(!h.reader().list_runs(filter()).unwrap().rows.is_empty());
}

#[test]
fn recording_burst_preserves_capture() {
    let h = Harness::new(true);
    h.service.start().unwrap();
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, gate) = crossbeam_channel::bounded(1);
    h.runtime
        .configure_recording_with_disk(
            h.root.path().into(),
            1_000_000,
            Arc::new(RecordingBarrier {
                entered,
                release: gate,
                block: AtomicBool::new(true),
            }),
        )
        .unwrap();
    let story = frames();
    h.send(&story[..1]);
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    // Fill the real recording queue while its disk worker is blocked. Invalid
    // Ethernet frames are harmless to decoding but still recording candidates.
    for index in 0..5000 {
        h.send(&[RawFrame {
            index,
            at: Timestamp(0),
            original_len: 4096,
            bytes: vec![0; 4096],
        }]);
    }
    // Positive control goes through capture, pipeline, engine, bridge and SQLite.
    h.send(&story[1..]);
    wait(|| h.runtime.snapshot().at.0 >= 12_000_000);
    h.runtime.flush_bridge().unwrap();
    let status = h.runtime.recording_status(1_000_000);
    assert_eq!(status.failure, None);
    assert!(status.dropped_frames > 0);
    assert!(!h.seen.lock().unwrap().is_empty());
    // SQLite receives and durably commits the decoded fight while disk is blocked.
    wait(|| {
        h.reader()
            .list_runs(filter())
            .is_ok_and(|page| !page.rows.is_empty())
    });
    assert_eq!(h.runtime.status().state, CaptureState::Live);
    release.send(()).unwrap();
    h.service.stop().unwrap();
    assert!(h.runtime.recording_status(1_000_000).dropped_frames >= status.dropped_frames);
    assert_drained(&h);
    let rows = h.reader().list_runs(filter()).unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert!(!h
        .reader()
        .get_run(&rows[0].id)
        .unwrap()
        .unwrap()
        .fights
        .is_empty());
}

#[test]
fn stop_update_exit_finish_recording_once() {
    let h = Harness::new(true);
    h.service
        .update(AppSettings {
            save_recordings: true,
            recording_limit_bytes: 1_000_000,
            ..h.service.settings.get().unwrap()
        })
        .unwrap();
    h.service.start().unwrap();
    let mut expected_bytes = 24;
    for frame in frames().into_iter().filter(|f| f.at.0 <= 12_000_000) {
        expected_bytes += 16 + frame.bytes.len() as u32;
        h.send(&[frame]);
        wait(|| h.runtime.recording_status(1_000_000).retained_bytes == expected_bytes);
    }
    wait(|| h.runtime.snapshot().at.0 >= 12_000_000);
    // Update uses this same lifecycle stop/flush, then exit may run it again.
    h.service.stop().unwrap();
    let read_segments = || -> Vec<Vec<u8>> {
        let mut paths: Vec<_> = fs::read_dir(h.root.path().join("recordings"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "pcap"))
            .collect();
        paths.sort();
        paths.iter().map(|p| fs::read(p).unwrap()).collect()
    };
    let before = read_segments();
    assert_eq!(before.len(), 1);
    h.service.stop().unwrap();
    h.runtime.shutdown().unwrap();
    assert_eq!(read_segments(), before);
    let mut reader = maxed_recording::PcapReader::new(before[0].as_slice()).unwrap();
    for expected in frames().into_iter().filter(|f| f.at.0 <= 12_000_000) {
        let actual = reader.next_frame().unwrap().unwrap();
        assert_eq!(actual.bytes, expected.bytes);
        assert_eq!(actual.at, expected.at);
    }
    assert!(reader.next_frame().unwrap().is_none());
    assert_drained(&h);
}

#[test]
fn recording_opt_in_arms_next_live_capture_and_persists_for_restart() {
    let h = Harness::new(true);
    let root = h.root.path().join("recordings");
    assert!(!root.exists());
    h.service
        .update(AppSettings {
            save_recordings: true,
            recording_limit_bytes: 1_000_000,
            ..h.service.settings.get().unwrap()
        })
        .unwrap();
    assert!(!h.runtime.is_running());
    assert_eq!(
        h.runtime.recording_status(1_000_000).state,
        RecordingState::Armed
    );
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        1,
        "idle opt-in creates only its lease, no segment"
    );
    assert!(
        maxed_app::settings::SettingsStore::new(h.root.path().into())
            .unwrap()
            .get()
            .unwrap()
            .save_recordings
    );
    for _ in 0..2 {
        h.service.start().unwrap();
        let mut bytes = h.runtime.recording_status(1_000_000).retained_bytes + 24;
        for frame in frames().into_iter().take(3) {
            bytes += 16 + frame.bytes.len() as u32;
            h.send(&[frame]);
            wait(|| h.runtime.recording_status(1_000_000).retained_bytes == bytes);
        }
        h.service.stop().unwrap();
        assert_eq!(
            h.service.recording_status().unwrap().state,
            RecordingState::Armed
        );
    }
    assert_eq!(
        fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "pcap"))
            .count(),
        2
    );
}

#[test]
fn recording_enable_while_live_and_cap_changes_write_without_restarting_capture() {
    let h = Harness::new(true);
    let root = h.root.path().join("recordings");
    h.service.start().unwrap();
    h.story_to(6_020_000);
    let stream = h.runtime.snapshot().stream_id;
    h.service
        .update(AppSettings {
            save_recordings: true,
            recording_limit_bytes: 1_000_000,
            ..h.service.settings.get().unwrap()
        })
        .unwrap();
    let tail: Vec<_> = frames()
        .into_iter()
        .filter(|f| f.at.0 > 6_020_000)
        .collect();
    assert!(tail.len() > 1);
    let mut bytes = 24;
    for (index, frame) in tail.iter().enumerate() {
        if index == 1 {
            h.service
                .update(AppSettings {
                    recording_limit_bytes: 2_000_000,
                    ..h.service.settings.get().unwrap()
                })
                .unwrap();
            assert_eq!(h.service.recording_status().unwrap().limit_bytes, 2_000_000);
            bytes += 24; // The replacement recording owns a new pcap segment.
        }
        bytes += 16 + frame.bytes.len() as u32;
        h.send(std::slice::from_ref(frame));
        wait(|| h.service.recording_status().unwrap().retained_bytes == bytes);
        assert_eq!(h.runtime.snapshot().stream_id, stream);
    }
    h.service.stop().unwrap();
    let mut paths: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "pcap"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 2);
    let mut actual = Vec::new();
    for path in paths {
        let mut reader = maxed_recording::PcapReader::new(fs::File::open(path).unwrap()).unwrap();
        while let Some(frame) = reader.next_frame().unwrap() {
            actual.push(frame);
        }
    }
    assert_eq!(actual.len(), tail.len());
    for (actual, expected) in actual.iter().zip(&tail) {
        assert_eq!(actual.bytes, expected.bytes);
        assert_eq!(actual.at, expected.at);
    }
    assert_eq!(
        h.service.recording_status().unwrap().state,
        RecordingState::Armed
    );
    h.service.start().unwrap();
    assert_eq!(h.service.recording_status().unwrap().limit_bytes, 2_000_000);
    h.service.stop().unwrap();
}

#[test]
fn recording_changes_keep_the_worker_stream_session_and_open_fights() {
    let h = Harness::new(true);
    h.service.start().unwrap();
    h.story_to(6_020_000);
    let before = h.runtime.snapshot();
    let open: Vec<_> = before
        .fights
        .iter()
        .filter(|f| f.ended.is_none())
        .map(|f| f.id.clone())
        .collect();
    assert!(!open.is_empty());
    let starts = h.capture.sources.lock().unwrap().len();
    for (enabled, limit) in [(true, 1_000_000), (true, 2_000_000), (false, 2_000_000)] {
        h.service
            .update(AppSettings {
                save_recordings: enabled,
                recording_limit_bytes: limit,
                ..h.service.settings.get().unwrap()
            })
            .unwrap();
        let after = h.runtime.snapshot();
        assert_eq!(h.capture.sources.lock().unwrap().len(), starts);
        assert_eq!(h.runtime.status().state, CaptureState::Live);
        assert_eq!(after.stream_id, before.stream_id);
        assert_eq!(
            after.session.as_ref().unwrap().id,
            before.session.as_ref().unwrap().id
        );
        for id in &open {
            assert!(after
                .fights
                .iter()
                .find(|f| &f.id == id)
                .unwrap()
                .ended
                .is_none());
        }
    }
    h.send(
        &frames()
            .into_iter()
            .filter(|f| f.at.0 > 6_020_000)
            .collect::<Vec<_>>(),
    );
    wait(|| h.runtime.snapshot().at.0 >= 12_000_000);
    assert_eq!(h.runtime.snapshot().stream_id, before.stream_id);
    h.service.stop().unwrap();
}
