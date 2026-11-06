//! #45: real capture -> stream -> decoder -> engine -> presentation/alert consumers.
//! Catalogs here are synthetic subsets; no client assets or capture rewrites.
use crate::combat_fixture;
use crate::support;
use maxed_cli::Pipeline;
use maxed_engine::{Engine, EngineConfig};
use maxed_protocol::{DecodeEvent, Diagnostic};
use maxed_recording::PcapReader;
use maxed_store::{open_reader, start_writer, AlertContext, WriterConfig};
use maxed_types::*;
use serde_json::{json, Value};
use std::{fs, path::PathBuf, sync::Arc};
// Compile the actual native presentation boundary without introducing a CLI ->
// desktop dependency. The encoder in this shared source is unused by replay.
#[allow(dead_code)]
#[path = "../../../app/src-tauri/src/bridge/payload.rs"]
mod presentation;

fn fixture(name: &str) -> (PathBuf, Value) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed")
        .join(name);
    let expected = serde_json::from_slice(&fs::read(dir.join("expected.json")).unwrap()).unwrap();
    (dir, expected)
}
fn timestamp(value: &Value) -> Timestamp {
    // Engine DTOs use millisecond precision; expected.json records pcap micros.
    let micros: u64 = value.as_str().unwrap().parse().unwrap();
    Timestamp(micros / 1000 * 1000)
}
fn catalog(expected: &Value, realm: bool) -> Catalog {
    let mut value =
        serde_json::to_value(combat_fixture::catalog(34489, "The Plague Doctor", true)).unwrap();
    let mut objects: Vec<Value> = maxed_protocol::privacy::PLAYER_CLASSES
        .iter()
        .map(|(ty, _)| {
            json!({
                "object_type":ty,"name":"Fixture class","is_player":true,
                "is_boss":false,"is_miniboss":false,"sprite_key":null
            })
        })
        .collect();
    if realm {
        for target in expected["bosses"].as_array().unwrap() {
            let ty = target["target_type"].as_u64().unwrap();
            if objects.iter().any(|object| object["object_type"] == ty) {
                continue;
            }
            objects.push(json!({"object_type":ty,"name":target["name"],
                "is_player":false,"is_boss":false,"is_miniboss":false,
                "event_boss":true,"sprite_key":null,
                "realm_event_key":target["name"].as_str().unwrap().replace(' ', "_")}));
        }
        value["dungeons"] = json!([
            {"name":"Realm of the Mad God","aliases":[],"kind":"realm","completion_index":null},
            {"name":"Ocean Trench","aliases":[],"kind":"dungeon","completion_index":null}
        ]);
    } else {
        objects.push(
            json!({"object_type":expected["key_pop"]["portal_object_type"],
            "name":"Fixture portal","dungeon_name":"Lair of Shaitan",
            "is_player":false,"is_boss":false,"is_miniboss":false,"sprite_key":null}),
        );
        value["dungeons"] = json!([
            {"name":"Nexus","aliases":[],"kind":"nexus","completion_index":null},
            {"name":"Lair of Shaitan","aliases":[],"kind":"dungeon","completion_index":null}
        ]);
    }
    value["objects"] = json!(objects);
    serde_json::from_value(value).unwrap()
}

struct Replay {
    events: Vec<EventEnvelope>,
    eligible: Vec<Option<String>>,
    // Snapshot and event boundary immediately before capture-stop retirement.
    snapshot: LiveSnapshot,
    live_snapshot: LiveSnapshot,
    before_finish: usize,
}
fn publish(
    engine: &mut Engine,
    output: &mut Vec<EventEnvelope>,
    eligible: &mut Vec<Option<String>>,
    events: Vec<EngineEvent>,
) {
    if events.is_empty() {
        return;
    }
    let at = engine.snapshot().at;
    for event in events {
        // Same engine gate called by runtime/join.rs while this connection's
        // identity is still current, never evaluated against end-of-replay state.
        eligible.push(engine.key_pop_dungeon(&event));
        let sequence = Sequence(Timestamp(output.len() as u64 + 1));
        output.push(EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "live-smoke-regression".into(),
            sequence,
            at,
            event,
        });
        engine.set_watermark(sequence);
    }
}
fn replay(dir: &std::path::Path, expected: &Value, catalog: Catalog) -> Replay {
    let mut engine = Engine::new(
        EngineConfig {
            stream_id: "live-smoke-regression".into(),
            identity_key: [7; 32],
            ..Default::default()
        },
        Arc::new(catalog),
    )
    .unwrap();
    let mut pipeline = Pipeline::default();
    let mut reader =
        PcapReader::new(fs::File::open(dir.join(expected["pcap"].as_str().unwrap())).unwrap())
            .unwrap();
    let mut output = Vec::new();
    let mut eligible = Vec::new();
    // Model the live adapter lifecycle without opening a capture device.
    let events = engine.capture_changed(CaptureStatus {
        state: CaptureState::Live,
        ..Default::default()
    });
    publish(&mut engine, &mut output, &mut eligible, events);
    let mut at = Timestamp(0);
    while let Some(frame) = reader.next_frame().unwrap() {
        at = Timestamp(at.0.max(frame.at.0));
        if maxed_capture::parse_frame(&frame)
            .unwrap()
            .is_some_and(|s| !s.bytes.is_empty())
        {
            let events = engine.observe_traffic(frame.at);
            publish(&mut engine, &mut output, &mut eligible, events);
        }
        for decoded in pipeline.push(frame) {
            consume(&mut engine, &mut output, &mut eligible, decoded);
        }
    }
    for decoded in pipeline.finish() {
        consume(&mut engine, &mut output, &mut eligible, decoded);
    }
    let snapshot = engine.snapshot();
    let live_snapshot = engine.live_snapshot();
    let before_finish = output.len();
    let events = engine.finish(at, FightEnd::CaptureStopped);
    publish(&mut engine, &mut output, &mut eligible, events);
    Replay {
        events: output,
        eligible,
        snapshot,
        live_snapshot,
        before_finish,
    }
}
fn consume(
    engine: &mut Engine,
    output: &mut Vec<EventEnvelope>,
    eligible: &mut Vec<Option<String>>,
    decoded: DecodeEvent,
) {
    let events = match decoded {
        DecodeEvent::Packet(p) if p.alignment_verified => engine.apply(p),
        DecodeEvent::Packet(p) => engine.diagnostic(p.conn, p.dir, p.at, Diagnostic::Recovering),
        DecodeEvent::Diagnostic {
            conn,
            dir,
            at,
            kind,
        } => engine.diagnostic(conn, dir, at, kind),
    };
    publish(engine, output, eligible, events);
}

#[test]
fn realm_return_kills_end_at_removal_and_short_tower_has_no_dps() {
    let (dir, expected) = fixture("maxed-realm-dungeon-return");
    let replay = replay(&dir, &expected, catalog(&expected, true));
    for target in expected["bosses"].as_array().unwrap() {
        let id = target["target_id"].as_i64().unwrap() as i32;
        let fight = replay
            .events
            .iter()
            .filter_map(|e| match &e.event {
                EngineEvent::FightEnded(f) if f.target_id == id => Some(f),
                _ => None,
            })
            .next_back()
            .unwrap_or_else(|| panic!("missing target {id}"));
        assert_eq!(
            fight.target_type,
            Some(target["target_type"].as_u64().unwrap() as u16)
        );
        let removal = timestamp(&target["removal_us"]);
        let kill = fight
            .kill
            .as_ref()
            .unwrap_or_else(|| panic!("missing kill {id}"));
        assert!(!kill.provisional, "target {id}");
        assert_eq!(kill.at, removal, "target {id}");
        assert_eq!(fight.ended, Some(removal), "target {id}");
        assert_eq!(fight.end_reason, Some(FightEnd::Killed));
        assert_eq!(
            kill.evidence,
            if target["oryx"].is_null() {
                KillEvidence::HpZeroRemoved
            } else {
                KillEvidence::AnnouncedDeathRemoved
            },
            "target {id}"
        );
        if id == 222624 {
            let seconds =
                (fight.last_hit.unwrap().0 - fight.first_hit.unwrap().0) as f64 / 1_000_000.;
            assert!((0.3..0.5).contains(&seconds), "{seconds}");
            assert!(!fight.players.is_empty());
            assert!(fight.players.iter().any(|p| p.damage > 0.));
            assert!(fight.players.iter().all(|p| p.dps.is_none()));
        }
    }
    let entries: Vec<_> = replay
        .events
        .iter()
        .filter_map(|e| match &e.event {
            EngineEvent::MapEntered {
                at,
                kind: AreaKind::Realm,
                realm_instance_key,
                ..
            } => Some((at, realm_instance_key)),
            _ => None,
        })
        .collect();
    assert_eq!(entries.len(), 2);
    assert!(entries[0].1.is_some());
    assert_eq!(entries[0].1, entries[1].1);
    assert_eq!(
        *entries[0].0,
        timestamp(&expected["connections"][0]["mapinfo_us"])
    );
    assert_eq!(
        *entries[1].0,
        timestamp(&expected["connections"][2]["mapinfo_us"])
    );
    // UI test invokes this exact replay and consumes fresh real engine DTOs.
    // No generated event dump or catalog is committed.
    if let Some(path) = std::env::var_os("MAXED_SMOKE_PROJECTION_PATH") {
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        serde_json::to_writer(
            file,
            &json!({
                "events": &replay.events[..replay.before_finish],
                "presentation_events": replay.events[..replay.before_finish].iter().cloned()
                    .map(presentation::presentation_event).collect::<Vec<_>>(),
                "snapshot": replay.snapshot,
                "live_snapshot": replay.live_snapshot
            }),
        )
        .unwrap();
    }
}

#[test]
fn nexus_key_pop_uses_live_identity_and_durable_alert_filter() {
    let (dir, expected) = fixture("maxed-nexus-key-pop");
    let replay = replay(&dir, &expected, catalog(&expected, false));
    let portals: Vec<_> = replay.events.iter().zip(&replay.eligible).filter(|(e, _)| matches!(&e.event,
        EngineEvent::InstanceEvent(row) if matches!(row.kind, InstanceEventKind::PortalOpened { .. }))).collect();
    assert_eq!(portals.len(), 1);
    let (portal, eligible) = portals[0];
    assert_eq!(eligible.as_deref(), Some("Lair of Shaitan"));
    let EngineEvent::InstanceEvent(row) = &portal.event else {
        unreachable!()
    };
    assert_eq!(row.at, timestamp(&expected["key_pop"]["notification_us"]));
    let InstanceEventKind::PortalOpened {
        opener, dungeon, ..
    } = &row.kind
    else {
        unreachable!()
    };
    assert_eq!(
        opener.as_deref(),
        expected["key_pop"]["opener_pseudonym"].as_str()
    );
    assert_eq!(dungeon.as_deref(), Some("Lair of Shaitan"));
    let local = replay
        .snapshot
        .character
        .as_ref()
        .unwrap()
        .name
        .as_ref()
        .unwrap();
    assert!(!local.eq_ignore_ascii_case(opener.as_ref().unwrap()));
    // A non-None engine eligibility result proves the observed local NAME_STAT
    // exists and differs from the opener, using the live own-pop suppression rule.
    for (filter, excluded, count) in [
        (None, vec![], 1),
        (None, vec!["Lair of Shaitan".into()], 0),
        (Some(vec!["Lair of Shaitan".into()]), vec![], 1),
        (Some(vec!["Ocean Trench".into()]), vec![], 0),
    ] {
        let temp = support::PrivateDir::new();
        let path = temp.file("alerts.sqlite");
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: portal.stream_id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        let mut preferences = AlertPreferences::disabled();
        preferences.key_popped = AlertLoudness::Toast;
        preferences.key_pop_dungeons = filter;
        preferences.key_pop_excluded_dungeons = excluded;
        for (index, (event, eligible)) in replay.events.iter().zip(&replay.eligible).enumerate() {
            support::submit_with_alert_context(
                &writer,
                event.clone(),
                AlertContext {
                    chat: None,
                    live: true,
                    preferences: preferences.clone(),
                    bag_window: None,
                    key_pop_dungeon: eligible.clone(),
                },
            );
            if index % 128 == 127 {
                writer.flush().unwrap();
            }
        }
        writer.flush().unwrap();
        // Re-publication with a new sequence must not create another occurrence.
        let mut repeated = portal.clone();
        repeated.sequence = Sequence(Timestamp(replay.events.len() as u64 + 1));
        support::submit_with_alert_context(
            &writer,
            repeated,
            AlertContext {
                chat: None,
                live: true,
                preferences,
                bag_window: None,
                key_pop_dungeon: eligible.clone(),
            },
        );
        writer.flush().unwrap();
        let claims = writer.claim_alerts(32).unwrap();
        assert_eq!(claims.len(), count);
        let reader = open_reader(&path).unwrap();
        let alerts = reader
            .list_alerts(AlertFilter {
                kinds: vec![],
                exclude_kinds: vec![],
                kind: Some(AlertKind::KeyPopped),
                range: None,
                query: String::new(),
                cursor: None,
                limit: 100,
            })
            .unwrap();
        assert_eq!(alerts.rows.len(), count);
        if count == 1 {
            assert_eq!(alerts.rows[0].explanation, "Lair of Shaitan key popped");
        }
        drop(reader);
        writer.shutdown().unwrap();
    }
}
