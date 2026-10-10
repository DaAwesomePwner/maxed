use maxed_cli::demo::{replay_engine, write_demo};
use maxed_engine::EngineConfig;
use maxed_protocol::{DecodeEvent, Packet};
use maxed_recording::PcapReader;
use maxed_types::{Catalog, EnchantState, EngineEvent, Evidence, FightEnd, Outcome};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-demo")
        .join(name)
}

fn config() -> EngineConfig {
    EngineConfig {
        stream_id: "demo-v1".into(),
        identity_key: [7; 32],
        ..Default::default()
    }
}

#[test]
fn full_story_uses_verified_pipeline_and_is_deterministic() {
    let catalog: Catalog =
        serde_json::from_slice(&std::fs::read(fixture("catalog.json")).unwrap()).unwrap();
    let catalog = Arc::new(catalog);
    let events = replay_engine(&fixture("story.pcap"), catalog.clone(), config()).unwrap();
    let again = replay_engine(&fixture("story.pcap"), catalog, config()).unwrap();
    assert_eq!(
        serde_json::to_vec_pretty(&events).unwrap(),
        std::fs::read(fixture("expected.json")).unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&events).unwrap(),
        serde_json::to_vec(&again).unwrap()
    );
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.sequence.0 .0, index as u64 + 1);
        assert_eq!(event.stream_id, "demo-v1");
    }
    let maps: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::MapEntered { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(maps, ["Nexus", "Realm", "Snake Pit", "Nexus"]);
    let runs: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::RunEnded(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(runs.len(), 1);
    let run = runs[0];
    assert_eq!(run.outcome, Some(Outcome::Cleared));
    assert_eq!(run.completed_at.unwrap().0 - run.started.0, 5_000_000);
    assert_eq!(run.ended.unwrap().0 - run.started.0, 7_000_000);
    assert_eq!(run.party_size, Some(2));
    assert!(run.evidence.iter().any(|e| e.evidence == Evidence::Victory));
    let fights: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::FightEnded(f) => Some(f),
            _ => None,
        })
        .collect();
    let boss = fights.iter().find(|f| f.boss_type == Some(257)).unwrap();
    assert!(boss.kill.as_ref().unwrap().provisional);
    assert_eq!(boss.end_reason, Some(FightEnd::MapChanged));
    assert_eq!(boss.total_damage, 300.);
    assert_eq!(boss.players.len(), 1);
    let hero = &boss.players[0];
    assert!(hero.is_local);
    assert_eq!(hero.hits, 2);
    assert_eq!(hero.dps, None);
    assert_eq!(hero.share, Some(1.));
    assert_eq!(hero.sources[0].item_type, Some(256));
    assert!(!hero.sources[0].estimated);
    let bags: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::BagDropped(b) => Some(b),
            _ => None,
        })
        .collect();
    assert_eq!(bags.len(), 1);
    let bag_updates: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::DropUpdated(b) => Some((e.at, b)),
            _ => None,
        })
        .collect();
    // Contents become complete in the spawn UPDATE, with no enrichment at
    // either 9.1/9.2 s NEWTICK; the map exit republishes the unchanged bag.
    assert_eq!(
        bag_updates.iter().map(|(at, _)| at.0).collect::<Vec<_>>(),
        [9_000_000, 12_000_000]
    );
    assert_eq!(bag_updates[0].1, bag_updates[1].1);
    assert_eq!(bags[0].at.0, 9_000_000);
    assert!(!bags[0].contents_known);
    for bag in bags
        .iter()
        .copied()
        .chain(bag_updates.iter().map(|(_, bag)| *bag))
    {
        assert_eq!(bag.id, bags[0].id);
        assert_eq!(bag.source_enemy, None);
        assert!(!bag.source_estimated);
        assert_eq!(bag.source_reason.as_deref(), Some("source-not-observed"));
    }
    let bag = bag_updates[0].1;
    assert!(bag.contents_known);
    assert_eq!(bag.items, [256]);
    assert_eq!(bag.contents[0].enchant.state, EnchantState::Recorded);
    assert!(events
        .iter()
        .any(|e| matches!(e.event, EngineEvent::FameChanged { fame: 125, .. })));
    let report = maxed_cli::replay(&fixture("story.pcap")).unwrap();
    assert_eq!(report.decoded, 21);
    assert_eq!(report.unverified_packets, 0);
    assert_eq!(report.failures, 0);
    assert_eq!(report.unknown, 0);
    assert!(!report.truncated);
}

#[test]
fn command_generates_identical_bytes_and_rejects_overwrite_and_bad_arguments() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("demo-cli-{}.pcap", std::process::id()));
    let command = || std::process::Command::new(env!("CARGO_BIN_EXE_maxed-cli"));
    let result = command().arg("make-demo").arg(&path).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let bytes = std::fs::read(&path).unwrap();
    let expected = std::fs::read(fixture("story.pcap")).unwrap();
    assert_eq!(Sha256::digest(&bytes), Sha256::digest(&expected));
    assert_eq!(bytes, expected);
    let result = command().arg("make-demo").arg(&path).output().unwrap();
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    std::fs::remove_file(path).unwrap();
    for args in [vec!["make-demo"], vec!["make-demo", "a", "b"]] {
        assert_eq!(
            command().args(args).output().unwrap().status.code(),
            Some(2)
        );
    }
    let help = command().arg("--help").output().unwrap();
    assert!(String::from_utf8(help.stdout)
        .unwrap()
        .contains("maxed-cli make-demo <new-output.pcap>"));
}

// Independent checksum verification, including the IPv4 pseudo-header for TCP.
fn valid_checksum(bytes: &[u8]) -> bool {
    let mut sum: u64 = bytes
        .chunks(2)
        .map(|p| (u64::from(p[0]) << 8) + u64::from(*p.get(1).unwrap_or(&0)))
        .sum();
    while sum > 65535 {
        sum = (sum & 65535) + (sum >> 16);
    }
    sum == 65535
}

#[test]
fn fixture_has_real_checksums_syn_provenance_and_only_authored_identifiers() {
    let bytes = std::fs::read(fixture("story.pcap")).unwrap();
    let mut reader = PcapReader::new(bytes.as_slice()).unwrap();
    let mut pipeline = maxed_cli::Pipeline::default();
    let mut packets = Vec::new();
    let mut syns = 0;
    while let Some(frame) = reader.next_frame().unwrap() {
        let ip = &frame.bytes[14..34];
        assert!(valid_checksum(ip));
        assert_eq!(
            &frame.bytes[26..34],
            if frame.bytes[34..36] == 2050u16.to_be_bytes() {
                &[198, 51, 100, 20, 192, 0, 2, 10]
            } else {
                &[192, 0, 2, 10, 198, 51, 100, 20]
            }
        );
        let mut pseudo = frame.bytes[26..34].to_vec();
        pseudo.extend([0, 6]);
        pseudo.extend(u16::try_from(frame.bytes.len() - 34).unwrap().to_be_bytes());
        pseudo.extend(&frame.bytes[34..]);
        assert!(valid_checksum(&pseudo));
        if frame.bytes[47] & 2 != 0 {
            syns += 1;
        }
        packets.extend(pipeline.push(frame));
    }
    packets.extend(pipeline.finish());
    assert_eq!(syns, 2);
    let mut names = Vec::new();
    let mut accounts = Vec::new();
    for event in packets {
        match event {
            DecodeEvent::Packet(p) => {
                assert!(p.alignment_verified);
                match p.packet {
                    Packet::MapInfo(m) => {
                        assert!(["Nexus", "Realm", "Snake Pit"].contains(&m.name.as_str()))
                    }
                    Packet::CreateSuccess(c) => {
                        assert_eq!(c.objectId, 1);
                        assert_eq!(c.charId, 1);
                    }
                    Packet::Update(u) => {
                        for object in u.newObjects {
                            assert!([100, 257, 258, 1292].contains(&object.objectType));
                            assert!((1..=5).contains(&object.status.objectId));
                            for stat in object.status.stats {
                                match (stat.statTypeNum, stat.stringStatValue) {
                                    (31, Some(name)) => names.push(name),
                                    (38, Some(account)) => accounts.push(account),
                                    (_, Some(s)) => {
                                        assert!(["", ",,,", "AAIEAQA=,,,,,,,"].contains(&s.as_str()))
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    Packet::NewTick(_)
                    | Packet::ServerPlayerShoot(_)
                    | Packet::Damage(_)
                    | Packet::Notification(_) => {}
                    _ => panic!("Unexpected packet in authored demo"),
                }
            }
            DecodeEvent::Diagnostic { kind, .. } => {
                assert_eq!(kind, maxed_protocol::Diagnostic::Cancelled)
            }
        }
    }
    assert_eq!(names, ["DemoHero", "DemoHero", "DemoAlly"]);
    assert_eq!(accounts, ["DemoAccount", "DemoAccount", "DemoAllyAccount"]);
}

#[test]
fn generator_refuses_to_replace_existing_file() {
    let before = std::fs::read(fixture("story.pcap")).unwrap();
    assert!(write_demo(&fixture("story.pcap")).is_err());
    assert_eq!(std::fs::read(fixture("story.pcap")).unwrap(), before);
}
