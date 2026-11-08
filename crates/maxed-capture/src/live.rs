use crate::{ffi, parse_frame, send_latest, CaptureError};
use crossbeam_channel::{bounded, Receiver};
use maxed_types::{CaptureState, CaptureStatus, RawFrame};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, MutexGuard,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct CaptureConfig {
    pub adapter_id: Option<String>,
    pub capacity: usize,
}
impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            adapter_id: None,
            capacity: 4096,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdapterInfo {
    pub id: String,
    pub description: String,
}

pub trait Device {
    fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError>;
    /// Cumulative kernel-buffer drops for this handle; unavailable backends return None.
    fn dropped_packets(&mut self) -> Result<Option<u32>, CaptureError> {
        Ok(None)
    }
}
pub trait Backend {
    fn discover(&mut self) -> Result<Vec<AdapterInfo>, CaptureError>;
    fn open(&mut self, adapter: &AdapterInfo) -> Result<Box<dyn Device>, CaptureError>;
}

fn lock(status: &Mutex<CaptureStatus>) -> MutexGuard<'_, CaptureStatus> {
    status.lock().unwrap_or_else(|poison| poison.into_inner())
}
fn set_error(status: &Mutex<CaptureStatus>, error: CaptureError) {
    let mut status = lock(status);
    status.state = match error {
        CaptureError::Missing => CaptureState::NpcapMissing,
        CaptureError::Incompatible => CaptureState::NpcapIncompatible,
        CaptureError::PermissionDenied => CaptureState::PermissionDenied,
        CaptureError::Stopped => CaptureState::Stopped,
        _ => CaptureState::Failed,
    };
    status.message = error.to_string();
}

fn record_loss(status: &Mutex<CaptureStatus>, drops: u32) {
    if drops > 0 {
        let mut status = lock(status);
        status.dropped_frames = status.dropped_frames.saturating_add(drops);
        status.state = CaptureState::Recovering;
        status.message = "Capture frames lost; waiting for decoder recovery".into();
    }
}

fn read_drops(device: &mut dyn Device) -> Option<u32> {
    match device.dropped_packets() {
        Ok(drops) => drops,
        Err(error) => {
            // CaptureError has fixed descriptions; never log native text or adapter IDs.
            tracing::warn!(error = %error, "Capture drop statistics unavailable");
            None
        }
    }
}

pub struct CaptureHandle {
    pub frames: Receiver<RawFrame>,
    status: Arc<Mutex<CaptureStatus>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), CaptureError>>>,
    readiness: Arc<Mutex<Option<Result<(), CaptureError>>>>,
}
impl CaptureHandle {
    /// Initialization evidence, independent of traffic or decoder state.
    /// A successful result remains successful after the capture is stopped.
    pub fn readiness(&self) -> Option<Result<(), CaptureError>> {
        *self.readiness.lock().unwrap_or_else(|p| p.into_inner())
    }
    pub fn status(&self) -> CaptureStatus {
        lock(&self.status).clone()
    }
    pub fn report_decode(&self, live: bool, unknown: u32, failures: u32) {
        let mut status = lock(&self.status);
        status.unknown_packets = status.unknown_packets.saturating_add(unknown);
        status.decode_failures = status.decode_failures.saturating_add(failures);
        if matches!(
            status.state,
            CaptureState::WaitingForGame | CaptureState::Recovering | CaptureState::Live
        ) {
            status.state = if live {
                CaptureState::Live
            } else {
                CaptureState::Recovering
            };
            status.message = if live {
                "Game traffic decoded"
            } else {
                "Waiting for decoder recovery"
            }
            .into();
        }
    }
    pub fn stop(mut self) -> Result<(), CaptureError> {
        self.join()
    }
    fn join(&mut self) -> Result<(), CaptureError> {
        self.stop.store(true, Ordering::Release);
        self.readiness
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_or_insert(Err(CaptureError::Stopped));
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            worker.join().map_err(|_| CaptureError::OpenFailed)??;
        }
        Ok(())
    }
}
impl Drop for CaptureHandle {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

pub fn start_capture(config: CaptureConfig) -> Result<CaptureHandle, CaptureError> {
    start_with_backend(config, ffi::NativeBackend::load)
}
pub fn list_adapters() -> Result<Vec<AdapterInfo>, CaptureError> {
    thread::Builder::new()
        .name("maxed-adapters".into())
        .spawn(|| ffi::NativeBackend::load()?.discover())
        .map_err(|_| CaptureError::OpenFailed)?
        .join()
        .map_err(|_| CaptureError::OpenFailed)?
}

/// The factory is sent to the thread; native libraries and handles never are.
pub fn start_with_backend<B, F>(
    config: CaptureConfig,
    factory: F,
) -> Result<CaptureHandle, CaptureError>
where
    B: Backend + 'static,
    F: FnOnce() -> Result<B, CaptureError> + Send + 'static,
{
    if config.capacity == 0
        || config
            .adapter_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.contains('\0'))
    {
        return Err(CaptureError::InvalidConfig);
    }
    let (tx, frames) = bounded(config.capacity);
    let eviction = frames.clone();
    let status = Arc::new(Mutex::new(CaptureStatus {
        state: CaptureState::WaitingForGame,
        message: "Waiting for game traffic".into(),
        ..Default::default()
    }));
    let stop = Arc::new(AtomicBool::new(false));
    let worker_status = status.clone();
    let worker_stop = stop.clone();
    let readiness = Arc::new(Mutex::new(None));
    let worker_readiness = readiness.clone();
    let worker = thread::Builder::new()
        .name("maxed-capture".into())
        .spawn(move || {
            // Also resolve pending initialization if an injected backend panics.
            struct Pending(Arc<Mutex<Option<Result<(), CaptureError>>>>);
            impl Drop for Pending {
                fn drop(&mut self) {
                    self.0.lock().unwrap_or_else(|p| p.into_inner())
                        .get_or_insert(Err(CaptureError::OpenFailed));
                }
            }
            let _pending = Pending(worker_readiness.clone());
            let result = (|| {
                let mut backend = factory()?;
                if worker_stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                let adapters = backend.discover()?;
                let mut devices = Vec::new();
                let mut errors = Vec::new();
                for adapter in adapters {
                    if worker_stop.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    if config
                        .adapter_id
                        .as_ref()
                        .is_some_and(|id| id != &adapter.id)
                    {
                        continue;
                    }
                    match backend.open(&adapter) {
                        Ok(device) => devices.push(device),
                        Err(error) => errors.push(error),
                    }
                }
                if devices.is_empty() {
                    return Err(if errors.is_empty() {
                        CaptureError::NoAdapters
                    } else if errors.iter().all(|e| *e == CaptureError::PermissionDenied) {
                        CaptureError::PermissionDenied
                    } else {
                        errors
                            .into_iter()
                            .find(|e| *e != CaptureError::PermissionDenied)
                            .unwrap_or(CaptureError::OpenFailed)
                    });
                }
                worker_readiness.lock().unwrap_or_else(|p| p.into_inner())
                    .get_or_insert(if worker_stop.load(Ordering::Acquire) {
                        Err(CaptureError::Stopped)
                    } else {
                        Ok(())
                    });
                let mut selected = false;
                let mut index = 0u64;
                let mut previous_drops = None;
                let mut stats_at = Instant::now();
                while !worker_stop.load(Ordering::Acquire) {
                    let mut winner = None;
                    let mut i = 0;
                    while i < devices.len() {
                        if worker_stop.load(Ordering::Acquire) {
                            break;
                        }
                        let polled = match devices[i].poll() {
                            Ok(frame) => frame,
                            Err(error) if !selected => {
                                // CaptureError has fixed descriptions; no native error text or adapter IDs.
                                tracing::warn!(error = %error, "Discarding capture candidate after polling failure");
                                errors.push(error);
                                devices.remove(i);
                                if devices.is_empty() {
                                    return Err(error);
                                }
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        // Check on both busy and idle polls, bounded to four calls per second.
                        if selected && stats_at.elapsed() >= Duration::from_millis(250) {
                            stats_at = Instant::now();
                            if let Some(current) = read_drops(devices[i].as_mut()) {
                                if let Some(previous) = previous_drops {
                                    let delta = current.wrapping_sub(previous);
                                    index = index.checked_add(u64::from(delta))
                                        .ok_or(CaptureError::OpenFailed)?;
                                    record_loss(&worker_status, delta);
                                }
                                previous_drops = Some(current);
                            }
                        }
                        if let Some(mut frame) = polled {
                            // BPF selects traffic; parser confirms the adapter before locking in.
                            if !selected && !matches!(parse_frame(&frame), Ok(Some(_))) {
                                i += 1;
                                continue;
                            }
                            frame.index = index;
                            index = index.checked_add(1).ok_or(CaptureError::OpenFailed)?;
                            let drops = send_latest(&tx, &eviction, frame)
                                .map_err(|_| CaptureError::Stopped)?;
                            record_loss(&worker_status, drops);
                            winner = Some(i);
                            break;
                        }
                        i += 1;
                    }
                    if let Some(winner) = winner {
                        if !selected {
                            let mut device = devices.swap_remove(winner);
                            // Ignore all loss before selection. If unavailable, the first
                            // successful sample becomes the baseline rather than guessing.
                            previous_drops = read_drops(device.as_mut());
                            stats_at = Instant::now();
                            devices.clear();
                            devices.push(device);
                            selected = true;
                        }
                    } else {
                        thread::park_timeout(Duration::from_millis(5));
                    }
                }
                Ok(())
            })();
            worker_readiness.lock().unwrap_or_else(|p| p.into_inner())
                .get_or_insert(Err(result.err().unwrap_or(CaptureError::Stopped)));
            match result {
                Ok(()) => set_error(&worker_status, CaptureError::Stopped),
                Err(error) => set_error(&worker_status, error),
            }
            result
        })
        .map_err(|_| CaptureError::OpenFailed)?;
    Ok(CaptureHandle {
        frames,
        status,
        stop,
        worker: Some(worker),
        readiness,
    })
}

#[cfg(test)]
mod readiness_tests {
    use super::*;
    struct Quiet;
    impl Backend for Quiet {
        fn discover(&mut self) -> Result<Vec<AdapterInfo>, CaptureError> {
            Ok(vec![])
        }
        fn open(&mut self, _: &AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
            unreachable!()
        }
    }
    #[test]
    fn cancellation_before_backend_initialization_resolves_pending_readiness() {
        let (entered, waiting) = bounded(1);
        let (release, gate) = bounded(1);
        let mut handle = start_with_backend(CaptureConfig::default(), move || {
            entered.send(()).unwrap();
            gate.recv().unwrap();
            Ok(Quiet)
        })
        .unwrap();
        waiting.recv().unwrap();
        assert_eq!(handle.readiness(), None);
        handle.stop.store(true, Ordering::Release);
        release.send(()).unwrap();
        handle.worker.take().unwrap().join().unwrap().unwrap();
        assert_eq!(handle.readiness(), Some(Err(CaptureError::Stopped)));
    }
    #[test]
    fn backend_panic_resolves_pending_readiness() {
        let mut handle = start_with_backend::<Quiet, _>(CaptureConfig::default(), || {
            panic!("synthetic failure")
        })
        .unwrap();
        assert!(handle.worker.take().unwrap().join().is_err());
        assert_eq!(handle.readiness(), Some(Err(CaptureError::OpenFailed)));
    }
}
