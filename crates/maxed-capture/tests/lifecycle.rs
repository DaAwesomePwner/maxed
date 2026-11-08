use maxed_capture::send_latest;

#[test]
fn full_queue_keeps_newest_and_counts_loss() {
    let (tx, rx) = crossbeam_channel::bounded(1);
    tx.send(1).unwrap();
    assert_eq!(send_latest(&tx, &rx, 2), Ok(1));
    assert_eq!(rx.recv().unwrap(), 2);
}

use maxed_capture::{
    backend::{start_with_backend, Backend, Device},
    AdapterInfo, CaptureConfig, CaptureError,
};
use maxed_types::{CaptureState, RawFrame, Timestamp};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

struct FakeDevice {
    reads: VecDeque<Result<Option<RawFrame>, CaptureError>>,
    closes: Arc<AtomicUsize>,
}
impl Device for FakeDevice {
    fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError> {
        self.reads.pop_front().unwrap_or(Ok(None))
    }
}
impl Drop for FakeDevice {
    fn drop(&mut self) {
        self.closes.fetch_add(1, Ordering::SeqCst);
    }
}
struct FakeBackend {
    devices: VecDeque<Result<FakeDevice, CaptureError>>,
    discovery: Option<crossbeam_channel::Receiver<()>>,
}
impl Backend for FakeBackend {
    fn discover(&mut self) -> Result<Vec<AdapterInfo>, CaptureError> {
        if let Some(gate) = &self.discovery {
            gate.recv().unwrap();
        }
        Ok((0..self.devices.len())
            .map(|i| AdapterInfo {
                id: i.to_string(),
                description: "Synthetic adapter".into(),
            })
            .collect())
    }
    fn open(&mut self, _: &AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
        self.devices
            .pop_front()
            .unwrap()
            .map(|d| Box::new(d) as Box<dyn Device>)
    }
}
fn frame() -> RawFrame {
    let mut bytes = vec![0; 54];
    bytes[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    bytes[14] = 0x45;
    bytes[16..18].copy_from_slice(&40u16.to_be_bytes());
    bytes[23] = 6;
    bytes[34..38].copy_from_slice(&[0xc3, 0x50, 8, 2]);
    bytes[46] = 0x50;
    bytes[47] = 2;
    RawFrame {
        index: 999,
        at: Timestamp(123),
        original_len: 54,
        bytes,
    }
}
fn device(
    closes: &Arc<AtomicUsize>,
    reads: Vec<Result<Option<RawFrame>, CaptureError>>,
) -> Result<FakeDevice, CaptureError> {
    Ok(FakeDevice {
        reads: reads.into(),
        closes: closes.clone(),
    })
}
fn backend(devices: Vec<Result<FakeDevice, CaptureError>>) -> FakeBackend {
    FakeBackend {
        devices: devices.into(),
        discovery: None,
    }
}
fn wait_until(mut condition: impl FnMut() -> bool) {
    let limit = Instant::now() + Duration::from_secs(2);
    while !condition() {
        assert!(
            Instant::now() < limit,
            "worker did not reach expected state"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn readiness_is_pending_until_an_eligible_device_opens_without_traffic() {
    let closes = Arc::new(AtomicUsize::new(0));
    let (release, gate) = crossbeam_channel::bounded(1);
    let mut fake = backend(vec![device(&closes, vec![])]);
    fake.discovery = Some(gate);
    let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
    assert_eq!(handle.readiness(), None);
    assert_eq!(handle.status().state, CaptureState::WaitingForGame);
    release.send(()).unwrap();
    wait_until(|| handle.readiness().is_some());
    assert_eq!(handle.readiness(), Some(Ok(())));
    assert_eq!(handle.status().state, CaptureState::WaitingForGame);
    handle.stop().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}

#[test]
fn readiness_reports_initialization_failure_without_requiring_a_frame() {
    for error in [
        CaptureError::Missing,
        CaptureError::Incompatible,
        CaptureError::PermissionDenied,
        CaptureError::OpenFailed,
    ] {
        let fake = backend(vec![Err(error)]);
        let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
        wait_until(|| handle.readiness().is_some());
        assert_eq!(handle.readiness(), Some(Err(error)));
        let _ = handle.stop();
    }
    let handle = start_with_backend(CaptureConfig::default(), || Ok(backend(vec![]))).unwrap();
    wait_until(|| handle.readiness().is_some());
    assert_eq!(handle.readiness(), Some(Err(CaptureError::NoAdapters)));
}

#[test]
fn readiness_requires_the_explicit_adapter_to_exist() {
    let closes = Arc::new(AtomicUsize::new(0));
    let fake = backend(vec![device(&closes, vec![])]);
    let config = CaptureConfig {
        adapter_id: Some("removed".into()),
        ..Default::default()
    };
    let handle = start_with_backend(config, || Ok(fake)).unwrap();
    wait_until(|| handle.readiness().is_some());
    assert_eq!(handle.readiness(), Some(Err(CaptureError::NoAdapters)));
}

#[test]
fn second_adapter_wins_and_live_requires_decode() {
    let closes = Arc::new(AtomicUsize::new(0));
    let fake = backend(vec![
        device(&closes, vec![Ok(None)]),
        device(&closes, vec![Ok(None), Ok(Some(frame()))]),
    ]);
    let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
    let received = handle.frames.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(received.index, 0);
    assert_eq!(received.at, Timestamp(123));
    assert_eq!(handle.status().state, CaptureState::WaitingForGame);
    assert_eq!(closes.load(Ordering::SeqCst), 0);
    handle.report_decode(true, u32::MAX, u32::MAX);
    handle.report_decode(true, 1, 1);
    assert_eq!(handle.status().state, CaptureState::Live);
    assert_eq!(handle.status().unknown_packets, u32::MAX);
    assert_eq!(handle.status().decode_failures, u32::MAX);
    handle.report_decode(false, 0, 0);
    assert_eq!(handle.status().state, CaptureState::Recovering);
    handle.report_decode(true, 0, 0);
    assert_eq!(handle.status().state, CaptureState::Live);
    handle.stop().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 2);
}

#[test]
fn failed_poll_during_discovery_does_not_discard_working_adapter() {
    let closes = Arc::new(AtomicUsize::new(0));
    let fake = backend(vec![
        device(&closes, vec![Err(CaptureError::OpenFailed)]),
        device(&closes, vec![Ok(None), Ok(Some(frame()))]),
    ]);
    let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
    assert_eq!(
        handle
            .frames
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .index,
        0
    );
    wait_until(|| closes.load(Ordering::SeqCst) == 1);
    assert_eq!(handle.status().state, CaptureState::WaitingForGame);
    handle.stop().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 2);
}

#[test]
fn selected_adapter_poll_error_is_terminal() {
    let closes = Arc::new(AtomicUsize::new(0));
    let fake = backend(vec![device(
        &closes,
        vec![Ok(Some(frame())), Err(CaptureError::OpenFailed)],
    )]);
    let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
    handle.frames.recv_timeout(Duration::from_secs(2)).unwrap();
    wait_until(|| handle.status().state == CaptureState::Failed);
    assert_eq!(handle.stop(), Err(CaptureError::OpenFailed));
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}

#[test]
fn failures_are_actionable_and_retry_uses_new_resources() {
    for (errors, expected) in [
        (vec![], CaptureState::Failed),
        (
            vec![
                CaptureError::PermissionDenied,
                CaptureError::PermissionDenied,
            ],
            CaptureState::PermissionDenied,
        ),
        (
            vec![CaptureError::Incompatible],
            CaptureState::NpcapIncompatible,
        ),
    ] {
        let fake = backend(errors.into_iter().map(Err).collect());
        let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
        wait_until(|| handle.status().state != CaptureState::WaitingForGame);
        assert_eq!(handle.status().state, expected);
        assert!(handle.stop().is_err());
    }
    let closes = Arc::new(AtomicUsize::new(0));
    let fake = backend(vec![device(&closes, vec![Err(CaptureError::OpenFailed)])]);
    let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
    wait_until(|| handle.status().state == CaptureState::Failed);
    handle.report_decode(true, 0, 0);
    assert_eq!(handle.status().state, CaptureState::Failed);
    assert_eq!(handle.stop(), Err(CaptureError::OpenFailed));
    for _ in 0..3 {
        let fake = backend(vec![
            Err(CaptureError::OpenFailed),
            device(&closes, vec![Ok(Some(frame()))]),
        ]);
        let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
        assert_eq!(
            handle
                .frames
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .index,
            0
        );
        handle.stop().unwrap();
    }
    assert_eq!(closes.load(Ordering::SeqCst), 4);
}

#[test]
fn loss_marks_recovery_and_newest_index_exposes_gap() {
    let closes = Arc::new(AtomicUsize::new(0));
    let fake = backend(vec![device(
        &closes,
        vec![Ok(Some(frame())), Ok(Some(frame())), Ok(Some(frame()))],
    )]);
    let handle = start_with_backend(
        CaptureConfig {
            capacity: 1,
            adapter_id: None,
        },
        || Ok(fake),
    )
    .unwrap();
    wait_until(|| handle.status().dropped_frames == 2);
    assert_eq!(handle.status().state, CaptureState::Recovering);
    assert_eq!(handle.frames.recv().unwrap().index, 2);
    handle.stop().unwrap();
}

#[test]
fn stop_during_discovery_and_quiet_capture_closes_once() {
    let closes = Arc::new(AtomicUsize::new(0));
    let (release, gate) = crossbeam_channel::bounded(1);
    let fake = FakeBackend {
        devices: vec![device(&closes, vec![])].into(),
        discovery: Some(gate),
    };
    let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
    let join = std::thread::spawn(move || handle.stop());
    let _ = release.send(());
    join.join().unwrap().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
    let fake = backend(vec![device(&closes, vec![])]);
    let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
    handle.stop().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 2);
}

#[test]
fn zero_capacity_is_rejected_before_starting_backend() {
    let result = start_with_backend(
        CaptureConfig {
            capacity: 0,
            adapter_id: None,
        },
        || -> Result<FakeBackend, CaptureError> { panic!("must not run") },
    );
    assert!(matches!(result, Err(CaptureError::InvalidConfig)));
}

#[test]
fn override_opens_only_selected_adapter() {
    struct OverrideBackend {
        opened: crossbeam_channel::Sender<String>,
        device: Option<FakeDevice>,
    }
    impl Backend for OverrideBackend {
        fn discover(&mut self) -> Result<Vec<AdapterInfo>, CaptureError> {
            Ok(["first", "chosen"]
                .into_iter()
                .map(|id| AdapterInfo {
                    id: id.into(),
                    description: "Synthetic".into(),
                })
                .collect())
        }
        fn open(&mut self, adapter: &AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
            self.opened.send(adapter.id.clone()).unwrap();
            Ok(Box::new(self.device.take().unwrap()))
        }
    }
    let closes = Arc::new(AtomicUsize::new(0));
    let (opened, received) = crossbeam_channel::unbounded();
    let fake = OverrideBackend {
        opened,
        device: Some(device(&closes, vec![Ok(Some(frame()))]).unwrap()),
    };
    let handle = start_with_backend(
        CaptureConfig {
            adapter_id: Some("chosen".into()),
            capacity: 1,
        },
        || Ok(fake),
    )
    .unwrap();
    handle.frames.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(received.try_iter().collect::<Vec<_>>(), ["chosen"]);
    handle.stop().unwrap();
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}

#[test]
fn loader_failures_map_to_safe_statuses() {
    for (error, state) in [
        (CaptureError::Missing, CaptureState::NpcapMissing),
        (CaptureError::Incompatible, CaptureState::NpcapIncompatible),
        (
            CaptureError::PermissionDenied,
            CaptureState::PermissionDenied,
        ),
        (CaptureError::UnsupportedPlatform, CaptureState::Failed),
    ] {
        let handle = start_with_backend(
            CaptureConfig::default(),
            move || -> Result<FakeBackend, CaptureError> { Err(error) },
        )
        .unwrap();
        wait_until(|| handle.status().state != CaptureState::WaitingForGame);
        assert_eq!(handle.status().state, state);
        assert_eq!(handle.status().message, error.to_string());
        assert_eq!(handle.stop(), Err(error));
    }
}
