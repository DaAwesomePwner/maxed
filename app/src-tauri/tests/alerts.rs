use maxed_app::{runtime::CaptureRuntime, settings::SettingsStore};
use maxed_types::*;
use std::sync::Arc;

#[test]
fn alert_settings_defaults_validation_and_serialized_patch() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SettingsStore::new(dir.path().into()).unwrap());
    let stale = store.get().unwrap();
    let mut preferences = stale.alerts.clone();
    preferences.muted = true;
    preferences.volume_percent = 61;
    preferences.key_popped = AlertLoudness::Banner;
    preferences.key_pop_dungeons = Some(vec!["Lost Halls".into()]);
    store.update_alert_preferences(preferences.clone()).unwrap();
    store
        .put(&AppSettings {
            hardware_acceleration: true,
            ..stale
        })
        .unwrap();
    assert_eq!(store.get().unwrap().alerts, preferences);
    assert!(store.get().unwrap().hardware_acceleration);
    let mut bad = preferences.clone();
    bad.volume_percent = 101;
    assert!(store.update_alert_preferences(bad).is_err());
    assert_eq!(store.get().unwrap().alerts, preferences);
    let workers: Vec<_> = (0..8)
        .map(|i| {
            let store = store.clone();
            std::thread::spawn(move || {
                if i % 2 == 0 {
                    let p = AlertPreferences {
                        muted: true,
                        ..Default::default()
                    };
                    store.update_alert_preferences(p).unwrap();
                } else {
                    store
                        .put(&AppSettings {
                            hardware_acceleration: true,
                            ..Default::default()
                        })
                        .unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let saved = store.get().unwrap();
    assert!(saved.alerts.muted);
    assert!(saved.hardware_acceleration);
}

#[test]
fn corrupt_alert_preferences_do_not_overwrite_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let bytes = b"{corrupt saved settings}";
    std::fs::write(&path, bytes).unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    assert!(store
        .update_alert_preferences(AlertPreferences::default())
        .is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn c0_delivery_and_unconfigured_settings_are_explicitly_unavailable() {
    let runtime = CaptureRuntime::default();
    let status = runtime.alert_delivery_status();
    assert_eq!(status.permission, NotificationPermission::Unavailable);
    assert!(!status.sound_available);
    assert_eq!(
        status.flawless_unavailable,
        maxed_engine::FLAWLESS_UNSUPPORTED
    );
    assert_eq!(
        runtime
            .update_alert_preferences(AlertPreferences::default())
            .unwrap_err(),
        "Alert settings unavailable"
    );
    for kind in [
        AlertKind::WhiteBag,
        AlertKind::PersonalBest,
        AlertKind::KeyPopped,
        AlertKind::Death,
        AlertKind::FlawlessRun,
    ] {
        assert!(runtime.test_alert_delivery(kind).is_err());
    }
}
