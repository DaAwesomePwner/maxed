//! Retained fixture observations, not evidence that selects exalt placement.
use maxed_cli::{demo::replay_engine, Pipeline};
use maxed_engine::EngineConfig;
use maxed_protocol::{DecodeEvent, Packet};
use maxed_recording::PcapReader;
use maxed_types::{Catalog, EngineEvent, StatSplitReason};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::Arc,
};

fn check_fixture(name: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/scrubbed");
    let dir = root.join(name);
    let metadata: Value =
        serde_json::from_slice(&fs::read(dir.join("expected.json")).unwrap()).unwrap();
    let path = dir.join(metadata["pcap"].as_str().unwrap());
    let mut pipeline = Pipeline::default();
    let mut reader = PcapReader::new(fs::File::open(&path).unwrap()).unwrap();
    let mut local = BTreeMap::new();
    let mut retained = BTreeMap::<_, BTreeMap<u8, i32>>::new();
    let mut equipment = BTreeSet::new();
    let mut complete = 0;
    while let Some(frame) = reader.next_frame().unwrap() {
        for event in pipeline.push(frame) {
            let DecodeEvent::Packet(packet) = event else {
                continue;
            };
            assert!(packet.alignment_verified);
            let statuses = match packet.packet {
                Packet::CreateSuccess(p) => {
                    local.insert(packet.conn.0, p.objectId);
                    retained.remove(&packet.conn.0);
                    vec![]
                }
                Packet::Update(p) => p.newObjects.into_iter().map(|o| o.status).collect(),
                Packet::NewTick(p) => p.status,
                _ => vec![],
            };
            for status in statuses {
                if local.get(&packet.conn.0) != Some(&status.objectId) {
                    continue;
                }
                let stats = retained.entry(packet.conn.0).or_default();
                for s in status.stats {
                    if s.stringStatValue.is_none() {
                        stats.insert(s.statTypeNum, s.statValue);
                    }
                }
                if !(105..=113).all(|id| stats.contains_key(&id)) {
                    continue;
                }
                assert_eq!(
                    (105..=112).map(|id| stats[&id]).collect::<Vec<_>>(),
                    [2, 1, 2, 0, 1, 1, 0, 0]
                );
                assert_eq!(stats[&113], 1000); // Constant observation only, no multiplier interpretation.
                if let Some(slots) = (8..=11)
                    .map(|id| stats.get(&id).copied())
                    .collect::<Option<Vec<_>>>()
                {
                    equipment.insert(slots);
                }
                complete += 1;
            }
        }
    }
    assert!(complete > 0);
    assert!(!equipment.is_empty());
    // Synthetic class recognition only: no client catalog/caps/gear claims.
    let mut catalog: Value =
        serde_json::from_slice(&fs::read(root.join("maxed-demo/catalog.json")).unwrap()).unwrap();
    catalog["objects"] = json!(maxed_protocol::privacy::PLAYER_CLASSES
        .iter()
        .map(|(ty, _)| json!({
            "object_type": ty, "name": "Synthetic class", "is_player": true,
            "is_boss": false, "is_miniboss": false, "sprite_key": null
        }))
        .collect::<Vec<_>>());
    catalog["classes"] = json!([]);
    catalog["items"] = json!([]);
    let catalog: Catalog = serde_json::from_value(catalog).unwrap();
    let events = replay_engine(
        &path,
        Arc::new(catalog),
        EngineConfig {
            stream_id: "character-evidence".into(),
            identity_key: [7; 32],
            ..Default::default()
        },
    )
    .unwrap();
    let mut checked = 0;
    for event in events {
        let EngineEvent::CharacterUpdated(c) = event.event else {
            continue;
        };
        if !(105..=113).all(|id| c.stats.iter().any(|s| s.stat_id == id)) {
            continue;
        }
        assert!(c.maxed.is_none());
        let panel = c.stat_panel.unwrap();
        assert!(panel.breakdown_provisional);
        for (id, amount) in [
            (20, 2),
            (21, 1),
            (22, 2),
            (26, 0),
            (27, 1),
            (28, 1),
            (0, 0),
            (3, 0),
        ] {
            let line = panel.lines.iter().find(|s| s.stat_id == id).unwrap();
            assert_eq!(line.exalt, Some(amount));
            assert_eq!(line.at_cap, None);
            if amount != 0 {
                assert!(line
                    .split_evidence
                    .as_ref()
                    .unwrap()
                    .reasons
                    .contains(&StatSplitReason::UnverifiedPlacement));
            }
        }
        let slots = c
            .equipment
            .iter()
            .map(|slot| {
                if slot.observed_empty == Some(true) {
                    -1
                } else {
                    i32::from(slot.item_type.unwrap())
                }
            })
            .collect::<Vec<_>>();
        assert!(equipment.contains(&slots));
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn shaitan_retains_observed_exalt_and_equipment() {
    check_fixture("maxed-shaitan-run");
}
#[test]
fn realm_focus_retains_observed_exalt_and_equipment() {
    check_fixture("maxed-realm-focus");
}
#[test]
fn realm_return_retains_observed_exalt_and_equipment() {
    check_fixture("maxed-realm-dungeon-return");
}
#[test]
fn nexus_retains_observed_exalt_and_equipment() {
    check_fixture("maxed-nexus-key-pop");
}
