use maxed_cli::demo::replay_engine;
use maxed_engine::EngineConfig;
use maxed_types::{Catalog, EngineEvent, FightEnd, KillEvidence, Timestamp};
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

#[test]
fn shaitan_fight_ends_at_victory_not_map_exit() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-shaitan-run");
    let expected: Value =
        serde_json::from_slice(&std::fs::read(dir.join("expected.json")).unwrap()).unwrap();
    let boss = &expected["boss_fight"];
    let catalog: Catalog = serde_json::from_value(serde_json::json!({
        "format_version": 1, "client_fingerprint": "synthetic-shaitan-regression",
        "items": [], "classes": [], "enchants": [], "bags": [], "sprites": [], "tiles": [], "crucible_bonuses": [],
        "dungeons": [{ "name": expected["run"]["map"], "aliases": [], "kind": "dungeon", "completion_index": null }],
        "objects": [{
            "object_type": boss["target_type"], "name": boss["name"], "is_player": false,
            "is_boss": boss["catalog_flags"]["is_boss"],
            "is_miniboss": boss["catalog_flags"]["is_miniboss"],
            "event_boss": boss["catalog_flags"]["event_boss"], "sprite_key": null
        }]
    })).unwrap();
    let events = replay_engine(
        &dir.join(expected["pcap"].as_str().unwrap()),
        Arc::new(catalog),
        EngineConfig::default(),
    )
    .unwrap();
    let fight = events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::FightEnded(f) if f.target_type == Some(28058) => Some(f),
            _ => None,
        })
        .next_back()
        .unwrap();
    let at = Timestamp(
        expected["run"]["completed_at_us"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
    );
    assert_eq!(at, Timestamp(1791143305741000));
    assert_eq!(
        fight.kill.as_ref().unwrap().evidence,
        KillEvidence::VictoryQuestTarget
    );
    assert!(!fight.kill.as_ref().unwrap().provisional);
    assert_eq!(fight.kill.as_ref().unwrap().at, at);
    let run = events
        .iter()
        .find_map(|e| match &e.event {
            EngineEvent::RunEnded(run)
                if run.dungeon == expected["run"]["map"].as_str().unwrap() =>
            {
                Some(run)
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(run.completed_at, Some(at));
    assert_eq!(fight.ended, run.completed_at);
    assert_eq!(fight.end_reason, Some(FightEnd::Killed));
}

use crate::combat_fixture;

#[test]
fn shaitan_recorded_sources_are_reconstructed_without_false_uncertainty() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-shaitan-run");
    let expected: Value =
        serde_json::from_slice(&std::fs::read(dir.join("expected.json")).unwrap()).unwrap();
    let events = replay_engine(
        &dir.join(expected["pcap"].as_str().unwrap()),
        Arc::new(combat_fixture::catalog(28058, "Shaitan the Advisor", false)),
        EngineConfig::default(),
    )
    .unwrap();
    let fight = events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::FightEnded(f) if f.target_type == Some(28058) => Some(f),
            _ => None,
        })
        .next_back()
        .unwrap();
    let local = fight.players.iter().find(|p| p.is_local).unwrap();
    for item in [588, 589] {
        let sources: Vec<_> = local
            .sources
            .iter()
            .filter(|s| s.item_type == Some(item))
            .collect();
        assert!(!sources.is_empty());
        assert!(sources
            .iter()
            .all(|s| s.enchant.state == maxed_types::EnchantState::Recorded));
        assert!(sources.iter().all(|s| !s.estimated), "{sources:?}");
    }
    assert!(!fight
        .issues
        .contains(&maxed_types::CoverageIssue::MissingStats));
    // This reconnect fixture cannot establish complete capture or identity.
}

#[test]
fn shaitan_typed_target_establishes_only_secondary_condition_omission() {
    use maxed_cli::Pipeline;
    use maxed_protocol::{DecodeEvent, Packet};
    use maxed_recording::PcapReader;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-shaitan-run/lair-of-shaitan-run.pcap");
    let mut reader = PcapReader::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut pipeline = Pipeline::default();
    let mut observed = false;
    while let Some(frame) = reader.next_frame().unwrap() {
        for event in pipeline.push(frame) {
            let DecodeEvent::Packet(packet) = event else {
                continue;
            };
            let Packet::Update(update) = packet.packet else {
                continue;
            };
            for object in update.newObjects.iter().filter(|o| o.objectType == 28058) {
                let has = |id| object.status.stats.iter().any(|s| s.statTypeNum == id);
                assert!(has(21));
                assert!(has(29));
                assert!(!has(96));
                observed = true;
            }
        }
    }
    assert!(observed);
}
