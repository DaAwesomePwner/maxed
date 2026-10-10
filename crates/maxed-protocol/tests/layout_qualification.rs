//! Synthetic coverage for unresolved layout reports. These are deliberately
//! rejected until field boundaries are established; no owner bytes are used.
use maxed_protocol::{
    decode_packet, key_bytes, DecodeEvent, Decoder, Diagnostic, ProtocolError, Rc4,
};
use maxed_types::{ConnId, Direction, StreamChunk, StreamEvent, Timestamp};

const SERVER: Direction = Direction::ServerToClient;

fn frame(cipher: &mut Rc4, id: u8, mut body: Vec<u8>) -> Vec<u8> {
    let mut bytes = ((body.len() + 5) as u32).to_be_bytes().to_vec();
    bytes.push(id);
    cipher.apply(&mut body);
    bytes.extend(body);
    bytes
}

fn tick(id: i32) -> Vec<u8> {
    let mut body = vec![0; 16];
    body[..4].copy_from_slice(&id.to_be_bytes());
    body
}

#[test]
fn unresolved_shapes_stay_strict_and_each_preserves_the_next_encrypted_tick() {
    let mut negative_count = vec![0; 9];
    negative_count.extend([0x41, 0, 0]); // negative tile count
    let mut impossible_count = vec![0; 9];
    impossible_count.extend([63, 0, 0]); // positive count beyond the available tiles
    let mut create = vec![0; 8];
    create.extend([0, 12]);
    create.extend(b"synthetic"); // fewer bytes than the declared string length
    let mut exaltation = vec![0; 10];
    exaltation[9] = 0x80; // continuation bit on the last progress field
    let cases = [
        (42, negative_count, ProtocolError::Length),
        (42, impossible_count, ProtocolError::Length),
        (101, create, ProtocolError::Truncated),
        (114, exaltation, ProtocolError::Truncated),
        (67, vec![24, 32, 0, 0], ProtocolError::Definition),
        (67, vec![24, 0, 0, 1], ProtocolError::Definition),
        (67, vec![30, 0, 0, 0], ProtocolError::Definition),
        (67, vec![74, 0, 0, 0], ProtocolError::Definition),
    ];
    for (id, body, expected) in cases {
        assert_eq!(decode_packet(id, SERVER, &body).err(), Some(expected));
        // Both coalesced frames and separate transport chunks must preserve the
        // established cipher position after the field decoder rejects a packet.
        for coalesced in [false, true] {
            let mut decoder = Decoder::default();
            decoder.explain_failures(true);
            decoder.push(StreamEvent::Opened {
                conn: ConnId(1),
                dir: SERVER,
                at: Timestamp(0),
                fresh: true,
                syn_proven: true,
            });
            let mut cipher = Rc4::new(&key_bytes(SERVER).unwrap()).unwrap();
            let frames = [
                frame(&mut cipher, 10, tick(0)),
                frame(&mut cipher, id, body.clone()),
                frame(&mut cipher, 10, tick(1)),
            ];
            let chunks = if coalesced {
                vec![frames.concat()]
            } else {
                frames.to_vec()
            };
            let mut events = Vec::new();
            for bytes in chunks {
                events.extend(decoder.push(StreamEvent::Data(StreamChunk {
                    conn: ConnId(1),
                    dir: SERVER,
                    at: Timestamp(1),
                    bytes,
                    origins: vec![],
                })));
            }
            assert_eq!(events.len(), 4);
            assert!(matches!(&events[0], DecodeEvent::Packet(p) if p.id == 10));
            assert!(matches!(
                &events[1],
                DecodeEvent::Diagnostic {
                    kind: Diagnostic::BadFields,
                    ..
                }
            ));
            assert!(matches!(
                &events[2],
                DecodeEvent::Diagnostic {
                    kind: Diagnostic::Packet {
                        id: packet_id,
                        problem: maxed_protocol::PacketDiagnostic::BadFields,
                        alignment_verified: true,
                    },
                    ..
                } if *packet_id == id
            ));
            let DecodeEvent::Packet(packet) = &events[3] else {
                panic!("the next aligned tick must decode after rejection");
            };
            assert_eq!(packet.id, 10);
            assert_eq!(packet.packet.fields().unwrap()["tickId"], 1);
            let failures: Vec<_> = decoder
                .take_failures()
                .into_iter()
                .filter_map(|entry| {
                    if let maxed_protocol::explanation::StructuralReport::Failure {
                        failure, ..
                    } = entry
                    {
                        Some(failure)
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(failures.len(), 1);
            assert_eq!(failures[0].error, expected);
        }
    }
}

#[test]
fn current_update_create_and_exaltation_layouts_remain_supported() {
    for (id, body) in [
        (42, vec![0; 12]),  // position, level, three empty lists
        (101, vec![0; 10]), // objectId, charId, empty string
        (114, vec![0; 10]), // class type, eight one-byte progress integers
    ] {
        let (_, remaining) = decode_packet(id, SERVER, &body).unwrap();
        assert_eq!(remaining, 0);
    }
}
