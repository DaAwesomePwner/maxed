mod common;
use common::*;
use maxed_engine::Engine;
use maxed_protocol::{DecodedPacket, Packet};
use maxed_types::*;

fn unverified(e: &mut Engine, conn: u64, at: u64, packet: Packet) -> Vec<EngineEvent> {
    e.apply(DecodedPacket {
        conn: ConnId(conn),
        dir: Direction::ServerToClient,
        at: Timestamp(at),
        id: 0,
        len: 0,
        alignment_verified: false,
        packet,
    })
}

#[test]
fn candidate_recovery_needs_a_verified_typed_packet_before_live() {
    let mut e = engine();
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(1_000),
        maxed_protocol::Diagnostic::Recovered,
    );
    assert_eq!(e.snapshot().capture.state, CaptureState::Recovering);
    unverified(&mut e, 1, 2_000, map("The Void"));
    assert_eq!(e.snapshot().capture.state, CaptureState::Recovering);
    assert!(e.snapshot().session.is_none());
    let events = apply(&mut e, 3_000, map("The Void"));
    assert!(events.iter().any(|event| matches!(event, EngineEvent::CaptureStateChanged(s) if s.state == CaptureState::Live)));
    assert_eq!(e.snapshot().capture.state, CaptureState::Live);
    assert!(e.snapshot().run.unwrap().complete_capture);
}

#[test]
fn unverified_map_and_bag_are_ignored_and_verified_map_starts_clean() {
    let mut e = engine();
    let mut events = unverified(&mut e, 1, 1000, map("The Void"));
    events.extend(unverified(
        &mut e,
        1,
        2000,
        update(vec![object(4, 1292, vec![])], vec![]),
    ));
    assert!(events
        .iter()
        .all(|event| matches!(event, EngineEvent::CaptureStateChanged(_))));
    let before = e.snapshot();
    assert!(before.map.is_none());
    assert!(before.run.is_none());
    assert!(before.drops.is_empty());
    assert_eq!(e.ignored_unverified_packets(), 2);
    assert!(before.issues.contains(&CoverageIssue::Midstream));

    // Loss stays visible until the next verified map boundary.
    apply(&mut e, 3000, tick(vec![]));
    assert!(e.snapshot().issues.contains(&CoverageIssue::Midstream));
    let events = apply(&mut e, 4000, map("The Void"));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::RunStarted(r)
        if r.complete_capture && r.issues.is_empty() && r.evidence.is_empty())));
    assert!(e.snapshot().issues.is_empty());
    let events = apply(&mut e, 5000, update(vec![object(4, 1292, vec![])], vec![]));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::BagDropped(b)
        if !b.issues.contains(&CoverageIssue::Midstream))));
    assert_eq!(e.ignored_unverified_packets(), 2);
    // A later fully observed map boundary restores coverage for that visit.
    apply(&mut e, 6000, map("The Shatters"));
    assert!(e.snapshot().run.unwrap().complete_capture);
}

#[test]
fn unverified_premap_packets_do_not_turn_a_fully_observed_exit_into_unknown() {
    for (lost_conn, verified_conn) in [(1, 1), (2, 1), (1, 2)] {
        let mut e = engine();
        unverified(&mut e, lost_conn, 1000, map("The Shatters"));
        unverified(&mut e, lost_conn, 2000, create(99, 99));
        assert!(e.snapshot().map.is_none());
        assert!(e.snapshot().issues.contains(&CoverageIssue::Midstream));
        connection(&mut e, verified_conn, 3000, map("The Void"));
        connection(&mut e, verified_conn, 4000, create(1, 1));
        connection(
            &mut e,
            verified_conn,
            5000,
            update(vec![player(), target()], vec![]),
        );
        connection(&mut e, verified_conn, 6000, tick(vec![]));
        connection(&mut e, verified_conn, 7000, damage(1, 1, 100));
        assert!(e.snapshot().fights[0].complete_capture);
        let events = connection(&mut e, verified_conn, 8000, map("Nexus"));
        assert!(events.iter().any(|event| matches!(event, EngineEvent::RunEnded(r)
            if r.dungeon == "The Void" && r.complete_capture && r.outcome == Some(Outcome::Left)
            && r.issues.is_empty() && !r.evidence.iter().any(|e| e.evidence == Evidence::CaptureGap))));
        assert_eq!(e.ignored_unverified_packets(), 2);
    }
}

#[test]
fn unverified_damage_preserves_fight_numbers_and_flags_coverage() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, tick(vec![]));
    apply(&mut e, 4000, damage(1, 1, 100));
    let before = e.snapshot().fights[0].clone();
    assert!(before.complete_capture);
    unverified(&mut e, 1, 5000, damage(1, 2, 999));
    let after = e.snapshot().fights[0].clone();
    let mut expected = before;
    expected.complete_capture = false;
    expected.issues.push(CoverageIssue::Midstream);
    assert_eq!(after, expected);
    let snapshot = e.snapshot();
    let run = snapshot.run.unwrap();
    assert!(!run.complete_capture);
    assert!(run.issues.contains(&CoverageIssue::Midstream));
    assert!(run
        .evidence
        .iter()
        .any(|e| e.evidence == Evidence::CaptureGap));
    assert!(e.advance_to(Timestamp(250_000)).iter().any(|event|
        matches!(event, EngineEvent::FightUpdated(f) if f.issues.contains(&CoverageIssue::Midstream))));
    apply(&mut e, 251_000, damage(1, 2, 25));
    assert_eq!(e.snapshot().fights[0].total_damage, 125.);
}

#[test]
fn unverified_packets_cannot_change_identity_fame_outcomes_or_timers() {
    let mut e = engine();
    setup(&mut e);
    let before = e.snapshot();
    let packets = [
        create(99, 99),
        tick(vec![status(
            1,
            vec![string(38, "other-account"), stat(57, 999)],
        )]),
        update(vec![object(4, 1292, vec![])], vec![1, 2]),
        victory(),
        death(),
        map("Nexus"),
        shoot(1),
        damage(1, 1, 999),
    ];
    for packet in packets {
        // A new connection and an idle-length timestamp must not close this map.
        let events = unverified(&mut e, 2, 2_000_000_000, packet);
        assert!(events.iter().all(|event| matches!(
            event,
            EngineEvent::RunUpdated(_) | EngineEvent::CaptureStateChanged(_)
        )));
    }
    let after = e.snapshot();
    assert_eq!(after.at, before.at);
    assert_eq!(after.character, before.character);
    assert_eq!(after.session, before.session);
    assert_eq!(after.map, before.map);
    assert!(after.fights.is_empty());
    assert!(after.drops.is_empty());
    let run = after.run.unwrap();
    assert!(run.outcome.is_none());
    assert!(run.ended.is_none());
    assert!(run.died_at.is_none());
    assert!(run.completed_at.is_none());
    assert_eq!(e.ignored_unverified_packets(), 8);
    // Rejected connection metadata must not retire the still-verified connection.
    apply(&mut e, 3000, tick(vec![]));
    apply(&mut e, 4000, damage(1, 1, 10));
    assert_eq!(e.snapshot().fights[0].total_damage, 10.);
}
