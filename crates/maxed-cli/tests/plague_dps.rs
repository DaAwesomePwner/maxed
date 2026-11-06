use crate::combat_fixture;
use maxed_cli::demo::replay_engine;
use maxed_engine::EngineConfig;
use maxed_types::EngineEvent;
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

#[test]
fn plague_doctor_local_dps_uses_exact_local_hit_window() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-realm-plague-doctor");
    let expected: Value =
        serde_json::from_slice(&std::fs::read(dir.join("expected.json")).unwrap()).unwrap();
    let catalog = combat_fixture::catalog(34489, "The Plague Doctor", true);
    let events = replay_engine(
        &dir.join(expected["pcap"].as_str().unwrap()),
        Arc::new(catalog),
        EngineConfig::default(),
    )
    .unwrap();
    let fight = events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::FightEnded(f) if f.target_type == Some(34489) => Some(f),
            _ => None,
        })
        .next_back()
        .unwrap();
    let local = fight.players.iter().find(|p| p.is_local).unwrap();
    let case = &expected["dps_window_case_for_35"]["local"];
    let first: u64 = case["first_local_enemyhit_us"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let last: u64 = case["last_local_enemyhit_us"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(local.damage, case["damage"].as_f64().unwrap());
    assert!(
        (local.dps.unwrap() - local.damage * 1_000_000. / (last - first) as f64).abs() < 0.000_001
    );
}
