use maxed_protocol::{key_bytes, Alignment, DecodeEvent, Decoder, Diagnostic, Rc4};
use maxed_types::{CloseReason, ConnId, Direction, GapReason, StreamChunk, StreamEvent, Timestamp};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
const S: Direction = Direction::ServerToClient;
const C: Direction = Direction::ClientToServer;

#[test]
fn completed_packet_precedes_bad_header_and_recovery() {
    let mut d = Decoder::default();
    open(&mut d, 1, C, true);
    let mut c = cipher(C);
    let mut chunk = frame(&mut c, 90, vec![255; 6]);
    chunk.extend(4u32.to_be_bytes());
    let events = data(&mut d, 1, C, chunk);
    assert_eq!(events.len(), 3);
    assert!(
        matches!(&events[0], DecodeEvent::Packet(p) if p.packet.fields().unwrap()["objectId"] == -1)
    );
    assert!(matches!(
        &events[1],
        DecodeEvent::Diagnostic {
            kind: Diagnostic::BadFrame,
            ..
        }
    ));
    assert!(matches!(
        &events[2],
        DecodeEvent::Diagnostic {
            kind: Diagnostic::Recovering,
            ..
        }
    ));
    // The next short chunk is discarded by the rearmed startup gate.
    assert!(data(&mut d, 1, C, frame(&mut c, 105, vec![])).is_empty());
}

#[test]
fn nonfinite_typed_packets_reach_live_consumers_and_keep_cipher_alignment() {
    let mut d = Decoder::default();
    open(&mut d, 1, C, true);
    let mut c = cipher(C);
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut body = 1i32.to_be_bytes().to_vec();
        body.extend(value.to_be_bytes());
        body.extend(0f32.to_be_bytes());
        let events = data(&mut d, 1, C, frame(&mut c, 103, body));
        assert_eq!(packets(&events), 1);
        assert!(!diag(&events, Diagnostic::BadFields));
        let DecodeEvent::Packet(packet) = &events[0] else {
            panic!("typed packet required")
        };
        assert!(matches!(
            maxed_protocol::canonical_line(0, packet),
            Err(maxed_protocol::ProtocolError::NonFinite)
        ));
    }
    assert_eq!(
        packets(&data(&mut d, 1, C, frame(&mut c, 90, vec![255; 6]))),
        1
    );
}
fn cipher(dir: Direction) -> Rc4 {
    Rc4::new(&key_bytes(dir).unwrap()).unwrap()
}
fn open(d: &mut Decoder, n: u64, dir: Direction, fresh: bool) {
    d.push(StreamEvent::Opened {
        conn: ConnId(n),
        dir,
        at: Timestamp(1),
        fresh,
        syn_proven: fresh,
    });
}
fn data(d: &mut Decoder, n: u64, dir: Direction, bytes: Vec<u8>) -> Vec<DecodeEvent> {
    d.push(StreamEvent::Data(StreamChunk {
        conn: ConnId(n),
        dir,
        at: Timestamp(7),
        bytes,
        origins: vec![],
    }))
}
fn frame(c: &mut Rc4, id: u8, mut body: Vec<u8>) -> Vec<u8> {
    c.apply(&mut body);
    let mut f = ((body.len() + 5) as u32).to_be_bytes().to_vec();
    f.push(id);
    f.extend(body);
    f
}
fn tick(n: i32) -> Vec<u8> {
    let mut b = vec![0; 16];
    b[..4].copy_from_slice(&n.to_be_bytes());
    b
}
fn packets(e: &[DecodeEvent]) -> usize {
    e.iter()
        .filter(|e| matches!(e, DecodeEvent::Packet(_)))
        .count()
}
fn diag(e: &[DecodeEvent], kind: Diagnostic) -> bool {
    e.iter()
        .any(|e| matches!(e,DecodeEvent::Diagnostic{kind:k,..} if *k==kind))
}
#[test]
fn fresh_packets_directions_unknown_and_bad_fields_advance_once() {
    let mut d = Decoder::default();
    open(&mut d, 1, C, true);
    open(&mut d, 1, S, true);
    let mut c = cipher(C);
    let mut s = cipher(S);
    let hello = vec![0; 24]; // i32 + two strings + i32 + blob + five strings
    assert_eq!(packets(&data(&mut d, 1, C, frame(&mut c, 74, hello))), 1);
    assert_eq!(packets(&data(&mut d, 1, S, frame(&mut s, 10, tick(0)))), 1);
    let e = data(&mut d, 1, C, frame(&mut c, 250, vec![1, 2, 3]));
    assert_eq!(packets(&e), 1);
    assert!(diag(&e, Diagnostic::UnknownId));
    let e = data(&mut d, 1, C, frame(&mut c, 90, vec![1]));
    assert_eq!(packets(&e), 0);
    assert!(diag(&e, Diagnostic::BadFields));
    let e = data(&mut d, 1, C, frame(&mut c, 90, vec![255; 6]));
    assert_eq!(packets(&e), 1);
    if let DecodeEvent::Packet(p) = &e[0] {
        assert_eq!(p.packet.fields().unwrap()["objectId"], -1);
        assert_eq!(p.at, Timestamp(7));
    } else {
        panic!("packet required")
    }
    assert_eq!(packets(&data(&mut d, 1, S, frame(&mut s, 10, tick(1)))), 1);
}
#[test]
fn tick_pair_recovers_suppresses_b_and_decrypts_next() {
    let mut a = Alignment::new(S, false).unwrap();
    let mut c = cipher(S);
    c.skip(1234);
    let mut first = tick(50);
    c.apply(&mut first);
    assert!(a.consume(10, &first).unwrap().is_none());
    let mut middle = vec![1, 2, 3, 4, 5];
    c.apply(&mut middle);
    assert!(a.consume(250, &middle).unwrap().is_none());
    let mut second = tick(51);
    c.apply(&mut second);
    assert!(a.consume(10, &second).unwrap().is_none());
    let mut third = tick(52);
    c.apply(&mut third);
    assert_eq!(a.consume(10, &third).unwrap().unwrap(), tick(52));
}
#[test]
fn mismatch_realigns_using_current_tick_as_a() {
    let mut a = Alignment::new(S, true).unwrap();
    let mut c = cipher(S);
    let mut first = tick(40);
    c.apply(&mut first);
    assert!(a.consume(10, &first).unwrap().is_none());
    let mut second = tick(41);
    c.apply(&mut second);
    assert!(a.consume(10, &second).unwrap().is_none());
    let mut third = tick(42);
    c.apply(&mut third);
    assert_eq!(a.consume(10, &third).unwrap().unwrap(), tick(42));
}
#[test]
fn gate_gap_bad_lengths_and_closed_generations() {
    let mut d = Decoder::default();
    open(&mut d, 1, S, false);
    assert!(data(&mut d, 1, S, vec![0; 1460]).is_empty());
    assert!(data(&mut d, 1, S, vec![0; 7]).is_empty());
    assert!(diag(&data(&mut d, 1, S, vec![0; 4]), Diagnostic::BadFrame));
    assert!(data(&mut d, 1, S, vec![0; 7]).is_empty()); // rearmed gate
    let mut s = cipher(S);
    s.skip(20);
    assert_eq!(packets(&data(&mut d, 1, S, frame(&mut s, 10, tick(8)))), 0);
    let e = data(&mut d, 1, S, frame(&mut s, 10, tick(9)));
    assert_eq!(packets(&e), 0);
    assert!(diag(&e, Diagnostic::Recovered));
    assert_eq!(packets(&data(&mut d, 1, S, frame(&mut s, 10, tick(10)))), 1);
    let e = d.push(StreamEvent::Gap {
        conn: ConnId(1),
        dir: S,
        at: Timestamp(8),
        reason: GapReason::CaptureDrop,
    });
    assert!(diag(&e, Diagnostic::Recovering));
    assert_eq!(packets(&data(&mut d, 1, S, frame(&mut s, 10, tick(11)))), 0);
    d.push(StreamEvent::Closed {
        conn: ConnId(1),
        dir: S,
        at: Timestamp(9),
        reason: CloseReason::Replaced,
    });
    open(&mut d, 2, S, true);
    assert!(data(&mut d, 1, S, vec![0; 10]).is_empty());
    let mut fresh = cipher(S);
    assert_eq!(
        packets(&data(&mut d, 2, S, frame(&mut fresh, 10, tick(0)))),
        1
    );
}
#[test]
fn cancellation_is_terminal_and_metadata_only() {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut d = Decoder::with_cancellation(cancel.clone());
    open(&mut d, 1, S, false);
    cancel.store(true, Ordering::Relaxed);
    let e = data(&mut d, 1, S, vec![0; 10]);
    assert!(diag(&e, Diagnostic::Cancelled));
    assert_eq!(packets(&e), 0);
    assert!(data(&mut d, 1, S, vec![0; 10]).is_empty());
}
#[test]
fn impossible_pair_finishes_and_new_pair_can_recover() {
    let mut a = Alignment::new(S, false).unwrap();
    assert!(a.consume(10, &[0; 4]).unwrap().is_none());
    // delta 4, deterministic synthetic pair has no match within the fixed search.
    assert!(a.consume(10, &[0; 4]).unwrap().is_none());
    let mut c = cipher(S);
    let mut x = tick(0);
    c.apply(&mut x);
    assert!(a.consume(10, &x).unwrap().is_none());
    let mut y = tick(1);
    c.apply(&mut y);
    assert!(a.consume(10, &y).unwrap().is_none());
    let mut z = tick(2);
    c.apply(&mut z);
    assert_eq!(a.consume(10, &z).unwrap().unwrap(), tick(2));
}

#[test]
fn search_limit_and_delta_budget_are_enforced() {
    use maxed_protocol::alignment::{find_tick_offset, RECOVERY_BUDGET, SEARCH_SIZE};
    let base = cipher(S);
    assert_eq!(find_tick_offset(&base, [0; 4], [0; 4], usize::MAX), None);
    assert_eq!(
        find_tick_offset(&base, [0; 4], [0; 4], RECOVERY_BUDGET + 1),
        None
    );
    for (offset, expected) in [
        (SEARCH_SIZE - 1, Some(SEARCH_SIZE - 1)),
        (SEARCH_SIZE, None),
    ] {
        let mut c = base.clone();
        c.skip(offset);
        let mut a = 100i32.to_be_bytes();
        let mut b = 101i32.to_be_bytes();
        c.apply(&mut a);
        c.apply(&mut b);
        assert_eq!(find_tick_offset(&base, a, b, 4), expected);
    }
}

#[test]
fn fragmented_decoder_preserves_trigger_time_and_fresh_reset_discards_partial_frame() {
    let mut d = Decoder::default();
    open(&mut d, 1, C, true);
    let mut c = cipher(C);
    let f = frame(&mut c, 90, vec![255; 6]);
    assert!(data(&mut d, 1, C, f[..3].to_vec()).is_empty());
    assert_eq!(packets(&data(&mut d, 1, C, f[3..].to_vec())), 1);
    data(&mut d, 1, C, f[..3].to_vec());
    open(&mut d, 1, C, true);
    let mut c = cipher(C);
    assert_eq!(
        packets(&data(&mut d, 1, C, frame(&mut c, 90, vec![255; 6]))),
        1
    );
}

#[test]
fn malformed_tick_fields_and_trailing_bytes_do_not_rewind_cipher() {
    let mut d = Decoder::default();
    open(&mut d, 1, S, true);
    let mut c = cipher(S);
    let e = data(&mut d, 1, S, frame(&mut c, 10, vec![0; 3]));
    assert!(diag(&e, Diagnostic::BadFields));
    assert_eq!(packets(&data(&mut d, 1, S, frame(&mut c, 10, tick(0)))), 1);
    let mut body = tick(1);
    body.push(255);
    let e = data(&mut d, 1, S, frame(&mut c, 10, body));
    assert_eq!(packets(&e), 1);
    assert!(diag(&e, Diagnostic::TrailingBytes));
    assert_eq!(packets(&data(&mut d, 1, S, frame(&mut c, 10, tick(2)))), 1);
}

#[test]
fn direction_registry_distinguishes_untyped_known_ids_from_unknown_ids() {
    use maxed_protocol::{defs::known_name, direction::packet_direction};
    for id in 0..=255u8 {
        assert_eq!(
            packet_direction(id).is_none(),
            known_name(id) == "UNKNOWN",
            "id={id}"
        );
    }
    assert_eq!(packet_direction(120), Some(S));
    assert_eq!(packet_direction(62), Some(C));
}
