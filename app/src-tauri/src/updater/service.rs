//! One joinable operation; cancellation drops the borrowed network future.
use super::{Channel, Error, NativeUpdater};
use maxed_types::{
    valid_update_request_id, UpdateChannel, UpdateFailure, UpdateOffer, UpdatePhase, UpdateStatus,
};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::Poll,
    thread::JoinHandle,
    time::Duration,
};

type Installer = Arc<dyn Fn(&mut NativeUpdater) -> super::Result<()> + Send + Sync>;
struct State {
    status: UpdateStatus,
    generation: u64,
    updater: Option<NativeUpdater>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    closing: bool,
    initialized: bool,
}
pub struct UpdateService {
    state: Arc<Mutex<State>>,
    commit: Mutex<()>,
    factory: Arc<dyn Fn() -> super::Result<NativeUpdater> + Send + Sync>,
    installer: Mutex<Option<Installer>>,
}
#[derive(Clone, Copy)]
enum Operation {
    Check,
    Download,
    Install,
}
fn channel(value: &UpdateChannel) -> Channel {
    match value {
        UpdateChannel::Stable => Channel::Stable,
        UpdateChannel::Beta => Channel::Beta,
    }
}
fn failed(error: Error) -> UpdateFailure {
    match error {
        Error::Network | Error::Deadline => UpdateFailure::Offline,
        Error::Size => UpdateFailure::TooLarge,
        Error::Target => UpdateFailure::UnsupportedTarget,
        Error::Configuration | Error::Verification(super::verification::VerificationError::Key) => {
            UpdateFailure::Unavailable
        }
        Error::Verification(_) => UpdateFailure::InvalidSignature,
        Error::Busy => UpdateFailure::Busy,
        Error::Stale => UpdateFailure::StaleOffer,
        Error::Shutdown => UpdateFailure::ShutdownBlocked,
        Error::Install => UpdateFailure::InstallFailed,
        _ => UpdateFailure::InvalidMetadata,
    }
}
fn busy(state: &State) -> UpdateStatus {
    let mut status = state.status.clone();
    status.failure = Some(UpdateFailure::Busy);
    status
}

impl UpdateService {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                status: UpdateStatus::idle(UpdateChannel::Stable),
                generation: 0,
                updater: None,
                cancel: Arc::new(AtomicBool::new(false)),
                worker: None,
                closing: false,
                initialized: false,
            })),
            commit: Mutex::new(()),
            factory: Arc::new(NativeUpdater::production),
            installer: Mutex::new(None),
        }
    }
    #[cfg(test)]
    pub(crate) fn with_factory(
        factory: Arc<dyn Fn() -> super::Result<NativeUpdater> + Send + Sync>,
    ) -> Self {
        let mut service = Self::new();
        service.factory = factory;
        service
    }
    pub fn configure_installer(&self, installer: Installer) {
        *self.installer.lock().unwrap_or_else(|p| p.into_inner()) = Some(installer);
    }
    pub fn status(&self) -> UpdateStatus {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .status
            .clone()
    }
    pub fn installing(&self) -> bool {
        self.status().phase == UpdatePhase::Installing
    }

    /// Called under setup's transition lock. No invalidation on failed persistence.
    pub fn commit_channel<T>(
        &self,
        next: UpdateChannel,
        commit: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let _commit = self
            .commit
            .lock()
            .map_err(|_| "Update service unavailable.")?;
        let state = self
            .state
            .lock()
            .map_err(|_| "Update service unavailable.")?;
        if state.status.phase == UpdatePhase::Installing || state.closing {
            return Err("Update operation is busy.".into());
        }
        let changed = state.status.channel != next;
        let generation = if changed {
            state
                .generation
                .checked_add(1)
                .ok_or("Update service unavailable.")?
        } else {
            state.generation
        };
        drop(state);
        let result = commit()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Update service unavailable.")?;
        state.initialized = true;
        if changed {
            state.cancel.store(true, Ordering::Release);
            state.generation = generation;
            if let Some(updater) = state.updater.as_mut() {
                let _ = updater.set_channel(channel(&next));
            }
            state.status = UpdateStatus::idle(next);
        }
        Ok(result)
    }
    pub fn check(&self) -> UpdateStatus {
        self.start(Operation::Check, None)
    }
    pub fn download(&self, request_id: &str) -> Result<UpdateStatus, String> {
        self.request(Operation::Download, request_id)
    }
    pub fn install(&self, request_id: &str) -> Result<UpdateStatus, String> {
        self.request(Operation::Install, request_id)
    }
    fn request(&self, operation: Operation, request_id: &str) -> Result<UpdateStatus, String> {
        if !valid_update_request_id(request_id) {
            return Err("Invalid update request.".into());
        }
        Ok(self.start(operation, Some(request_id)))
    }
    fn start(&self, operation: Operation, request_id: Option<&str>) -> UpdateStatus {
        let Ok(_commit) = self.commit.try_lock() else {
            return busy(&self.state.lock().unwrap_or_else(|p| p.into_inner()));
        };
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !state.initialized {
            state.status.phase = UpdatePhase::Failed;
            state.status.failure = Some(UpdateFailure::Unavailable);
            return state.status.clone();
        }
        if state.closing
            || state.status.phase == UpdatePhase::Installing
            || state.worker.as_ref().is_some_and(|w| !w.is_finished())
        {
            return busy(&state);
        }
        if let Some(worker) = state.worker.take() {
            let _ = worker.join();
        }
        if !matches!(operation, Operation::Check)
            && (state.status.offer.as_ref().map(|o| o.request_id.as_str()) != request_id
                || matches!(operation, Operation::Download)
                    && !matches!(
                        state.status.phase,
                        UpdatePhase::Available | UpdatePhase::Failed
                    )
                || matches!(operation, Operation::Install)
                    && !(state.status.phase == UpdatePhase::Ready
                        || state.status.phase == UpdatePhase::Failed
                            && matches!(
                                state.status.failure,
                                Some(UpdateFailure::ShutdownBlocked | UpdateFailure::Busy)
                            )))
        {
            let mut status = state.status.clone();
            status.failure = Some(UpdateFailure::StaleOffer);
            return status;
        }
        if matches!(operation, Operation::Check) {
            let Some(next) = state.generation.checked_add(1) else {
                state.updater = None;
                state.status = UpdateStatus::idle(state.status.channel.clone());
                state.status.phase = UpdatePhase::Failed;
                state.status.failure = Some(UpdateFailure::Unavailable);
                return state.status.clone();
            };
            state.generation = next;
            state.status = UpdateStatus::idle(state.status.channel.clone());
        }
        state.status.phase = match operation {
            Operation::Check => UpdatePhase::Checking,
            Operation::Download => UpdatePhase::Downloading,
            Operation::Install => UpdatePhase::Installing,
        };
        state.status.failure = None;
        if matches!(operation, Operation::Download) {
            state.status.downloaded_bytes = 0;
            state.status.total_bytes = None;
        }
        state.cancel = Arc::new(AtomicBool::new(false));
        let cancel = state.cancel.clone();
        let generation = state.generation;
        let selected = state.status.channel.clone();
        let mut updater = state.updater.take();
        let shared = self.state.clone();
        let installer = self
            .installer
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let factory = self.factory.clone();
        let worker = std::thread::Builder::new()
            .name("maxed-update".into())
            .spawn(move || {
                let result = (|| {
                    if updater.is_none() {
                        let mut native = factory()?;
                        native.set_channel(channel(&selected))?;
                        updater = Some(native);
                    }
                    let native = updater.as_mut().ok_or(Error::Stale)?;
                    if matches!(operation, Operation::Install) {
                        installer.ok_or(Error::Install)?(native)?;
                        // The child completed, but this process still runs the old binary.
                        return Ok(UpdatePhase::Idle);
                    }
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| Error::Network)?;
                    runtime.block_on(async {
                        match operation {
                            Operation::Check => {
                                let version = cancellable(&cancel, native.check())
                                    .await
                                    .ok_or(Error::Stale)??;
                                let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                                if state.generation != generation || cancel.load(Ordering::Acquire)
                                {
                                    return Err(Error::Stale);
                                }
                                if let Some(version) = version {
                                    state.status.offer = Some(
                                        UpdateOffer::new(
                                            generation.to_string(),
                                            version,
                                            selected.clone(),
                                            native.offer_notes().to_owned(),
                                        )
                                        .map_err(|_| Error::Manifest)?,
                                    );
                                    Ok(UpdatePhase::Available)
                                } else {
                                    Ok(UpdatePhase::UpToDate)
                                }
                            }
                            Operation::Download => {
                                cancellable(
                                    &cancel,
                                    native.download(|downloaded| {
                                        let mut state =
                                            shared.lock().unwrap_or_else(|p| p.into_inner());
                                        if state.generation == generation
                                            && !cancel.load(Ordering::Acquire)
                                        {
                                            state.status.downloaded_bytes = downloaded as u32;
                                            state.status.total_bytes = None;
                                        }
                                    }),
                                )
                                .await
                                .ok_or(Error::Stale)??;
                                Ok(UpdatePhase::Ready)
                            }
                            Operation::Install => unreachable!(),
                        }
                    })
                })();
                let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                if state.generation == generation && !cancel.load(Ordering::Acquire) {
                    match result {
                        Ok(phase) => {
                            state.status.phase = phase;
                            if matches!(operation, Operation::Install) {
                                state.status.offer = None;
                                state.status.downloaded_bytes = 0;
                                state.status.total_bytes = None;
                            }
                            if state.status.phase == UpdatePhase::Ready {
                                state.status.downloaded_bytes = updater
                                    .as_ref()
                                    .and_then(|u| u.verified.as_ref())
                                    .map_or(0, |b| b.len() as u32);
                            }
                        }
                        Err(error) => {
                            if matches!(error, Error::Install)
                                || updater.as_ref().is_none_or(|native| native.offer.is_none())
                            {
                                state.status.offer = None;
                                state.status.downloaded_bytes = 0;
                                state.status.total_bytes = None;
                            }
                            state.status.phase = UpdatePhase::Failed;
                            state.status.failure = Some(failed(error));
                        }
                    }
                } else if let Some(native) = updater.as_mut() {
                    let _ = native.set_channel(channel(&state.status.channel));
                }
                state.updater = updater;
            });
        match worker {
            Ok(worker) => state.worker = Some(worker),
            Err(_) => {
                state.status.phase = UpdatePhase::Failed;
                state.status.failure = Some(UpdateFailure::Unavailable);
            }
        }
        state.status.clone()
    }
    pub fn cancel(&self) -> UpdateStatus {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.status.phase == UpdatePhase::Installing {
            return busy(&state);
        }
        state.cancel.store(true, Ordering::Release);
        state.status = UpdateStatus::idle(state.status.channel.clone());
        let selected = channel(&state.status.channel);
        if let Some(updater) = state.updater.as_mut() {
            let _ = updater.set_channel(selected);
        }
        state.status.clone()
    }
    /// Exit cancels networking and joins cleanup. Installer handoff cannot be cancelled.
    pub fn shutdown(&self) -> Result<(), String> {
        let worker = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "Update service unavailable.")?;
            if state.status.phase == UpdatePhase::Installing {
                return Err("Update operation is busy.".into());
            }
            state.closing = true;
            state.cancel.store(true, Ordering::Release);
            state.updater = None;
            state.worker.take()
        };
        if let Some(worker) = worker {
            worker.join().map_err(|_| "Update worker failed.")?;
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Update service unavailable.")?;
        state.updater = None;
        state.status = UpdateStatus::idle(state.status.channel.clone());
        Ok(())
    }
    pub fn resume(&self) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).closing = false;
    }
}
impl Default for UpdateService {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for UpdateService {
    fn drop(&mut self) {
        if self.shutdown().is_err() {
            let worker = self
                .state
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .worker
                .take();
            if let Some(worker) = worker {
                let _ = worker.join();
            }
        }
    }
}

async fn cancellable<T>(cancel: &AtomicBool, future: impl Future<Output = T>) -> Option<T> {
    let mut future = std::pin::pin!(future);
    let mut tick = Box::pin(tokio::time::sleep(Duration::from_millis(25)));
    std::future::poll_fn(|cx| {
        if cancel.load(Ordering::Acquire) {
            return Poll::Ready(None);
        }
        if let Poll::Ready(value) = future.as_mut().poll(cx) {
            return Poll::Ready(Some(value));
        }
        if tick.as_mut().poll(cx).is_ready() {
            tick.as_mut()
                .reset(tokio::time::Instant::now() + Duration::from_millis(25));
            let _ = tick.as_mut().poll(cx);
        }
        Poll::Pending
    })
    .await
}

#[cfg(test)]
#[path = "../../tests/support/update_service.rs"]
mod tests;
