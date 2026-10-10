use maxed_protocol::explanation::{ReportContext, StructuralReport};
use maxed_protocol::{key_bytes, Decoder, Rc4};
use maxed_types::{CloseReason, ConnId, Direction, GapReason, StreamChunk, StreamEvent, Timestamp};

const S: Direction = Direction::ServerToClient;
const C: Direction = Direction::ClientToServer;

fn open(decoder: &mut Decoder, conn: u64, dir: Direction, fresh: bool) {
    decoder.push(StreamEvent::Opened {
        conn: ConnId(conn),
        dir,
        at: Timestamp(5),
        fresh,
        syn_proven: fresh,
    });
}
fn push(decoder: &mut Decoder, conn: u64, dir: Direction, bytes: Vec<u8>) {
    decoder.push(StreamEvent::Data(StreamChunk {
        conn: ConnId(conn),
        dir,
        at: Timestamp(100),
        bytes,
        origins: vec![],
    }));
}
fn frame(cipher: &mut Rc4, id: u8, mut body: Vec<u8>) -> Vec<u8> {
    let mut out = ((body.len() + 5) as u32).to_be_bytes().to_vec();
    out.push(id);
    cipher.apply(&mut body);
    out.extend(body);
    out
}
fn tick(id: i32) -> Vec<u8> {
    let mut body = vec![0; 16];
    body[..4].copy_from_slice(&id.to_be_bytes());
    body
}
fn context(report: &StructuralReport) -> &ReportContext {
    match report {
        StructuralReport::Failure { context, .. }
        | StructuralReport::Stream { context, .. }
        | StructuralReport::CreateSuccess { context, .. } => context,
    }
}

#[test]
fn global_ordinals_tick_history_and_wire_order_are_independent_of_direction() {
    let mut d = Decoder::default();
    d.explain_failures(true);
    open(&mut d, 1, S, true);
    let mut server = Rc4::new(&key_bytes(S).unwrap()).unwrap();
    push(&mut d, 1, S, frame(&mut server, 10, tick(0)));
    open(&mut d, 1, C, true); // opening the peer direction must retain the server tick
    open(&mut d, 2, S, true);
    let mut client = Rc4::new(&key_bytes(C).unwrap()).unwrap();
    let mut other = Rc4::new(&key_bytes(S).unwrap()).unwrap();
    push(&mut d, 1, C, frame(&mut client, 90, vec![]));
    push(&mut d, 2, S, frame(&mut other, 67, vec![24, 32, 0, 0]));
    let mut create = vec![0; 8];
    create.extend([0, 8]);
    create.extend(b"AAAAAAA=");
    let mut bytes = frame(&mut server, 67, vec![24, 32, 0, 0]);
    bytes.extend(frame(&mut server, 101, create));
    bytes.extend(frame(&mut server, 67, vec![24, 32, 0, 0]));
    push(&mut d, 1, S, bytes);
    let reports = d.take_failures();
    assert_eq!(reports.len(), 8);
    assert!(reports[0]
        .to_string()
        .starts_with("event=Opened fresh=true packetOrdinal=0"));
    assert_eq!(context(&reports[1]).last_newtick, Some(0));
    assert_eq!(context(&reports[2]).last_newtick, None);
    for (entry, ordinal, conn, dir, last) in [
        (&reports[3], 1, 1, C, Some(0)),
        (&reports[4], 2, 2, S, None),
        (&reports[5], 3, 1, S, Some(0)),
        (&reports[6], 4, 1, S, Some(0)),
        (&reports[7], 5, 1, S, Some(0)),
    ] {
        let context = context(entry);
        assert_eq!(context.packet_ordinal, ordinal);
        assert_eq!(context.conn, ConnId(conn));
        assert_eq!(context.dir, dir);
        assert_eq!(context.at, Timestamp(100));
        assert_eq!(context.last_newtick, last);
        assert_eq!(context.packets_since_recovered, None);
    }
    assert!(matches!(
        &reports[6],
        StructuralReport::CreateSuccess { .. }
    ));
    assert!(reports[6].to_string().contains("tailHex=00"));

    for dir in [S, C] {
        d.push(StreamEvent::Closed {
            conn: ConnId(1),
            dir,
            at: Timestamp(200),
            reason: CloseReason::Fin,
        });
    }
    let closed = d.take_failures();
    assert_eq!(closed.len(), 2);
    assert!(closed
        .iter()
        .all(|entry| context(entry).last_newtick == Some(0)));
    assert!(closed[0]
        .to_string()
        .starts_with("event=Closed packetOrdinal=6"));
    open(&mut d, 1, S, true);
    assert_eq!(context(&d.take_failures()[0]).last_newtick, None);
}

#[test]
fn recovery_window_counts_rejected_packets_and_clears_on_gap_and_close() {
    let mut d = Decoder::default();
    d.explain_failures(true);
    open(&mut d, 7, S, false);
    push(&mut d, 7, S, vec![0; 7]); // startup gate, not a complete framed packet
    let mut cipher = Rc4::new(&key_bytes(S).unwrap()).unwrap();
    cipher.skip(20);
    for n in [8, 9] {
        push(&mut d, 7, S, frame(&mut cipher, 10, tick(n)));
    }
    push(&mut d, 7, S, frame(&mut cipher, 67, vec![24, 32, 0, 0]));
    push(&mut d, 7, S, frame(&mut cipher, 10, tick(10)));
    // Alignment can read the sequence number but the field decoder rejects it.
    push(
        &mut d,
        7,
        S,
        frame(&mut cipher, 10, 11i32.to_be_bytes().to_vec()),
    );
    push(&mut d, 7, S, frame(&mut cipher, 67, vec![24, 32, 0, 0]));
    d.push(StreamEvent::Gap {
        conn: ConnId(7),
        dir: S,
        at: Timestamp(150),
        reason: GapReason::CaptureDrop,
    });
    let reports = d.take_failures();
    assert_eq!(reports.len(), 7);
    assert!(reports[0]
        .to_string()
        .starts_with("event=Opened fresh=false packetOrdinal=0"));
    assert!(reports[1]
        .to_string()
        .starts_with("event=Recovering packetOrdinal=0"));
    assert!(reports[2]
        .to_string()
        .starts_with("event=Recovered packetOrdinal=1"));
    assert_eq!(context(&reports[2]).packets_since_recovered, Some(0));
    assert_eq!(context(&reports[2]).last_newtick, None); // recovery tick is suppressed
    for (entry, ordinal, since, last) in [
        (&reports[3], 2, 1, None),
        (&reports[4], 4, 3, Some(10)),
        (&reports[5], 5, 4, Some(10)),
    ] {
        assert_eq!(context(entry).packet_ordinal, ordinal);
        assert_eq!(context(entry).packets_since_recovered, Some(since));
        assert_eq!(context(entry).last_newtick, last);
    }
    assert!(reports[6].to_string().contains("event=Recovering packetOrdinal=6 connId=7 dir=s2c timestampMicros=150 lastNewTickId=10 postRecovered=false packetsSinceRecovered=none"));

    push(&mut d, 7, S, frame(&mut cipher, 10, tick(12))); // gate
    for n in [13, 14] {
        push(&mut d, 7, S, frame(&mut cipher, 10, tick(n)));
    }
    d.push(StreamEvent::Closed {
        conn: ConnId(7),
        dir: S,
        at: Timestamp(200),
        reason: CloseReason::Fin,
    });
    let reports = d.take_failures();
    assert_eq!(reports.len(), 2);
    assert!(reports[0]
        .to_string()
        .starts_with("event=Recovered packetOrdinal=7"));
    assert_eq!(context(&reports[0]).packets_since_recovered, Some(0));
    assert!(reports[1]
        .to_string()
        .starts_with("event=Closed packetOrdinal=8"));
    assert_eq!(context(&reports[1]).packets_since_recovered, None);
}

#[test]
fn reports_remain_opt_in() {
    let mut d = Decoder::default();
    open(&mut d, 1, S, true);
    let mut cipher = Rc4::new(&key_bytes(S).unwrap()).unwrap();
    push(&mut d, 1, S, frame(&mut cipher, 67, vec![24, 32, 0, 0]));
    d.push(StreamEvent::Closed {
        conn: ConnId(1),
        dir: S,
        at: Timestamp(200),
        reason: CloseReason::Fin,
    });
    assert!(d.take_failures().is_empty());
}
