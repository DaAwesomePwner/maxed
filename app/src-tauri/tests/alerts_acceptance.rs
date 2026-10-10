//! Invented domain-event fixtures. These test the durable seam, not packet decoding.
use maxed_store::*;
use maxed_types::*;
use rusqlite::Connection;

fn submit(writer: &StoreWriter, mut event: EventEnvelope, context: AlertContext) {
    // Nonblocking submission may report Full while the writer holds its mutex,
    // even for an empty queue. Preserve the fixture's explicit commit boundaries.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match writer.try_submit_with_alert_context(event, context.clone()) {
            Ok(()) => return,
            Err(SubmitError::Full(returned)) if std::time::Instant::now() < deadline => {
                event = returned;
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => panic!("fixture submission failed: {error:?}"),
        }
    }
}

fn filter() -> AlertFilter {
    AlertFilter {
        kinds: vec![],
        exclude_kinds: vec![],
        kind: None,
        range: None,
        query: String::new(),
        cursor: None,
        limit: 100,
    }
}
fn config(stream: &str) -> WriterConfig {
    WriterConfig {
        stream_id: stream.into(),
        flush_millis: 60_000,
        ..Default::default()
    }
}
fn context(at: Timestamp) -> AlertContext {
    const DAY: u64 = 86_400_000_000;
    let date = at.0 / DAY * DAY;
    AlertContext {
        chat: None,
        live: true,
        preferences: AlertPreferences::default(),
        key_pop_dungeon: None,
        bag_window: Some(TimeRange {
            start: Timestamp(date.saturating_sub(6 * DAY)),
            end: Timestamp(date + DAY),
        }),
    }
}

#[test]
fn synthetic_fixture_pack_has_exact_durable_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.sqlite3");
    let writer = start_writer(path.clone(), config("live-fixture")).unwrap();
    let events: Vec<EventEnvelope> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/scrubbed/maxed-alerts/events.json"
    ))
    .unwrap();
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/scrubbed/maxed-alerts/expected.json"
    ))
    .unwrap();
    let mut sequence = 0;
    for event in events {
        sequence = sequence.max(event.sequence.0 .0);
        let at = event.at;
        submit(&writer, event, context(at));
        writer.flush().unwrap();
    }
    let ordinary: FightView = serde_json::from_str(include_str!(
        "../../../tests/fixtures/scrubbed/maxed-alerts/ordinary-fight.json"
    ))
    .unwrap();
    for i in 0..expected["ordinary_fights"].as_u64().unwrap() {
        let mut fight = ordinary.clone();
        fight.id = format!("ordinary-{i}");
        sequence += 1;
        let at = Timestamp(1_800_604_820_000_000);
        submit(
            &writer,
            EventEnvelope {
                fight_chart_omitted: false,
                stream_id: "live-fixture".into(),
                sequence: Sequence(Timestamp(sequence)),
                at,
                event: EngineEvent::FightEnded(fight),
            },
            context(at),
        );
        if i % 100 == 0 {
            writer.flush().unwrap();
        }
    }
    writer.flush().unwrap();
    let logs = open_reader(&path)
        .unwrap()
        .list_alerts(filter())
        .unwrap()
        .rows;
    assert_eq!(logs.len() as u64, expected["alerts"].as_u64().unwrap());
    for (kind, key) in [
        (AlertKind::WhiteBag, "white_bags"),
        (AlertKind::PersonalBest, "personal_bests"),
        (AlertKind::Death, "death_alerts"),
    ] {
        assert_eq!(
            logs.iter().filter(|v| v.kind == kind).count() as u64,
            expected[key].as_u64().unwrap()
        );
    }
    assert!(!logs.iter().any(|v| v.kind == AlertKind::FlawlessRun));
    for log in &logs {
        if let Some(AlertDetails::WhiteBag {
            drop_id, ordinal, ..
        }) = &log.details
        {
            assert_eq!(
                u64::from(ordinal.unwrap()),
                expected["ordinals"][drop_id].as_u64().unwrap()
            );
        }
    }
    let claims = writer.claim_alerts(32).unwrap();
    assert_eq!(claims.len(), logs.len());
    assert!(writer.claim_alerts(32).unwrap().is_empty());
    assert!(claims
        .iter()
        .all(|v| v.view.presentation == Some(v.loudness)));
    writer.shutdown().unwrap();
}

#[test]
fn claim_before_effect_crash_matrix() {
    // Restart exactly at persisted boundaries. We deliberately do not assert
    // exactly-once effects: a crash after claim can lose either external effect.
    for boundary in 0..4 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crash.sqlite3");
        let writer = start_writer(path.clone(), config("live-crash")).unwrap();
        let session = EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "live-crash".into(),
            sequence: Sequence(Timestamp(1)),
            at: Timestamp(180_000_000),
            event: EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
        };
        submit(&writer, session, context(Timestamp(180_000_000)));
        writer.flush().unwrap();
        let source = EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "live-crash".into(),
            sequence: Sequence(Timestamp(2)),
            at: Timestamp(180_000_000),
            event: {
                let mut bag = demo::demo_snapshot().drops.remove(0);
                bag.run_id = None;
                bag.bag_rank = Some(255);
                EngineEvent::BagDropped(bag)
            },
        };
        submit(&writer, source.clone(), context(source.at));
        writer.flush().unwrap();
        let mut effects = 0;
        if boundary >= 1 {
            let claim = writer.claim_alerts(32).unwrap().remove(0);
            assert_eq!(claim.committed_at, source.at);
            if boundary >= 2 {
                effects += 1;
            }
            if boundary == 3 {
                submit(
                    &writer,
                    EventEnvelope {
                        fight_chart_omitted: false,
                        stream_id: "live-crash".into(),
                        sequence: Sequence(Timestamp(3)),
                        at: claim.committed_at,
                        event: EngineEvent::AlertFired(claim.view),
                    },
                    context(source.at),
                );
                writer.flush().unwrap();
            }
        }
        writer.shutdown().unwrap();
        let writer = start_writer(path.clone(), config("live-crash")).unwrap();
        submit(&writer, source.clone(), context(source.at));
        writer.flush().unwrap();
        let recovered = writer.claim_alerts(32).unwrap();
        assert_eq!(recovered.len(), usize::from(boundary == 0));
        for claim in recovered {
            assert_eq!(claim.committed_at, Timestamp(180_000_000));
            assert_eq!(
                maxed_app::runtime::alerts::delivery::effective(
                    &claim,
                    &AlertPreferences::default(),
                    Timestamp(181_000_000)
                ),
                AlertLoudness::Banner
            );
            assert_eq!(
                maxed_app::runtime::alerts::delivery::effective(
                    &claim,
                    &AlertPreferences::default(),
                    Timestamp(211_000_000)
                ),
                AlertLoudness::QuietLogOnly
            );
            effects += 1;
        }
        assert!(effects <= 1);
        assert_eq!(
            open_reader(&path)
                .unwrap()
                .list_alerts(filter())
                .unwrap()
                .rows
                .len(),
            1
        );
        assert!(writer.claim_alerts(32).unwrap().is_empty());
        writer.shutdown().unwrap();
    }
}

#[test]
fn precommit_storage_failure_has_no_claim_and_retry_keeps_original_clock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("failure.sqlite3");
    let writer = start_writer(path.clone(), config("live-failure")).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER reject_alert BEFORE INSERT ON alert_occurrences BEGIN SELECT RAISE(ABORT, 'injected storage failure'); END;").unwrap();
    let events = [
        EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
        {
            let mut bag = demo::demo_snapshot().drops.remove(0);
            bag.run_id = None;
            bag.bag_rank = Some(255);
            EngineEvent::BagDropped(bag)
        },
    ];
    for (i, event) in events.into_iter().enumerate() {
        submit(
            &writer,
            EventEnvelope {
                fight_chart_omitted: false,
                stream_id: "live-failure".into(),
                sequence: Sequence(Timestamp(i as u64 + 1)),
                at: Timestamp(180_000_000),
                event,
            },
            context(Timestamp(180_000_000)),
        );
    }
    assert!(writer.flush().is_err());
    assert!(writer.claim_alerts(32).unwrap().is_empty());
    assert!(open_reader(&path)
        .unwrap()
        .list_alerts(filter())
        .unwrap()
        .rows
        .is_empty());
    db.execute_batch("DROP TRIGGER reject_alert").unwrap();
    writer.retry().unwrap();
    let claim = writer.claim_alerts(32).unwrap().remove(0);
    assert_eq!(claim.committed_at, Timestamp(180_000_000));
    assert_eq!(
        maxed_app::runtime::alerts::delivery::effective(
            &claim,
            &AlertPreferences::default(),
            Timestamp(211_000_000)
        ),
        AlertLoudness::QuietLogOnly
    );
    assert!(writer.claim_alerts(32).unwrap().is_empty());
    writer.shutdown().unwrap();
}
