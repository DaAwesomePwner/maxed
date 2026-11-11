//! Seeded property tests for `Reassembler` delivery under reordering,
//! duplication and overlap of a known byte stream.
//!
//! Each segment's `ByteOrigin::frame_index` holds its first byte's absolute
//! stream offset (with `offset == 0`), so every delivered chunk reports where
//! in the known stream it came from and can be checked against that stream.
use maxed_stream::Reassembler;
use maxed_types::*;

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
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

fn flow() -> FlowKey {
    FlowKey {
        client: Endpoint {
            ip: [127, 0, 0, 1],
            port: 50000,
        },
        server: Endpoint {
            ip: [127, 0, 0, 2],
            port: 2050,
        },
    }
}

fn segment(isn: u32, stream: &[u8], from: usize, to: usize) -> TcpSegment {
    TcpSegment {
        flow: flow(),
        dir: Direction::ClientToServer,
        at: Timestamp(1),
        // Stream byte 0 sits at sequence isn + 1, after the SYN.
        seq: isn.wrapping_add(1).wrapping_add(from as u32),
        syn: false,
        fin: false,
        rst: false,
        bytes: stream[from..to].to_vec(),
        origin: ByteOrigin {
            frame_index: from as u64,
            offset: 0,
            len: to - from,
        },
    }
}

fn syn(isn: u32) -> TcpSegment {
    TcpSegment {
        seq: isn,
        syn: true,
        bytes: Vec::new(),
        origin: ByteOrigin {
            frame_index: 0,
            offset: 0,
            len: 0,
        },
        ..segment(isn, &[], 0, 0)
    }
}

fn fin(isn: u32, stream: &[u8]) -> TcpSegment {
    TcpSegment {
        fin: true,
        ..segment(isn, stream, stream.len(), stream.len())
    }
}

fn random_isn(rng: &mut Rng) -> u32 {
    match rng.below(4) {
        0 => u32::MAX - rng.below(64) as u32,
        1 => 0,
        _ => rng.next() as u32,
    }
}

/// The base chunks plus extra segments that retransmit or overlap them, all
/// carrying true stream bytes.
fn plan(
    rng: &mut Rng,
    stream: &[u8],
    chunk_sizes: (usize, usize),
    extras: usize,
) -> Vec<(usize, usize)> {
    let (min_chunk, max_chunk) = chunk_sizes;
    let mut spans = Vec::new();
    let mut at = 0;
    while at < stream.len() {
        let end = (at + min_chunk + rng.below(max_chunk - min_chunk + 1)).min(stream.len());
        spans.push((at, end));
        at = end;
    }
    for _ in 0..extras {
        let (a, b) = spans[rng.below(spans.len())];
        match rng.below(3) {
            // Exact retransmission.
            0 => spans.push((a, b)),
            // Overlap one end of the chunk, possibly spanning neighbours.
            1 => {
                let start = a.saturating_sub(rng.below(max_chunk + 1));
                spans.push((start, b));
            }
            _ => {
                let end = (b + rng.below(max_chunk + 1)).min(stream.len());
                spans.push((a, end));
            }
        }
    }
    spans
}

struct Delivery {
    /// (absolute start, length) of every Data chunk, in event order.
    chunks: Vec<(usize, usize)>,
    gaps: usize,
    closes: usize,
    bytes: Vec<u8>,
}

/// Feed events to `stream`, checking every delivered chunk against `expect`.
fn run(
    stream: &mut Reassembler,
    segments: Vec<TcpSegment>,
    expect: &[u8],
    context: &str,
) -> Delivery {
    let mut delivery = Delivery {
        chunks: Vec::new(),
        gaps: 0,
        closes: 0,
        bytes: Vec::new(),
    };
    // End of the last delivered chunk, and whether a Gap has occurred since.
    let mut delivered_to = 0usize;
    let mut gap_since_chunk = false;
    for (step, segment) in segments.into_iter().enumerate() {
        for event in stream.push(segment) {
            match event {
                StreamEvent::Data(chunk) => {
                    assert_eq!(chunk.origins.len(), 1, "{context} step={step}");
                    let origin = &chunk.origins[0];
                    assert_eq!(origin.len, chunk.bytes.len(), "{context} step={step}");
                    let start = origin.frame_index as usize + origin.offset;
                    let end = start + chunk.bytes.len();
                    assert!(
                        !chunk.bytes.is_empty(),
                        "{context} step={step}: empty chunk"
                    );
                    assert!(end <= expect.len(), "{context} step={step}: past end");
                    assert_eq!(
                        chunk.bytes,
                        expect[start..end],
                        "{context} step={step}: corrupted bytes at {start}"
                    );
                    if gap_since_chunk {
                        assert!(
                            start >= delivered_to,
                            "{context} step={step}: reordered or duplicated bytes after a gap"
                        );
                    } else {
                        assert_eq!(
                            start, delivered_to,
                            "{context} step={step}: silent skip, reorder or duplicate"
                        );
                    }
                    delivered_to = end;
                    gap_since_chunk = false;
                    delivery.chunks.push((start, chunk.bytes.len()));
                    delivery.bytes.extend(chunk.bytes);
                }
                StreamEvent::Gap { reason, .. } => {
                    assert_eq!(
                        reason,
                        GapReason::ReorderLimit,
                        "{context} step={step}: consistent bytes must not conflict"
                    );
                    delivery.gaps += 1;
                    gap_since_chunk = true;
                }
                StreamEvent::Closed { reason, .. } => {
                    assert_eq!(reason, CloseReason::Fin, "{context} step={step}");
                    delivery.closes += 1;
                }
                StreamEvent::Opened { .. } => {}
            }
        }
    }
    delivery
}

fn random_stream(rng: &mut Rng, len: usize) -> Vec<u8> {
    (0..len).map(|_| rng.next() as u8).collect()
}

/// Fewer than 32 segments can never pend at once, so no forced gap occurs and
/// the whole stream must come out exactly once, in order, followed by one FIN.
#[test]
fn bounded_disorder_delivers_the_stream_exactly_once_in_order() {
    const SEED: u64 = 0x1357_9bdf_2468_ace0;
    let mut rng = Rng::new(SEED);
    for case in 0..2_000 {
        let isn = random_isn(&mut rng);
        let len = 1 + rng.below(300);
        let stream = random_stream(&mut rng, len);
        // At most 20 base chunks plus 8 extras, a FIN and no SYN pending: always
        // below the 32-segment forced-drain threshold.
        let min_chunk = len.div_ceil(20);
        let max_chunk = min_chunk + rng.below(40);
        let extras = rng.below(9);
        let spans = plan(&mut rng, &stream, (min_chunk, max_chunk), extras);
        let context = format!("seed={SEED:#x} case={case} isn={isn:#x} len={len} spans={spans:?}");
        let mut order: Vec<TcpSegment> = spans
            .iter()
            .map(|&(a, b)| segment(isn, &stream, a, b))
            .collect();
        order.push(fin(isn, &stream));
        rng.shuffle(&mut order);
        let mut segments = vec![syn(isn)];
        segments.extend(order);
        let mut reassembler = Reassembler::default();
        let delivery = run(&mut reassembler, segments, &stream, &context);
        assert_eq!(delivery.gaps, 0, "{context}: unexpected gap");
        assert_eq!(delivery.bytes, stream, "{context}: stream not delivered");
        assert_eq!(delivery.closes, 1, "{context}: FIN not delivered once");
    }
}

/// Heavy disorder may force gaps, but never corruption, duplication or a
/// silent skip.
#[test]
fn heavy_disorder_only_skips_through_explicit_gaps() {
    const SEED: u64 = 0xdead_beef_0bad_f00d;
    let mut rng = Rng::new(SEED);
    let mut gapped = 0;
    let mut clean = 0;
    for case in 0..1_000 {
        let isn = random_isn(&mut rng);
        let len = 200 + rng.below(1_800);
        let stream = random_stream(&mut rng, len);
        let max_chunk = 1 + rng.below(60);
        let extras = rng.below(120);
        let spans = plan(&mut rng, &stream, (1, max_chunk), extras);
        let context = format!(
            "seed={SEED:#x} case={case} isn={isn:#x} len={len} max_chunk={max_chunk} segments={}",
            spans.len()
        );
        let mut order: Vec<TcpSegment> = spans
            .iter()
            .map(|&(a, b)| segment(isn, &stream, a, b))
            .collect();
        rng.shuffle(&mut order);
        let mut segments = vec![syn(isn)];
        segments.extend(order);
        let mut reassembler = Reassembler::default();
        let delivery = run(&mut reassembler, segments, &stream, &context);
        if delivery.gaps == 0 {
            clean += 1;
            assert_eq!(delivery.bytes, stream, "{context}: gap-free run lost bytes");
        } else {
            gapped += 1;
        }
    }
    // Both regimes must actually be exercised.
    assert!(gapped > 100, "seed={SEED:#x} too few gapped runs: {gapped}");
    assert!(clean > 20, "seed={SEED:#x} too few clean runs: {clean}");
}

/// Mostly in-order delivery with local swaps, retransmissions and overlaps,
/// the shape of real captures, over many segments.
#[test]
fn local_swaps_and_retransmissions_deliver_everything() {
    const SEED: u64 = 0x0123_4567_89ab_cdef;
    let mut rng = Rng::new(SEED);
    for case in 0..300 {
        let isn = random_isn(&mut rng);
        let len = 500 + rng.below(4_000);
        let stream = random_stream(&mut rng, len);
        let max_chunk = 1 + rng.below(80);
        let spans = plan(&mut rng, &stream, (1, max_chunk), 0);
        let mut order: Vec<TcpSegment> = Vec::new();
        for &(a, b) in &spans {
            order.push(segment(isn, &stream, a, b));
            if rng.below(5) == 0 {
                order.push(segment(isn, &stream, a, b));
            }
            if rng.below(7) == 0 {
                let start = a.saturating_sub(rng.below(30));
                order.push(segment(isn, &stream, start, b));
            }
        }
        // Swap neighbours up to three places away: at most a few pending.
        for i in 0..order.len() {
            if rng.below(4) == 0 {
                let j = (i + 1 + rng.below(3)).min(order.len() - 1);
                order.swap(i, j);
            }
        }
        order.push(fin(isn, &stream));
        let context = format!("seed={SEED:#x} case={case} isn={isn:#x} len={len}");
        let mut segments = vec![syn(isn)];
        segments.extend(order);
        let mut reassembler = Reassembler::default();
        let delivery = run(&mut reassembler, segments, &stream, &context);
        assert_eq!(delivery.gaps, 0, "{context}: unexpected gap");
        assert_eq!(delivery.bytes, stream, "{context}: stream not delivered");
        assert_eq!(delivery.closes, 1, "{context}");
    }
}
