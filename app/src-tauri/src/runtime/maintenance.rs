//! Native ownership for explicitly requested storage work. Capture transfers the
//! stopped lease before opening its writer; backup copies run on separate workers.
use super::{storage::namespace_lease, CaptureRuntime, Namespace};
use maxed_store::{
    backup::{
        BackupEnvironment, BackupError, BackupId, BackupUsage, HistoryUpgrade, ManagedBackup,
    },
    maintenance::{
        Control, ControlResult, JobStatus, PauseReason, Phase, ReceiptBackupTask,
        StoppedMaintenance,
    },
    StoreError,
};
use maxed_types::*;
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const UNAVAILABLE: &str = "Storage maintenance is unavailable; retry after storage is ready.";
const STOPPED: &str = "Stop capture before upgrading history.";

pub(crate) fn now() -> Timestamp {
    super::chat::wall_now()
}

pub(super) fn failure(error: BackupError) -> StorageFailure {
    match error {
        BackupError::SpaceUnknown => StorageFailure::SpaceUnknown,
        BackupError::InsufficientSpace => StorageFailure::InsufficientSpace,
        BackupError::Cancelled => StorageFailure::Cancelled,
        BackupError::Busy => StorageFailure::Busy,
        BackupError::Io => StorageFailure::Io,
        BackupError::OwnerPoisoned => StorageFailure::OwnerPoisoned,
        BackupError::Invalid | BackupError::VerificationFailed => {
            StorageFailure::VerificationFailed
        }
        BackupError::Superseded => StorageFailure::Superseded,
        BackupError::WalPressure => StorageFailure::WalPressure,
        BackupError::InUse => StorageFailure::InUse,
    }
}

#[derive(Default)]
struct UpgradeState {
    running: bool,
    status: Option<HistoryUpgradeStatus>,
}
#[derive(Default)]
pub(super) struct UpgradeLifecycle {
    state: Mutex<UpgradeState>,
    finished: Condvar,
    cancel: AtomicBool,
}
impl UpgradeLifecycle {
    pub(super) fn running(&self) -> bool {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).running
    }
    pub(super) fn cancel_and_wait(&self) {
        self.cancel.store(true, Ordering::Release);
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        drop(
            self.finished
                .wait_while(state, |s| s.running)
                .unwrap_or_else(|p| p.into_inner()),
        );
    }
}
struct UpgradeRun<'a>(&'a UpgradeLifecycle);
impl Drop for UpgradeRun<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.running {
            state.running = false;
            if let Some(status) = state.status.as_mut() {
                status.phase = HistoryUpgradePhase::Blocked;
                status.failure = Some(StorageFailure::Unavailable);
            }
        }
        self.0.finished.notify_all();
    }
}

struct UpgradeEnvironment<'a> {
    inner: &'a dyn BackupEnvironment,
    external: &'a AtomicBool,
    cancel: &'a AtomicBool,
    closing: &'a AtomicBool,
}
impl BackupEnvironment for UpgradeEnvironment<'_> {
    fn available_space(&self, directory: &Path) -> Option<u64> {
        self.inner.available_space(directory)
    }
    fn timestamp(&self) -> u64 {
        self.inner.timestamp()
    }
    fn boundary(&self, at: maxed_store::backup::Boundary) -> Result<(), BackupError> {
        if self.external.load(Ordering::Acquire)
            || self.cancel.load(Ordering::Acquire)
            || self.closing.load(Ordering::Acquire)
        {
            return Err(BackupError::Cancelled);
        }
        self.inner.boundary(at)?;
        if self.external.load(Ordering::Acquire)
            || self.cancel.load(Ordering::Acquire)
            || self.closing.load(Ordering::Acquire)
        {
            return Err(BackupError::Cancelled);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryUpgradeError {
    #[error("Stop capture before upgrading history")]
    StoppedRequired,
    #[error("History upgrade is unavailable")]
    Unavailable,
    #[error(transparent)]
    Backup(#[from] BackupError),
}

pub struct SystemBackupEnvironment;
impl BackupEnvironment for SystemBackupEnvironment {
    fn available_space(&self, directory: &Path) -> Option<u64> {
        platform_available_space(directory)
    }
    fn timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros()
            .min(i64::MAX as u128) as u64
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // The only native FFI call; all pointers are owned below.
fn platform_available_space(directory: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{core::PCWSTR, Win32::Storage::FileSystem::GetDiskFreeSpaceExW};
    // Preserve verbatim/long paths and query space available to this caller,
    // including per-user quotas on the directory's actual volume.
    let mut path: Vec<u16> = directory.as_os_str().encode_wide().collect();
    if path.is_empty() || path.contains(&0) {
        return None;
    }
    // UNC directories require a trailing separator.
    if !matches!(path.last(), Some(47 | 92)) {
        path.push(92);
    }
    path.push(0);
    let mut available = 0;
    // SAFETY: path is NUL-terminated and available is valid for this call.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(path.as_ptr()), Some(&mut available), None, None) }.ok()?;
    Some(available)
}
#[cfg(not(windows))]
fn platform_available_space(_: &Path) -> Option<u64> {
    None
}

impl CaptureRuntime {
    pub fn history_upgrade_status(&self) -> Result<HistoryUpgradeStatus, String> {
        self.history_upgrade_status_with(&SystemBackupEnvironment)
    }

    pub(super) fn history_upgrade_status_with(
        &self,
        env: &dyn BackupEnvironment,
    ) -> Result<HistoryUpgradeStatus, String> {
        let state = self.shared.upgrade.state.lock().map_err(|_| UNAVAILABLE)?;
        if state.running {
            return state.status.clone().ok_or_else(|| UNAVAILABLE.into());
        }
        let previous = state.status.as_ref().and_then(|s| s.failure);
        drop(state);
        let path = self.shared.home.live_database().ok_or(UNAVAILABLE)?;
        let mut status = probe(&path, env);
        status.last_failure = previous;
        Ok(status)
    }

    /// Runs on the command's blocking worker. Concurrent clicks join the active
    /// lifecycle and return its status, without recording a second request.
    pub fn backup_and_upgrade_history(&self) -> Result<HistoryUpgradeStatus, String> {
        let result =
            self.backup_and_upgrade_with(&SystemBackupEnvironment, &AtomicBool::new(false));
        if let Err(HistoryUpgradeError::StoppedRequired) = result {
            return Err(STOPPED.into());
        }
        self.history_upgrade_status()
    }

    pub(super) fn backup_and_upgrade_with(
        &self,
        environment: &dyn BackupEnvironment,
        cancel: &AtomicBool,
    ) -> Result<(), HistoryUpgradeError> {
        let session = self
            .session
            .lock()
            .map_err(|_| HistoryUpgradeError::Unavailable)?;
        if self.shared.upgrade.running() {
            return Ok(());
        }
        if self.closing.load(Ordering::Acquire)
            || self
                .shared
                .views
                .maintenance
                .update_paused
                .load(Ordering::Acquire)
            || !self.live_storage_mode()
        {
            return Err(HistoryUpgradeError::StoppedRequired);
        }
        if session.is_some()
            || self
                .shared
                .recovery
                .lock()
                .map_err(|_| HistoryUpgradeError::Unavailable)?
                .is_some()
            || self
                .shared
                .completed
                .lock()
                .map_err(|_| HistoryUpgradeError::Unavailable)?
                .is_some()
        {
            return Err(HistoryUpgradeError::StoppedRequired);
        }
        let path = self
            .shared
            .home
            .live_database()
            .ok_or(HistoryUpgradeError::Unavailable)?;
        self.shared.views.maintenance.stop_background();
        let upgrade = &self.shared.upgrade;
        {
            let mut state = upgrade
                .state
                .lock()
                .map_err(|_| HistoryUpgradeError::Unavailable)?;
            state.running = true;
            state.status = Some(HistoryUpgradeStatus {
                phase: HistoryUpgradePhase::BackingUp,
                schema_version: None,
                required_bytes: None,
                free_bytes: None,
                failure: None,
                last_failure: None,
            });
            upgrade
                .cancel
                .store(cancel.load(Ordering::Acquire), Ordering::Release);
        }
        let _run = UpgradeRun(upgrade);
        drop(session);
        let environment = UpgradeEnvironment {
            inner: environment,
            external: cancel,
            cancel: &upgrade.cancel,
            closing: &self.closing,
        };
        let mut migrated = false;
        let result = (|| {
            self.shared
                .home
                .invalidate_and_wait()
                .map_err(|_| HistoryUpgradeError::Unavailable)?;
            self.shared.home.shutdown_reads();
            let root = path.parent().ok_or(HistoryUpgradeError::Unavailable)?;
            let _namespace = namespace_lease(root, Namespace::Live)
                .map_err(|_| HistoryUpgradeError::StoppedRequired)?;
            let preflight = HistoryUpgrade::preflight(&path, &environment)?;
            if !preflight.schema.is_some_and(|s| (1..11).contains(&s)) {
                return Ok(());
            }
            {
                let mut state = upgrade
                    .state
                    .lock()
                    .map_err(|_| HistoryUpgradeError::Unavailable)?;
                state.status = Some(HistoryUpgradeStatus {
                    phase: HistoryUpgradePhase::BackingUp,
                    schema_version: preflight.schema.and_then(|v| u32::try_from(v).ok()),
                    required_bytes: preflight.space.required_bytes.map(StorageCount),
                    free_bytes: preflight.space.free_bytes.map(StorageCount),
                    failure: None,
                    last_failure: None,
                });
            }
            preflight.space.check()?;
            let mut owner = HistoryUpgrade::open(&path).map_err(|error| match error {
                StoreError::Busy => HistoryUpgradeError::StoppedRequired,
                _ => HistoryUpgradeError::Unavailable,
            })?;
            owner.confirm_and_upgrade_with(&environment, &upgrade.cancel, &mut || {
                if cancel.load(Ordering::Acquire) || self.closing.load(Ordering::Acquire) {
                    upgrade.cancel.store(true, Ordering::Release);
                }
                if let Some(status) = upgrade
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .status
                    .as_mut()
                {
                    status.phase = HistoryUpgradePhase::Upgrading;
                }
            })?;
            migrated = true;
            Ok(())
        })();
        self.shared.home.resume_reads();
        let result = if result.is_ok() && migrated {
            self.shared
                .home
                .upgraded()
                .map_err(|_| HistoryUpgradeError::Unavailable)
        } else {
            let _ = self.shared.home.restore_live();
            result
        };
        let mut status = probe(&path, &environment);
        if let Err(error) = &result {
            status.phase = HistoryUpgradePhase::Blocked;
            status.failure = Some(match error {
                HistoryUpgradeError::Backup(error) => failure(*error),
                HistoryUpgradeError::StoppedRequired => StorageFailure::StoppedRequired,
                HistoryUpgradeError::Unavailable => StorageFailure::Unavailable,
            });
        }
        let mut state = upgrade.state.lock().unwrap_or_else(|p| p.into_inner());
        state.running = false;
        state.status = Some(status);
        upgrade.finished.notify_all();
        result
    }
}

fn probe(path: &Path, env: &dyn BackupEnvironment) -> HistoryUpgradeStatus {
    match HistoryUpgrade::preflight(path, env) {
        Ok(preflight) => {
            let required = preflight.schema.is_some_and(|s| (1..11).contains(&s));
            let failure = if required {
                preflight.space.check().err().map(failure)
            } else {
                None
            };
            HistoryUpgradeStatus {
                phase: if failure.is_some() {
                    HistoryUpgradePhase::Blocked
                } else if required {
                    HistoryUpgradePhase::Required
                } else {
                    HistoryUpgradePhase::Ready
                },
                schema_version: preflight.schema.and_then(|v| u32::try_from(v).ok()),
                required_bytes: preflight.space.required_bytes.map(StorageCount),
                free_bytes: preflight.space.free_bytes.map(StorageCount),
                failure,
                last_failure: None,
            }
        }
        Err(error) => HistoryUpgradeStatus {
            phase: HistoryUpgradePhase::Blocked,
            schema_version: None,
            required_bytes: None,
            free_bytes: None,
            failure: Some(failure(error)),
            last_failure: None,
        },
    }
}

type NoticeSink = Arc<dyn Fn(StorageMaintenanceChanged) + Send + Sync>;
type Inventory = (Vec<ManagedBackup>, BackupUsage);
pub(super) type StorageObservation = (DatabaseStorage, Vec<JobStatus>, Option<StorageEstimate>);
#[derive(Clone)]
enum Operation {
    Compact,
    Resume,
    Cancel,
    Restore(Vec<JobStatus>),
    Delete(BackupId),
}
enum Reply {
    Done,
    Queued,
}
impl From<Reply> for StorageCommandOutcome {
    fn from(reply: Reply) -> Self {
        match reply {
            Reply::Done => Self::Accepted,
            Reply::Queued => Self::Queued,
        }
    }
}
struct Request {
    operation: Operation,
    reply: Option<crossbeam_channel::Sender<Result<Reply, String>>>,
}
#[derive(Default)]
struct ServiceState {
    background_running: bool,
    requests: VecDeque<Request>,
    jobs: Option<Vec<JobStatus>>,
    failure: Option<StorageFailure>,
    estimate: StorageEstimate,
    pending_notice: bool,
    last_notice: Option<Instant>,
    sink: Option<NoticeSink>,
    copy_retry_at: Option<Instant>,
    copy_retry_delay: Duration,
}

#[derive(Default)]
pub(super) struct StorageService {
    #[cfg(test)]
    clock: Mutex<Option<Arc<dyn Fn() -> Instant + Send + Sync>>>,
    #[cfg(test)]
    pub(super) on_jobs_retry: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    #[cfg(test)]
    environment: Mutex<Option<Arc<dyn BackupEnvironment + Send + Sync>>>,
    state: Mutex<ServiceState>,
    worker: Mutex<Option<JoinHandle<()>>>,
    copy_cancel: Mutex<Option<Arc<AtomicBool>>>,
    halted: AtomicBool,
    owns_namespace: AtomicBool,
    online: AtomicBool,
    retry_changed: tokio::sync::Notify,
    pub(super) update_paused: AtomicBool,
    // Explicit estimates take this gate on the history thread. Admission of new
    // destructive work uses the same gate; active jobs make estimates unavailable.
    pub(super) estimate_gate: Mutex<()>,
}
// The flag describes actual ownership, not a worker waiting to acquire it.
// Close the handle before clearing: a waiter observing false can acquire it.
struct BackgroundLease<'a> {
    lease: Option<std::fs::File>,
    owned: &'a AtomicBool,
}
impl Drop for BackgroundLease<'_> {
    fn drop(&mut self) {
        drop(self.lease.take());
        self.owned.store(false, Ordering::Release);
    }
}
struct BackgroundRun<'a>(&'a StorageService);
impl Drop for BackgroundRun<'_> {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .background_running = false;
    }
}
impl StorageService {
    pub(super) fn owns_stopped_namespace(&self) -> bool {
        self.owns_namespace.load(Ordering::Acquire)
    }

    fn now(&self) -> Instant {
        #[cfg(test)]
        if let Some(clock) = self.clock.lock().unwrap().as_ref() {
            return clock();
        }
        Instant::now()
    }
    fn copy_ready(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .copy_retry_at
            .is_none_or(|at| self.now() >= at)
    }
    fn reset_copy_retry(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.copy_retry_at = None;
        state.copy_retry_delay = Duration::ZERO;
        self.retry_changed.notify_waiters();
    }
    fn begin_copy(&self) {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .copy_retry_at = None;
        self.retry_changed.notify_waiters();
    }
    fn defer_copy(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.copy_retry_delay =
            (state.copy_retry_delay * 2).clamp(Duration::from_secs(3), Duration::from_secs(60));
        state.copy_retry_at = Some(self.now() + state.copy_retry_delay);
        self.retry_changed.notify_waiters();
    }
    fn schedule_copy_retry(self: &Arc<Self>, path: PathBuf, at: Instant) {
        let weak = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            loop {
                let Some(service) = weak.upgrade() else {
                    return;
                };
                // Register before inspecting state: clock changes and lifecycle
                // cancellation must not be lost between inspection and sleep.
                let changed = service.retry_changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if service.halted.load(Ordering::Acquire)
                    || service.online.load(Ordering::Acquire)
                    || service
                        .state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .copy_retry_at
                        != Some(at)
                {
                    return;
                }
                let delay = at.saturating_duration_since(service.now());
                if delay.is_zero() {
                    let service = service.clone();
                    let _ = tauri::async_runtime::spawn_blocking(move || {
                        service.start_background_inner(path, Some(at))
                    })
                    .await;
                    return;
                }
                let _ = tokio::time::timeout(delay, &mut changed).await;
            }
        });
    }
    #[cfg(test)]
    pub(super) fn set_environment(&self, environment: Arc<dyn BackupEnvironment + Send + Sync>) {
        *self.environment.lock().unwrap() = Some(environment);
    }
    fn environment(&self) -> Arc<dyn BackupEnvironment + Send + Sync> {
        #[cfg(test)]
        if let Some(environment) = self.environment.lock().unwrap().clone() {
            return environment;
        }
        Arc::new(SystemBackupEnvironment)
    }
    pub(super) fn observe(&self, jobs: Vec<JobStatus>) {
        let jobs: Vec<_> = jobs
            .into_iter()
            .filter(|j| j.reason != Some(PauseReason::Superseded))
            .collect();
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(error) = jobs.iter().rev().find_map(|j| match j.reason {
            Some(PauseReason::BlockedStream) => Some(StorageFailure::BlockedStream),
            Some(PauseReason::BackupPoisoned) => Some(StorageFailure::OwnerPoisoned),
            Some(PauseReason::Full) => Some(StorageFailure::Full),
            Some(PauseReason::Io) => Some(StorageFailure::Io),
            Some(PauseReason::InvalidUnit) => Some(StorageFailure::InvalidUnit),
            _ => None,
        }) {
            state.pending_notice |= state.failure != Some(error);
            state.failure = Some(error);
        } else if !jobs.is_empty() && jobs.iter().all(|j| j.phase == Phase::Reclaimable) {
            state.pending_notice |= state.failure.is_some();
            state.failure = None;
        }
        if state.jobs.as_ref() != Some(&jobs) {
            state.jobs = Some(jobs);
            state.pending_notice = true;
        }
    }
    fn fail(&self, error: Option<StorageFailure>) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.failure != error {
            state.failure = error;
            state.pending_notice = true;
        }
    }
    fn clear_deferred_failure(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if matches!(
            state.failure,
            Some(StorageFailure::Busy | StorageFailure::InUse | StorageFailure::WalPressure)
        ) {
            state.failure = None;
            state.pending_notice = true;
        }
    }
    pub(super) fn pump(&self, now: Instant) {
        let delivery = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if !state.pending_notice
                || state.last_notice.is_some_and(|last| {
                    now.saturating_duration_since(last) < Duration::from_secs(1)
                })
            {
                return;
            }
            let Some(sink) = state.sink.clone() else {
                return;
            };
            state.last_notice = Some(now);
            state.pending_notice = false;
            (
                sink,
                StorageMaintenanceChanged {
                    measured_at: super::chat::wall_now(),
                    maintenance: state
                        .jobs
                        .as_deref()
                        .map(maxed_store::storage_usage::maintenance_usage),
                    failure: state.failure,
                },
            )
        };
        (delivery.0)(delivery.1);
    }
    fn cancel_copy(&self) {
        if let Some(cancel) = self
            .copy_cancel
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            cancel.store(true, Ordering::Release);
        }
    }
    pub(super) fn stop_background(&self) {
        self.handoff_background();
        self.reject_requests();
    }
    /// Release stopped ownership while preserving accepted requests, including
    /// commands not yet persisted and any following Cancel, for the next owner.
    pub(super) fn handoff_background(&self) {
        self.halted.store(true, Ordering::Release);
        self.retry_changed.notify_waiters();
        self.cancel_copy();
        if let Some(worker) = self.worker.lock().unwrap_or_else(|p| p.into_inner()).take() {
            if worker.join().is_err() {
                self.fail(Some(StorageFailure::Unavailable));
            }
        }
    }
    pub(super) fn attach(&self, live: bool) {
        self.online.store(live, Ordering::Release);
        self.halted.store(!live, Ordering::Release);
        if live {
            let jobs = self
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .jobs
                .clone();
            if let Some(jobs) = jobs.filter(|jobs| jobs.iter().any(runnable)) {
                self.enqueue(Operation::Restore(jobs), None);
            }
        }
    }
    fn enqueue(
        &self,
        operation: Operation,
        reply: Option<crossbeam_channel::Sender<Result<Reply, String>>>,
    ) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if matches!(operation, Operation::Compact)
            && state
                .requests
                .iter()
                .any(|r| matches!(r.operation, Operation::Compact))
        {
            if let Some(reply) = reply {
                let _ = reply.send(Ok(Reply::Queued));
            }
            return;
        }
        if state.requests.len() >= 8 {
            if let Some(reply) = reply {
                let _ = reply.send(Err("Storage maintenance is busy; retry.".into()));
            }
        } else {
            state.requests.push_back(Request { operation, reply });
        }
    }
    pub(super) fn has_background(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .background_running
    }
    fn start_background(self: &Arc<Self>, path: PathBuf) -> Result<(), String> {
        self.start_background_inner(path, None)
    }
    fn start_background_inner(
        self: &Arc<Self>,
        path: PathBuf,
        retry: Option<Instant>,
    ) -> Result<(), String> {
        let mut worker = self.worker.lock().map_err(|_| UNAVAILABLE)?;
        if let Some(at) = retry {
            let mut state = self.state.lock().map_err(|_| UNAVAILABLE)?;
            if self.halted.load(Ordering::Acquire)
                || self.online.load(Ordering::Acquire)
                || self.update_paused.load(Ordering::Acquire)
                || state.copy_retry_at != Some(at)
                || self.now() < at
            {
                return Ok(());
            }
            let Some(jobs) = state.jobs.clone().filter(|jobs| jobs.iter().any(runnable)) else {
                return Ok(());
            };
            if !state
                .requests
                .iter()
                .any(|r| matches!(r.operation, Operation::Restore(_)))
            {
                state.requests.push_back(Request {
                    operation: Operation::Restore(jobs),
                    reply: None,
                });
            }
        }
        if self.has_background() {
            return Ok(());
        }
        if let Some(done) = worker.take() {
            done.join().map_err(|_| UNAVAILABLE)?;
        }
        // Only explicit lifecycle admission may reopen a halted service. A
        // delayed wake racing capture/update/close must preserve their barrier.
        if retry.is_none() {
            self.halted.store(false, Ordering::Release);
        }
        let service = self.clone();
        self.state
            .lock()
            .map_err(|_| UNAVAILABLE)?
            .background_running = true;
        *worker = Some(
            std::thread::Builder::new()
                .name("storage-maintenance".into())
                .spawn(move || {
                    let _run = BackgroundRun(&service);
                    service.run_stopped(path);
                })
                .map_err(|_| {
                    self.state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .background_running = false;
                    UNAVAILABLE
                })?,
        );
        Ok(())
    }
    fn run_stopped(self: &Arc<Self>, path: PathBuf) {
        let Some(root) = path.parent() else { return };
        let lease = loop {
            if self.halted.load(Ordering::Acquire) {
                return;
            }
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if state.requests.is_empty() {
                state.background_running = false;
                return;
            }
            drop(state);
            match namespace_lease(root, Namespace::Live) {
                Ok(lease) => {
                    self.owns_namespace.store(true, Ordering::Release);
                    break BackgroundLease {
                        lease: Some(lease),
                        owned: &self.owns_namespace,
                    };
                }
                Err(_) => {
                    let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    state.requests.retain_mut(|request| {
                        let query = matches!(request.operation, Operation::Delete(_));
                        if let Some(reply) = request.reply.take() {
                            let _ = reply.send(if query {
                                Err("Storage maintenance is busy; retry.".into())
                            } else {
                                Ok(Reply::Queued)
                            });
                        }
                        !query
                    });
                    let work = !state.requests.is_empty();
                    drop(state);
                    if work {
                        self.fail(Some(StorageFailure::Busy));
                    }
                    self.pump(Instant::now());
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        };
        let compatible = HistoryUpgrade::preflight(&path, &SystemBackupEnvironment)
            .is_ok_and(|p| p.schema == Some(maxed_store::SCHEMA_VERSION));
        if !compatible {
            self.fail(Some(StorageFailure::Unavailable));
            self.reject_requests();
            return;
        }
        let mut owner = match StoppedMaintenance::receipt_conversion(&path) {
            Ok(owner) => owner,
            Err(_) => {
                self.fail(Some(StorageFailure::Unavailable));
                self.reject_requests();
                return;
            }
        };
        let mut runner = OwnerWork::default();
        if let Ok(jobs) = owner.statuses() {
            self.observe(jobs);
        }
        while !self.halted.load(Ordering::Acquire) {
            runner.service(&mut Owner::Stopped(&mut owner), self);
            self.pump(Instant::now());
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let pending = !state.requests.is_empty();
            let backing_off = state.copy_retry_at.is_some_and(|at| self.now() < at);
            if !pending && !runner.running(backing_off) {
                // Enqueue and idle retirement share this lock. Admission either
                // keeps this owner alive or joins it before starting the next.
                runner.stop();
                drop(owner);
                drop(lease);
                state.background_running = false;
                let retry_at = state.copy_retry_at.filter(|_| {
                    state
                        .jobs
                        .as_ref()
                        .is_some_and(|jobs| jobs.iter().any(runnable))
                });
                let notice_delay = (state.pending_notice && state.sink.is_some()).then(|| {
                    Duration::from_secs(1)
                        .saturating_sub(state.last_notice.map_or(Duration::ZERO, |at| at.elapsed()))
                });
                drop(state);
                if let Some(at) = retry_at {
                    self.schedule_copy_retry(path.clone(), at);
                }
                if let Some(delay) = notice_delay {
                    let service = Arc::downgrade(self);
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(delay).await;
                        if let Some(service) = service.upgrade() {
                            service.pump(Instant::now());
                        }
                    });
                }
                return;
            }
            drop(state);
            std::thread::sleep(Duration::from_millis(25));
        }
        runner.stop();
        drop(owner);
        drop(lease);
    }
    /// A suspended owner cannot service command replies. Keep admitted controls
    /// in order, including a Compact joined by an already-answered duplicate.
    /// Deletion is synchronous and cannot be promised across a suspension.
    pub(super) fn defer_requests(&self) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.requests.retain_mut(|request| {
            let delete = matches!(request.operation, Operation::Delete(_));
            if let Some(reply) = request.reply.take() {
                let _ = reply.send(if delete {
                    Err(UNAVAILABLE.into())
                } else {
                    Ok(Reply::Queued)
                });
            }
            !delete
        });
    }
    fn reject_requests(&self) {
        let requests = {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            let requests = std::mem::take(&mut state.requests);
            if requests.iter().any(|request| {
                request.reply.is_some() || !matches!(request.operation, Operation::Restore(_))
            }) {
                // Queued requests no longer have a reply channel. Publish a
                // terminal failure even if ordinary progress was just sent:
                // there may be no subsequent owner turn to flush this notice.
                state.failure = Some(StorageFailure::Unavailable);
                state.pending_notice = true;
                state.last_notice = None;
            }
            requests
        };
        for request in requests {
            if let Some(reply) = request.reply {
                let _ = reply.send(Err(UNAVAILABLE.into()));
            }
        }
        self.pump(self.now());
    }
}

pub(super) fn runnable(job: &JobStatus) -> bool {
    job.phase != Phase::Reclaimable
        && !job.paused
        && !matches!(
            job.reason,
            Some(
                PauseReason::Cancelled
                    | PauseReason::BlockedStream
                    | PauseReason::BackupPoisoned
                    | PauseReason::Superseded
            )
        )
}
fn needs_resume(job: &JobStatus) -> bool {
    maxed_store::storage_usage::maintenance_usage(std::slice::from_ref(job)).jobs[0].needs_resume
}

enum Owner<'a> {
    Live(&'a maxed_store::StoreWriter),
    Stopped(&'a mut StoppedMaintenance),
}
impl Owner<'_> {
    fn request(&mut self) -> Result<ControlResult, BackupError> {
        let at = SystemBackupEnvironment.timestamp();
        match self {
            Self::Live(w) => w.request_receipt_conversion(at),
            Self::Stopped(w) => w.request_receipt_conversion(at),
        }
    }
    fn control(&mut self, control: Control) -> Result<ControlResult, BackupError> {
        Ok(match self {
            Self::Live(w) => w.maintenance(control),
            Self::Stopped(w) => w.control(control),
        }?)
    }
    fn backup(&self) -> Result<ReceiptBackupTask, BackupError> {
        match self {
            Self::Live(w) => w.receipt_backup_task(),
            Self::Stopped(w) => w.receipt_backup_task(),
        }
    }
    fn delete(&self, id: BackupId) -> Result<(), BackupError> {
        match self {
            Self::Live(w) => w.delete_backup(id, &SystemBackupEnvironment),
            Self::Stopped(w) => w.delete_backup(id, &SystemBackupEnvironment),
        }
    }
}

struct CopyWork {
    cancel: Arc<AtomicBool>,
    worker: JoinHandle<Result<(maxed_store::backup::BackupInfo, i64), BackupError>>,
}
#[derive(Default)]
pub(super) struct OwnerWork {
    job: Option<JobStatus>,
    copy: Option<CopyWork>,
    attempted: bool,
    backup_failed: bool,
    closed: bool,
}
impl OwnerWork {
    fn running(&self, backing_off: bool) -> bool {
        self.copy.is_some()
            || !backing_off && !self.backup_failed && self.job.as_ref().is_some_and(runnable)
    }
    pub(super) fn stop(&mut self) {
        self.closed = true;
        if let Some(copy) = self.copy.take() {
            copy.cancel.store(true, Ordering::Release);
            let _ = copy.worker.join();
        }
    }
    pub(super) fn closed(&self) -> bool {
        self.closed
    }
    pub(super) fn service_live(
        &mut self,
        writer: &maxed_store::StoreWriter,
        service: &Arc<StorageService>,
    ) {
        self.service(&mut Owner::Live(writer), service);
        service.pump(Instant::now());
    }
    fn remember(&mut self, job: JobStatus, service: &StorageService) {
        self.job = Some(job.clone());
        service.observe(vec![job]);
    }
    fn service(&mut self, owner: &mut Owner<'_>, service: &Arc<StorageService>) {
        if self.closed || service.halted.load(Ordering::Acquire) {
            return;
        }
        let Ok(_estimate) = service.estimate_gate.try_lock() else {
            return;
        };
        if self
            .copy
            .as_ref()
            .is_some_and(|copy| copy.worker.is_finished())
        {
            let copy = self.copy.take().unwrap();
            *service
                .copy_cancel
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = None;
            let result = copy
                .worker
                .join()
                .unwrap_or(Err(BackupError::OwnerPoisoned));
            if matches!(result, Err(BackupError::OwnerPoisoned)) {
                if let Ok(result) = owner.control(Control::BackupOwnerFailed) {
                    self.remember(result.status, service);
                }
            }
            match result.and_then(|(_, id)| owner.control(Control::AuthorizationChanged(id))) {
                Ok(result) => {
                    service.reset_copy_retry();
                    service.clear_deferred_failure();
                    self.backup_failed = false;
                    self.remember(result.status, service);
                }
                Err(error) => {
                    let deferred = matches!(
                        error,
                        BackupError::Busy | BackupError::InUse | BackupError::WalPressure
                    );
                    self.backup_failed = !deferred;
                    self.attempted = !deferred;
                    if deferred {
                        service.defer_copy();
                    }
                    service.fail(Some(failure(error)));
                }
            }
        }
        let request = service
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .requests
            .pop_front();
        if let Some(request) = request {
            let observation = matches!(request.operation, Operation::Delete(_));
            let mut result = self.apply(owner, service, request.operation.clone());
            if matches!(result, Err(BackupError::Busy))
                && matches!(request.operation, Operation::Compact | Operation::Resume)
            {
                // Preserve command order (especially a following Cancel) and
                // retry on a later owner turn without recording a lasting error.
                service
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .requests
                    .push_front(Request {
                        operation: request.operation,
                        reply: None,
                    });
                result = Ok(Reply::Queued);
            }
            if !observation {
                if let Err(error) = result.as_ref() {
                    service.fail(Some(failure(*error)));
                }
            }
            if let Some(reply) = request.reply {
                let _ = reply.send(result.map_err(|e| e.to_string()));
            }
        }
        if let Some(id) = self.job.as_ref().map(|j| j.id) {
            match owner.control(Control::Status(id)) {
                Ok(result) => self.remember(result.status, service),
                Err(error) => {
                    service.fail(Some(failure(error)));
                    return;
                }
            }
        }
        if self.copy.is_none()
            && !self.attempted
            && service.copy_ready()
            && self.job.as_ref().is_some_and(runnable)
        {
            service.begin_copy();
            self.attempted = true;
            match owner.backup().and_then(|task| {
                let environment = service.environment();
                let cancel = Arc::new(AtomicBool::new(false));
                let worker_cancel = cancel.clone();
                let worker = std::thread::Builder::new()
                    .name("history-backup".into())
                    .spawn(move || {
                        task.run(
                            environment.as_ref(),
                            &worker_cancel,
                            &mut std::thread::yield_now,
                        )
                    })
                    .map_err(|_| BackupError::Io)?;
                *service
                    .copy_cancel
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = Some(cancel.clone());
                Ok(CopyWork { cancel, worker })
            }) {
                Ok(copy) => self.copy = Some(copy),
                Err(error) => {
                    let deferred = matches!(
                        error,
                        BackupError::Busy | BackupError::InUse | BackupError::WalPressure
                    );
                    self.backup_failed = !deferred;
                    self.attempted = !deferred;
                    if deferred {
                        service.defer_copy();
                    }
                    service.fail(Some(failure(error)));
                }
            }
        }
        if self.copy.is_none()
            && !self.backup_failed
            && service.copy_ready()
            && self.job.as_ref().is_some_and(runnable)
        {
            if let Owner::Stopped(owner) = owner {
                if let Err(error) = owner.step() {
                    service.fail(Some(failure(error.into())));
                }
            }
        }
    }
    fn apply(
        &mut self,
        owner: &mut Owner<'_>,
        service: &StorageService,
        operation: Operation,
    ) -> Result<Reply, BackupError> {
        match operation {
            Operation::Compact => {
                service.reset_copy_retry();
                // Repeated clicks join the existing request, including its copy.
                if self.copy.is_none() {
                    let result = owner.request()?;
                    let retry = self.job.as_ref().is_none_or(|j| j.id != result.status.id)
                        || service
                            .state
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .failure
                            .is_some();
                    if retry {
                        self.attempted = false;
                        self.backup_failed = false;
                    }
                    self.remember(result.status, service);
                }
            }
            Operation::Restore(jobs) => {
                if self.job.is_none() {
                    if let Some(job) = jobs.into_iter().rev().find(runnable) {
                        self.remember(owner.control(Control::Status(job.id))?.status, service);
                    }
                }
            }
            Operation::Resume => {
                service.reset_copy_retry();
                if self.backup_failed || !self.attempted {
                    self.backup_failed = false;
                    self.attempted = false;
                }
                if self.job.is_none() {
                    self.job = service
                        .state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .jobs
                        .as_ref()
                        .and_then(|j| j.last().cloned());
                }
                if self.job.as_ref().is_some_and(needs_resume) {
                    // Renewal precedes Resume when a paused/blocked job lost its
                    // backup. Resume alone never clears the native failure note.
                    let id = owner.request()?.status.id;
                    self.remember(owner.control(Control::Resume(id))?.status, service);
                    self.attempted = false;
                    self.backup_failed = false;
                }
            }
            Operation::Cancel => {
                service.reset_copy_retry();
                if let Some(copy) = self.copy.take() {
                    copy.cancel.store(true, Ordering::Release);
                    let _ = copy.worker.join();
                    *service
                        .copy_cancel
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = None;
                }
                if self.job.is_none() {
                    self.job = service
                        .state
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .jobs
                        .as_ref()
                        .and_then(|j| j.last().cloned());
                }
                if let Some(id) = self.job.as_ref().map(|j| j.id) {
                    self.remember(owner.control(Control::Pause(id))?.status, service);
                }
            }
            Operation::Delete(id) => {
                owner.delete(id)?;
            }
        }
        Ok(Reply::Done)
    }
}
impl Drop for OwnerWork {
    fn drop(&mut self) {
        self.stop();
    }
}

impl CaptureRuntime {
    fn live_storage_mode(&self) -> bool {
        self.shared
            .alerts
            .lock()
            .is_ok_and(|state| state.namespace == Namespace::Live)
    }
    pub(crate) fn set_storage_notice_sink(&self, sink: NoticeSink) {
        let mut state = self
            .shared
            .views
            .maintenance
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        state.sink = Some(sink);
        state.pending_notice = true;
    }
    fn storage_operation(&self, operation: Operation) -> Result<Reply, String> {
        let session = self.session.lock().map_err(|_| UNAVAILABLE)?;
        let service = &self.shared.views.maintenance;
        if self.closing.load(Ordering::Acquire)
            || service.update_paused.load(Ordering::Acquire)
            || self.shared.upgrade.running()
            || !self.live_storage_mode()
            || self.has_unsaved_summaries()
            || self
                .shared
                .completed
                .lock()
                .map_err(|_| UNAVAILABLE)?
                .is_some()
        {
            return Err(UNAVAILABLE.into());
        }
        if session.as_ref().is_some_and(|s| !s.worker.is_finished())
            && !service.online.load(Ordering::Acquire)
        {
            return Err("Storage is starting; retry maintenance when capture is ready.".into());
        }
        let path = self.shared.home.live_database().ok_or(UNAVAILABLE)?;
        let (send, receive) = crossbeam_channel::bounded(1);
        if !service.online.load(Ordering::Acquire) && !service.has_background() {
            let state = service.state.lock().map_err(|_| UNAVAILABLE)?;
            let needed = match &operation {
                Operation::Resume => state.jobs.as_ref().map(|jobs| {
                    jobs.iter().any(needs_resume)
                        || state.copy_retry_at.is_some() && jobs.iter().any(runnable)
                }),
                Operation::Cancel => state.jobs.as_ref().map(|jobs| jobs.iter().any(runnable)),
                _ => Some(true),
            };
            if needed == Some(false)
                || needed.is_none()
                    && !path
                        .parent()
                        .ok_or(UNAVAILABLE)?
                        .join("backups/live/manifest.sqlite3")
                        .is_file()
            {
                return Ok(Reply::Done);
            }
        }
        service.enqueue(operation, Some(send));
        if !service.online.load(Ordering::Acquire) {
            if let Err(error) = service.start_background(path) {
                service.reject_requests();
                return Err(error);
            }
        }
        drop(session);
        loop {
            match receive.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => return result,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Err(UNAVAILABLE.into())
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if !self.is_running() && !service.has_background() {
                        // A reply can arrive during is_running's lifecycle-lock
                        // wait (notably synchronous replay). Never revoke the
                        // retained FIFO just because this caller lost its owner.
                        service.defer_requests();
                        return receive
                            .try_recv()
                            .unwrap_or_else(|_| Err(UNAVAILABLE.into()));
                    }
                }
            }
        }
    }
    pub fn compact_storage(&self) -> Result<StorageCommandOutcome, String> {
        self.storage_operation(Operation::Compact).map(Into::into)
    }
    fn load_storage_jobs(self: &Arc<Self>) -> Result<(), String> {
        let service = &self.shared.views.maintenance;
        if service.online.load(Ordering::Acquire)
            || service.has_background()
            || service
                .state
                .lock()
                .map_err(|_| UNAVAILABLE)?
                .jobs
                .is_some()
        {
            return Ok(());
        }
        let jobs = tauri::async_runtime::block_on(self.read_storage_jobs())?;
        service.observe(jobs);
        Ok(())
    }
    pub(super) async fn read_storage_jobs(self: &Arc<Self>) -> Result<Vec<JobStatus>, String> {
        let mut delay = Duration::from_millis(100);
        loop {
            if self.closing.load(Ordering::Acquire)
                || self
                    .shared
                    .views
                    .maintenance
                    .update_paused
                    .load(Ordering::Acquire)
                || !self.live_storage_mode()
                || self.shared.upgrade.running()
            {
                return Err(UNAVAILABLE.into());
            }
            self.wait_history_preparation().await;
            let runtime = self.clone();
            match self
                .submit_history(
                    super::history::HistoryCaller::Storage,
                    super::history::HistoryKey::MaintenanceJobs,
                    move |job| {
                        runtime.read_history(job, Vec::new, |reader| reader.maintenance_statuses())
                    },
                )
                .await
            {
                Ok(HistoryReply::Ready { value }) => return Ok(value),
                Ok(HistoryReply::Busy | HistoryReply::Superseded) => {}
                Err(error)
                    if error == super::home::UNAVAILABLE || error == super::home::CHANGED => {}
                Err(error) => return Err(error),
            }
            #[cfg(test)]
            {
                let hook = self
                    .shared
                    .views
                    .maintenance
                    .on_jobs_retry
                    .lock()
                    .unwrap()
                    .clone();
                if let Some(hook) = hook {
                    hook();
                }
            }
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(5));
        }
    }
    pub fn resume_storage_maintenance(self: &Arc<Self>) -> Result<StorageCommandOutcome, String> {
        self.load_storage_jobs()?;
        self.storage_operation(Operation::Resume).map(Into::into)
    }
    pub fn cancel_storage_maintenance(self: &Arc<Self>) -> Result<StorageCommandOutcome, String> {
        self.shared.upgrade.cancel.store(true, Ordering::Release);
        if self.shared.upgrade.running() {
            return Ok(StorageCommandOutcome::Accepted);
        }
        self.shared.views.maintenance.cancel_copy();
        self.load_storage_jobs()?;
        self.storage_operation(Operation::Cancel).map(Into::into)
    }
    pub(super) fn resume_requested_storage(&self, jobs: Vec<JobStatus>) {
        let service = &self.shared.views.maintenance;
        service.observe(jobs.clone());
        if jobs.iter().any(runnable) {
            let _ = self.storage_operation(Operation::Restore(jobs));
        }
    }
    pub(super) fn resume_cached_storage(&self) {
        // Serialize admission with capture/playback startup and command enqueue.
        // Do not wait for an owner turn while holding this lifecycle lock.
        let session = self.session.lock().unwrap_or_else(|p| p.into_inner());
        let service = &self.shared.views.maintenance;
        if self.closing.load(Ordering::Acquire)
            || service.update_paused.load(Ordering::Acquire)
            || self.shared.upgrade.running()
            || !self.live_storage_mode()
            || self.has_unsaved_summaries()
            || session.is_some()
            || self
                .shared
                .completed
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_some()
        {
            return;
        }
        let jobs = service
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .jobs
            .clone();
        if let Some(jobs) = jobs.filter(|jobs| jobs.iter().any(runnable)) {
            service.enqueue(Operation::Restore(jobs), None);
        }
        // Compact may not have created a durable job, and Resume may still be
        // queued behind a busy backup owner while every cached job is paused.
        if service
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .requests
            .is_empty()
        {
            return;
        }
        if self
            .shared
            .home
            .live_database()
            .is_none_or(|path| service.start_background(path).is_err())
        {
            service.reject_requests();
        }
    }
    fn stopped_inventory(&self, delete: Option<BackupId>) -> Result<Inventory, String> {
        let _session = self.session.lock().map_err(|_| UNAVAILABLE)?;
        if self.shared.upgrade.running()
            || self.has_unsaved_summaries()
            || !self.live_storage_mode()
            || self.closing.load(Ordering::Acquire)
            || self
                .shared
                .views
                .maintenance
                .update_paused
                .load(Ordering::Acquire)
        {
            return Err(UNAVAILABLE.into());
        }
        let path = self.shared.home.live_database().ok_or(UNAVAILABLE)?;
        if delete.is_none() {
            return maxed_store::backup::BackupStore::read_inventory(&path)
                .map_err(|e| e.to_string());
        }
        let root = path.parent().ok_or(UNAVAILABLE)?;
        let _lease = namespace_lease(root, Namespace::Live).map_err(|_| UNAVAILABLE)?;
        let manifest = root.join("backups/live/manifest.sqlite3");
        if !manifest.try_exists().map_err(|_| UNAVAILABLE)? {
            return if delete.is_some() {
                Err("Storage backup ID is invalid.".into())
            } else {
                Ok((
                    vec![],
                    BackupUsage {
                        retained_bytes: Some(0),
                        temporary_bytes: Some(0),
                    },
                ))
            };
        }
        let mut owner = maxed_store::backup::BackupStore::open(&path).map_err(|e| e.to_string())?;
        if let Some(id) = delete {
            owner
                .delete_managed(id, &SystemBackupEnvironment)
                .map_err(|e| e.to_string())?;
        }
        Ok((
            owner.list_managed().map_err(|e| e.to_string())?,
            owner.usage().map_err(|e| e.to_string())?,
        ))
    }
    fn storage_inventory(&self) -> Result<Inventory, String> {
        self.stopped_inventory(None)
    }
    pub fn list_storage_backups(&self) -> Result<Vec<ManagedStorageBackup>, String> {
        let (backups, _) = self.storage_inventory()?;
        Ok(backups
            .into_iter()
            .map(|b| ManagedStorageBackup {
                id: b.id.0.to_string(),
                role: match b.slot {
                    maxed_store::backup::Slot::Initial => StorageBackupRole::Initial,
                    maxed_store::backup::Slot::Rolling => StorageBackupRole::Rolling,
                },
                completed_at: b.completed_at.map(Timestamp),
                bytes: b.bytes.map(StorageCount),
                ready: b.ready,
                in_use: b.in_use,
            })
            .collect())
    }
    pub fn delete_storage_backup(&self, id: &str) -> Result<(), String> {
        let id = managed_id(id)?;
        let service = &self.shared.views.maintenance;
        if service.online.load(Ordering::Acquire) || service.has_background() {
            self.storage_operation(Operation::Delete(id)).map(|_| ())
        } else {
            self.stopped_inventory(Some(id)).map(|_| ())
        }
    }
    pub async fn storage_status(
        self: &Arc<Self>,
        mut status: StorageStatus,
        estimate: bool,
    ) -> StorageStatus {
        use super::history::{HistoryCaller, HistoryKey};
        let runtime = self.clone();
        if let Ok(HistoryReply::Ready {
            value: Some((logical, jobs, measured)),
        }) = self
            .submit_history(
                HistoryCaller::Storage,
                HistoryKey::StorageStatus(estimate),
                move |job| runtime.read_existing_storage(job, estimate),
            )
            .await
        {
            crate::storage_usage::merge_logical(&mut status.database, logical);
            self.shared.views.maintenance.observe(jobs);
            if let Some(measured) = measured {
                self.shared
                    .views
                    .maintenance
                    .state
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .estimate = measured;
            }
        }
        let runtime = self.clone();
        let usage = tauri::async_runtime::spawn_blocking(move || {
            runtime.storage_inventory().map(|(_, usage)| usage)
        })
        .await;
        let state = self
            .shared
            .views
            .maintenance
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        status.backups = match usage {
            Ok(Ok(usage)) => maxed_store::storage_usage::backup_usage(usage),
            _ => BackupStorage::default(),
        };
        status.maintenance = state
            .jobs
            .as_deref()
            .map(maxed_store::storage_usage::maintenance_usage);
        status.receipt_estimate = state.estimate.clone();
        status.failure = state.failure;
        status
    }
}

fn managed_id(text: &str) -> Result<BackupId, String> {
    if text.is_empty() || text.starts_with('0') || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Storage backup ID is invalid.".into());
    }
    text.parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .map(BackupId)
        .ok_or_else(|| "Storage backup ID is invalid.".into())
}

#[cfg(test)]
#[path = "../../tests/support/maintenance_handoff.rs"]
mod handoff_tests;

#[cfg(test)]
mod maintenance_tests {
    use super::super::maintenance_tests::{current_fixture, eventually, initialized};
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct DeferredCopy {
        error: Mutex<Option<BackupError>>,
        copies: AtomicUsize,
    }
    impl BackupEnvironment for DeferredCopy {
        fn available_space(&self, _: &Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            100
        }
        fn boundary(&self, at: maxed_store::backup::Boundary) -> Result<(), BackupError> {
            if at == maxed_store::backup::Boundary::Copy {
                self.copies.fetch_add(1, Ordering::AcqRel);
                if let Some(error) = *self.error.lock().unwrap() {
                    return Err(error);
                }
            }
            Ok(())
        }
    }

    #[test]
    fn successful_deferred_copy_clears_only_transient_failure_before_conversion_finishes() {
        for error in [
            BackupError::Busy,
            BackupError::WalPressure,
            BackupError::InUse,
        ] {
            for terminal in [
                None,
                Some(StorageFailure::Io),
                Some(StorageFailure::OwnerPoisoned),
            ] {
                let root = current_fixture();
                let writer =
                    maxed_store::start_writer(root.path().join("live.sqlite3"), Default::default())
                        .unwrap();
                writer.set_maintenance_blocked(true);
                let service = Arc::new(StorageService::default());
                service.attach(true);
                let now = Arc::new(Mutex::new(Instant::now()));
                *service.clock.lock().unwrap() = Some({
                    let now = now.clone();
                    Arc::new(move || *now.lock().unwrap())
                });
                let env = Arc::new(DeferredCopy {
                    error: Mutex::new(Some(error)),
                    copies: AtomicUsize::new(0),
                });
                service.set_environment(env.clone());
                service.enqueue(Operation::Compact, None);
                let mut work = OwnerWork::default();
                work.service_live(&writer, &service);
                eventually(|| work.copy.as_ref().is_some_and(|c| c.worker.is_finished()));
                work.service_live(&writer, &service);
                assert_eq!(service.state.lock().unwrap().failure, Some(failure(error)));
                *env.error.lock().unwrap() = None;
                *now.lock().unwrap() += Duration::from_secs(3);
                work.service_live(&writer, &service);
                eventually(|| work.copy.as_ref().is_some_and(|c| c.worker.is_finished()));
                if let Some(terminal) = terminal {
                    service.fail(Some(terminal));
                }
                if terminal == Some(StorageFailure::OwnerPoisoned) {
                    writer.maintenance(Control::BackupOwnerFailed).unwrap();
                }
                work.service_live(&writer, &service);
                assert!(work.copy.is_none());
                assert_ne!(work.job.as_ref().unwrap().phase, Phase::Reclaimable);
                assert_eq!(
                    service.state.lock().unwrap().failure,
                    terminal,
                    "successful retry must clear only the deferred failure"
                );
                if terminal == Some(StorageFailure::OwnerPoisoned) {
                    let db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
                    let note: String = db
                        .query_row("SELECT safe_error FROM maintenance_jobs", [], |r| r.get(0))
                        .unwrap();
                    assert_eq!(
                        note, "backup_poisoned",
                        "copy success must preserve the job's persisted failure"
                    );
                }
                work.stop();
                writer.shutdown().unwrap();
            }
        }
    }

    #[test]
    fn busy_conversion_request_is_queued_until_backup_owner_is_available() {
        for resume in [false, true] {
            let root = current_fixture();
            let writer =
                maxed_store::start_writer(root.path().join("live.sqlite3"), Default::default())
                    .unwrap();
            writer.set_maintenance_blocked(true);
            let id = writer.request_receipt_conversion(100).unwrap().status.id;
            let paused = writer.maintenance(Control::Pause(id)).unwrap().status;
            let service = Arc::new(StorageService::default());
            service.observe(vec![paused]);
            service.attach(true);
            let task = writer.receipt_backup_task().unwrap();
            let (entered, waiting) = crossbeam_channel::bounded(1);
            let (release, blocked) = crossbeam_channel::bounded(1);
            let copy = std::thread::spawn(move || {
                let mut first = true;
                task.run(
                    &super::super::maintenance_tests::Space(Some(u64::MAX)),
                    &AtomicBool::new(false),
                    &mut || {
                        if first {
                            first = false;
                            entered.send(()).unwrap();
                            blocked.recv_timeout(Duration::from_secs(15)).unwrap();
                        }
                    },
                )
            });
            waiting.recv_timeout(Duration::from_secs(15)).unwrap();
            let (send, receive) = crossbeam_channel::bounded(1);
            service.enqueue(
                if resume {
                    Operation::Resume
                } else {
                    Operation::Compact
                },
                Some(send),
            );
            let mut work = OwnerWork::default();
            work.service_live(&writer, &service);
            let reply = receive.recv_timeout(Duration::from_secs(1));
            let queued = service.state.lock().unwrap().requests.len();
            let failure = service.state.lock().unwrap().failure;
            let _ = release.send(());
            copy.join().unwrap().unwrap();
            assert!(matches!(reply, Ok(Ok(Reply::Queued))));
            assert_eq!(queued, 1);
            assert_eq!(failure, None);
            work.service_live(&writer, &service);
            assert!(service.state.lock().unwrap().requests.is_empty());
            assert!(work.job.is_some());
            if resume {
                assert!(!needs_resume(work.job.as_ref().unwrap()));
            }
            work.stop();
            writer.shutdown().unwrap();
        }
    }

    #[test]
    fn busy_stopped_request_survives_live_handoff_with_cancel_order_preserved() {
        use super::super::{Bridge, RuntimeOptions};

        for resume in [false, true] {
            for cancel in [false, true] {
                let root = current_fixture();
                let path = root.path().join("live.sqlite3");
                if resume {
                    let mut owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
                    let id = owner.request_receipt_conversion(100).unwrap().status.id;
                    owner.control(Control::Pause(id)).unwrap();
                } else {
                    // Create only the manifest: Compact must survive even when
                    // there is no durable job for attach(true) to restore.
                    drop(maxed_store::backup::BackupStore::open(&path).unwrap());
                }
                let runtime = initialized(root.path());
                let service = &runtime.shared.views.maintenance;
                // Keep backup/conversion pending in all cases. This test proves
                // durable admission and command order across ownership transfer;
                // it must not depend on backup/fsync throughput under CI load.
                service.set_environment(Arc::new(super::super::maintenance_tests::Space(None)));
                let manifest =
                    rusqlite::Connection::open(root.path().join("backups/live/manifest.sqlite3"))
                        .unwrap();
                manifest.execute_batch("BEGIN IMMEDIATE").unwrap();
                let outcome = if resume {
                    runtime.resume_storage_maintenance()
                } else {
                    runtime.compact_storage()
                };
                assert_eq!(
                    outcome,
                    Ok(StorageCommandOutcome::Queued),
                    "resume={resume}, cancel={cancel}"
                );
                let db = rusqlite::Connection::open(&path).unwrap();
                assert_eq!(
                    db.query_row("SELECT count(*) FROM maintenance_jobs", [], |r| r
                        .get::<_, i64>(0))
                        .unwrap(),
                    i64::from(resume)
                );
                let (send, receive) = crossbeam_channel::bounded(1);
                if cancel {
                    service.enqueue(Operation::Cancel, Some(send));
                }
                let mut adapter = runtime
                    .shared
                    .open_home(
                        &RuntimeOptions::live(root.path().into(), None),
                        &AtomicBool::new(false),
                        Bridge::default(),
                        None,
                    )
                    .unwrap();
                // The lock spans the handoff so the stopped owner cannot win a
                // retry and hide a lost volatile command behind a durable job.
                {
                    let state = service.state.lock().unwrap();
                    assert_eq!(
                        state.requests.len(),
                        1 + usize::from(cancel),
                        "handoff lost queued work: resume={resume}, cancel={cancel}"
                    );
                }
                manifest.execute_batch("ROLLBACK").unwrap();
                drop(manifest);
                // One synchronous owner turn must persist Compact/Resume before
                // the following Cancel gets its own turn. No retry worker or
                // manifest lock remains to race these assertions.
                adapter.service_maintenance();
                let assert_job = |paused: bool| {
                    let state = service.state.lock().unwrap();
                    assert!(
                        state.jobs.as_ref().is_some_and(|jobs| jobs.len() == 1
                            && jobs[0].phase == Phase::AwaitingBackup && jobs[0].paused == paused),
                        "handoff did not persist command: resume={resume}, cancel={cancel}, paused={paused}, requests={}, jobs={:?}, failure={:?}",
                        state.requests.len(),
                        state.jobs.as_ref().map(|jobs| jobs.iter().map(|job| (
                            job.phase, job.paused, job.reason, job.processed_records
                        )).collect::<Vec<_>>()),
                        state.failure,
                    );
                    drop(state);
                    assert_eq!(
                        db.query_row("SELECT count(*) FROM maintenance_jobs WHERE phase='awaiting_backup' AND paused=?1",
                            [paused], |r| r.get::<_, i64>(0)).unwrap(),
                        1,
                        "durable command: resume={resume}, cancel={cancel}, paused={paused}"
                    );
                };
                assert_job(false);
                assert_eq!(
                    service.state.lock().unwrap().requests.len(),
                    usize::from(cancel)
                );
                if cancel {
                    adapter.service_maintenance();
                    assert_job(true);
                    assert!(matches!(receive.try_recv(), Ok(Ok(Reply::Done))));
                }
                assert!(service.state.lock().unwrap().requests.is_empty());
                adapter.close().unwrap();
                drop(adapter);
                runtime.shutdown().unwrap();
            }
        }
    }

    #[test]
    fn live_wal_pressure_copy_waits_for_backoff() {
        let root = current_fixture();
        let writer =
            maxed_store::start_writer(root.path().join("live.sqlite3"), Default::default())
                .unwrap();
        let service = Arc::new(StorageService::default());
        service.attach(true);
        let now = Arc::new(Mutex::new(Instant::now()));
        *service.clock.lock().unwrap() = Some({
            let now = now.clone();
            Arc::new(move || *now.lock().unwrap())
        });
        let env = Arc::new(DeferredCopy {
            error: Mutex::new(Some(BackupError::WalPressure)),
            copies: AtomicUsize::new(0),
        });
        service.set_environment(env.clone());
        service.enqueue(Operation::Compact, None);
        let mut work = OwnerWork::default();
        work.service_live(&writer, &service);
        eventually(|| work.copy.as_ref().is_some_and(|c| c.worker.is_finished()));
        work.service_live(&writer, &service);
        for _ in 0..20 {
            work.service_live(&writer, &service);
        }
        assert!(
            work.copy.is_none(),
            "deferred copy restarted on a service turn"
        );
        assert_eq!(env.copies.load(Ordering::Acquire), 1);
        work.stop();
        work = OwnerWork::default();
        service.attach(true);
        work.service_live(&writer, &service);
        assert!(work.copy.is_none(), "owner transfer must preserve backoff");
        *now.lock().unwrap() += Duration::from_secs(2);
        work.service_live(&writer, &service);
        assert!(work.copy.is_none());
        *env.error.lock().unwrap() = None;
        *now.lock().unwrap() += Duration::from_secs(1);
        work.service_live(&writer, &service);
        eventually(|| {
            work.service_live(&writer, &service);
            work.job
                .as_ref()
                .is_some_and(|j| j.phase == Phase::Reclaimable)
        });
        assert!(!writer.list_backups().unwrap().is_empty());
        assert_eq!(
            service.state.lock().unwrap().copy_retry_delay,
            Duration::ZERO
        );
        work.stop();
        writer.shutdown().unwrap();
    }

    #[test]
    fn stopped_busy_copy_retires_between_retries_and_eventually_completes() {
        let root = current_fixture();
        let runtime = initialized(root.path());
        let service = &runtime.shared.views.maintenance;
        let now = Arc::new(Mutex::new(Instant::now()));
        *service.clock.lock().unwrap() = Some({
            let now = now.clone();
            Arc::new(move || *now.lock().unwrap())
        });
        let env = Arc::new(DeferredCopy {
            error: Mutex::new(Some(BackupError::Busy)),
            copies: AtomicUsize::new(0),
        });
        service.set_environment(env.clone());
        runtime.compact_storage().unwrap();
        eventually(|| service.state.lock().unwrap().failure == Some(StorageFailure::Busy));
        eventually(|| !service.has_background());
        runtime.clear_chat_history().unwrap();
        runtime
            .set_party_mark(
                "synthetic".into(),
                "Synthetic".into(),
                Some(PartyMark::Watched),
                None,
            )
            .unwrap();
        assert_eq!(env.copies.load(Ordering::Acquire), 1);
        *now.lock().unwrap() += Duration::from_secs(3);
        service.retry_changed.notify_waiters();
        eventually(|| env.copies.load(Ordering::Acquire) == 2 && !service.has_background());
        assert_eq!(
            service.state.lock().unwrap().copy_retry_delay,
            Duration::from_secs(6)
        );
        runtime.clear_chat_history().unwrap();
        runtime
            .set_party_mark("synthetic".into(), "Synthetic".into(), None, None)
            .unwrap();
        *now.lock().unwrap() += Duration::from_secs(5);
        service.retry_changed.notify_waiters();
        let at = service.state.lock().unwrap().copy_retry_at;
        assert!(service
            .start_background_inner(root.path().join("live.sqlite3"), at)
            .is_ok());
        assert!(!service.has_background());
        assert_eq!(env.copies.load(Ordering::Acquire), 2);
        *env.error.lock().unwrap() = None;
        *now.lock().unwrap() += Duration::from_secs(1);
        service.retry_changed.notify_waiters();
        eventually(|| {
            service
                .state
                .lock()
                .unwrap()
                .jobs
                .as_ref()
                .is_some_and(|jobs| jobs.iter().any(|j| j.phase == Phase::Reclaimable))
        });
        runtime.shutdown().unwrap();
    }

    #[test]
    fn deferred_copy_explicit_retry_resets_delay_and_cancel_does_not_wake() {
        let root = current_fixture();
        let runtime = initialized(root.path());
        let service = &runtime.shared.views.maintenance;
        let now = Arc::new(Mutex::new(Instant::now()));
        *service.clock.lock().unwrap() = Some({
            let now = now.clone();
            Arc::new(move || *now.lock().unwrap())
        });
        let env = Arc::new(DeferredCopy {
            error: Mutex::new(Some(BackupError::InUse)),
            copies: AtomicUsize::new(0),
        });
        service.set_environment(env.clone());
        runtime.compact_storage().unwrap();
        eventually(|| env.copies.load(Ordering::Acquire) == 1 && !service.has_background());
        let old_deadline = service.state.lock().unwrap().copy_retry_at.unwrap();
        runtime.resume_storage_maintenance().unwrap();
        eventually(|| env.copies.load(Ordering::Acquire) == 2 && !service.has_background());
        assert_eq!(
            service.state.lock().unwrap().copy_retry_delay,
            Duration::from_secs(3)
        );
        runtime.cancel_storage_maintenance().unwrap();
        eventually(|| !service.has_background());
        *now.lock().unwrap() += Duration::from_secs(60);
        service
            .start_background_inner(root.path().join("live.sqlite3"), Some(old_deadline))
            .unwrap();
        assert!(!service.has_background());
        assert_eq!(env.copies.load(Ordering::Acquire), 2);
        runtime.clear_chat_history().unwrap();
        runtime.shutdown().unwrap();
    }

    #[test]
    fn stopped_copy_retry_wake_respects_capture_and_shutdown() {
        for capture in [false, true] {
            let root = current_fixture();
            let runtime = initialized(root.path());
            let service = &runtime.shared.views.maintenance;
            let now = Arc::new(Mutex::new(Instant::now()));
            *service.clock.lock().unwrap() = Some({
                let now = now.clone();
                Arc::new(move || *now.lock().unwrap())
            });
            let env = Arc::new(DeferredCopy {
                error: Mutex::new(Some(BackupError::Busy)),
                copies: AtomicUsize::new(0),
            });
            service.set_environment(env.clone());
            runtime.compact_storage().unwrap();
            eventually(|| env.copies.load(Ordering::Acquire) == 1 && !service.has_background());
            let deadline = service.state.lock().unwrap().copy_retry_at.unwrap();
            if capture {
                service.stop_background();
                service.attach(true);
            } else {
                runtime.shutdown().unwrap();
            }
            *now.lock().unwrap() += Duration::from_secs(3);
            service
                .start_background_inner(root.path().join("live.sqlite3"), Some(deadline))
                .unwrap();
            assert!(!service.has_background());
            assert_eq!(env.copies.load(Ordering::Acquire), 1);
            assert!(namespace_lease(root.path(), Namespace::Live).is_ok());
        }
    }

    #[test]
    fn terminal_copy_failure_after_backoff_does_not_schedule_another_retry() {
        let root = current_fixture();
        let runtime = initialized(root.path());
        let service = &runtime.shared.views.maintenance;
        let now = Arc::new(Mutex::new(Instant::now()));
        *service.clock.lock().unwrap() = Some({
            let now = now.clone();
            Arc::new(move || *now.lock().unwrap())
        });
        let env = Arc::new(DeferredCopy {
            error: Mutex::new(Some(BackupError::Busy)),
            copies: AtomicUsize::new(0),
        });
        service.set_environment(env.clone());
        runtime.compact_storage().unwrap();
        eventually(|| env.copies.load(Ordering::Acquire) == 1 && !service.has_background());
        let deadline = service.state.lock().unwrap().copy_retry_at.unwrap();
        *env.error.lock().unwrap() = Some(BackupError::Io);
        *now.lock().unwrap() += Duration::from_secs(3);
        service.retry_changed.notify_waiters();
        eventually(|| env.copies.load(Ordering::Acquire) == 2 && !service.has_background());
        assert_eq!(service.state.lock().unwrap().copy_retry_at, None);
        service
            .start_background_inner(root.path().join("live.sqlite3"), Some(deadline))
            .unwrap();
        assert!(!service.has_background());
        runtime.clear_chat_history().unwrap();
        runtime.shutdown().unwrap();
    }

    #[test]
    fn busy_lease_reports_queued_instead_of_done() {
        let root = current_fixture();
        let runtime = initialized(root.path());
        let lease = namespace_lease(root.path(), Namespace::Live).unwrap();
        let result = runtime.compact_storage();
        let service = &runtime.shared.views.maintenance;
        for operation in [Operation::Resume, Operation::Cancel] {
            let (send, receive) = crossbeam_channel::bounded(1);
            service.enqueue(operation, Some(send));
            assert!(matches!(
                receive.recv_timeout(Duration::from_secs(5)).unwrap(),
                Ok(Reply::Queued)
            ));
        }
        drop(lease);
        runtime.shutdown().unwrap();
        assert_eq!(serde_json::to_value(result.unwrap()).unwrap(), "queued");
    }

    #[test]
    fn copy_retry_expiring_during_another_stopped_request_is_not_lost() {
        let root = current_fixture();
        let path = root.path().join("live.sqlite3");
        let runtime = initialized(root.path());
        let service = &runtime.shared.views.maintenance;
        let now = Arc::new(Mutex::new(Instant::now()));
        *service.clock.lock().unwrap() = Some({
            let now = now.clone();
            Arc::new(move || *now.lock().unwrap())
        });
        let env = Arc::new(DeferredCopy {
            error: Mutex::new(Some(BackupError::Busy)),
            copies: AtomicUsize::new(0),
        });
        service.set_environment(env.clone());
        runtime.compact_storage().unwrap();
        eventually(|| env.copies.load(Ordering::Acquire) == 1 && !service.has_background());
        let at = service.state.lock().unwrap().copy_retry_at.unwrap();
        let gate = service.estimate_gate.lock().unwrap();
        service.enqueue(Operation::Delete(BackupId(999)), None);
        service.start_background(path.clone()).unwrap();
        assert!(service.has_background());
        *env.error.lock().unwrap() = None;
        *now.lock().unwrap() += Duration::from_secs(3);
        service.start_background_inner(path, Some(at)).unwrap();
        drop(gate);
        eventually(|| {
            service
                .state
                .lock()
                .unwrap()
                .jobs
                .as_ref()
                .is_some_and(|jobs| jobs.iter().any(|j| j.phase == Phase::Reclaimable))
        });
        runtime.shutdown().unwrap();
    }

    #[test]
    fn worker_delete_refusals_do_not_replace_the_maintenance_failure() {
        let root = tempfile::tempdir().unwrap();
        let mut owner =
            StoppedMaintenance::receipt_conversion(&root.path().join("live.sqlite3")).unwrap();
        owner.request_receipt_conversion(10).unwrap();
        let backup = owner
            .backup_receipt_conversion(
                &super::super::maintenance_tests::Space(Some(u64::MAX)),
                &AtomicBool::new(false),
                &mut || {},
            )
            .unwrap();
        owner.step().unwrap();
        let service = Arc::new(StorageService::default());
        service.fail(Some(StorageFailure::SpaceUnknown));
        let mut work = OwnerWork::default();
        for id in [BackupId(999), backup.id] {
            let (send, receive) = crossbeam_channel::bounded(1);
            service.enqueue(Operation::Delete(id), Some(send));
            work.service(&mut Owner::Stopped(&mut owner), &service);
            assert!(receive.recv().unwrap().is_err());
            assert_eq!(
                service.state.lock().unwrap().failure,
                Some(StorageFailure::SpaceUnknown)
            );
        }
    }
}
