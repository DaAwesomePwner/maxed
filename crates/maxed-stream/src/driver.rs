//! Shared frame ordering, teardown and directional traffic trust.
//! Wire parsing and decoding are supplied by adapters to keep this crate independent
//! of capture and protocol implementations.
use crate::Reassembler;
use maxed_types::{
    ConnId, Direction, FlowKey, GapReason, RawFrame, StreamEvent, TcpSegment, Timestamp,
};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct SegmentMetadata {
    pub flow: FlowKey,
    pub dir: Direction,
    pub syn: bool,
    pub has_payload: bool,
}
impl From<&TcpSegment> for SegmentMetadata {
    fn from(s: &TcpSegment) -> Self {
        Self {
            flow: s.flow,
            dir: s.dir,
            syn: s.syn,
            has_payload: !s.bytes.is_empty(),
        }
    }
}
pub enum ParsedFrame {
    Segment(TcpSegment),
    Ignored,
    Unsupported,
    Gap {
        flow: Option<(FlowKey, Direction)>,
        reason: GapReason,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub enum TrustChange {
    Packet {
        conn: ConnId,
        dir: Direction,
        verified: bool,
    },
    Reset {
        conn: ConnId,
        dir: Direction,
    },
    Closed {
        conn: ConnId,
        dir: Direction,
    },
    None,
}
pub trait Codec {
    type Event;
    fn parse(frame: &RawFrame) -> ParsedFrame;
    fn decode(&mut self, event: StreamEvent) -> Vec<Self::Event>;
    fn trust(event: &Self::Event) -> TrustChange;
    fn is_teardown(event: &Self::Event) -> bool;
    fn cancelled(conn: ConnId, dir: Direction, at: Timestamp) -> Self::Event;
    fn clear(&mut self);
}
#[allow(clippy::large_enum_variant)]
pub enum PipelineEvent<E> {
    Opened { conn: ConnId, dir: Direction },
    Decoded(E),
}
pub struct FrameOutput<E> {
    pub segment: Option<SegmentMetadata>,
    pub events: Vec<PipelineEvent<E>>,
    pub trusted_traffic: bool,
}
/// Trust is directional and belongs to a stream generation, never a socket
/// tuple alone. Retain only live entries (bounded by the reassembler limit).
#[derive(Default)]
struct TrafficTrust {
    streams: HashMap<(ConnId, Direction), (FlowKey, bool)>,
}
impl TrafficTrust {
    fn observe<C: Codec>(
        &mut self,
        segment: Option<SegmentMetadata>,
        events: &[PipelineEvent<C::Event>],
    ) -> bool {
        let mut verified_packet = false;
        for event in events {
            match event {
                PipelineEvent::Opened { conn, dir } => {
                    if let Some(s) = segment {
                        // Match the decoder fresh && syn_proven rule, including
                        // verified fragments before the first complete packet.
                        self.streams.insert((*conn, *dir), (s.flow, s.syn));
                    }
                }
                PipelineEvent::Decoded(event) => match C::trust(event) {
                    TrustChange::Packet {
                        conn,
                        dir,
                        verified,
                    } => {
                        if let Some((_, value)) = self.streams.get_mut(&(conn, dir)) {
                            *value = verified;
                        }
                        verified_packet |= verified;
                    }
                    TrustChange::Reset { conn, dir } => {
                        if let Some((_, value)) = self.streams.get_mut(&(conn, dir)) {
                            *value = false;
                        }
                    }
                    TrustChange::Closed { conn, dir } => {
                        self.streams.remove(&(conn, dir));
                    }
                    TrustChange::None => {}
                },
            }
        }
        segment.is_some_and(|s| {
            s.has_payload
                && (verified_packet
                    || self.streams.iter().any(|((_, dir), (flow, verified))| {
                        *dir == s.dir && *flow == s.flow && *verified
                    }))
        })
    }
}
pub struct FrameDriver<C: Codec> {
    streams: Reassembler,
    pub codec: C,
    trust: TrafficTrust,
    next_index: u64,
    last_at: Timestamp,
    pub unsupported_frames: u64,
}
impl<C: Codec> FrameDriver<C> {
    pub fn new(codec: C) -> Self {
        Self {
            streams: Reassembler::default(),
            codec,
            trust: TrafficTrust::default(),
            next_index: 0,
            last_at: Timestamp(0),
            unsupported_frames: 0,
        }
    }
    fn decode(&mut self, events: Vec<StreamEvent>) -> Vec<PipelineEvent<C::Event>> {
        let mut output = Vec::new();
        for event in events {
            if let StreamEvent::Opened { conn, dir, .. } = &event {
                output.push(PipelineEvent::Opened {
                    conn: *conn,
                    dir: *dir,
                });
            }
            output.extend(
                self.codec
                    .decode(event)
                    .into_iter()
                    .map(PipelineEvent::Decoded),
            );
        }
        output
    }
    pub fn push_frame(&mut self, frame: RawFrame) -> FrameOutput<C::Event> {
        let parsed = C::parse(&frame);
        let mut events = self.streams.expire(frame.at);
        self.last_at = frame.at;
        if frame.index != self.next_index {
            events.extend(self.streams.gap_all(frame.at, GapReason::CaptureDrop));
        }
        self.next_index = frame.index.saturating_add(1);
        let mut segment = None;
        match parsed {
            ParsedFrame::Segment(s) => {
                segment = Some((&s).into());
                events.extend(self.streams.push(s));
            }
            ParsedFrame::Ignored => {}
            ParsedFrame::Unsupported => {
                self.unsupported_frames = self.unsupported_frames.saturating_add(1);
            }
            ParsedFrame::Gap { flow, reason } => match flow {
                Some((flow, dir)) => {
                    events.extend(self.streams.gap_flow(flow, dir, frame.at, reason))
                }
                None => events.extend(self.streams.gap_all(frame.at, reason)),
            },
        }
        let events = self.decode(events);
        let trusted_traffic = self.trust.observe::<C>(segment, &events);
        FrameOutput {
            segment,
            events,
            trusted_traffic,
        }
    }
    pub fn finish(&mut self) -> Vec<PipelineEvent<C::Event>> {
        self.finish_at(self.last_at)
    }
    pub fn finish_at(&mut self, at: Timestamp) -> Vec<PipelineEvent<C::Event>> {
        let mut output = Vec::new();
        // Timers may have advanced beyond the last frame: close at the current
        // adapter time, never at an older packet receipt.
        for event in self.streams.close_all(at.max(self.last_at)) {
            let close = match &event {
                StreamEvent::Closed { conn, dir, at, .. } => Some((*conn, *dir, *at)),
                _ => None,
            };
            // Stopped streams discard holes and retain partial-frame diagnostics.
            // Synthesize teardown even when decoder cancellation silences it.
            output.extend(
                self.decode(vec![event])
                    .into_iter()
                    .filter(|e| !matches!(e, PipelineEvent::Decoded(e) if C::is_teardown(e))),
            );
            if let Some((conn, dir, at)) = close {
                output.push(PipelineEvent::Decoded(C::cancelled(conn, dir, at)));
            }
        }
        self.codec.clear();
        self.trust = TrafficTrust::default();
        output
    }
}
