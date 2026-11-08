//! Test the driver contract independently of either protocol adapter.
use maxed_stream::driver::{Codec, FrameDriver, ParsedFrame, PipelineEvent, TrustChange};
use maxed_types::*;
use std::cell::Cell;

thread_local! { static PARSES: Cell<usize> = const { Cell::new(0) }; }
struct TestCodec;
impl Codec for TestCodec {
    type Event = StreamEvent;
    fn parse(frame: &RawFrame) -> ParsedFrame {
        PARSES.with(|n| n.set(n.get() + 1));
        match frame.bytes[0] {
            8 => {
                return ParsedFrame::Gap {
                    flow: None,
                    reason: GapReason::CaptureDrop,
                }
            }
            9 => return ParsedFrame::Unsupported,
            _ => {}
        }
        ParsedFrame::Segment(TcpSegment {
            flow: FlowKey {
                client: Endpoint {
                    ip: [127, 0, 0, 1],
                    port: 50000,
                },
                server: Endpoint {
                    ip: [127, 0, 0, 2],
                    port: 2050,
                },
            },
            dir: Direction::ClientToServer,
            at: frame.at,
            seq: frame.index as u32,
            syn: frame.bytes[0] == 1,
            fin: false,
            rst: false,
            bytes: frame.bytes[1..].to_vec(),
            origin: ByteOrigin {
                frame_index: frame.index,
                offset: 1,
                len: frame.bytes.len() - 1,
            },
        })
    }
    fn decode(&mut self, event: StreamEvent) -> Vec<StreamEvent> {
        // Model cancellation suppressing the decoder's teardown diagnostic.
        if matches!(event, StreamEvent::Closed { .. }) {
            vec![]
        } else {
            vec![event]
        }
    }
    fn trust(event: &StreamEvent) -> TrustChange {
        match event {
            StreamEvent::Data(c) if c.bytes == [2] => TrustChange::Packet {
                conn: c.conn,
                dir: c.dir,
                verified: true,
            },
            StreamEvent::Data(c) if c.bytes == [3] => TrustChange::Reset {
                conn: c.conn,
                dir: c.dir,
            },
            StreamEvent::Gap { conn, dir, .. } => TrustChange::Reset {
                conn: *conn,
                dir: *dir,
            },
            StreamEvent::Closed { conn, dir, .. } => TrustChange::Closed {
                conn: *conn,
                dir: *dir,
            },
            _ => TrustChange::None,
        }
    }
    fn is_teardown(event: &StreamEvent) -> bool {
        matches!(event, StreamEvent::Closed { .. })
    }
    fn cancelled(conn: ConnId, dir: Direction, at: Timestamp) -> StreamEvent {
        StreamEvent::Closed {
            conn,
            dir,
            at,
            reason: CloseReason::Stopped,
        }
    }
    fn clear(&mut self) {}
}
fn frame(index: u64, bytes: &[u8]) -> RawFrame {
    RawFrame {
        index,
        at: Timestamp(index + 10),
        original_len: bytes.len() as u32,
        bytes: bytes.to_vec(),
    }
}

#[test]
fn parse_once_and_preserve_metadata_and_open_order() {
    PARSES.with(|n| n.set(0));
    let mut driver = FrameDriver::new(TestCodec);
    let output = driver.push_frame(frame(0, &[1, 0]));
    PARSES.with(|n| assert_eq!(n.get(), 1));
    let metadata = output.segment.unwrap();
    assert!(metadata.syn && metadata.has_payload);
    assert!(output.trusted_traffic); // SYN-proven fragments need no complete packet.
    assert!(matches!(
        output.events.first(),
        Some(PipelineEvent::Opened { .. })
    ));
    driver.push_frame(frame(1, &[9]));
    assert_eq!(driver.unsupported_frames, 1);
}

#[test]
fn midstream_needs_verification_and_recovery_revokes_trust() {
    let mut driver = FrameDriver::new(TestCodec);
    assert!(!driver.push_frame(frame(0, &[0, 0])).trusted_traffic);
    assert!(driver.push_frame(frame(1, &[0, 2])).trusted_traffic);
    assert!(driver.push_frame(frame(2, &[0, 0])).trusted_traffic);
    assert!(!driver.push_frame(frame(3, &[0, 3])).trusted_traffic);
    assert!(!driver.push_frame(frame(4, &[0, 0])).trusted_traffic);
    assert!(driver.push_frame(frame(5, &[0, 2])).trusted_traffic);
    assert!(!driver.push_frame(frame(6, &[8])).trusted_traffic);
}

#[test]
fn index_gap_revokes_trust_and_eof_synthesizes_current_time_close() {
    let mut driver = FrameDriver::new(TestCodec);
    driver.push_frame(frame(0, &[1]));
    assert!(driver.push_frame(frame(1, &[0, 0])).trusted_traffic);
    let output = driver.push_frame(frame(3, &[0, 0]));
    assert!(!output.trusted_traffic);
    assert!(output.events.iter().any(|e| matches!(
        e,
        PipelineEvent::Decoded(StreamEvent::Gap {
            reason: GapReason::CaptureDrop,
            ..
        })
    )));
    let events = driver.finish_at(Timestamp(99));
    // Reassembly tears down both directional halves, including the unopened half.
    assert_eq!(events.len(), 2);
    for (event, expected_dir) in events
        .iter()
        .zip([Direction::ClientToServer, Direction::ServerToClient])
    {
        assert!(matches!(event, PipelineEvent::Decoded(StreamEvent::Closed {
            at: Timestamp(99), reason: CloseReason::Stopped, dir, ..
        }) if *dir == expected_dir));
    }
    assert!(driver.finish().is_empty());
}
