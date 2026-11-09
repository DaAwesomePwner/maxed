use super::*;

fn large_event() -> EventEnvelope {
    let buckets: Vec<_> = (0..600)
        .map(|i| {
            serde_json::json!({
                "from": (i * 1000).to_string(), "until": ((i + 1) * 1000).to_string(),
                "total": 123.25, "local": 12.5, "others_damage": 110.75, "others_presence": 3.0
            })
        })
        .collect();
    let fight: FightView = serde_json::from_value(serde_json::json!({
        "id": "synthetic-fight", "run_id": null, "session_id": "synthetic-session",
        "target_id": 42, "target_generation": 1, "boss_type": null,
        "target_type": null, "max_hp": null, "started": "0", "ended": null,
        "first_hit": "0", "last_hit": "600000", "end_reason": null,
        "players": [], "total_damage": 73950.0, "unattributed_damage": 0.0,
        "buckets": buckets, "complete_capture": true, "issues": [],
        "player_series": [{"player_id": "synthetic-player", "object_id": 7,
            "is_local": true, "points": vec![12.5; 600]}]
    }))
    .unwrap();
    EventEnvelope {
        stream_id: "synthetic".into(),
        sequence: Sequence(Timestamp(1)),
        at: Timestamp(600000),
        fight_chart_omitted: false,
        event: EngineEvent::FightUpdated(fight),
    }
}

#[test]
fn large_fight_digest_pin() {
    let event = large_event();
    // Pin recorded from the original DOM/sorted-key implementation before changing submission.
    let (size, hash) = validate_event(&event).unwrap();
    assert!(size > 50_000);
    assert_eq!(validate_submission(&event).unwrap(), size);
    assert_eq!(
        hash,
        [
            15, 76, 84, 193, 55, 85, 11, 166, 197, 219, 170, 3, 0, 142, 239, 239, 177, 9, 63, 255,
            149, 148, 67, 87, 209, 41, 71, 134, 136, 204, 63, 170
        ]
    );
}

#[test]
fn submission_matches_reference_validation() {
    let base = large_event();
    let mut cases = vec![base.clone()];
    let mut empty = base.clone();
    empty.stream_id.clear();
    cases.push(empty);
    for n in [0, 1, i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
        let mut event = base.clone();
        event.sequence = Sequence(Timestamp(n));
        cases.push(event);
        let mut event = base.clone();
        event.at = Timestamp(n);
        cases.push(event);
        let mut event = base.clone();
        if let EngineEvent::FightUpdated(f) = &mut event.event {
            f.buckets[599].until = Timestamp(n);
        }
        cases.push(event);
    }
    for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, f64::MAX] {
        let mut event = base.clone();
        if let EngineEvent::FightUpdated(f) = &mut event.event {
            f.player_series.as_mut().unwrap()[0].points[599] = n;
        }
        cases.push(event);
    }
    let mut oversized = base;
    oversized.stream_id = "x".repeat(MAX_EVENT_BYTES);
    cases.push(oversized);
    for event in cases {
        match (validate_submission(&event), validate_event(&event)) {
            (Ok(actual), Ok((expected, _))) => assert_eq!(actual, expected),
            (Err(StoreError::InvalidEvent), Err(StoreError::InvalidEvent)) => {}
            other => panic!("validation mismatch: {other:?}"),
        }
    }
}

#[test]
fn streaming_timestamp_rules_cover_nested_keys_and_json_types() {
    use serde::de::DeserializeSeed;
    for key in [
        "at",
        "sequence",
        "started",
        "ended",
        "last_traffic",
        "observed_at",
        "taken_observed_at",
        "completed_at",
        "died_at",
        "first_hit",
        "last_hit",
        "from",
        "until",
    ] {
        for (value, valid) in [
            ("null", true),
            (r#""0""#, true),
            (r#""9223372036854775807""#, true),
            (r#""9223372036854775808""#, false),
            (r#""-1""#, false),
            (r#""not-a-time""#, false),
            ("1", false),
            ("1.5", false),
            ("true", false),
            ("[]", false),
            ("{}", false),
        ] {
            let json = format!(r#"{{"nested":[{{"{key}":{value}}}]}}"#);
            let result =
                TimestampVisitor(false).deserialize(&mut serde_json::Deserializer::from_str(&json));
            assert_eq!(result.is_ok(), valid, "{json}");
        }
    }
    // Escaped keys must be recognized; unrelated strings need no numeric check.
    for (json, valid) in [
        (r#"{"\u0061t":"9223372036854775808"}"#, false),
        (
            r#"{"name":"not-a-time","local_last_hit":"18446744073709551615"}"#,
            true,
        ),
    ] {
        assert_eq!(
            TimestampVisitor(false)
                .deserialize(&mut serde_json::Deserializer::from_str(json))
                .is_ok(),
            valid,
        );
    }
}

#[test]
fn large_fight_worker_persists_reference_receipt_and_rejects_invalid_synchronously() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.sqlite3");
    let writer = crate::start_writer(
        path.clone(),
        crate::WriterConfig {
            stream_id: "synthetic".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut fight = large_event();
    fight.sequence = Sequence(Timestamp(2));
    let expected = validate_event(&fight).unwrap().1;
    let mut invalid = fight.clone();
    if let EngineEvent::FightUpdated(f) = &mut invalid.event {
        f.buckets[599].from = Timestamp(u64::MAX);
    }
    assert!(matches!(
        writer.try_submit(invalid),
        Err(crate::SubmitError::Invalid(_))
    ));
    let mut session = large_event();
    session.event = EngineEvent::SessionUpdated(PlaySessionView {
        id: "synthetic-session".into(),
        started: Timestamp(0),
        ended: None,
        last_traffic: Timestamp(600000),
        issues: vec![],
    });
    for mut event in [session, fight] {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            match writer.try_submit(event) {
                Ok(()) => break,
                Err(crate::SubmitError::Full(returned)) => {
                    assert!(std::time::Instant::now() < deadline);
                    event = returned;
                    std::thread::yield_now();
                }
                error => panic!("unexpected submit result: {error:?}"),
            }
        }
        writer.flush().unwrap();
    }
    let conn = crate::migrate::open_writer(&path).unwrap();
    assert_eq!(
        crate::receipt_tests::lookup(&conn, "synthetic", 2).unwrap(),
        expected
    );
    writer.shutdown().unwrap();
}
