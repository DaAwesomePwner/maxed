use maxed_types::*;

#[test]
fn aliases_and_curated_values() {
    for (alias, canonical) in [
        ("ICE CAVE", "Ice Citadel"),
        ("advanced nest", "Plagued Nest"),
        ("ASW", "Advanced Kogbold Steamworks"),
        ("LH", "Lost Halls"),
    ] {
        assert_eq!(dungeon_meta(alias), dungeon_meta(canonical));
        assert!(dungeon_meta(alias).is_some());
    }
    assert_eq!(dungeon_meta("Ice Cave").unwrap().difficulty, 7.0);
    assert_eq!(
        dungeon_meta("Advanced Nest").unwrap().category,
        DungeonCategory::Advanced
    );
    for meta in DUNGEON_META {
        assert!((0.0..=10.0).contains(&meta.difficulty));
        assert_eq!((meta.difficulty * 2.0).fract(), 0.0);
        for name in std::iter::once(&meta.name).chain(meta.aliases) {
            assert_eq!(dungeon_meta(name), Some(meta));
        }
    }
    assert!(dungeon_meta("Tutorial").is_none());
}

fn all() -> AlertPreferences {
    AlertPreferences {
        key_pop_dungeons: None,
        ..Default::default()
    }
}

#[test]
fn legacy_settings_keep_both_filter_modes() {
    for selected in [None, Some(vec!["Pirate Cave".into()])] {
        let original = AlertPreferences {
            key_pop_dungeons: selected,
            key_pop_excluded_dungeons: vec!["The Nest".into()],
            ..Default::default()
        };
        let mut json = serde_json::to_value(&original).unwrap();
        json.as_object_mut()
            .unwrap()
            .remove("key_pop_min_difficulty");
        json.as_object_mut().unwrap().remove("key_pop_exalt_only");
        let legacy: AlertPreferences = serde_json::from_value(json).unwrap();
        assert_eq!(legacy, original);
        for name in ["Pirate Cave", "The Nest", "Unknown"] {
            assert_eq!(
                legacy.includes_key_pop_dungeon(name),
                original.includes_key_pop_dungeon(name)
            );
        }
    }
}

#[test]
fn minimum_exalt_and_exclusions_combine() {
    let mut prefs = AlertPreferences {
        key_pop_min_difficulty: Some(7.5),
        ..all()
    };
    assert!(prefs.includes_key_pop_dungeon("Unknown"));
    assert!(!prefs.includes_key_pop_dungeon("Ice Cave"));
    assert!(prefs.includes_key_pop_dungeon("Fungal Cavern"));
    prefs.key_pop_exalt_only = true;
    assert!(!prefs.includes_key_pop_dungeon("Unknown"));
    assert!(prefs.includes_key_pop_dungeon("Fungal Cavern"));
    assert!(prefs.includes_key_pop_dungeon("Advanced Nest"));
    assert!(!prefs.includes_key_pop_dungeon("Oryxmania"));
    prefs.key_pop_excluded_dungeons.push("Fungal Cavern".into());
    assert!(!prefs.includes_key_pop_dungeon("Fungal Cavern"));
    prefs.key_pop_min_difficulty = None;
    assert!(prefs.includes_key_pop_dungeon("Ice Cave"));
    assert!(!prefs.includes_key_pop_dungeon("Pirate Cave"));
}

#[test]
fn selected_dungeons_ignore_rules_and_exclusions() {
    let prefs = AlertPreferences {
        key_pop_dungeons: Some(vec!["Pirate Cave".into(), "Unknown".into()]),
        key_pop_excluded_dungeons: vec!["Pirate Cave".into()],
        key_pop_min_difficulty: Some(10.0),
        key_pop_exalt_only: true,
        ..Default::default()
    };
    assert!(prefs.includes_key_pop_dungeon("Pirate Cave"));
    assert!(prefs.includes_key_pop_dungeon("Unknown"));
    assert!(!prefs.includes_key_pop_dungeon("The Shatters"));
}

#[test]
fn validates_difficulty() {
    for value in [10.5, -1.0, 7.25, f32::NAN, f32::INFINITY] {
        let prefs = AlertPreferences {
            key_pop_min_difficulty: Some(value),
            ..all()
        };
        assert!(!prefs.valid());
        if value.is_finite() {
            assert!(serde_json::from_value::<AlertPreferences>(
                serde_json::to_value(prefs).unwrap()
            )
            .is_err());
        }
    }
    for half in 0..=20 {
        assert!(AlertPreferences {
            key_pop_min_difficulty: Some(half as f32 / 2.0),
            ..all()
        }
        .valid());
    }
}

#[test]
fn legacy_catalog_metadata_defaults_to_unknown() {
    let dungeon: DungeonDef = serde_json::from_str(
        r#"{"name":"Old","aliases":[],"kind":"dungeon","completion_index":null}"#,
    )
    .unwrap();
    assert_eq!(dungeon.difficulty, None);
    assert_eq!(dungeon.category, None);
    assert!(!dungeon.exalt);
    assert!(!dungeon.exalt_gateway);
}
