use crate::{
    maintenance::{Control, ControlResult, JobStatus, MaintenanceService, Step},
    migrate::{digest, open_writer},
    project::{apply_contexts, json, validate_submission},
    receipts::{self, ReceiptDigest},
    StoreError,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use maxed_types::*;
use rusqlite::Connection;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_QUEUED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct WriterConfig {
    pub stream_id: String,
    pub capacity: usize,
    pub batch_size: usize,
    pub flush_millis: u64,
}

impl Default for WriterConfig {
    fn default() -> Self {
        Self {
            stream_id: "live".into(),
            capacity: 1024,
            batch_size: 128,
            flush_millis: 250,
        }
    }
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // Ownership is deliberately returned intact by this frozen API.
pub enum SubmitError {
    Invalid(EventEnvelope),
    Full(EventEnvelope),
    Failed(EventEnvelope),
    Closed(EventEnvelope),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlushReceipt {
    pub stream_id: String,
    pub sequence: Sequence,
}

struct Pending {
    event: Arc<EventEnvelope>,
    context: Arc<crate::AlertContext>,
    bytes: usize,
    ticket: u64,
}
struct Shared {
    path: PathBuf,
    maintenance_blocked: bool,
    maintenance_barriers: usize,
    queue: VecDeque<Pending>,
    bytes: usize,
    last_ticket: u64,
    urgent_through: u64,
    committed: Sender<()>,
    state: StoreState,
    durable: Option<Sequence>,
    failure: Option<StoreError>,
    accepting: bool,
}
type Reply = Sender<Result<FlushReceipt, StoreError>>;
enum Command {
    AttachLegacy(
        String,
        Vec<String>,
        Sender<Result<crate::accounts::AttachCounts, StoreError>>,
    ),
    RecoveryProgress(String, u16, Sender<Result<(), StoreError>>),
    Maintenance(Control, Sender<Result<ControlResult, StoreError>>),
    PartyMark(
        String,
        String,
        Option<PartyMark>,
        Option<String>,
        Sender<Result<(), StoreError>>,
    ),
    ClearParty(ClearPartyHistory, Sender<Result<(), StoreError>>),
    Chat(
        crate::chat::ChatMaintenance,
        Sender<Result<u64, StoreError>>,
    ),
    Claim(u16, Sender<Result<Vec<crate::CommittedAlert>, StoreError>>),
    Flush(u64, Reply),
    Retry(u64, Reply),
    Shutdown(u64, Reply),
    #[cfg(test)]
    Inject(crate::project::fault::Point, i32, Sender<()>),
    #[cfg(test)]
    Panic,
}

/// Own on a background orchestration thread. Explicit shutdown is required to
/// receive unsaved events on failure; dropping without shutdown logs an error.
pub struct StoreWriter {
    config: WriterConfig,
    shared: Arc<Mutex<Shared>>,
    commands: Sender<Command>,
    wake: Sender<()>,
    committed: Receiver<()>,
    maintenance_notices: Receiver<JobStatus>,
    worker: Option<JoinHandle<()>>,
    _lease: WriterLease,
    conversion: Option<crate::maintenance::ConversionAccess>,
}

pub(crate) struct WriterLease(PathBuf);
fn writer_paths() -> &'static Mutex<std::collections::HashSet<PathBuf>> {
    static PATHS: std::sync::OnceLock<Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    PATHS.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}
impl WriterLease {
    pub(crate) fn acquire(path: &std::path::Path) -> Result<Self, StoreError> {
        let path = if path.exists() {
            std::fs::canonicalize(path).map_err(|_| StoreError::Open)?
        } else {
            std::fs::canonicalize(
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(std::path::Path::new(".")),
            )
            .map_err(|_| StoreError::Open)?
            .join(path.file_name().ok_or(StoreError::Open)?)
        };
        if !writer_paths()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(path.clone())
        {
            return Err(StoreError::Busy);
        }
        Ok(Self(path))
    }

    pub(crate) fn has_abandoned_events(&self) -> bool {
        abandoned()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .any(|shared| {
                let state = lock(shared);
                state.path == self.0 && !state.queue.is_empty()
            })
    }
}
impl Drop for WriterLease {
    fn drop(&mut self) {
        writer_paths()
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&self.0);
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|poison| poison.into_inner())
}

pub fn start_writer(path: PathBuf, config: WriterConfig) -> Result<StoreWriter, StoreError> {
    let conversion = crate::maintenance::ConversionAccess::new(path.clone());
    let mut writer = start_writer_with_maintenance(path, config, conversion.service())?;
    writer.conversion = Some(conversion);
    Ok(writer)
}

/// Install the maintenance executor/authorization seams on the single writer.
/// No jobs are requested by startup. Previously requested unpaused jobs may run.
pub fn start_writer_with_maintenance(
    path: PathBuf,
    config: WriterConfig,
    mut maintenance: MaintenanceService,
) -> Result<StoreWriter, StoreError> {
    if config.stream_id.is_empty()
        || config.capacity == 0
        || config.capacity > 1024
        || config.batch_size == 0
        || config.batch_size > 128
        || config.flush_millis == 0
        || config.flush_millis > 60_000
    {
        return Err(StoreError::InvalidEvent);
    }
    let lease = WriterLease::acquire(&path)?;
    let mut conn = open_writer(&path)?;
    recover_interrupted(&mut conn)?;
    conn.execute("UPDATE alert_occurrences SET state='suppressed' WHERE state='pending' AND source_stream_id<>?1", [&config.stream_id])?;
    let durable = receipts::head(&conn, &config.stream_id)?
        .map(|n| {
            u64::try_from(n)
                .map(|n| Sequence(Timestamp(n)))
                .map_err(|_| StoreError::InvalidEvent)
        })
        .transpose()?;
    let (commit_wake, committed) = bounded(1);
    let shared = Arc::new(Mutex::new(Shared {
        path: lease.0.clone(),
        maintenance_blocked: lease.has_abandoned_events(),
        maintenance_barriers: 0,
        queue: VecDeque::new(),
        bytes: 0,
        last_ticket: 0,
        urgent_through: 0,
        committed: commit_wake,
        state: StoreState::Ready,
        durable,
        failure: None,
        accepting: true,
    }));
    let (commands, command_rx) = bounded(8);
    let (notice_send, maintenance_notices) = bounded(1);
    let (wake, wake_rx) = bounded(1);
    let worker_shared = Arc::clone(&shared);
    let worker_config = config.clone();
    let worker = thread::Builder::new()
        .name("maxed-summary-writer".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                worker_loop(
                    &mut conn,
                    &worker_config,
                    &worker_shared,
                    command_rx,
                    wake_rx,
                    &mut maintenance,
                    notice_send,
                )
            }));
            if result.is_err() {
                let mut shared = lock(&worker_shared);
                shared.state = StoreState::Failed;
                shared.failure = Some(StoreError::WorkerFailed { pending: vec![] });
                tracing::error!("Summary worker panicked; pending summaries remain retained");
            }
        })
        .map_err(|_| StoreError::WorkerFailed { pending: vec![] })?;
    Ok(StoreWriter {
        config,
        shared,
        commands,
        wake,
        committed,
        maintenance_notices,
        worker: Some(worker),
        _lease: lease,
        conversion: None,
    })
}

impl StoreWriter {
    pub fn list_backups(
        &self,
    ) -> Result<Vec<crate::backup::ManagedBackup>, crate::backup::BackupError> {
        self.conversion
            .as_ref()
            .ok_or(crate::backup::BackupError::Invalid)?
            .list_backups()
    }
    pub fn backup_usage(&self) -> Result<crate::backup::BackupUsage, crate::backup::BackupError> {
        self.conversion
            .as_ref()
            .ok_or(crate::backup::BackupError::Invalid)?
            .backup_usage()
    }
    pub fn delete_backup(
        &self,
        id: crate::backup::BackupId,
        env: &dyn crate::backup::BackupEnvironment,
    ) -> Result<(), crate::backup::BackupError> {
        self.conversion
            .as_ref()
            .ok_or(crate::backup::BackupError::Invalid)?
            .delete_backup(id, env)
    }
    pub fn receipt_backup_task(
        &self,
    ) -> Result<crate::maintenance::ReceiptBackupTask, crate::backup::BackupError> {
        Ok(self
            .conversion
            .as_ref()
            .ok_or(crate::backup::BackupError::Invalid)?
            .backup_task())
    }

    /// Explicit user request; call on the orchestration thread, outside capture.
    pub fn request_receipt_conversion(
        &self,
        at: u64,
    ) -> Result<ControlResult, crate::backup::BackupError> {
        let request = self
            .conversion
            .as_ref()
            .ok_or(crate::backup::BackupError::Invalid)?
            .request(at);
        if matches!(request, Err(crate::backup::BackupError::OwnerPoisoned)) {
            let _ = self.maintenance(Control::BackupOwnerFailed);
        }
        Ok(self.maintenance(Control::Request(request?))?)
    }

    /// Creates/verifies this class's backup outside the writer, then wakes its
    /// job. Cancellation never implicitly resumes a durably paused job.
    pub fn backup_receipt_conversion(
        &self,
        env: &dyn crate::backup::BackupEnvironment,
        cancel: &std::sync::atomic::AtomicBool,
        between_steps: &mut dyn FnMut(),
    ) -> Result<crate::backup::BackupInfo, crate::backup::BackupError> {
        let result = self
            .conversion
            .as_ref()
            .ok_or(crate::backup::BackupError::Invalid)?
            .backup(env, cancel, between_steps);
        if matches!(result, Err(crate::backup::BackupError::OwnerPoisoned)) {
            let _ = self.maintenance(Control::BackupOwnerFailed);
        }
        let (info, id) = result?;
        self.maintenance(Control::AuthorizationChanged(id))?;
        Ok(info)
    }

    /// Durable request/pause/resume or metadata-only status on the writer thread.
    pub fn maintenance(&self, control: Control) -> Result<ControlResult, StoreError> {
        let (send, recv) = bounded(1);
        self.commands
            .send(Command::Maintenance(control, send))
            .map_err(|_| self.worker_failed())?;
        wait_response(recv, self.worker.as_ref().ok_or(StoreError::Closed)?)
    }

    pub fn maintenance_notices(&self) -> Receiver<JobStatus> {
        self.maintenance_notices.clone()
    }

    /// The orchestration owner sets this while retaining unaccepted capture
    /// events outside the writer queue. Clearing it never clears storage failure.
    pub fn set_maintenance_blocked(&self, blocked: bool) {
        lock(&self.shared).maintenance_blocked = blocked;
        let _ = self.wake.try_send(());
    }
    /// Coalesced durable-head wake for the single owning orchestration loop.
    /// Receivers share one permit; always read status after waking.
    pub fn committed(&self) -> Receiver<()> {
        self.committed.clone()
    }

    pub fn validate(&self, event: &EventEnvelope) -> Result<(), StoreError> {
        if event.stream_id != self.config.stream_id {
            return Err(StoreError::InvalidEvent);
        }
        validate_submission(event).map(|_| ())
    }

    #[allow(clippy::result_large_err)] // The required API returns ownership of the original event.
    pub fn try_submit(&self, event: EventEnvelope) -> Result<(), SubmitError> {
        self.try_submit_with_alert_context(event, crate::AlertContext::default())
    }

    #[allow(clippy::result_large_err)]
    pub fn try_submit_with_alert_context(
        &self,
        event: EventEnvelope,
        context: crate::AlertContext,
    ) -> Result<(), SubmitError> {
        let context_bytes = match context.validate() {
            Ok(n) => n,
            Err(_) => return Err(SubmitError::Invalid(event)),
        };
        if event.stream_id != self.config.stream_id {
            return Err(SubmitError::Invalid(event));
        }
        let bytes = match validate_submission(&event) {
            // Retain the legacy 32-byte receipt allowance even though hashing now
            // happens on the worker. Queue admission is byte-for-byte unchanged.
            Ok(bytes) => bytes
                .saturating_add(context_bytes)
                .saturating_add(std::mem::size_of::<ReceiptDigest>()),
            Err(_) => return Err(SubmitError::Invalid(event)),
        };
        // Never wait for the worker, another producer, or SQLite on the submit path.
        let mut shared = match self.shared.try_lock() {
            Ok(shared) => shared,
            Err(std::sync::TryLockError::WouldBlock) => return Err(SubmitError::Full(event)),
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(SubmitError::Failed(event)),
        };
        if !shared.accepting {
            return Err(SubmitError::Closed(event));
        }
        if shared.failure.is_some() {
            return Err(SubmitError::Failed(event));
        }
        if shared.queue.len() >= self.config.capacity
            || shared.bytes.saturating_add(bytes) > MAX_QUEUED_BYTES
        {
            shared.state = StoreState::Backpressured;
            return Err(SubmitError::Full(event));
        }
        let Some(ticket) = shared.last_ticket.checked_add(1) else {
            return Err(SubmitError::Invalid(event));
        };
        shared.last_ticket = ticket;
        if context.needs_prompt_commit(&event.event) {
            shared.urgent_through = ticket;
        }
        shared.bytes += bytes;
        shared.queue.push_back(Pending {
            event: Arc::new(event),
            context: Arc::new(context),
            bytes,
            ticket,
        });
        drop(shared);
        let _ = self.wake.try_send(());
        Ok(())
    }

    /// Flushes accepted events first so a clear covers everything submitted before it.
    /// The caller restricts maintenance to the live namespace. Returns the largest
    /// delete count across chat, alert and occurrence tables; repeat full batches.
    pub fn maintain_chat(&self, op: crate::chat::ChatMaintenance) -> Result<u64, StoreError> {
        self.flush()?;
        let (send, recv) = bounded(1);
        self.commands
            .send(Command::Chat(op, send))
            .map_err(|_| self.worker_failed())?;
        wait_response(recv, self.worker.as_ref().ok_or(StoreError::Closed)?)
    }

    /// Flush accepted events, then attach exact legacy IDs on the writer connection.
    pub fn attach_legacy_characters(
        &self,
        account_id: &str,
        candidate_ids: &[String],
    ) -> Result<crate::accounts::AttachCounts, StoreError> {
        self.flush()?;
        let (send, recv) = bounded(1);
        self.commands
            .send(Command::AttachLegacy(
                account_id.into(),
                candidate_ids.to_vec(),
                send,
            ))
            .map_err(|_| self.worker_failed())?;
        wait_response(recv, self.worker.as_ref().ok_or(StoreError::Closed)?)
    }

    /// Call after the matching attach operation succeeds; retries are monotonic.
    pub fn record_recovery_progress(
        &self,
        account_id: &str,
        completed_through: u16,
    ) -> Result<(), StoreError> {
        self.flush()?;
        let (send, recv) = bounded(1);
        self.commands
            .send(Command::RecoveryProgress(
                account_id.into(),
                completed_through,
                send,
            ))
            .map_err(|_| self.worker_failed())?;
        wait_response(recv, self.worker.as_ref().ok_or(StoreError::Closed)?)
    }

    pub fn set_party_mark(
        &self,
        key: &str,
        name: &str,
        mark: Option<PartyMark>,
        note: Option<&str>,
    ) -> Result<(), StoreError> {
        crate::queries::identifier(key)?;
        crate::queries::identifier(name)?;
        if note.is_some_and(|n| n.chars().count() > 500) {
            return Err(StoreError::InvalidEvent);
        }
        self.flush()?;
        let (send, recv) = bounded(1);
        self.commands
            .send(Command::PartyMark(
                key.into(),
                name.into(),
                mark,
                note.map(str::to_owned),
                send,
            ))
            .map_err(|_| self.worker_failed())?;
        wait_response(recv, self.worker.as_ref().ok_or(StoreError::Closed)?)
    }

    pub fn clear_party(&self, opts: &ClearPartyHistory) -> Result<(), StoreError> {
        self.flush()?;
        let (send, recv) = bounded(1);
        self.commands
            .send(Command::ClearParty(opts.clone(), send))
            .map_err(|_| self.worker_failed())?;
        wait_response(recv, self.worker.as_ref().ok_or(StoreError::Closed)?)
    }

    pub fn claim_alerts(&self, limit: u16) -> Result<Vec<crate::CommittedAlert>, StoreError> {
        if !(1..=32).contains(&limit) {
            return Err(StoreError::Limit);
        }
        let (send, recv) = bounded(1);
        self.commands
            .send(Command::Claim(limit, send))
            .map_err(|_| self.worker_failed())?;
        wait_response(recv, self.worker.as_ref().ok_or(StoreError::Closed)?)
    }

    pub fn status(&self) -> StoreStatus {
        let shared = lock(&self.shared);
        let message = if let Some(failure) = &shared.failure {
            match (shared.queue.front(), shared.queue.back()) {
                (Some(first), Some(last)) => format!(
                    "{failure}; unacknowledged sequence range {}..={}",
                    first.event.sequence.0 .0, last.event.sequence.0 .0
                ),
                _ => failure.to_string(),
            }
        } else {
            match shared.state {
                StoreState::Ready => "Summary writer ready",
                StoreState::Backpressured => {
                    "Summary queue full; retain and retry the returned event"
                }
                StoreState::Closing => "Saving pending summaries",
                StoreState::Closed => "Summary writer closed",
                StoreState::Failed => "Summary writer failed; pending summaries are not saved",
            }
            .into()
        };
        StoreStatus {
            state: shared.state.clone(),
            queued: shared.queue.len() as u32,
            durable: shared.durable,
            stream_id: Some(self.config.stream_id.clone()),
            message,
        }
    }

    fn request(&self, retry: bool) -> Result<FlushReceipt, StoreError> {
        let mut shared = lock(&self.shared);
        if !shared.accepting {
            return Err(StoreError::Closed);
        }
        let ticket = shared.last_ticket;
        shared.maintenance_barriers += 1;
        drop(shared);
        let (reply, response) = bounded(1);
        let command = if retry {
            Command::Retry(ticket, reply)
        } else {
            Command::Flush(ticket, reply)
        };
        self.commands
            .send(command)
            .map_err(|_| self.worker_failed())?;
        wait_response(response, self.worker.as_ref().ok_or(StoreError::Closed)?).map_err(|error| {
            match error {
                StoreError::WorkerFailed { .. } => self.worker_failed(),
                other => other,
            }
        })
    }

    fn worker_failed(&self) -> StoreError {
        // A non-consuming request cannot transfer the queue. Return a recovery
        // copy while keeping shutdown's ownership-preserving path intact.
        StoreError::WorkerFailed {
            pending: lock(&self.shared)
                .queue
                .iter()
                .map(|p| (*p.event).clone())
                .collect(),
        }
    }

    pub fn flush(&self) -> Result<FlushReceipt, StoreError> {
        self.request(false)
    }
    pub fn retry(&self) -> Result<(), StoreError> {
        self.request(true).map(|_| ())
    }

    /// Context-preserving recovery for adapters that supplied explicit policies.
    pub fn shutdown_with_alert_context(
        mut self,
    ) -> Result<FlushReceipt, crate::ContextualShutdownError> {
        self.stop().map_err(|error| {
            let pending = lock(&self.shared)
                .queue
                .drain(..)
                .map(|p| crate::ContextualEvent {
                    event: Arc::unwrap_or_clone(p.event),
                    context: Arc::unwrap_or_clone(p.context),
                })
                .collect();
            crate::ContextualShutdownError { error, pending }
        })
    }

    pub fn shutdown(mut self) -> Result<FlushReceipt, StoreError> {
        let result = self.stop();
        if result.is_err() {
            let pending = lock(&self.shared)
                .queue
                .drain(..)
                .map(|p| Arc::unwrap_or_clone(p.event))
                .collect();
            return Err(StoreError::WorkerFailed { pending });
        }
        result
    }

    fn stop(&mut self) -> Result<FlushReceipt, StoreError> {
        let Some(worker) = self.worker.take() else {
            return Err(StoreError::Closed);
        };
        let mut shared = lock(&self.shared);
        shared.accepting = false;
        if shared.failure.is_none() {
            shared.state = StoreState::Closing;
        }
        let ticket = shared.last_ticket;
        drop(shared);
        let (reply, response) = bounded(1);
        let result = if self.commands.send(Command::Shutdown(ticket, reply)).is_ok() {
            wait_response(response, &worker)
        } else {
            Err(StoreError::WorkerFailed { pending: vec![] })
        };
        if worker.join().is_err() {
            return Err(StoreError::WorkerFailed { pending: vec![] });
        }
        result
    }
}

fn wait_response<T>(
    response: Receiver<Result<T, StoreError>>,
    worker: &JoinHandle<()>,
) -> Result<T, StoreError> {
    wait_response_until(response, || worker.is_finished())
}

fn wait_response_until<T>(
    response: Receiver<Result<T, StoreError>>,
    is_finished: impl Fn() -> bool,
) -> Result<T, StoreError> {
    loop {
        match response.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => return result,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(StoreError::WorkerFailed { pending: vec![] })
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) if is_finished() => {
                return response
                    .try_recv()
                    .unwrap_or_else(|_| Err(StoreError::WorkerFailed { pending: vec![] }))
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
    }
}

impl Drop for StoreWriter {
    fn drop(&mut self) {
        if self.worker.is_some() && self.stop().is_err() {
            // Rust Drop cannot return ownership. Keep failed data for process
            // lifetime rather than silently destroying accepted summaries.
            retain_abandoned(Arc::clone(&self.shared));
            tracing::error!("Writer dropped without successful shutdown; unsaved summaries retained for process lifetime");
        }
    }
}

fn abandoned() -> &'static Mutex<Vec<Arc<Mutex<Shared>>>> {
    static ABANDONED: std::sync::OnceLock<Mutex<Vec<Arc<Mutex<Shared>>>>> =
        std::sync::OnceLock::new();
    ABANDONED.get_or_init(|| Mutex::new(Vec::new()))
}
fn retain_abandoned(shared: Arc<Mutex<Shared>>) {
    abandoned()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(shared);
}

/// Recover events retained by writers dropped without explicit shutdown.
/// Normal callers should instead handle `shutdown`'s `WorkerFailed.pending`.
pub fn take_abandoned_events() -> Vec<EventEnvelope> {
    let retained = std::mem::take(&mut *abandoned().lock().unwrap_or_else(|p| p.into_inner()));
    retained
        .into_iter()
        .flat_map(|shared| {
            lock(&shared)
                .queue
                .drain(..)
                .map(|p| Arc::unwrap_or_clone(p.event))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn receipt(config: &WriterConfig, shared: &Mutex<Shared>) -> FlushReceipt {
    FlushReceipt {
        stream_id: config.stream_id.clone(),
        sequence: lock(shared).durable.unwrap_or(Sequence(Timestamp(0))),
    }
}

fn mark_failed(shared: &Mutex<Shared>, error: &StoreError) {
    let mut shared = lock(shared);
    shared.state = StoreState::Failed;
    shared.failure = Some(error.clone());
}

fn commit_next(
    conn: &mut Connection,
    config: &WriterConfig,
    shared: &Mutex<Shared>,
    through: u64,
) -> Result<bool, StoreError> {
    let events: Vec<_> = lock(shared)
        .queue
        .iter()
        .take_while(|p| p.ticket <= through)
        .take(config.batch_size)
        .map(|p| (Arc::clone(&p.event), Arc::clone(&p.context)))
        .collect();
    if events.is_empty() {
        return Ok(false);
    }
    if let Err(error) = apply_contexts(conn, events.iter().map(|(e, c)| (e.as_ref(), c.as_ref()))) {
        mark_failed(shared, &error);
        return Err(error);
    }
    let mut shared = lock(shared);
    for (event, _) in &events {
        if let Some(pending) = shared.queue.pop_front() {
            shared.bytes -= pending.bytes;
        }
        let previous = shared.durable.map_or(0, |s| s.0 .0);
        shared.durable = Some(Sequence(Timestamp(previous.max(event.sequence.0 .0))));
    }
    shared.state = if shared.accepting {
        StoreState::Ready
    } else {
        StoreState::Closing
    };
    let _ = shared.committed.try_send(());
    Ok(true)
}

fn commit_ready(state: &Shared, config: &WriterConfig, elapsed: Duration) -> bool {
    state.failure.is_none()
        && !state.queue.is_empty()
        && (state.queue.len() >= config.batch_size
            || state
                .queue
                .front()
                .is_some_and(|p| p.ticket <= state.urgent_through)
            || elapsed >= Duration::from_millis(config.flush_millis))
}

fn flush_through(
    conn: &mut Connection,
    config: &WriterConfig,
    shared: &Mutex<Shared>,
    through: u64,
) -> Result<FlushReceipt, StoreError> {
    if let Some(error) = &lock(shared).failure {
        return Err(error.clone());
    }
    while commit_next(conn, config, shared, through)? {}
    // PASSIVE never waits for readers or removes history. WAL commits remain
    // durable even if a reader pins pages and prevents a full checkpoint.
    conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)")
        .inspect_err(|e| mark_failed(shared, &StoreError::from_sqlite_ref(e)))?;
    Ok(receipt(config, shared))
}

impl StoreError {
    fn from_sqlite_ref(error: &rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                Self::Busy
            }
            Some(rusqlite::ErrorCode::DiskFull) => Self::Full,
            _ => Self::Io,
        }
    }
}

fn maintenance_interrupted(shared: &Mutex<Shared>, commands: &Receiver<Command>) -> bool {
    let state = lock(shared);
    !commands.is_empty()
        || !state.queue.is_empty()
        || state.failure.is_some()
        || !state.accepting
        || state.maintenance_blocked
        || state.maintenance_barriers != 0
}

fn worker_loop(
    conn: &mut Connection,
    config: &WriterConfig,
    shared: &Mutex<Shared>,
    commands: Receiver<Command>,
    wake: Receiver<()>,
    maintenance: &mut MaintenanceService,
    notices: Sender<JobStatus>,
) {
    let interval = Duration::from_millis(config.flush_millis);
    let mut last_flush = Instant::now();
    let mut next_maintenance = Instant::now();
    loop {
        let ready = {
            let state = lock(shared);
            commit_ready(&state, config, last_flush.elapsed())
        };
        let mut wait = if ready {
            Duration::ZERO
        } else {
            interval.saturating_sub(last_flush.elapsed())
        };
        if maintenance.has_runnable_jobs() {
            wait = wait.min(next_maintenance.saturating_duration_since(Instant::now()));
        }
        crossbeam_channel::select_biased! {
            recv(commands) -> command => {
                let Ok(command) = command else { break; };
                let (through,reply,closing,retry) = match command {
                    Command::Maintenance(control, reply) => {
                        let wake = matches!(control, Control::Request(_) | Control::Resume(_) | Control::AuthorizationChanged(_));
                        let result = maintenance.control(conn, control);
                        if wake && result.is_ok() { next_maintenance = Instant::now(); }
                        let _ = reply.send(result);
                        continue;
                    }
                    Command::AttachLegacy(account, ids, reply) => { let _ = reply.send(crate::accounts::attach_legacy_characters(conn, &account, &ids)); continue; }
                    Command::RecoveryProgress(account, through, reply) => { let _ = reply.send(crate::accounts::record_recovery_progress(conn, &account, through)); continue; }
                    Command::PartyMark(key, name, mark, note, reply) => { let _ = reply.send(crate::party::set_mark(conn, &key, &name, mark, note.as_deref())); continue; }
                    Command::ClearParty(opts, reply) => { let _ = reply.send(crate::party::clear(conn, &opts)); continue; }
                    Command::Chat(op, reply) => { let _ = reply.send(crate::chat::maintain(conn, op)); continue; }
                    Command::Claim(limit, reply) => { let result = crate::alerts::claim(conn, &config.stream_id, limit); let _ = reply.send(result); continue; }
                    Command::Flush(t,r) => (t,r,false,false), Command::Retry(t,r) => (t,r,false,true), Command::Shutdown(t,r) => (t,r,true,false),
                    #[cfg(test)]
                    Command::Inject(point,code,reply) => { crate::project::fault::arm(point,code); let _=reply.send(()); continue; }
                    #[cfg(test)]
                    Command::Panic => panic!("synthetic worker failure"),
                };
                if !closing { let mut state=lock(shared); state.maintenance_barriers=state.maintenance_barriers.saturating_sub(1); }
                if retry { let mut state=lock(shared); state.failure=None; state.state=StoreState::Ready; }
                let result=flush_through(conn,config,shared,through);
                if closing && result.is_ok() { lock(shared).state=StoreState::Closed; }
                let _=reply.send(result); last_flush=Instant::now();
                if closing { break; }
            }
            recv(wake) -> _ => {}
            default(wait) => {}
        }
        let ready = {
            let state = lock(shared);
            commit_ready(&state, config, last_flush.elapsed())
        };
        if ready {
            let _ = commit_next(conn, config, shared, u64::MAX);
            last_flush = Instant::now();
        }
        if last_flush.elapsed() >= interval {
            last_flush = Instant::now();
        }
        if maintenance.has_runnable_jobs() && Instant::now() >= next_maintenance {
            let interrupted = || maintenance_interrupted(shared, &commands);
            let result = maintenance.step(conn, false, &interrupted);
            next_maintenance = Instant::now()
                + if matches!(result, Ok(Step::Committed(_))) {
                    // Return to the command/capture arbitration between units.
                    thread::yield_now();
                    Duration::ZERO
                } else {
                    Duration::from_secs(1)
                };
        }
        if !notices.is_full() {
            if let Some(notice) = maintenance.take_notice() {
                let _ = notices.try_send(notice);
            }
        }
    }
}

fn recover_interrupted(conn: &mut Connection) -> Result<(), StoreError> {
    let sessions = {
        let mut statement=conn.prepare("SELECT json FROM play_sessions WHERE ended IS NULL OR id IN (SELECT session_id FROM runs WHERE ended IS NULL UNION SELECT session_id FROM fights WHERE ended IS NULL UNION SELECT session_id FROM map_visits WHERE ended IS NULL) ORDER BY id")?;
        let strings = statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        strings
            .into_iter()
            .map(|s| serde_json::from_str::<PlaySessionView>(&s).map_err(StoreError::from))
            .collect::<Result<Vec<_>, _>>()?
    };
    for mut session in sessions {
        let runs: Vec<RunView> = read_json_rows(
            conn,
            "SELECT json FROM runs WHERE session_id=?1 AND ended IS NULL ORDER BY id",
            &session.id,
        )?;
        let fights: Vec<FightView> = read_json_rows(
            conn,
            "SELECT json FROM fights WHERE session_id=?1 AND ended IS NULL ORDER BY id",
            &session.id,
        )?;
        let stream = format!(
            "store-recovery-{}",
            digest(json(&(&session, &runs, &fights))?.as_bytes())
        );
        let at = Timestamp(
            runs.iter()
                .flat_map(|r| [Some(r.started), r.completed_at, r.died_at])
                .flatten()
                .chain(
                    fights
                        .iter()
                        .flat_map(|f| [Some(f.started), f.last_hit])
                        .flatten(),
                )
                .map(|t| t.0)
                .max()
                .unwrap_or(0)
                .max(session.last_traffic.0)
                .max(session.started.0),
        );
        session.ended = Some(
            session
                .ended
                .map_or(at, |ended| Timestamp(ended.0.max(at.0))),
        );
        let mut events = vec![EngineEvent::SessionUpdated(session)];
        for mut run in runs {
            run.ended = Some(at);
            run.complete_capture = false;
            if !matches!(run.outcome, Some(Outcome::Cleared | Outcome::Died)) {
                run.outcome = Some(Outcome::Unknown);
            }
            run.evidence.push(EvidenceAt {
                at,
                evidence: Evidence::AppClosed,
            });
            if !run.issues.contains(&CoverageIssue::StreamGap) {
                run.issues.push(CoverageIssue::StreamGap);
            }
            events.push(EngineEvent::RunEnded(run));
        }
        for mut fight in fights {
            fight.ended = Some(at);
            fight.end_reason = Some(FightEnd::AppClosed);
            fight.complete_capture = false;
            if !fight.issues.contains(&CoverageIssue::StreamGap) {
                fight.issues.push(CoverageIssue::StreamGap);
            }
            events.push(EngineEvent::FightEnded(fight));
        }
        let envelopes: Vec<_> = events
            .into_iter()
            .enumerate()
            .map(|(i, event)| EventEnvelope {
                fight_chart_omitted: false,
                stream_id: stream.clone(),
                sequence: Sequence(Timestamp(i as u64 + 1)),
                at,
                event,
            })
            .collect();
        for batch in envelopes.chunks(128) {
            crate::apply_batch(conn, batch)?;
        }
    }
    Ok(())
}

fn read_json_rows<T: serde::de::DeserializeOwned>(
    conn: &Connection,
    sql: &str,
    id: &str,
) -> Result<Vec<T>, StoreError> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map([id], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maintenance::{
        Environment, Job, JobExecutor, JobRequest, Limits, PauseReason, Record, UnitBudget,
        UnitOutcome,
    };
    use crate::project::fault::Point;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct MaintenanceClock;
    impl Environment for MaintenanceClock {
        fn monotonic(&self) -> Duration {
            Duration::ZERO
        }
        fn timestamp(&self) -> u64 {
            100
        }
    }
    struct MaintenanceProbe {
        entered: Sender<()>,
        release: Receiver<()>,
        calls: Arc<AtomicUsize>,
        failure: Option<i32>,
    }
    impl JobExecutor for MaintenanceProbe {
        fn execute(
            &mut self,
            tx: &rusqlite::Transaction<'_>,
            _: &Job,
            budget: &mut UnitBudget<'_>,
        ) -> Result<UnitOutcome, StoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.failure.is_some() {
                assert!(budget.reserve(Record::Receipt, 1));
            }
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(10)).unwrap();
            if self.failure.is_none() && !budget.reserve(Record::Receipt, 1) {
                return Ok(UnitOutcome::default());
            }
            tx.execute(
                "INSERT INTO event_streams(stream_id) VALUES('synthetic-maintenance')",
                [],
            )?;
            if let Some(code) = self.failure {
                return Err(
                    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(code), None).into(),
                );
            }
            Ok(UnitOutcome {
                cursor: Some("1".into()),
                ..Default::default()
            })
        }
    }
    fn maintenance_request() -> Control {
        Control::Request(JobRequest {
            kind: "synthetic".into(),
            key: "scope".into(),
            format_version: 1,
            input_generation: "generation".into(),
            input_revision: 0,
        })
    }
    fn maintenance_probe(
        failure: Option<i32>,
    ) -> (
        MaintenanceService,
        Receiver<()>,
        Sender<()>,
        Arc<AtomicUsize>,
    ) {
        let (entered, observed) = bounded(8);
        let (release, wait) = bounded(8);
        let calls = Arc::new(AtomicUsize::new(0));
        let service = MaintenanceService::new(
            Box::new(MaintenanceProbe {
                entered,
                release: wait,
                calls: calls.clone(),
                failure,
            }),
            Box::new(|_, _| Ok(true)),
            Box::new(MaintenanceClock),
            Limits::default(),
        )
        .unwrap();
        (service, observed, release, calls)
    }

    #[test]
    fn maintenance_yields_to_queued_capture_flush_retry_and_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let (service, entered, release, calls) = maintenance_probe(None);
        let mut writer = start_writer_with_maintenance(
            path.clone(),
            WriterConfig {
                stream_id: "fault-test".into(),
                capacity: 1,
                flush_millis: 60_000,
                ..Default::default()
            },
            service,
        )
        .unwrap();
        writer.maintenance(maintenance_request()).unwrap();
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        submit(&writer, event(1));
        assert!(matches!(
            writer.try_submit(event(2)),
            Err(SubmitError::Full(_))
        ));
        let (flush, flushed) = bounded(1);
        let (retry, retried) = bounded(1);
        let (shutdown, stopped) = bounded(1);
        writer.commands.send(Command::Flush(1, flush)).unwrap();
        writer.commands.send(Command::Retry(1, retry)).unwrap();
        writer
            .commands
            .send(Command::Shutdown(1, shutdown))
            .unwrap();
        release.send(()).unwrap();
        for reply in [flushed, retried, stopped] {
            assert_eq!(
                reply
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap()
                    .unwrap()
                    .sequence
                    .0
                     .0,
                1
            );
        }
        writer.worker.take().unwrap().join().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let db = Connection::open(path).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM event_streams WHERE stream_id='synthetic-maintenance'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(crate::receipt_tests::head(&db, "fault-test"), Some(1));
        assert_eq!(crate::receipt_tests::count(&db), 1);
        assert_eq!(
            db.query_row("SELECT processed_records FROM maintenance_jobs", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(writer.status().queued, 0);
    }

    #[test]
    fn maintenance_errors_leave_capture_ready_and_capture_failure_retains_ownership() {
        for code in [
            rusqlite::ffi::SQLITE_BUSY,
            rusqlite::ffi::SQLITE_FULL,
            rusqlite::ffi::SQLITE_IOERR,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("db");
            let (service, entered, release, calls) = maintenance_probe(Some(code));
            let writer = start_writer_with_maintenance(
                path.clone(),
                WriterConfig {
                    stream_id: "fault-test".into(),
                    flush_millis: 60_000,
                    ..Default::default()
                },
                service,
            )
            .unwrap();
            let id = writer.maintenance(maintenance_request()).unwrap().status.id;
            entered.recv_timeout(Duration::from_secs(10)).unwrap();
            writer.set_maintenance_blocked(true);
            release.send(()).unwrap();
            // Wait for a durable-head-independent acknowledgment from the worker.
            assert!(
                !writer
                    .maintenance(Control::Status(id))
                    .unwrap()
                    .status
                    .paused
            );
            assert_eq!(writer.status().state, StoreState::Ready);
            submit(&writer, event(1));
            writer.flush().unwrap();
            assert_eq!(writer.status().durable, Some(Sequence(Timestamp(1))));
            let (send, recv) = bounded(1);
            writer
                .commands
                .send(Command::Inject(Point::Commit, code, send))
                .unwrap();
            recv.recv().unwrap();
            submit(&writer, event(2));
            assert!(writer.flush().is_err());
            assert_eq!(writer.status().state, StoreState::Failed);
            assert_eq!(writer.status().queued, 1);
            writer.maintenance(Control::Resume(id)).unwrap();
            assert_eq!(
                writer
                    .maintenance(Control::Status(id))
                    .unwrap()
                    .status
                    .processed_records,
                0
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            // Keep the job paused while retry drains precisely the retained queue.
            writer.maintenance(Control::Pause(id)).unwrap();
            writer.retry().unwrap();
            assert_eq!(writer.status().durable, Some(Sequence(Timestamp(2))));
            assert_eq!(writer.status().queued, 0);
            writer.shutdown().unwrap();
            let db = Connection::open(path).unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM event_streams WHERE stream_id='synthetic-maintenance'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            assert_eq!(crate::receipt_tests::head(&db, "fault-test"), Some(2));
            assert_eq!(crate::receipt_tests::count(&db), 2);
        }
    }

    #[test]
    fn maintenance_busy_retries_on_worker_cadence_without_resume() {
        let dir = tempfile::tempdir().unwrap();
        let (service, entered, release, calls) =
            maintenance_probe(Some(rusqlite::ffi::SQLITE_BUSY));
        let writer = start_writer_with_maintenance(
            dir.path().join("db"),
            WriterConfig {
                flush_millis: 60_000,
                ..Default::default()
            },
            service,
        )
        .unwrap();
        let id = writer.maintenance(maintenance_request()).unwrap().status.id;
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        release.send(()).unwrap();
        // No command wakes maintenance between these attempts. The normal
        // retry deadline must wake even with a sixty-second capture interval.
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        writer.set_maintenance_blocked(true);
        release.send(()).unwrap();
        assert!(
            !writer
                .maintenance(Control::Status(id))
                .unwrap()
                .status
                .paused
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(writer.status().state, StoreState::Ready);
        writer.shutdown().unwrap();
    }

    #[test]
    fn maintenance_pressure_checks_include_pending_barriers_and_unsaved_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let mut conn = open_writer(&path).unwrap();
        let (mut service, _, _, calls) = maintenance_probe(None);
        service.control(&mut conn, maintenance_request()).unwrap();
        let (commands, command_rx) = bounded(8);
        let (committed, _) = bounded(1);
        let shared = Mutex::new(Shared {
            path: std::fs::canonicalize(&path).unwrap(),
            maintenance_blocked: false,
            maintenance_barriers: 0,
            queue: VecDeque::new(),
            bytes: 0,
            last_ticket: 0,
            urgent_through: 0,
            committed,
            state: StoreState::Ready,
            durable: None,
            failure: None,
            accepting: true,
        });
        for reason in 0..5 {
            {
                let mut state = lock(&shared);
                state.maintenance_blocked = reason == 0;
                state.maintenance_barriers = usize::from(reason == 1);
                state.failure = (reason == 2).then_some(StoreError::Full);
                state.accepting = reason != 3;
            }
            if reason == 4 {
                let (reply, _) = bounded(1);
                commands.send(Command::Retry(0, reply)).unwrap();
            }
            assert_eq!(
                service
                    .step(&mut conn, false, &|| maintenance_interrupted(
                        &shared,
                        &command_rx
                    ))
                    .unwrap(),
                Step::Deferred(PauseReason::Capture)
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(conn.is_autocommit());
        assert_eq!(
            conn.query_row("PRAGMA busy_timeout", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            250
        );
        // Abandoned capture stays owned and also excludes stopped maintenance.
        lock(&shared).queue.push_back(Pending {
            event: Arc::new(event(1)),
            context: Arc::default(),
            bytes: 0,
            ticket: 1,
        });
        let shared = Arc::new(shared);
        retain_abandoned(shared.clone());
        assert!(matches!(
            crate::maintenance::StoppedMaintenance::open(&path, MaintenanceService::default()),
            Err(StoreError::Busy)
        ));
        // Remove only this test's exceptional owner; other tests may retain data.
        abandoned()
            .lock()
            .unwrap()
            .retain(|item| !Arc::ptr_eq(item, &shared));
        assert_eq!(lock(&shared).queue.len(), 1);
    }

    #[test]
    fn worker_receipt_matches_direct_projection_and_duplicate_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prepared.sqlite3");
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: "fault-test".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let envelope = event(1);
        let expected = crate::project::validate_event(&envelope).unwrap().1;
        submit(&writer, envelope.clone());
        writer.flush().unwrap();
        let mut conn = open_writer(&path).unwrap();
        let stored = crate::receipt_tests::lookup(&conn, "fault-test", 1).unwrap();
        assert_eq!(stored, expected);
        crate::apply_batch(&mut conn, std::slice::from_ref(&envelope)).unwrap();
        let mut changed = envelope.clone();
        changed.at = Timestamp(2);
        assert!(crate::apply_batch(&mut conn, &[changed]).is_err());
        let mut direct = open_writer(&dir.path().join("direct.sqlite3")).unwrap();
        crate::apply_batch(&mut direct, &[envelope]).unwrap();
        let direct_hash = crate::receipt_tests::lookup(&direct, "fault-test", 1).unwrap();
        assert_eq!(stored, direct_hash);
        writer.shutdown().unwrap();
    }

    #[test]
    fn commit_schedule_uses_urgent_ticket_without_advancing_batch_clock() {
        let config = WriterConfig::default();
        let (committed, _) = bounded(1);
        let mut state = Shared {
            path: PathBuf::new(),
            maintenance_blocked: false,
            maintenance_barriers: 0,
            queue: VecDeque::new(),
            bytes: 0,
            last_ticket: 3,
            urgent_through: 0,
            committed,
            state: StoreState::Ready,
            durable: None,
            failure: None,
            accepting: true,
        };
        for ticket in 1..=3 {
            state.queue.push_back(Pending {
                event: Arc::new(event(ticket)),
                context: Arc::new(crate::AlertContext::default()),
                bytes: 0,
                ticket,
            });
        }
        let before_tick = Duration::from_millis(249);
        assert!(!commit_ready(&state, &config, Duration::ZERO));
        assert!(!commit_ready(&state, &config, before_tick));
        assert!(commit_ready(&state, &config, Duration::from_millis(250)));
        state.urgent_through = 2;
        assert!(commit_ready(&state, &config, Duration::ZERO));
        state.queue.pop_front();
        assert!(commit_ready(&state, &config, Duration::ZERO));
        state.queue.pop_front();
        assert!(!commit_ready(&state, &config, Duration::ZERO));
        assert!(!commit_ready(&state, &config, before_tick));
        state.urgent_through = 3;
        state.failure = Some(StoreError::Full);
        assert!(!commit_ready(&state, &config, Duration::ZERO));
    }

    #[test]
    fn only_visible_alert_candidates_request_prompt_commit() {
        let mut context = crate::AlertContext {
            chat: None,
            live: false,
            preferences: AlertPreferences::default(),
            key_pop_dungeon: None,
            bag_window: None,
        };
        let death = EngineEvent::PlayerDied {
            at: Timestamp(1),
            character_id: None,
            run_id: None,
        };
        let mut bag = demo::demo_snapshot().drops.remove(0);
        bag.bag_rank = Some(255);
        let white = EngineEvent::BagDropped(bag.clone());
        let updated = EngineEvent::DropUpdated(bag.clone());
        let fight = EngineEvent::FightEnded(crate::alerts_workload::eligible_fight());
        let finalized = EngineEvent::FightUpdated(crate::alerts_workload::eligible_fight());
        assert!(!context.needs_prompt_commit(&death));
        for candidate in [&white, &updated, &fight, &finalized] {
            assert!(context.needs_prompt_commit(candidate));
        }
        context.preferences = AlertPreferences {
            death: AlertLoudness::QuietLogOnly,
            white_bag: AlertLoudness::Off,
            personal_best: AlertLoudness::Off,
            ..Default::default()
        };
        for candidate in [&death, &white, &updated, &fight, &finalized] {
            assert!(!context.needs_prompt_commit(candidate));
        }
        context.preferences = AlertPreferences::default();
        bag.bag_rank = Some(1);
        assert!(!context.needs_prompt_commit(&EngineEvent::BagDropped(bag)));
        assert!(!context.needs_prompt_commit(&event(1).event));
        let mut active = crate::alerts_workload::eligible_fight();
        active.ended = None;
        assert!(!context.needs_prompt_commit(&EngineEvent::FightUpdated(active)));
        let run = EngineEvent::RunEnded(demo::demo_run_story().run);
        assert!(!context.needs_prompt_commit(&run));
        context.live = true;
        assert!(context.needs_prompt_commit(&run));
    }

    #[test]
    fn commit_wake_follows_durability_and_coalesces_without_flushing_normal_work() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prompt.sqlite3");
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..Default::default()
            },
        )
        .unwrap();
        let committed = writer.committed();
        submit(&writer, event(1));
        assert!(writer.claim_alerts(32).unwrap().is_empty());
        assert!(writer.status().durable.is_none());
        assert!(committed.try_recv().is_err());
        let mut source = event(2);
        let mut bag = demo::demo_snapshot().drops.remove(0);
        bag.run_id = None;
        bag.session_id = "session".into();
        bag.bag_rank = Some(255);
        source.event = EngineEvent::BagDropped(bag);
        submit_with_context(
            &writer,
            source,
            crate::AlertContext {
                chat: None,
                live: false,
                preferences: AlertPreferences::default(),
                key_pop_dungeon: None,
                bag_window: None,
            },
        );
        committed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(writer.status().durable, Some(Sequence(Timestamp(2))));
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM alert_occurrences WHERE state='pending'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(writer.claim_alerts(32).unwrap().len(), 1);
        for sequence in 3..=4 {
            submit(&writer, event(sequence));
            writer.flush().unwrap();
        }
        // A second commit cannot block the writer behind an unconsumed permit.
        assert_eq!(committed.len(), 1);
        committed.recv().unwrap();
        assert_eq!(writer.status().durable, Some(Sequence(Timestamp(4))));
        writer.shutdown().unwrap();
    }

    #[test]
    fn failed_urgent_commit_never_signals_or_claims_until_retry() {
        let dir = tempfile::tempdir().unwrap();
        let writer = start_writer(
            dir.path().join("failed.sqlite3"),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..Default::default()
            },
        )
        .unwrap();
        let committed = writer.committed();
        submit(&writer, event(1));
        writer.flush().unwrap();
        committed.recv().unwrap();
        let (reply, response) = bounded(1);
        writer
            .commands
            .send(Command::Inject(
                Point::Commit,
                rusqlite::ffi::SQLITE_FULL,
                reply,
            ))
            .unwrap();
        response.recv().unwrap();
        let mut source = event(2);
        let mut bag = demo::demo_snapshot().drops.remove(0);
        bag.run_id = None;
        bag.session_id = "session".into();
        bag.bag_rank = Some(255);
        source.event = EngineEvent::BagDropped(bag);
        submit_with_context(
            &writer,
            source,
            crate::AlertContext {
                chat: None,
                live: false,
                preferences: AlertPreferences::default(),
                key_pop_dungeon: None,
                bag_window: None,
            },
        );
        assert!(writer.flush().is_err());
        assert_eq!(writer.status().durable, Some(Sequence(Timestamp(1))));
        assert!(committed.try_recv().is_err());
        assert!(writer.claim_alerts(32).unwrap().is_empty());
        writer.retry().unwrap();
        committed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(writer.claim_alerts(32).unwrap().len(), 1);
        assert!(writer.claim_alerts(32).unwrap().is_empty());
        writer.shutdown().unwrap();
    }

    #[test]
    fn clear_chat_flushes_accepted_events_first() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("chat.sqlite3");
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..Default::default()
            },
        )
        .unwrap();
        submit(&writer, event(1));
        let mut source = event(2);
        source.event = EngineEvent::Chat(ChatMessageView {
            id: "fault-test:chat:2".into(),
            session_id: "session".into(),
            run_id: None,
            map: None,
            at: Timestamp(1),
            channel: ChatChannel::Public,
            direction: None,
            sender_kind: ChatSenderKind::Player,
            sender: "SynthAlpha".into(),
            counterpart: None,
            is_local: false,
            stars: None,
            supporter: false,
            text: "synthetic message".into(),
            truncated: false,
        });
        submit(&writer, source);
        assert_eq!(
            writer.maintain_chat(crate::ChatMaintenance::Clear).unwrap(),
            1
        );
        assert_eq!(writer.status().durable, Some(Sequence(Timestamp(2))));
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM chat_messages", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        writer.shutdown().unwrap();
    }

    #[test]
    fn response_arriving_between_timeout_and_worker_exit_is_not_lost() {
        let (reply, response) = bounded(1);
        let expected = FlushReceipt {
            stream_id: "race-test".into(),
            sequence: Sequence(Timestamp(42)),
        };
        // Deterministically deliver the acknowledgment after recv_timeout but
        // before the finished check returns, exactly the shutdown race window.
        let actual = wait_response_until(response, || {
            reply.send(Ok(expected.clone())).unwrap();
            true
        })
        .unwrap();
        assert_eq!(actual, expected);
    }

    fn event(sequence: u64) -> EventEnvelope {
        EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "fault-test".into(),
            sequence: Sequence(Timestamp(sequence)),
            at: Timestamp(1),
            event: EngineEvent::SessionUpdated(PlaySessionView {
                issues: vec![],
                id: "session".into(),
                started: Timestamp(1),
                last_traffic: Timestamp(sequence),
                ended: None,
            }),
        }
    }

    fn submit(writer: &StoreWriter, event: EventEnvelope) {
        submit_with_context(writer, event, crate::AlertContext::default());
    }

    fn submit_with_context(
        writer: &StoreWriter,
        mut event: EventEnvelope,
        context: crate::AlertContext,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match writer.try_submit_with_alert_context(event, context.clone()) {
                Ok(()) => return,
                Err(SubmitError::Full(returned)) => {
                    assert!(
                        Instant::now() < deadline,
                        "submit timed out after 10s on Full: {returned:?}"
                    );
                    event = returned;
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => panic!("submit failed: {error:?}"),
            }
        }
    }

    #[test]
    fn boundary_fault_matrix_retains_exact_batch_and_retry_commits_once() {
        for point in [Point::Begin, Point::Commit] {
            for code in [
                rusqlite::ffi::SQLITE_FULL,
                rusqlite::ffi::SQLITE_READONLY,
                rusqlite::ffi::SQLITE_BUSY,
                rusqlite::ffi::SQLITE_IOERR,
            ] {
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("db");
                let writer = start_writer(
                    path.clone(),
                    WriterConfig {
                        stream_id: "fault-test".into(),
                        flush_millis: 60_000,
                        ..WriterConfig::default()
                    },
                )
                .unwrap();
                submit(&writer, event(1));
                writer.flush().unwrap();
                let (reply, response) = bounded(1);
                writer
                    .commands
                    .send(Command::Inject(point, code, reply))
                    .unwrap();
                response.recv().unwrap();
                submit(&writer, event(2));
                submit(&writer, event(3));
                let expected = StoreError::from(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(code),
                    None,
                ));
                for _ in 0..3 {
                    let error = writer.flush().unwrap_err();
                    assert_eq!(
                        std::mem::discriminant(&error),
                        std::mem::discriminant(&expected)
                    );
                    assert!(!writer.worker.as_ref().unwrap().is_finished());
                }
                let status = writer.status();
                assert_eq!(status.state, StoreState::Failed);
                assert_eq!(status.queued, 2);
                assert_eq!(status.durable, Some(Sequence(Timestamp(1))));
                assert!(status.message.contains("2..=3"));
                let db = Connection::open(&path).unwrap();
                assert_eq!(crate::receipt_tests::count(&db), 1);
                // Contention can return Full before the worker failure is checked.
                let deadline = Instant::now() + Duration::from_secs(10);
                let mut pending = event(4);
                let result = loop {
                    match writer.try_submit(pending) {
                        Err(SubmitError::Full(returned)) => {
                            assert!(
                                Instant::now() < deadline,
                                "waiting for Failed timed out after 10s on Full: {returned:?}"
                            );
                            pending = returned;
                            thread::sleep(Duration::from_millis(1));
                        }
                        result => break result,
                    }
                };
                assert!(matches!(result, Err(SubmitError::Failed(_))));
                writer.retry().unwrap();
                assert_eq!(writer.flush().unwrap().sequence.0 .0, 3);
                assert_eq!(crate::receipt_tests::count(&db), 3);
                assert_eq!(writer.status().queued, 0);
                writer.shutdown().unwrap();
            }
        }
    }

    #[test]
    fn panic_exposes_unacknowledged_range_and_shutdown_returns_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let writer = start_writer(
            dir.path().join("db"),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..WriterConfig::default()
            },
        )
        .unwrap();
        submit(&writer, event(1));
        writer.commands.send(Command::Panic).unwrap();
        match writer.flush().unwrap_err() {
            StoreError::WorkerFailed { pending } => assert_eq!(pending, vec![event(1)]),
            error => panic!("wrong panic result: {error}"),
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while writer.status().state != StoreState::Failed && Instant::now() < deadline {
            thread::yield_now();
        }
        assert!(writer.status().message.contains("1..=1"));
        match writer.shutdown().unwrap_err() {
            StoreError::WorkerFailed { pending } => assert_eq!(pending, vec![event(1)]),
            e => panic!("{e}"),
        }
    }

    #[test]
    fn byte_budget_is_enforced_independently_of_event_count() {
        let dir = tempfile::tempdir().unwrap();
        let writer = start_writer(
            dir.path().join("db"),
            WriterConfig {
                stream_id: "fault-test".into(),
                flush_millis: 60_000,
                ..WriterConfig::default()
            },
        )
        .unwrap();
        // This summary text is synthetic; no packet/chat fixture is involved.
        for sequence in 1..=4 {
            let mut e = event(sequence);
            if let EngineEvent::SessionUpdated(s) = &mut e.event {
                s.id = "x".repeat(7 * 1024 * 1024);
            }
            submit(&writer, e);
        }
        let mut e = event(5);
        if let EngineEvent::SessionUpdated(s) = &mut e.event {
            s.id = "x".repeat(7 * 1024 * 1024);
        }
        assert!(matches!(writer.try_submit(e), Err(SubmitError::Full(_))));
        assert_eq!(writer.status().state, StoreState::Backpressured);
        assert_eq!(writer.status().queued, 4);
        // Prevent writing the large fixture; shutdown returns it intact.
        let (reply, response) = bounded(1);
        writer
            .commands
            .send(Command::Inject(
                Point::Begin,
                rusqlite::ffi::SQLITE_FULL,
                reply,
            ))
            .unwrap();
        response.recv().unwrap();
        match writer.shutdown().unwrap_err() {
            StoreError::WorkerFailed { pending } => assert_eq!(pending.len(), 4),
            e => panic!("{e}"),
        }
    }
}
