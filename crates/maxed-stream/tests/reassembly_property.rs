//! Seeded property tests for `Reassembler` delivery under reordering,
//! duplication and overlap of a known byte stream.
//!
//! Each segment's `ByteOrigin::frame_index` holds its first byte's absolute
//! stream offset (with `offset == 0`), so every delivered chunk reports where
//! in the known stream it came from and can be checked against that stream.
//!
//! Not covered: hole aging. Every segment carries the same capture timestamp,
//! so `HOLE_AGE_MICROS` expiry never fires; only the pending-segment limit
//! forces gaps here. Driving aging would need controlled timestamps and is
//! left to the example-based tests in `reassembly.rs`.
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
    gaps: usize,
    conflicts: usize,
    closes: usize,
    bytes: Vec<u8>,
}

/// Where the next delivered chunk must start.
#[derive(Clone, Copy)]
enum Resume {
    /// Exactly where the previous chunk ended: no silent skip or repeat.
    Contiguous,
    /// After a forced gap: at this absolute offset.
    At(usize),
    /// After a conflict reset: anywhere at or after the delivered end.
    Forward,
}

/// Feed `segments` to `stream`, checking every event against `expect`.
///
/// After a `ReorderLimit` gap the reassembler jumps to the nearest buffered
/// segment start (`drain` picks the minimum start beyond the cursor), so the
/// next chunk must start exactly at the smallest start among the segments sent
/// so far that still have undelivered bytes ahead of the cursor; skipping
/// further would drop buffered bytes. `StreamEvent::Gap` carries no skipped
/// length, so the amount skipped is only checked through that next chunk. The
/// prediction assumes the cursor-invalidating paths (a jump over `MAX_BYTES`,
/// oversized segments) are unreachable, which holds for streams of a few KB.
/// A gap not followed by data in the run is not checked.
fn run(
    stream: &mut Reassembler,
    segments: Vec<TcpSegment>,
    expect: &[u8],
    allow_conflict: bool,
    context: &str,
) -> Delivery {
    let mut delivery = Delivery {
        gaps: 0,
        conflicts: 0,
        closes: 0,
        bytes: Vec::new(),
    };
    let mut delivered_to = 0usize;
    let mut resume = Resume::Contiguous;
    let mut closed = false;
    // (start, end, fin) of every sent segment that could be buffered.
    let mut sent: Vec<(usize, usize, bool)> = Vec::new();
    for (step, segment) in segments.into_iter().enumerate() {
        let start = segment.origin.frame_index as usize;
        let end = start + segment.bytes.len();
        let plain_empty = segment.bytes.is_empty() && !segment.syn && !segment.fin && !segment.rst;
        if !segment.syn && (end > start || segment.fin) {
            sent.push((start, end, segment.fin));
        }
        let events = stream.push(segment);
        if plain_empty {
            assert!(
                events.is_empty(),
                "{context} step={step}: empty segment produced events"
            );
        }
        for event in events {
            match event {
                StreamEvent::Data(chunk) => {
                    assert!(!closed, "{context} step={step}: data after FIN close");
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
                    match resume {
                        Resume::Contiguous => assert_eq!(
                            start, delivered_to,
                            "{context} step={step}: silent skip, reorder or duplicate"
                        ),
                        Resume::At(offset) => assert_eq!(
                            start, offset,
                            "{context} step={step}: resumed away from the nearest buffered segment"
                        ),
                        Resume::Forward => assert!(
                            start >= delivered_to,
                            "{context} step={step}: bytes repeated after a conflict reset"
                        ),
                    }
                    delivered_to = end;
                    resume = Resume::Contiguous;
                    delivery.bytes.extend(chunk.bytes);
                }
                StreamEvent::Gap { reason, .. } => match reason {
                    GapReason::ReorderLimit => {
                        delivery.gaps += 1;
                        let nearest = sent
                            .iter()
                            .filter(|&&(from, to, fin)| {
                                from >= delivered_to && (to > from || (fin && from > delivered_to))
                            })
                            .map(|&(from, ..)| from)
                            .min()
                            .unwrap_or_else(|| panic!("{context} step={step}: gap with no buffer"));
                        resume = Resume::At(nearest);
                    }
                    GapReason::ConflictingOverlap if allow_conflict => {
                        delivery.conflicts += 1;
                        resume = Resume::Forward;
                    }
                    other => panic!("{context} step={step}: unexpected gap {other:?}"),
                },
                StreamEvent::Closed { reason, .. } => {
                    assert_eq!(reason, CloseReason::Fin, "{context} step={step}");
                    assert!(!closed, "{context} step={step}: closed twice");
                    closed = true;
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

/// An empty segment without SYN/FIN/RST at an arbitrary stream position.
fn empty(rng: &mut Rng, isn: u32, stream: &[u8]) -> TcpSegment {
    let at = rng.below(stream.len() + 1);
    segment(isn, stream, at, at)
}

/// Insert `count` empty segments at random positions, possibly before the SYN.
fn sprinkle_empties(
    rng: &mut Rng,
    segments: &mut Vec<TcpSegment>,
    isn: u32,
    stream: &[u8],
    count: usize,
) {
    for _ in 0..count {
        let at = rng.below(segments.len() + 1);
        segments.insert(at, empty(rng, isn, stream));
    }
}

/// Shuffle inside consecutive windows of `window` items: nothing moves more
/// than `window - 1` places, and at most `window - 1` segments can be buffered
/// ahead of a hole because each window must complete before the next starts.
fn shuffle_windows<T>(rng: &mut Rng, items: &mut [T], window: usize) {
    for block in items.chunks_mut(window) {
        rng.shuffle(block);
    }
}

/// The reassembler forces a gap once its hole limit (`HOLE_SEGMENTS`, 32 at
/// the time of writing and private to `reassembly.rs`) of segments are
/// buffered. Gap-free expectations below keep the total number of segments,
/// including extras and the FIN, at half of that so they hold for any limit of
/// at least `GAP_FREE_SEGMENTS`; lowering the limit below it must fail loudly.
const GAP_FREE_SEGMENTS: usize = 16;

/// Few enough segments can never pend at once, so no forced gap occurs and the
/// whole stream must come out exactly once, in order, followed by one FIN.
#[test]
fn bounded_disorder_delivers_the_stream_exactly_once_in_order() {
    const SEED: u64 = 0x1357_9bdf_2468_ace0;
    let mut rng = Rng::new(SEED);
    for case in 0..2_000 {
        let isn = random_isn(&mut rng);
        let len = 1 + rng.below(300);
        let stream = random_stream(&mut rng, len);
        // At most 10 base chunks, 4 extras and the FIN.
        let min_chunk = len.div_ceil(10);
        let max_chunk = min_chunk + rng.below(40);
        let extras = rng.below(5);
        let spans = plan(&mut rng, &stream, (min_chunk, max_chunk), extras);
        assert!(
            spans.len() < GAP_FREE_SEGMENTS,
            "seed={SEED:#x} case={case}"
        );
        let context = format!("seed={SEED:#x} case={case} isn={isn:#x} len={len} spans={spans:?}");
        let mut order: Vec<TcpSegment> = spans
            .iter()
            .map(|&(a, b)| segment(isn, &stream, a, b))
            .collect();
        order.push(fin(isn, &stream));
        rng.shuffle(&mut order);
        let mut segments = vec![syn(isn)];
        segments.extend(order);
        // Empty segments are never buffered, so they do not count toward the bound.
        let empties = rng.below(4);
        sprinkle_empties(&mut rng, &mut segments, isn, &stream, empties);
        let mut reassembler = Reassembler::default();
        let delivery = run(&mut reassembler, segments, &stream, false, &context);
        assert_eq!(delivery.gaps, 0, "{context}: unexpected gap");
        assert_eq!(delivery.bytes, stream, "{context}: stream not delivered");
        assert_eq!(delivery.closes, 1, "{context}: FIN not delivered once");
    }
}

/// Heavy disorder, with or without a FIN, may force gaps, but never
/// corruption, duplication or a silent skip.
#[test]
fn heavy_disorder_only_skips_through_explicit_gaps() {
    const SEED: u64 = 0xdead_beef_0bad_f00d;
    let mut rng = Rng::new(SEED);
    let mut gapped = 0;
    let mut clean = 0;
    let mut closed = 0;
    for case in 0..1_000 {
        let isn = random_isn(&mut rng);
        let len = 200 + rng.below(1_800);
        let stream = random_stream(&mut rng, len);
        let max_chunk = 1 + rng.below(60);
        let extras = rng.below(120);
        let spans = plan(&mut rng, &stream, (1, max_chunk), extras);
        let with_fin = rng.below(2) == 0;
        let context = format!(
            "seed={SEED:#x} case={case} isn={isn:#x} len={len} max_chunk={max_chunk} \
             segments={} fin={with_fin}",
            spans.len()
        );
        let mut order: Vec<TcpSegment> = spans
            .iter()
            .map(|&(a, b)| segment(isn, &stream, a, b))
            .collect();
        if with_fin {
            order.push(fin(isn, &stream));
        }
        rng.shuffle(&mut order);
        let mut segments = vec![syn(isn)];
        segments.extend(order);
        let empties = rng.below(6);
        sprinkle_empties(&mut rng, &mut segments, isn, &stream, empties);
        let mut reassembler = Reassembler::default();
        let delivery = run(&mut reassembler, segments, &stream, false, &context);
        closed += delivery.closes;
        if delivery.gaps == 0 {
            clean += 1;
            assert_eq!(delivery.bytes, stream, "{context}: gap-free run lost bytes");
            assert_eq!(delivery.closes, usize::from(with_fin), "{context}");
        } else {
            gapped += 1;
        }
    }
    // Both regimes, and FIN closes under disorder, must actually be exercised.
    assert!(gapped > 100, "seed={SEED:#x} too few gapped runs: {gapped}");
    assert!(clean > 20, "seed={SEED:#x} too few clean runs: {clean}");
    assert!(closed > 100, "seed={SEED:#x} too few FIN closes: {closed}");
}

/// Mostly in-order delivery with local swaps, retransmissions and overlaps,
/// the shape of real captures, over many segments.
#[test]
fn local_swaps_and_retransmissions_deliver_everything() {
    const SEED: u64 = 0x0123_4567_89ab_cdef;
    // Each base chunk yields at most three items (original, copy, overlap), so
    // a window of 6 items buffers at most 5 segments ahead of a hole.
    const WINDOW: usize = 6;
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
        shuffle_windows(&mut rng, &mut order, WINDOW);
        order.push(fin(isn, &stream));
        let context = format!("seed={SEED:#x} case={case} isn={isn:#x} len={len}");
        let mut segments = vec![syn(isn)];
        segments.extend(order);
        let empties = rng.below(10);
        sprinkle_empties(&mut rng, &mut segments, isn, &stream, empties);
        let mut reassembler = Reassembler::default();
        let delivery = run(&mut reassembler, segments, &stream, false, &context);
        assert_eq!(delivery.gaps, 0, "{context}: unexpected gap");
        assert_eq!(delivery.bytes, stream, "{context}: stream not delivered");
        assert_eq!(delivery.closes, 1, "{context}");
    }
}

/// A retransmission whose overlapping bytes differ from what was already
/// delivered, or from what is buffered, is a conflict. Observed behavior:
/// the segment is dropped, the connection state is reset with exactly one
/// `ConflictingOverlap` gap, and the next in-order segment resumes delivery at
/// its own start. Resending the stream from the cursor then delivers every
/// byte exactly once, in order, and the conflicting bytes are never delivered.
#[test]
fn conflicting_retransmit_resets_once_and_delivery_resumes_exactly_once() {
    const SEED: u64 = 0xc0ff_ee00_dead_1234;
    let mut rng = Rng::new(SEED);
    let mut buffered_conflicts = 0;
    for case in 0..1_000 {
        let isn = random_isn(&mut rng);
        let len = 60 + rng.below(540);
        let stream = random_stream(&mut rng, len);
        // Chunks of at least 4 bytes keep delivered history under its entry cap.
        let spans = plan(&mut rng, &stream, (4, 40), 0);
        if spans.len() < 3 {
            continue;
        }
        // Chunks 0..k are delivered in order before the conflict.
        let k = 1 + rng.below(spans.len() - 2);
        let delivered_end = spans[k - 1].1;
        let against_buffered = rng.below(2) == 0;
        let mut segments = vec![syn(isn)];
        segments.extend(spans[..k].iter().map(|&(a, b)| segment(isn, &stream, a, b)));
        let (a, b, flip) = if against_buffered {
            // Buffer chunk k + 1 behind a hole, then contradict it.
            let (a, b) = spans[k + 1];
            segments.push(segment(isn, &stream, a, b));
            buffered_conflicts += 1;
            (a, b, a + rng.below(b - a))
        } else {
            // Contradict bytes that were already delivered.
            let a = rng.below(delivered_end);
            let b = (a + 1 + rng.below(40)).min(stream.len());
            (a, b, a + rng.below(delivered_end.min(b) - a))
        };
        let mut corrupt = segment(isn, &stream, a, b);
        corrupt.bytes[flip - a] ^= 0xff;
        segments.push(corrupt);
        // Resend from the cursor: every chunk from k on, then the FIN.
        segments.extend(spans[k..].iter().map(|&(a, b)| segment(isn, &stream, a, b)));
        segments.push(fin(isn, &stream));
        let context = format!(
            "seed={SEED:#x} case={case} isn={isn:#x} len={len} k={k} \
             buffered={against_buffered} corrupt={a}..{b} flip={flip}"
        );
        let mut reassembler = Reassembler::default();
        let delivery = run(&mut reassembler, segments, &stream, true, &context);
        assert_eq!(
            delivery.conflicts, 1,
            "{context}: conflict not reported once"
        );
        assert_eq!(delivery.gaps, 0, "{context}: unexpected reorder gap");
        assert_eq!(
            delivery.bytes, stream,
            "{context}: not exactly once, in order"
        );
        assert_eq!(delivery.closes, 1, "{context}");
    }
    assert!(
        buffered_conflicts > 100,
        "seed={SEED:#x} too few buffered conflicts"
    );
}
