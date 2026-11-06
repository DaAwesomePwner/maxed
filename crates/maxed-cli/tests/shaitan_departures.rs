use crate::support;
use maxed_cli::{demo::replay_engine, Pipeline};
use maxed_engine::EngineConfig;
use maxed_protocol::{DecodeEvent, Packet};
use maxed_recording::PcapReader;
use maxed_types::{Catalog, EngineEvent, InstanceEventKind, Timestamp};
use serde_json::{json, Value};
use std::{collections::BTreeSet, fs, path::PathBuf, sync::Arc};

#[test]
fn shaitan_post_victory_departures_do_not_count_as_left() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-shaitan-run");
    let expected: Value =
        serde_json::from_slice(&fs::read(dir.join("expected.json")).unwrap()).unwrap();
    let path = dir.join(expected["pcap"].as_str().unwrap());
    let completed_at = Timestamp(
        expected["run"]["completed_at_us"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
    );
    let departures: Vec<u64> = expected["departures_observed_live"]
        .as_array()
        .unwrap()
        .iter()
        .map(|at| at.as_str().unwrap().parse().unwrap())
        .collect();
    assert_eq!(departures.len(), 15);
    assert!(departures.iter().all(|at| *at >= completed_at.0));

    // Use only captured classes from the protocol's committed class IDs;
    // retain no identity strings or client-derived catalog.
    let mut classes = BTreeSet::new();
    let mut players = BTreeSet::new();
    let mut removals = Vec::new();
    let mut death_notices = Vec::new();
    let mut death_has_victim = false;
    let mut pipeline = Pipeline::default();
    let mut reader = PcapReader::new(fs::File::open(&path).unwrap()).unwrap();
    while let Some(frame) = reader.next_frame().unwrap() {
        for event in pipeline.push(frame) {
            if let DecodeEvent::Packet(packet) = event {
                if let Packet::Notification(notice) = &packet.packet {
                    if notice.data.effect == Some("PlayerDeath") {
                        death_notices.push((packet.at.0 / 1000 * 1000).to_string());
                        // #45: recognize the owner's future re-scrub from the
                        // wire notice, not from successful engine cancellation.
                        if let Some(message) = &notice.data.message {
                            if let Ok(value) =
                                serde_json::from_str::<Value>(&message.replace(",}", "}"))
                            {
                                death_has_victim |= value["k"] == "s.death"
                                    && value["t"]["player"].as_str().is_some_and(|p| !p.is_empty());
                            }
                        }
                    }
                }
                if let Packet::Update(update) = packet.packet {
                    for object in update.newObjects {
                        if maxed_protocol::privacy::PLAYER_CLASSES
                            .iter()
                            .any(|(ty, _)| *ty == object.objectType)
                        {
                            classes.insert(object.objectType);
                            players.insert(object.status.objectId);
                        }
                    }
                    removals.extend(
                        update
                            .drops
                            .iter()
                            .filter(|id| players.contains(id))
                            .map(|_| packet.at.0 / 1000 * 1000),
                    );
                }
            }
        }
    }
    assert!(!classes.is_empty());
    assert_eq!(json!(death_notices), expected["party_deaths_observed_live"]);
    for at in &departures {
        let index = removals.iter().position(|removed| removed == at).unwrap();
        removals.remove(index);
    }
    let catalog: Catalog = serde_json::from_value(json!({
        "format_version": 1, "client_fingerprint": "synthetic-shaitan-departures",
        "items": [], "classes": [], "enchants": [], "bags": [], "sprites": [], "tiles": [], "crucible_bonuses": [],
        "dungeons": [{ "name": expected["run"]["map"], "aliases": [], "kind": "dungeon", "completion_index": null }],
        "objects": classes.iter().map(|object_type| json!({
            "object_type": object_type, "name": "Fixture class", "is_player": true,
            "is_boss": false, "is_miniboss": false, "sprite_key": null
        })).collect::<Vec<_>>()
    })).unwrap();
    let events = replay_engine(&path, Arc::new(catalog), EngineConfig::default()).unwrap();
    let run = events
        .iter()
        .find_map(|e| match &e.event {
            EngineEvent::RunEnded(run) => Some(run),
            _ => None,
        })
        .unwrap();
    assert_eq!(run.completed_at, Some(completed_at));
    assert_eq!(
        run.party_size,
        Some(expected["run"]["party_size_peak"].as_u64().unwrap() as u32)
    );
    let left: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::InstanceEvent(row)
                if row.run_id.as_deref() == Some(&run.id)
                    && matches!(row.kind, InstanceEventKind::PlayerLeft { .. }) =>
            {
                Some(row.at)
            }
            _ => None,
        })
        .collect();
    assert!(
        left.iter().all(|at| *at < completed_at),
        "post-Victory Left timestamps: {left:?}"
    );
    assert!(left.iter().all(|at| !departures.contains(&at.0)));
    if death_has_victim {
        assert!(
            left.is_empty(),
            "a preserved death must cancel the pre-clear departure"
        );
        assert!(events.iter().any(|e| matches!(&e.event,
            EngineEvent::InstanceEvent(row) if matches!(row.kind,
                InstanceEventKind::PlayerDeath { object_id: Some(_), .. }))));
    } else {
        // #45: the owner must re-scrub this fixture on the test PC with the
        // s.death grammar. Until then its missing victim leaves exactly one
        // pre-Victory departure. After re-scrub the branch above requires zero.
        assert_eq!(left, [Timestamp(1791143261185000)]);
    }
    // Exercise the same saved-run projection used by Runs and Run Story.
    let temp = support::PrivateDir::new();
    let database = temp.file("shaitan.sqlite");
    let writer = maxed_store::start_writer(
        database.clone(),
        maxed_store::WriterConfig {
            stream_id: events[0].stream_id.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    for batch in events.chunks(128) {
        for event in batch {
            support::submit_with_alert_context(&writer, event.clone(), Default::default());
        }
        writer.flush().unwrap();
    }
    writer.shutdown().unwrap();
    let reader = maxed_store::open_reader(&database).unwrap();
    let story = reader.get_run(&run.id).unwrap().unwrap();
    let saved_left: Vec<_> = story
        .timeline
        .iter()
        .filter(|row| row.kind == maxed_types::TimelineKind::PlayerLeft)
        .map(|row| row.at)
        .collect();
    assert_eq!(saved_left, left);
    assert!(saved_left.iter().all(|at| *at < completed_at));
}
