use maxed_protocol::{
    canonical::{lower_hex, six_significant},
    canonical_line, decode_packet, write_jsonl, DecodedPacket, ProtocolError,
};
use maxed_types::{ConnId, Direction, Timestamp};
use serde_json::json;
#[test]
fn canonical_escape_shape() {
    let (packet, _) = decode_packet(105, Direction::ClientToServer, &[]).unwrap();
    let mut p = DecodedPacket {
        conn: ConnId(1),
        dir: Direction::ClientToServer,
        at: Timestamp(1_000_000),
        id: 105,
        len: 5,
        alignment_verified: false,
        packet,
    };
    let line = canonical_line(0, &p).unwrap();
    assert_eq!(
        serde_json::to_value(&line).unwrap(),
        json!({"i":0,"t":1000000,"dir":"c2s","id":105,"name":"ESCAPE","f":{}})
    );
    let mut bytes = vec![];
    write_jsonl(&mut bytes, &line).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert!(!bytes.starts_with(&[0xef, 0xbb, 0xbf]));
    assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
    p.alignment_verified = true;
    let mut verified = vec![];
    write_jsonl(&mut verified, &canonical_line(0, &p).unwrap()).unwrap();
    assert_eq!(bytes, verified);
}
#[test]
fn rounding_exact_wire_floats_ties_and_nonfinite() {
    assert_eq!(six_significant(123456.5).unwrap(), 123456.0);
    assert_eq!(six_significant(123457.5).unwrap(), 123458.0);
    assert_eq!(six_significant(1.2345678).unwrap(), 1.23457);
    assert_eq!(six_significant(-0.0).unwrap().to_bits(), 0f64.to_bits());
    assert!(six_significant(f32::from_bits(1)).unwrap() > 0.0);
    assert!(six_significant(f32::MAX).unwrap().is_finite());
    for n in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(six_significant(n), Err(ProtocolError::NonFinite));
        let mut b = 0i32.to_be_bytes().to_vec();
        b.extend(n.to_be_bytes());
        b.extend(0f32.to_be_bytes());
        assert_eq!(
            decode_packet(103, Direction::ClientToServer, &b)
                .unwrap()
                .0
                .fields(),
            Err(ProtocolError::NonFinite)
        );
    }
}
#[test]
fn nested_floats_integers_and_unknown_names() {
    let mut b = i32::MAX.to_be_bytes().to_vec();
    b.extend(1.2345678f32.to_be_bytes());
    b.extend((-0.0f32).to_be_bytes());
    assert_eq!(
        decode_packet(103, Direction::ClientToServer, &b)
            .unwrap()
            .0
            .fields()
            .unwrap(),
        json!({"time":2147483647,"position":{"x":1.23457,"y":0.0}})
    );
    assert_eq!(
        decode_packet(62, Direction::ClientToServer, &[0; 4])
            .unwrap()
            .0
            .name(),
        "MOVE"
    );
    assert_eq!(
        decode_packet(250, Direction::ServerToClient, &[0; 4])
            .unwrap()
            .0
            .fields()
            .unwrap(),
        json!({"len":9})
    );
    assert_eq!(lower_hex(&[0, 15, 16, 128, 255]), "000f1080ff");
    let (packet, _) = decode_packet(105, Direction::ClientToServer, &[]).unwrap();
    let p = DecodedPacket {
        conn: ConnId(1),
        dir: Direction::ClientToServer,
        at: Timestamp(u64::MAX),
        id: 105,
        len: 5,
        alignment_verified: true,
        packet,
    };
    assert_eq!(
        serde_json::to_value(canonical_line(u64::MAX, &p).unwrap()).unwrap()["t"],
        json!(u64::MAX)
    );
}
