use crate::common;
use common::*;
use maxed_engine::Engine;
use maxed_protocol::{key_bytes, DecodeEvent, Decoder, Diagnostic, Packet, PacketDiagnostic, Rc4};
use maxed_types::*;

fn ready() -> Engine {
    let mut e = engine();
    setup(&mut e);
    apply(&mut e, 3000, tick(vec![]));
    e
}
fn failure(e: &mut Engine, at: u64, id: u8) {
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(at),
        Diagnostic::BadFields,
    );
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(at),
        Diagnostic::Packet {
            id,
            problem: PacketDiagnostic::BadFields,
            alignment_verified: true,
        },
    );
}
fn second_fight(e: &mut Engine, at: u64) {
    apply(
        e,
        at,
        update(vec![object(3, 300, vec![stat(0, 1000)])], vec![]),
    );
    let Packet::Damage(mut p) = damage(1, 2, 100) else {
        unreachable!()
    };
    p.targetId = 3;
    apply(e, at + 1, Packet::Damage(p));
}
fn bag(e: &mut Engine, at: u64, id: i32) {
    apply(
        e,
        at,
        update(
            vec![object(id, 1292, (8..16).map(|s| stat(s, -1)).collect())],
            vec![],
        ),
    );
}
#[test]
fn relevant_decode_failure_marks_only_open_fights_and_current_run() {
    let mut e = ready();
    apply(&mut e, 4000, damage(1, 1, 100));
    failure(&mut e, 5000, 75);
    second_fight(&mut e, 6000);
    let s = e.snapshot();
    assert!(s.fights[0].issues.contains(&CoverageIssue::DecodeFailure));
    assert!(s.fights[1].complete_capture);
    assert!(s.fights[1].issues.is_empty());
    let r = s.run.unwrap();
    assert!(!r.complete_capture);
    assert!(r
        .evidence
        .iter()
        .any(|v| v.evidence == Evidence::CaptureGap));
    apply(&mut e, 7000, map("The Shatters"));
    assert!(e.snapshot().run.unwrap().complete_capture);
    assert!(e.snapshot().issues.is_empty());
}
#[test]
fn known_unmodelled_and_unknown_packets_do_not_mark_records() {
    let mut e = ready();
    apply(&mut e, 4000, damage(1, 1, 100));
    for id in [11, 250] {
        apply(&mut e, 5000, Packet::Unknown { id, len: 5 });
    }
    let s = e.snapshot();
    assert!(s.issues.is_empty());
    assert!(s.fights[0].complete_capture);
    assert!(s.run.unwrap().complete_capture);
}
#[test]
fn unrelated_fields_and_unverified_failures_only_count() {
    let mut e = ready();
    apply(&mut e, 4000, damage(1, 1, 100));
    failure(&mut e, 5000, 74); // HELLO is not gameplay evidence.
    failure(&mut e, 5000, 114); // ExaltationUpdate is not consumed by a reducer.
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(5000),
        Diagnostic::Packet {
            id: 75,
            problem: PacketDiagnostic::BadFields,
            alignment_verified: false,
        },
    );
    let s = e.snapshot();
    assert!(s.fights[0].complete_capture);
    assert!(s.run.unwrap().complete_capture);
    assert_eq!(s.capture.decode_failures, 2);
    assert_eq!(s.capture.packet_diagnostics.len(), 3);
}
#[test]
fn stream_gap_marks_open_fight_and_run_without_tainting_later_fight() {
    let mut e = ready();
    apply(&mut e, 4000, damage(1, 1, 100));
    e.diagnostic(
        ConnId(1),
        Direction::ServerToClient,
        Timestamp(5000),
        Diagnostic::Recovering,
    );
    second_fight(&mut e, 6000);
    let s = e.snapshot();
    assert!(s.fights[0].issues.contains(&CoverageIssue::StreamGap));
    assert!(s.fights[1].complete_capture);
    let r = s.run.unwrap();
    assert!(!r.complete_capture);
    assert!(r
        .evidence
        .iter()
        .any(|v| v.evidence == Evidence::CaptureGap));
}
#[test]
fn assumed_stats_are_local_to_damage_fight_including_first_event() {
    let mut e = ready();
    // Unknown active bonus IDs exercise uncertain reconstruction defaults.
    let mut p = player();
    p.status
        .stats
        .retain(|s| !matches!(s.statTypeNum, 128 | 155));
    p.status.stats.push(string(128, "unknown-active"));
    apply(
        &mut e,
        4000,
        update(vec![p, object(2, 300, vec![stat(0, 1000)])], vec![1, 2]),
    );
    apply(&mut e, 5000, shoot(1));
    let events = apply(&mut e, 6000, hit(1));
    assert!(events.iter().any(|v| matches!(v, EngineEvent::FightStarted(f) if f.issues.contains(&CoverageIssue::MissingStats))));
    second_fight(&mut e, 7000);
    let s = e.snapshot();
    assert!(s.fights[0].issues.contains(&CoverageIssue::MissingStats));
    assert!(s.fights[1].complete_capture);
    assert!(!s.issues.contains(&CoverageIssue::MissingStats));
    assert!(s.run.unwrap().complete_capture);
}
#[test]
fn drop_window_is_two_seconds_in_both_directions() {
    let mut e = ready();
    bag(&mut e, 1_000_000, 10);
    bag(&mut e, 3_000_000, 11);
    failure(&mut e, 5_000_000, 75);
    bag(&mut e, 7_000_000, 12);
    bag(&mut e, 7_000_001, 13);
    let drops = e.snapshot().drops;
    assert!(drops[0].issues.is_empty());
    assert!(drops[1].issues.contains(&CoverageIssue::DecodeFailure));
    assert!(drops[2].issues.contains(&CoverageIssue::DecodeFailure));
    assert!(drops[3].issues.is_empty());
}
#[test]
fn packet_diagnostic_counters_are_bounded_and_existing_ids_keep_counting() {
    let mut e = ready();
    for id in 0..=u8::MAX {
        for problem in [PacketDiagnostic::UnknownId, PacketDiagnostic::BadFields] {
            e.diagnostic(
                ConnId(1),
                Direction::ServerToClient,
                Timestamp(5000),
                Diagnostic::Packet {
                    id,
                    problem,
                    alignment_verified: false,
                },
            );
        }
    }
    failure(&mut e, 6000, 42);
    let counts = e.snapshot().capture.packet_diagnostics;
    assert_eq!(counts.len(), 64);
    assert_eq!(counts[42].packet_id, 42);
    assert_eq!(counts[42].unknown_packets, 1);
    assert_eq!(counts[42].field_failures, 2);
    assert!(counts.windows(2).all(|w| w[0].packet_id < w[1].packet_id));
}
#[test]
fn decoder_context_reaches_engine_once_without_payload_diagnostics() {
    let mut e = ready();
    apply(&mut e, 4000, damage(1, 1, 100));
    let dir = Direction::ServerToClient;
    let mut decoder = Decoder::default();
    decoder.push(StreamEvent::Opened {
        conn: ConnId(1),
        dir,
        at: Timestamp(4000),
        fresh: true,
        syn_proven: true,
    });
    let mut cipher = Rc4::new(&key_bytes(dir).unwrap()).unwrap();
    for (id, mut body) in [(11, vec![]), (250, vec![]), (75, vec![0])] {
        cipher.apply(&mut body);
        let mut bytes = ((body.len() + 5) as u32).to_be_bytes().to_vec();
        bytes.push(id);
        bytes.extend(body);
        for event in decoder.push(StreamEvent::Data(StreamChunk {
            conn: ConnId(1),
            dir,
            at: Timestamp(5000),
            bytes,
            origins: vec![],
        })) {
            match event {
                DecodeEvent::Packet(p) => {
                    e.apply(p);
                }
                DecodeEvent::Diagnostic {
                    conn,
                    dir,
                    at,
                    kind,
                } => {
                    e.diagnostic(conn, dir, at, kind);
                }
            }
        }
    }
    let s = e.snapshot();
    assert_eq!(s.capture.unknown_packets, 1);
    assert_eq!(s.capture.decode_failures, 1);
    assert_eq!(
        s.capture.packet_diagnostics,
        vec![
            PacketDiagnosticCount {
                packet_id: 75,
                unknown_packets: 0,
                field_failures: 1
            },
            PacketDiagnosticCount {
                packet_id: 250,
                unknown_packets: 1,
                field_failures: 0
            },
        ]
    );
    assert!(s.fights[0].issues.contains(&CoverageIssue::DecodeFailure));
}

#[test]
fn native_capture_status_updates_preserve_engine_packet_counters() {
    let mut e = ready();
    failure(&mut e, 5000, 75);
    let before = e.snapshot().capture.packet_diagnostics;
    let events = e.capture_changed(CaptureStatus {
        state: CaptureState::Live,
        dropped_frames: 2,
        ..Default::default()
    });
    assert_eq!(e.snapshot().capture.packet_diagnostics, before);
    assert!(
        matches!(&events[0], EngineEvent::CaptureStateChanged(s) if s.packet_diagnostics == before && s.dropped_frames == 2)
    );
}

#[test]
fn missing_reconstruction_stats_and_catalog_stay_on_the_target_record() {
    let mut e = engine();
    apply(&mut e, 0, map("The Void"));
    apply(&mut e, 1000, create(1, 1));
    let mut p = player();
    p.status.stats.retain(|s| s.statTypeNum != 20);
    apply(&mut e, 4000, update(vec![p, target()], vec![]));
    apply(&mut e, 4000, tick(vec![]));
    second_fight(&mut e, 5000);
    apply(&mut e, 6000, shoot(1));
    apply(&mut e, 7000, hit(1));
    assert!(e.snapshot().fights[0]
        .issues
        .contains(&CoverageIssue::MissingStats));
    assert!(e.snapshot().fights[1].complete_capture);
    let Packet::Damage(mut p) = damage(1, 2, 100) else {
        unreachable!()
    };
    p.targetId = 99;
    apply(&mut e, 8000, Packet::Damage(p));
    let s = e.snapshot();
    assert!(s.fights[2].issues.contains(&CoverageIssue::MissingCatalog));
    assert!(s.fights[1].complete_capture);
    assert!(s.run.unwrap().complete_capture);
    assert!(s.issues.is_empty());
    assert!(s
        .character
        .unwrap()
        .issues
        .contains(&CoverageIssue::MissingStats));
}

#[test]
fn verified_dungeon_entry_clears_startup_midstream_coverage() {
    let mut e = engine();
    e.apply(maxed_protocol::DecodedPacket {
        conn: ConnId(1),
        dir: Direction::ServerToClient,
        at: Timestamp(1000),
        id: 92,
        len: 0,
        alignment_verified: false,
        packet: map("The Void"),
    });
    assert!(e.snapshot().issues.contains(&CoverageIssue::Midstream));
    apply(&mut e, 2000, map("The Void"));
    let s = e.snapshot();
    assert!(s.issues.is_empty());
    let run = s.run.unwrap();
    assert!(run.complete_capture);
    assert!(run.issues.is_empty());
    assert!(run.evidence.is_empty());
}

#[test]
fn trailing_bytes_count_as_field_failures_without_double_counting_totals() {
    let mut e = ready();
    apply(&mut e, 4000, damage(1, 1, 100));
    for id in [75, 114] {
        for kind in [
            Diagnostic::TrailingBytes,
            Diagnostic::Packet {
                id,
                problem: PacketDiagnostic::TrailingBytes,
                alignment_verified: true,
            },
        ] {
            e.diagnostic(ConnId(1), Direction::ServerToClient, Timestamp(5000), kind);
        }
    }
    let s = e.snapshot();
    assert_eq!(s.capture.decode_failures, 2);
    assert_eq!(
        s.capture.packet_diagnostics,
        vec![
            PacketDiagnosticCount {
                packet_id: 75,
                unknown_packets: 0,
                field_failures: 1
            },
            PacketDiagnosticCount {
                packet_id: 114,
                unknown_packets: 0,
                field_failures: 1
            },
        ]
    );
    assert!(s.fights[0].issues.contains(&CoverageIssue::DecodeFailure));
}
