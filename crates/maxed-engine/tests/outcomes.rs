use crate::common;
use common::*;
use maxed_types::*;

fn close_connection(e: &mut maxed_engine::Engine, conn: u64, at: u64) -> Vec<EngineEvent> {
    e.diagnostic(
        ConnId(conn),
        Direction::ServerToClient,
        Timestamp(at),
        maxed_protocol::Diagnostic::Closed,
    )
}

#[test]
fn unexpected_close_followed_by_mapinfo_is_an_interrupted_unknown_run() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3_000, damage(1, 1, 100));
    // Preserve the close timestamp even if a previous gap already marked coverage.
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(4_000),
        maxed_protocol::Diagnostic::Recovering,
    );
    close_connection(&mut e, 1, 5_000);
    let snapshot = e.snapshot();
    assert!(!snapshot.fights[0].complete_capture);
    assert!(snapshot.fights[0]
        .issues
        .contains(&CoverageIssue::StreamGap));
    let run = snapshot.run.unwrap();
    assert!(!run.complete_capture);
    assert!(run.evidence.contains(&EvidenceAt {
        at: Timestamp(5_000),
        evidence: Evidence::CaptureGap
    }));
    let events = connection(&mut e, 2, 6_000, map("Nexus"));
    let ended = events
        .iter()
        .find_map(|event| match event {
            EngineEvent::RunEnded(r) => Some(r),
            _ => None,
        })
        .unwrap();
    assert_eq!(ended.outcome, Some(Outcome::Unknown));
    assert!(!ended.complete_capture);
    assert!(!ended
        .evidence
        .iter()
        .any(|e| e.evidence == Evidence::ObservedExit));
    assert!(events.iter().any(|event| matches!(event, EngineEvent::FightEnded(f) if !f.complete_capture && f.total_damage == 100.)));
}

#[test]
fn verified_reconnect_then_close_preserves_deliberate_map_exit() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3_000, damage(1, 1, 100));
    let (reconnect, _) =
        maxed_protocol::decode_packet(45, Direction::ServerToClient, &[0; 16]).unwrap();
    apply(&mut e, 4_000, reconnect);
    let fight = e.snapshot().fights[0].clone();
    close_connection(&mut e, 1, 5_000);
    assert!(e.snapshot().run.unwrap().complete_capture);
    assert_eq!(e.snapshot().fights[0], fight);
    let events = connection(&mut e, 2, 6_000, map("Nexus"));
    assert!(events
        .iter()
        .any(|event| matches!(event, EngineEvent::RunEnded(r)
        if r.complete_capture && r.outcome == Some(Outcome::Left)
            && r.evidence.iter().any(|e| e.evidence == Evidence::ObservedExit)
            && !r.evidence.iter().any(|e| e.evidence == Evidence::CaptureGap))));
    // The handoff exemption belongs only to the old connection.
    connection(&mut e, 2, 7_000, map("The Void"));
    close_connection(&mut e, 1, 8_000);
    assert!(e.snapshot().run.unwrap().complete_capture);
    close_connection(&mut e, 2, 9_000);
    assert!(!e.snapshot().run.unwrap().complete_capture);
}

#[test]
fn close_after_completion_or_death_preserves_coverage_and_outcomes() {
    for (packet, outcome) in [(victory(), Outcome::Cleared), (death(), Outcome::Died)] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 3_000, damage(1, 1, 100));
        apply(&mut e, 4_000, packet);
        let before = e.snapshot();
        close_connection(&mut e, 1, 5_000);
        let after = e.snapshot();
        assert_eq!(after.run, before.run);
        assert_eq!(after.fights, before.fights);
        assert_eq!(after.issues, before.issues);
        let run = e.snapshot().run.unwrap();
        assert_eq!(run.outcome, Some(outcome));
        assert!(run.complete_capture);
        assert!(!run
            .evidence
            .iter()
            .any(|e| e.evidence == Evidence::CaptureGap));
    }
}

#[test]
fn close_without_an_open_run_does_not_mark_realm_fights_incomplete() {
    let mut e = engine();
    apply(&mut e, 0, map("Realm"));
    apply(&mut e, 1_000, create(1, 1));
    apply(&mut e, 2_000, update(vec![player(), target()], vec![]));
    apply(&mut e, 3_000, tick(vec![]));
    apply(&mut e, 4_000, damage(1, 1, 100));
    let before = e.snapshot();
    assert!(before.run.is_none());
    assert!(before.fights[0].complete_capture);
    close_connection(&mut e, 1, 5_000);
    assert_eq!(e.snapshot().fights, before.fights);
    assert_eq!(e.snapshot().issues, before.issues);
}

#[test]
fn partial_escape_retains_unknown_outcome_and_exit_time_before_next_map() {
    use maxed_protocol::{defs::Escape, Diagnostic, Packet};
    let mut e = engine();
    setup(&mut e);
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(3000),
        Diagnostic::Recovering,
    );
    apply(&mut e, 4000, Packet::Escape(Escape {}));
    let run = e.snapshot().run.unwrap();
    assert_eq!(run.outcome, Some(Outcome::Unknown));
    assert_eq!(run.ended, Some(Timestamp(4000)));
    assert!(!run.complete_capture);
    assert!(run
        .evidence
        .iter()
        .any(|e| e.evidence == Evidence::CaptureGap));
    assert!(run
        .evidence
        .iter()
        .any(|e| e.at == Timestamp(4000) && e.evidence == Evidence::ObservedExit));
    assert!(!apply(&mut e, 5000, map("Nexus"))
        .iter()
        .any(|event| { matches!(event, EngineEvent::RunEnded(later) if later.id == run.id) }));
}

#[test]
fn completion_and_death_keep_order_and_both_facts() {
    for clear_first in [true, false] {
        let mut e = engine();
        setup(&mut e);
        apply(&mut e, 3000, if clear_first { victory() } else { death() });
        apply(&mut e, 3000, if clear_first { death() } else { victory() });
        let r = e.snapshot().run.unwrap();
        assert_eq!(
            r.outcome,
            Some(if clear_first {
                Outcome::Cleared
            } else {
                Outcome::Died
            })
        );
        assert_eq!(r.died_at, Some(Timestamp(3000)));
        assert!(apply(&mut e, 4000, death())
            .iter()
            .all(|v| !matches!(v, EngineEvent::PlayerDied { .. })));
    }
}
#[test]
fn exit_and_shutdown_differ() {
    let mut e = engine();
    setup(&mut e);
    let events = apply(&mut e, 3000, map("Nexus"));
    assert!(events
        .iter()
        .any(|v| matches!(v,EngineEvent::RunEnded(r) if r.outcome==Some(Outcome::Left))));
    let mut e = engine();
    setup(&mut e);
    let events = e.finish(Timestamp(3000), FightEnd::AppClosed);
    assert!(events
        .iter()
        .any(|v| matches!(v,EngineEvent::RunEnded(r) if r.outcome==Some(Outcome::Unknown))));
}
#[test]
fn exact_completion_rules_and_forgeries() {
    let lines = [
        (
            "Moonlight Village",
            "#Kitsune Umi",
            "This fully concludes the Moonlight Festival!",
            "moonlight-kitsune",
        ),
        (
            "Moonlight Village",
            "#Dancer Miko",
            "Thank you all for coming tonight.",
            "moonlight-miko",
        ),
        (
            "Moonlight Village",
            "#Umi, Goddess of Revelry",
            "This fully concludes the Moonlight Festival.",
            "moonlight-revelry",
        ),
        (
            "The Void",
            "#Void Entity",
            "You fools... You can never truly defeat me! I am in all of you! I AM all of you!",
            "void-entity",
        ),
        (
            "The Shatters",
            "#The Accursed King",
            "...do you truly think your end will be any different?",
            "shatters-king",
        ),
        (
            "The Shatters",
            "#King Azamoth",
            "This fate is mine to bear... not hers.",
            "shatters-azamoth",
        ),
    ];
    for (map, npc, text, id) in lines {
        assert_eq!(maxed_engine::completion_rule(map, npc, text), Some(id));
        assert_eq!(maxed_engine::completion_rule("Nexus", npc, text), None);
        assert_eq!(maxed_engine::completion_rule(map, &npc[1..], text), None);
        assert_eq!(
            maxed_engine::completion_rule(map, npc, &format!("{text}!")),
            None
        );
    }
}
#[test]
fn duplicate_death_without_identity_is_once() {
    let mut e = engine();
    apply(&mut e, 0, map("Nexus"));
    assert!(apply(&mut e, 1000, death())
        .iter()
        .any(|v| matches!(v, EngineEvent::PlayerDied { .. })));
    assert!(apply(&mut e, 2000, death())
        .iter()
        .all(|v| !matches!(v, EngineEvent::PlayerDied { .. })));
}
#[test]
fn death_first_preserves_later_same_visit_completion_fact() {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, death());
    apply(&mut e, 4000, victory());
    let r = e.snapshot().run.unwrap();
    assert_eq!(r.outcome, Some(Outcome::Died));
    assert_eq!(r.completed_at, Some(Timestamp(4000)));
    assert_eq!(r.ended, Some(Timestamp(3000)));
}
#[test]
fn player_death_notification_is_not_local_death() {
    let mut e = engine();
    setup(&mut e);
    let p = maxed_protocol::Packet::Notification(maxed_protocol::defs::Notification {
        data: maxed_protocol::defs::NotificationData {
            effect: Some("PlayerDeath"),
            message: Some("Synthetic".into()),
            ..Default::default()
        },
    });
    assert!(apply(&mut e, 3000, p)
        .iter()
        .all(|v| !matches!(v, EngineEvent::PlayerDied { .. })));
    assert!(e.snapshot().character.unwrap().died_at.is_none());
}
