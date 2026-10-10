//! Evidence for #11. Packet indices are zero-based maxed-cli decode indices.
use maxed_cli::{demo::replay_engine, Pipeline, ReplayReport};
use maxed_engine::EngineConfig;
use maxed_protocol::{canonical_line, CanonicalLine, DecodeEvent};
use maxed_recording::PcapReader;
use maxed_types::{Catalog, EngineEvent, KillEvidence, ObjectDef};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-realm-focus")
        .join(name)
}

fn decoded() -> Vec<CanonicalLine> {
    let bytes = std::fs::read(fixture("realm-visit.pcap")).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "c5ffe28bf67eba20e0d4b97940bc7a4da42d7c706ad7aaaaf1f7eb98acd3b29c"
    );
    let mut reader = PcapReader::new(bytes.as_slice()).unwrap();
    let mut pipeline = Pipeline::default();
    let mut events = Vec::new();
    while let Some(frame) = reader.next_frame().unwrap() {
        events.extend(pipeline.push(frame));
    }
    events.extend(pipeline.finish());
    let mut report = ReplayReport::default();
    let mut lines = Vec::new();
    for event in events {
        report.observe(&event);
        if let DecodeEvent::Packet(p) = event {
            assert!(p.alignment_verified);
            lines.push(canonical_line(lines.len() as u64, &p).unwrap());
        }
    }
    assert_eq!(report.failures, 0);
    assert_eq!(lines.len(), 28_206);
    lines
}

fn hp(lines: &[CanonicalLine], target: i64) -> Vec<(u64, u64, i64)> {
    lines
        .iter()
        .filter(|p| p.name == "NEWTICK")
        .flat_map(|p| {
            p.f["status"]
                .as_array()
                .unwrap()
                .iter()
                .filter(move |s| s["objectId"] == target)
                .flat_map(move |s| {
                    s["stats"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|stat| stat["statTypeNum"] == 1)
                        .map(move |stat| (p.i, p.t, stat["statValue"].as_i64().unwrap()))
                })
        })
        .collect()
}

#[test]
fn quest_terminals_are_one_hp_without_victory_or_quest_clear() {
    let lines = decoded();
    assert_eq!(
        lines.iter().filter(|p| p.name == "NOTIFICATION").count(),
        811
    );
    assert!(!lines
        .iter()
        .any(|p| p.name == "NOTIFICATION" && p.f["effect"] == "Victory"));
    assert_eq!(lines.iter().filter(|p| p.name == "QUESTOBJID").count(), 11);
    assert!(!lines
        .iter()
        .any(|p| p.name == "QUESTOBJID" && p.f["objectId"] == -1));
    // target, preceding HP, first/last HP1 indices, HP1 count, drop index, last-HP-to-drop us.
    for (id, before, first, last, count, drop, delay) in [
        (244589, 3567, 3154, 3223, 3, 3310, 606219),
        (252023, 2835, 13327, 13327, 1, 13328, 0),
        (239725, 407, 25975, 25999, 3, 26033, 603868),
        (251628, 969, 24162, 24563, 11, 24564, 0),
    ] {
        let samples = hp(&lines, id);
        assert!(!samples.iter().any(|s| s.2 == 0));
        let ones: Vec<_> = samples.iter().filter(|s| s.2 == 1).collect();
        assert_eq!(ones.len(), count);
        assert_eq!(ones.first().unwrap().0, first);
        assert_eq!(ones.last().unwrap().0, last);
        assert_eq!(samples.iter().rev().find(|s| s.2 > 1).unwrap().2, before);
        let removed = &lines[drop];
        assert_eq!(removed.name, "UPDATE");
        assert!(removed.f["drops"]
            .as_array()
            .unwrap()
            .contains(&Value::from(id)));
        assert_eq!(removed.t - ones.last().unwrap().1, delay);
    }
    // Control: a real zero-HP observation is followed by removal in this capture.
    let sentry = hp(&lines, 250996);
    assert_eq!(sentry.last().unwrap(), &(27413, 1791044769263791, 0));
    assert!(lines[27414].f["drops"]
        .as_array()
        .unwrap()
        .contains(&Value::from(250996)));
}

#[test]
fn quest_switch_and_drop_do_not_prove_a_kill_when_the_object_returns() {
    let lines = decoded();
    assert_eq!(lines[15647].f["objectId"], 251628);
    assert_eq!(hp(&lines[..15708], 239725).last().unwrap().2, 17910);
    assert!(lines[15708].f["drops"]
        .as_array()
        .unwrap()
        .contains(&Value::from(239725)));
    let returned = lines[24562].f["newObjects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["status"]["objectId"] == 239725)
        .unwrap();
    assert_eq!(returned["objectType"], 34553);
    assert_eq!(
        returned["status"]["pos"],
        serde_json::json!({"x":1026.5,"y":1249.5})
    );
    assert!(returned["status"]["stats"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["statTypeNum"] == 1 && s["statValue"] == 15080));

    // Use fixture metadata, never a client-derived catalog or verified-local assumption.
    let expected: Value =
        serde_json::from_slice(&std::fs::read(fixture("expected.json")).unwrap()).unwrap();
    let mut catalog: Catalog = serde_json::from_str(include_str!(
        "../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
    ))
    .unwrap();
    catalog.objects = expected["boss_tier_targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            serde_json::from_value::<ObjectDef>(serde_json::json!({
                "object_type": t["target_type"],
                "name": t["name"],
                "is_player": false,
                "is_boss": false,
                "is_miniboss": false,
                "sprite_key": null,
                "event_boss": false
            }))
            .unwrap()
        })
        .collect();
    let events = replay_engine(
        &fixture("realm-visit.pcap"),
        Arc::new(catalog),
        EngineConfig::default(),
    )
    .unwrap();
    let mut final_fights = BTreeMap::new();
    for e in events {
        match e.event {
            EngineEvent::FightStarted(f)
            | EngineEvent::FightUpdated(f)
            | EngineEvent::FightEnded(f) => {
                if [244589, 252023, 239725, 251628].contains(&f.target_id) {
                    assert!(
                        f.kill.is_none(),
                        "HP1/switch/drop alone is not confirmed kill evidence"
                    );
                }
                final_fights.insert(f.target_id, f);
            }
            _ => {}
        }
    }
    for id in [244589, 252023, 239725, 251628] {
        assert_eq!(final_fights[&id].target_hp, Some(1));
    }
    assert_eq!(
        final_fights[&250996].kill.as_ref().unwrap().evidence,
        KillEvidence::HpZeroRemoved
    );
}
