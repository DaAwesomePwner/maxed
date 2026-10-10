use maxed_stream::serial_distance;
use maxed_stream::Reassembler;
use maxed_types::*;

fn segment(seq: u32, syn: bool, bytes: &[u8]) -> TcpSegment {
    TcpSegment {
        flow: FlowKey {
            client: Endpoint {
                ip: [127, 0, 0, 1],
                port: 50000,
            },
            server: Endpoint {
                ip: [127, 0, 0, 2],
                port: 2050,
            },
        },
        dir: Direction::ClientToServer,
        at: Timestamp(1),
        seq,
        syn,
        fin: false,
        rst: false,
        bytes: bytes.to_vec(),
        origin: ByteOrigin {
            frame_index: seq as u64,
            offset: 54,
            len: bytes.len(),
        },
    }
}
fn data(events: Vec<StreamEvent>) -> Vec<u8> {
    events
        .into_iter()
        .filter_map(|e| match e {
            StreamEvent::Data(c) => Some(c.bytes),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn syn_then_payload() {
    let mut stream = Reassembler::default();
    assert!(matches!(
        stream.push(segment(9, true, b"")).as_slice(),
        [StreamEvent::Opened {
            fresh: true,
            syn_proven: true,
            ..
        }]
    ));
    assert_eq!(data(stream.push(segment(10, false, b"abc"))), b"abc");
}

#[test]
fn serial_wrap_orders_nearby_bytes() {
    assert_eq!(serial_distance(1, u32::MAX), 2);
    assert_eq!(serial_distance(u32::MAX, 1), -2);
}

#[test]
fn duplicate_syn_and_ack_do_not_reset_or_open() {
    let mut stream = Reassembler::default();
    assert!(stream.push(segment(0, false, b"")).is_empty());
    stream.push(segment(9, true, b""));
    assert_eq!(data(stream.push(segment(10, false, b"abc"))), b"abc");
    assert!(stream.push(segment(9, true, b"")).is_empty());
    assert!(stream.push(segment(10, false, b"abc")).is_empty());
    assert_eq!(data(stream.push(segment(13, false, b"de"))), b"de");
}

#[test]
fn reorder_and_overlap_deliver_each_byte_once() {
    let mut stream = Reassembler::default();
    stream.push(segment(9, true, b""));
    assert!(stream.push(segment(12, false, b"cde")).is_empty());
    let mut unblock = segment(10, false, b"abc");
    unblock.at = Timestamp(42);
    let events = stream.push(unblock);
    let chunks: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let StreamEvent::Data(c) = e {
                Some(c)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(chunks.len(), 2);
    assert_eq!(
        chunks[1].origins,
        vec![ByteOrigin {
            frame_index: 12,
            offset: 55,
            len: 2
        }]
    );
    assert!(chunks.iter().all(|c| c.at == Timestamp(42)));
    assert_eq!(data(events), b"abcde");
    assert!(stream.push(segment(10, false, b"abc")).is_empty());
}

#[test]
fn wrap_and_extreme_serial_distance_do_not_overflow() {
    let mut stream = Reassembler::default();
    stream.push(segment(u32::MAX - 1, true, b""));
    assert!(stream.push(segment(1, false, b"cd")).is_empty());
    assert_eq!(data(stream.push(segment(u32::MAX, false, b"ab"))), b"abcd");
    assert!(stream
        .push(segment(3u32.wrapping_add(0x8000_0000), false, b"old"))
        .is_empty());
}

#[test]
fn fin_waits_for_contiguous_data_and_rst_retires_both_directions() {
    let mut stream = Reassembler::default();
    stream.push(segment(9, true, b""));
    let mut fin = segment(12, false, b"cd");
    fin.fin = true;
    assert!(stream.push(fin).is_empty());
    let events = stream.push(segment(10, false, b"ab"));
    assert!(matches!(
        events.last(),
        Some(StreamEvent::Closed {
            reason: CloseReason::Fin,
            ..
        })
    ));
    assert_eq!(data(events), b"abcd");
    assert!(stream.push(segment(15, false, b"late")).is_empty());
    assert!(stream.push(segment(9, true, b"")).is_empty());
    let mut rst = segment(15, false, b"");
    rst.rst = true;
    let events = stream.push(rst);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                StreamEvent::Closed {
                    reason: CloseReason::Reset,
                    ..
                }
            ))
            .count(),
        2
    );
    assert!(stream.push(segment(10, false, b"old")).is_empty());
}

#[test]
fn opposite_syn_and_new_generation_are_independent() {
    let mut stream = Reassembler::default();
    let events = stream.push(segment(9, true, b""));
    let old = match events[0] {
        StreamEvent::Opened { conn, .. } => conn,
        _ => panic!("expected open"),
    };
    let mut server = segment(99, true, b"");
    server.dir = Direction::ServerToClient;
    assert!(
        matches!(stream.push(server).as_slice(), [StreamEvent::Opened { conn, fresh:true, .. }] if *conn == old)
    );
    assert_eq!(data(stream.push(segment(10, false, b"abc"))), b"abc");
    let events = stream.push(segment(500, true, b""));
    assert!(
        matches!(events.as_slice(), [StreamEvent::Closed {..}, StreamEvent::Closed {..}, StreamEvent::Opened {conn,..}] if *conn != old)
    );
    assert!(stream.push(segment(10, false, b"abc")).is_empty());
    assert_eq!(data(stream.push(segment(501, false, b"new"))), b"new");
}

#[test]
fn server_change_retired_late_data_and_midstream_are_explicit() {
    let mut stream = Reassembler::default();
    assert!(matches!(
        stream.push(segment(10, false, b"abc")).first(),
        Some(StreamEvent::Opened { fresh: false, .. })
    ));
    let mut other = segment(50, true, b"");
    other.flow.server.ip[3] = 3;
    assert!(matches!(
        stream.push(other).as_slice(),
        [
            StreamEvent::Closed { .. },
            StreamEvent::Closed { .. },
            StreamEvent::Opened { fresh: true, .. }
        ]
    ));
    assert!(stream.push(segment(13, false, b"old")).is_empty());
}

#[test]
fn contradictory_delivered_and_pending_overlap_require_recovery() {
    for pending in [false, true] {
        let mut stream = Reassembler::default();
        stream.push(segment(9, true, b""));
        let seq = if pending { 12 } else { 10 };
        stream.push(segment(seq, false, b"abc"));
        assert!(matches!(
            stream.push(segment(seq + 1, false, b"X")).as_slice(),
            [StreamEvent::Gap {
                reason: GapReason::ConflictingOverlap,
                ..
            }]
        ));
        // The next data goes to an already-gapped decoder, never across the old hole.
        assert_eq!(
            data(stream.push(segment(99, false, b"recovery"))),
            b"recovery"
        );
    }
}

#[test]
fn pending_limits_drop_invalidation_and_stop_are_bounded() {
    let mut stream = Reassembler::default();
    stream.push(segment(0, true, b""));
    for seq in 2..258 {
        assert!(stream.push(segment(seq, false, b"x")).is_empty());
    }
    assert!(matches!(
        stream.push(segment(258, false, b"x")).as_slice(),
        [StreamEvent::Gap {
            reason: GapReason::ReorderLimit,
            ..
        }]
    ));
    assert_eq!(data(stream.push(segment(500, false, b"y"))), b"y");
    assert_eq!(
        stream.gap_all(Timestamp(7), GapReason::CaptureDrop).len(),
        1
    );
    assert_eq!(data(stream.push(segment(600, false, b"z"))), b"z");
    assert_eq!(stream.close_all(Timestamp(8)).len(), 2);
    assert!(stream.close_all(Timestamp(9)).is_empty());
    assert!(stream.push(segment(601, false, b"late")).is_empty());

    let mut stream = Reassembler::default();
    stream.push(segment(0, true, b""));
    assert!(matches!(
        stream
            .push(segment(3, false, &vec![0; 2_097_153]))
            .as_slice(),
        [StreamEvent::Gap {
            reason: GapReason::ReorderLimit,
            ..
        }]
    ));
}

#[test]
fn active_and_retired_generations_are_bounded() {
    let mut stream = Reassembler::default();
    for port in 1..=64 {
        let mut s = segment(0, true, b"");
        s.flow.client.port = port;
        assert_eq!(stream.push(s).len(), 1);
    }
    let mut s = segment(0, true, b"");
    s.flow.client.port = 65;
    assert!(matches!(
        stream.push(s).as_slice(),
        [
            StreamEvent::Closed {
                reason: CloseReason::Replaced,
                ..
            },
            StreamEvent::Closed { .. },
            StreamEvent::Opened { .. }
        ]
    ));
    assert_eq!(stream.close_all(Timestamp(1)).len(), 128);
    for port in 66..=400 {
        let mut s = segment(0, true, b"");
        s.flow.client.port = port;
        stream.push(s);
        stream.close_all(Timestamp(1));
    }
    let mut oldest = segment(1, false, b"new midstream");
    oldest.flow.client.port = 1;
    assert!(matches!(
        stream.push(oldest).first(),
        Some(StreamEvent::Opened { fresh: false, .. })
    ));
}

#[test]
fn pending_byte_budget_and_every_open_direction_are_invalidated() {
    let mut stream = Reassembler::default();
    stream.push(segment(0, true, b""));
    for i in 0..32 {
        assert!(stream
            .push(segment(2 + i * 65_535, false, &vec![0; 65_535]))
            .is_empty());
    }
    assert!(matches!(
        stream
            .push(segment(2 + 32 * 65_535, false, &[0; 33]))
            .as_slice(),
        [StreamEvent::Gap {
            reason: GapReason::ReorderLimit,
            ..
        }]
    ));
    let mut opposite = segment(5, true, b"");
    opposite.dir = Direction::ServerToClient;
    stream.push(opposite);
    let mut other = segment(8, true, b"");
    other.flow.client.port = 1234;
    stream.push(other);
    let events = stream.gap_all(Timestamp(88), GapReason::CaptureDrop);
    assert_eq!(events.len(), 3);
    assert!(events.iter().all(|event| matches!(
        event,
        StreamEvent::Gap {
            at: Timestamp(88),
            reason: GapReason::CaptureDrop,
            ..
        }
    )));
}

#[test]
fn syn_payload_and_malformed_origins_are_checked() {
    let mut stream = Reassembler::default();
    assert_eq!(data(stream.push(segment(9, true, b"abc"))), b"abc");
    assert_eq!(data(stream.push(segment(13, false, b"d"))), b"d");
    let mut malformed = segment(14, false, b"e");
    malformed.origin.offset = usize::MAX;
    assert!(matches!(
        stream.push(malformed).as_slice(),
        [StreamEvent::Gap {
            reason: GapReason::InvalidFrame,
            ..
        }]
    ));
}

#[test]
fn reset_state_is_directional_consumed_once_and_does_not_change_segment_boundaries() {
    let mut stream = Reassembler::default();
    let mut rst = segment(0, false, &[]);
    rst.rst = true;
    stream.push(rst);
    let mut incoming = segment(100, false, &[0; 1460]);
    incoming.flow.client.port += 1;
    incoming.dir = Direction::ServerToClient;
    assert!(matches!(
        stream.push(incoming.clone()).first(),
        Some(StreamEvent::Opened { fresh: false, .. })
    ));
    let mut outgoing = incoming.clone();
    outgoing.dir = Direction::ClientToServer;
    assert!(matches!(
        stream.push(outgoing.clone()).first(),
        Some(StreamEvent::Opened {
            fresh: true,
            syn_proven: false,
            ..
        })
    ));
    outgoing.seq += 1460;
    outgoing.bytes = vec![0; 1198];
    outgoing.origin.len = 1198;
    let events = stream.push(outgoing.clone());
    assert!(matches!(events.as_slice(), [StreamEvent::Data(chunk)] if chunk.bytes.len() == 1198));
    outgoing.flow.client.port += 1;
    assert!(matches!(
        stream.push(outgoing).first(),
        Some(StreamEvent::Opened {
            fresh: true,
            syn_proven: false,
            ..
        })
    ));
}

#[test]
fn connection_replacement_without_rst_resets_only_the_observed_direction() {
    for dir in [Direction::ClientToServer, Direction::ServerToClient] {
        for change_client_socket in [false, true] {
            let mut stream = Reassembler::default();
            let mut old = segment(100, false, b"old");
            old.dir = dir;
            assert!(matches!(
                stream.push(old.clone()).first(),
                Some(StreamEvent::Opened { fresh: false, .. })
            ));
            let mut next = segment(900, false, b"new");
            next.dir = dir;
            if change_client_socket {
                next.flow.client.port += 1;
            } else {
                next.flow.server.ip[3] += 1;
            }
            let mut ack = next.clone();
            ack.bytes.clear();
            ack.origin.len = 0;
            assert!(stream.push(ack).is_empty());
            old.seq += 3;
            assert!(matches!(
                stream.push(old).as_slice(),
                [StreamEvent::Data(_)]
            ));
            let events = stream.push(next.clone());
            assert!(events.iter().any(|event| matches!(event,
                StreamEvent::Opened { dir: actual, fresh: true, syn_proven: false, .. } if *actual == dir)));
            assert_eq!(data(events), b"new");
            next.dir = if dir == Direction::ClientToServer {
                Direction::ServerToClient
            } else {
                Direction::ClientToServer
            };
            assert!(matches!(
                stream.push(next).first(),
                Some(StreamEvent::Opened { fresh: false, .. })
            ));
        }
    }
}
