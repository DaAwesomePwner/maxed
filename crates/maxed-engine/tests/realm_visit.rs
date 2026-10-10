mod common;
use common::*;
use maxed_protocol::defs::Packet;
use maxed_types::*;

fn entry(e: &mut maxed_engine::Engine, name: &str, seed: i32, opened: i32) -> serde_json::Value {
    let Packet::MapInfo(mut packet) = map(name) else {
        unreachable!()
    };
    packet.realmName = "NexusPortal.Zephyr".into();
    packet.seed = seed;
    packet.gameOpenedTime = opened;
    let at = e.snapshot().at.0 + 1_000_000;
    apply(e, at, Packet::MapInfo(packet))
        .into_iter()
        .find_map(|event| {
            matches!(event, EngineEvent::MapEntered { .. })
                .then(|| serde_json::to_value(event).unwrap())
        })
        .unwrap()["map_entered"]
        .clone()
}

#[test]
fn realm_instance_identity_uses_all_mapinfo_fields_and_never_keys_dungeon_runs() {
    let mut catalog = catalog();
    catalog.dungeons[1].name = "Realm of the Mad God".into();
    catalog.dungeons[2].name = "Ocean Trench".into();
    let mut e = maxed_engine::Engine::new(config(), std::sync::Arc::new(catalog)).unwrap();
    let a = entry(&mut e, "Realm of the Mad God", 1486421971, 1791146155);
    assert_eq!(
        a["realm_instance_key"],
        serde_json::to_string(&(
            "Realm of the Mad God",
            "NexusPortal.Zephyr",
            1486421971,
            1791146155
        ))
        .unwrap()
    );
    let dungeon = entry(&mut e, "Ocean Trench", 1580120007, 1791147646);
    assert!(dungeon["realm_instance_key"].is_null());
    let first_run = e.snapshot().run.unwrap().id;
    assert_eq!(
        entry(&mut e, "Realm of the Mad God", 1486421971, 1791146155)["realm_instance_key"],
        a["realm_instance_key"]
    );
    entry(&mut e, "Ocean Trench", 1580120007, 1791147646);
    assert_ne!(e.snapshot().run.unwrap().id, first_run);
    assert_ne!(
        entry(&mut e, "Realm of the Mad God", 1486421972, 1791146155)["realm_instance_key"],
        a["realm_instance_key"]
    );
    assert_ne!(
        entry(&mut e, "Realm of the Mad God", 1486421971, 1791146156)["realm_instance_key"],
        a["realm_instance_key"]
    );
    assert!(entry(&mut e, "Nexus", 158508552, 1791094827)["realm_instance_key"].is_null());
}

#[test]
fn missing_mapinfo_does_not_invent_a_realm_identity() {
    let mut e = engine();
    let events = apply(&mut e, 1000, create(1, 1));
    assert!(!events
        .iter()
        .any(|event| matches!(event, EngineEvent::MapEntered { .. })));
    assert!(e.snapshot().map.is_none());
    let legacy: EngineEvent = serde_json::from_value(
        serde_json::json!({"map_entered": {"at":"1000", "name":"Realm", "kind":"realm"}}),
    )
    .unwrap();
    assert!(serde_json::to_value(legacy).unwrap()["map_entered"]["realm_instance_key"].is_null());
}
