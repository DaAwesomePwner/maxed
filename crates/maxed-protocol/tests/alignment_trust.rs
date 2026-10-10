use maxed_protocol::{canonical_line, key_bytes, DecodeEvent, Decoder, Diagnostic, Rc4};
use maxed_types::{ConnId, Direction, GapReason, StreamChunk, StreamEvent, Timestamp};

const S: Direction = Direction::ServerToClient;
const C: Direction = Direction::ClientToServer;

fn open(d: &mut Decoder, dir: Direction, syn_proven: bool) {
    d.push(StreamEvent::Opened {
        conn: ConnId(1),
        dir,
        at: Timestamp(0),
        fresh: true,
        syn_proven,
    });
}
fn frame(cipher: &mut Rc4, id: u8, mut body: Vec<u8>) -> Vec<u8> {
    cipher.apply(&mut body);
    let mut bytes = ((body.len() + 5) as u32).to_be_bytes().to_vec();
    bytes.push(id);
    bytes.extend(body);
    bytes
}
fn push(d: &mut Decoder, dir: Direction, bytes: Vec<u8>) -> Vec<DecodeEvent> {
    d.push(StreamEvent::Data(StreamChunk {
        conn: ConnId(1),
        dir,
        at: Timestamp(1),
        bytes,
        origins: vec![],
    }))
}
fn tick(dir: Direction, n: i32) -> (u8, Vec<u8>) {
    let mut body = vec![0; if dir == S { 16 } else { 4 }];
    body[..4].copy_from_slice(&n.to_be_bytes());
    (if dir == S { 10 } else { 62 }, body)
}
fn plain(dir: Direction) -> (u8, Vec<u8>) {
    if dir == S {
        (101, vec![0; 10])
    } else {
        (105, vec![])
    }
}
fn send(
    d: &mut Decoder,
    cipher: &mut Rc4,
    dir: Direction,
    packet: (u8, Vec<u8>),
) -> Vec<DecodeEvent> {
    push(d, dir, frame(cipher, packet.0, packet.1))
}

#[test]
fn syn_proven_starts_verified_and_reset_assumptions_need_two_ticks_without_export_changes() {
    for dir in [S, C] {
        let mut exports = Vec::new();
        for syn_proven in [false, true] {
            let mut d = Decoder::default();
            open(&mut d, dir, syn_proven);
            let mut cipher = Rc4::new(&key_bytes(dir).unwrap()).unwrap();
            let mut trust = Vec::new();
            let mut lines = Vec::new();
            for (index, packet) in [
                plain(dir),
                tick(dir, 0),
                plain(dir),
                tick(dir, 1),
                plain(dir),
            ]
            .into_iter()
            .enumerate()
            {
                let events = send(&mut d, &mut cipher, dir, packet);
                let DecodeEvent::Packet(p) = &events[0] else {
                    panic!("packet required");
                };
                trust.push(p.alignment_verified);
                lines.push(
                    serde_json::to_string(&canonical_line(index as u64, p).unwrap()).unwrap(),
                );
            }
            assert_eq!(
                trust,
                if syn_proven {
                    vec![true; 5]
                } else {
                    vec![false, false, false, true, true]
                }
            );
            exports.push(lines);
        }
        assert_eq!(exports[0], exports[1]);
    }
}

#[test]
fn recovering_suppresses_packets_until_recovered_and_new_opens_reset_trust() {
    for dir in [S, C] {
        let mut d = Decoder::default();
        d.explain_failures(true);
        open(&mut d, dir, true);
        let mut cipher = Rc4::new(&key_bytes(dir).unwrap()).unwrap();
        let initial = send(&mut d, &mut cipher, dir, plain(dir));
        assert!(matches!(&initial[0], DecodeEvent::Packet(p) if p.alignment_verified));
        cipher.skip(13); // unknown ciphertext lost before the next observed tick
        let first = send(&mut d, &mut cipher, dir, tick(dir, 70));
        assert!(matches!(
            first.as_slice(),
            [DecodeEvent::Diagnostic {
                kind: Diagnostic::Recovering,
                ..
            }]
        ));
        assert!(send(&mut d, &mut cipher, dir, plain(dir)).is_empty());
        let second = send(&mut d, &mut cipher, dir, tick(dir, 71));
        assert!(matches!(
            second.as_slice(),
            [DecodeEvent::Diagnostic {
                kind: Diagnostic::Recovered,
                ..
            }]
        ));
        let recovered = send(&mut d, &mut cipher, dir, plain(dir));
        assert!(matches!(&recovered[0], DecodeEvent::Packet(p) if !p.alignment_verified));
        if dir == S {
            assert!(d
                .take_failures()
                .last()
                .unwrap()
                .to_string()
                .contains("prefixHex=redacted-unverified"));
        }
        let confirming = send(&mut d, &mut cipher, dir, tick(dir, 72));
        assert!(matches!(&confirming[0], DecodeEvent::Packet(p) if p.alignment_verified));
        let following = send(&mut d, &mut cipher, dir, plain(dir));
        assert!(matches!(&following[0], DecodeEvent::Packet(p) if p.alignment_verified));

        if dir == S {
            assert!(!d
                .take_failures()
                .last()
                .unwrap()
                .to_string()
                .contains("redacted-unverified"));
        }
        open(&mut d, dir, false);
        let mut cipher = Rc4::new(&key_bytes(dir).unwrap()).unwrap();
        let reset = send(&mut d, &mut cipher, dir, plain(dir));
        assert!(matches!(&reset[0], DecodeEvent::Packet(p) if !p.alignment_verified));
    }
}

#[test]
fn trust_is_directional_and_gap_recovery_does_not_restore_syn_proof_early() {
    let mut d = Decoder::default();
    open(&mut d, S, true);
    open(&mut d, C, false);
    let mut server = Rc4::new(&key_bytes(S).unwrap()).unwrap();
    let mut client = Rc4::new(&key_bytes(C).unwrap()).unwrap();
    assert!(
        matches!(&send(&mut d, &mut server, S, plain(S))[0], DecodeEvent::Packet(p) if p.alignment_verified)
    );
    assert!(
        matches!(&send(&mut d, &mut client, C, plain(C))[0], DecodeEvent::Packet(p) if !p.alignment_verified)
    );
    let gap = d.push(StreamEvent::Gap {
        conn: ConnId(1),
        dir: S,
        at: Timestamp(2),
        reason: GapReason::CaptureDrop,
    });
    assert!(matches!(
        gap.as_slice(),
        [DecodeEvent::Diagnostic {
            kind: Diagnostic::Recovering,
            ..
        }]
    ));
    assert!(send(&mut d, &mut server, S, plain(S)).is_empty()); // startup gate
    assert!(send(&mut d, &mut server, S, plain(S)).is_empty()); // cannot use stale SYN proof
    assert!(send(&mut d, &mut server, S, tick(S, 50)).is_empty());
    let recovered = send(&mut d, &mut server, S, tick(S, 51));
    assert!(matches!(
        recovered.as_slice(),
        [DecodeEvent::Diagnostic {
            kind: Diagnostic::Recovered,
            ..
        }]
    ));
    assert!(
        matches!(&send(&mut d, &mut server, S, plain(S))[0], DecodeEvent::Packet(p) if !p.alignment_verified)
    );
    assert!(
        matches!(&send(&mut d, &mut server, S, tick(S, 52))[0], DecodeEvent::Packet(p) if p.alignment_verified)
    );
    assert!(
        matches!(&send(&mut d, &mut server, S, plain(S))[0], DecodeEvent::Packet(p) if p.alignment_verified)
    );
    assert!(
        matches!(&send(&mut d, &mut client, C, plain(C))[0], DecodeEvent::Packet(p) if !p.alignment_verified)
    );
}

#[test]
fn opposite_direction_tick_ids_cannot_verify_an_assumed_start() {
    let mut d = Decoder::default();
    open(&mut d, C, false);
    let mut cipher = Rc4::new(&key_bytes(C).unwrap()).unwrap();
    for n in [0, 1] {
        let events = send(&mut d, &mut cipher, C, tick(S, n));
        assert!(matches!(&events[0], DecodeEvent::Packet(p) if !p.alignment_verified));
        assert!(events.iter().any(|e| matches!(
            e,
            DecodeEvent::Diagnostic {
                kind: Diagnostic::WrongDirection,
                ..
            }
        )));
    }
    let first = send(&mut d, &mut cipher, C, tick(C, 2));
    assert!(matches!(&first[0], DecodeEvent::Packet(p) if !p.alignment_verified));
    let second = send(&mut d, &mut cipher, C, tick(C, 3));
    assert!(matches!(&second[0], DecodeEvent::Packet(p) if p.alignment_verified));
}

#[test]
fn diagnostic_previews_require_verified_alignment_including_successful_outer_packets() {
    for syn_proven in [false, true] {
        let mut d = Decoder::default();
        d.explain_failures(true);
        open(&mut d, S, syn_proven);
        d.take_failures();
        let mut cipher = Rc4::new(&key_bytes(S).unwrap()).unwrap();
        let mut create = vec![0; 8];
        create.extend(8u16.to_be_bytes());
        create.extend(b"AAAAAAA="); // complete outer, incomplete PcStats with one tail byte
        let mut update = vec![0; 9];
        update.extend([0, 1, 0x12, 0x34, 1]); // object type and ID
        update.extend([0; 8]); // position
        update.extend([1, 31, 0, 80]); // truncated string stat
        update.extend(b"SECRET");
        let mut newtick = vec![0; 14];
        newtick.extend([0, 1, 1]); // status count and object ID
        newtick.extend([0; 8]); // position
        newtick.extend([1, 20, 255, 255, 255, 255, 255, 42]); // overflowing integer stat
        let mut missing_tile = vec![0; 9];
        missing_tile.push(1);
        let cases = [
            (42, missing_tile),
            (42, update),
            (10, newtick),
            (67, vec![24, 32, 1, 104]),
            (114, vec![0x12]),
            (101, create),
        ];
        for (ordinal, packet) in cases.into_iter().enumerate() {
            let id = packet.0;
            send(&mut d, &mut cipher, S, packet);
            let reports = d.take_failures();
            assert_eq!(reports.len(), 1, "id={id}");
            let text = reports[0].to_string();
            assert!(text.contains(&format!(
                "packetOrdinal={ordinal} connId=1 dir=s2c timestampMicros=1"
            )));
            if syn_proven {
                assert!(!text.contains("redacted-unverified"));
                match id {
                    42 if ordinal == 0 => {
                        assert!(text.contains("prefixHex=00000000000000000001"));
                    }
                    42 => assert!(text.contains("stats=[(4660,31,string)]")),
                    10 => assert!(text.contains("stats=[(unknown,20,int)]")),
                    67 => {
                        assert!(text.contains("effectByte=24 extraByte=32"));
                        assert!(text.contains("remainingLength=2 remainingHex=0168"));
                    }
                    114 => assert!(text.contains("nextHex=12")),
                    101 => assert!(text.contains("tailHex=00")),
                    _ => unreachable!(),
                }
            } else {
                assert!(text.contains("prefixHex=redacted-unverified"));
                for preview in [
                    "nextHex=",
                    "remainingHex=",
                    "tailHex=",
                    "stats=[",
                    "effectByte=",
                    "extraByte=",
                ] {
                    assert!(!text.contains(preview), "{text}");
                }
                if id != 101 {
                    assert!(text.contains("offset="));
                    assert!(text.contains("bodyOffset="));
                }
                if matches!(id, 42 | 10) {
                    assert!(text.contains("stats=redacted-unverified"));
                }
                assert!(text.contains("len="));
                if id == 67 {
                    assert!(text.contains("remainingLength=2"));
                }
                if id == 101 {
                    assert!(text.contains("base64Length=8"));
                    assert!(text.contains("decodedLength=5"));
                }
            }
        }
        // The malformed NEWTICK above already consumed tick 0.
        send(&mut d, &mut cipher, S, tick(S, 1));
        send(&mut d, &mut cipher, S, tick(S, 2));
        send(&mut d, &mut cipher, S, (67, vec![24, 32, 1, 104]));
        let reports = d.take_failures();
        assert!(reports
            .last()
            .unwrap()
            .to_string()
            .contains("remainingHex=0168"));
    }
}
