mod common;
use common::*;
use maxed_types::*;
use std::time::Duration;

#[test]
fn ten_thousand_payload_frames_publish_only_heartbeats_and_idle_still_expires() {
    let mut e = engine();
    let mut updates = 0;
    for frame in 0..10_000 {
        let at = Timestamp(frame * 60_000_000 / 9_999);
        let events = e.observe_traffic(at);
        updates += events
            .iter()
            .filter(|e| matches!(e, EngineEvent::SessionUpdated(_)))
            .count();
        // Multiple decoded packets in the frame must not count as more traffic.
        for _ in 0..2 {
            assert!(e
                .apply_observed(maxed_protocol::DecodedPacket {
                    conn: ConnId(1),
                    dir: Direction::ServerToClient,
                    at,
                    id: 10,
                    len: 0,
                    alignment_verified: true,
                    packet: tick(vec![]),
                })
                .iter()
                .all(|e| !matches!(e, EngineEvent::SessionUpdated(_))));
        }
    }
    assert!((2..=3).contains(&updates), "{updates} session updates");
    assert_eq!(
        e.snapshot().session.unwrap().last_traffic,
        Timestamp(60_000_000)
    );
    e.advance_to(Timestamp(1_859_999_999));
    assert!(e.snapshot().session.unwrap().ended.is_none());
    let events = e.advance_to(Timestamp(1_860_000_000));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, EngineEvent::SessionUpdated(_)))
            .count(),
        1
    );
    assert_eq!(
        e.snapshot().session.unwrap().ended,
        Some(Timestamp(60_000_000))
    );
}

#[test]
fn continuous_traffic_clock_jumps_preserve_session_and_backward_durations() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3_000, damage(1, 1, 100));
    let session = e.snapshot().session.unwrap().id;
    let run = e.snapshot().run.unwrap().id;
    // Two consecutive arrivals, separated by a forward capture-clock correction.
    e.observe_live_traffic(Timestamp(3_600_003_000));
    e.advance_live_to(Timestamp(3_600_004_000), Duration::from_millis(1));
    assert_eq!(e.snapshot().session.unwrap().id, session);
    assert_eq!(e.snapshot().run.unwrap().id, run);
    assert!(e.snapshot().session.unwrap().ended.is_none());
    assert!(e.snapshot().character.unwrap().identity_verified);
    apply(&mut e, 4_000, tick(vec![]));
    let snapshot = e.snapshot();
    assert_eq!(snapshot.session.unwrap().id, session);
    assert!(snapshot.issues.contains(&CoverageIssue::ClockRegression));
    let events = e.advance_live_to(Timestamp(3_600_004_000), Duration::from_secs(1800));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::RunEnded(r)
        if r.ended.unwrap() >= r.started && r.outcome == Some(Outcome::Unknown))));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::FightEnded(f)
        if f.ended.unwrap() >= f.started)));
    let session = e.snapshot().session.unwrap();
    assert!(session.ended.unwrap() >= session.started);
}
#[test]
fn replay_traffic_expiry_checks_the_exact_thirty_minute_boundary() {
    for gap in [1_799_999_999, 1_800_000_000, 1_860_000_000] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 3_000, damage(1, 1, 100));
        let old = e.snapshot().session.unwrap().id;
        let events = e.observe_traffic(Timestamp(3_000 + gap));
        assert_eq!(
            e.snapshot().session.unwrap().id != old,
            gap >= 1_800_000_000
        );
        assert_eq!(
            events.iter().any(|e| matches!(e, EngineEvent::FightEnded(f)
            if f.end_reason == Some(FightEnd::SessionIdle))),
            gap >= 1_800_000_000
        );
    }
}

#[test]
fn idle_is_traffic_based_and_exact() {
    let mut e = engine();
    setup(&mut e);
    e.observe_traffic(Timestamp(10_000));
    e.advance_to(Timestamp(1_800_009_999));
    assert!(e.snapshot().session.unwrap().ended.is_none());
    let events = e.advance_to(Timestamp(1_800_010_000));
    assert!(events
        .iter()
        .any(|e| matches!(e,EngineEvent::SessionUpdated(s) if s.ended==Some(Timestamp(10_000)))));
    let id = e.snapshot().session.unwrap().id;
    e.observe_traffic(Timestamp(1_800_011_000));
    assert_ne!(e.snapshot().session.unwrap().id, id);
}

#[test]
fn idle_finalizes_incomplete_run_and_fights_then_restarts_cleanly() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    let old = e.snapshot().session.unwrap().id;
    let events = e.advance_to(Timestamp(1_800_003_000));
    assert!(events.iter().any(|v|matches!(v,EngineEvent::RunEnded(r) if r.outcome==Some(Outcome::Unknown) && !r.complete_capture)));
    assert!(events.iter().any(|v|matches!(v,EngineEvent::FightEnded(f) if f.end_reason==Some(FightEnd::SessionIdle) && f.total_damage==100.)));
    assert!(e.snapshot().fights.is_empty());
    assert!(e.snapshot().run.is_none());
    apply(&mut e, 1_800_004_000, map("The Void"));
    apply(&mut e, 1_800_005_000, create(1, 1));
    apply(
        &mut e,
        1_800_006_000,
        update(vec![player(), target()], vec![]),
    );
    apply(&mut e, 1_800_007_000, damage(1, 1, 25));
    let s = e.snapshot();
    assert_ne!(s.session.unwrap().id, old);
    assert_eq!(s.fights[0].total_damage, 25.);
}

#[test]
fn explicit_stop_finalizes_once_and_preserves_last_traffic_time() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, damage(1, 1, 100));
    let events = e.finish(Timestamp(10_000), FightEnd::AppClosed);
    assert_eq!(events.iter().filter(|v|matches!(v,EngineEvent::FightEnded(f) if f.end_reason==Some(FightEnd::AppClosed))).count(),1);
    assert!(events
        .iter()
        .any(|v| matches!(v,EngineEvent::RunEnded(r) if r.outcome==Some(Outcome::Unknown))));
    assert_eq!(e.snapshot().session.unwrap().ended, Some(Timestamp(3000)));
    assert!(!e
        .finish(Timestamp(11_000), FightEnd::AppClosed)
        .iter()
        .any(|v| matches!(v, EngineEvent::FightEnded(_) | EngineEvent::RunEnded(_))));
}
