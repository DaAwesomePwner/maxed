use super::*;
use crate::recording::{RecordAdmission, RecordingRuntime};
use maxed_types::{RawFrame, RecordingState, Timestamp};

struct UnusedBackend;
impl CaptureBackend for UnusedBackend {
    fn probe(&self) -> Result<(), CaptureError> {
        panic!("no capture transition expected")
    }
    fn adapters(&self) -> Result<Vec<maxed_capture::AdapterInfo>, CaptureError> {
        panic!("no enumeration expected")
    }
    fn start(&self, _: CaptureConfig) -> Result<CaptureHandle, CaptureError> {
        panic!("no capture restart expected")
    }
}
struct RecordingLifecycle {
    root: PathBuf,
    recording: RecordingRuntime,
    running: AtomicBool,
    pause: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
}
impl Lifecycle for RecordingLifecycle {
    fn configure_recording(&self, enabled: bool, limit: u32, live: bool) -> Result<(), String> {
        self.recording
            .configure(self.root.join("recordings"), enabled, limit, live)
            .map_err(|_| "Synthetic recording admission failed".to_string())?;
        if enabled {
            let pause = self.pause.lock().unwrap().take();
            if let Some((entered, release)) = pause {
                entered.send(()).unwrap();
                release.recv().unwrap();
            }
        }
        Ok(())
    }
    fn recording_status(&self, limit: u32) -> maxed_types::RecordingStatus {
        self.recording.status(limit)
    }
    fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
    fn live(&self) -> bool {
        self.running()
    }
    fn stop(&self) -> Result<(), String> {
        self.running.store(false, Ordering::Release);
        self.recording.retire();
        Ok(())
    }
    fn start(&self, _: Option<CachedCatalog>, _: CaptureHandle) -> Result<(), String> {
        panic!("capture must remain live")
    }
    fn demo(&self, _: bool) -> Result<(), String> {
        Ok(())
    }
}
fn harness() -> (tempfile::TempDir, Arc<RecordingLifecycle>, SetupService) {
    let temp = tempfile::tempdir().unwrap();
    let life = Arc::new(RecordingLifecycle {
        root: temp.path().into(),
        recording: RecordingRuntime::default(),
        running: AtomicBool::new(true),
        pause: Mutex::new(None),
    });
    let service = SetupService::with_discovery_roots(
        temp.path().into(),
        Arc::new(UnusedBackend),
        life.clone(),
        Arc::new(AssetBridge::default()),
        vec![],
    )
    .unwrap();
    service.settings.put(&AppSettings::default()).unwrap();
    (temp, life, service)
}
fn enabled(service: &SetupService) -> AppSettings {
    AppSettings {
        save_recordings: true,
        recording_limit_bytes: 1000,
        ..service.settings.get().unwrap()
    }
}

#[test]
fn enable_acknowledges_owner_before_committing_and_disable_survives_failed_recorder() {
    let (_temp, life, service) = harness();
    service.update(enabled(&service)).unwrap();
    assert!(service.settings.get().unwrap().save_recordings);
    assert_eq!(life.recording.status(1000).state, RecordingState::Armed);
    let invalid = RawFrame {
        index: 0,
        at: Timestamp(0),
        original_len: 0,
        bytes: vec![1],
    };
    assert!(matches!(
        life.recording.try_record(&invalid),
        RecordAdmission::Failed(_)
    ));
    service
        .update(AppSettings {
            save_recordings: false,
            ..service.settings.get().unwrap()
        })
        .unwrap();
    assert!(!service.settings.get().unwrap().save_recordings);
    assert_eq!(life.recording.status(1000).state, RecordingState::Disabled);
    assert!(life.running());
}

#[test]
fn failed_settings_commit_rolls_back_recording_and_keeps_capture_live() {
    let (temp, life, service) = harness();
    std::fs::create_dir(temp.path().join("settings.pending")).unwrap();
    assert!(service.update(enabled(&service)).is_err());
    assert!(!service.settings.get().unwrap().save_recordings);
    assert_eq!(life.recording.status(1000).state, RecordingState::Disabled);
    assert!(life.running());
    // Rollback joined the worker and released its real namespace lease.
    let owner =
        crate::recording::RecordingOwner::open(temp.path().join("recordings"), 1000).unwrap();
    owner.finish().unwrap();
}

#[test]
fn rejected_owner_leaves_settings_and_capture_unchanged() {
    let (temp, life, service) = harness();
    std::fs::create_dir(temp.path().join("recordings")).unwrap();
    let foreign = temp.path().join("recordings/foreign");
    std::fs::write(&foreign, b"untouched").unwrap();
    assert!(service.update(enabled(&service)).is_err());
    assert!(!service.settings.get().unwrap().save_recordings);
    assert!(life.running());
    assert_eq!(std::fs::read(foreign).unwrap(), b"untouched");
}

#[test]
fn demo_prevents_new_opt_in_without_creating_a_lease() {
    let (temp, life, service) = harness();
    service.set_demo(true).unwrap();
    assert!(service.update(enabled(&service)).is_err());
    assert!(!service.settings.get().unwrap().save_recordings);
    assert!(!temp.path().join("recordings").exists());
    assert!(!life.running());
}

#[test]
fn concurrent_enable_disable_serializes_admission_and_settings_commit() {
    let (_temp, life, service) = harness();
    let service = Arc::new(service);
    let (entered, waiting) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    *life.pause.lock().unwrap() = Some((entered, gate));
    let enabling = service.clone();
    let enable = std::thread::spawn(move || enabling.update(enabled(&enabling)));
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!service.settings.get().unwrap().save_recordings);
    assert_eq!(life.recording.status(1000).state, RecordingState::Armed);
    let disabling = service.clone();
    let (started, acknowledged) = std::sync::mpsc::channel();
    let (done, completion) = std::sync::mpsc::channel();
    let disable = std::thread::spawn(move || {
        started.send(()).unwrap();
        done.send(disabling.update(AppSettings::default())).unwrap();
    });
    acknowledged.recv_timeout(Duration::from_secs(5)).unwrap();
    let premature = completion.recv_timeout(Duration::from_millis(50));
    release.send(()).unwrap();
    enable.join().unwrap().unwrap();
    assert!(
        premature.is_err(),
        "disable overtook admitted but uncommitted enable"
    );
    completion
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    disable.join().unwrap();
    assert!(!service.settings.get().unwrap().save_recordings);
    assert_eq!(life.recording.status(1000).state, RecordingState::Disabled);
    assert!(life.running());
}

#[test]
fn persisted_opt_in_reports_armed_on_launch_without_opening_storage() {
    let (temp, life, service) = harness();
    life.running.store(false, Ordering::Release);
    service.settings.put(&enabled(&service)).unwrap();
    assert_eq!(
        service.recording_status().unwrap().state,
        RecordingState::Armed
    );
    assert!(!temp.path().join("recordings").exists());
}
