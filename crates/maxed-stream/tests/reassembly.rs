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
    for seq in 2..33 {
        assert!(stream.push(segment(seq, false, b"x")).is_empty());
    }
    assert!(matches!(
        stream.push(segment(33, false, b"x")).first(),
        Some(StreamEvent::Gap {
            reason: GapReason::ReorderLimit,
            ..
        })
    ));
    assert_eq!(data(stream.push(segment(34, false, b"y"))), b"y");
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
    for i in 0..16 {
        assert!(stream
            .push(segment(2 + i * 131_071, false, &vec![0; 131_071]))
            .is_empty());
    }
    assert!(matches!(
        stream
            .push(segment(2 + 16 * 131_071, false, &[0; 33]))
            .first(),
        Some(StreamEvent::Gap {
            reason: GapReason::ReorderLimit,
            ..
        })
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

#[test]
fn hole_budget_delivers_all_buffered_bytes_after_gap() {
    for start in [0, u32::MAX - 16] {
        let mut stream = Reassembler::default();
        stream.push(segment(start, true, b""));
        for offset in 2..33 {
            assert!(stream
                .push(segment(start.wrapping_add(offset), false, b"x"))
                .is_empty());
        }
        let events = stream.push(segment(start.wrapping_add(33), false, b"x"));
        assert!(matches!(events.first(), Some(StreamEvent::Gap { .. })));
        assert_eq!(data(events), vec![b'x'; 32]);
        assert_eq!(
            data(stream.push(segment(start.wrapping_add(34), false, b"y"))),
            b"y"
        );
    }
}

#[test]
fn short_holes_expire_or_flush_at_wire_reset_with_data_after_gap() {
    for close in [false, true] {
        let mut stream = Reassembler::default();
        stream.push(segment(9, true, b""));
        stream.push(segment(12, false, b"tail"));
        assert!(stream.expire(Timestamp(1_000_000)).is_empty());
        let events = if close {
            {
                let mut rst = segment(10, false, b"");
                rst.rst = true;
                rst.at = Timestamp(2);
                stream.push(rst)
            }
        } else {
            stream.expire(Timestamp(1_000_001))
        };
        assert!(matches!(events.first(), Some(StreamEvent::Gap { .. })));
        assert_eq!(data(events), b"tail");
    }
}

#[test]
fn flow_gap_leaves_other_flow_and_opposite_direction_contiguous() {
    let mut stream = Reassembler::default();
    let first = segment(9, true, b"");
    let mut other = first.clone();
    other.flow.client.port += 1;
    let mut opposite = first.clone();
    opposite.dir = Direction::ServerToClient;
    stream.push(first.clone());
    stream.push(other.clone());
    stream.push(opposite.clone());
    let events = stream.gap_flow(first.flow, first.dir, Timestamp(2), GapReason::InvalidFrame);
    assert!(matches!(
        events.as_slice(),
        [StreamEvent::Gap {
            conn: ConnId(1),
            dir: Direction::ClientToServer,
            ..
        }]
    ));
    for mut s in [other, opposite] {
        s.syn = false;
        s.seq = 10;
        s.bytes = b"ok".to_vec();
        s.origin.len = 2;
        let events = stream.push(s);
        assert!(matches!(events.as_slice(), [StreamEvent::Data(_)]));
        assert_eq!(data(events), b"ok");
    }
}

#[test]
fn forced_far_ahead_hole_invalidates_without_pinning_the_cursor() {
    for budget in [false, true] {
        let mut stream = Reassembler::default();
        stream.push(segment(9, true, b""));
        stream.push(segment(10 + 0x4000_0000, false, b"stray"));
        assert_eq!(data(stream.push(segment(10, false, b"ok"))), b"ok");
        let events = if budget {
            for offset in 1..31 {
                assert!(stream
                    .push(segment(10 + 0x4000_0000 + offset * 5, false, b"stray"))
                    .is_empty());
            }
            stream.push(segment(10 + 0x4000_0000 + 31 * 5, false, b"stray"))
        } else {
            stream.expire(Timestamp(1_000_001))
        };
        assert!(matches!(
            events.as_slice(),
            [StreamEvent::Gap {
                reason: GapReason::ReorderLimit,
                ..
            }]
        ));
        assert_eq!(data(stream.push(segment(12, false, b"next"))), b"next");
    }
}

#[test]
fn teardown_discards_pending_holes_without_gap_or_data() {
    let mut stream = Reassembler::default();
    stream.push(segment(9, true, b""));
    stream.push(segment(12, false, b"tail"));
    let events = stream.close_all(Timestamp(2_000_000));
    assert!(events.iter().all(|e| matches!(
        e,
        StreamEvent::Closed {
            reason: CloseReason::Stopped,
            ..
        }
    )));
}

#[test]
fn forced_advance_byte_boundary_including_wraparound() {
    for expected in [10u32, u32::MAX - 10] {
        for distance in [2_097_152u32, 2_097_153] {
            for dir in [Direction::ClientToServer, Direction::ServerToClient] {
                let mut stream = Reassembler::default();
                let mut syn = segment(expected.wrapping_sub(1), true, b"");
                syn.dir = dir;
                stream.push(syn);
                let next = expected.wrapping_add(distance);
                let mut ahead = segment(next, false, b"x");
                ahead.dir = dir;
                assert!(stream.push(ahead).is_empty());
                let events = stream.expire(Timestamp(1_000_001));
                assert!(matches!(events.first(), Some(StreamEvent::Gap { .. })));
                if distance == 2_097_152 {
                    assert_eq!(data(events), b"x");
                } else {
                    assert_eq!(events.len(), 1);
                    let mut normal = segment(expected, false, b"ok");
                    normal.dir = dir;
                    normal.at = Timestamp(1_000_001);
                    assert_eq!(data(stream.push(normal)), b"ok");
                }
            }
        }
    }
}

#[test]
fn backward_clock_step_restores_reorder_tolerance_in_both_directions() {
    for dir in [Direction::ClientToServer, Direction::ServerToClient] {
        let mut stream = Reassembler::default();
        let mut send = |seq, at, bytes: &[u8]| {
            let mut s = segment(seq, seq == 9, bytes);
            s.dir = dir;
            s.at = Timestamp(at);
            stream.push(s)
        };
        send(9, 10_000_000, b"");
        assert!(send(12, 2_000_000, b"tail").is_empty());
        assert_eq!(data(send(10, 2_000_001, b"ok")), b"oktail");
        assert!(send(18, 2_000_002, b"next").is_empty());
        assert!(stream.expire(Timestamp(3_000_001)).is_empty());
        let events = stream.expire(Timestamp(3_000_002));
        assert!(matches!(events.first(), Some(StreamEvent::Gap { .. })));
        assert_eq!(data(events), b"next");
    }
}

#[test]
fn existing_hole_keeps_its_age_across_clock_step_and_small_reorders() {
    for dir in [Direction::ClientToServer, Direction::ServerToClient] {
        let mut stream = Reassembler::default();
        for (seq, at, bytes) in [(9, 10_000_000, &b""[..]), (12, 10_000_000, &b"tail"[..])] {
            let mut s = segment(seq, seq == 9, bytes);
            s.dir = dir;
            s.at = Timestamp(at);
            stream.push(s);
        }
        assert!(stream.expire(Timestamp(10_500_000)).is_empty());
        assert!(stream.expire(Timestamp(2_000_000)).is_empty());
        assert!(stream.expire(Timestamp(1_900_000)).is_empty());
        assert!(stream.expire(Timestamp(2_499_999)).is_empty());
        let events = stream.expire(Timestamp(2_500_000));
        assert!(matches!(events.first(), Some(StreamEvent::Gap { .. })));
        assert_eq!(data(events), b"tail");
    }
}

#[test]
fn stalled_capture_clock_still_expires_by_segment_budget() {
    for dir in [Direction::ClientToServer, Direction::ServerToClient] {
        let mut stream = Reassembler::default();
        let mut syn = segment(9, true, b"");
        syn.dir = dir;
        syn.at = Timestamp(10_000_000);
        stream.push(syn);
        for offset in 0..32 {
            let mut s = segment(12 + offset, false, b"x");
            s.dir = dir;
            s.at = Timestamp(1);
            let events = stream.push(s);
            if offset < 31 {
                assert!(events.is_empty());
            } else {
                assert!(matches!(events.first(), Some(StreamEvent::Gap { .. })));
                assert_eq!(data(events), vec![b'x'; 32]);
            }
        }
    }
}

#[test]
fn delivery_reuses_segment_allocation_even_with_partial_overlap() {
    let mut stream = Reassembler::default();
    stream.push(segment(9, true, b""));
    let first = segment(10, false, b"abc");
    let allocation = first.bytes.as_ptr();
    let events = stream.push(first);
    let StreamEvent::Data(chunk) = &events[0] else {
        panic!("expected data")
    };
    assert_eq!(chunk.bytes.as_ptr(), allocation);
    let overlap = segment(11, false, b"bcdef");
    let allocation = overlap.bytes.as_ptr();
    let events = stream.push(overlap);
    let StreamEvent::Data(chunk) = &events[0] else {
        panic!("expected data")
    };
    assert_eq!(chunk.bytes.as_ptr(), allocation);
    assert_eq!(chunk.bytes, b"def");
    assert_eq!(chunk.origins[0].offset, 56);
}

#[test]
fn capture_time_regression_cannot_postpone_an_already_aged_hole() {
    let mut stream = Reassembler::default();
    let mut syn = segment(9, true, b"");
    // Exactly the age budget remains a reorder, not a clock re-baseline.
    syn.at = Timestamp(1_000_001);
    stream.push(syn);
    let mut late = segment(12, false, b"tail");
    late.at = Timestamp(1);
    let events = stream.push(late);
    assert!(matches!(events.first(), Some(StreamEvent::Gap { .. })));
    assert_eq!(data(events), b"tail");
    assert_eq!(data(stream.push(segment(16, false, b"next"))), b"next");
}
