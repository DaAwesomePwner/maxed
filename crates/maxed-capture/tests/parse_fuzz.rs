//! Seeded structured fuzzing of `parse_frame`.
//!
//! The fully random case in `parse.rs` almost never passes the ethertype
//! check, so these cases fix a valid Ethernet prefix and randomize the IPv4
//! and TCP fields behind it. A spec-based model predicts the documented result
//! for every generated frame, and the final test checks that the generator
//! actually exercises the IPv4 and TCP branches.
use maxed_capture::{parse_frame, ParseError};
use maxed_types::{Direction, RawFrame, Timestamp};

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
    /// True with probability `percent` out of 100.
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

#[derive(Debug, Clone)]
struct Spec {
    tags: usize,
    version: u8,
    ihl_words: u8,
    total: u16,
    frag_field: u16,
    proto: u8,
    tcp_words: u8,
    src_port: u16,
    dst_port: u16,
    payload_len: usize,
    truncate_to: Option<usize>,
    short_original_len: bool,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Stage {
    Link,
    Ip,
    Tcp,
}

fn random_spec(rng: &mut Rng) -> Spec {
    let tags = match rng.below(20) {
        0..=9 => 0,
        10..=14 => 1,
        15..=18 => 2,
        _ => 3,
    };
    let ihl_words = if rng.chance(60) {
        5
    } else {
        rng.below(16) as u8
    };
    let tcp_words = if rng.chance(60) {
        5
    } else {
        rng.below(16) as u8
    };
    let payload_len = rng.below(48) as usize;
    let tcp_options = usize::from(tcp_words).saturating_sub(5) * 4;
    let honest_total = (usize::from(ihl_words.max(5)) * 4 + 20 + tcp_options + payload_len) as u16;
    let total = match rng.below(10) {
        0..=6 => honest_total,
        7 => honest_total.wrapping_add(rng.below(8) as u16 + 1),
        8 => honest_total.saturating_sub(rng.below(8) as u16 + 1),
        _ => rng.next() as u16,
    };
    let frag_field = if rng.chance(85) {
        // Only the DF and reserved bits, which the parser must ignore.
        (rng.below(3) as u16) << 14
    } else {
        rng.next() as u16
    };
    let proto = if rng.chance(85) { 6 } else { rng.next() as u8 };
    let port = |rng: &mut Rng| -> u16 {
        if rng.chance(10) {
            2050
        } else {
            rng.next() as u16
        }
    };
    let (src_port, dst_port) = match rng.below(10) {
        0..=4 => (port(rng), 2050),
        5..=8 => (2050, port(rng)),
        _ => (port(rng), port(rng)),
    };
    Spec {
        tags,
        version: if rng.chance(90) {
            4
        } else {
            rng.below(16) as u8
        },
        ihl_words,
        total,
        frag_field,
        proto,
        tcp_words,
        src_port,
        dst_port,
        payload_len,
        truncate_to: None,
        short_original_len: rng.chance(5),
    }
}

fn build(rng: &mut Rng, spec: &mut Spec, index: u64) -> RawFrame {
    let mut bytes = Vec::new();
    for _ in 0..12 {
        bytes.push(rng.next() as u8);
    }
    let tag_types = [0x8100u16, 0x88a8];
    let mut kinds = Vec::new();
    for _ in 0..spec.tags {
        kinds.push(tag_types[rng.below(2) as usize]);
    }
    // The chain always ends in IPv4; three tags exceed the two the parser walks.
    kinds.push(0x0800);
    bytes.extend(kinds[0].to_be_bytes());
    for tag in 0..spec.tags {
        bytes.extend(rng.next().to_be_bytes()[..2].iter());
        bytes.extend(kinds[tag + 1].to_be_bytes());
    }
    bytes.push(spec.version << 4 | spec.ihl_words);
    bytes.push(rng.next() as u8);
    bytes.extend(spec.total.to_be_bytes());
    bytes.extend((rng.next() as u16).to_be_bytes());
    bytes.extend(spec.frag_field.to_be_bytes());
    bytes.push(64);
    bytes.push(spec.proto);
    bytes.extend((rng.next() as u16).to_be_bytes());
    bytes.extend([10, 0, 0, 1, 10, 0, 0, 2]);
    for _ in 0..usize::from(spec.ihl_words).saturating_sub(5) * 4 {
        bytes.push(rng.next() as u8);
    }
    bytes.extend(spec.src_port.to_be_bytes());
    bytes.extend(spec.dst_port.to_be_bytes());
    bytes.extend((rng.next() as u32).to_be_bytes());
    bytes.extend((rng.next() as u32).to_be_bytes());
    bytes.push(spec.tcp_words << 4);
    bytes.push(rng.next() as u8);
    bytes.extend((rng.next() as u16).to_be_bytes());
    bytes.extend((rng.next() as u16).to_be_bytes());
    bytes.extend((rng.next() as u16).to_be_bytes());
    for _ in 0..usize::from(spec.tcp_words).saturating_sub(5) * 4 {
        bytes.push(rng.next() as u8);
    }
    for _ in 0..spec.payload_len {
        bytes.push(rng.next() as u8);
    }
    if rng.chance(20) {
        let cut = rng.below(bytes.len() as u64 + 1) as usize;
        bytes.truncate(cut);
        spec.truncate_to = Some(cut);
    }
    let original_len = if spec.short_original_len && !bytes.is_empty() {
        bytes.len() as u32 - 1
    } else {
        bytes.len() as u32 + rng.below(3) as u32
    };
    RawFrame {
        index,
        at: Timestamp(index),
        original_len,
        bytes,
    }
}

/// Independent prediction from the generator's own fields and the final length.
fn model(spec: &Spec, frame: &RawFrame) -> (Result<Option<(Direction, usize)>, ParseError>, Stage) {
    let len = frame.bytes.len();
    if len > frame.original_len as usize {
        return (Err(ParseError::InvalidHeader), Stage::Link);
    }
    if spec.tags == 3 {
        let error = if len < 22 {
            ParseError::Truncated
        } else {
            ParseError::UnsupportedLink
        };
        return (Err(error), Stage::Link);
    }
    let ip = 14 + spec.tags * 4;
    if len < ip + 20 {
        return (Err(ParseError::Truncated), Stage::Link);
    }
    let ihl = usize::from(spec.ihl_words) * 4;
    if spec.version != 4 || ihl < 20 {
        return (Err(ParseError::InvalidHeader), Stage::Ip);
    }
    let total = usize::from(spec.total);
    if len < ip + ihl {
        return (Err(ParseError::Truncated), Stage::Ip);
    }
    if total < ihl {
        return (Err(ParseError::InvalidHeader), Stage::Ip);
    }
    if len < ip + total {
        return (Err(ParseError::Truncated), Stage::Ip);
    }
    if spec.proto != 6 {
        return (Ok(None), Stage::Ip);
    }
    if spec.frag_field & 0x3fff != 0 {
        return (Err(ParseError::Fragmented), Stage::Ip);
    }
    if total < ihl + 20 {
        return (Err(ParseError::InvalidHeader), Stage::Ip);
    }
    let tcp_len = usize::from(spec.tcp_words) * 4;
    if tcp_len < 20 || ihl + tcp_len > total {
        return (Err(ParseError::InvalidHeader), Stage::Tcp);
    }
    let dir = match (spec.src_port == 2050, spec.dst_port == 2050) {
        (true, true) => return (Err(ParseError::InvalidHeader), Stage::Tcp),
        (false, false) => return (Ok(None), Stage::Tcp),
        (false, true) => Direction::ClientToServer,
        (true, false) => Direction::ServerToClient,
    };
    (Ok(Some((dir, total - ihl - tcp_len))), Stage::Tcp)
}

fn check_invariants(raw: &RawFrame, context: &str) {
    match parse_frame(raw) {
        Ok(Some(segment)) => {
            let origin = segment.origin;
            assert_eq!(origin.frame_index, raw.index, "{context}");
            assert_eq!(origin.len, segment.bytes.len(), "{context}");
            let end = origin.offset + origin.len;
            assert!(end <= raw.bytes.len(), "{context}: origin past frame");
            assert_eq!(segment.bytes, raw.bytes[origin.offset..end], "{context}");
            let flow = segment.flow;
            assert_eq!(flow.server.port, 2050, "{context}");
            assert_ne!(flow.client.port, 2050, "{context}");
        }
        Ok(None) => {}
        Err(error) => {
            // Attribution recovery is also reached from errors; it must not panic.
            let _ = error.affected_flow(raw);
        }
    }
}

#[test]
fn structured_frames_match_the_documented_outcome() {
    const SEED: u64 = 0x9e37_79b9_7f4a_7c15;
    const FRAMES: u64 = 20_000;
    let mut rng = Rng::new(SEED);
    let mut link = 0u64;
    let mut ip = 0u64;
    let mut tcp = 0u64;
    let mut segments = 0u64;
    let mut kinds = std::collections::BTreeMap::<&'static str, u64>::new();
    for index in 0..FRAMES {
        let mut spec = random_spec(&mut rng);
        let raw = build(&mut rng, &mut spec, index);
        let context = format!("seed={SEED:#x} index={index} spec={spec:?}");
        let (expected, stage) = model(&spec, &raw);
        let actual = parse_frame(&raw);
        match (&expected, &actual) {
            (Ok(Some((dir, payload))), Ok(Some(segment))) => {
                assert_eq!(segment.dir, *dir, "{context}");
                assert_eq!(segment.bytes.len(), *payload, "{context}");
                segments += 1;
            }
            (Ok(None), Ok(None)) => {}
            (Err(want), Err(got)) => assert_eq!(got, want, "{context}"),
            _ => panic!(
                "{context}: expected {expected:?}, got {:?}",
                actual.as_ref().map(|segment| segment.is_some())
            ),
        }
        check_invariants(&raw, &context);
        match stage {
            Stage::Link => link += 1,
            Stage::Ip => ip += 1,
            Stage::Tcp => tcp += 1,
        }
        let kind = match actual {
            Ok(Some(_)) => "segment",
            Ok(None) => "skipped",
            Err(ParseError::Truncated) => "truncated",
            Err(ParseError::InvalidHeader) => "invalid-header",
            Err(ParseError::Fragmented) => "fragmented",
            Err(ParseError::UnsupportedLink) => "unsupported-link",
        };
        *kinds.entry(kind).or_default() += 1;
    }
    println!("seed={SEED:#x} stages: link={link} ip={ip} tcp={tcp} outcomes={kinds:?}");
    // Most inputs must get past the Ethernet checks; the old random test
    // reached the IPv4 parser for under 1% of inputs.
    assert!(
        (ip + tcp) * 10 >= FRAMES * 7,
        "seed={SEED:#x} ip+tcp too low: {kinds:?}"
    );
    assert!(
        tcp * 10 >= FRAMES * 3,
        "seed={SEED:#x} tcp too low: {kinds:?}"
    );
    assert!(
        segments * 10 >= FRAMES,
        "seed={SEED:#x} too few segments: {kinds:?}"
    );
    for kind in [
        "segment",
        "skipped",
        "truncated",
        "invalid-header",
        "fragmented",
        "unsupported-link",
    ] {
        assert!(kinds.contains_key(kind), "seed={SEED:#x} never saw {kind}");
    }
}

#[test]
fn mutated_valid_frames_never_panic() {
    const SEED: u64 = 0x0bad_cafe_f00d_1234;
    let mut rng = Rng::new(SEED);
    let mut parsed = 0u64;
    for index in 0..20_000u64 {
        // A consistent two-tag-at-most frame, then 0..4 random byte edits.
        let mut spec = Spec {
            tags: rng.below(3) as usize,
            version: 4,
            ihl_words: 5 + rng.below(3) as u8,
            total: 0,
            frag_field: 0,
            proto: 6,
            tcp_words: 5 + rng.below(3) as u8,
            src_port: 50000,
            dst_port: 2050,
            payload_len: rng.below(24) as usize,
            truncate_to: None,
            short_original_len: false,
        };
        spec.total = (usize::from(spec.ihl_words) * 4
            + usize::from(spec.tcp_words) * 4
            + spec.payload_len) as u16;
        let mut raw = build(&mut rng, &mut spec, index);
        for _ in 0..rng.below(5) {
            if raw.bytes.is_empty() {
                break;
            }
            let at = rng.below(raw.bytes.len() as u64) as usize;
            // Bias edits to the header region where the index arithmetic lives.
            let at = if rng.chance(70) {
                at.min(70) % raw.bytes.len()
            } else {
                at
            };
            raw.bytes[at] = rng.next() as u8;
        }
        let context = format!("seed={SEED:#x} index={index} spec={spec:?}");
        check_invariants(&raw, &context);
        if matches!(parse_frame(&raw), Ok(Some(_))) {
            parsed += 1;
        }
    }
    assert!(parsed > 2_000, "seed={SEED:#x} only {parsed} frames parsed");
}

#[test]
fn every_prefix_of_structured_frames_never_panics() {
    const SEED: u64 = 0x5eed_0000_0000_0042;
    let mut rng = Rng::new(SEED);
    for index in 0..300u64 {
        let mut spec = random_spec(&mut rng);
        spec.truncate_to = None;
        let raw = build(&mut rng, &mut spec, index);
        for size in 0..=raw.bytes.len() {
            let mut cut = raw.clone();
            cut.bytes.truncate(size);
            cut.original_len = raw.original_len.max(size as u32);
            let context = format!("seed={SEED:#x} index={index} size={size} spec={spec:?}");
            check_invariants(&cut, &context);
        }
    }
}
