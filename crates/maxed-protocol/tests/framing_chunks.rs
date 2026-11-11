//! Seeded property tests: `Framer` output must not depend on chunk boundaries.
use maxed_protocol::{Framer, ProtocolError};

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
}

/// A well-formed frame: big-endian total length (header included), then body.
fn frame(rng: &mut Rng) -> Vec<u8> {
    let len = match rng.below(10) {
        0 => 5,
        1 => 5 + rng.below(4_000),
        _ => 5 + rng.below(60),
    };
    let mut bytes = (len as u32).to_be_bytes().to_vec();
    bytes.extend((4..len).map(|_| rng.next() as u8));
    bytes
}

/// Split `bytes` into chunks of the given style (0 = whole, 1 = single bytes).
fn chunks(rng: &mut Rng, bytes: &[u8], style: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let size = match style {
            0 => bytes.len(),
            1 => 1,
            2 => 1 + rng.below(3),
            3 => 1 + rng.below(16),
            _ => 1 + rng.below(512),
        };
        let end = (at + size).min(bytes.len());
        out.push(bytes[at..end].to_vec());
        at = end;
    }
    out
}

#[test]
fn any_chunking_yields_the_same_frames_as_one_chunk() {
    const SEED: u64 = 0x2468_ace0_1357_9bdf;
    let mut rng = Rng::new(SEED);
    for case in 0..500 {
        let frames: Vec<Vec<u8>> = (0..1 + rng.below(12)).map(|_| frame(&mut rng)).collect();
        let stream: Vec<u8> = frames.concat();
        let mut whole = Framer::default();
        let reference = whole.push(&stream);
        assert!(reference.error.is_none(), "seed={SEED:#x} case={case}");
        assert_eq!(reference.frames, frames, "seed={SEED:#x} case={case}");
        assert!(!whole.has_pending(), "seed={SEED:#x} case={case}");
        for style in 1..5 {
            let pieces = chunks(&mut rng, &stream, style);
            let mut framer = Framer::default();
            let mut got = Vec::new();
            for (n, piece) in pieces.iter().enumerate() {
                let output = framer.push(piece);
                assert!(
                    output.error.is_none(),
                    "seed={SEED:#x} case={case} style={style} chunk={n}"
                );
                got.extend(output.frames);
            }
            assert_eq!(got, frames, "seed={SEED:#x} case={case} style={style}");
            assert!(
                !framer.has_pending(),
                "seed={SEED:#x} case={case} style={style}: leftover bytes"
            );
        }
    }
}

#[test]
fn a_cut_stream_yields_only_complete_frames_and_keeps_the_rest_pending() {
    const SEED: u64 = 0x7777_0000_aaaa_5555;
    let mut rng = Rng::new(SEED);
    for case in 0..500 {
        let frames: Vec<Vec<u8>> = (0..1 + rng.below(8)).map(|_| frame(&mut rng)).collect();
        let stream: Vec<u8> = frames.concat();
        let cut = rng.below(stream.len() + 1);
        let mut boundary = 0;
        let mut complete = 0;
        for f in &frames {
            if boundary + f.len() > cut {
                break;
            }
            boundary += f.len();
            complete += 1;
        }
        for style in 0..5 {
            let mut framer = Framer::default();
            let mut got = Vec::new();
            for piece in chunks(&mut rng, &stream[..cut], style) {
                let output = framer.push(&piece);
                assert!(output.error.is_none(), "seed={SEED:#x} case={case}");
                got.extend(output.frames);
            }
            assert_eq!(
                got,
                frames[..complete],
                "seed={SEED:#x} case={case} style={style} cut={cut}"
            );
            assert_eq!(
                framer.has_pending(),
                cut != boundary,
                "seed={SEED:#x} case={case} style={style} cut={cut}"
            );
        }
    }
}

#[test]
fn an_impossible_length_reports_after_every_earlier_frame_at_any_chunking() {
    const SEED: u64 = 0x0f0f_f0f0_1234_4321;
    let mut rng = Rng::new(SEED);
    for case in 0..500 {
        let frames: Vec<Vec<u8>> = (0..rng.below(6)).map(|_| frame(&mut rng)).collect();
        let bad = [0u32, 1, 4, 200_001, u32::MAX][rng.below(5)];
        let header_end = frames.concat().len() + 4;
        let mut stream = frames.concat();
        stream.extend(bad.to_be_bytes());
        stream.extend((0..rng.below(8)).map(|_| rng.next() as u8));
        for style in 0..5 {
            let mut framer = Framer::default();
            let mut got = Vec::new();
            // Stream byte range [before, after) of the chunk that reported the error.
            let mut before = 0;
            let mut error = None;
            for piece in chunks(&mut rng, &stream, style) {
                let after = before + piece.len();
                let output = framer.push(&piece);
                got.extend(output.frames);
                if let Some(e) = output.error {
                    error = Some((e, before, after));
                    break;
                }
                before = after;
            }
            let context = format!("seed={SEED:#x} case={case} style={style} bad={bad}");
            assert_eq!(got, frames, "{context}");
            let (e, before, after) = error.unwrap_or_else(|| panic!("{context}: no error"));
            assert_eq!(e, ProtocolError::Framing, "{context}");
            // Reported by the chunk holding the header's last byte, not earlier or later.
            let last_header_byte = header_end - 1;
            assert!(
                before <= last_header_byte && last_header_byte < after,
                "{context}: error from chunk {before}..{after}, header ends at {header_end}"
            );
            // The rejected header is dropped, so the framer is usable again.
            assert!(!framer.has_pending(), "{context}");
            let ok = frame(&mut rng);
            let output = framer.push(&ok);
            assert!(output.error.is_none() && output.frames == [ok], "{context}");
        }
    }
}
