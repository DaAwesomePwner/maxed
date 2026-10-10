use maxed_cli::Pipeline;
use maxed_protocol::{key_bytes, DecodeEvent, Rc4};
use maxed_types::{Direction, RawFrame, Timestamp};

fn packet(cipher: &mut Rc4, id: u8, len: usize, tick: Option<i32>) -> Vec<u8> {
    let mut body = vec![0; len - 5];
    if let Some(n) = tick {
        body[..4].copy_from_slice(&n.to_be_bytes());
    }
    cipher.apply(&mut body);
    let mut out = (len as u32).to_be_bytes().to_vec();
    out.push(id);
    out.extend(body);
    out
}
fn frame(
    index: u64,
    at: u64,
    (server, stale): (bool, bool),
    seq: u32,
    flags: u8,
    payload: &[u8],
    padding: bool,
) -> RawFrame {
    let mut b = vec![0; 54 + payload.len()];
    b[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    b[14] = 0x45;
    b[16..18].copy_from_slice(&((40 + payload.len()) as u16).to_be_bytes());
    b[23] = 6;
    b[26..30].copy_from_slice(&[127, 0, 0, if server { 2 } else { 1 }]);
    b[30..34].copy_from_slice(&[127, 0, 0, if server { 1 } else { 2 }]);
    let client = if stale { 50001u16 } else { 50000u16 };
    b[34..36].copy_from_slice(&(if server { 2050 } else { client }).to_be_bytes());
    b[36..38].copy_from_slice(&(if server { client } else { 2050 }).to_be_bytes());
    b[38..42].copy_from_slice(&seq.to_be_bytes());
    b[46] = 0x50;
    b[47] = flags;
    b[54..].copy_from_slice(payload);
    if padding {
        b.extend([0x5a; 6]);
    }
    RawFrame {
        index,
        at: Timestamp(at),
        original_len: b.len() as u32,
        bytes: b,
    }
}

// Synthetic structural startup trace. No capture bytes are used.
const TRACE: &str = "
1 1790914361650035 s 0 0 6 20
2 1790914362783556 s 1 0 1460 16
3 1790914362783631 c 1 0 0 16
4 1790914362783854 s 1 1460 1460 16
5 1790914362783872 c 1 0 0 16
6 1790914362784390 s 1 2920 1460 16
7 1790914362784442 c 1 0 0 16
8 1790914362784634 s 1 4380 1460 16
9 1790914362784669 c 1 0 0 16
10 1790914362784721 s 1 5840 1460 16
11 1790914362784732 c 1 0 0 16
12 1790914362784796 s 1 7300 1460 16
13 1790914362784831 c 1 0 0 16
14 1790914362784962 s 1 8760 1460 16
15 1790914362784988 c 1 0 0 16
16 1790914362785160 s 1 10220 1460 16
17 1790914362785184 c 1 0 0 16
18 1790914362785231 s 1 11680 1460 16
19 1790914362785242 c 1 0 0 16
20 1790914362785384 s 1 13140 1460 16
21 1790914362785408 c 1 0 0 16
22 1790914362816079 c 1 0 9 24
23 1790914362816116 c 1 9 5 24
24 1790914362818961 s 1 14600 1460 16
25 1790914362818962 s 1 16060 1460 16
26 1790914362818962 s 1 17520 1460 16
27 1790914362819020 c 1 14 0 16
28 1790914362819057 s 1 18980 1460 16
29 1790914362819081 c 1 14 0 16
30 1790914362819107 s 1 20440 1198 24
31 1790914362819115 c 1 14 0 16
32 1790914362830931 s 1 21638 11 24
33 1790914362830966 c 1 14 0 16
34 1790914362851096 s 1 21649 6 16
35 1790914362851096 s 1 21649 6 16
36 1790914362921144 c 1 14 5 24
37 1790914362921178 c 1 19 5 24
38 1790914362921200 c 1 24 5 24
39 1790914362921215 c 1 29 5 24
40 1790914362921229 c 1 34 5 24
41 1790914362921241 c 1 39 5 24
42 1790914362921257 c 1 44 5 24
43 1790914362921270 c 1 49 5 24
44 1790914362921286 c 1 54 5 24
45 1790914362921298 c 1 59 5 24
46 1790914362921313 c 1 64 5 24
47 1790914362921323 c 1 69 5 24
48 1790914362921334 c 1 74 5 24
49 1790914362921343 c 1 79 5 24
50 1790914362921354 c 1 84 5 24
51 1790914362921366 c 1 89 5 24
52 1790914362921381 c 1 94 5 24
53 1790914362921394 c 1 99 5 24
54 1790914362921409 c 1 104 5 24
55 1790914362921427 c 1 109 5 24
56 1790914362921440 c 1 114 5 24
57 1790914362921456 c 1 119 5 24
58 1790914362921468 c 1 124 5 24
59 1790914362921482 c 1 129 5 24
60 1790914362921495 c 1 134 5 24
61 1790914362921508 c 1 139 5 24
62 1790914362921523 c 1 144 5 24
63 1790914362921539 c 1 149 5 24
64 1790914362921553 c 1 154 5 24
65 1790914362921567 c 1 159 5 24
66 1790914362930873 s 1 21649 1460 16
67 1790914362930878 s 1 23109 584 24
68 1790914362930948 c 1 164 0 16
69 1790914362954801 s 1 23693 6 16
70 1790914362954801 s 1 23693 6 16
71 1790914362954902 s 1 23693 6 16
72 1790914362954902 s 1 23693 6 16
73 1790914362954902 s 1 23693 6 16
74 1790914362954903 s 1 23693 6 16
75 1790914362954903 s 1 23693 6 16
76 1790914362954903 s 1 23693 6 16
77 1790914362954904 s 1 23693 6 16
78 1790914362954904 s 1 23693 6 16
79 1790914362954936 s 1 23693 6 16
80 1790914362954939 s 1 23693 6 16
";

#[test]
fn stale_rst_disarms_only_its_direction_startup_gate_before_the_short_segment() {
    let mut cipher = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
    cipher.skip(1234);
    // A spans seven full segments; B finishes in frame 30, followed by the
    // first emitted packet. Neither tick nor any earlier packet is replayed.
    let mut server = packet(&mut cipher, 10, 10220, Some(50));
    server.extend(packet(&mut cipher, 10, 11407, Some(51)));
    server.extend(packet(&mut cipher, 120, 11, None));
    server.extend(packet(&mut cipher, 82, 11, None));
    server.extend(packet(&mut cipher, 10, 21, Some(52)));
    for (id, len) in [(82, 11), (120, 11), (8, 9), (82, 1408)] {
        server.extend(packet(&mut cipher, id, len, None));
    }
    server.extend(packet(&mut cipher, 10, 605, Some(53)));
    server.extend(packet(&mut cipher, 82, 11, None));
    let mut client = vec![0, 0, 0, 9, 250, 0, 0, 0, 0];
    for _ in 0..32 {
        client.extend([0, 0, 0, 5, 105]);
    }
    for with_reset in [true, false] {
        let mut pipeline = Pipeline::default();
        let mut emitted = Vec::new();
        for line in TRACE.trim().lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            assert_eq!(fields.len(), 7);
            let index = fields[0].parse::<u64>().unwrap() - 1;
            let at = fields[1].parse().unwrap();
            let incoming = fields[2] == "s";
            let stale = fields[3] == "0";
            let seq = fields[4].parse::<usize>().unwrap();
            let size = fields[5].parse::<usize>().unwrap();
            let mut flags = fields[6].parse().unwrap();
            if stale && !with_reset {
                flags = 16;
            }
            let padding = size == 6;
            let payload = if padding {
                &[][..]
            } else if incoming {
                &server[seq..seq + size]
            } else {
                &client[seq..seq + size]
            };
            let raw = frame(
                index,
                at,
                (incoming, stale),
                seq as u32,
                flags,
                payload,
                padding,
            );
            let parsed = maxed_capture::parse_frame(&raw).unwrap().unwrap();
            assert_eq!(parsed.bytes.len(), if padding { 0 } else { size });
            for e in pipeline.push(raw) {
                if let DecodeEvent::Packet(p) = e {
                    assert_eq!(p.dir, Direction::ServerToClient);
                    emitted.push((p.id, p.at.0));
                }
            }
        }
        if with_reset {
            assert_eq!(
                emitted,
                vec![
                    (120, 1790914362819107),
                    (82, 1790914362830931),
                    (10, 1790914362930873),
                    (82, 1790914362930873),
                    (120, 1790914362930873),
                    (8, 1790914362930873),
                    (82, 1790914362930873)
                ]
            );
        } else {
            assert!(emitted.is_empty());
        }
        let events = pipeline.push(frame(
            80,
            1790914363331561,
            (true, false),
            23693,
            24,
            &server[23693..],
            false,
        ));
        let packets: Vec<_> = events
            .into_iter()
            .filter_map(|e| match e {
                DecodeEvent::Packet(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(packets.last().unwrap().id, 82);
        assert_eq!(packets.last().unwrap().at, Timestamp(1790914363331561));
    }
}

#[test]
fn first_non_tick_after_stale_rst_decodes_immediately_with_a_fresh_cipher() {
    let mut pipeline = Pipeline::default();
    let reset = pipeline.push(frame(0, 10, (true, true), 0, 20, &[], true));
    assert!(!reset.iter().any(|e| matches!(e, DecodeEvent::Packet(_))));
    let mut cipher = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
    for (index, at, seq) in [(1, 20, 200), (2, 30, 215)] {
        let payload = packet(&mut cipher, 101, 15, None);
        let events = pipeline.push(frame(index, at, (true, false), seq, 24, &payload, false));
        assert!(matches!(events.as_slice(), [DecodeEvent::Packet(p)]
            if !p.alignment_verified && p.id == 101 && p.at == Timestamp(at) && p.packet.fields().unwrap() ==
                serde_json::json!({"objectId":0,"charId":0,"str":""})));
    }
}

#[test]
fn first_non_tick_after_connection_replacement_without_rst_uses_reset_cipher() {
    for change_client_socket in [false, true] {
        let mut pipeline = Pipeline::default();
        let mut old_cipher = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
        let old_payload = packet(&mut old_cipher, 101, 15, None);
        // Initial midstream flow has neither SYN nor a pending reset, so its
        // first short segment is gated. Replacement must clear that state.
        let old = pipeline.push(frame(
            0,
            10,
            (true, change_client_socket),
            100,
            24,
            &old_payload,
            false,
        ));
        assert!(!old.iter().any(|e| matches!(e, DecodeEvent::Packet(_))));
        let mut new_cipher = Rc4::new(&key_bytes(Direction::ServerToClient).unwrap()).unwrap();
        let payload = packet(&mut new_cipher, 101, 15, None);
        let mut new = frame(1, 20, (true, false), 900, 24, &payload, false);
        if !change_client_socket {
            new.bytes[29] = 3;
        }
        let packets: Vec<_> = pipeline
            .push(new)
            .into_iter()
            .filter_map(|e| match e {
                DecodeEvent::Packet(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].id, 101);
        assert!(!packets[0].alignment_verified);
        assert_eq!(packets[0].at, Timestamp(20));
        assert_eq!(
            packets[0].packet.fields().unwrap(),
            serde_json::json!({"objectId":0,"charId":0,"str":""})
        );
    }
}
