mod common;
use common::stats::*;
use maxed_types::*;

const C: &str = "synth-character";

fn start(db: &mut rusqlite::Connection) -> Stream {
    let mut s = Stream::new("live");
    s.apply(
        db,
        vec![session("s1", 0, 0, None), character(C, "SynthAlpha", 0)],
    );
    s
}

#[test]
fn map_change_closes_previous_visit_and_opens_next() {
    let (_dir, _path, mut db) = database();
    let mut s = start(&mut db);
    let first = s.next_sequence();
    s.apply(&mut db, vec![map("Nexus", AreaKind::Nexus, MIN)]);
    s.apply(
        &mut db,
        vec![map("Realm of the Mad God", AreaKind::Realm, 3 * MIN)],
    );
    let visits = visits(&db);
    assert_eq!(visits.len(), 2);
    assert_eq!(visits[0].id, format!("live:visit:{first}"));
    assert_eq!(
        (visits[0].started, visits[0].ended),
        (Timestamp(MIN), Some(Timestamp(3 * MIN)))
    );
    assert_eq!(visits[0].kind, AreaKind::Nexus);
    assert_eq!(visits[1].ended, None);
    assert_eq!(visits[1].source, VisitSource::Live);
}

#[test]
fn new_visit_inherits_character_and_switch_replaces_it() {
    let (_dir, _path, mut db) = database();
    let mut s = start(&mut db);
    s.apply(
        &mut db,
        vec![map("Nexus", AreaKind::Nexus, MIN), fame(C, 100, MIN)],
    );
    // No CharacterUpdated on this map change: the visit inherits SynthAlpha.
    s.apply(
        &mut db,
        vec![map("Realm of the Mad God", AreaKind::Realm, 2 * MIN)],
    );
    assert_eq!(visits(&db)[1].character_id.as_deref(), Some(C));
    // Switching characters: the new character's update replaces the inherited one.
    s.apply(
        &mut db,
        vec![
            map("Nexus", AreaKind::Nexus, 3 * MIN),
            character("synth-other", "SynthBravo", 3 * MIN),
        ],
    );
    assert_eq!(visits(&db)[2].character_id.as_deref(), Some("synth-other"));
}

#[test]
fn visit_is_skipped_when_stream_has_no_session() {
    let (_dir, _path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![EngineEvent::CaptureStateChanged(CaptureStatus::default())],
    );
    s.apply(&mut db, vec![map("Nexus", AreaKind::Nexus, MIN)]);
    assert!(visits(&db).is_empty());
}

#[test]
fn run_start_links_dungeon_visit_and_renames_map() {
    let (_dir, _path, mut db) = database();
    let mut s = start(&mut db);
    let mut r = run("run-1", "s1", "Synthetic Halls", MIN);
    s.apply(
        &mut db,
        vec![
            map("synthetic halls raw", AreaKind::Dungeon, MIN),
            EngineEvent::RunStarted(r.clone()),
        ],
    );
    r.character_id = Some(C.into());
    s.apply(&mut db, vec![EngineEvent::RunUpdated(r)]);
    let visit = &visits(&db)[0];
    assert_eq!(visit.run_id.as_deref(), Some("run-1"));
    assert_eq!(visit.map, "Synthetic Halls");
    assert_eq!(visit.character_id.as_deref(), Some(C));
}

#[test]
fn run_start_does_not_link_non_dungeon_visit() {
    let (_dir, _path, mut db) = database();
    let mut s = start(&mut db);
    s.apply(
        &mut db,
        vec![
            map("Nexus", AreaKind::Nexus, MIN),
            EngineEvent::RunStarted(run("run-1", "s1", "Synthetic Halls", MIN)),
        ],
    );
    let visit = &visits(&db)[0];
    assert_eq!(
        (visit.run_id.as_deref(), visit.map.as_str()),
        (None, "Nexus")
    );
}

#[test]
fn session_end_closes_open_visits_at_session_end() {
    let (_dir, _path, mut db) = database();
    let mut s = start(&mut db);
    s.apply(&mut db, vec![map("Nexus", AreaKind::Nexus, MIN)]);
    s.apply(&mut db, vec![session("s1", 0, 9 * MIN, Some(9 * MIN))]);
    assert_eq!(visits(&db)[0].ended, Some(Timestamp(9 * MIN)));
}

#[test]
fn closing_before_start_clamps_to_start() {
    let (_dir, _path, mut db) = database();
    let mut s = start(&mut db);
    s.apply(&mut db, vec![map("Nexus", AreaKind::Nexus, 5 * MIN)]);
    s.apply(&mut db, vec![map("Vault", AreaKind::OtherHub, 4 * MIN)]);
    let all = visits(&db);
    let nexus = all.iter().find(|visit| visit.map == "Nexus").unwrap();
    assert_eq!(nexus.ended, Some(Timestamp(5 * MIN)));
}

#[test]
fn idle_session_end_closes_visit_and_resume_waits_for_map_change() {
    let (_dir, _path, mut db) = database();
    let mut s = start(&mut db);
    s.apply(
        &mut db,
        vec![map("Realm of the Mad God", AreaKind::Realm, MIN)],
    );
    // Idle timeout: the engine ends the session at its last traffic.
    s.apply(&mut db, vec![session("s1", 0, 2 * MIN, Some(2 * MIN))]);
    // Play resumes in the same map: a new session, no MapEntered yet.
    s.apply(
        &mut db,
        vec![
            session("s2", 40 * MIN, 40 * MIN, None),
            fame(C, 120, 41 * MIN),
        ],
    );
    let all = visits(&db);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].ended, Some(Timestamp(2 * MIN)));
    // The next map change opens a visit in the new session, without inheriting across sessions.
    s.apply(&mut db, vec![map("Nexus", AreaKind::Nexus, 42 * MIN)]);
    let all = visits(&db);
    assert_eq!(
        (all[1].session_id.as_str(), all[1].character_id.as_deref()),
        ("s2", None)
    );
}

#[test]
fn merge_repoints_visit_character() {
    let (_dir, _path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 0, None),
            character("provisional", "SynthAlpha", 0),
            character("verified", "SynthAlpha", 0),
            map("Nexus", AreaKind::Nexus, MIN),
        ],
    );
    s.apply(&mut db, vec![fame("provisional", 10, MIN)]);
    s.apply(
        &mut db,
        vec![EngineEvent::CharacterMerged {
            provisional_id: "provisional".into(),
            verified_id: "verified".into(),
        }],
    );
    assert_eq!(visits(&db)[0].character_id.as_deref(), Some("verified"));
    // Later events naming the provisional id resolve to the verified one.
    s.apply(&mut db, vec![fame("provisional", 20, 2 * MIN)]);
    assert_eq!(visits(&db)[0].character_id.as_deref(), Some("verified"));
}

#[test]
fn replayed_batch_is_idempotent() {
    let (_dir, _path, mut db) = database();
    let mut s = start(&mut db);
    s.apply(&mut db, vec![map("Nexus", AreaKind::Nexus, MIN)]);
    let before = visits(&db);
    // Re-applying an already-received envelope is a no-op through event_receipts.
    let replay = EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "live".into(),
        sequence: Sequence(Timestamp(s.next_sequence() - 1)),
        at: Timestamp(0),
        event: map("Nexus", AreaKind::Nexus, MIN),
    };
    maxed_store::apply_batch(&mut db, &[replay]).unwrap();
    assert_eq!(visits(&db), before);
}

#[test]
fn recovery_closes_visit_left_open_in_an_ended_session() {
    let (_dir, path, mut db) = database();
    let mut s = start(&mut db);
    s.apply(&mut db, vec![map("Nexus", AreaKind::Nexus, MIN)]);
    // Model a visit left open although its session ended (not reachable through the
    // projection, but a crash between releases must not leave it open forever).
    db.execute(
        "UPDATE play_sessions SET ended=5*60000000, last_traffic=5*60000000,
        json=json_set(json,'$.ended','300000000','$.last_traffic','300000000')",
        [],
    )
    .unwrap();
    drop(db);
    let writer =
        maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()).unwrap();
    writer.shutdown().unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(visits(&db)[0].ended, Some(Timestamp(5 * MIN)));
}

#[test]
fn recovery_closes_visit_of_interrupted_session() {
    let (_dir, path, mut db) = database();
    let mut s = Stream::new("live");
    s.apply(
        &mut db,
        vec![
            session("s1", 0, 7 * MIN, None),
            map("Realm of the Mad God", AreaKind::Realm, MIN),
        ],
    );
    drop(db);
    let writer =
        maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()).unwrap();
    writer.shutdown().unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(visits(&db)[0].ended, Some(Timestamp(7 * MIN)));
}
