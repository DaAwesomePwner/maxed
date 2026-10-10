use maxed_types::*;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};
use std::fmt::Debug;
use ts_rs::TS;

fn assert_states<T: Serialize + DeserializeOwned + PartialEq + Debug + TS>(states: &[(T, &str)]) {
    for (state, spelling) in states {
        assert_eq!(serde_json::to_value(state).unwrap(), json!(spelling));
        assert!(T::decl().contains(&format!("\"{spelling}\"")));
        assert_eq!(
            &serde_json::from_value::<T>(json!(spelling)).unwrap(),
            state
        );
    }
}

#[test]
fn all_setup_discriminants_are_snake_case() {
    assert_states(&[
        (NpcapCheck::Unchecked, "unchecked"),
        (NpcapCheck::Compatible, "compatible"),
        (NpcapCheck::Missing, "missing"),
        (NpcapCheck::Incompatible, "incompatible"),
        (NpcapCheck::PermissionDenied, "permission_denied"),
        (NpcapCheck::Failed, "failed"),
    ]);
    assert_states(&[
        (CaptureAccess::Unknown, "unknown"),
        (CaptureAccess::Allowed, "allowed"),
        (CaptureAccess::Denied, "denied"),
    ]);
    assert_states(&[
        (AssetCheck::Missing, "missing"),
        (AssetCheck::Finding, "finding"),
        (AssetCheck::Extracting, "extracting"),
        (AssetCheck::Ready, "ready"),
        (AssetCheck::Unsupported, "unsupported"),
        (AssetCheck::Failed, "failed"),
    ]);
    assert_states(&[
        (ExtractionStage::Fingerprinting, "fingerprinting"),
        (ExtractionStage::Reading, "reading"),
        (ExtractionStage::Catalog, "catalog"),
        (ExtractionStage::Sprites, "sprites"),
        (ExtractionStage::Publishing, "publishing"),
    ]);
    assert_states(&[
        (UpdateChannel::Stable, "stable"),
        (UpdateChannel::Beta, "beta"),
    ]);
}

#[test]
fn missing_presentation_and_setup_values_stay_null() {
    let setup = serde_json::to_value(SetupStatus::default()).unwrap();
    assert_eq!(setup["extraction_stage"], Value::Null);
    let visual = ObjectVisual {
        object_type: 65535,
        name: None,
        sprite: None,
        bag: None,
        item_tier: None,
    };
    assert_eq!(
        serde_json::to_value(visual).unwrap(),
        json!({
            "object_type": 65535, "name": null, "sprite": null, "bag": null, "item_tier": null
        })
    );
}

#[test]
fn settings_defaults_round_trip_without_persisted_demo() {
    let settings = AppSettings::default();
    let value = serde_json::to_value(&settings).unwrap();
    assert_eq!(
        value,
        json!({
            "version": 1, "adapter_id": null, "assets_path": null,
            "save_recordings": false, "recording_limit_bytes": 1_000_000_000,
            "update_channel": "stable", "hardware_acceleration": false,
            "alerts": AlertPreferences::default(),
            "loot": { "chase": [] },
            "chat": {
                "store": { "public": true, "guild": true, "party": true, "whisper": true },
                "retention_days": 90, "hidden_senders": [], "hidden_keywords": []
            }
        })
    );
    assert_eq!(
        serde_json::from_value::<AppSettings>(value).unwrap(),
        settings
    );
}

#[test]
fn settings_reject_unknown_fields_versions_and_invalid_values() {
    for (key, value) in [
        ("version", json!(0)),
        ("version", json!(2)),
        ("recording_limit_bytes", json!(0)),
        ("recording_limit_bytes", json!(1_000_000_001_u32)),
        ("recording_limit_bytes", json!(-1)),
        ("recording_limit_bytes", json!(1.5)),
        ("update_channel", json!("nightly")),
        ("save_recordings", json!("true")),
        ("hardware_acceleration", json!("true")),
        ("hardware_acceleration", Value::Null),
        ("demo", json!(true)),
    ] {
        let mut settings = serde_json::to_value(AppSettings::default()).unwrap();
        settings[key] = value;
        assert!(
            serde_json::from_value::<AppSettings>(settings).is_err(),
            "{key}"
        );
    }
    for limit in [1, 1_000_000_000] {
        let mut settings = serde_json::to_value(AppSettings::default()).unwrap();
        settings["recording_limit_bytes"] = json!(limit);
        assert!(serde_json::from_value::<AppSettings>(settings).is_ok());
    }
}

#[test]
fn older_settings_disable_acceleration_and_binding_declares_preference() {
    let mut older = serde_json::to_value(AppSettings::default()).unwrap();
    older
        .as_object_mut()
        .unwrap()
        .remove("hardware_acceleration");
    assert!(
        !serde_json::from_value::<AppSettings>(older)
            .unwrap()
            .hardware_acceleration
    );
    assert!(AppSettings::decl().contains("hardware_acceleration: boolean"));
}

#[test]
fn ready_assets_and_demo_never_confirm_decoded_live_traffic() {
    let mut setup = SetupStatus {
        assets: AssetCheck::Ready,
        ..SetupStatus::default()
    };
    setup.confirm_live(false);
    assert!(!setup.live_confirmed);
    setup.demo = true;
    setup.confirm_live(true);
    assert!(!setup.live_confirmed);
    setup.demo = false;
    setup.confirm_live(true);
    assert!(setup.live_confirmed);
    setup.confirm_live(false);
    assert!(!setup.live_confirmed);
}
