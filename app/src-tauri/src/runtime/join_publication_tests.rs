use super::*;
use crate::bridge::payload::SnapshotEncoder;
use maxed_protocol::{defs::MapInfo, DecodedPacket, Packet};
use std::time::Duration;

fn packet(at: u64, packet: Packet) -> PipelineEvent {
    PipelineEvent::Decoded(DecodeEvent::Packet(DecodedPacket {
        conn: ConnId(1),
        dir: Direction::ServerToClient,
        at: Timestamp(at),
        id: 101,
        len: 0,
        alignment_verified: true,
        packet,
    }))
}

fn map(name: &str) -> Packet {
    Packet::MapInfo(MapInfo {
        width: 1,
        height: 1,
        name: name.into(),
        displayName: name.into(),
        realmName: String::new(),
        fp: 1,
        seed: 1,
        background: 0,
        difficulty: 0.,
        allowPlayerTeleport: false,
        noSave: false,
        showDisplays: false,
        maxPlayerCount: 85,
        gameOpenedTime: 0,
        versionNumber: String::new(),
        unknown1: 0,
        viewDistance: 0,
        unknown2: false,
        unknownInt: 0,
        modifiers: Default::default(),
        dungeonModifiers: None,
        dungeonModifiers2: None,
        dungeonModifiers3: None,
        dungeonModifiers4: None,
        dungeonGrade: None,
        bgColor: 0,
        has_scores: true,
        maxRealmScore: 300000,
        currentRealmScore: 100,
    })
}

#[test]
fn silent_realm_scores_publish_after_queries_without_a_mismatched_delta_base() {
    let dir = tempfile::tempdir().unwrap();
    let (send, receive) = crossbeam_channel::unbounded();
    let mut adapter = Adapter::open(
        dir.path().join("realm-publication"),
        unavailable_catalog(),
        EngineConfig::default(),
        WriterConfig {
            stream_id: "engine".into(),
            ..Default::default()
        },
        true,
        Arc::new(Views::default()),
        Bridge {
            snapshot: Arc::new(move |snapshot| send.send(snapshot).unwrap()),
            ..Default::default()
        },
    )
    .unwrap();
    let mut encoder = SnapshotEncoder::default();
    encoder.encode(receive.try_recv().unwrap());
    adapter.decoded(packet(1, map("Realm"))).unwrap();
    adapter.publish_due(Duration::from_secs(1));
    let baseline = receive.try_recv().unwrap();
    assert_eq!(
        baseline
            .instance
            .as_ref()
            .unwrap()
            .realm_score
            .as_ref()
            .unwrap()
            .current,
        100
    );
    encoder.encode(baseline.clone());

    let score =
        |value| Packet::RealmScoreUpdate(maxed_protocol::defs::RealmScoreUpdate { score: value });
    adapter.decoded(packet(2, score(120))).unwrap();
    // The adapter's watermark only advances for domain events.
    assert_eq!(adapter.sequence, baseline.sequence);
    assert!(adapter.gate.dirty);
    let queried = adapter.read_snapshot();
    assert_eq!(
        queried
            .instance
            .as_ref()
            .unwrap()
            .realm_score
            .as_ref()
            .unwrap()
            .current,
        120
    );
    assert_eq!(queried.sequence, baseline.sequence);
    assert_eq!(
        queried.revision.unwrap().0 .0,
        baseline.revision.unwrap().0 .0 + 1
    );

    // A second silent packet arrives between the query and the scheduled tick.
    adapter.decoded(packet(3, score(150))).unwrap();
    adapter.publish_due(Duration::from_millis(1999));
    assert!(receive.try_recv().is_err());
    adapter.publish_due(Duration::from_secs(2));
    let published = receive.try_recv().unwrap();
    assert_eq!(published.sequence, baseline.sequence);
    assert_eq!(
        published
            .instance
            .as_ref()
            .unwrap()
            .realm_score
            .as_ref()
            .unwrap()
            .current,
        150
    );
    assert!(published.revision.unwrap().0 > queried.revision.unwrap().0);
    let LiveSnapshotUpdate::Full(full) = encoder.encode(published.clone()) else {
        panic!("publication must not require the reader's obsolete delta base");
    };
    assert_eq!(full, published);

    // With no intervening read, ordinary silent updates remain compact deltas.
    adapter.decoded(packet(4, score(180))).unwrap();
    adapter.publish_due(Duration::from_secs(3));
    let next = receive.try_recv().unwrap();
    assert_eq!(next.sequence, baseline.sequence);
    let LiveSnapshotUpdate::Delta(delta) = encoder.encode(next) else {
        panic!("consecutive publications should use deltas");
    };
    assert_eq!(delta.base_revision, published.revision);
    assert_eq!(
        delta
            .snapshot
            .instance
            .unwrap()
            .realm_score
            .unwrap()
            .current,
        180
    );

    // A query with no subsequent mutation reuses the exact published revision.
    adapter.decoded(packet(5, score(200))).unwrap();
    let queried = adapter.read_snapshot();
    adapter.publish_due(Duration::from_secs(4));
    let same = receive.try_recv().unwrap();
    assert_eq!(same, queried);
    encoder.encode(same);
    adapter.decoded(packet(6, score(220))).unwrap();
    adapter.publish_due(Duration::from_secs(5));
    let LiveSnapshotUpdate::Delta(delta) = encoder.encode(receive.try_recv().unwrap()) else {
        panic!("publishing the queried revision must establish the next delta base");
    };
    assert_eq!(delta.base_revision, queried.revision);
    adapter
        .finish(Timestamp(6), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
}
