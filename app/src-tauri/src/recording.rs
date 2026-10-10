//! Native recording worker boundary. All disk operations stay on its joinable worker.
use maxed_types::{RawFrame, RecordingFailure, RecordingState, RecordingStatus};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

#[derive(Debug, thiserror::Error)]
pub enum RecordingError {
    #[error("Recording storage failed.")]
    Pcap(#[from] maxed_recording::RecordingError),
    #[error("Recording worker failed.")]
    Worker,
    #[error("Recording owner failed.")]
    Owner(#[from] maxed_recording::RecordingFailure),
}
#[derive(Debug, PartialEq, Eq)]
pub enum RecordAdmission {
    Accepted,
    Disabled,
    Failed(RecordingFailure),
}

fn failure(value: maxed_recording::RecordingFailure) -> RecordingFailure {
    use maxed_recording::RecordingFailure as F;
    match value {
        F::Lease => RecordingFailure::InUse,
        F::LimitTooSmall | F::Inventory => RecordingFailure::Quota,
        F::QueueFull => RecordingFailure::QueueFull,
        F::InvalidFrame => RecordingFailure::InvalidFrame,
        _ => RecordingFailure::Disk,
    }
}

pub struct RecordingOwner {
    input: Option<maxed_recording::RecordingInput>,
    status: Arc<Mutex<RecordingStatus>>,
    monitor: maxed_recording::RecordingMonitor,
    worker: Option<JoinHandle<Result<(), RecordingError>>>,
}
impl RecordingOwner {
    /// Only the native transition worker calls open, with resolved app-data.
    /// Successful return acknowledges namespace and ownership validation.
    pub fn open(root: PathBuf, limit_bytes: u32) -> Result<Self, RecordingError> {
        Self::open_with_disk(
            root,
            limit_bytes,
            Arc::new(maxed_recording::SystemRecordingDisk),
        )
    }
    #[doc(hidden)]
    pub fn open_with_disk(
        root: PathBuf,
        limit_bytes: u32,
        disk: Arc<dyn maxed_recording::RecordingDisk>,
    ) -> Result<Self, RecordingError> {
        let (input, pending) = maxed_recording::recording_queue();
        let status = Arc::new(Mutex::new(RecordingStatus {
            state: RecordingState::Armed,
            limit_bytes,
            retained_bytes: 0,
            dropped_frames: 0,
            failure: None,
        }));
        let shared = status.clone();
        let (ready, wait) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("maxed-recording".into())
            .spawn(move || {
                let result = (|| {
                    let mut owner = match maxed_recording::RecordingOwner::open_with_disk(
                        root,
                        limit_bytes.into(),
                        pending,
                        disk,
                    ) {
                        Ok(owner) => owner,
                        Err(error) => {
                            let _ = ready.send(Err(error));
                            return Err(error);
                        }
                    };
                    shared
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .retained_bytes = owner.accounted_bytes() as u32;
                    let _ = ready.send(Ok(()));
                    while owner.process_next()? {
                        let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                        state.state = RecordingState::Writing;
                        state.retained_bytes = owner.accounted_bytes() as u32;
                    }
                    owner.finish()
                })();
                let mut state = shared.lock().unwrap_or_else(|p| p.into_inner());
                match result {
                    Ok(()) => state.state = RecordingState::Disabled,
                    Err(error) => {
                        state.state = RecordingState::Failed;
                        state.failure = Some(failure(error));
                    }
                }
                result.map_err(RecordingError::from)
            })
            .map_err(|_| RecordingError::Worker)?;
        match wait.recv() {
            Ok(Ok(())) => Ok(Self {
                monitor: input.monitor(),
                input: Some(input),
                status,
                worker: Some(worker),
            }),
            result => {
                drop(input);
                let _ = worker.join();
                Err(match result {
                    Ok(Err(error)) => error.into(),
                    _ => RecordingError::Worker,
                })
            }
        }
    }
    pub fn try_record(&self, frame: &RawFrame) -> RecordAdmission {
        match &self.input {
            Some(input) => match input.try_record(frame) {
                Ok(()) => RecordAdmission::Accepted,
                Err(error) => RecordAdmission::Failed(failure(error)),
            },
            None => RecordAdmission::Disabled,
        }
    }
    pub fn status(&self) -> RecordingStatus {
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        status.dropped_frames = self.monitor.dropped_frames();
        if let maxed_recording::RecordingStatus::Failed(error) = self.monitor.status() {
            status.state = RecordingState::Failed;
            status.failure = Some(failure(error));
        }
        status
    }
    pub fn finish(mut self) -> Result<(), RecordingError> {
        self.join()
    }
    fn join(&mut self) -> Result<(), RecordingError> {
        self.input.take();
        self.worker
            .take()
            .map_or(Ok(()), |w| w.join().map_err(|_| RecordingError::Worker)?)
    }
}
impl Drop for RecordingOwner {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

/// Native setup admits opt-in sinks; the capture runtime owns retirement.
#[derive(Default)]
pub struct RecordingRuntime {
    transition: Mutex<()>,
    slot: Mutex<RecordingSlot>,
}
#[derive(Default)]
struct RecordingSlot {
    owner: Option<RecordingOwner>,
    last: Option<RecordingStatus>,
    dropped_base: u64,
    live: bool,
    swapping: bool,
}
impl RecordingRuntime {
    fn attach(&self, owner: RecordingOwner) {
        let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        slot.owner = Some(owner);
        slot.last = None;
        slot.swapping = false;
    }

    /// Called only by the serialized native transition owner, never the capture
    /// loop. An idle opt-in validates storage but leaves no background worker.
    pub fn configure(
        &self,
        root: PathBuf,
        enabled: bool,
        limit_bytes: u32,
        live: bool,
    ) -> Result<(), RecordingError> {
        self.configure_with_disk(
            root,
            enabled,
            limit_bytes,
            live,
            Arc::new(maxed_recording::SystemRecordingDisk),
        )
    }
    #[doc(hidden)]
    pub fn configure_with_disk(
        &self,
        root: PathBuf,
        enabled: bool,
        limit_bytes: u32,
        live: bool,
        disk: Arc<dyn maxed_recording::RecordingDisk>,
    ) -> Result<(), RecordingError> {
        let _transition = self.transition.lock().unwrap_or_else(|p| p.into_inner());
        let swapping = {
            let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
            let swapping = slot.live && enabled && live;
            slot.live = enabled && live;
            swapping
        };
        self.retire_inner(swapping);
        if !swapping {
            let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
            slot.dropped_base = 0;
            slot.last = None;
        }
        if !enabled {
            return Ok(());
        }
        let owner = match RecordingOwner::open_with_disk(root, limit_bytes, disk) {
            Ok(owner) => owner,
            Err(error) => {
                let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
                slot.swapping = false;
                slot.last = Some(RecordingStatus {
                    state: RecordingState::Failed,
                    limit_bytes,
                    retained_bytes: 0,
                    dropped_frames: 0,
                    failure: Some(match &error {
                        RecordingError::Owner(e) => failure(*e),
                        _ => RecordingFailure::Disk,
                    }),
                });
                return Err(error);
            }
        };
        if live {
            self.attach(owner);
        } else {
            let mut status = owner.status();
            owner.finish()?;
            status.state = RecordingState::Armed;
            self.slot.lock().unwrap_or_else(|p| p.into_inner()).last = Some(status);
        }
        Ok(())
    }
    pub fn try_record(&self, frame: &RawFrame) -> RecordAdmission {
        let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(owner) = &slot.owner {
            return owner.try_record(frame);
        }
        if slot.swapping {
            slot.dropped_base = slot.dropped_base.saturating_add(1);
        }
        RecordAdmission::Disabled
    }
    pub fn status(&self, limit_bytes: u32) -> RecordingStatus {
        let slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        let mut status = slot
            .owner
            .as_ref()
            .map(RecordingOwner::status)
            .or_else(|| slot.last.clone())
            .unwrap_or(RecordingStatus {
                state: RecordingState::Disabled,
                limit_bytes,
                retained_bytes: 0,
                dropped_frames: 0,
                failure: None,
            });
        status.dropped_frames = status.dropped_frames.saturating_add(slot.dropped_base);
        if slot.owner.is_none() {
            status.limit_bytes = limit_bytes;
        }
        if slot.swapping {
            status.state = RecordingState::Armed;
            status.failure = None;
        }
        status
    }
    /// Detach admission before joining. Join failures affect recording only.
    pub fn retire(&self) {
        let _transition = self.transition.lock().unwrap_or_else(|p| p.into_inner());
        self.slot.lock().unwrap_or_else(|p| p.into_inner()).live = false;
        self.retire_inner(false);
    }

    fn retire_inner(&self, swapping: bool) {
        // Move the old counter into the base atomically with detaching admission.
        // Neither status reads nor swap-gap admissions wait for the disk join.
        let owner = {
            let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
            slot.swapping = swapping;
            let owner = slot.owner.take();
            if let Some(owner) = &owner {
                let mut status = owner.status();
                slot.dropped_base = slot.dropped_base.saturating_add(status.dropped_frames);
                status.dropped_frames = 0;
                slot.last = Some(status);
            }
            owner
        };
        if let Some(mut owner) = owner {
            let result = owner.join();
            let mut status = owner.status();
            status.dropped_frames = 0; // Already included in the runtime base.
            if result.is_err() && status.failure.is_none() {
                status.state = RecordingState::Failed;
                status.failure = Some(RecordingFailure::Disk);
            }
            self.slot.lock().unwrap_or_else(|p| p.into_inner()).last = Some(status);
        }
    }
}
