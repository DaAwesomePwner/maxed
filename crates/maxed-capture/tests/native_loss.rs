use crossbeam_channel::{unbounded, Receiver};
use maxed_capture::{
    backend::{start_with_backend, Backend, Device},
    AdapterInfo, CaptureConfig, CaptureError,
};
use maxed_types::{CaptureState, RawFrame, Timestamp};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

struct StatsDevice {
    frames: Receiver<RawFrame>,
    drops: Arc<AtomicU32>,
    fail: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    failures: Arc<AtomicUsize>,
}
impl Device for StatsDevice {
    fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError> {
        Ok(self.frames.try_recv().ok())
    }
    fn dropped_packets(&mut self) -> Result<Option<u32>, CaptureError> {
        let result = if self.fail.load(Ordering::SeqCst) {
            self.failures.fetch_add(1, Ordering::SeqCst);
            Err(CaptureError::OpenFailed)
        } else {
            Ok(Some(self.drops.load(Ordering::SeqCst)))
        };
        self.calls.fetch_add(1, Ordering::SeqCst);
        result
    }
}
struct FakeBackend(Vec<StatsDevice>);
impl Backend for FakeBackend {
    fn discover(&mut self) -> Result<Vec<AdapterInfo>, CaptureError> {
        Ok((0..self.0.len())
            .map(|i| AdapterInfo {
                id: i.to_string(),
                description: "Synthetic".into(),
            })
            .collect())
    }
    fn open(&mut self, _: &AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
        Ok(Box::new(self.0.remove(0)))
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

fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "capture did not report native loss"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn exercise(initial: u32, next: u32, expected: u64, failed_poll: bool) {
    let (tx, frames) = unbounded();
    let drops = Arc::new(AtomicU32::new(initial));
    let fail = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(AtomicUsize::new(0));
    let device = StatsDevice {
        frames,
        drops: drops.clone(),
        fail: fail.clone(),
        calls: calls.clone(),
        failures: failures.clone(),
    };
    let handle =
        start_with_backend(CaptureConfig::default(), || Ok(FakeBackend(vec![device]))).unwrap();
    tx.send(frame()).unwrap();
    assert_eq!(
        handle
            .frames
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .index,
        0
    );
    wait(|| calls.load(Ordering::SeqCst) >= 1);
    assert_eq!(
        handle.status().dropped_frames,
        0,
        "pre-selection loss must be ignored"
    );
    if failed_poll {
        fail.store(true, Ordering::SeqCst);
        wait(|| failures.load(Ordering::SeqCst) >= 1);
        // Change the counter only after the worker has entered the failure window.
        drops.store(next, Ordering::SeqCst);
        let before = failures.load(Ordering::SeqCst);
        wait(|| failures.load(Ordering::SeqCst) > before);
        assert_eq!(handle.status().dropped_frames, 0);
        assert_eq!(handle.status().state, CaptureState::WaitingForGame);
        fail.store(false, Ordering::SeqCst);
    } else {
        drops.store(next, Ordering::SeqCst);
    }
    let before = calls.load(Ordering::SeqCst);
    wait(|| calls.load(Ordering::SeqCst) > before);
    wait(|| u64::from(handle.status().dropped_frames) == expected);
    assert_eq!(
        handle.status().state,
        if expected == 0 {
            CaptureState::WaitingForGame
        } else {
            CaptureState::Recovering
        }
    );
    let before = calls.load(Ordering::SeqCst);
    wait(|| calls.load(Ordering::SeqCst) > before + 1);
    assert_eq!(u64::from(handle.status().dropped_frames), expected);
    tx.send(frame()).unwrap();
    assert_eq!(
        handle
            .frames
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .index,
        1 + expected
    );
    handle.stop().unwrap();
}
#[test]
fn native_tail_loss_updates_status_and_skips_next_frame() {
    exercise(17, 20, 3, false);
}
#[test]
fn native_counter_wraps() {
    exercise(u32::MAX - 1, 2, 4, false);
}
#[test]
fn failed_stats_poll_preserves_baseline_and_capture() {
    exercise(17, 20, 3, true);
}
#[test]
fn unchanged_native_counter_preserves_behavior() {
    exercise(17, 17, 0, false);
}

#[test]
fn unselected_candidates_are_not_sampled() {
    let (_quiet_tx, quiet_frames) = unbounded();
    let (tx, frames) = unbounded();
    let quiet_calls = Arc::new(AtomicUsize::new(0));
    let selected_calls = Arc::new(AtomicUsize::new(0));
    let fake = FakeBackend(vec![
        StatsDevice {
            frames: quiet_frames,
            drops: Arc::new(AtomicU32::new(900)),
            fail: Arc::new(AtomicBool::new(false)),
            calls: quiet_calls.clone(),
            failures: Arc::new(AtomicUsize::new(0)),
        },
        StatsDevice {
            frames,
            drops: Arc::new(AtomicU32::new(100)),
            fail: Arc::new(AtomicBool::new(false)),
            calls: selected_calls.clone(),
            failures: Arc::new(AtomicUsize::new(0)),
        },
    ]);
    let handle = start_with_backend(CaptureConfig::default(), || Ok(fake)).unwrap();
    wait(|| handle.readiness().is_some());
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(quiet_calls.load(Ordering::SeqCst), 0);
    assert_eq!(selected_calls.load(Ordering::SeqCst), 0);
    tx.send(frame()).unwrap();
    assert_eq!(
        handle
            .frames
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .index,
        0
    );
    wait(|| selected_calls.load(Ordering::SeqCst) >= 2);
    assert_eq!(quiet_calls.load(Ordering::SeqCst), 0);
    assert_eq!(handle.status().dropped_frames, 0);
    handle.stop().unwrap();
}
