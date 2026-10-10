mod common;
use common::*;
use maxed_types::*;
#[test]
fn map_clears_and_prior_snapshots_are_detached() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 1_000_000, damage(1, 1, 100));
    let old = e.snapshot();
    assert_eq!(old.fights.len(), 1);
    let events = apply(&mut e, 2_000_000, map("Nexus"));
    assert!(events
        .iter()
        .any(|v| matches!(v, EngineEvent::FightEnded(_))));
    assert!(e.snapshot().fights.is_empty());
    assert_eq!(old.fights[0].total_damage, 100.);
}
#[test]
fn unknown_stats_and_recovery_remain_partial() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, tick(vec![status(1, vec![stat(255, 9)])]));
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(4000),
        maxed_protocol::Diagnostic::Packet {
            id: 42,
            problem: maxed_protocol::PacketDiagnostic::BadFields,
            alignment_verified: true,
        },
    );
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(5000),
        maxed_protocol::Diagnostic::Recovered,
    );
    assert!(!e.snapshot().run.unwrap().complete_capture);
    assert!(e.snapshot().issues.contains(&CoverageIssue::DecodeFailure));
    e.advance_to(Timestamp(1));
    assert!(e
        .snapshot()
        .issues
        .contains(&CoverageIssue::ClockRegression));
}
#[test]
fn retired_connection_cannot_change_new_map() {
    let mut e = engine();
    setup(&mut e);
    connection(&mut e, 2, 4000, map("Nexus"));
    connection(&mut e, 1, 5000, map("The Void"));
    assert_eq!(e.snapshot().map.as_deref(), Some("Nexus"));
}
#[test]
fn terminal_stop_mid_fight_is_partial() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 1_000_000, tick(vec![]));
    apply(&mut e, 1_001_000, damage(1, 1, 100));
    let events = e.finish(Timestamp(2_000_000), FightEnd::AppClosed);
    assert!(events
        .iter()
        .any(|v| matches!(v,EngineEvent::FightEnded(f) if !f.complete_capture)));
}
