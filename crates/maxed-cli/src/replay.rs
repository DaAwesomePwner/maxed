use crate::{CliError, Pipeline};
use maxed_protocol::{DecodeEvent, Diagnostic, Packet};
use maxed_recording::{PcapReader, RecordingError};
use std::{fs::File, io::BufReader, path::Path};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayReport {
    pub frames: u64,
    pub decoded: u64,
    pub unknown: u64,
    pub failures: u64,
    pub field_failures: u64,
    pub trailing_byte_packets: u64,
    pub unknown_id_packets: u64,
    pub wrong_direction_packets: u64,
    pub unverified_packets: u64,
    pub truncated: bool,
}
impl ReplayReport {
    pub fn observe(&mut self, event: &DecodeEvent) {
        match event {
            DecodeEvent::Packet(packet) => {
                self.decoded = self.decoded.saturating_add(1);
                if !packet.alignment_verified {
                    self.unverified_packets = self.unverified_packets.saturating_add(1);
                }
                if matches!(packet.packet, Packet::Unknown { .. }) {
                    self.unknown = self.unknown.saturating_add(1);
                }
            }
            DecodeEvent::Diagnostic { kind, .. } => {
                match kind {
                    Diagnostic::BadFields => {
                        self.field_failures = self.field_failures.saturating_add(1)
                    }
                    Diagnostic::TrailingBytes => {
                        self.trailing_byte_packets = self.trailing_byte_packets.saturating_add(1)
                    }
                    Diagnostic::UnknownId => {
                        self.unknown_id_packets = self.unknown_id_packets.saturating_add(1)
                    }
                    Diagnostic::WrongDirection => {
                        self.wrong_direction_packets =
                            self.wrong_direction_packets.saturating_add(1)
                    }
                    _ => {}
                }
                if matches!(
                    kind,
                    Diagnostic::BadFrame | Diagnostic::BadFields | Diagnostic::TrailingBytes
                ) {
                    self.failures = self.failures.saturating_add(1);
                }
            }
        }
    }
}

impl std::fmt::Display for ReplayReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "frames={} decoded={} unknown={} failures={} fieldFailures={} trailingBytePackets={} unknownIdPackets={} wrongDirectionPackets={} unverifiedPackets={} truncated={}",
            self.frames, self.decoded, self.unknown, self.failures, self.field_failures,
            self.trailing_byte_packets, self.unknown_id_packets, self.wrong_direction_packets, self.unverified_packets, self.truncated)
    }
}

pub fn replay(path: &Path) -> Result<ReplayReport, CliError> {
    let mut reader = PcapReader::new(BufReader::new(File::open(path)?))?;
    let mut pipeline = Pipeline::default();
    let mut report = ReplayReport::default();
    loop {
        let frame = match reader.next_frame() {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(RecordingError::Truncated) => {
                report.truncated = true;
                break;
            }
            Err(error) => return Err(error.into()),
        };
        report.frames = report.frames.saturating_add(1);
        for event in pipeline.push(frame) {
            report.observe(&event);
        }
    }
    for event in pipeline.finish() {
        report.observe(&event);
    }
    Ok(report)
}
