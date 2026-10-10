use maxed_capture::{parse_frame, ParseError};
use maxed_protocol::{DecodeEvent, Decoder};
use maxed_stream::Reassembler;
use maxed_types::{GapReason, RawFrame, StreamEvent, Timestamp};

#[derive(Default)]
pub struct Pipeline {
    streams: Reassembler,
    decoder: Decoder,
    next_index: u64,
    last_at: u64,
    unsupported_frames: u64,
}
impl Pipeline {
    pub fn explain_failures(&mut self, enabled: bool) {
        self.decoder.explain_failures(enabled);
    }
    pub fn take_failures(&mut self) -> Vec<maxed_protocol::explanation::StructuralFailure> {
        self.decoder.take_failures()
    }
    pub fn unsupported_frames(&self) -> u64 {
        self.unsupported_frames
    }
    fn decode(&mut self, events: Vec<StreamEvent>) -> Vec<DecodeEvent> {
        events
            .into_iter()
            .flat_map(|event| self.decoder.push(event))
            .collect()
    }
    pub fn push(&mut self, frame: RawFrame) -> Vec<DecodeEvent> {
        let mut events = Vec::new();
        self.last_at = frame.at.0;
        if frame.index != self.next_index {
            events.extend(self.streams.gap_all(frame.at, GapReason::CaptureDrop));
        }
        self.next_index = frame.index.saturating_add(1);
        match parse_frame(&frame) {
            Ok(Some(segment)) => events.extend(self.streams.push(segment)),
            Ok(None) => {}
            Err(ParseError::UnsupportedLink) => {
                self.unsupported_frames = self.unsupported_frames.saturating_add(1);
            }
            Err(error) => events.extend(self.streams.gap_all(
                frame.at,
                match error {
                    ParseError::Truncated => GapReason::Truncated,
                    _ => GapReason::InvalidFrame,
                },
            )),
        }
        self.decode(events)
    }
    pub fn finish(&mut self) -> Vec<DecodeEvent> {
        let events = self.streams.close_all(Timestamp(self.last_at));
        let mut decoded = self.decode(events);
        // EOF/teardown is not a captured TCP interruption. Keep this distinction
        // at the pipeline boundary for demo, domain export and packet replay.
        for event in &mut decoded {
            if let DecodeEvent::Diagnostic { kind, .. } = event {
                if *kind == maxed_protocol::Diagnostic::Closed {
                    *kind = maxed_protocol::Diagnostic::Cancelled;
                }
            }
        }
        decoded
    }
}
