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
        let mut events = self.streams.expire(frame.at);
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

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_protocol::Diagnostic;
    use maxed_types::{ConnId, Direction};

    fn frame(index: u64, port: u16, seq: u32, syn: bool, payload: &[u8]) -> RawFrame {
        let mut bytes = vec![0; 54 + payload.len()];
        bytes[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
        bytes[14] = 0x45;
        bytes[16..18].copy_from_slice(&((40 + payload.len()) as u16).to_be_bytes());
        bytes[23] = 6;
        bytes[26..30].copy_from_slice(&[127, 0, 0, 1]);
        bytes[30..34].copy_from_slice(&[127, 0, 0, 2]);
        bytes[34..36].copy_from_slice(&port.to_be_bytes());
        bytes[36..38].copy_from_slice(&2050u16.to_be_bytes());
        bytes[38..42].copy_from_slice(&seq.to_be_bytes());
        bytes[46] = 0x50;
        bytes[47] = if syn { 2 } else { 16 };
        bytes[54..].copy_from_slice(payload);
        RawFrame {
            index,
            at: Timestamp(index),
            original_len: bytes.len() as u32,
            bytes,
        }
    }

    #[test]
    fn bad_frame_does_not_reset_other_flow_decoder() {
        for failure in 0..4 {
            let mut p = Pipeline::default();
            p.push(frame(0, 50000, 9, true, &[]));
            p.push(frame(1, 50001, 9, true, &[]));
            let mut bad = frame(2, 50000, 10, false, &[0, 0, 0, 5, 105]);
            match failure {
                0 => {
                    bad.bytes.pop();
                }
                1 => bad.bytes[20] = 0x20,
                2 => bad.bytes[46] = 0x40,
                _ => bad.bytes.truncate(15),
            }
            let events = p.push(bad);
            if failure == 3 {
                assert!(events.is_empty());
            } else {
                assert!(matches!(
                    events.as_slice(),
                    [DecodeEvent::Diagnostic {
                        conn: ConnId(1),
                        dir: Direction::ClientToServer,
                        kind: Diagnostic::Recovering,
                        ..
                    }]
                ));
            }
            let events = p.push(frame(3, 50001, 10, false, &[0, 0, 0, 5, 105]));
            assert!(matches!(events.first(), Some(DecodeEvent::Packet(packet))
                if packet.conn == ConnId(2) && packet.alignment_verified));
        }
    }

    #[test]
    fn eof_preserves_partial_frame_loss() {
        let mut p = Pipeline::default();
        p.push(frame(0, 50000, 9, true, &[]));
        p.push(frame(1, 50000, 10, false, &[0, 0]));
        let events = p.finish();
        assert!(matches!(
            events.first(),
            Some(DecodeEvent::Diagnostic {
                kind: Diagnostic::BadFrame,
                ..
            })
        ));
        assert!(events.iter().any(|e| matches!(
            e,
            DecodeEvent::Diagnostic {
                kind: Diagnostic::Recovering,
                ..
            }
        )));
        assert!(!events.iter().any(|e| matches!(
            e,
            DecodeEvent::Diagnostic {
                kind: Diagnostic::Closed,
                ..
            }
        )));
    }
}
