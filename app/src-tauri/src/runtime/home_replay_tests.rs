//! Synthetic Home replay acceptance without real recordings or the capture DLL.
use super::*;
use maxed_engine::EngineConfig;
use maxed_recording::PcapReader;
use maxed_store::{open_reader, WriterConfig};
use maxed_types::*;
use std::{io::Cursor, path::PathBuf};

fn ready_home(runtime: &Arc<CaptureRuntime>, scope: HomeScope) -> Result<HomeSummary, String> {
    match tauri::async_runtime::block_on(runtime.home_summary(scope))? {
        HistoryReply::Ready { value } => Ok(value),
        other => panic!("expected ready history, got {other:?}"),
    }
}

const OFFSET: u64 = 1_772_949_595_000_000;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-home")
        .join(format!("{name}.pcap"))
}

fn session(snapshot: &LiveSnapshot) -> HomeScope {
    HomeScope::Session {
        id: snapshot
            .session
            .as_ref()
            .expect("synthetic session")
            .id
            .clone(),
    }
}

fn replay_home_fixture(name: &str) -> (Vec<LiveSnapshot>, HomeSummary) {
    let dir = tempfile::tempdir().expect("synthetic temp directory");
    let runtime = Arc::new(CaptureRuntime::default());
    let mut options = RuntimeOptions::live(dir.path().into(), None);
    options.namespace = Namespace::Demo;
    let snapshots: Arc<Mutex<Vec<LiveSnapshot>>> = Arc::default();
    let received = snapshots.clone();
    let events: Arc<Mutex<Vec<EventEnvelope>>> = Arc::default();
    let recorded = events.clone();
    runtime
        .replay_reader(
            options,
            Cursor::new(std::fs::read(fixture(name)).expect("reviewed synthetic pcap")),
            Bridge {
                visibility: Arc::default(),
                party_changed: Arc::new(|_| {}),
                batch: None,
                snapshot: Arc::new(move |value| received.lock().unwrap().push(value)),
                event: Arc::new(move |value| recorded.lock().unwrap().push(value)),
            },
        )
        .expect("synthetic native replay");
    let final_snapshot = runtime.snapshot();
    let status = runtime.store_status();
    assert_eq!(
        status.stream_id.as_deref(),
        Some(final_snapshot.stream_id.as_str())
    );
    assert_eq!(status.durable, Some(final_snapshot.sequence));
    let home =
        ready_home(&runtime, session(&final_snapshot)).expect("native Home after replay stop");
    if name == "complete" {
        // The dungeon starts exactly at Chicago midnight. Today is half-open:
        // the prior day excludes the run and the new 23-hour day includes it.
        let midnight = OFFSET + 5_000_000;
        let day = |start, end| HomeScope::Today {
            range: TimeRange {
                start: Timestamp(start),
                end: Timestamp(end),
            },
        };
        let previous = ready_home(&runtime, day(midnight - 86_400_000_000, midnight)).unwrap();
        let current = ready_home(&runtime, day(midnight, midnight + 82_800_000_000)).unwrap();
        assert_eq!(previous.runs, 0);
        assert_eq!(current.runs, 1);
        assert_eq!(home.recent_runs[0].started, Timestamp(midnight));
    }
    let path = dir.path().join("demo.sqlite3");
    let reopened = open_reader(&path).expect("reopened database");
    assert_eq!(
        reopened
            .home_summary(
                session(&final_snapshot),
                reopened.current_account().unwrap().as_deref()
            )
            .unwrap(),
        home
    );
    if name == "zero-duration" {
        let story = reopened.get_run(&home.recent_runs[0].id).unwrap().unwrap();
        let fight = reopened.get_fight(&story.fights[0].id).unwrap().unwrap();
        assert_eq!(fight.run_id.as_ref(), Some(&story.run.id));
        let local = fight.players.iter().find(|player| player.is_local).unwrap();
        assert_eq!((local.damage, local.hits, local.dps), (100.0, 1, None));
        assert_eq!(fight.first_hit, fight.last_hit);
    } else {
        let story = reopened.get_run(&home.recent_runs[0].id).unwrap().unwrap();
        let fight = reopened.get_fight(&story.fights[0].id).unwrap().unwrap();
        let local = fight.players.iter().find(|player| player.is_local).unwrap();
        assert_eq!((local.damage, local.hits, local.dps), (300.0, 2, None));
        if name == "disconnected" {
            assert_eq!(fight.end_reason, Some(FightEnd::CaptureStopped));
        }
    }
    if name == "complete" {
        let maps: Vec<_> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|envelope| {
                if let EngineEvent::MapEntered { name, .. } = &envelope.event {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(maps, ["Nexus", "Realm", "Snake Pit", "Nexus"]);
    }
    drop(reopened);
    // Idempotent receipts: replaying every original envelope cannot double count
    // a run, death, bag or fame point, even after closing/reopening the reader.
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::apply_batch(&mut db, &events.lock().unwrap()).unwrap();
    drop(db);
    assert_eq!(
        ready_home(&runtime, session(&final_snapshot)).unwrap(),
        home
    );
    let mut output = snapshots.lock().unwrap().clone();
    assert!(!output.is_empty(), "Bridge must actually publish snapshots");
    if output.last() != Some(&final_snapshot) {
        output.push(final_snapshot);
    }
    (output, home)
}

/// Normalize ONLY opaque ID fields. No deletion of timestamps, coverage, values
/// or outcome evidence is allowed to make independent replay compare equal.
fn semantic_home(home: &HomeSummary) -> serde_json::Value {
    fn visit(value: &mut serde_json::Value, ids: &mut std::collections::BTreeMap<String, String>) {
        match value {
            serde_json::Value::Array(rows) => rows.iter_mut().for_each(|row| visit(row, ids)),
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(
                        key.as_str(),
                        "id" | "run_id" | "session_id" | "character_id" | "player_id"
                    ) {
                        if let Some(raw) = value.as_str() {
                            let next = format!("opaque-{}", ids.len());
                            let normalized = ids.entry(raw.to_owned()).or_insert(next).clone();
                            *value = serde_json::Value::String(normalized);
                        }
                    } else {
                        visit(value, ids);
                    }
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(home).unwrap();
    visit(&mut value, &mut Default::default());
    value
}

#[test]
fn native_replay_home_survives_stop_reopen_and_duplicate_receipts() {
    let (snapshots, home) = replay_home_fixture("complete");
    assert_eq!((home.runs, home.clears, home.deaths), (1, 1, 0));
    assert_eq!(home.recent_runs.len(), 1);
    assert_eq!(home.recent_details.len(), 1);
    assert_eq!(home.recent_details[0].run_id, home.recent_runs[0].id);
    assert_eq!(home.recent_details[0].observed_drop_count, 1);
    assert_eq!(home.recent_details[0].local_share, Some(1.0));
    assert!(home.top_fight.is_none());
    let bag = home.best_drop.as_ref().unwrap();
    assert_eq!(bag.bag_rank, Some(255));
    assert!(bag.contents_known);
    assert_eq!(bag.source_enemy, None);
    assert_eq!(home.recent_runs[0].outcome, Some(Outcome::Cleared));
    // finish() clears the world; the ordered MapEntered events above establish
    // the final Nexus visit, not a fabricated terminal snapshot location.
    assert_eq!(snapshots.last().unwrap().area_kind, AreaKind::Unknown);
    let (_, second) = replay_home_fixture("complete");
    assert_eq!(semantic_home(&home), semantic_home(&second));
}

#[test]
fn disconnected_partial_run_does_not_invent_completion_or_loot() {
    let (_, home) = replay_home_fixture("disconnected");
    assert_eq!(home.runs, 1);
    assert_eq!(home.clears, 0);
    let run = &home.recent_runs[0];
    assert!(run.ended.is_some());
    assert!(run.completed_at.is_none());
    assert!(!run.complete_capture);
    assert_eq!(run.outcome, Some(Outcome::Unknown));
    assert!(home.top_fight.is_none());
    assert_eq!(home.recent_details[0].observed_drop_count, 0);
    assert!(home.best_drop.is_none());
}

#[test]
fn zero_duration_packet_prefix_preserves_known_damage_without_inventing_dps() {
    let (_, home) = replay_home_fixture("zero-duration");
    assert_eq!(home.runs, 1);
    let run_id = &home.recent_runs[0].id;
    // The earlier Realm fight is a distinct candidate, never a manufactured
    // dungeon zero. The selected run's zero-duration fight has no known DPS.
    assert!(home
        .top_fight
        .as_ref()
        .is_none_or(|top| top.run_id.as_ref() != Some(run_id)));
}

fn frames(name: &str) -> Vec<RawFrame> {
    let mut reader = PcapReader::new(Cursor::new(std::fs::read(fixture(name)).unwrap())).unwrap();
    let mut frames = vec![];
    while let Some(frame) = reader.next_frame().unwrap() {
        frames.push(frame);
    }
    frames
}

#[test]
fn late_revision_updates_one_recent_run() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("counter.sqlite3");
    let views = Arc::new(Views::default());
    let events: Arc<Mutex<Vec<EventEnvelope>>> = Arc::default();
    let received = events.clone();
    let mut adapter = Adapter::open(
        path.clone(),
        Arc::new(
            serde_json::from_slice::<Catalog>(include_bytes!(
                "../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
            ))
            .unwrap(),
        ),
        EngineConfig {
            stream_id: "demo-home-counter".into(),
            identity_key: [7; 32],
            ..Default::default()
        },
        WriterConfig {
            stream_id: "demo-home-counter".into(),
            ..Default::default()
        },
        false,
        views.clone(),
        Bridge {
            event: Arc::new(move |event| received.lock().unwrap().push(event)),
            ..Default::default()
        },
    )
    .unwrap();
    let mut pipeline = Pipeline::new(Arc::new(AtomicBool::new(false)));
    for frame in frames("complete") {
        for event in pipeline.push(frame) {
            if matches!(&event, PipelineEvent::Decoded(DecodeEvent::Packet(packet)) if matches!(packet.packet, Packet::Notification(_)))
            {
                continue;
            }
            adapter.decoded(event).unwrap();
        }
    }
    adapter.writer_flush_for_test();
    adapter.publish_snapshot();
    let scope = session(&views.snapshot.lock().unwrap());
    let reader = open_reader(&path).unwrap();
    let before = reader
        .home_summary(scope.clone(), reader.current_account().unwrap().as_deref())
        .unwrap();
    assert_eq!((before.runs, before.clears), (1, 0));
    assert_eq!(before.recent_runs[0].outcome, Some(Outcome::Left));
    // Completion index 65 is bit 41. This is decoded packet injection, not a
    // claim that this variant traversed encrypted framing on disk.
    adapter
        .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(
            maxed_protocol::DecodedPacket {
                conn: ConnId(1),
                dir: Direction::ServerToClient,
                at: Timestamp(OFFSET + 12_010_000),
                id: 101,
                len: 0,
                alignment_verified: true,
                packet: Packet::CreateSuccess(maxed_protocol::defs::CreateSuccess {
                    objectId: 1,
                    charId: 1,
                    str: "AQAAAAAAAAAAAgAAAAAAAAAAAAAB".into(),
                }),
            },
        )))
        .unwrap();
    let mut second = Pipeline::new(Arc::new(AtomicBool::new(false)));
    for frame in frames("complete")
        .into_iter()
        .filter(|frame| frame.at.0 <= OFFSET + 1_020_000)
    {
        for event in second.push(frame) {
            if let PipelineEvent::Decoded(DecodeEvent::Packet(mut packet)) = event {
                if matches!(packet.packet, Packet::Update(_)) {
                    packet.at = Timestamp(OFFSET + 12_020_000);
                    adapter
                        .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(packet)))
                        .unwrap();
                }
            }
        }
    }
    adapter
        .finish(Timestamp(OFFSET + 12_030_000), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
    let after = reader
        .home_summary(scope.clone(), reader.current_account().unwrap().as_deref())
        .unwrap();
    assert_eq!((after.runs, after.clears), (1, 1));
    assert_eq!(after.recent_runs.len(), 1);
    assert_eq!(after.recent_runs[0].id, before.recent_runs[0].id);
    assert_eq!(after.recent_runs[0].outcome, Some(Outcome::Cleared));
    assert_eq!(after.recent_details[0].observed_drop_count, 1);
    drop(reader);
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::apply_batch(&mut db, &events.lock().unwrap()).unwrap();
    drop(db);
    assert_eq!(
        open_reader(&path)
            .unwrap()
            .home_summary(
                scope,
                open_reader(&path)
                    .unwrap()
                    .current_account()
                    .unwrap()
                    .as_deref()
            )
            .unwrap(),
        after
    );
}

#[test]
fn typed_death_after_clear_and_consecutive_sessions_keep_separate_facts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("typed.sqlite3");
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let snapshot = demo::demo_snapshot();
    let mut story = demo::demo_run_story();
    story.run.died_at = Some(Timestamp(6_000_000));
    story.run.ended = Some(Timestamp(6_000_000));
    let mut next_session = snapshot.session.clone().unwrap();
    next_session.id = "synthetic-next-session".into();
    next_session.started = Timestamp(2_000_000_000);
    next_session.last_traffic = next_session.started;
    let mut next_run = story.run.clone();
    next_run.id = "synthetic-next-run".into();
    next_run.session_id.clone_from(&next_session.id);
    next_run.started = next_session.started;
    next_run.ended = None;
    next_run.completed_at = None;
    next_run.died_at = None;
    next_run.outcome = None;
    next_run.evidence.clear();
    let mut old_session = snapshot.session.clone().unwrap();
    old_session.ended = Some(Timestamp(6_000_000));
    let events: Vec<_> = [
        EngineEvent::SessionUpdated(old_session),
        EngineEvent::CharacterUpdated(story.character.clone().unwrap()),
        EngineEvent::RunStarted(story.run.clone()),
        EngineEvent::PlayerDied {
            at: Timestamp(6_000_000),
            character_id: story.run.character_id.clone(),
            run_id: Some(story.run.id.clone()),
        },
        EngineEvent::RunEnded(story.run.clone()),
        EngineEvent::SessionUpdated(next_session.clone()),
        EngineEvent::RunStarted(next_run.clone()),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, event)| EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "synthetic-typed-home".into(),
        sequence: Sequence(Timestamp(index as u64 + 1)),
        at: Timestamp(index as u64 + 1),
        event,
    })
    .collect();
    maxed_store::apply_batch(&mut db, &events).unwrap();
    let reader = open_reader(&path).unwrap();
    let first = reader
        .home_summary(
            session(&snapshot),
            reader.current_account().unwrap().as_deref(),
        )
        .unwrap();
    assert_eq!((first.runs, first.clears, first.deaths), (1, 1, 1));
    assert!(first.recent_runs[0].completed_at.is_some());
    assert!(first.recent_runs[0].died_at.is_some());
    let second = reader
        .home_summary(
            HomeScope::Session {
                id: next_session.id,
            },
            reader.current_account().unwrap().as_deref(),
        )
        .unwrap();
    assert_eq!((second.runs, second.clears, second.deaths), (1, 0, 0));
    assert_eq!(second.recent_runs[0].id, next_run.id);
    maxed_store::apply_batch(&mut db, &events).unwrap();
    assert_eq!(
        reader
            .home_summary(
                session(&snapshot),
                reader.current_account().unwrap().as_deref()
            )
            .unwrap(),
        first
    );
}

#[test]
fn run_end_before_durability_retry_converges_to_one_summary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lag.sqlite3");
    let views = Arc::new(Views::default());
    let mut adapter = Adapter::open(
        path.clone(),
        Arc::new(
            serde_json::from_slice::<Catalog>(include_bytes!(
                "../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
            ))
            .unwrap(),
        ),
        EngineConfig {
            stream_id: "demo-home-lag".into(),
            identity_key: [7; 32],
            ..Default::default()
        },
        WriterConfig {
            stream_id: "demo-home-lag".into(),
            ..Default::default()
        },
        false,
        views.clone(),
        Bridge::default(),
    )
    .unwrap();
    let mut pipeline = Pipeline::new(Arc::new(AtomicBool::new(false)));
    for frame in frames("disconnected") {
        for event in pipeline.push(frame) {
            adapter.decoded(event).unwrap();
        }
    }
    adapter.writer_flush_for_test();
    adapter.publish_snapshot();
    let scope = session(&views.snapshot.lock().unwrap());
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(adapter
        .finish(Timestamp(OFFSET + 8_500_000), FightEnd::AppClosed)
        .is_err());
    let status = views.store.lock().unwrap().clone();
    let snapshot = views.snapshot.lock().unwrap().clone();
    assert_eq!(status.state, StoreState::Failed);
    assert!(status.queued > 0);
    assert!(status.durable.unwrap().0 .0 < snapshot.sequence.0 .0);
    let before = open_reader(&path)
        .unwrap()
        .home_summary(
            scope.clone(),
            open_reader(&path)
                .unwrap()
                .current_account()
                .unwrap()
                .as_deref(),
        )
        .unwrap();
    assert_eq!(before.runs, 1);
    assert!(before.recent_runs[0].ended.is_none());
    db.execute_batch("ROLLBACK").unwrap();
    adapter.retry_and_close().unwrap();
    let after = open_reader(&path)
        .unwrap()
        .home_summary(
            scope,
            open_reader(&path)
                .unwrap()
                .current_account()
                .unwrap()
                .as_deref(),
        )
        .unwrap();
    assert_eq!(after.runs, 1);
    assert_eq!(after.recent_runs.len(), 1);
    assert!(after.recent_runs[0].ended.is_some());
    assert!(after.recent_runs[0]
        .evidence
        .iter()
        .any(|point| point.evidence == Evidence::AppClosed));
    assert_eq!(views.store.lock().unwrap().queued, 0);
    assert_eq!(
        views.store.lock().unwrap().durable,
        Some(views.snapshot.lock().unwrap().sequence)
    );
}
