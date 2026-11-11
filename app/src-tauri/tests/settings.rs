use maxed_app::settings::{validate, SettingsStore};
use maxed_types::{AppSettings, UpdateChannel};
use std::{fs, sync::Arc};

#[test]
fn absent_defaults_and_round_trip_use_only_the_supplied_app_data() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("app-data");
    let store = SettingsStore::new(root.clone()).unwrap();
    assert_eq!(store.get().unwrap(), AppSettings::default());
    assert!(!root.exists());
    let next = AppSettings {
        update_channel: UpdateChannel::Beta,
        ..Default::default()
    };
    store.put(&next).unwrap();
    assert_eq!(store.get().unwrap(), next);
    assert!(root.join("settings.json").is_file());
    assert!(!root.join("settings.pending").exists());
    assert!(SettingsStore::new("relative".into()).is_err());
}

#[test]
fn malformed_newer_oversized_and_unknown_fields_are_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    let mut newer = serde_json::to_value(AppSettings::default()).unwrap();
    newer["version"] = 2.into();
    let mut unknown = serde_json::to_value(AppSettings::default()).unwrap();
    unknown["secret"] = "private-token".into();
    for bytes in [
        b"private-token".to_vec(),
        serde_json::to_vec(&newer).unwrap(),
        serde_json::to_vec(&unknown).unwrap(),
        vec![b' '; 65537],
    ] {
        fs::write(dir.path().join("settings.json"), &bytes).unwrap();
        let error = store.get().unwrap_err();
        assert!(!error.contains("private-token"));
        assert!(!error.contains(&dir.path().to_string_lossy().to_string()));
        assert!(store.put(&AppSettings::default()).is_err());
        assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), bytes);
    }
}

#[test]
fn rejected_values_and_failed_atomic_write_preserve_previous_document() {
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    store.put(&AppSettings::default()).unwrap();
    let old = fs::read(dir.path().join("settings.json")).unwrap();
    for value in [
        AppSettings {
            recording_limit_bytes: 0,
            ..Default::default()
        },
        AppSettings {
            version: 2,
            ..Default::default()
        },
        AppSettings {
            adapter_id: Some("bad\0id".into()),
            ..Default::default()
        },
        AppSettings {
            assets_path: Some("relative/resources.assets".into()),
            ..Default::default()
        },
    ] {
        assert!(validate(&value).is_err());
        assert!(store.put(&value).is_err());
        assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), old);
    }
    fs::create_dir(dir.path().join("settings.pending")).unwrap();
    assert!(store
        .put(&AppSettings {
            update_channel: UpdateChannel::Beta,
            ..Default::default()
        })
        .is_err());
    assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), old);
    assert!(dir.path().join("settings.pending").is_dir());
}

#[cfg(windows)]
#[test]
fn replacement_failure_keeps_old_settings_and_cleans_own_staging_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    store.put(&AppSettings::default()).unwrap();
    let path = dir.path().join("settings.json");
    let old = fs::read(&path).unwrap();
    let _lease = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(store
        .put(&AppSettings {
            update_channel: UpdateChannel::Beta,
            ..Default::default()
        })
        .is_err());
    assert_eq!(fs::read(path).unwrap(), old);
    assert!(!dir.path().join("settings.pending").exists());
}

#[test]
fn concurrent_writes_serialize_complete_documents() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SettingsStore::new(dir.path().into()).unwrap());
    let workers: Vec<_> = (1..=16)
        .map(|limit| {
            let store = store.clone();
            std::thread::spawn(move || {
                store
                    .put(&AppSettings {
                        recording_limit_bytes: limit,
                        ..Default::default()
                    })
                    .unwrap()
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert!((1..=16).contains(&store.get().unwrap().recording_limit_bytes));
    assert!(!dir.path().join("settings.pending").exists());
}

#[test]
fn stale_staging_file_is_recovered_without_losing_committed_settings() {
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    store.put(&AppSettings::default()).unwrap();
    fs::write(
        dir.path().join("settings.pending"),
        b"interrupted partial write",
    )
    .unwrap();
    let next = AppSettings {
        update_channel: UpdateChannel::Beta,
        ..Default::default()
    };
    store.put(&next).unwrap();
    assert_eq!(store.get().unwrap(), next);
    assert!(!dir.path().join("settings.pending").exists());
}

#[cfg(windows)]
#[test]
fn independent_store_waits_for_os_lease_before_recovering_staging() {
    use std::{os::windows::fs::OpenOptionsExt, time::Duration};
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    store.put(&AppSettings::default()).unwrap();
    fs::write(
        dir.path().join("settings.pending"),
        b"active writer staging",
    )
    .unwrap();
    let lease = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(dir.path().join("settings.lock"))
        .unwrap();
    let other = SettingsStore::new(dir.path().into()).unwrap();
    let (done, wait) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = other.put(&AppSettings {
            update_channel: UpdateChannel::Beta,
            ..Default::default()
        });
        done.send(result).unwrap();
    });
    assert!(wait.recv_timeout(Duration::from_millis(50)).is_err());
    assert_eq!(store.get().unwrap(), AppSettings::default());
    assert_eq!(
        fs::read(dir.path().join("settings.pending")).unwrap(),
        b"active writer staging"
    );
    drop(lease);
    wait.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
    worker.join().unwrap();
    assert_eq!(store.get().unwrap().update_channel, UpdateChannel::Beta);
}

#[test]
fn independent_store_instances_serialize_concurrent_writes() {
    let dir = tempfile::tempdir().unwrap();
    let workers: Vec<_> = (1..=16)
        .map(|limit| {
            let store = SettingsStore::new(dir.path().into()).unwrap();
            std::thread::spawn(move || {
                store
                    .put(&AppSettings {
                        recording_limit_bytes: limit,
                        ..Default::default()
                    })
                    .unwrap()
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert!((1..=16).contains(
        &SettingsStore::new(dir.path().into())
            .unwrap()
            .get()
            .unwrap()
            .recording_limit_bytes
    ));
}

#[test]
fn recording_opt_in_persists_and_absent_settings_stay_off() {
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    assert!(!store.get().unwrap().save_recordings);
    store
        .put(&AppSettings {
            save_recordings: true,
            ..Default::default()
        })
        .unwrap();
    let reopened = SettingsStore::new(dir.path().into()).unwrap();
    assert!(reopened.get().unwrap().save_recordings);
    assert!(!dir.path().join("recordings").exists());
}

#[test]
fn oversized_dungeon_filters_preserve_readable_settings() {
    use maxed_app::settings::SETTINGS_INVALID;
    use maxed_types::AlertPreferences;
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    let original = AppSettings {
        update_channel: UpdateChannel::Beta,
        ..Default::default()
    };
    store.put(&original).unwrap();
    let path = dir.path().join("settings.json");
    let bytes = fs::read(&path).unwrap();
    for names in [
        vec!["x".repeat(256); 512],
        vec!["dungeon".into(); 257],
        vec!["x".repeat(65)],
    ] {
        assert_eq!(
            store
                .update_alert_preferences(AlertPreferences {
                    key_pop_dungeons: Some(names),
                    ..Default::default()
                })
                .unwrap_err(),
            SETTINGS_INVALID
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            SettingsStore::new(dir.path().into())
                .unwrap()
                .get()
                .unwrap(),
            original
        );
        assert!(!dir.path().join("settings.pending").exists());
    }
}

#[test]
fn encoded_settings_limit_includes_newline_and_preserves_both_write_paths() {
    use maxed_app::settings::SETTINGS_INVALID;
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    let mut original = AppSettings {
        assets_path: Some("C:\\".into()),
        ..Default::default()
    };
    let remaining = 64 * 1024 - (serde_json::to_vec(&original).unwrap().len() + 1);
    let path_value = original.assets_path.as_mut().unwrap();
    path_value.push_str(&"\\".repeat(remaining / 2));
    path_value.push_str(&"x".repeat(remaining % 2));
    validate(&original).unwrap();
    store.put(&original).unwrap();
    let path = dir.path().join("settings.json");
    let bytes = fs::read(&path).unwrap();
    assert_eq!(bytes.len(), 64 * 1024);
    assert_eq!(store.get().unwrap(), original);
    let mut preferences = original.alerts.clone();
    preferences.key_pop_dungeons = Some(vec!["Lost Halls".into()]);
    assert!(preferences.valid());
    assert_eq!(
        store.update_alert_preferences(preferences).unwrap_err(),
        SETTINGS_INVALID
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        SettingsStore::new(dir.path().into())
            .unwrap()
            .get()
            .unwrap(),
        original
    );
    let mut oversized = original.clone();
    oversized.assets_path.as_mut().unwrap().push('x');
    validate(&oversized).unwrap();
    assert_eq!(serde_json::to_vec(&oversized).unwrap().len(), 64 * 1024);
    assert_eq!(store.put(&oversized).unwrap_err(), SETTINGS_INVALID);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(store.get().unwrap(), original);
    assert!(!dir.path().join("settings.pending").exists());
}

#[test]
fn every_catalog_dungeon_filter_round_trips() {
    let catalog = maxed_assets::build_catalog(
        maxed_assets::ExtractedAssets {
            english: Default::default(),
            texts: vec![],
            textures: vec![],
            sprite_mapping: vec![],
        },
        "a".repeat(64),
    )
    .unwrap();
    let names = maxed_app::assets_bridge::catalog_dungeons(&catalog);
    assert!(!names.is_empty());
    assert!(names.len() <= 256);
    assert!(names.iter().all(|name| name.len() <= 64));
    let preferences = maxed_types::AlertPreferences {
        key_pop_dungeons: Some(names),
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    store.update_alert_preferences(preferences.clone()).unwrap();
    assert_eq!(
        SettingsStore::new(dir.path().into())
            .unwrap()
            .get()
            .unwrap()
            .alerts,
        preferences
    );
}

#[test]
fn settings_put_preserves_loot() {
    use maxed_types::{ChaseItem, Timestamp};
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    let mut stale = store.get().unwrap();
    let pin = ChaseItem {
        item_type: 3180,
        dungeon: None,
        pinned_at: Timestamp(1),
    };
    store.put_chase(vec![pin.clone()]).unwrap();
    stale.save_recordings = true;
    store.put(&stale).unwrap();
    let now = SettingsStore::new(dir.path().into())
        .unwrap()
        .get()
        .unwrap();
    assert_eq!(now.loot.chase, vec![pin]);
    assert!(now.save_recordings);
}

#[test]
fn settings_put_preserves_recap_image() {
    use maxed_types::RecapImageSettings;
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    let mut stale = store.get().unwrap();
    let saved = RecapImageSettings {
        names: false,
        loot: true,
        fights: false,
    };
    store.put_recap_image(saved.clone()).unwrap();
    stale.save_recordings = true;
    stale.recap_image = RecapImageSettings {
        names: true,
        loot: false,
        fights: true,
    };
    store.put(&stale).unwrap();
    let now = SettingsStore::new(dir.path().into())
        .unwrap()
        .get()
        .unwrap();
    assert_eq!(now.recap_image, saved);
    assert!(now.save_recordings);
}

#[test]
fn put_recap_image_patches_only_recap_image() {
    use maxed_types::{ChaseItem, RecapImageSettings, Timestamp};
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    let pin = ChaseItem {
        item_type: 3180,
        dungeon: None,
        pinned_at: Timestamp(1),
    };
    store
        .put(&AppSettings {
            update_channel: UpdateChannel::Beta,
            save_recordings: true,
            ..Default::default()
        })
        .unwrap();
    store.put_chase(vec![pin.clone()]).unwrap();
    let before = store.get().unwrap();
    for names in [false, true] {
        for loot in [false, true] {
            for fights in [false, true] {
                let recap_image = RecapImageSettings {
                    names,
                    loot,
                    fights,
                };
                store.put_recap_image(recap_image.clone()).unwrap();
                let now = SettingsStore::new(dir.path().into())
                    .unwrap()
                    .get()
                    .unwrap();
                assert_eq!(
                    now,
                    AppSettings {
                        recap_image,
                        ..before.clone()
                    }
                );
            }
        }
    }
    assert_eq!(store.get().unwrap().loot.chase, vec![pin]);
    assert!(!dir.path().join("settings.pending").exists());
}

#[test]
fn put_recap_image_never_overwrites_unreadable_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut newer = serde_json::to_value(AppSettings::default()).unwrap();
    newer["version"] = 2.into();
    let bytes = serde_json::to_vec(&newer).unwrap();
    fs::write(&path, &bytes).unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    assert_eq!(
        store
            .put_recap_image(maxed_types::RecapImageSettings {
                names: false,
                loot: false,
                fights: false,
            })
            .unwrap_err(),
        maxed_app::settings::SETTINGS_ERROR
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn put_chase_rejects_duplicates_and_oversize() {
    use maxed_app::settings::SETTINGS_ERROR;
    use maxed_types::{ChaseItem, Timestamp, MAX_CHASE_ITEMS};
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    let pin = ChaseItem {
        item_type: 1,
        dungeon: None,
        pinned_at: Timestamp(1),
    };
    store.put_chase(vec![pin.clone()]).unwrap();
    let old = fs::read(dir.path().join("settings.json")).unwrap();
    let mut duplicate = pin.clone();
    duplicate.pinned_at = Timestamp(2);
    let many = (0..=MAX_CHASE_ITEMS)
        .map(|i| ChaseItem {
            item_type: i as u16,
            ..pin.clone()
        })
        .collect();
    let mut invalid = vec![
        vec![pin.clone(), duplicate],
        many,
        vec![ChaseItem {
            pinned_at: Timestamp(i64::MAX as u64 + 1),
            ..pin.clone()
        }],
    ];
    for dungeon in [
        String::new(),
        "x".repeat(257),
        "é".repeat(129),
        "bad\nname".into(),
        "bad\u{007f}".into(),
    ] {
        invalid.push(vec![ChaseItem {
            dungeon: Some(dungeon),
            ..pin.clone()
        }]);
    }
    for items in invalid {
        let mut settings = store.get().unwrap();
        settings.loot.chase = items.clone();
        assert_eq!(validate(&settings).unwrap_err(), SETTINGS_ERROR);
        assert_eq!(store.put_chase(items).unwrap_err(), SETTINGS_ERROR);
        assert_eq!(fs::read(dir.path().join("settings.json")).unwrap(), old);
    }
    let items = (0..MAX_CHASE_ITEMS)
        .map(|i| ChaseItem {
            dungeon: Some(format!("{i:03}{}", "x".repeat(253))),
            pinned_at: Timestamp(i64::MAX as u64),
            ..pin.clone()
        })
        .collect::<Vec<_>>();
    store.put_chase(items.clone()).unwrap();
    assert_eq!(store.get().unwrap().loot.chase, items);
    store.put_chase(vec![]).unwrap();
    assert!(store.get().unwrap().loot.chase.is_empty());
}

#[test]
fn old_key_pop_settings_load_from_disk_and_round_trip_with_exclusions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut old = serde_json::to_value(maxed_types::AppSettings::default()).unwrap();
    old["alerts"]
        .as_object_mut()
        .unwrap()
        .remove("key_pop_excluded_dungeons");
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let store = SettingsStore::new(dir.path().into()).unwrap();
    let mut loaded = store.get().unwrap();
    assert!(loaded.alerts.key_pop_excluded_dungeons.is_empty());
    loaded.alerts.key_pop_excluded_dungeons = vec!["Lair of Shaitan".into()];
    store
        .update_alert_preferences(loaded.alerts.clone())
        .unwrap();
    let reopened = SettingsStore::new(dir.path().into())
        .unwrap()
        .get()
        .unwrap();
    assert_eq!(reopened, loaded);
    let persisted: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        persisted["alerts"]["key_pop_excluded_dungeons"],
        serde_json::json!(["Lair of Shaitan"])
    );
}
