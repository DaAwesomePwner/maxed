use maxed_capture::{parse_frame, ParseError};
use maxed_protocol::{DecodeEvent, Decoder, Diagnostic};
use maxed_stream::driver::{Codec, FrameDriver, ParsedFrame, TrustChange};
use maxed_types::{ConnId, Direction, GapReason, RawFrame, StreamEvent, Timestamp};

pub struct ProtocolCodec(Decoder);
impl Codec for ProtocolCodec {
    type Event = DecodeEvent;
    fn parse(frame: &RawFrame) -> ParsedFrame {
        match parse_frame(frame) {
            Ok(Some(s)) => ParsedFrame::Segment(s),
            Ok(None) => ParsedFrame::Ignored,
            Err(ParseError::UnsupportedLink) => ParsedFrame::Unsupported,
            Err(e) => ParsedFrame::Gap {
                flow: e.affected_flow(frame),
                reason: if matches!(e, ParseError::Truncated) {
                    GapReason::Truncated
                } else {
                    GapReason::InvalidFrame
                },
            },
        }
    }
    fn decode(&mut self, event: StreamEvent) -> Vec<DecodeEvent> {
        self.0.push(event)
    }
    fn trust(event: &DecodeEvent) -> TrustChange {
        match event {
            DecodeEvent::Packet(p) => TrustChange::Packet {
                conn: p.conn,
                dir: p.dir,
                verified: p.alignment_verified,
            },
            DecodeEvent::Diagnostic {
                conn, dir, kind, ..
            } => match kind {
                Diagnostic::Closed | Diagnostic::Cancelled => TrustChange::Closed {
                    conn: *conn,
                    dir: *dir,
                },
                Diagnostic::Recovering | Diagnostic::Recovered => TrustChange::Reset {
                    conn: *conn,
                    dir: *dir,
                },
                _ => TrustChange::None,
            },
        }
    }
    fn is_teardown(event: &DecodeEvent) -> bool {
        matches!(
            event,
            DecodeEvent::Diagnostic {
                kind: Diagnostic::Closed | Diagnostic::Cancelled,
                ..
            }
        )
    }
    fn cancelled(conn: ConnId, dir: Direction, at: Timestamp) -> DecodeEvent {
        DecodeEvent::Diagnostic {
            conn,
            dir,
            at,
            kind: Diagnostic::Cancelled,
        }
    }
    fn clear(&mut self) {
        self.0 = Decoder::default();
    }
}
use std::sync::{atomic::AtomicBool, Arc};
pub(super) type PipelineEvent = maxed_stream::driver::PipelineEvent<DecodeEvent>;
pub(super) struct Pipeline(FrameDriver<ProtocolCodec>);
impl Pipeline {
    pub(super) fn new(cancel: Arc<AtomicBool>) -> Self {
        Self(FrameDriver::new(ProtocolCodec(Decoder::with_cancellation(
            cancel,
        ))))
    }
    #[cfg(test)]
    pub(super) fn push(&mut self, frame: RawFrame) -> Vec<PipelineEvent> {
        self.push_frame(frame).events
    }
    pub(super) fn push_frame(
        &mut self,
        frame: RawFrame,
    ) -> maxed_stream::driver::FrameOutput<DecodeEvent> {
        self.0.push_frame(frame)
    }
    pub(super) fn finish(&mut self) -> Vec<PipelineEvent> {
        self.0.finish()
    }
    pub(super) fn finish_at(&mut self, at: Timestamp) -> Vec<PipelineEvent> {
        self.0.finish_at(at)
    }
}
