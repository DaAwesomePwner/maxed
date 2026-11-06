use maxed_types::*;

#[test]
fn legacy_alerts_and_settings_remain_readable() {
    let alert: AlertView = serde_json::from_str(
        r#"{"id":"old","at":"1","kind":"death","run_id":null,"explanation":"Legacy text"}"#,
    )
    .unwrap();
    assert_eq!(alert.details, None);
    assert_eq!(alert.presentation, None);
    assert_eq!(
        serde_json::from_str::<AlertView>(&serde_json::to_string(&alert).unwrap()).unwrap(),
        alert
    );
    let mut old = serde_json::to_value(AppSettings::default()).unwrap();
    old.as_object_mut().unwrap().remove("alerts");
    let settings: AppSettings = serde_json::from_value(old).unwrap();
    assert_eq!(settings.alerts, AlertPreferences::default());
    assert_eq!(settings.version, 1);
}

#[test]
fn preferences_fail_closed_and_pb_numbers_are_finite() {
    for (field, value) in [
        ("volume_percent", serde_json::json!(101)),
        ("death", serde_json::json!("loud")),
        ("flawless_run", serde_json::json!("banner")),
        ("unknown", serde_json::json!(true)),
    ] {
        let mut p = serde_json::to_value(AlertPreferences::default()).unwrap();
        p[field] = value;
        assert!(serde_json::from_value::<AlertPreferences>(p).is_err());
    }
    let d = AlertDetails::PersonalBest {
        fight_id: "fight".into(),
        character_id: "self".into(),
        boss_type: 1,
        dps: f64::NAN,
        previous_dps: 1.,
    };
    assert!(serde_json::to_string(&d).is_err());
}

#[test]
fn legacy_death_preference_loads_but_is_inactive() {
    let mut value = serde_json::to_value(AppSettings::default()).unwrap();
    value["alerts"]["death"] = serde_json::json!("banner");
    let settings: AppSettings = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        settings.alerts.loudness(&AlertKind::Death),
        AlertLoudness::Off
    );
    value["alerts"].as_object_mut().unwrap().remove("death");
    assert!(serde_json::from_value::<AppSettings>(value).is_ok());
}

#[test]
fn key_pop_settings_round_trip_and_old_settings_defaults() {
    let mut value = serde_json::to_value(AppSettings::default()).unwrap();
    value["alerts"]["key_popped"] = serde_json::json!("banner");
    value["alerts"]["key_pop_dungeons"] = serde_json::json!(["The Void", "Lost Halls"]);
    let settings: AppSettings = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(settings).unwrap(), value);
    value["alerts"]
        .as_object_mut()
        .unwrap()
        .remove("key_popped");
    value["alerts"]
        .as_object_mut()
        .unwrap()
        .remove("key_pop_dungeons");
    value["alerts"]
        .as_object_mut()
        .unwrap()
        .remove("key_pop_excluded_dungeons");
    let old: AppSettings = serde_json::from_value(value).unwrap();
    let saved = serde_json::to_value(old).unwrap();
    assert_eq!(
        saved["alerts"]["key_pop_excluded_dungeons"],
        serde_json::json!([])
    );
    assert_eq!(saved["alerts"]["key_popped"], "off");
    assert_eq!(saved["alerts"]["key_pop_dungeons"], serde_json::Value::Null);
}

#[test]
fn key_pop_filter_bounds_leave_room_for_settings() {
    for names in [
        vec!["dungeon".into(); 257],
        vec!["x".repeat(65)],
        vec!["é".repeat(33)],
    ] {
        let preferences = AlertPreferences {
            key_pop_dungeons: Some(names),
            ..Default::default()
        };
        assert!(!preferences.valid());
    }
    let settings = AppSettings {
        alerts: AlertPreferences {
            key_pop_dungeons: Some(vec!["\\".repeat(64); 256]),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(settings.alerts.valid());
    assert!(serde_json::to_vec(&settings).unwrap().len() + 1 < 36 * 1024);
    let mut combined = settings;
    combined.alerts.key_pop_dungeons = Some(vec!["\\".repeat(64); 128]);
    combined.alerts.key_pop_excluded_dungeons = vec!["\\".repeat(64); 128];
    assert!(combined.alerts.valid());
    assert!(serde_json::to_vec(&combined).unwrap().len() + 1 < 36 * 1024);
    combined
        .alerts
        .key_pop_excluded_dungeons
        .push("extra".into());
    assert!(!combined.alerts.valid());
}

#[test]
fn key_pop_exclusions_and_allowlists_apply_to_known_and_unknown_dungeons() {
    for (only, excluded, expected) in [
        (serde_json::Value::Null, vec![], [true, true]),
        (serde_json::Value::Null, vec!["Lost Halls"], [false, true]),
        (
            serde_json::json!(["Lost Halls"]),
            vec!["Lost Halls"],
            [true, false],
        ),
        (serde_json::json!([]), vec![], [false, false]),
    ] {
        let mut value = serde_json::to_value(AlertPreferences::default()).unwrap();
        value["key_pop_dungeons"] = only;
        value["key_pop_excluded_dungeons"] = serde_json::json!(excluded);
        let preferences: AlertPreferences = serde_json::from_value(value).unwrap();
        for (dungeon, allowed) in ["Lost Halls", "Unknown dungeon"].into_iter().zip(expected) {
            let alert: AlertView = serde_json::from_value(serde_json::json!({
                "id": "key", "at": "1", "kind": "key_popped", "run_id": null,
                "explanation": "Key popped", "details": {"type": "key_popped", "dungeon": dungeon}
            }))
            .unwrap();
            assert_eq!(preferences.allows_alert(&alert), allowed);
        }
    }
}

#[test]
fn key_pop_exclusion_validation_matches_allowlist_bounds() {
    for names in [
        vec!["dungeon".into(); 257],
        vec!["x".repeat(65)],
        vec!["é".repeat(33)],
        vec![String::new()],
        vec!["bad\nname".into()],
        vec!["bad\u{7f}".into()],
    ] {
        let mut value = serde_json::to_value(AlertPreferences::default()).unwrap();
        value["key_pop_excluded_dungeons"] = serde_json::json!(names);
        let preferences: AlertPreferences = serde_json::from_value(value).unwrap();
        assert!(!preferences.valid());
    }
}
