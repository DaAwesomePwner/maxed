use maxed_capture::{parse_frame, ParseError};
use maxed_protocol::{DecodeEvent, Decoder};
use maxed_stream::Reassembler;
use maxed_types::{ConnId, Direction, GapReason, RawFrame, StreamEvent, Timestamp};
use std::sync::{atomic::AtomicBool, Arc};

// Keep stream openings ordered with decoder diagnostics without boxing hot-path packets.
#[allow(clippy::large_enum_variant)]
pub(super) enum PipelineEvent {
    Opened { conn: ConnId, dir: Direction },
    Decoded(DecodeEvent),
}

// Same frame -> stream -> protocol contract as maxed-cli, with cancellation for exit.
pub(super) struct Pipeline {
    streams: Reassembler,
    decoder: Decoder,
    next_index: u64,
    last_at: Timestamp,
}

impl Pipeline {
    pub(super) fn new(cancel: Arc<AtomicBool>) -> Self {
        Self {
            streams: Reassembler::default(),
            decoder: Decoder::with_cancellation(cancel),
            next_index: 0,
            last_at: Timestamp(0),
        }
    }

    fn decode(&mut self, events: Vec<StreamEvent>) -> Vec<PipelineEvent> {
        events
            .into_iter()
            .flat_map(|event| {
                let opened = match &event {
                    StreamEvent::Opened { conn, dir, .. } => Some(PipelineEvent::Opened {
                        conn: *conn,
                        dir: *dir,
                    }),
                    _ => None,
                };
                opened.into_iter().chain(
                    self.decoder
                        .push(event)
                        .into_iter()
                        .map(PipelineEvent::Decoded),
                )
            })
            .collect()
    }

    pub(super) fn push(&mut self, frame: RawFrame) -> Vec<PipelineEvent> {
        let mut events = self.streams.expire(frame.at);
        self.last_at = frame.at;
        if frame.index != self.next_index {
            events.extend(self.streams.gap_all(frame.at, GapReason::CaptureDrop));
        }
        self.next_index = frame.index.saturating_add(1);
        match parse_frame(&frame) {
            Ok(Some(segment)) => events.extend(self.streams.push(segment)),
            Ok(None) | Err(ParseError::UnsupportedLink) => {}
            Err(error) => {
                if let Some((flow, dir)) = error.affected_flow(&frame) {
                    events.extend(self.streams.gap_flow(
                        flow,
                        dir,
                        frame.at,
                        match error {
                            ParseError::Truncated => GapReason::Truncated,
                            _ => GapReason::InvalidFrame,
                        },
                    ));
                }
            }
        }
        self.decode(events)
    }

    pub(super) fn finish(&mut self) -> Vec<PipelineEvent> {
        self.finish_at(self.last_at)
    }

    pub(super) fn finish_at(&mut self, at: Timestamp) -> Vec<PipelineEvent> {
        // Runtime timers may already have advanced beyond the last frame. The
        // synthetic shutdown close occurs now, not at an older packet receipt.
        let events = self.streams.close_all(at.max(self.last_at));
        let mut decoded = self.decode(events);
        // Drop buffers even when decoder cancellation suppresses diagnostics.
        self.decoder = Decoder::default();
        // Preserve loss diagnostics and recovered buffered data at teardown.
        for event in &mut decoded {
            if let PipelineEvent::Decoded(DecodeEvent::Diagnostic { kind, .. }) = event {
                if *kind == maxed_protocol::Diagnostic::Closed {
                    *kind = maxed_protocol::Diagnostic::Cancelled;
                }
            }
        }
        decoded
    }
}
