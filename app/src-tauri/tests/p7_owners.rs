use maxed_app::{
    interactive_rail::InteractiveRail,
    recording::{RecordAdmission, RecordingOwner, RecordingRuntime},
};
use maxed_types::{AppSettings, RawFrame, RecordingFailure, RecordingState, Timestamp};

fn frame() -> RawFrame {
    RawFrame {
        index: 1,
        at: Timestamp(1_000_000),
        original_len: 3,
        bytes: vec![1, 2, 3],
    }
}

#[test]
fn startup_receipt_is_opt_in_main_only_fixed_and_once() {
    for value in [None, Some("0"), Some("true"), Some("01")] {
        let marker = InteractiveRail::new(value);
        let mut bytes = Vec::new();
        marker.report(true, &mut bytes).unwrap();
        marker.report(false, &mut bytes).unwrap();
        assert!(bytes.is_empty());
    }
    let marker = InteractiveRail::new(Some("1"));
    let mut bytes = Vec::new();
    assert!(marker.report(false, &mut bytes).is_err());
    marker.report(true, &mut bytes).unwrap();
    marker.report(true, &mut bytes).unwrap();
    assert_eq!(bytes, b"MAXED_INTERACTIVE_RAIL_V1\n");
}

#[test]
fn recording_default_off_and_explicit_opt_in_is_valid() {
    let runtime = RecordingRuntime::default();
    assert_eq!(runtime.try_record(&frame()), RecordAdmission::Disabled);
    assert_eq!(runtime.status(43).state, RecordingState::Disabled);
    runtime.retire();
    let mut settings = AppSettings {
        recording_limit_bytes: 43,
        ..Default::default()
    };
    assert!(maxed_app::settings::validate(&settings).is_ok());
    settings.save_recordings = true;
    assert!(maxed_app::settings::validate(&settings).is_ok());
}

#[test]
fn native_recording_open_acknowledges_inventory_and_finish_joins_prefix() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("recordings");
    let owner = RecordingOwner::open(directory.clone(), 1000).unwrap();
    assert_eq!(owner.status().state, RecordingState::Armed);
    assert_eq!(owner.try_record(&frame()), RecordAdmission::Accepted);
    owner.finish().unwrap();
    let owner = RecordingOwner::open(directory, 1000).unwrap();
    assert_eq!(owner.status().retained_bytes, 43);
    owner.finish().unwrap();
}

#[test]
fn native_recording_failure_is_sticky_and_preserves_known_inventory() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("recordings");
    let first = RecordingOwner::open(directory.clone(), 1000).unwrap();
    first.try_record(&frame());
    first.finish().unwrap();
    let owner = RecordingOwner::open(directory, 1000).unwrap();
    let invalid = RawFrame {
        original_len: 1,
        ..frame()
    };
    assert_eq!(
        owner.try_record(&invalid),
        RecordAdmission::Failed(RecordingFailure::InvalidFrame)
    );
    assert_eq!(
        owner.try_record(&frame()),
        RecordAdmission::Failed(RecordingFailure::InvalidFrame)
    );
    assert_eq!(owner.status().state, RecordingState::Failed);
    assert_eq!(owner.status().retained_bytes, 43);
    assert!(owner.finish().is_err());
}
