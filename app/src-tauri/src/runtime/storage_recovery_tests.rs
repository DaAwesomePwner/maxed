use super::*;
use maxed_capture::{
    backend::{start_with_backend, Backend, Device},
    AdapterInfo,
};
use maxed_types::RawFrame;
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

#[test]
fn retry_preserves_summaries_and_allows_capture_restart() {
    recovery_scenario(false);
}

#[test]
fn native_close_retries_recovered_writer_without_discard() {
    recovery_scenario(true);
}

// Use the real runtime for every storage/lifecycle operation; only the native
// window and process exit are replaced by observations.
struct RuntimeExit<'a> {
    runtime: &'a CaptureRuntime,
    discarded: std::cell::Cell<bool>,
    code: std::cell::Cell<Option<i32>>,
}
impl crate::exit::ExitHost for RuntimeExit<'_> {
    fn installing(&self) -> bool {
        false
    }
    fn stop_updates(&self) -> Result<(), String> {
        Ok(())
    }
    fn prepare(&self) {}
    fn resume(&self) {}
    fn unsaved(&self) -> bool {
        self.runtime.has_unsaved_summaries()
    }
    fn delivery_failed(&self) -> bool {
        self.runtime.presentation_failed()
    }
    fn retry_save(&self) -> Result<(), String> {
        self.runtime.retry_store()
    }
    fn shutdown(&self) -> Result<(), String> {
        self.runtime.shutdown()
    }
    fn discard(&self) {
        self.discarded.set(true);
        self.runtime.exit_without_saving();
        let _ = self.runtime.flush_bridge();
    }
    fn warn(&self, _: bool) {}
    fn exit(&self, code: i32) {
        self.code.set(Some(code));
    }
}

fn recovery_scenario(native_close: bool) {
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
    let control = crate::exit::ExitControl::default();
    let host = RuntimeExit {
        runtime: &runtime,
        discarded: std::cell::Cell::new(false),
        code: std::cell::Cell::new(None),
    };
    if native_close {
        let publisher = runtime
            .publisher_with(|failed| Publisher::new(|_| Err(()), failed))
            .unwrap();
        publisher.publish(Publication::Capture(CaptureStatus::default()));
        assert!(publisher.flush().is_err());
        assert!(runtime.presentation_failed());
        assert!(crate::exit::run(&control, &host, false).is_err());
        assert_eq!(host.code.get(), None);
    } else {
        assert!(runtime.shutdown().is_err());
    }
    assert!(
        !runtime.closing.load(Ordering::Acquire),
        "cancelled exit permits recovery/restart"
    );
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    if native_close {
        assert_eq!(runtime.status().state, CaptureState::Failed);
        assert_eq!(runtime.status().message, crate::bridge::DELIVERY_FAILED);
        assert_eq!(
            runtime.shared.status.lock().unwrap().state,
            CaptureState::Stopped
        );
    } else {
        assert_eq!(runtime.status().state, CaptureState::Stopped);
    }
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
    if native_close {
        // shutdown still refuses retained recovery even though SQLite is now writable.
        assert!(runtime.shutdown().is_err());
        thread::sleep(crate::exit::NATIVE_DISCARD_DELAY + Duration::from_millis(20));
        crate::exit::run(&control, &host, false).unwrap();
        assert_eq!(host.code.get(), Some(1));
        assert!(!host.discarded.get());
        assert!(!runtime.has_unsaved_summaries());
        let saved = rusqlite::Connection::open(&path).unwrap();
        let ended: i64 = saved.query_row(
            "SELECT COUNT(*) FROM fights WHERE json_extract(json, '$.end_reason') = 'app_closed'",
            [], |row| row.get(0),
        ).unwrap();
        assert!(ended > 0, "the close-time summary was committed by retry");
    } else {
        runtime.retry_store().unwrap();
        assert!(!runtime.has_unsaved_summaries());
        let (_send, _) = start_fake(&runtime, Arc::new(|_| {}));
        runtime.shutdown().unwrap();
    }
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
fn presentation_failure_allows_capture_update_and_reconnect() {
    let runtime = CaptureRuntime::default();
    let failing = Arc::new(AtomicBool::new(true));
    let emit_failure = failing.clone();
    let publisher = runtime
        .publisher_with(|failed| {
            Publisher::new(
                move |_| {
                    if emit_failure.load(Ordering::Acquire) {
                        Err(())
                    } else {
                        Ok(())
                    }
                },
                failed,
            )
        })
        .unwrap();
    publisher.publish(Publication::Capture(CaptureStatus::default()));
    assert!(publisher.flush().is_err());
    assert!(!runtime.shared.failed.load(Ordering::Acquire));
    let (_send, _) = start_fake(&runtime, Arc::new(|_| {}));
    runtime.prepare_update().unwrap();
    runtime.finish_update();
    failing.store(false, Ordering::Release);
    runtime.reconnect_presentation().unwrap();
    assert!(!runtime.shared.bridge_failed.load(Ordering::Acquire));
    publisher.flush().unwrap();
    assert_ne!(runtime.status().state, CaptureState::Failed);
}

#[test]
fn retry_without_pending_storage_keeps_capture_running() {
    let runtime = CaptureRuntime::default();
    let (_send, closes) = start_fake(&runtime, Arc::new(|_| {}));
    wait_until(|| runtime.status().state == CaptureState::WaitingForGame);
    runtime.retry_store().unwrap();
    assert!(runtime.is_running());
    assert_eq!(closes.load(Ordering::Acquire), 0);
    runtime.stop().unwrap();
}
