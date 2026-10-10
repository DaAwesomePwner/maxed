use maxed_app::{settings::SettingsStore, window_policy::*};
use maxed_types::AppSettings;

#[test]
fn browser_defaults_survive_both_acceleration_choices() {
    assert_eq!(
        DEFAULT_BROWSER_ARGS,
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"
    );
    assert_eq!(browser_args(true), DEFAULT_BROWSER_ARGS);
    assert_eq!(
        browser_args(false),
        format!("{DEFAULT_BROWSER_ARGS} --disable-gpu --in-process-gpu --enable-features=NetworkServiceInProcess2")
    );
    assert!(!browser_args(true)
        .split_whitespace()
        .any(|arg| arg == "--disable-gpu"));
}

#[test]
fn old_settings_load_gpu_off_and_preference_survives_restart() {
    let root = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(root.path().to_owned()).unwrap();
    let mut old = serde_json::to_value(AppSettings::default()).unwrap();
    old.as_object_mut().unwrap().remove("hardware_acceleration");
    std::fs::write(
        root.path().join("settings.json"),
        serde_json::to_vec(&old).unwrap(),
    )
    .unwrap();
    let mut settings = store.get().unwrap();
    assert!(!settings.hardware_acceleration);
    settings.hardware_acceleration = true;
    store.put(&settings).unwrap();
    let restarted = SettingsStore::new(root.path().to_owned()).unwrap();
    assert!(restarted.get().unwrap().hardware_acceleration);
    settings.hardware_acceleration = false;
    restarted.put(&settings).unwrap();
    assert!(!store.get().unwrap().hardware_acceleration);
}

#[test]
fn demo_measurement_requires_explicit_opt_in() {
    assert!(measurement_demo(Some("1")));
    for value in [None, Some(""), Some("0"), Some("true")] {
        assert!(!measurement_demo(value));
    }
}

#[test]
fn measurement_loops_require_both_flags_and_a_bounded_decimal_count() {
    for count in ["1", "2", "113", "500"] {
        assert_eq!(
            measurement_demo_loops(Some("1"), Some(count)),
            count.parse().ok()
        );
        for gate in [None, Some(""), Some("0"), Some("true"), Some(" 1")] {
            assert_eq!(measurement_demo_loops(gate, Some(count)), None);
        }
    }
    for count in [
        None,
        Some(""),
        Some("0"),
        Some("501"),
        Some("65536"),
        Some("-1"),
        Some("+1"),
        Some("1.0"),
        Some(" 1"),
        Some("1 "),
        Some("true"),
    ] {
        assert_eq!(measurement_demo_loops(Some("1"), count), None);
    }
}

#[test]
fn main_window_waits_for_settings_policy() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    let windows = config["app"]["windows"].as_array().unwrap();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0]["label"], "main");
    assert_eq!(windows[0]["create"], false);
}
