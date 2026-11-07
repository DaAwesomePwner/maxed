use super::*;
use maxed_engine::EngineConfig;
use maxed_recording::PcapReader;
use maxed_store::{open_reader, WriterConfig};
use maxed_types::*;
use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/scrubbed/maxed-demo")
        .join(name)
}
fn frames() -> Vec<RawFrame> {
    let mut reader =
        PcapReader::new(Cursor::new(std::fs::read(fixture("story.pcap")).unwrap())).unwrap();
    let mut frames = vec![];
    while let Some(frame) = reader.next_frame().unwrap() {
        frames.push(frame);
    }
    frames
}
type Events = Arc<Mutex<Vec<EventEnvelope>>>;
fn adapter(path: &Path, capacity: usize) -> (Adapter, Arc<Views>, Events) {
    let catalog: Catalog =
        serde_json::from_slice(&std::fs::read(fixture("catalog.json")).unwrap()).unwrap();
    let views = Arc::new(Views::default());
    let events: Events = Arc::default();
    let recorded = events.clone();
    let seen = events.clone();
    let bridge = Bridge {
        account_changed: Arc::new(|_| {}),
        visibility: Arc::default(),
        party_changed: Arc::new(|_| {}),
        batch: None,
        event: Arc::new(move |event| recorded.lock().unwrap().push(event)),
        snapshot: Arc::new(move |snapshot| {
            let events = seen.lock().unwrap();
            assert_eq!(snapshot.sequence.0 .0, events.len() as u64);
        }),
    };
    let adapter = Adapter::open(
        path.into(),
        Arc::new(catalog),
        EngineConfig {
            stream_id: "demo-join".into(),
            identity_key: [7; 32],
            ..Default::default()
        },
        WriterConfig {
            stream_id: "demo-join".into(),
            capacity,
            batch_size: capacity.min(128),
            flush_millis: 60_000,
        },
        false,
        views.clone(),
        bridge,
    )
    .unwrap();
    (adapter, views, events)
}
fn feed(adapter: &mut Adapter, until: u64) -> Pipeline {
    let mut pipeline = Pipeline::new(Arc::new(AtomicBool::new(false)));
    for frame in frames().into_iter().filter(|f| f.at.0 <= until) {
        if maxed_capture::parse_frame(&frame)
            .ok()
            .flatten()
            .is_some_and(|s| !s.bytes.is_empty())
        {
            adapter.traffic(frame.at).unwrap();
        }
        for event in pipeline.push(frame) {
            adapter.decoded(event).unwrap();
        }
    }
    pipeline
}
fn filter() -> RunFilter {
    RunFilter {
        character_id: None,
        dungeon: None,
        outcome: None,
        range: None,
        sort: RunSort::Newest,
        cursor: None,
        limit: 30,
    }
}

#[test]
fn replay_to_rows_reopens_with_breakdown_and_idempotent_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("replay.sqlite3");
    let (mut adapter, views, events) = adapter(&path, 2);
    adapter.ephemeral = true;
    adapter
        .capture(CaptureStatus {
            state: CaptureState::WaitingForGame,
            ..Default::default()
        })
        .unwrap();
    let mut pipeline = feed(&mut adapter, u64::MAX);
    for event in pipeline.finish() {
        adapter.decoded(event).unwrap();
    }
    adapter
        .finish(adapter.engine.snapshot().at, FightEnd::CaptureStopped)
        .unwrap();
    let count = events.lock().unwrap().len();
    adapter
        .finish(adapter.engine.snapshot().at, FightEnd::AppClosed)
        .unwrap();
    assert_eq!(events.lock().unwrap().len(), count, "finalize exactly once");
    adapter.close().unwrap();
    assert_eq!(
        views.store.lock().unwrap().durable,
        Some(views.snapshot.lock().unwrap().sequence)
    );
    drop(adapter);
    let reader = open_reader(&path).unwrap();
    let page = reader.list_runs(filter()).unwrap();
    assert_eq!(page.rows.len(), 1);
    let story = reader.get_run(&page.rows[0].id).unwrap().unwrap();
    assert_eq!(story.run.outcome, Some(Outcome::Cleared));
    assert_eq!(story.drops.len(), 1);
    assert_eq!(
        story
            .timeline
            .iter()
            .filter(|e| e.kind == TimelineKind::Completion)
            .count(),
        1
    );
    let fight = reader.get_fight(&story.fights[0].id).unwrap().unwrap();
    assert_eq!(fight.total_damage, 300.);
    let local = fight.players.iter().find(|p| p.is_local).unwrap();
    let player = reader
        .get_player_by_id(&fight.id, &local.player_id)
        .unwrap()
        .unwrap();
    assert_eq!(player.damage, 300.);
    assert_eq!(player.dps, None);
    assert_eq!(player.sources[0].item_type, Some(256));
    assert_eq!(player.sources[0].damage, 300.);
    let envelopes = events.lock().unwrap().clone();
    for (index, e) in envelopes.iter().enumerate() {
        assert_eq!(e.sequence.0 .0, index as u64 + 1);
    }
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    maxed_store::apply_batch(&mut db, &envelopes).unwrap();
    maxed_store::apply_batch(&mut db, &envelopes).unwrap();
    assert_eq!(reader.get_run(&story.run.id).unwrap().unwrap(), story);
    assert_eq!(reader.list_runs(filter()).unwrap().rows.len(), 1);
    assert_eq!(super::receipt_tests::count(&db), count as i64);
}

#[test]
fn app_closed_and_user_stop_keep_distinct_terminal_evidence() {
    for reason in [FightEnd::AppClosed, FightEnd::CaptureStopped] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("story.sqlite3");
        let (mut adapter, _, _) = adapter(&path, 1024);
        let mut pipeline = feed(&mut adapter, 8_020_000);
        // Directional closes cannot finalize the open run.
        adapter.tick(Timestamp(8_250_000)).unwrap();
        let closes = pipeline.finish_at(adapter.engine.snapshot().at);
        assert!(!closes.is_empty());
        for event in closes {
            adapter.decoded(event).unwrap();
        }
        assert!(!adapter
            .engine
            .snapshot()
            .issues
            .contains(&CoverageIssue::ClockRegression));
        assert!(adapter.engine.snapshot().run.unwrap().ended.is_none());
        adapter
            .finish(Timestamp(8_500_000), reason.clone())
            .unwrap();
        adapter.close().unwrap();
        let reader = open_reader(&path).unwrap();
        let run = reader.list_runs(filter()).unwrap().rows.remove(0);
        let story = reader.get_run(&run.id).unwrap().unwrap();
        assert_eq!(run.outcome, Some(Outcome::Unknown));
        assert_eq!(
            reader
                .get_fight(&story.fights[0].id)
                .unwrap()
                .unwrap()
                .end_reason,
            Some(reason.clone())
        );
        assert_eq!(
            run.evidence
                .iter()
                .any(|e| e.evidence == Evidence::AppClosed),
            reason == FightEnd::AppClosed
        );
    }
}

#[test]
fn failed_final_flush_retains_pending_and_retries_without_refinalizing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("failure.sqlite3");
    let (mut adapter, views, events) = adapter(&path, 1024);
    let _pipeline = feed(&mut adapter, 8_020_000);
    // Commit the entry, then cause a real SQLite busy error in the final flush.
    adapter.writer_flush_for_test();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(adapter
        .finish(Timestamp(8_500_000), FightEnd::AppClosed)
        .is_err());
    let status = views.store.lock().unwrap().clone();
    assert_eq!(status.state, StoreState::Failed);
    assert!(status.queued > 0);
    assert!(status.durable.unwrap().0 .0 < views.snapshot.lock().unwrap().sequence.0 .0);
    let count = events.lock().unwrap().len();
    assert!(adapter
        .finish(Timestamp(9_000_000), FightEnd::AppClosed)
        .is_err());
    assert_eq!(events.lock().unwrap().len(), count);
    db.execute_batch("ROLLBACK").unwrap();
    adapter.retry_and_close().unwrap();
    assert_eq!(views.store.lock().unwrap().queued, 0);
    let reader = open_reader(&path).unwrap();
    let runs = reader.list_runs(filter()).unwrap();
    assert_eq!(runs.rows.len(), 1);
    assert!(runs.rows[0]
        .evidence
        .iter()
        .any(|e| e.evidence == Evidence::AppClosed));
}

#[test]
fn failed_shutdown_transfers_accepted_and_rejected_events_in_original_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shutdown.sqlite3");
    let (mut adapter, views, events) = adapter(&path, 2);
    let _pipeline = feed(&mut adapter, 8_020_000);
    adapter.writer_flush_for_test();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(adapter
        .finish(Timestamp(8_500_000), FightEnd::AppClosed)
        .is_err());
    assert!(
        views.store.lock().unwrap().queued > 2,
        "retain both writer-accepted and adapter-rejected events"
    );
    adapter.fail_shutdown_for_test();
    db.execute_batch("ROLLBACK").unwrap();
    adapter.retry_and_close().unwrap();
    assert_eq!(views.store.lock().unwrap().queued, 0);
    let reader = open_reader(&path).unwrap();
    let runs = reader.list_runs(filter()).unwrap();
    assert_eq!(runs.rows.len(), 1);
    let story = reader.get_run(&runs.rows[0].id).unwrap().unwrap();
    assert_eq!(story.fights[0].total_damage, 300.);
    assert_eq!(
        reader
            .get_fight(&story.fights[0].id)
            .unwrap()
            .unwrap()
            .end_reason,
        Some(FightEnd::AppClosed)
    );
    let count: i64 = super::receipt_tests::count_stream(&db, "demo-join");
    assert_eq!(count, events.lock().unwrap().len() as i64);
}

#[test]
fn cancellation_still_returns_each_directional_teardown_once() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut pipeline = Pipeline::new(cancel.clone());
    for frame in frames().into_iter().take(3) {
        pipeline.push(frame);
    }
    cancel.store(true, Ordering::Release);
    let closed = pipeline.finish();
    assert_eq!(closed.len(), 2);
    assert!(closed.iter().all(|e| matches!(
        e,
        PipelineEvent::Decoded(DecodeEvent::Diagnostic {
            kind: Diagnostic::Cancelled,
            ..
        })
    )));
    assert!(pipeline.finish().is_empty());
}

#[test]
fn native_worker_teardown_is_not_an_unexpected_tcp_interruption() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, _, events) = adapter(&dir.path().join("teardown"), 1024);
    let mut pipeline = feed(&mut adapter, 6_020_000);
    assert!(adapter.engine.snapshot().run.is_some());
    assert!(!adapter.engine.snapshot().fights.is_empty());
    stop_pipeline(&mut pipeline, &mut adapter).unwrap();
    let run = adapter.engine.snapshot().run.unwrap();
    assert!(run.complete_capture);
    assert!(!run
        .evidence
        .iter()
        .any(|e| e.evidence == Evidence::CaptureGap));
    adapter.finish(adapter.at(), FightEnd::AppClosed).unwrap();
    assert!(events.lock().unwrap().iter().any(|e| matches!(&e.event,
        EngineEvent::RunEnded(r) if r.outcome == Some(Outcome::Unknown)
        && r.evidence.iter().any(|e| e.evidence == Evidence::AppClosed)
        && !r.evidence.iter().any(|e| e.evidence == Evidence::CaptureGap))));
    adapter.close().unwrap();
}

#[test]
fn replay_eof_mid_run_saves_no_tcp_capture_gap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("replay-eof");
    let (mut adapter, _, _) = adapter(&path, 1024);
    let mut pipeline = feed(&mut adapter, 6_020_000);
    for event in pipeline.finish() {
        adapter.decoded(event).unwrap();
    }
    assert!(!adapter
        .engine
        .snapshot()
        .run
        .unwrap()
        .issues
        .contains(&CoverageIssue::StreamGap));
    adapter
        .finish(adapter.at(), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
    let reader = open_reader(&path).unwrap();
    let run = reader.list_runs(filter()).unwrap().rows.remove(0);
    assert_eq!(run.outcome, Some(Outcome::Unknown));
    assert!(!run.issues.contains(&CoverageIssue::StreamGap));
    assert!(!run
        .evidence
        .iter()
        .any(|e| e.evidence == Evidence::CaptureGap));
    assert!(reader
        .get_run(&run.id)
        .unwrap()
        .unwrap()
        .fights
        .iter()
        .all(|f| !reader
            .get_fight(&f.id)
            .unwrap()
            .unwrap()
            .issues
            .contains(&CoverageIssue::StreamGap)));
}

#[test]
fn normal_death_then_server_close_saves_died_with_coverage_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("death-close");
    let (mut adapter, _, _) = adapter(&path, 1024);
    let _pipeline = feed(&mut adapter, 6_020_000);
    adapter
        .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(
            maxed_protocol::DecodedPacket {
                conn: ConnId(1),
                dir: Direction::ServerToClient,
                at: Timestamp(7_000_000),
                id: 46,
                len: 0,
                alignment_verified: true,
                packet: Packet::Death(maxed_protocol::defs::Death {
                    accountId: "DemoAccount".into(),
                    charId: 1,
                    killedBy: "Demo Serpent".into(),
                    gravestoneType: 0,
                    totalFame: 125,
                    fameData: vec![],
                    stats: String::new(),
                }),
            },
        )))
        .unwrap();
    let before = adapter.engine.snapshot();
    let run = before.run.unwrap();
    assert_eq!(run.outcome, Some(Outcome::Died));
    assert!(run.complete_capture);
    adapter
        .decoded(PipelineEvent::Decoded(DecodeEvent::Diagnostic {
            conn: ConnId(1),
            dir: Direction::ServerToClient,
            at: Timestamp(7_001_000),
            kind: Diagnostic::Closed,
        }))
        .unwrap();
    assert_eq!(adapter.engine.snapshot().run, Some(run.clone()));
    assert_eq!(adapter.engine.snapshot().fights, before.fights);
    adapter
        .finish(adapter.at(), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
    let reader = open_reader(&path).unwrap();
    assert_eq!(reader.get_run(&run.id).unwrap().unwrap().run, run);
}

#[test]
fn idle_timer_expires_session_without_new_game_traffic() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, _, _) = adapter(&dir.path().join("idle"), 1024);
    let _pipeline = feed(&mut adapter, 8_020_000);
    let last = adapter.engine.snapshot().session.unwrap().last_traffic;
    adapter
        .tick(Timestamp(last.0 + 30 * 60 * 1_000_000))
        .unwrap();
    assert_eq!(adapter.engine.snapshot().session.unwrap().ended, Some(last));
    adapter
        .finish(
            Timestamp(last.0 + 30 * 60 * 1_000_000),
            FightEnd::CaptureStopped,
        )
        .unwrap();
    adapter.close().unwrap();
}

#[test]
fn next_area_counter_revises_left_to_cleared_once_in_storage() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("counter.sqlite3");
    let (mut adapter, _, events) = adapter(&path, 1024);
    let mut pipeline = Pipeline::new(Arc::new(AtomicBool::new(false)));
    for frame in frames() {
        for event in pipeline.push(frame) {
            // Keep the authored story's exit, but remove its Victory evidence.
            if matches!(&event, PipelineEvent::Decoded(DecodeEvent::Packet(p)) if matches!(p.packet, Packet::Notification(_)))
            {
                continue;
            }
            adapter.decoded(event).unwrap();
        }
    }
    adapter.writer_flush_for_test();
    let reader = open_reader(&path).unwrap();
    let run = reader.list_runs(filter()).unwrap().rows.remove(0);
    assert_eq!(run.outcome, Some(Outcome::Left));
    // Completion index 65 uses wire bit 41. Extended count = one.
    let conn = ConnId(1);
    let packet = |at, packet| {
        PipelineEvent::Decoded(DecodeEvent::Packet(maxed_protocol::DecodedPacket {
            conn,
            dir: Direction::ServerToClient,
            at: Timestamp(at),
            id: 101,
            len: 0,
            alignment_verified: true,
            packet,
        }))
    };
    adapter
        .decoded(packet(
            12_010_000,
            Packet::CreateSuccess(maxed_protocol::defs::CreateSuccess {
                objectId: 1,
                charId: 1,
                str: "AQAAAAAAAAAAAgAAAAAAAAAAAAAB".into(),
            }),
        ))
        .unwrap();
    // Re-observe the same synthetic character in the next area via the real decoder.
    let mut second = Pipeline::new(Arc::new(AtomicBool::new(false)));
    for frame in frames().into_iter().filter(|f| f.at.0 <= 1_020_000) {
        for event in second.push(frame) {
            if let PipelineEvent::Decoded(DecodeEvent::Packet(mut p)) = event {
                if matches!(p.packet, Packet::Update(_)) {
                    p.at = Timestamp(12_020_000);
                    adapter
                        .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(p)))
                        .unwrap();
                }
            }
        }
    }
    adapter
        .finish(Timestamp(12_030_000), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
    let story = reader.get_run(&run.id).unwrap().unwrap();
    assert_eq!(story.run.outcome, Some(Outcome::Cleared));
    assert_eq!(
        story
            .timeline
            .iter()
            .filter(|e| e.kind == TimelineKind::Completion)
            .count(),
        1
    );
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    maxed_store::apply_batch(&mut db, &events.lock().unwrap()).unwrap();
    assert_eq!(reader.get_run(&run.id).unwrap().unwrap(), story);
}

#[test]
fn unavailable_catalog_and_unverified_packets_stay_explicitly_partial() {
    let dir = tempfile::tempdir().unwrap();
    let views = Arc::new(Views::default());
    let mut adapter = Adapter::open(
        dir.path().join("partial"),
        join::unavailable_catalog(),
        EngineConfig {
            stream_id: "partial".into(),
            ..Default::default()
        },
        WriterConfig {
            stream_id: "partial".into(),
            ..Default::default()
        },
        true,
        views.clone(),
        Bridge::default(),
    )
    .unwrap();
    assert!(views
        .snapshot
        .lock()
        .unwrap()
        .issues
        .contains(&CoverageIssue::MissingCatalog));
    let mut pipeline = Pipeline::new(Arc::new(AtomicBool::new(false)));
    for frame in frames().into_iter().filter(|f| f.at.0 <= 1_020_000) {
        for event in pipeline.push(frame) {
            if let PipelineEvent::Decoded(DecodeEvent::Packet(mut p)) = event {
                p.alignment_verified = false;
                adapter
                    .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(p)))
                    .unwrap();
            }
        }
    }
    adapter.publish_snapshot();
    let snapshot = views.snapshot.lock().unwrap().clone();
    assert!(snapshot.character.is_none());
    assert!(snapshot.map.is_none());
    assert!(snapshot.session.is_none());
    assert!(snapshot.issues.contains(&CoverageIssue::Midstream));
    assert!(snapshot.capture.message.starts_with("Ignored "));
    assert!(adapter.engine.ignored_unverified_packets() > 0);
    adapter
        .finish(Timestamp(0), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
}

#[test]
fn presentation_is_coalesced_and_native_status_preserves_engine_diagnostics() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, views, events) = adapter(&dir.path().join("coalesced"), 1024);
    let waiting = CaptureStatus {
        state: CaptureState::WaitingForGame,
        ..Default::default()
    };
    adapter.capture(waiting.clone()).unwrap();
    for _ in 0..1000 {
        adapter.capture(waiting.clone()).unwrap();
    }
    assert_eq!(
        adapter.snapshot_builds, 1,
        "no deep snapshot on status-only loop iterations"
    );
    assert_eq!(events.lock().unwrap().len(), 1);
    adapter.publish_snapshot();
    assert_eq!(adapter.snapshot_builds, 2);
    adapter
        .decoded(PipelineEvent::Decoded(DecodeEvent::Diagnostic {
            conn: ConnId(1),
            dir: Direction::ServerToClient,
            at: Timestamp(1),
            kind: Diagnostic::Recovering,
        }))
        .unwrap();
    let count = events.lock().unwrap().len();
    for _ in 0..1000 {
        adapter
            .capture(CaptureStatus {
                state: CaptureState::Recovering,
                ..Default::default()
            })
            .unwrap();
    }
    assert_eq!(
        events.lock().unwrap().len(),
        count,
        "no duplicate sanitized capture envelope"
    );
    assert_eq!(
        adapter.snapshot_builds, 2,
        "packets only mark presentation dirty"
    );
    adapter.tick(Timestamp(1)).unwrap();
    adapter.publish_due(Duration::from_millis(1000));
    assert_eq!(
        views.snapshot.lock().unwrap().capture.message,
        "Recovering capture"
    );
    assert_eq!(adapter.snapshot_builds, 3);
    adapter
        .finish(Timestamp(1), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
}

#[test]
fn clock_tick_cannot_overtake_a_delayed_batch() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, _, _) = adapter(&dir.path().join("clock"), 1024);
    feed(&mut adapter, 8_020_000);
    let arrival = Instant::now();
    let mut clock = CaptureClock::default();
    clock.frame(Timestamp(8_020_000), arrival);
    let wall_tick = arrival + Duration::from_millis(250);
    adapter.tick(clock.tick(wall_tick).unwrap()).unwrap();
    // Older than the wall-clock estimate (8.270 s), but ordered capture time.
    adapter.traffic(Timestamp(8_030_000)).unwrap();
    let snapshot = adapter.engine.snapshot();
    assert!(!snapshot.issues.contains(&CoverageIssue::ClockRegression));
    assert!(!snapshot
        .run
        .unwrap()
        .issues
        .contains(&CoverageIssue::ClockRegression));
    assert!(snapshot
        .fights
        .iter()
        .all(|f| !f.issues.contains(&CoverageIssue::ClockRegression)));
    assert_eq!(
        clock.tick(arrival + Duration::from_secs(5)),
        Some(Timestamp(8_020_000))
    );
    assert_eq!(
        clock.tick(arrival + Duration::from_secs(6)),
        Some(Timestamp(9_020_000))
    );
    adapter
        .finish(Timestamp(8_030_000), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
}

#[test]
fn live_idle_clock_uses_arrivals_across_both_capture_clock_jumps() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, _, _) = adapter(&dir.path().join("live-clock"), 1024);
    feed(&mut adapter, 8_020_000);
    let session = adapter.engine.snapshot().session.unwrap().id;
    let mut clock = CaptureClock::default();
    let arrival = Instant::now();
    clock.frame(adapter.at(), arrival);
    let next = arrival + Duration::from_millis(10);
    adapter.live_traffic(Timestamp(3_608_020_000)).unwrap();
    clock.frame(adapter.at(), next);
    let back = next + Duration::from_millis(10);
    adapter.live_traffic(Timestamp(8_030_000)).unwrap();
    clock.frame(adapter.at(), back);
    let before = back + Duration::from_secs(1805) - Duration::from_micros(1);
    adapter
        .live_tick(clock.tick(before).unwrap(), clock.quiet(before))
        .unwrap();
    assert_eq!(adapter.engine.snapshot().session.unwrap().id, session);
    assert!(adapter.engine.snapshot().session.unwrap().ended.is_none());
    let idle = back + Duration::from_secs(1805);
    adapter
        .live_tick(clock.tick(idle).unwrap(), clock.quiet(idle))
        .unwrap();
    assert_eq!(
        adapter.engine.snapshot().session.unwrap().ended,
        Some(Timestamp(3_608_020_000))
    );
    adapter
        .finish(adapter.at(), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
}

#[test]
fn explicit_exit_without_saving_releases_failed_storage_and_keeps_committed_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("discard.sqlite3");
    let (mut adapter, views, _) = adapter(&path, 1024);
    feed(&mut adapter, 8_020_000);
    adapter.tick(Timestamp(8_300_000)).unwrap();
    adapter.writer_flush_for_test();
    adapter.refresh_store();
    let durable = views.store.lock().unwrap().durable;
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(adapter
        .finish(Timestamp(8_500_000), FightEnd::AppClosed)
        .is_err());
    let runtime = CaptureRuntime::default();
    *runtime.shared.recovery.lock().unwrap() = Some(adapter);
    assert!(runtime.shutdown().is_err());
    assert!(!runtime.closing.load(Ordering::Acquire));
    runtime.exit_without_saving();
    assert!(!runtime.has_unsaved_summaries());
    assert!(runtime.shutdown().is_ok());
    assert_eq!(views.store.lock().unwrap().queued, 0);
    assert_eq!(views.store.lock().unwrap().durable, durable);
    db.execute_batch("ROLLBACK").unwrap();
    let reader = open_reader(&path).unwrap();
    let run = reader.list_runs(filter()).unwrap().rows.remove(0);
    assert!(
        run.ended.is_none(),
        "discard did not claim terminal events were durable"
    );
    assert_eq!(
        reader.get_run(&run.id).unwrap().unwrap().fights[0].total_damage,
        300.
    );
}

#[test]
fn older_non_payload_frame_cannot_regress_the_next_tick() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, _, _) = adapter(&dir.path().join("clock-order"), 1024);
    feed(&mut adapter, 8_020_000);
    let arrival = Instant::now();
    let mut clock = CaptureClock::default();
    clock.frame(Timestamp(8_020_000), arrival);
    // This frame has no payload and never advances the engine.
    let frame = RawFrame {
        index: 99,
        at: Timestamp(8_010_000),
        original_len: 0,
        bytes: vec![],
    };
    assert!(maxed_capture::parse_frame(&frame).ok().flatten().is_none());
    clock.frame(frame.at, arrival + Duration::from_millis(10));
    adapter
        .tick(clock.tick(arrival + Duration::from_millis(250)).unwrap())
        .unwrap();
    assert!(!adapter
        .engine
        .snapshot()
        .issues
        .contains(&CoverageIssue::ClockRegression));
    assert_eq!(adapter.at(), Timestamp(8_020_000));
    adapter
        .finish(adapter.at(), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
}

#[test]
fn recovering_message_does_not_leak_into_waiting_state_or_rows() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, views, events) = adapter(&dir.path().join("status"), 1024);
    adapter
        .decoded(PipelineEvent::Decoded(DecodeEvent::Diagnostic {
            conn: ConnId(1),
            dir: Direction::ServerToClient,
            at: Timestamp(1),
            kind: Diagnostic::Recovering,
        }))
        .unwrap();
    // Counter-only reconciliation retains a diagnostic belonging to this state.
    adapter
        .capture(CaptureStatus {
            state: CaptureState::Recovering,
            decode_failures: 1,
            ..Default::default()
        })
        .unwrap();
    adapter.publish_snapshot();
    assert_eq!(
        views.snapshot.lock().unwrap().capture.message,
        "Recovering capture"
    );
    adapter
        .capture(CaptureStatus {
            state: CaptureState::WaitingForGame,
            ..Default::default()
        })
        .unwrap();
    adapter.publish_snapshot();
    assert_eq!(
        views.snapshot.lock().unwrap().capture.message,
        sanitize(&CaptureState::WaitingForGame)
    );
    assert!(matches!(&events.lock().unwrap().last().unwrap().event,
        EngineEvent::CaptureStateChanged(status) if status.message == sanitize(&CaptureState::WaitingForGame)));
    adapter
        .finish(Timestamp(1), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
    assert_eq!(
        views.store.lock().unwrap().durable,
        Some(views.snapshot.lock().unwrap().sequence)
    );
}

#[test]
fn last_demo_remains_queryable_and_next_playback_replaces_it() {
    let namespace = Namespace::Demo;
    let dir = tempfile::tempdir().unwrap();
    let runtime = CaptureRuntime::default();
    let play = || {
        let mut options = RuntimeOptions::live(dir.path().into(), None);
        options.namespace = namespace;
        runtime
            .replay_reader(
                options,
                Cursor::new(std::fs::read(fixture("story.pcap")).unwrap()),
                Bridge::default(),
            )
            .unwrap();
    };
    play();
    let first = runtime.snapshot();
    assert_eq!(runtime.store_status().durable, Some(first.sequence));
    let path = dir.path().join(format!("{}.sqlite3", namespace.label()));
    {
        let reader = open_reader(&path).unwrap();
        let runs = reader.list_runs(filter()).unwrap();
        assert_eq!(runs.rows.len(), 1);
        let story = reader.get_run(&runs.rows[0].id).unwrap().unwrap();
        assert!(!story.fights.is_empty());
        let fight = reader.get_fight(&story.fights[0].id).unwrap().unwrap();
        let local = fight.players.iter().find(|p| p.is_local).unwrap();
        assert!(reader
            .get_player_by_id(&fight.id, &local.player_id)
            .unwrap()
            .is_some());
    }
    play();
    assert_ne!(runtime.snapshot().stream_id, first.stream_id);
    let reader = open_reader(&path).unwrap();
    assert_eq!(reader.list_runs(filter()).unwrap().rows.len(), 1);
}

#[test]
fn query_clones_remain_consistent_during_publication() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, views, _) = adapter(&dir.path().join("concurrent"), 1024);
    let mut runtime = CaptureRuntime::default();
    runtime.shared = Arc::new(Shared {
        views,
        ..Default::default()
    });
    let runtime = Arc::new(runtime);
    let query = runtime.clone();
    let reader = thread::spawn(move || {
        for _ in 0..1000 {
            let snapshot = query.snapshot();
            assert_eq!(snapshot.stream_id, "demo-join");
            assert!(snapshot.sequence.0 .0 <= 100);
        }
    });
    for count in 1..=100 {
        adapter
            .capture(CaptureStatus {
                state: CaptureState::WaitingForGame,
                unknown_packets: count,
                ..Default::default()
            })
            .unwrap();
        adapter.publish_snapshot();
    }
    reader.join().unwrap();
    assert_eq!(runtime.snapshot().sequence.0 .0, 100);
    adapter
        .finish(adapter.at(), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
}

#[test]
fn review1_adapter_defaults_to_non_live() {
    let dir = tempfile::tempdir().unwrap();
    let (mut adapter, _, _) = adapter(&dir.path().join("default.sqlite3"), 32);
    assert!(!adapter.live);
    assert!(!adapter.ephemeral);
    adapter.close().unwrap();
}

#[test]
fn default_adapter_discard_preserves_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("default-discard.sqlite3");
    let (mut adapter, _, _) = adapter(&path, 32);
    assert!(!adapter.live);
    assert!(!adapter.ephemeral);
    adapter.close().unwrap();
    assert!(path.exists());
    adapter.discard();
    assert!(path.exists());
}
