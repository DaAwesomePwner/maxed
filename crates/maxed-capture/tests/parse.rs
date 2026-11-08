use maxed_capture::parse_frame;
use maxed_types::{RawFrame, Timestamp};

#[test]
fn every_short_ethernet_prefix_is_rejected() {
    for size in 0..14 {
        assert!(parse_frame(&RawFrame {
            index: 0,
            at: Timestamp(1),
            original_len: 54,
            bytes: vec![0; size]
        })
        .is_err());
    }
}

#[test]
fn arbitrary_frames_never_panic() {
    let mut seed = 0x1234_5678u32;
    for size in 0..512 {
        let bytes = (0..size)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let _ = parse_frame(&RawFrame {
            index: 0,
            at: Timestamp(0),
            original_len: size as u32,
            bytes,
        });
    }
}

fn frame(payload: &[u8], ip_options: usize, tcp_options: usize, tags: usize) -> RawFrame {
    let ip = 14 + tags * 4;
    let tcp = ip + 20 + ip_options;
    let data = tcp + 20 + tcp_options;
    let mut bytes = vec![0; data + payload.len()];
    bytes[12..14].copy_from_slice(&(if tags == 0 { 0x0800u16 } else { 0x88a8 }).to_be_bytes());
    for tag in 0..tags {
        bytes[16 + tag * 4..18 + tag * 4]
            .copy_from_slice(&(if tag + 1 == tags { 0x0800u16 } else { 0x8100 }).to_be_bytes());
    }
    bytes[ip] = 0x45 + (ip_options / 4) as u8;
    bytes[ip + 2..ip + 4].copy_from_slice(&((data + payload.len() - ip) as u16).to_be_bytes());
    bytes[ip + 9] = 6;
    bytes[ip + 12..ip + 16].copy_from_slice(&[127, 0, 0, 1]);
    bytes[ip + 16..ip + 20].copy_from_slice(&[127, 0, 0, 2]);
    bytes[tcp..tcp + 2].copy_from_slice(&50000u16.to_be_bytes());
    bytes[tcp + 2..tcp + 4].copy_from_slice(&2050u16.to_be_bytes());
    bytes[tcp + 4..tcp + 8].copy_from_slice(&u32::MAX.to_be_bytes());
    bytes[tcp + 12] = ((20 + tcp_options) / 4 * 16) as u8;
    bytes[tcp + 13] = 7;
    bytes[data..].copy_from_slice(payload);
    RawFrame {
        index: 7,
        at: Timestamp(42),
        original_len: bytes.len() as u32,
        bytes,
    }
}

#[test]
fn syn_options_vlan_padding_and_origins() {
    use maxed_types::Direction;
    for (ip, tcp, tags) in [(0, 0, 0), (4, 8, 1), (40, 40, 2)] {
        let mut raw = frame(b"abc", ip, tcp, tags);
        let expected = raw.bytes.len() - 3;
        raw.bytes.extend_from_slice(&[0; 12]);
        raw.original_len = raw.bytes.len() as u32;
        let parsed = parse_frame(&raw).unwrap().unwrap();
        assert_eq!(parsed.bytes, b"abc");
        assert_eq!(parsed.origin.offset, expected);
        assert_eq!(parsed.origin.frame_index, 7);
        assert_eq!(parsed.origin.len, 3);
        assert_eq!(parsed.seq, u32::MAX);
        assert!(parsed.syn && parsed.fin && parsed.rst);
        assert_eq!(parsed.dir, Direction::ClientToServer);
    }
    assert_eq!(frame(b"", 0, 0, 0).bytes.len(), 54);
    let mut raw = frame(b"", 0, 0, 0);
    raw.bytes[34..38].copy_from_slice(&[8, 2, 0xc3, 0x50]);
    assert_eq!(
        parse_frame(&raw).unwrap().unwrap().dir,
        Direction::ServerToClient
    );
    raw.bytes[36..38].copy_from_slice(&[8, 2]);
    assert!(parse_frame(&raw).is_err());
}

#[test]
fn rejects_each_truncation_and_invalid_header() {
    use maxed_capture::ParseError;
    let valid = frame(b"synthetic", 4, 4, 2);
    for size in 0..valid.bytes.len() {
        let mut raw = valid.clone();
        raw.bytes.truncate(size);
        assert!(parse_frame(&raw).is_err(), "size={size}");
    }
    for (offset, value, error) in [
        (14, 0x44, ParseError::InvalidHeader),
        (16, 0, ParseError::InvalidHeader),
        (20, 0x20, ParseError::Fragmented),
        (46, 0x40, ParseError::InvalidHeader),
        (46, 0xf0, ParseError::InvalidHeader),
    ] {
        let mut raw = frame(b"", 0, 0, 0);
        if offset == 16 {
            raw.bytes[17] = 39;
        }
        raw.bytes[offset] = value;
        assert_eq!(parse_frame(&raw).err(), Some(error));
    }
    let mut raw = frame(b"", 0, 0, 0);
    raw.bytes[12..14].copy_from_slice(&0x86ddu16.to_be_bytes());
    assert_eq!(parse_frame(&raw).err(), Some(ParseError::UnsupportedLink));
    raw.bytes[12..14].copy_from_slice(&0x0806u16.to_be_bytes());
    assert!(parse_frame(&raw).unwrap().is_none());
}

#[test]
fn malformed_frames_retain_only_trustworthy_directional_attribution() {
    for tags in 0..=2 {
        let valid = frame(b"body", 4, 0, tags);
        let segment = parse_frame(&valid).unwrap().unwrap();
        let ip = 14 + tags * 4;
        let tcp = ip + 24;
        for failure in 0..3 {
            let mut raw = valid.clone();
            match failure {
                0 => {
                    raw.bytes.pop();
                }
                1 => raw.bytes[ip + 6] = 0x20,
                _ => raw.bytes[tcp + 12] = 0x40,
            }
            let error = parse_frame(&raw).err().unwrap();
            assert_eq!(error.affected_flow(&raw), Some((segment.flow, segment.dir)));
        }
        let mut noninitial = valid.clone();
        noninitial.bytes[ip + 7] = 1;
        assert_eq!(
            parse_frame(&noninitial)
                .err()
                .unwrap()
                .affected_flow(&noninitial),
            None
        );
        let mut short = valid;
        short.bytes.truncate(tcp + 3);
        assert_eq!(
            parse_frame(&short).err().unwrap().affected_flow(&short),
            None
        );
    }
}
