//! Synthetic runtime integration; execution requires Windows.
use super::*;
use crate::runtime::{history_replay_tests::reconstruction as packets, storage::Namespace};
use maxed_protocol::{defs::CrucibleResponse, DecodedPacket, Packet};

fn definition() -> maxed_types::CrucibleDef {
    maxed_types::CrucibleDef {
        id: "saved".into(),
        damage_multiplier: 1.5,
    }
}

fn open(root: &std::path::Path, namespace: Namespace) -> Adapter {
    let path = root.join(format!("{}.sqlite3", namespace.label()));
    let lease = super::super::storage::namespace_lease(root, namespace).unwrap();
    if namespace != Namespace::Live {
        remove_playback_database(&path).unwrap();
    }
    let mut adapter = Adapter::open(
        path,
        Arc::new(packets::catalog()),
        packets::config(),
        WriterConfig {
            stream_id: packets::config().stream_id,
            ..Default::default()
        },
        false,
        Arc::new(Views::default()),
        Bridge::default(),
    )
    .unwrap();
    adapter.lease = Some(lease);
    adapter.initialize_namespace(namespace);
    adapter
}

fn seed(root: &std::path::Path) {
    let mut db = rusqlite::Connection::open(root.join("live.sqlite3")).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    maxed_store::upsert_crucible_definitions(&mut db, &[definition()], Timestamp(1)).unwrap();
}

fn feed(adapter: &mut Adapter, at: u64, packet: Packet) {
    adapter
        .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(DecodedPacket {
            conn: ConnId(1),
            dir: Direction::ServerToClient,
            at: Timestamp(at),
            id: 183,
            len: 0,
            alignment_verified: true,
            packet,
        })))
        .unwrap();
}

fn learn(adapter: &mut Adapter) {
    feed(
        adapter,
        3000,
        Packet::CrucibleResponse(CrucibleResponse {
            crucibleIds: vec![0],
            crucibleJsons: vec![
                r#"{"array":[{"id":"learned","bonuses":[{"type":5,"amount":2}]}]}"#.into(),
            ],
        }),
    );
}

fn hit(adapter: &mut Adapter) {
    feed(adapter, 0, packets::map("The Void"));
    feed(adapter, 1000, packets::create(1, 1));
    feed(
        adapter,
        2000,
        packets::update(vec![packets::player(), packets::target()], vec![]),
    );
    feed(
        adapter,
        4000,
        packets::tick(vec![packets::status(
            1,
            vec![packets::string(128, "saved")],
        )]),
    );
    feed(adapter, 5000, packets::shoot(1));
    feed(adapter, 6000, packets::hit(1));
}

#[test]
fn live_saved_definitions_resolve_before_response() {
    let dir = tempfile::tempdir().unwrap();
    seed(dir.path());
    let mut adapter = open(dir.path(), Namespace::Live);
    hit(&mut adapter);
    assert_eq!(
        adapter.engine.reconstruction_coverage()["assumedCrucibleDefault"],
        0
    );
    assert_eq!(adapter.engine.snapshot().fights[0].total_damage, 150.);
    adapter.close().unwrap();
}

#[test]
fn live_response_persists_and_write_failure_keeps_event_batch() {
    for fail in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut adapter = open(dir.path(), Namespace::Live);
        let db = rusqlite::Connection::open(dir.path().join("live.sqlite3")).unwrap();
        if fail {
            db.execute_batch("CREATE TRIGGER fail_crucible BEFORE INSERT ON crucible_definitions BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
        }
        learn(&mut adapter);
        feed(&mut adapter, 4000, packets::map("Realm"));
        adapter.writer_flush_for_test();
        let rows = maxed_store::read_crucible_definitions(&db).unwrap();
        assert_eq!(rows.len(), usize::from(!fail));
        assert!(adapter.writer.as_ref().unwrap().status().durable.is_some());
        assert!(!adapter.rejected);
        adapter.close().unwrap();
    }
}

#[test]
fn playback_neither_reads_nor_writes_live_definitions() {
    for namespace in [Namespace::Replay, Namespace::Demo] {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path());
        let mut adapter = open(dir.path(), namespace);
        hit(&mut adapter);
        assert_eq!(
            adapter.engine.reconstruction_coverage()["assumedCrucibleDefault"],
            1
        );
        learn(&mut adapter);
        adapter.writer_flush_for_test();
        let db = rusqlite::Connection::open(dir.path().join("live.sqlite3")).unwrap();
        assert_eq!(
            maxed_store::read_crucible_definitions(&db).unwrap(),
            vec![definition()]
        );
        adapter.close().unwrap();
    }
}

#[test]
fn definition_read_error_does_not_stop_capture() {
    let dir = tempfile::tempdir().unwrap();
    let mut adapter = open(dir.path(), Namespace::Live);
    let db = rusqlite::Connection::open(dir.path().join("live.sqlite3")).unwrap();
    db.execute_batch(
        "ALTER TABLE crucible_definitions RENAME COLUMN damage_multiplier TO unavailable",
    )
    .unwrap();
    adapter.initialize_namespace(Namespace::Live);
    hit(&mut adapter);
    assert_eq!(
        adapter.engine.reconstruction_coverage()["assumedCrucibleDefault"],
        1
    );
    adapter.writer_flush_for_test();
    assert!(adapter.writer.as_ref().unwrap().status().durable.is_some());
    adapter.close().unwrap();
}
