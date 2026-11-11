use crate::common;
use maxed_assets::{build_catalog, ExtractedAssets, TextAsset};
use maxed_types::{BagDef, PotionStat};

#[test]
fn potion_stat_maps_all_24_stat_potion_names() {
    for (name, stat) in [
        ("Life", PotionStat::Life),
        ("Mana", PotionStat::Mana),
        ("Attack", PotionStat::Attack),
        ("Defense", PotionStat::Defense),
        ("Speed", PotionStat::Speed),
        ("Dexterity", PotionStat::Dexterity),
        ("Vitality", PotionStat::Vitality),
        ("Wisdom", PotionStat::Wisdom),
    ] {
        for potion in [
            format!("Potion of {name}"),
            format!("Potion of {name} (SB)"),
            format!("Greater Potion of {name}"),
        ] {
            assert_eq!(
                maxed_assets::rules::potion_stat(&potion),
                Some(stat),
                "{potion}"
            );
        }
    }
}

#[test]
fn potion_stat_accepts_greater_soulbound_pattern() {
    assert_eq!(
        maxed_assets::rules::potion_stat("Greater Potion of Life (SB)"),
        Some(PotionStat::Life)
    );
}

#[test]
fn potion_stat_rejects_other_items() {
    for name in [
        "Elixir of Life",
        "Potion of Lifelike",
        "potion of life",
        "Greater Potion of Life Fragment",
        "Potion of Luck",
        " Potion of Life",
        "Potion of Life ",
        "Greater Greater Potion of Life",
        "Potion of Life (SB) (SB)",
    ] {
        assert_eq!(maxed_assets::rules::potion_stat(name), None, "{name}");
    }
}

#[test]
fn extracted_catalog_sets_potion_stat() {
    let catalog = build_catalog(
        ExtractedAssets {
            english: Default::default(),
            texts: vec![TextAsset {
                name: "synthetic".into(),
                bytes: br#"<Objects>
                  <Object type="1" id="Potion of Speed"><Class>Equipment</Class></Object>
                  <Object type="2" id="Synth Sword"><Class>Equipment</Class></Object>
                </Objects>"#
                    .to_vec(),
            }],
            textures: vec![],
            sprite_mapping: vec![],
        },
        "a".repeat(64),
    )
    .unwrap();
    assert_eq!(catalog.items[0].potion_stat, Some(PotionStat::Speed));
    assert_eq!(catalog.items[1].potion_stat, None);
    let mut json = serde_json::to_value(&catalog).unwrap();
    assert_eq!(json["items"][0]["potion_stat"], "speed");
    assert_eq!(catalog, serde_json::from_value(json.clone()).unwrap());
    for item in json["items"].as_array_mut().unwrap() {
        item.as_object_mut().unwrap().remove("potion_stat");
    }
    let old: maxed_types::Catalog = serde_json::from_value(json).unwrap();
    assert!(old.items.iter().all(|item| item.potion_stat.is_none()));
}

#[test]
fn catalog_populates_new_fields() {
    let catalog = build_catalog(
        ExtractedAssets {
            english: Default::default(),
            texts: vec![TextAsset {
                name: "synthetic".into(),
                bytes: br#"<Objects>
                  <Object type="1" id="Synth Sword"><Class>Equipment</Class><Soulbound/>
                    <ActivateOnEquip stat="DEF" amount="4">IncrementStat</ActivateOnEquip>
                    <ActivateOnEquip stat="21" amount="-1">IncrementStat</ActivateOnEquip>
                    <ActivateOnEquip stat="HP" amount="20">IncrementStat</ActivateOnEquip>
                    <ActivateOnEquip stat="UNKNOWN" amount="5">IncrementStat</ActivateOnEquip>
                  </Object>
                  <Object type="2" id="Synth Ring"><Class>Equipment</Class></Object>
                  <Object type="3" id="Synth Portal"><Class>Portal</Class><DungeonName>Synth Cave</DungeonName></Object>
                  <Object type="4" id="Grand Sphinx"><Class>Character</Class></Object>
                  <Object type="5" id="grand sphinx"><DungeonName>Not a portal</DungeonName></Object>
                </Objects>"#.to_vec(),
            }],
            textures: vec![],
            sprite_mapping: vec![],
        },
        "a".repeat(64),
    ).unwrap();
    assert_eq!(catalog.items[0].soulbound, Some(true));
    assert_eq!(catalog.items[1].soulbound, Some(false));
    assert_eq!(
        catalog.items[0].stat_bonuses,
        Some(vec![
            maxed_types::StatView {
                stat_id: 0,
                value: 20
            },
            maxed_types::StatView {
                stat_id: 21,
                value: 3
            },
        ])
    );
    assert_eq!(catalog.items[1].stat_bonuses, Some(vec![]));
    assert_eq!(catalog.objects[2].class_name.as_deref(), Some("Portal"));
    assert_eq!(
        catalog.objects[2].dungeon_name.as_deref(),
        Some("Synth Cave")
    );
    assert_eq!(catalog.objects[3].event_boss, Some(true));
    assert_eq!(catalog.objects[4].event_boss, Some(false));
    assert_eq!(catalog.objects[4].class_name, None);
    assert_eq!(catalog.objects[4].dungeon_name, None);
    assert_eq!(
        catalog,
        serde_json::from_slice(&serde_json::to_vec(&catalog).unwrap()).unwrap()
    );
}

#[test]
fn cache_version_bumped_invalidates_old_generation() {
    use maxed_assets::{load_or_extract, AssetPaths, AssetProgress};
    use sha2::{Digest, Sha256};
    use std::{fs, sync::atomic::AtomicBool};

    let dir = tempfile::tempdir().unwrap();
    let bytes = common::fixture();
    let paths = AssetPaths {
        resources: dir.path().join("resources.assets"),
        app_data: dir.path().join("app"),
    };
    fs::write(&paths.resources, &bytes).unwrap();
    // Reproduce the v8 fingerprint with the SAME rules and resource bytes,
    // isolating the cache version from the automatic rules-source invalidation.
    let mut hash = Sha256::new();
    hash.update(b"Maxed-assets\0reader-2\0catalog-1\0cache-version\0");
    hash.update(8u32.to_le_bytes());
    hash.update(b"rules-sha256\0");
    hash.update(Sha256::digest(include_bytes!("../src/rules.rs")));
    hash.update(1u32.to_le_bytes());
    hash.update(16u32.to_le_bytes());
    hash.update(b"resources.assets");
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(format!("{:x}", Sha256::digest(&bytes)).as_bytes());
    let old_fingerprint = format!("{:x}", hash.finalize());
    let root = paths.app_data.join("assets/v1");
    let old = root.join(&old_fingerprint);
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("catalog.json"), b"old generation must not be read").unwrap();
    fs::write(root.join("current"), &old_fingerprint).unwrap();
    let mut phases = vec![];
    let cached = load_or_extract(&paths, &AtomicBool::new(false), &mut |p| phases.push(p)).unwrap();
    assert_ne!(cached.catalog.client_fingerprint, old_fingerprint);
    assert!(phases.contains(&AssetProgress::Reading));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(cached.root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], 14);
    assert_eq!(
        fs::read_to_string(root.join("current")).unwrap(),
        cached.catalog.client_fingerprint
    );
    assert_eq!(
        fs::read(old.join("catalog.json")).unwrap(),
        b"old generation must not be read"
    );
}

#[test]
fn cache_rebuilds_manifest_from_before_consumable_tier_rules() {
    use maxed_assets::{load_or_extract, AssetPaths, AssetProgress};
    use std::{fs, sync::atomic::AtomicBool};

    let dir = tempfile::tempdir().unwrap();
    let paths = AssetPaths {
        resources: dir.path().join("resources.assets"),
        app_data: dir.path().join("app"),
    };
    fs::write(&paths.resources, common::fixture()).unwrap();
    let flag = AtomicBool::new(false);
    let cached = load_or_extract(&paths, &flag, &mut |_| {}).unwrap();
    let manifest_path = cached.root.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["version"] = 8.into();
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    drop(cached);

    let mut phases = vec![];
    let rebuilt = load_or_extract(&paths, &flag, &mut |p| phases.push(p)).unwrap();
    assert!(phases.contains(&AssetProgress::Reading));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(rebuilt.root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["version"], 14);
    phases.clear();
    load_or_extract(&paths, &flag, &mut |p| phases.push(p)).unwrap();
    assert!(!phases.contains(&AssetProgress::Reading));
}

#[test]
fn brown_pink_and_soulbound_capture_flags_are_disabled() {
    for bag in bags() {
        assert_eq!(
            bag.capture,
            !matches!(bag.object_type, 1280 | 1283 | 1286 | 1709 | 1710)
        );
    }
}

fn bags() -> Vec<BagDef> {
    build_catalog(
        ExtractedAssets {
            english: Default::default(),
            texts: vec![TextAsset {
                name: "synthetic".into(),
                bytes: b"<Objects/>".to_vec(),
            }],
            textures: vec![],
            sprite_mapping: vec![],
        },
        "a".repeat(64),
    )
    .unwrap()
    .bags
}

#[test]
fn white_bags_keep_rank_255() {
    let bags = bags();
    let whites: Vec<_> = bags.iter().filter(|b| b.is_white).collect();
    assert_eq!(whites.len(), 2);
    assert!(whites.iter().all(|b| b.rank == Some(255)));
}

#[test]
fn bag_ranks_follow_game_order() {
    let bags = bags();
    for (base, rank) in [
        (1280, 10),
        (1286, 20),
        (1287, 30),
        (1288, 40),
        (1289, 50),
        (1291, 60),
        (1294, 70),
        (1295, 80),
        (1708, 90),
        (1292, 255),
    ] {
        let variants: Vec<_> = bags.iter().filter(|b| b.base_type == base).collect();
        assert_eq!(variants.len(), 2);
        assert!(variants.iter().all(|b| b.rank == Some(rank)));
    }
    assert_eq!(
        bags.iter().find(|b| b.object_type == 1283).unwrap().rank,
        None
    );
}

#[test]
fn realm_encounter_xml_marks_bosses_and_keeps_helpers_ordinary() {
    let catalog = build_catalog(
        ExtractedAssets {
            english: Default::default(),
            texts: vec![TextAsset {
                name: "synthetic-encounters".into(),
                bytes: include_bytes!("../../../tests/fixtures/scrubbed/realm-encounters.xml")
                    .to_vec(),
            }],
            textures: vec![],
            sprite_mapping: vec![],
        },
        "a".repeat(64),
    )
    .unwrap();
    for object in &catalog.objects {
        let encounter =
            [10, 22146, 45104, 34565, 47930, 24005, 0x0d50, 0x55ec].contains(&object.object_type);
        assert_eq!(object.event_boss, Some(encounter), "{}", object.name);
        assert!(!object.is_boss && !object.is_miniboss, "{}", object.name);
        assert_eq!(
            object.realm_event_key.is_some(),
            encounter,
            "{}",
            object.name
        );
    }
    assert_eq!(
        catalog
            .objects
            .iter()
            .find(|o| o.object_type == 47930)
            .unwrap()
            .realm_event_key
            .as_deref(),
        Some("Big_Barnacle")
    );
    let mut json = serde_json::to_value(&catalog).unwrap();
    assert_eq!(catalog, serde_json::from_value(json.clone()).unwrap());
    for object in json["objects"].as_array_mut().unwrap() {
        object.as_object_mut().unwrap().remove("realm_event_key");
    }
    let old: maxed_types::Catalog = serde_json::from_value(json).unwrap();
    assert!(old.objects.iter().all(|o| o.realm_event_key.is_none()));
    maxed_assets::validate_catalog(&old).unwrap();
}

#[test]
fn dungeon_portal_type_is_derived_from_portal_dungeon_names() {
    let catalog = build_catalog(
        ExtractedAssets {
            english: Default::default(),
            texts: vec![TextAsset {
                name: "synthetic".into(),
                bytes: br#"<Objects>
                  <Object type="40" id="Synth Cave Portal B"><Class>Portal</Class><DungeonPortal/><DungeonName>Synth Cave</DungeonName></Object>
                  <Object type="30" id="Synth Cave Portal A"><Class>Portal</Class><DungeonPortal/><DungeonName>Synth Cave</DungeonName></Object>
                  <Object type="50" id="Synth Lair Portal"><Class>Portal</Class><DungeonPortal/><DungeonName>Synth Lair</DungeonName></Object>
                  <Object type="51" id="Synth Lair Event Portal"><Class>Portal</Class><DungeonPortal/><DisplayId>Synth Lair (Event)</DisplayId><DungeonName>Synth Lair</DungeonName></Object>
                  <Object type="60" id="Not A Portal"><Class>GameObject</Class><DungeonName>Synth Ruin</DungeonName></Object>
                  <Object type="70" id="Synth Cave Key"><Class>Equipment</Class><Item/><Activate id="Synth Cave Portal A">CreatePortal</Activate></Object>
                  <Object type="80" id="Snowball Solo Portal"><Class>Portal</Class><DungeonName>Snowball Rolling Solo</DungeonName></Object>
                </Objects>"#.to_vec(),
            }],
            textures: vec![],
            sprite_mapping: vec![],
        },
        "a".repeat(64),
    )
    .unwrap();
    let portal = |name: &str| {
        catalog
            .dungeons
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.portal_type)
    };
    // Lowest object type wins regardless of XML order.
    assert_eq!(portal("Synth Cave"), Some(Some(30)));
    assert_eq!(portal("Synth Lair"), Some(Some(50)));
    // A portal naming a reviewed alias maps to the canonical area.
    assert_eq!(portal("Snowball Rolling (Map 1)"), Some(Some(80)));
    // Reviewed areas without a portal in this XML keep no mapping.
    assert_eq!(portal("Nexus"), Some(None));
    // DungeonName on a non-portal object never creates or maps an area.
    assert_eq!(portal("Synth Ruin"), None);
    assert_eq!(
        catalog,
        serde_json::from_slice(&serde_json::to_vec(&catalog).unwrap()).unwrap()
    );
}

#[test]
fn dungeon_portal_type_must_open_that_dungeon() {
    let mut catalog = build_catalog(
        ExtractedAssets {
            english: Default::default(),
            texts: vec![TextAsset {
                name: "synthetic".into(),
                bytes: br#"<Objects>
                  <Object type="30" id="Synth Cave Portal"><Class>Portal</Class><DungeonPortal/><DungeonName>Synth Cave</DungeonName></Object>
                  <Object type="31" id="Synth Rock"><Class>GameObject</Class></Object>
                </Objects>"#
                    .to_vec(),
            }],
            textures: vec![],
            sprite_mapping: vec![],
        },
        "a".repeat(64),
    )
    .unwrap();
    maxed_assets::validate_catalog(&catalog).unwrap();
    let cave = catalog
        .dungeons
        .iter_mut()
        .find(|d| d.name == "Synth Cave")
        .unwrap();
    cave.portal_type = Some(31);
    assert!(maxed_assets::validate_catalog(&catalog).is_err());
    let cave = catalog
        .dungeons
        .iter_mut()
        .find(|d| d.name == "Synth Cave")
        .unwrap();
    cave.portal_type = Some(999);
    assert!(maxed_assets::validate_catalog(&catalog).is_err());
}
