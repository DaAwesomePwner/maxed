use super::*;
use maxed_types::{AlertPreferences, HomeScope, TimeRange};

fn configured(root: &std::path::Path) -> Arc<CaptureRuntime> {
    let runtime = Arc::new(CaptureRuntime::default());
    runtime
        .configure_alert_settings(Arc::new(
            crate::settings::SettingsStore::new(root.into()).unwrap(),
        ))
        .unwrap();
    runtime
}
fn changed() -> AlertPreferences {
    AlertPreferences {
        muted: true,
        volume_percent: 73,
        ..Default::default()
    }
}

#[test]
fn new_volume_default_preserves_saved_volume_and_notification_choice() {
    let root = tempfile::tempdir().unwrap();
    let settings = crate::settings::SettingsStore::new(root.path().into()).unwrap();
    assert_eq!(settings.get().unwrap().alerts.volume_percent, 70);
    settings
        .update_alert_preferences(AlertPreferences {
            volume_percent: 35,
            windows_notifications: true,
            ..Default::default()
        })
        .unwrap();
    let reopened = crate::settings::SettingsStore::new(root.path().into()).unwrap();
    assert_eq!(reopened.get().unwrap().alerts.volume_percent, 35);
    assert!(reopened.get().unwrap().alerts.windows_notifications);
}

#[test]
fn alert_edits_work_during_schema_preparation_and_after_schema_failure() {
    for broken in [false, true] {
        let root = tempfile::tempdir().unwrap();
        if broken {
            std::fs::write(root.path().join("live.sqlite3"), b"invalid database").unwrap();
        }
        let runtime = configured(root.path());
        let (entered, waiting) = bounded(1);
        let (release, resume) = bounded(1);
        *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(5)).unwrap();
        }));
        runtime.initialize_home(root.path().into()).unwrap();
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(runtime.shared.home.context.lock().unwrap().is_none());
        let edited = runtime.update_alert_preferences(changed());
        release.send(()).unwrap();
        tauri::async_runtime::block_on(runtime.wait_history_preparation());
        assert!(edited.is_ok(), "edit during preparation: {edited:?}");
        if broken {
            assert!(runtime.shared.home.context.lock().unwrap().is_none());
        }
        runtime
            .update_alert_preferences(AlertPreferences::default())
            .unwrap();
        assert_eq!(
            *runtime.shared.views.alert_policy.lock().unwrap(),
            AlertPreferences::default()
        );
    }
}

#[test]
fn opening_live_cannot_overwrite_a_concurrent_saved_policy() {
    let root = tempfile::tempdir().unwrap();
    let runtime = configured(root.path());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let (entered, waiting) = bounded(1);
    let (release, resume) = bounded(1);
    *runtime.shared.on_alert_policy_read.lock().unwrap() = Some(Arc::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let opening = runtime.clone();
    let options = RuntimeOptions::live(root.path().into(), None);
    let worker = thread::spawn(move || {
        opening
            .shared
            .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
            .unwrap()
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let editing = runtime.clone();
    let (started, start) = bounded(1);
    let (done, saved) = bounded(1);
    let edit = thread::spawn(move || {
        started.send(()).unwrap();
        done.send(editing.update_alert_preferences(changed()))
            .unwrap();
    });
    start.recv_timeout(Duration::from_secs(5)).unwrap();
    let early = saved.recv_timeout(Duration::from_millis(100));
    release.send(()).unwrap();
    let mut adapter = worker.join().unwrap();
    edit.join().unwrap();
    let serialized = early.is_err();
    early
        .unwrap_or_else(|_| saved.recv_timeout(Duration::from_secs(5)).unwrap())
        .unwrap();
    assert!(
        serialized,
        "policy save must serialize with the open read/publication"
    );
    assert_eq!(*adapter.alert_policy.lock().unwrap(), changed());
    assert_eq!(
        crate::settings::SettingsStore::new(root.path().into())
            .unwrap()
            .get()
            .unwrap()
            .alerts,
        changed()
    );
    adapter.close().unwrap();
}

#[test]
fn alert_save_does_not_hold_history_admission_mutex() {
    let root = tempfile::tempdir().unwrap();
    let runtime = configured(root.path());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let (entered, waiting) = bounded(1);
    let (release, resume) = bounded(1);
    *runtime.shared.on_alert_policy_save.lock().unwrap() = Some(Arc::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let editing = runtime.clone();
    let worker = thread::spawn(move || editing.update_alert_preferences(changed()));
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let admission_free = runtime.shared.home.context.try_lock().is_ok();
    let reading = runtime.clone();
    let (done, read) = bounded(1);
    let reader = thread::spawn(move || {
        done.send(tauri::async_runtime::block_on(reading.home_summary(
            HomeScope::Today {
                range: TimeRange {
                    start: Timestamp(0),
                    end: Timestamp(10),
                },
            },
        )))
        .unwrap()
    });
    let early_read = read.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    reader.join().unwrap();
    assert!(
        admission_free,
        "settings I/O must not lock history admission"
    );
    assert!(
        early_read.unwrap().is_ok(),
        "history should complete while the settings save is paused"
    );
}

#[test]
fn failed_live_policy_read_leaves_history_and_policy_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let runtime = configured(root.path());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    runtime.update_alert_preferences(changed()).unwrap();
    let generation = runtime.shared.home.generation.load(Ordering::Acquire);
    let snapshot = runtime.snapshot();
    std::fs::write(root.path().join("settings.json"), b"corrupt").unwrap();
    let result = runtime.shared.open_home(
        &RuntimeOptions::live(root.path().into(), None),
        &AtomicBool::new(false),
        Bridge::default(),
        None,
    );
    assert_eq!(result.err().unwrap(), crate::settings::SETTINGS_ERROR);
    assert_eq!(
        runtime.shared.home.generation.load(Ordering::Acquire),
        generation
    );
    assert_eq!(
        runtime
            .shared
            .home
            .context
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .namespace,
        Namespace::Live
    );
    assert_eq!(runtime.snapshot(), snapshot);
    assert!(!root.path().join("live.sqlite3").exists());
    assert_eq!(
        runtime
            .update_alert_preferences(Default::default())
            .unwrap_err(),
        crate::settings::SETTINGS_ERROR
    );
    assert_eq!(
        *runtime.shared.views.alert_policy.lock().unwrap(),
        changed()
    );
}

#[test]
fn replay_with_corrupt_settings_stays_ephemeral_and_can_exit() {
    let root = tempfile::tempdir().unwrap();
    let runtime = configured(root.path());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let corrupt = b"corrupt replay settings";
    std::fs::write(root.path().join("settings.json"), corrupt).unwrap();
    let mut options = RuntimeOptions::live(root.path().into(), None);
    options.namespace = Namespace::Replay;
    let authored =
        std::fs::read(crate::demo::DemoResources::development_root().join("story.pcap")).unwrap();
    runtime
        .replay_reader(options, std::io::Cursor::new(authored), Bridge::default())
        .unwrap();
    assert!(runtime.snapshot().stream_id.starts_with("replay-"));
    assert!(runtime.store_status().durable.is_some());
    assert_eq!(
        runtime.update_alert_preferences(changed()).unwrap_err(),
        "Live alert preferences cannot be changed during preview or replay."
    );
    runtime.reset_preview().unwrap();
    assert_eq!(
        runtime
            .shared
            .home
            .context
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .namespace,
        Namespace::Live
    );
    assert_eq!(
        runtime.update_alert_preferences(changed()).unwrap_err(),
        crate::settings::SETTINGS_ERROR
    );
    assert_eq!(
        std::fs::read(root.path().join("settings.json")).unwrap(),
        corrupt
    );
}
