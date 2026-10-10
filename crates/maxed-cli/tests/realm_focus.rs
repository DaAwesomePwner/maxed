use maxed_cli::{demo::replay_engine, Pipeline};
use maxed_engine::EngineConfig;
use maxed_protocol::{DecodeEvent, Packet};
use maxed_recording::PcapReader;
use maxed_types::{Catalog, EngineEvent};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

#[test]
fn scrubbed_realm_replay_exposes_local_recency_and_current_quest() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scrubbed");
    let fixture = root.join("maxed-realm-focus");
    let expected: Value =
        serde_json::from_slice(&fs::read(fixture.join("expected.json")).unwrap()).unwrap();
    let local_id = expected["visit"]["local_object_id"].as_i64().unwrap() as i32;
    let mut pipeline = Pipeline::default();
    let mut reader =
        PcapReader::new(fs::File::open(fixture.join("realm-visit.pcap")).unwrap()).unwrap();
    let mut local_type = None;
    let mut weapons = std::collections::BTreeSet::new();
    while let Some(frame) = reader.next_frame().unwrap() {
        for event in pipeline.push(frame) {
            if let DecodeEvent::Packet(packet) = event {
                if let Packet::PlayerShoot(shot) = &packet.packet {
                    weapons.insert(shot.weaponId);
                }
                if let Packet::Update(update) = packet.packet {
                    for object in update.newObjects {
                        if object.status.objectId == local_id {
                            local_type = Some(object.objectType);
                        }
                    }
                }
            }
        }
    }
    // Minimal test catalog: only documented targets and the captured local class.
    // It contains no client assets and does not claim reconstructed weapon damage.
    let mut catalog: Value =
        serde_json::from_slice(&fs::read(root.join("maxed-demo/catalog.json")).unwrap()).unwrap();
    let targets = expected["boss_tier_targets"].as_array().unwrap();
    let mut objects: Vec<Value> = targets
        .iter()
        .map(|target| {
            json!({
                "object_type": target["target_type"], "name": target["name"], "is_player": false,
                "is_boss": false, "is_miniboss": false, "event_boss": false, "sprite_key": null
            })
        })
        .collect();
    objects.push(
        json!({"object_type": local_type.unwrap(), "name": "Fixture local class",
        "is_player": true, "is_boss": false, "is_miniboss": false, "sprite_key": null}),
    );
    catalog["objects"] = json!(objects);
    catalog["classes"] = json!([{"object_type": local_type.unwrap(), "stat_caps": []}]);
    // Arbitrary synthetic weapon damage enables ENEMYHIT attribution without
    // distributing client-derived metadata. Only hit timing is asserted.
    catalog["items"] = json!(weapons
        .iter()
        .map(|ty| json!({
            "object_type": ty, "name": "Synthetic timing weapon", "tier": null,
            "slot_type": 1, "projectiles": [{"index":0,"min_damage":1000,"max_damage":1000,
            "armor_piercing":true,"lifetime_millis":10000}], "scaling":null,"sprite_key":null
        }))
        .collect::<Vec<_>>());
    catalog["dungeons"] = json!([{"name":"Realm of the Mad God", "aliases":[], "kind":"realm", "completion_index":null}]);
    let catalog: Catalog = serde_json::from_value(catalog).unwrap();
    let events = replay_engine(
        &fixture.join("realm-visit.pcap"),
        Arc::new(catalog),
        EngineConfig {
            stream_id: "realm-focus-regression".into(),
            identity_key: [7; 32],
            ..Default::default()
        },
    )
    .unwrap();
    let mut first_local = BTreeMap::new();
    let mut last_local = BTreeMap::new();
    let mut quest_seen = BTreeMap::new();
    let mut quest_released = BTreeMap::new();
    for event in &events {
        let f = match &event.event {
            EngineEvent::FightStarted(f)
            | EngineEvent::FightUpdated(f)
            | EngineEvent::FightEnded(f) => f,
            _ => continue,
        };
        if let Some(hit) = f.local_last_hit {
            first_local.entry(f.target_id).or_insert(hit);
            last_local.insert(f.target_id, hit);
        }
        if f.is_current_quest {
            quest_seen.insert(f.target_id, true);
        }
        if !f.is_current_quest && quest_seen.contains_key(&f.target_id) && f.ended.is_none() {
            quest_released.insert(f.target_id, true);
        }
    }
    let mut observations = Vec::new();
    for target in targets {
        let id = target["target_id"].as_i64().unwrap() as i32;
        let has_local = target["local_damage"].as_f64().unwrap() > 0.;
        assert_eq!(
            first_local.contains_key(&id),
            has_local,
            "{}",
            target["name"]
        );
        if target["observed_target_kind"] == "quest" {
            assert!(quest_seen.contains_key(&id), "{}", target["name"]);
        }
        observations.push(json!({"target_id": id, "name": target["name"],
            "first_published_local_hit_us": first_local.get(&id), "last_local_hit_us": last_local.get(&id)}));
    }
    assert!(quest_released.contains_key(&244589)); // Maze -> another arrow inside this visit.
    let actual = json!(observations);
    let path = fixture.join("focus-hit-observations.json");
    if std::env::var_os("UPDATE_REALM_FOCUS_FIXTURE").is_some() {
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&actual).unwrap()),
        )
        .unwrap();
    }
    let recorded: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(actual, recorded);
}
