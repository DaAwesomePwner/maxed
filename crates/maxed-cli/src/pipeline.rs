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
        // CLI decoding is never cancelled; Closed already releases stream buffers.
        // Preserve structural explanations until the exporter drains them.
    }
}
pub struct Pipeline(FrameDriver<ProtocolCodec>);
impl Default for Pipeline {
    fn default() -> Self {
        Self(FrameDriver::new(ProtocolCodec(Decoder::default())))
    }
}
fn decoded(events: Vec<maxed_stream::driver::PipelineEvent<DecodeEvent>>) -> Vec<DecodeEvent> {
    events
        .into_iter()
        .filter_map(|e| match e {
            maxed_stream::driver::PipelineEvent::Decoded(e) => Some(e),
            _ => None,
        })
        .collect()
}
pub struct FrameOutput {
    pub events: Vec<DecodeEvent>,
    pub trusted_traffic: bool,
}
impl Pipeline {
    pub fn explain_failures(&mut self, enabled: bool) {
        self.0.codec.0.explain_failures(enabled);
    }
    pub fn take_failures(&mut self) -> Vec<maxed_protocol::explanation::StructuralFailure> {
        self.0.codec.0.take_failures()
    }
    pub fn unsupported_frames(&self) -> u64 {
        self.0.unsupported_frames
    }
    pub fn push_frame(&mut self, frame: RawFrame) -> FrameOutput {
        let output = self.0.push_frame(frame);
        FrameOutput {
            events: decoded(output.events),
            trusted_traffic: output.trusted_traffic,
        }
    }
    pub fn push(&mut self, frame: RawFrame) -> Vec<DecodeEvent> {
        self.push_frame(frame).events
    }
    pub fn finish(&mut self) -> Vec<DecodeEvent> {
        decoded(self.0.finish())
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
    fn live_and_cli_classify_recovery_and_teardown_identically() {
        let conn = ConnId(7);
        let dir = Direction::ServerToClient;
        for kind in [
            Diagnostic::Recovering,
            Diagnostic::Recovered,
            Diagnostic::Closed,
            Diagnostic::Cancelled,
            Diagnostic::BadFrame,
            Diagnostic::UnknownId,
        ] {
            let event = DecodeEvent::Diagnostic {
                conn,
                dir,
                at: Timestamp(42),
                kind,
            };
            let expected = match kind {
                Diagnostic::Recovering | Diagnostic::Recovered => TrustChange::Reset { conn, dir },
                Diagnostic::Closed | Diagnostic::Cancelled => TrustChange::Closed { conn, dir },
                _ => TrustChange::None,
            };
            assert_eq!(ProtocolCodec::trust(&event), expected);
            assert_eq!(live_pipeline::ProtocolCodec::trust(&event), expected);
            let teardown = matches!(kind, Diagnostic::Closed | Diagnostic::Cancelled);
            assert_eq!(ProtocolCodec::is_teardown(&event), teardown);
            assert_eq!(live_pipeline::ProtocolCodec::is_teardown(&event), teardown);
        }
    }

    fn event_keys(events: &[DecodeEvent]) -> Vec<String> {
        events
            .iter()
            .map(|event| match event {
                DecodeEvent::Packet(p) => format!(
                    "{}:{}:{}:{}:{}",
                    p.conn.0,
                    p.at.0,
                    p.id,
                    p.alignment_verified,
                    serde_json::to_string(
                        &maxed_protocol::canonical::canonical_line(0, p).unwrap()
                    )
                    .unwrap()
                ),
                DecodeEvent::Diagnostic {
                    conn,
                    dir,
                    at,
                    kind,
                } => format!("{conn:?}:{dir:?}:{at:?}:{kind:?}"),
            })
            .collect()
    }

    #[test]
    fn live_and_offline_recording_have_identical_events_and_sessions() {
        use maxed_engine::{Engine, EngineConfig};
        use maxed_recording::{PcapReader, PcapWriter};
        use maxed_types::{Catalog, FightEnd};
        use std::sync::{atomic::AtomicBool, Arc};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/scrubbed/maxed-demo");
        let catalog: Catalog =
            serde_json::from_slice(&std::fs::read(root.join("catalog.json")).unwrap()).unwrap();
        let config = EngineConfig::default();
        let mut live_engine = Engine::new(config.clone(), Arc::new(catalog.clone())).unwrap();
        let mut offline_engine = Engine::new(config, Arc::new(catalog)).unwrap();
        let mut writer = PcapWriter::new(Vec::new()).unwrap();
        // Unverified midstream garbage must not create a session before the story.
        writer
            .write_frame(&frame(0, 50002, 10, false, &[0, 0, 0, 5, 105]))
            .unwrap();
        let mut story =
            PcapReader::new(std::fs::File::open(root.join("story.pcap")).unwrap()).unwrap();
        while let Some(mut frame) = story.next_frame().unwrap() {
            frame.at.0 += 10_000_000;
            writer.write_frame(&frame).unwrap();
        }
        let bytes = writer.finish().unwrap();
        let mut reader = PcapReader::new(bytes.as_slice()).unwrap();
        let mut live = live_pipeline::Pipeline::new(Arc::new(AtomicBool::new(false)));
        let mut offline = Pipeline::default();
        let mut live_events = Vec::new();
        let mut offline_events = Vec::new();
        let mut at = Timestamp(0);
        while let Some(frame) = reader.next_frame().unwrap() {
            at = frame.at;
            let first = frame.index == 0;
            let a = live.push_frame(frame.clone());
            let b = offline.push_frame(frame);
            assert_eq!(a.trusted_traffic, b.trusted_traffic);
            if first {
                assert!(!a.trusted_traffic);
            }
            if a.trusted_traffic {
                live_events.extend(live_engine.observe_live_traffic(at));
                offline_events.extend(offline_engine.observe_traffic(at));
            }
            let a = decoded(a.events);
            assert_eq!(event_keys(&a), event_keys(&b.events));
            for (engine, output, events) in [
                (&mut live_engine, &mut live_events, a),
                (&mut offline_engine, &mut offline_events, b.events),
            ] {
                for event in events {
                    output.extend(match event {
                        DecodeEvent::Packet(p) => engine.apply_observed(p),
                        DecodeEvent::Diagnostic {
                            conn,
                            dir,
                            at,
                            kind,
                        } => engine.diagnostic(conn, dir, at, kind),
                    });
                }
            }
        }
        let a = decoded(live.finish());
        let b = offline.finish();
        assert_eq!(event_keys(&a), event_keys(&b));
        for (engine, output, events) in [
            (&mut live_engine, &mut live_events, a),
            (&mut offline_engine, &mut offline_events, b),
        ] {
            for event in events {
                if let DecodeEvent::Diagnostic {
                    conn,
                    dir,
                    at,
                    kind,
                } = event
                {
                    output.extend(engine.diagnostic(conn, dir, at, kind));
                }
            }
            output.extend(engine.finish(at, FightEnd::CaptureStopped));
        }
        let sessions: Vec<_> = live_events
            .iter()
            .filter_map(|e| match e {
                maxed_types::EngineEvent::SessionUpdated(session) => Some(session),
                _ => None,
            })
            .collect();
        assert!(!sessions.is_empty());
        assert!(sessions.iter().all(|s| s.started.0 >= 10_000_000));
        assert!(sessions.last().unwrap().ended.is_some());
        assert_eq!(
            serde_json::to_value(live_events).unwrap(),
            serde_json::to_value(offline_events).unwrap()
        );
    }

    #[test]
    fn attributable_bad_frame_does_not_reset_other_flow_decoder() {
        for failure in 0..3 {
            let mut p = Pipeline::default();
            p.push(frame(0, 50000, 9, true, &[]));
            p.push(frame(1, 50001, 9, true, &[]));
            let mut bad = frame(2, 50000, 10, false, &[0, 0, 0, 5, 105]);
            match failure {
                0 => {
                    bad.bytes.pop();
                }
                1 => bad.bytes[20] = 0x20,
                _ => bad.bytes[46] = 0x40,
            }
            let events = p.push(bad);
            assert!(matches!(
                events.as_slice(),
                [DecodeEvent::Diagnostic {
                    conn: ConnId(1),
                    dir: Direction::ClientToServer,
                    kind: Diagnostic::Recovering,
                    ..
                }]
            ));
            let events = p.push(frame(3, 50001, 10, false, &[0, 0, 0, 5, 105]));
            assert!(matches!(events.first(), Some(DecodeEvent::Packet(packet))
                if packet.conn == ConnId(2) && packet.alignment_verified));
        }
    }

    #[test]
    fn eof_preserves_partial_frame_diagnostic_without_coverage_loss() {
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
        assert!(!events.iter().any(|e| matches!(
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

// Compile the actual live codec adapter against the same synthetic recording.
#[cfg(test)]
#[allow(dead_code)] // The imported runtime adapter also exposes shutdown/test helpers.
#[path = "../../../app/src-tauri/src/runtime/pipeline.rs"]
mod live_pipeline;
