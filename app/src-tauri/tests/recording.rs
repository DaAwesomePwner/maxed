use maxed_app::recording::{RecordAdmission, RecordingRuntime};
use maxed_types::{RawFrame, RecordingState, Timestamp};
use std::{
    fs::File,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

fn frame() -> RawFrame {
    RawFrame {
        index: 0,
        at: Timestamp(1),
        original_len: 1,
        bytes: vec![1],
    }
}

#[test]
fn recording_defaults_off_and_no_files() {
    let temp = tempfile::tempdir().unwrap();
    let runtime = RecordingRuntime::default();
    runtime
        .configure(temp.path().join("recordings"), false, 100, true)
        .unwrap();
    assert_eq!(runtime.try_record(&frame()), RecordAdmission::Disabled);
    runtime.retire();
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    let settings = maxed_app::settings::SettingsStore::new(temp.path().into()).unwrap();
    assert!(!settings.get().unwrap().save_recordings);
    assert!(!temp.path().join("recordings").exists());
}

#[test]
fn admission_failure_does_not_install_a_sink_and_disable_clears_failure() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("recordings");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("foreign.pcap"), b"untouched").unwrap();
    let runtime = RecordingRuntime::default();
    assert!(runtime.configure(root.clone(), true, 100, true).is_err());
    assert_eq!(runtime.status(100).state, RecordingState::Failed);
    assert_eq!(runtime.try_record(&frame()), RecordAdmission::Disabled);
    runtime.configure(root.clone(), false, 100, true).unwrap();
    assert_eq!(runtime.status(100).state, RecordingState::Disabled);
    assert_eq!(
        std::fs::read(root.join("foreign.pcap")).unwrap(),
        b"untouched"
    );
}

#[test]
fn idle_opt_in_arms_without_keeping_worker_or_namespace_lease() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("recordings");
    let runtime = RecordingRuntime::default();
    runtime.configure(root.clone(), true, 100, false).unwrap();
    assert_eq!(runtime.status(100).state, RecordingState::Armed);
    assert_eq!(runtime.try_record(&frame()), RecordAdmission::Disabled);
    let owner = maxed_app::recording::RecordingOwner::open(root.clone(), 100).unwrap();
    owner.finish().unwrap();
    assert_eq!(std::fs::read_dir(root).unwrap().count(), 1);
}

struct BlockDisk {
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
    once: AtomicBool,
}
impl maxed_recording::RecordingDisk for BlockDisk {
    fn write(&self, file: &mut File, bytes: &[u8]) -> std::io::Result<usize> {
        if self.once.swap(false, Ordering::AcqRel) {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
        }
        std::io::Write::write(file, bytes)
    }
}

#[test]
fn retiring_blocked_disk_never_holds_capture_admission_mutex() {
    let temp = tempfile::tempdir().unwrap();
    let runtime = Arc::new(RecordingRuntime::default());
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, gate) = crossbeam_channel::bounded(1);
    runtime
        .configure_with_disk(
            temp.path().join("recordings"),
            true,
            1000,
            true,
            Arc::new(BlockDisk {
                entered,
                release: gate,
                once: AtomicBool::new(true),
            }),
        )
        .unwrap();
    assert_eq!(runtime.try_record(&frame()), RecordAdmission::Accepted);
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let retiring = runtime.clone();
    let retire = std::thread::spawn(move || retiring.retire());
    let probing = runtime.clone();
    let (sent, received) = crossbeam_channel::bounded(1);
    let probe = std::thread::spawn(move || {
        // A bounded recording queue lets this probe eventually see Disabled,
        // even if scheduled immediately before retirement takes its short lock.
        loop {
            if probing.try_record(&frame()) == RecordAdmission::Disabled {
                break;
            }
            std::thread::yield_now();
        }
        sent.send(()).unwrap();
    });
    let result = received.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    retire.join().unwrap();
    probe.join().unwrap();
    assert!(result.is_ok(), "capture waited for blocked recording disk");
    assert_eq!(runtime.status(1000).failure, None);
}

#[test]
fn cap_swap_preserves_overflow_count_and_counts_gap_without_blocking_capture() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("recordings");
    let runtime = Arc::new(RecordingRuntime::default());
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, gate) = crossbeam_channel::bounded(1);
    runtime
        .configure_with_disk(
            root.clone(),
            true,
            64_000_000,
            true,
            Arc::new(BlockDisk {
                entered,
                release: gate,
                once: AtomicBool::new(true),
            }),
        )
        .unwrap();
    runtime.try_record(&frame());
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let packet = RawFrame {
        original_len: 4096,
        bytes: vec![0; 4096],
        ..frame()
    };
    for _ in 0..maxed_recording::QUEUE_BYTE_CAPACITY / 4096 + 3 {
        assert_eq!(runtime.try_record(&packet), RecordAdmission::Accepted);
    }
    let before = runtime.status(64_000_000).dropped_frames;
    assert_eq!(before, 3);
    let changing = runtime.clone();
    let next_root = root.clone();
    let swap = std::thread::spawn(move || changing.configure(next_root, true, 32_000_000, true));
    // Probe admission on another thread so a lock regression cannot hang the test.
    let probing = runtime.clone();
    let (sent, received) = crossbeam_channel::bounded(1);
    let probe = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while probing.try_record(&frame()) != RecordAdmission::Disabled {
            if std::time::Instant::now() >= deadline {
                return;
            }
            std::thread::yield_now();
        }
        let gap_start = probing.status(32_000_000).dropped_frames;
        for _ in 0..7 {
            assert_eq!(probing.try_record(&frame()), RecordAdmission::Disabled);
        }
        sent.send((gap_start, probing.status(32_000_000).dropped_frames))
            .unwrap();
    });
    let observed = received.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    swap.join().unwrap().unwrap();
    probe.join().unwrap();
    let (gap_start, gap_end) = observed.expect("capture/status waited for blocked disk swap");
    assert!(gap_start > before); // Includes the first refused probe admission.
    assert_eq!(gap_end, gap_start + 7);
    assert_eq!(runtime.status(32_000_000).dropped_frames, gap_end);
    assert_eq!(runtime.try_record(&frame()), RecordAdmission::Accepted);
    runtime.retire();
    assert_eq!(runtime.status(32_000_000).dropped_frames, gap_end);
    runtime.configure(root, true, 32_000_000, true).unwrap();
    assert_eq!(runtime.status(32_000_000).dropped_frames, 0);
    runtime.retire();
}
