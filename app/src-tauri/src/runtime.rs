mod account_batches;
mod account_history;
#[cfg(test)]
#[path = "../../../crates/maxed-store/tests/common/receipts.rs"]
mod receipt_tests;
mod recording;
use crate::bridge::{Publication, Publisher};
use crossbeam_channel::{bounded, select_biased, tick, Receiver, Sender};
use maxed_capture::{CaptureConfig, CaptureError, CaptureHandle};
use maxed_protocol::{DecodeEvent, Diagnostic, Packet};
use maxed_types::{CaptureState, CaptureStatus, FightEnd, Timestamp};
use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::Manager;

pub mod alerts;
mod character;
mod chat;
mod history;
mod home;
mod join;
pub mod maintenance;
#[cfg(test)]
mod maintenance_tests;
mod party;
pub(crate) use party::open_realmeye;
#[cfg(feature = "perf-live")]
pub mod perf;
mod pipeline;
mod replay;
mod stats;
mod stats_csv;
mod storage;
use join::{Adapter, Bridge, Views};
use pipeline::{Pipeline, PipelineEvent};
pub(crate) use stats::TauriSaveDialog;
pub use storage::{discover_resources, Namespace, RuntimeOptions};

const WORKER_FAILED: &str = "Capture worker failed; restart Maxed";
type StatusSink = Arc<dyn Fn(CaptureStatus) + Send + Sync>;
type SnapshotRequest = Sender<maxed_types::LiveSnapshot>;
#[cfg(test)]
type BeforeWorkerLoop =
    Arc<dyn Fn(&Receiver<SnapshotRequest>, &Receiver<()>) -> SnapshotRequest + Send + Sync>;

#[derive(Default)]
struct Shared {
    upgrade: maintenance::UpgradeLifecycle,
    recording: crate::recording::RecordingRuntime,
    alerts: Mutex<alerts::AlertSettingsState>,
    #[cfg(test)]
    on_alert_policy_read: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    on_alert_policy_save: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    after_character_calculation: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    home: Arc<home::HomeReads>,
    status: Mutex<CaptureStatus>,
    status_delivery: Mutex<()>,
    failed: AtomicBool,
    bridge_failed: AtomicBool,
    views: Arc<Views>,
    snapshot_requests: Mutex<Option<Sender<SnapshotRequest>>>,
    #[cfg(test)]
    before_worker_loop: Mutex<Option<BeforeWorkerLoop>>,
    recovery: Mutex<Option<Adapter>>,
    completed: Mutex<Option<Adapter>>,
}

fn failed_status() -> CaptureStatus {
    CaptureStatus {
        state: CaptureState::Failed,
        message: WORKER_FAILED.into(),
        ..Default::default()
    }
}

// Rebuild messages from an allowlist at the WebView boundary, never native errors.
fn sanitize(state: &CaptureState) -> &'static str {
    match state {
        CaptureState::Stopped => "Capture stopped.",
        CaptureState::NpcapMissing => "Install Npcap, then retry capture.",
        CaptureState::NpcapIncompatible => "Repair or update Npcap, then retry capture.",
        CaptureState::PermissionDenied => "Check Npcap access settings, then retry capture.",
        CaptureState::WaitingForGame => {
            "Start the game and enter a realm. Reconnect if capture started during play."
        }
        CaptureState::Live => "Game packets are being decoded.",
        CaptureState::Recovering => "Waiting for decoder recovery. Reconnect if this continues.",
        CaptureState::Failed => {
            "Capture could not continue. Check Npcap and your network adapter, then retry."
        }
    }
}

impl Shared {
    fn stopped_status(&self) -> CaptureStatus {
        CaptureStatus {
            state: CaptureState::Stopped,
            message: String::new(),
            ..self.status()
        }
    }

    fn status(&self) -> CaptureStatus {
        if self.bridge_failed.load(Ordering::Acquire) {
            return CaptureStatus {
                message: crate::bridge::DELIVERY_FAILED.into(),
                ..failed_status()
            };
        }
        match self.status.lock() {
            Ok(status) if !self.failed.load(Ordering::Acquire) => status.clone(),
            _ => {
                self.failed.store(true, Ordering::Release);
                failed_status()
            }
        }
    }

    fn publish(&self, mut status: CaptureStatus, sink: &StatusSink) {
        // Lock order: delivery -> status; release status before character/wake/sink.
        // Readers take only status. Keep delivery through the sink so updates and
        // publications stay ordered, including the final Stopped after worker join.
        let _delivery = self.status_delivery.lock().unwrap_or_else(|poison| {
            self.failed.store(true, Ordering::Release);
            poison.into_inner()
        });
        let mut current = self.status.lock().unwrap_or_else(|poison| {
            self.failed.store(true, Ordering::Release);
            poison.into_inner()
        });
        let message = if self.failed.load(Ordering::Acquire) {
            status.state = CaptureState::Failed;
            status.dropped_frames = 0;
            status.unknown_packets = 0;
            status.decode_failures = 0;
            if self.bridge_failed.load(Ordering::Acquire) {
                crate::bridge::DELIVERY_FAILED
            } else {
                WORKER_FAILED
            }
        } else {
            sanitize(&status.state)
        };
        if current.state != status.state
            || current.message != message
            || current.dropped_frames != status.dropped_frames
            || current.unknown_packets != status.unknown_packets
            || current.decode_failures != status.decode_failures
        {
            status.message.clear();
            status.message.push_str(message);
            *current = status.clone();
            drop(current);
            self.views
                .character
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .capture_changed(status.state.clone());
            self.views.alert_wake.signal();
            sink(status);
        }
    }
}

struct Session {
    stop: Sender<()>,
    cancel: Arc<AtomicBool>,
    worker: JoinHandle<()>,
    sink: StatusSink,
}

#[derive(Default)]
pub struct CaptureRuntime {
    shared: Arc<Shared>,
    session: Mutex<Option<Session>>,
    closing: Arc<AtomicBool>,
    publisher: Mutex<Option<Publisher>>,
}

impl CaptureRuntime {
    pub fn recording_status(&self, limit_bytes: u32) -> maxed_types::RecordingStatus {
        self.shared.recording.status(limit_bytes)
    }
    pub fn is_running(&self) -> bool {
        self.session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|s| !s.worker.is_finished())
    }

    /// The setup owner supplies an already-open capture and an immutable catalog
    /// lease. Wait for the engine/store join before accepting a new selection.
    pub fn start_prepared(
        &self,
        app: tauri::AppHandle,
        options: RuntimeOptions,
        handle: CaptureHandle,
    ) -> Result<(), String> {
        self.start_prepared_published(options, handle, self.publisher(app)?)
    }

    /// The production live join with an injected ordered publication sink.
    /// Capture still comes from a prepared handle; this does not bypass decoding.
    pub fn start_prepared_published(
        &self,
        options: RuntimeOptions,
        handle: CaptureHandle,
        publisher: Publisher,
    ) -> Result<(), String> {
        if options.namespace != Namespace::Live {
            return Err("Live capture requires the live namespace".into());
        }
        let status = publisher.clone();
        let (ready, wait) = bounded(1);
        self.launch(
            Arc::new(move |value| status.publish(Publication::Capture(value))),
            Self::domain_bridge(publisher.clone()),
            options,
            move || {
                let _ = ready.send(());
                Ok(handle)
            },
        )?;
        *self.publisher.lock().map_err(|_| WORKER_FAILED)? = Some(publisher);
        wait.recv().map_err(|_| {
            "Capture storage could not start. Retry storage before changing settings.".into()
        })
    }

    pub fn status(&self) -> CaptureStatus {
        self.shared.status()
    }

    pub fn start(&self, app: tauri::AppHandle) -> Result<(), String> {
        let options = RuntimeOptions::live(
            app.path()
                .app_data_dir()
                .map_err(|_| "Local app data is unavailable")?,
            discover_resources(),
        );
        self.start_configured(app, options)
    }

    fn start_configured(
        &self,
        app: tauri::AppHandle,
        options: RuntimeOptions,
    ) -> Result<(), String> {
        if options.namespace != Namespace::Live {
            return Err("Live capture requires the live namespace".into());
        }
        let publisher = self.publisher(app)?;
        let status_publisher = publisher.clone();
        self.launch(
            Arc::new(move |status| status_publisher.publish(Publication::Capture(status))),
            Self::domain_bridge(publisher),
            options,
            || maxed_capture::start_capture(CaptureConfig::default()),
        )
    }

    pub(crate) fn publisher(&self, app: tauri::AppHandle) -> Result<Publisher, String> {
        self.publisher_with(|failed| Publisher::native(app, failed))
    }

    // The factory substitutes only the presentation transport in tests; failure
    // reporting and publisher ownership are the production path.
    fn publisher_with(
        &self,
        create: impl FnOnce(Arc<dyn Fn() + Send + Sync>) -> Result<Publisher, String>,
    ) -> Result<Publisher, String> {
        let mut publisher = self.publisher.lock().map_err(|_| WORKER_FAILED)?;
        if let Some(publisher) = publisher.as_ref() {
            return Ok(publisher.clone());
        }
        let shared = Arc::downgrade(&self.shared);
        let created = create(Arc::new(move || {
            if let Some(shared) = shared.upgrade() {
                shared.bridge_failed.store(true, Ordering::Release);
            }
        }))?;
        let account_publisher = created.clone();
        *self
            .shared
            .home
            .account_notice
            .lock()
            .map_err(|_| WORKER_FAILED)? = Some(Arc::new(move |notice| {
            account_publisher.publish(Publication::AccountHistoryChanged(notice))
        }));
        *publisher = Some(created.clone());
        Ok(created)
    }

    fn domain_bridge(publisher: Publisher) -> Bridge {
        let events = publisher.clone();
        let single = publisher.clone();
        let party = publisher.clone();
        let account = publisher.clone();
        Bridge {
            account_changed: Arc::new(move |notice| {
                account.publish(Publication::AccountHistoryChanged(notice))
            }),
            visibility: publisher.visibility(),
            party_changed: Arc::new(move |notice| party.publish(Publication::PartyChanged(notice))),
            event: Arc::new(move |event| single.publish(Publication::Events(vec![event]))),
            batch: Some(Arc::new(move |batch| {
                events.publish(Publication::Events(batch))
            })),
            snapshot: Arc::new(move |snapshot| {
                publisher.publish(Publication::Snapshot(Box::new(snapshot)))
            }),
        }
    }

    pub(crate) fn presentation_failed(&self) -> bool {
        self.shared.bridge_failed.load(Ordering::Acquire)
    }

    pub(crate) fn reconnect_presentation(&self) -> Result<(), String> {
        let publisher = self.publisher.lock().map_err(|_| WORKER_FAILED)?.clone();
        let shared = self.shared.clone();
        if let Some(publisher) = publisher {
            publisher.reconnect(move || shared.bridge_failed.store(false, Ordering::Release))?;
        } else {
            shared.bridge_failed.store(false, Ordering::Release);
        }
        Ok(())
    }

    pub fn flush_bridge(&self) -> Result<(), String> {
        let publisher = self.publisher.lock().map_err(|_| WORKER_FAILED)?.clone();
        if let Some(publisher) = publisher {
            publisher.flush()?;
        }
        Ok(())
    }
    pub fn stop_and_flush(&self) -> Result<(), String> {
        let stopped = self.stop();
        self.flush_bridge()?;
        stopped
    }

    /// Setup holds its transition gate throughout this reversible install gate.
    pub(crate) fn prepare_update(&self) -> Result<(), String> {
        let session = self.session.lock().map_err(|_| WORKER_FAILED)?;
        self.shared
            .views
            .maintenance
            .update_paused
            .store(true, Ordering::Release);
        drop(session);
        self.shared.upgrade.cancel_and_wait();
        self.stop()?;
        let _ = self.flush_bridge(); // Presentation failure cannot veto durable update shutdown.
        if self
            .shared
            .recording
            .status(maxed_types::AppSettings::default().recording_limit_bytes)
            .failure
            .is_some()
        {
            return Err(
                "Recording storage failed; resolve recording storage before updating.".into(),
            );
        }
        self.shared.home.pause_for_update()
    }

    pub(crate) fn finish_update(&self) {
        self.shared.home.resume_after_update();
        self.shared
            .views
            .maintenance
            .update_paused
            .store(false, Ordering::Release);
        self.resume_cached_storage();
    }

    pub fn publish_preview(
        &self,
        app: tauri::AppHandle,
        snapshot: maxed_types::LiveSnapshot,
    ) -> Result<(), String> {
        self.publisher(app)?
            .publish(Publication::Snapshot(Box::new(snapshot)));
        self.flush_bridge()
    }

    #[cfg(test)]
    fn start_with(
        &self,
        sink: StatusSink,
        factory: impl FnOnce() -> Result<CaptureHandle, CaptureError> + Send + 'static,
    ) -> Result<(), String> {
        let temporary = tempfile::tempdir().unwrap();
        let mut options = RuntimeOptions::live(temporary.path().into(), None);
        options.temporary = Some(temporary);
        self.launch(sink, Bridge::default(), options, factory)
    }

    fn launch(
        &self,
        sink: StatusSink,
        bridge: Bridge,
        options: RuntimeOptions,
        factory: impl FnOnce() -> Result<CaptureHandle, CaptureError> + Send + 'static,
    ) -> Result<(), String> {
        let mut session = self.session.lock().map_err(|_| {
            self.shared.failed.store(true, Ordering::Release);
            self.shared.publish(failed_status(), &sink);
            WORKER_FAILED.to_string()
        })?;
        if self.closing.load(Ordering::Acquire) {
            return Err("Maxed is closing".into());
        }
        if self.shared.upgrade.running() {
            return Err("History upgrade is running; wait for it to finish.".into());
        }
        if self.shared.failed.load(Ordering::Acquire) {
            return Err(WORKER_FAILED.into());
        }
        if self
            .shared
            .recovery
            .lock()
            .map_err(|_| WORKER_FAILED)?
            .is_some()
        {
            return Err(
                "Pending summaries are not saved; retry storage before starting capture".into(),
            );
        }
        if let Some(previous) = session.as_ref() {
            if !previous.worker.is_finished() {
                return Err("Capture is already running".into());
            }
        }
        if let Some(previous) = session.take() {
            if previous.worker.join().is_err() {
                self.shared.failed.store(true, Ordering::Release);
                self.shared.publish(failed_status(), &sink);
                return Err(WORKER_FAILED.into());
            }
        }
        self.close_completed()?;
        let (stop, stopped) = bounded(1);
        // Rendezvous: Stop cannot strand an unclaimed reply in a buffered queue.
        let (snapshot_requests, snapshot_reads) = bounded(0);
        *self
            .shared
            .snapshot_requests
            .lock()
            .map_err(|_| WORKER_FAILED)? = Some(snapshot_requests);
        let cancel = Arc::new(AtomicBool::new(false));
        let shared = self.shared.clone();
        let worker_sink = sink.clone();
        let worker_cancel = cancel.clone();
        let closing = self.closing.clone();
        self.shared.publish(
            CaptureStatus {
                state: CaptureState::WaitingForGame,
                ..Default::default()
            },
            &sink,
        );
        let worker = thread::Builder::new()
            .name("maxed-app-capture".into())
            .spawn(move || {
                // A panic cannot expose packet contents or leave a stale Live snapshot.
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut adapter = match shared.open_home(&options, &worker_cancel, bridge, None)
                    {
                        Ok(adapter) => adapter,
                        Err(message) => {
                            let mut store =
                                shared.views.store.lock().unwrap_or_else(|p| p.into_inner());
                            store.state = maxed_types::StoreState::Failed;
                            store.message = message;
                            drop(store);
                            shared.publish(failed_status(), &worker_sink);
                            return;
                        }
                    };
                    let initial = adapter.capture(shared.status());
                    #[cfg(test)]
                    let hook = shared.before_worker_loop.lock().unwrap().take();
                    // Keep any claimed test reply alive through finalization;
                    // dropping it at worker exit releases the waiting reader.
                    #[cfg(test)]
                    let _pending_snapshot_reply = hook.map(|hook| hook(&snapshot_reads, &stopped));
                    let result = if initial.is_ok() {
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            run_worker(
                                factory,
                                stopped,
                                &snapshot_reads,
                                worker_cancel,
                                &shared,
                                &worker_sink,
                                &mut adapter,
                            )
                        }))
                        .unwrap_or_else(|_| {
                            shared.failed.store(true, Ordering::Release);
                            Err(WORKER_FAILED.into())
                        })
                    } else {
                        initial
                    };
                    // Transport close never chooses a lifecycle reason. The app's
                    // shutdown flag wins when Stop races with application exit.
                    let reason = if closing.load(Ordering::Acquire) {
                        FightEnd::AppClosed
                    } else {
                        FightEnd::CaptureStopped
                    };
                    let at = adapter.at();
                    let finalized = adapter.finish(at, reason);
                    let terminal_status =
                        if result.is_err() && !shared.failed.load(Ordering::Acquire) {
                            shared.stopped_status()
                        } else {
                            shared.status()
                        };
                    let _ = adapter.capture(terminal_status.clone());
                    adapter.publish_snapshot();
                    if result.is_err() {
                        shared.publish(terminal_status, &worker_sink);
                    }
                    if finalized.is_err() {
                        adapter.refresh_store();
                        *shared.recovery.lock().unwrap_or_else(|p| p.into_inner()) = Some(adapter);
                    } else {
                        // Keep the writer available until the lifecycle owner
                        // joins. A user Stop after a native failure must still
                        // persist and publish the final capture-state envelope.
                        *shared.completed.lock().unwrap_or_else(|p| p.into_inner()) = Some(adapter);
                    }
                }));
                if result.is_err() {
                    shared.failed.store(true, Ordering::Release);
                    shared.publish(failed_status(), &worker_sink);
                }
            })
            .map_err(|_| {
                self.shared.publish(failed_status(), &sink);
                WORKER_FAILED.to_string()
            })?;
        *session = Some(Session {
            stop,
            cancel,
            worker,
            sink,
        });
        Ok(())
    }

    pub fn stop(&self) -> Result<(), String> {
        // Hold the lifecycle lock across join so a new start cannot overtake cleanup.
        let mut session = self.session.lock().unwrap_or_else(|poison| {
            self.shared.failed.store(true, Ordering::Release);
            poison.into_inner()
        });
        if self.shared.upgrade.running() {
            return Err("History upgrade is running; wait for it to finish.".into());
        }
        // Retire calculation freshness before joining. A reader released by the
        // worker's exit must not briefly return Live before final status delivery.
        self.shared
            .views
            .character
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .capture_changed(CaptureState::Stopped);
        self.shared.views.maintenance.handoff_background();
        self.retire_alert_delivery();
        if let Some(session) = session.take() {
            session.cancel.store(true, Ordering::Release);
            let _ = session.stop.try_send(());
            if session.worker.join().is_err() {
                self.shared.failed.store(true, Ordering::Release);
            }
            self.shared.recording.retire();
            let closed = self.close_completed();
            let mut recovery = self
                .shared
                .recovery
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            if let Some(adapter) = recovery.as_mut() {
                let _ = adapter.capture(self.shared.stopped_status());
                adapter.publish_snapshot();
                adapter.refresh_store();
            }
            drop(recovery);
            self.shared
                .publish(self.shared.stopped_status(), &session.sink);
            closed?;
        }
        self.shared.recording.retire();
        if self
            .shared
            .recovery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
        {
            return Err("Changes are not saved; retry summary storage before closing".into());
        }
        let result = if self.shared.failed.load(Ordering::Acquire) {
            Err(WORKER_FAILED.into())
        } else {
            Ok(())
        };
        drop(session);
        if result.is_ok() {
            self.resume_cached_storage();
        }
        result
    }

    fn close_completed(&self) -> Result<(), String> {
        let completed = self
            .shared
            .completed
            .lock()
            .map_err(|_| WORKER_FAILED)?
            .take();
        if let Some(mut adapter) = completed {
            let captured = adapter.capture(self.shared.stopped_status());
            adapter.publish_snapshot();
            if captured.is_err() || adapter.close().is_err() {
                adapter.refresh_store();
                *self.shared.recovery.lock().map_err(|_| WORKER_FAILED)? = Some(adapter);
                return Err("Changes are not saved; retry summary storage before closing".into());
            }
        }
        Ok(())
    }

    /// Only retained unsaved summaries prevent exit. Worker failure is not an
    /// exit veto, and cancelled exit returns the runtime to its normal lifecycle.
    pub fn shutdown(&self) -> Result<(), String> {
        let session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        self.closing.store(true, Ordering::Release);
        drop(session);
        self.shared.upgrade.cancel_and_wait();
        self.shared.views.maintenance.handoff_background();
        self.shared.home.close_idle_reader();
        let _ = self.stop();
        // Join the live owner before rejecting: its last turn may have put a
        // busy request back on the FIFO after the initial handoff barrier.
        self.shared.views.maintenance.stop_background();
        // Final events and the terminal snapshot must drain before exit, even
        // when storage failed. Retain an IPC failure in the queryable status.
        let delivery = self.flush_bridge();
        if delivery.is_err() {
            self.shared.bridge_failed.store(true, Ordering::Release);
        }
        if self.has_unsaved_summaries() {
            self.closing.store(false, Ordering::Release);
            Err("Changes are not saved; retry storage or exit without saving".into())
        } else {
            self.shared.home.shutdown_reads();
            self.shutdown_alert_delivery();
            delivery
        }
    }

    pub fn has_unsaved_summaries(&self) -> bool {
        self.shared
            .recovery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    pub fn exit_without_saving(&self) {
        let session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        self.closing.store(true, Ordering::Release);
        drop(session);
        self.shared.upgrade.cancel_and_wait();
        let _ = self.stop();
        self.shared.views.maintenance.stop_background();
        self.shared.home.shutdown_reads();
        self.shutdown_alert_delivery();
        let _session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(mut adapter) = self
            .shared
            .recovery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            adapter.discard();
        }
    }
}

impl Drop for CaptureRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn run_worker(
    factory: impl FnOnce() -> Result<CaptureHandle, CaptureError>,
    stopped: Receiver<()>,
    snapshot_reads: &Receiver<SnapshotRequest>,
    cancel: Arc<AtomicBool>,
    shared: &Shared,
    sink: &StatusSink,
    adapter: &mut Adapter,
) -> Result<(), String> {
    let handle = match factory() {
        Ok(handle) => handle,
        Err(error) => {
            shared.publish(
                CaptureStatus {
                    state: match error {
                        CaptureError::Missing => CaptureState::NpcapMissing,
                        CaptureError::Incompatible => CaptureState::NpcapIncompatible,
                        CaptureError::PermissionDenied => CaptureState::PermissionDenied,
                        _ => CaptureState::Failed,
                    },
                    ..Default::default()
                },
                sink,
            );
            adapter.capture(shared.status())?;
            return Ok(());
        }
    };
    let mut pipeline = Pipeline::new(cancel.clone());
    let mut unknown = 0u32;
    let mut failures = 0u32;
    let mut open_streams = HashSet::new();
    let mut waiting_for_game = true;
    // CaptureHandle has no status channel. Poll native discovery/failure status only;
    // frame-driven decoder transitions are published immediately below.
    // Stop has its own priority channel and decoder cancellation; it never waits for this tick.
    let clock = tick(Duration::from_millis(250));
    let committed = adapter.committed();
    let mut time = CaptureClock::default();
    let presentation_clock = Instant::now();
    let mut storage_error = None;
    while !cancel.load(Ordering::Acquire) {
        let mut disconnected = false;
        select_biased! {
            recv(stopped) -> _ => break,
            recv(snapshot_reads) -> request => {
                if let Ok(reply) = request {
                    let _ = reply.send(adapter.read_snapshot());
                }
            },
            recv(committed) -> _ => {
                if let Err(error) = adapter.drain() { storage_error = Some(error); }
            },
            recv(handle.frames) -> frame => match frame {
                Ok(frame) => {
                    let _ = shared.recording.try_record(&frame);
                    let at = frame.at;
                    let arrived = Instant::now();
                    // Expire an actual idle gap before accepting returning traffic.
                    // The incoming capture timestamp cannot establish inactivity.
                    if time.quiet(arrived) >= Duration::from_secs(1800) {
                        if let Some(at) = time.tick(arrived) {
                            if let Err(error) = adapter.live_tick(at, time.quiet(arrived)) { storage_error = Some(error); }
                        }
                    }
                    let output = pipeline.push_frame(frame);
                    let events = output.events;
                    if output.trusted_traffic {
                        if let Err(error) = adapter.live_traffic(at) { storage_error = Some(error); }
                        time.frame(at.max(adapter.at()), arrived);
                    }
                    let mut decode_report = None;
                    for event in events {
                        // A decoded batch is owned already. Retain all its events
                        // even if Stop or a writer error arrives midway through it.
                        match &event {
                            PipelineEvent::Opened { conn, dir } => { open_streams.insert((*conn, *dir)); }
                            PipelineEvent::Decoded(DecodeEvent::Packet(packet)) if packet.alignment_verified && !matches!(packet.packet, Packet::Unknown { .. }) => {
                                decode_report = Some(true); waiting_for_game = false;
                            }
                            PipelineEvent::Decoded(DecodeEvent::Diagnostic { kind: Diagnostic::UnknownId, .. }) => { unknown = unknown.saturating_add(1); }
                            PipelineEvent::Decoded(DecodeEvent::Diagnostic { kind: Diagnostic::Recovering, .. }) => { decode_report = Some(false); waiting_for_game = false; }
                            PipelineEvent::Decoded(DecodeEvent::Diagnostic { kind: Diagnostic::BadFrame | Diagnostic::BadFields | Diagnostic::TrailingBytes, .. }) => { failures = failures.saturating_add(1); }
                            PipelineEvent::Decoded(DecodeEvent::Diagnostic { conn, dir, kind: Diagnostic::Closed, .. }) => {
                                open_streams.remove(&(*conn, *dir));
                                if open_streams.is_empty() { waiting_for_game = true; }
                            }
                            _ => {}
                        }
                        if let Err(error) = adapter.decoded_observed(event) { storage_error = Some(error); }
                    }
                    if let Some(live) = decode_report {
                        handle.report_decode(live, 0, 0);
                    }
                }
                Err(_) => disconnected = true,
            },
            recv(clock) -> _ => {
                // Frames have priority: never advance past buffered capture.
                if let Err(error) = time.service_tick(adapter, Instant::now()) { storage_error = Some(error); }
                // Readiness changes must publish even before the first frame.
                time.publish_due(adapter, presentation_clock.elapsed(), Instant::now());
                adapter.refresh_store();
            },
        }
        publish_handle(
            &handle,
            unknown,
            failures,
            waiting_for_game,
            shared,
            sink,
            &cancel,
        );
        if let Err(error) = adapter.capture(shared.status()) {
            storage_error = Some(error);
        }
        // A continuously populated frame queue must not starve presentation.
        time.publish_due(adapter, presentation_clock.elapsed(), Instant::now());
        if disconnected || storage_error.is_some() {
            break;
        }
    }
    if let Err(error) = stop_pipeline(&mut pipeline, adapter) {
        storage_error = Some(error);
    }
    let _ = handle.stop();
    match storage_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn stop_pipeline(pipeline: &mut Pipeline, adapter: &mut Adapter) -> Result<(), String> {
    let mut result = Ok(());
    // The pipeline labels synthetic teardown as cancellation for every caller.
    // The lifecycle owner supplies CaptureStopped/AppClosed after worker join.
    for event in pipeline.finish_at(adapter.at()) {
        if let Err(error) = adapter.decoded(event) {
            result = Err(error);
        }
    }
    result
}

#[derive(Default)]
struct CaptureClock {
    anchor: Option<(Timestamp, Instant)>,
}
impl CaptureClock {
    const SAFETY_MARGIN: Duration = Duration::from_secs(5);
    fn frame(&mut self, at: Timestamp, arrived: Instant) {
        let at = self.anchor.map_or(at, |(previous, _)| at.max(previous));
        self.anchor = Some((at, arrived));
    }
    fn quiet(&self, now: Instant) -> Duration {
        self.anchor.map_or(Duration::ZERO, |(_, arrived)| {
            now.saturating_duration_since(arrived)
                .saturating_sub(Self::SAFETY_MARGIN)
        })
    }
    fn service_tick(&self, adapter: &mut Adapter, now: Instant) -> Result<(), String> {
        if let Some(at) = self.tick(now) {
            // live_tick drains events and services storage maintenance.
            adapter.live_tick(at, self.quiet(now))?;
        } else {
            // History writes and retention do not depend on the first frame.
            adapter.service_maintenance();
        }
        Ok(())
    }
    fn publish_due(&self, adapter: &mut Adapter, elapsed: Duration, now: Instant) {
        // Presentation recency uses capture time plus the full arrival-clock
        // elapsed time. The delayed expiry clock must not extend the 4 Hz window.
        let at = self.anchor.map_or(adapter.at(), |(at, arrived)| {
            let elapsed = now.saturating_duration_since(arrived);
            Timestamp(at.0.saturating_add(elapsed.as_micros().min(u64::MAX as u128) as u64))
        });
        adapter.publish_due_at(elapsed, at.max(adapter.at()));
    }
    fn tick(&self, now: Instant) -> Option<Timestamp> {
        self.anchor.map(|(at, _)| {
            // Leave five seconds for delayed Npcap batches; only genuine quiet
            // time advances idle expiry beyond the last observed frame timestamp.
            let quiet = self.quiet(now);
            Timestamp(at.0.saturating_add(quiet.as_micros().min(u64::MAX as u128) as u64))
        })
    }
}

fn publish_handle(
    handle: &CaptureHandle,
    unknown: u32,
    failures: u32,
    waiting_for_game: bool,
    shared: &Shared,
    sink: &StatusSink,
    cancel: &AtomicBool,
) {
    if !cancel.load(Ordering::Acquire) {
        let mut status = handle.status();
        status.unknown_packets = unknown;
        status.decode_failures = failures;
        if waiting_for_game
            && matches!(
                status.state,
                CaptureState::WaitingForGame | CaptureState::Live | CaptureState::Recovering
            )
        {
            status.state = CaptureState::WaitingForGame;
        }
        shared.publish(status, sink);
    }
}

#[cfg(test)]
mod alert_tests;
#[cfg(test)]
mod alerts_history;
#[cfg(test)]
mod character_tests;
#[cfg(test)]
mod chat_tests;
#[cfg(test)]
mod history_replay_tests;
#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod home_replay_tests;
#[cfg(test)]
mod home_tests;
#[cfg(test)]
mod join_tests;
#[cfg(test)]
mod key_pop_tests;
#[cfg(test)]
mod loot_tests;
#[cfg(test)]
mod party_tests;
#[cfg(test)]
mod stats_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod storage_recovery_tests;
