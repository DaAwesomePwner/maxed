use maxed_types::*;
use serde_json::{json, Value};
use ts_rs::TS;

#[test]
fn enum_casing_nullability_and_generated_wire_shape_are_frozen() {
    let status = UpdateStatus::idle(UpdateChannel::Stable);
    assert_eq!(
        serde_json::to_value(status).unwrap(),
        json!({"phase":"idle","channel":"stable","offer":null,"downloaded_bytes":0,"total_bytes":null,"failure":null})
    );
    for (phase, text) in [
        (UpdatePhase::Checking, "checking"),
        (UpdatePhase::Available, "available"),
        (UpdatePhase::Downloading, "downloading"),
        (UpdatePhase::Ready, "ready"),
        (UpdatePhase::Installing, "installing"),
        (UpdatePhase::UpToDate, "up_to_date"),
        (UpdatePhase::Failed, "failed"),
    ] {
        assert_eq!(serde_json::to_value(phase).unwrap(), text);
    }
    for (failure, text) in [
        (UpdateFailure::Offline, "offline"),
        (UpdateFailure::Unavailable, "unavailable"),
        (UpdateFailure::InvalidMetadata, "invalid_metadata"),
        (UpdateFailure::InvalidSignature, "invalid_signature"),
        (UpdateFailure::UnsupportedTarget, "unsupported_target"),
        (UpdateFailure::TooLarge, "too_large"),
        (UpdateFailure::Busy, "busy"),
        (UpdateFailure::StaleOffer, "stale_offer"),
        (UpdateFailure::ShutdownBlocked, "shutdown_blocked"),
        (UpdateFailure::InstallFailed, "install_failed"),
    ] {
        assert_eq!(serde_json::to_value(failure).unwrap(), text);
    }
    for (state, text) in [
        (RecordingState::Disabled, "disabled"),
        (RecordingState::Armed, "armed"),
        (RecordingState::Writing, "writing"),
        (RecordingState::Failed, "failed"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), text);
    }
    for (failure, text) in [
        (RecordingFailure::InUse, "in_use"),
        (RecordingFailure::Quota, "quota"),
        (RecordingFailure::QueueFull, "queue_full"),
        (RecordingFailure::Disk, "disk"),
        (RecordingFailure::InvalidFrame, "invalid_frame"),
    ] {
        assert_eq!(serde_json::to_value(failure).unwrap(), text);
    }
    let wire = UpdateStatus::decl();
    assert!(wire.contains("offer: UpdateOffer | null"));
    assert!(wire.contains("total_bytes: number | null"));
    assert!(UpdateOffer::decl().contains("request_id: string"));
    assert!(RecordingStatus::decl().contains("failure: RecordingFailure | null"));
    assert!(RecordingStatus::decl().contains("dropped_frames: number"));
}

#[test]
fn offer_constructor_and_deserializer_enforce_utf8_semver_and_id_bounds() {
    let valid = json!({"request_id":"18446744073709551615","version":"1.2.3-beta.1","channel":"beta","notes":"Synthetic notes"});
    assert!(serde_json::from_value::<UpdateOffer>(valid.clone()).is_ok());
    for (field, value) in [
        ("request_id", "".into()),
        ("request_id", "1".repeat(21)),
        ("request_id", "١".into()),
        ("version", "latest".into()),
        ("version", format!("1.0.0+{}", "x".repeat(123))),
        ("notes", "é".repeat(4097)),
    ] {
        let mut row = valid.clone();
        row[field] = Value::String(value);
        assert!(serde_json::from_value::<UpdateOffer>(row.clone()).is_err());
        assert!(UpdateOffer::new(
            row["request_id"].as_str().unwrap().into(),
            row["version"].as_str().unwrap().into(),
            UpdateChannel::Beta,
            row["notes"].as_str().unwrap().into()
        )
        .is_err());
    }
    assert!(UpdateOffer::new(
        "1".into(),
        "2.0.0".into(),
        UpdateChannel::Stable,
        "é".repeat(4096)
    )
    .is_ok());
}

#[test]
fn bounded_counts_reject_negative_overflow_and_preserve_unknown_total() {
    let mut update = serde_json::to_value(UpdateStatus::idle(UpdateChannel::Stable)).unwrap();
    for field in ["downloaded_bytes", "total_bytes"] {
        for value in [json!(-1), json!(20_000_001u32), json!(u64::MAX)] {
            update[field] = value;
            assert!(serde_json::from_value::<UpdateStatus>(update.clone()).is_err());
        }
        update[field] = json!(0);
    }
    let mut recording =
        json!({"state":"disabled","limit_bytes":1_000_000_000,"retained_bytes":0,"failure":null});
    assert_eq!(
        serde_json::from_value::<RecordingStatus>(recording.clone())
            .unwrap()
            .dropped_frames,
        0
    );
    recording["dropped_frames"] = json!(5000);
    assert_eq!(
        serde_json::from_value::<RecordingStatus>(recording.clone())
            .unwrap()
            .dropped_frames,
        5000
    );
    recording["limit_bytes"] = json!(0);
    assert!(serde_json::from_value::<RecordingStatus>(recording.clone()).is_err());
    recording["limit_bytes"] = json!(43);
    recording["retained_bytes"] = json!(1_000_000_001);
    assert!(serde_json::from_value::<RecordingStatus>(recording).is_err());
}

#[test]
fn old_settings_keep_small_quota_defaults_and_reject_unknown_fields() {
    let old = json!({"version":1,"adapter_id":null,"assets_path":null,"save_recordings":false,"recording_limit_bytes":43,"update_channel":"beta"});
    let settings: AppSettings = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(settings.recording_limit_bytes, 43);
    assert!(!settings.hardware_acceleration);
    assert!(!settings.save_recordings);
    assert_eq!(settings.alerts, AlertPreferences::default());
    let mut invalid = old;
    invalid["remote_url"] = json!("https://example.invalid");
    assert!(serde_json::from_value::<AppSettings>(invalid).is_err());
}
