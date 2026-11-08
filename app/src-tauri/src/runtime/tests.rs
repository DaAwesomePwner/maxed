use super::*;

#[test]
fn offline_does_not_block_live() {
    let (entered, waiting) = bounded(1);
    let (release, gate) = bounded(1);
    let updates = crate::updater::service::UpdateService::with_factory(Arc::new(move || {
        entered.send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
        Err(crate::updater::Error::Network)
    }));
    updates
        .commit_channel(maxed_types::UpdateChannel::Stable, || Ok(()))
        .unwrap();
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    let (send, closes) = start_fake(&runtime, sink);
    updates.check();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    send.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    assert_eq!(closes.load(Ordering::Acquire), 0);
    assert_eq!(updates.status().phase, maxed_types::UpdatePhase::Checking);
    release.send(()).unwrap();
    wait_until(|| updates.status().failure == Some(maxed_types::UpdateFailure::Offline));
    assert_eq!(runtime.status().state, CaptureState::Live);
    runtime.stop_and_flush().unwrap();
    assert_eq!(closes.load(Ordering::Acquire), 1);
}

#[test]
fn failed_installer_launch_leaves_capture_stopped_and_restartable() {
    for changed_bytes in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = CaptureRuntime::default();
        let (sink, _) = sink();
        let (send, closes) = start_fake(&runtime, sink.clone());
        send.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
        wait_until(|| runtime.status().state == CaptureState::Live);
        let mut updater = if changed_bytes {
            crate::updater::tests::changed_verified_owner()
        } else {
            crate::updater::tests::verified_owner()
        };
        assert_eq!(
            updater.install_with::<()>(
                root.path(),
                || runtime
                    .prepare_update()
                    .map_err(|_| crate::updater::Error::Shutdown),
                |_| {
                    assert!(!changed_bytes, "changed bytes must never reach launch");
                    Err(crate::updater::Error::Install)
                }
            ),
            Err(crate::updater::Error::Install)
        );
        runtime.finish_update();
        assert!(!runtime.is_running());
        assert_ne!(runtime.status().state, CaptureState::Live);
        assert_eq!(closes.load(Ordering::Acquire), 1);
        let (send, _) = start_fake(&runtime, sink);
        send.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
        wait_until(|| runtime.status().state == CaptureState::Live);
        runtime.stop_and_flush().unwrap();
    }
}

#[test]
fn update_waits_for_durable_shutdown() {
    struct SlowSync {
        entered: Sender<()>,
        release: Receiver<()>,
    }
    impl maxed_recording::RecordingDisk for SlowSync {
        fn sync(&self, file: &std::fs::File) -> std::io::Result<()> {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(10)).unwrap();
            file.sync_all()
        }
    }
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let (published, publishing) = bounded(1);
    let (flush, flushing) = bounded(1);
    let block = Arc::new(AtomicBool::new(false));
    let blocking = block.clone();
    let publisher = Publisher::new(
        move |_| {
            if blocking.swap(false, Ordering::AcqRel) {
                published.send(()).unwrap();
                flushing.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            Ok(())
        },
        Arc::new(|| panic!("synthetic bridge failed")),
    )
    .unwrap();
    *runtime.publisher.lock().unwrap() = Some(publisher.clone());
    let (send, closes) = start_fake(
        &runtime,
        Arc::new(move |status| publisher.publish(Publication::Capture(status))),
    );
    send.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    runtime.flush_bridge().unwrap();
    let (sync, syncing) = bounded(1);
    let (finish, finishing) = bounded(1);
    runtime
        .configure_recording_with_disk(
            root.path().into(),
            10000,
            Arc::new(SlowSync {
                entered: sync,
                release: finishing,
            }),
        )
        .unwrap();
    assert_eq!(
        runtime
            .shared
            .recording
            .try_record(&frame(15, false, &[0, 0, 0, 5, 105])),
        crate::recording::RecordAdmission::Accepted
    );
    let mut updater = crate::updater::tests::verified_owner();
    let launched = Arc::new(AtomicBool::new(false));
    let launch = launched.clone();
    let owner = runtime.clone();
    let app_data = root.path().to_owned();
    block.store(true, Ordering::Release);
    let worker = std::thread::spawn(move || {
        updater.install_with(
            &app_data,
            || {
                owner
                    .prepare_update()
                    .map_err(|_| crate::updater::Error::Shutdown)
            },
            |_| {
                assert!(!owner.is_running());
                assert!(!owner.has_unsaved_summaries());
                launch.store(true, Ordering::Release);
                Ok(())
            },
        )
    });
    syncing.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(closes.load(Ordering::Acquire), 1);
    assert!(!launched.load(Ordering::Acquire));
    finish.send(()).unwrap();
    publishing.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(!launched.load(Ordering::Acquire));
    flush.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert!(launched.load(Ordering::Acquire));
    runtime.finish_update();
}

#[test]
fn update_recording_sync_failure_blocks_shutdown_without_discarding_summaries() {
    struct FailingSync;
    impl maxed_recording::RecordingDisk for FailingSync {
        fn sync(&self, _: &std::fs::File) -> std::io::Result<()> {
            Err(std::io::Error::other("synthetic sync failure"))
        }
    }
    let root = tempfile::tempdir().unwrap();
    let runtime = CaptureRuntime::default();
    runtime
        .configure_recording_with_disk(root.path().into(), 10000, Arc::new(FailingSync))
        .unwrap();
    runtime.shared.recording.try_record(&frame(1, false, &[]));
    assert!(runtime.prepare_update().is_err());
    assert!(runtime.shared.recording.status(10000).failure.is_some());
    assert!(!runtime.is_running());
}

#[test]
fn failed_stop_waits_for_queued_capture_delivery_before_returning_error() {
    let runtime = Arc::new(CaptureRuntime::default());
    let (send, frames) = bounded(1);
    let backend = FakeBackend {
        device: Some(FakeDevice {
            frames,
            closes: Arc::new(AtomicUsize::new(0)),
        }),
        opening: None,
    };
    let block = Arc::new(AtomicBool::new(false));
    let should_block = block.clone();
    let (entered, waiting) = bounded(1);
    let (release, gate) = bounded(1);
    let publisher = Publisher::new(
        move |_| {
            if should_block.swap(false, Ordering::AcqRel) {
                entered.send(()).unwrap();
                gate.recv().unwrap();
            }
            Ok(())
        },
        Arc::new(|| panic!("unexpected publication failure")),
    )
    .unwrap();
    *runtime.publisher.lock().unwrap() = Some(publisher.clone());
    runtime
        .start_with(
            Arc::new(move |status| publisher.publish(Publication::Capture(status))),
            move || start_with_backend(CaptureConfig::default(), || Ok(backend)),
        )
        .unwrap();
    send.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    runtime.flush_bridge().unwrap();
    block.store(true, Ordering::Release);
    runtime.shared.failed.store(true, Ordering::Release);
    let stopping = runtime.clone();
    let (finished, completion) = bounded(1);
    let worker = std::thread::spawn(move || finished.send(stopping.stop_and_flush()).unwrap());
    waiting.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(completion.recv_timeout(Duration::from_millis(50)).is_err());
    release.send(()).unwrap();
    assert!(completion
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .is_err());
    worker.join().unwrap();
}
use maxed_capture::{
    backend::{start_with_backend, Backend, Device},
    AdapterInfo,
};
use maxed_types::{RawFrame, Timestamp};
use std::sync::atomic::AtomicUsize;
use std::time::Instant;

struct FakeDevice {
    frames: Receiver<RawFrame>,
    closes: Arc<AtomicUsize>,
}
impl Device for FakeDevice {
    fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError> {
        Ok(self.frames.try_recv().ok())
    }
}
impl Drop for FakeDevice {
    fn drop(&mut self) {
        self.closes.fetch_add(1, Ordering::SeqCst);
    }
}
struct FakeBackend {
    device: Option<FakeDevice>,
    opening: Option<(Sender<()>, Receiver<()>)>,
}
impl Backend for FakeBackend {
    fn discover(&mut self) -> Result<Vec<AdapterInfo>, CaptureError> {
        Ok(vec![AdapterInfo {
            id: "synthetic".into(),
            description: "synthetic".into(),
        }])
    }
    fn open(&mut self, _: &AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
        if let Some((entered, release)) = &self.opening {
            entered.send(()).unwrap();
            release.recv().unwrap();
        }
        Ok(Box::new(self.device.take().unwrap()))
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "worker did not reach expected state"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn sink() -> (StatusSink, Receiver<CaptureStatus>) {
    let (send, receive) = crossbeam_channel::unbounded();
    (
        Arc::new(move |status| {
            let _ = send.send(status);
        }),
        receive,
    )
}

fn start_fake(runtime: &CaptureRuntime, sink: StatusSink) -> (Sender<RawFrame>, Arc<AtomicUsize>) {
    let (send, frames) = crossbeam_channel::unbounded();
    let closes = Arc::new(AtomicUsize::new(0));
    let backend = FakeBackend {
        device: Some(FakeDevice {
            frames,
            closes: closes.clone(),
        }),
        opening: None,
    };
    runtime
        .start_with(sink, move || {
            start_with_backend(CaptureConfig::default(), || Ok(backend))
        })
        .unwrap();
    (send, closes)
}

// Synthetic client -> server Ethernet/IPv4/TCP with a fresh SYN and packet body.
fn frame(seq: u32, syn: bool, body: &[u8]) -> RawFrame {
    let mut bytes = vec![0u8; 54 + body.len()];
    bytes[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    bytes[14] = 0x45;
    bytes[16..18].copy_from_slice(&((40 + body.len()) as u16).to_be_bytes());
    bytes[23] = 6;
    bytes[26..30].copy_from_slice(&[192, 0, 2, 1]);
    bytes[30..34].copy_from_slice(&[192, 0, 2, 2]);
    bytes[34..38].copy_from_slice(&[0xc3, 0x50, 8, 2]);
    bytes[38..42].copy_from_slice(&seq.to_be_bytes());
    bytes[46] = 0x50;
    bytes[47] = if syn { 2 } else { 0x18 };
    bytes[54..].copy_from_slice(body);
    RawFrame {
        index: 0,
        at: Timestamp(1),
        original_len: bytes.len() as u32,
        bytes,
    }
}

#[test]
fn duplicate_start_and_shutdown_close_once_and_prevent_restart() {
    let runtime = CaptureRuntime::default();
    let (sink, events) = sink();
    let (_frames, closes) = start_fake(&runtime, sink.clone());
    assert_eq!(
        runtime.start_with(sink.clone(), || panic!("duplicate must not open")),
        Err("Capture is already running".into())
    );
    runtime.shutdown().unwrap();
    runtime.stop().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status().state, CaptureState::Stopped);
    assert_eq!(
        events.try_iter().last().unwrap().state,
        CaptureState::Stopped
    );
    assert_eq!(
        runtime.start_with(sink, || panic!("closed app must not open")),
        Err("Maxed is closing".into())
    );
}

#[test]
fn stop_during_pending_open_joins_and_suppresses_late_live() {
    let runtime = Arc::new(CaptureRuntime::default());
    let (sink, events) = sink();
    let (entered, opened) = bounded(1);
    let (release, gate) = bounded(1);
    let (frames_tx, frames) = crossbeam_channel::unbounded();
    frames_tx.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    let closes = Arc::new(AtomicUsize::new(0));
    let backend = FakeBackend {
        device: Some(FakeDevice {
            frames,
            closes: closes.clone(),
        }),
        opening: Some((entered, gate)),
    };
    runtime
        .start_with(sink, move || {
            start_with_backend(CaptureConfig::default(), || Ok(backend))
        })
        .unwrap();
    opened.recv_timeout(Duration::from_secs(5)).unwrap();
    let cancel = runtime
        .session
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .cancel
        .clone();
    let stop_runtime = runtime.clone();
    let stop = thread::spawn(move || stop_runtime.stop());
    wait_until(|| cancel.load(Ordering::Acquire));
    release.send(()).unwrap();
    stop.join().unwrap().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status().state, CaptureState::Stopped);
    assert!(!events
        .try_iter()
        .any(|status| status.state == CaptureState::Live));
}

#[test]
fn native_failures_are_safe_terminal_states_and_can_retry() {
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    for (error, state) in [
        (CaptureError::Missing, CaptureState::NpcapMissing),
        (CaptureError::Incompatible, CaptureState::NpcapIncompatible),
        (
            CaptureError::PermissionDenied,
            CaptureState::PermissionDenied,
        ),
        (CaptureError::NoAdapters, CaptureState::Failed),
    ] {
        runtime
            .start_with(sink.clone(), move || {
                start_with_backend(
                    CaptureConfig::default(),
                    move || -> Result<FakeBackend, CaptureError> { Err(error) },
                )
            })
            .unwrap();
        wait_until(|| runtime.status().state == state);
        wait_until(|| {
            runtime
                .session
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .worker
                .is_finished()
        });
        assert_eq!(runtime.status().message, sanitize(&runtime.status().state));
        assert_eq!(runtime.snapshot().capture.state, state);
    }
    let (_frames, closes) = start_fake(&runtime, sink);
    runtime.stop().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.snapshot().capture.state, CaptureState::Stopped);
}

#[test]
fn unknown_only_never_goes_live_but_typed_packet_does_and_stop_is_final() {
    let runtime = CaptureRuntime::default();
    let (sink, events) = sink();
    let (frames, closes) = start_fake(&runtime, sink);
    assert_eq!(runtime.status().state, CaptureState::WaitingForGame);
    frames.send(frame(10, true, &[0, 0, 0, 5, 254])).unwrap();
    wait_until(|| runtime.status().unknown_packets == 1);
    assert_eq!(runtime.status().state, CaptureState::WaitingForGame);
    // ESCAPE has no body; the empty body requires no RC4 advancement.
    frames.send(frame(16, false, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    runtime.stop().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    let statuses: Vec<_> = events.try_iter().map(|s| s.state).collect();
    assert!(statuses.contains(&CaptureState::Live));
    assert_eq!(statuses.last(), Some(&CaptureState::Stopped));
    assert!(frames.send(frame(21, false, &[0, 0, 0, 5, 105])).is_err());
    assert_eq!(runtime.status().state, CaptureState::Stopped);
}

#[test]
fn field_failures_preserve_readiness_and_index_loss_invalidates_cipher() {
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    // PLAYERHIT requires fields, so an empty body is a decode failure.
    frames.send(frame(10, true, &[0, 0, 0, 5, 90])).unwrap();
    wait_until(|| runtime.status().decode_failures == 1);
    assert_eq!(runtime.status().state, CaptureState::WaitingForGame);
    frames.send(frame(16, false, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    frames.send(frame(21, false, &[0, 0, 0, 5, 90])).unwrap();
    wait_until(|| runtime.status().decode_failures == 2);
    assert_eq!(runtime.status().state, CaptureState::Live);
    runtime.stop().unwrap();

    let mut pipeline = Pipeline::new(Arc::new(AtomicBool::new(false)));
    let first = pipeline.push(frame(10, true, &[0, 0, 0, 5, 105]));
    assert!(first
        .iter()
        .any(|e| matches!(e, PipelineEvent::Decoded(DecodeEvent::Packet(_)))));
    let mut lost = frame(16, false, &[0, 0, 0, 5, 105]);
    lost.index = 2;
    let events = pipeline.push(lost);
    assert!(events.iter().any(|e| matches!(
        e,
        PipelineEvent::Decoded(DecodeEvent::Diagnostic {
            kind: Diagnostic::Recovering,
            ..
        })
    )));
    assert!(!events
        .iter()
        .any(|e| matches!(e, PipelineEvent::Decoded(DecodeEvent::Packet(_)))));
}

#[test]
fn typed_packet_with_trailing_bytes_stays_live_and_emits_one_snapshot() {
    let runtime = CaptureRuntime::default();
    let (sink, events) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    // ESCAPE is typed and bodyless; any decrypted byte is a trailing byte.
    frames.send(frame(10, true, &[0, 0, 0, 6, 105, 0])).unwrap();
    wait_until(|| runtime.status().decode_failures == 1);
    assert_eq!(runtime.status().state, CaptureState::Live);
    runtime.stop().unwrap();
    let snapshots: Vec<_> = events.try_iter().collect();
    assert_eq!(snapshots.len(), 3); // Waiting, one complete frame snapshot, Stopped.
    assert_eq!(snapshots[1].state, CaptureState::Live);
    assert_eq!(snapshots[1].decode_failures, 1);
}

#[test]
fn bad_frame_only_publishes_final_recovery_and_counts_failure() {
    let runtime = CaptureRuntime::default();
    let (sink, events) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    // A complete typed packet followed by an impossible framed length.
    frames
        .send(frame(10, true, &[0, 0, 0, 5, 105, 0, 0, 0, 4, 105]))
        .unwrap();
    wait_until(|| runtime.status().decode_failures == 1);
    assert_eq!(runtime.status().state, CaptureState::Recovering);
    runtime.stop().unwrap();
    let snapshots: Vec<_> = events.try_iter().collect();
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[1].state, CaptureState::Recovering);
    assert_eq!(snapshots[1].decode_failures, 1);
}

#[test]
fn unchanged_frames_and_idle_ticks_do_not_emit_and_unknowns_are_coalesced() {
    let runtime = CaptureRuntime::default();
    let (sink, events) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    frames.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    frames.send(frame(16, false, &[0, 0, 0, 5, 105])).unwrap();
    frames
        .send(frame(21, false, &[0, 0, 0, 5, 254, 0, 0, 0, 5, 254]))
        .unwrap();
    wait_until(|| runtime.status().unknown_packets == 2);
    // Cover multiple native-status ticks as well as an unchanged decoded frame.
    thread::sleep(Duration::from_millis(550));
    runtime.stop().unwrap();
    let snapshots: Vec<_> = events.try_iter().collect();
    assert_eq!(snapshots.len(), 4); // Waiting, Live, both unknowns, Stopped.
    assert_eq!(snapshots[2].unknown_packets, 2);
}

fn server_frame(seq: u32, syn: bool, body: &[u8]) -> RawFrame {
    let mut frame = frame(seq, syn, body);
    frame.bytes[26..30].copy_from_slice(&[192, 0, 2, 2]);
    frame.bytes[30..34].copy_from_slice(&[192, 0, 2, 1]);
    frame.bytes[34..38].copy_from_slice(&[8, 2, 0xc3, 0x50]);
    frame
}

#[test]
fn native_ten_thousand_payload_frames_publish_at_most_three_session_updates() {
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    let (send, frames) = crossbeam_channel::unbounded();
    let backend = FakeBackend {
        device: Some(FakeDevice {
            frames,
            closes: Arc::new(AtomicUsize::new(0)),
        }),
        opening: None,
    };
    let sessions = Arc::new(Mutex::new(Vec::new()));
    let seen = sessions.clone();
    let bridge = Bridge {
        event: Arc::new(move |event| {
            if let maxed_types::EngineEvent::SessionUpdated(session) = event.event {
                seen.lock().unwrap().push(session);
            }
        }),
        ..Default::default()
    };
    let temporary = tempfile::tempdir().unwrap();
    let mut options = RuntimeOptions::live(temporary.path().into(), None);
    options.temporary = Some(temporary);
    runtime
        .launch(sink, bridge, options, move || {
            start_with_backend(
                CaptureConfig {
                    capacity: 16_384,
                    ..Default::default()
                },
                || Ok(backend),
            )
        })
        .unwrap();
    for i in 0..10_000_u32 {
        let mut payload = frame(
            if i == 0 { 10 } else { 11 + i * 5 },
            i == 0,
            &[0, 0, 0, 5, if i == 9_999 { 254 } else { 105 }],
        );
        payload.at = Timestamp(u64::from(i) * 60_000_000 / 9_999);
        send.send(payload).unwrap();
    }
    wait_until(|| runtime.status().unknown_packets == 1);
    assert_eq!(runtime.status().dropped_frames, 0);
    assert!((2..=3).contains(&sessions.lock().unwrap().len()));
    runtime.stop().unwrap();
    let sessions = sessions.lock().unwrap();
    assert!(sessions.iter().filter(|s| s.ended.is_none()).count() <= 3);
    assert_eq!(sessions.iter().filter(|s| s.ended.is_some()).count(), 1);
    assert_eq!(sessions.last().unwrap().last_traffic, Timestamp(60_000_000));
}

#[test]
fn midstream_candidate_packets_cannot_announce_live_or_create_a_session() {
    use maxed_protocol::{key_bytes, Rc4};
    use maxed_types::Direction;
    let runtime = CaptureRuntime::default();
    let (sink, statuses) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    let mut cipher = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
    cipher.skip(13);
    let mut seq = 100;
    let mut send = |id: u8, mut body: Vec<u8>| {
        cipher.apply(&mut body);
        let mut bytes = ((body.len() + 5) as u32).to_be_bytes().to_vec();
        bytes.push(id);
        bytes.extend(body);
        frames.send(server_frame(seq, false, &bytes)).unwrap();
        seq += bytes.len() as u32;
    };
    // Midstream attach discards its first short segment before searching.
    send(254, vec![]);
    for n in [50_i32, 51] {
        let mut body = vec![0; 16];
        body[..4].copy_from_slice(&n.to_be_bytes());
        send(10, body);
    }
    // This typed packet follows a search match but precedes verification.
    send(101, vec![0; 10]);
    send(254, vec![]); // unknown counter is an ordered processing fence
    wait_until(|| runtime.status().unknown_packets == 1);
    runtime.flush_bridge().unwrap();
    assert!(matches!(
        runtime.status().state,
        CaptureState::WaitingForGame | CaptureState::Recovering
    ));
    assert!(runtime.snapshot().session.is_none());
    assert!(!statuses.try_iter().any(|s| s.state == CaptureState::Live));
    let mut body = vec![0; 16];
    body[..4].copy_from_slice(&52_i32.to_be_bytes());
    send(10, body);
    wait_until(|| runtime.status().state == CaptureState::Live);
    wait_until(|| runtime.snapshot().session.is_some());
    runtime.stop().unwrap();
}

#[test]
fn verified_fragments_keep_traffic_alive_but_unverified_other_direction_does_not() {
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    frames.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    // No complete packet: the verified directional stream still observed traffic.
    let mut fragment = frame(16, false, &[0, 0]);
    fragment.at = Timestamp(60_000_000);
    frames.send(fragment).unwrap();
    thread::sleep(Duration::from_millis(300));
    // A new, unverified server direction must not inherit client trust.
    let mut unverified = server_frame(100, false, &[0, 0, 0, 5, 254]);
    unverified.at = Timestamp(120_000_000);
    frames.send(unverified).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Recovering);
    runtime.stop().unwrap();
    assert_eq!(
        runtime.snapshot().session.unwrap().last_traffic,
        Timestamp(60_000_000)
    );
}

#[test]
fn native_continuous_frames_survive_forward_and_backward_clock_jumps() {
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    frames.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.snapshot().session.is_some());
    let id = runtime.snapshot().session.unwrap().id;
    let mut forward = frame(16, false, &[0, 0, 0, 5, 105]);
    forward.at = Timestamp(3_600_000_000);
    frames.send(forward).unwrap();
    let mut backward = frame(21, false, &[0, 0, 0, 5, 254]);
    backward.at = Timestamp(2);
    frames.send(backward).unwrap();
    wait_until(|| runtime.status().unknown_packets == 1);
    thread::sleep(Duration::from_millis(300)); // exercise the native idle tick
    let snapshot = runtime.snapshot();
    let session = snapshot.session.unwrap();
    assert_eq!(session.id, id);
    assert!(session.ended.is_none());
    assert_eq!(session.last_traffic, Timestamp(3_600_000_000));
    assert!(snapshot
        .issues
        .contains(&maxed_types::CoverageIssue::ClockRegression));
    runtime.stop().unwrap();
    let session = runtime.snapshot().session.unwrap();
    assert!(session.ended.unwrap() >= session.started);
}

#[path = "snapshot_reader_tests.rs"]
mod snapshot_reader_tests;

#[test]
fn last_direction_closing_waits_and_a_fresh_syn_does_not_restore_stale_live() {
    let runtime = CaptureRuntime::default();
    let (sink, events) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    frames.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    frames
        .send(server_frame(20, true, &[0, 0, 0, 5, 254]))
        .unwrap();
    wait_until(|| runtime.status().unknown_packets == 1);
    let mut client_fin = frame(16, false, &[]);
    client_fin.bytes[47] = 0x11;
    frames.send(client_fin).unwrap();
    // This counter update is a processing fence after the client FIN.
    frames
        .send(server_frame(26, false, &[0, 0, 0, 5, 254]))
        .unwrap();
    wait_until(|| runtime.status().unknown_packets == 2);
    assert_eq!(runtime.status().state, CaptureState::Live);
    let mut server_fin = server_frame(31, false, &[]);
    server_fin.bytes[47] = 0x11;
    frames.send(server_fin).unwrap();
    wait_until(|| runtime.status().state == CaptureState::WaitingForGame);
    frames.send(frame(100, true, &[])).unwrap();
    frames.send(frame(101, false, &[0, 0, 0, 5, 254])).unwrap();
    wait_until(|| runtime.status().unknown_packets == 3);
    assert_eq!(runtime.status().state, CaptureState::WaitingForGame);
    frames.send(frame(106, false, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    runtime.stop().unwrap();
    assert_eq!(
        events
            .try_iter()
            .filter(|s| s.state == CaptureState::WaitingForGame)
            .count(),
        3
    );
}

#[test]
fn closing_one_connection_does_not_hide_another_open_connection() {
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    let (frames, _) = start_fake(&runtime, sink);
    frames.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    let mut second = frame(20, true, &[0, 0, 0, 5, 105]);
    second.bytes[34..36].copy_from_slice(&50001u16.to_be_bytes());
    frames.send(second).unwrap();
    let mut reset = frame(16, false, &[]);
    reset.bytes[47] = 4;
    frames.send(reset).unwrap();
    let mut fence = frame(26, false, &[0, 0, 0, 5, 254]);
    fence.bytes[34..36].copy_from_slice(&50001u16.to_be_bytes());
    frames.send(fence).unwrap();
    wait_until(|| runtime.status().unknown_packets == 1);
    assert_eq!(runtime.status().state, CaptureState::Live);
    let mut final_reset = frame(31, false, &[]);
    final_reset.bytes[34..36].copy_from_slice(&50001u16.to_be_bytes());
    final_reset.bytes[47] = 4;
    frames.send(final_reset).unwrap();
    wait_until(|| runtime.status().state == CaptureState::WaitingForGame);
    runtime.stop().unwrap();
}

#[test]
fn poisoned_mutexes_fail_with_fixed_strings() {
    let runtime = Arc::new(CaptureRuntime::default());
    let other = runtime.clone();
    let _ = thread::spawn(move || {
        let _guard = other.shared.status.lock().unwrap();
        panic!("synthetic poison");
    })
    .join();
    assert_eq!(runtime.status(), failed_status());
    let (sink, _) = sink();
    assert_eq!(
        runtime.start_with(sink, || panic!("must not start")),
        Err(WORKER_FAILED.into())
    );

    let runtime = Arc::new(CaptureRuntime::default());
    let other = runtime.clone();
    let _ = thread::spawn(move || {
        let _guard = other.session.lock().unwrap();
        panic!("synthetic poison");
    })
    .join();
    assert_eq!(runtime.stop(), Err(WORKER_FAILED.into()));
    assert_eq!(runtime.status(), failed_status());
}

#[test]
fn webview_message_allowlist_discards_arbitrary_native_text() {
    let shared = Shared::default();
    let (sink, events) = sink();
    shared.publish(
        CaptureStatus {
            state: CaptureState::Failed,
            message: "secret-token chat native-path".into(),
            ..Default::default()
        },
        &sink,
    );
    let status = events.recv().unwrap();
    let json = serde_json::to_string(&status).unwrap();
    assert!(!json.contains("secret-token"));
    assert!(!json.contains("chat"));
    assert!(!json.contains("native-path"));
}

#[test]
fn runtime_close_during_run_retains_failed_flush_and_retry_allows_exit() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    let (send, frames) = crossbeam_channel::unbounded();
    let closes = Arc::new(AtomicUsize::new(0));
    let backend = FakeBackend {
        device: Some(FakeDevice {
            frames,
            closes: closes.clone(),
        }),
        opening: None,
    };
    let mut options = RuntimeOptions::live(directory.path().into(), None);
    options.namespace = Namespace::Demo; // isolated authored catalog, no installed client required
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let trace = recorded.clone();
    runtime
        .launch(
            sink,
            Bridge {
                event: Arc::new(move |event| trace.lock().unwrap().push(event)),
                ..Default::default()
            },
            options,
            move || start_with_backend(CaptureConfig::default(), || Ok(backend)),
        )
        .unwrap();
    let mut reader = maxed_recording::PcapReader::new(std::io::Cursor::new(include_bytes!(
        "../../../../tests/fixtures/scrubbed/maxed-demo/story.pcap"
    )))
    .unwrap();
    while let Some(frame) = reader.next_frame().unwrap() {
        if frame.at.0 <= 8_020_000 {
            send.send(frame).unwrap();
        }
    }
    wait_until(|| {
        runtime
            .snapshot()
            .fights
            .iter()
            .any(|f| f.total_damage == 300.)
    });
    wait_until(|| runtime.store_status().queued == 0);
    let path = directory.path().join("demo.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(runtime.shutdown().is_err());
    assert!(
        !runtime.closing.load(Ordering::Acquire),
        "cancelled exit permits recovery/restart"
    );
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status().state, CaptureState::Stopped);
    assert_eq!(runtime.snapshot().capture.state, CaptureState::Stopped);
    assert_eq!(
        runtime.store_status().state,
        maxed_types::StoreState::Failed
    );
    assert!(runtime.store_status().queued > 0);
    assert!(runtime.shared.recovery.lock().unwrap().is_some());
    assert!(runtime.stop().is_err());
    assert!(path.exists(), "recovery retains the demo database");
    db.execute_batch("ROLLBACK").unwrap();
    drop(db);
    runtime.retry_store().unwrap();
    runtime.shutdown().unwrap();
    assert_eq!(runtime.store_status().queued, 0);
    assert!(
        path.exists(),
        "successful recovery retains the last demo database for history queries"
    );
    let events = recorded.lock().unwrap();
    assert!(events.iter().any(|e| matches!(&e.event, maxed_types::EngineEvent::RunEnded(r)
        if r.outcome == Some(maxed_types::Outcome::Unknown) && r.evidence.iter().any(|e| e.evidence == maxed_types::Evidence::AppClosed))));
    assert!(events.iter().any(|e| matches!(&e.event, maxed_types::EngineEvent::FightEnded(f) if f.end_reason == Some(FightEnd::AppClosed))));
}

#[test]
fn worker_panic_and_poisoned_lock_do_not_veto_shutdown() {
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    runtime
        .start_with(sink, || panic!("synthetic worker failure"))
        .unwrap();
    wait_until(|| runtime.shared.failed.load(Ordering::Acquire));
    assert!(runtime.shutdown().is_ok());
    assert!(!runtime.has_unsaved_summaries());
    let runtime = Arc::new(CaptureRuntime::default());
    let other = runtime.clone();
    let _ = thread::spawn(move || {
        let _guard = other.session.lock().unwrap();
        panic!("synthetic poison");
    })
    .join();
    assert!(runtime.shutdown().is_ok());
}

#[test]
fn snapshot_subscribers_receive_waiting_state_without_any_game_traffic() {
    let runtime = CaptureRuntime::default();
    let (sink, _) = sink();
    let (_frames_tx, frames) = crossbeam_channel::unbounded();
    let backend = FakeBackend {
        device: Some(FakeDevice {
            frames,
            closes: Arc::default(),
        }),
        opening: None,
    };
    let directory = tempfile::tempdir().unwrap();
    let (snapshots_tx, snapshots) = crossbeam_channel::unbounded();
    runtime
        .launch(
            sink,
            Bridge {
                snapshot: Arc::new(move |s| {
                    snapshots_tx.send(s).unwrap();
                }),
                ..Default::default()
            },
            RuntimeOptions::live(
                directory.path().into(),
                Some(directory.path().join("invalid-client")),
            ),
            move || start_with_backend(CaptureConfig::default(), || Ok(backend)),
        )
        .unwrap();
    let initial = snapshots.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(initial.session.is_none());
    let waiting = snapshots.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(waiting.capture.state, CaptureState::WaitingForGame);
    assert!(waiting.session.is_none());
    assert!(waiting
        .issues
        .contains(&maxed_types::CoverageIssue::MissingCatalog));
    assert!(runtime
        .store_status()
        .message
        .contains("catalog validation failed"));
    assert!(!runtime.store_status().message.contains("Npcap"));
    runtime.stop().unwrap();
}

#[test]
fn stop_delivers_final_domain_and_snapshot_before_capture_stopped() {
    let directory = tempfile::tempdir().unwrap();
    let runtime = CaptureRuntime::default();
    let (send, frames) = crossbeam_channel::unbounded();
    let backend = FakeBackend {
        device: Some(FakeDevice {
            frames,
            closes: Arc::new(AtomicUsize::new(0)),
        }),
        opening: None,
    };
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let trace = recorded.clone();
    let publisher = Publisher::new(
        move |publication| {
            trace.lock().unwrap().push(publication);
            Ok(())
        },
        Arc::new(|| panic!("unexpected delivery failure")),
    )
    .unwrap();
    let sink = publisher.clone();
    runtime
        .launch(
            Arc::new(move |status| sink.publish(Publication::Capture(status))),
            CaptureRuntime::domain_bridge(publisher.clone()),
            RuntimeOptions::live(directory.path().into(), None),
            move || start_with_backend(CaptureConfig::default(), || Ok(backend)),
        )
        .unwrap();
    send.send(frame(10, true, &[0, 0, 0, 5, 105])).unwrap();
    wait_until(|| runtime.status().state == CaptureState::Live);
    runtime.stop().unwrap();
    publisher.flush().unwrap();
    let trace = recorded.lock().unwrap();
    let last_event = trace
        .iter()
        .rposition(|p| matches!(p, Publication::Event(_)))
        .unwrap();
    let last_snapshot = trace
        .iter()
        .rposition(|p| matches!(p, Publication::Snapshot(_)))
        .unwrap();
    let stopped = trace
        .iter()
        .position(|p| matches!(p, Publication::Capture(s) if s.state == CaptureState::Stopped))
        .unwrap();
    assert!(last_event < last_snapshot && last_snapshot < stopped);
    assert!(
        matches!(&trace[last_snapshot], Publication::Snapshot(s) if s.capture.state == CaptureState::Stopped)
    );
    assert_eq!(
        runtime.store_status().durable,
        Some(runtime.snapshot().sequence)
    );
}

#[test]
fn bridge_failure_is_visible_in_capture_queries_and_publications() {
    let shared = Shared::default();
    shared.bridge_failed.store(true, Ordering::Release);
    shared.failed.store(true, Ordering::Release);
    let (sink, events) = sink();
    shared.publish(CaptureStatus::default(), &sink);
    let status = events.recv().unwrap();
    assert_eq!(status.state, CaptureState::Failed);
    assert_eq!(status.message, crate::bridge::DELIVERY_FAILED);
    assert_eq!(shared.status(), status);
}

#[test]
fn status_read_returns_while_capture_sink_is_blocked() {
    let shared = Arc::new(Shared::default());
    let (entered, waiting) = bounded(1);
    let (release, gate) = bounded(1);
    let sink: StatusSink = Arc::new(move |_| {
        entered.send(()).unwrap();
        gate.recv().unwrap();
    });
    let publisher = shared.clone();
    let publishing = thread::spawn(move || {
        publisher.publish(
            CaptureStatus {
                state: CaptureState::Live,
                ..Default::default()
            },
            &sink,
        );
    });
    let entered = waiting.recv_timeout(Duration::from_secs(5));
    let (read, result) = bounded(1);
    let reader = thread::spawn(move || {
        read.send(shared.status()).unwrap();
    });
    let status = result.recv_timeout(Duration::from_secs(5));
    // Release and join even when the read times out, so a regression cannot hang cleanup.
    release.send(()).unwrap();
    publishing.join().unwrap();
    reader.join().unwrap();
    entered.unwrap();
    assert_eq!(status.unwrap().state, CaptureState::Live);
}

#[test]
fn concurrent_capture_publications_preserve_status_order_and_stopped_last() {
    let shared = Arc::new(Shared::default());
    let (entered, waiting) = bounded(1);
    let (release, gate) = bounded(1);
    let (delivered, events) = crossbeam_channel::unbounded();
    let observed = Arc::downgrade(&shared);
    let sink: StatusSink = Arc::new(move |status| {
        let shared = observed.upgrade().unwrap();
        if status.state == CaptureState::Live {
            // Delivery owns serialization even though status readers are free.
            entered
                .send(shared.status_delivery.try_lock().is_err())
                .unwrap();
            gate.recv().unwrap();
        }
        let current = shared.status();
        delivered.send((status.state, current.state)).unwrap();
    });
    let first_shared = shared.clone();
    let first_sink = sink.clone();
    let first = thread::spawn(move || {
        first_shared.publish(
            CaptureStatus {
                state: CaptureState::Live,
                ..Default::default()
            },
            &first_sink,
        );
    });
    let serialized = waiting.recv_timeout(Duration::from_secs(5));
    let (started, attempting) = bounded(0);
    let last = thread::spawn(move || {
        started.send(()).unwrap();
        shared.publish(CaptureStatus::default(), &sink);
    });
    let attempted = attempting.recv_timeout(Duration::from_secs(5));
    release.send(()).unwrap();
    first.join().unwrap();
    last.join().unwrap();
    assert!(serialized.unwrap());
    attempted.unwrap();
    assert_eq!(
        events.try_iter().collect::<Vec<_>>(),
        vec![
            (CaptureState::Live, CaptureState::Live),
            (CaptureState::Stopped, CaptureState::Stopped),
        ]
    );
}
