//! Opt-in diagnostics. PcStats values/tails and short unknown notification tails
//! are explicitly authorized; other wire strings and blobs remain redacted.
//! Numeric previews follow the current schema and stop at strings or uncertain boundaries.
//! Keep the walk in parity with the typed decoder (the prefix tests enforce this).
use crate::{defs::known_name, ProtocolError, Reader};
use maxed_types::{ConnId, Direction, Timestamp};
use std::fmt;
mod pcstats;
pub use pcstats::explain_create_success;

#[derive(Debug, PartialEq, Eq)]
pub struct StatShape {
    // NEWTICK does not carry objectType; never substitute its objectId.
    pub object_type: Option<u16>,
    pub stat_type_num: u8,
    pub value_kind: &'static str,
}

#[derive(Debug)]
pub struct FailureDetails {
    pub id: u8,
    pub alignment_verified: bool,
    pub framed_length: usize,
    /// Zero-based offset in the complete game frame, including its five-byte header.
    pub offset: usize,
    pub error: ProtocolError,
    pub stats: Vec<StatShape>,
    pub next_hex: String,
    /// Body prefix (not frame prefix), cut before the first possible string field.
    pub prefix_hex: String,
    pub details: String,
}
impl fmt::Display for FailureDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "name={} id={} len={} offset={} error={:?}",
            known_name(self.id),
            self.id,
            self.framed_length,
            self.offset,
            self.error
        )?;
        if !self.alignment_verified && matches!(self.id, 42 | 10) {
            write!(f, " stats=redacted-unverified")?;
        } else if matches!(self.id, 42 | 10) {
            write!(f, " stats=[")?;
            for (i, stat) in self.stats.iter().enumerate() {
                if i != 0 {
                    write!(f, ",")?;
                }
                match stat.object_type {
                    Some(kind) => write!(f, "({kind},")?,
                    None => write!(f, "(unknown,")?,
                }
                write!(f, "{},{})", stat.stat_type_num, stat.value_kind)?;
            }
            write!(f, "]")?;
        }
        if !self.alignment_verified {
            write!(
                f,
                " bodyOffset={} prefixHex=redacted-unverified",
                self.offset - 5
            )?;
        } else {
            write!(f, " nextHex={}", self.next_hex)?;
        }
        if self.alignment_verified && matches!(self.id, 42 | 10) {
            write!(
                f,
                " bodyOffset={} prefixHex={} prefixEnd={} prefixSchema=current",
                self.offset - 5,
                self.prefix_hex,
                self.prefix_hex.len() / 2
            )?;
        }
        write!(f, "{}", self.details)
    }
}

/// A global zero-based complete-frame ordinal, including rejected/suppressed
/// frames. Lifecycle events between frames use the next frame's ordinal.
#[derive(Clone, Copy, Debug)]
pub struct ReportContext {
    pub packet_ordinal: u64,
    pub conn: ConnId,
    pub dir: Direction,
    pub at: Timestamp,
    pub last_newtick: Option<i32>,
    /// The recovery frame is zero; the next framed packet in this direction is
    /// one. The window ends on Recovering, Closed, or a new Opened event.
    pub packets_since_recovered: Option<u64>,
}
impl fmt::Display for ReportContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, " packetOrdinal={} connId={} dir={} timestampMicros={} lastNewTickId={} postRecovered={} packetsSinceRecovered={}",
            self.packet_ordinal, self.conn.0,
            match self.dir { Direction::ClientToServer => "c2s", Direction::ServerToClient => "s2c" },
            self.at.0, self.last_newtick.map_or_else(|| "none".into(), |v| v.to_string()),
            self.packets_since_recovered.is_some(),
            self.packets_since_recovered.map_or_else(|| "none".into(), |v| v.to_string()))
    }
}

#[derive(Debug)]
pub enum StreamReportEvent {
    Opened { fresh: bool },
    Recovering,
    Recovered,
    Closed,
}

#[derive(Debug)]
pub enum StructuralReport {
    Failure {
        context: ReportContext,
        failure: FailureDetails,
    },
    Stream {
        context: ReportContext,
        event: StreamReportEvent,
    },
    CreateSuccess {
        context: ReportContext,
        details: String,
    },
}

// Preserve the existing pipeline drain's source-compatible type name. Its
// opt-in output now includes lifecycle and successful CREATE_SUCCESS reports.
pub type StructuralFailure = StructuralReport;

impl fmt::Display for StructuralReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failure { context, failure } => write!(f, "{failure}{context}"),
            Self::CreateSuccess { context, details } => write!(f, "{details}{context}"),
            Self::Stream { context, event } => {
                match event {
                    StreamReportEvent::Opened { fresh } => write!(f, "event=Opened fresh={fresh}")?,
                    StreamReportEvent::Recovering => write!(f, "event=Recovering")?,
                    StreamReportEvent::Recovered => write!(f, "event=Recovered")?,
                    StreamReportEvent::Closed => write!(f, "event=Closed")?,
                }
                write!(f, "{context}")
            }
        }
    }
}

struct Walk<'a> {
    reader: Reader<'a>,
    stats: Vec<StatShape>,
    safe_end: usize,
    preview: bool,
    prefix_end: usize,
}
impl<'a> Walk<'a> {
    fn new(body: &'a [u8]) -> Self {
        Self {
            reader: Reader::new(body),
            stats: Vec::new(),
            safe_end: 0,
            preview: false,
            prefix_end: 0,
        }
    }
    fn number(&mut self, n: usize) -> Result<&'a [u8], ProtocolError> {
        self.safe_end = self.reader.position() + n;
        self.prefix_end = self
            .safe_end
            .min(self.reader.position() + self.reader.remaining());
        self.reader.take(n)
    }
    fn byte(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.number(1)?[0])
    }
    fn short(&mut self) -> Result<u16, ProtocolError> {
        let b = self.number(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn ci(&mut self) -> Result<i32, ProtocolError> {
        self.safe_end = self.reader.position();
        let result = self.reader.ci();
        self.prefix_end = self.reader.position();
        result
    }
    fn private(&mut self) -> Result<(), ProtocolError> {
        if self.preview {
            // Stop even before the length prefix; never resume after a string.
            return Err(ProtocolError::Definition);
        }
        // Redact both the length prefix and payload on every failing string/blob read.
        self.safe_end = 0;
        let n = usize::from(self.reader.u16()?);
        self.reader.take(n)?;
        Ok(())
    }
    fn fixed(&mut self, sizes: &[usize]) -> Result<(), ProtocolError> {
        for &n in sizes {
            self.number(n)?;
        }
        Ok(())
    }
    fn list(
        &mut self,
        n: i32,
        minimum: usize,
        mut item: impl FnMut(&mut Self) -> Result<(), ProtocolError>,
    ) -> Result<(), ProtocolError> {
        self.safe_end = 0;
        let n = usize::try_from(n).map_err(|_| ProtocolError::Length)?;
        if !self.preview && n > self.reader.remaining() / minimum {
            return Err(ProtocolError::Length);
        }
        for _ in 0..n {
            item(self)?;
        }
        Ok(())
    }
    fn status(&mut self, object_type: Option<u16>) -> Result<(), ProtocolError> {
        self.ci()?;
        self.fixed(&[4, 4])?;
        let n = self.ci()?;
        self.list(n, 3, |w| {
            let stat_type_num = w.byte()?;
            let string = matches!(
                stat_type_num,
                6 | 31 | 38 | 54 | 62 | 71 | 72 | 80 | 82 | 115 | 121 | 127 | 128 | 147 | 155
            );
            w.stats.push(StatShape {
                object_type,
                stat_type_num,
                value_kind: if string { "string" } else { "int" },
            });
            if string {
                w.private()?;
            } else {
                w.ci()?;
            }
            w.ci()?;
            Ok(())
        })
    }
    fn notification(&mut self) -> Result<(), ProtocolError> {
        let code = self.byte()?;
        if !matches!(code, 0..=16 | 20..=23) {
            self.safe_end = 0;
            return Err(ProtocolError::Definition);
        }
        let extra = self.byte()?;
        match code {
            0..=3 | 9 => self.private()?,
            4 => {
                self.private()?;
                self.number(2)?;
            }
            5 => self.fixed(&[4, 2])?,
            6 | 12 => {
                self.private()?;
                self.fixed(&[4, 4])?;
            }
            7 | 8 => {
                self.private()?;
                self.number(4)?;
            }
            10 => {
                self.private()?;
                self.fixed(&[4, 2])?;
            }
            11 if extra != 0 => {
                if extra & 3 != 0 {
                    self.private()?;
                }
                self.fixed(&[4, 2])?;
            }
            13 => self.fixed(&[4, 4])?,
            _ => {}
        }
        Ok(())
    }
    fn packet(&mut self, id: u8) -> Result<(), ProtocolError> {
        match id {
            105 => {}
            90 => self.fixed(&[2, 4])?,
            0 => {
                self.number(4)?;
                self.private()?;
            }
            25 => self.fixed(&[4, 2, 4, 4, 1, 4])?,
            74 => {
                self.number(4)?;
                self.private()?;
                self.private()?;
                self.number(4)?;
                for _ in 0..6 {
                    self.private()?;
                }
            }
            101 => {
                self.fixed(&[4, 4])?;
                self.private()?;
            }
            44 => {
                self.private()?;
                self.fixed(&[4, 2, 1])?;
                for _ in 0..3 {
                    self.private()?;
                }
                self.fixed(&[1, 4])?;
            }
            30 => self.fixed(&[4, 2, 2, 1, 4, 4, 4, 1, 1, 1, 4, 4])?,
            49 => self.fixed(&[2, 4, 4, 4, 1])?,
            75 => {
                self.number(4)?;
                let n = self.byte()?;
                self.list(i32::from(n), 1, |w| w.number(1).map(|_| ()))?;
                self.fixed(&[2, 1, 2, 4])?;
            }
            64 => self.fixed(&[4, 4, 4, 2, 1, 4, 2, 4, 1])?,
            103 => self.fixed(&[4, 4, 4])?,
            45 => {
                self.private()?;
                self.private()?;
                self.fixed(&[2, 4, 4])?;
                self.private()?;
            }
            114 => {
                self.number(2)?;
                for _ in 0..8 {
                    self.ci()?;
                }
            }
            46 => {
                self.private()?;
                self.ci()?;
                self.private()?;
                self.number(4)?;
                self.ci()?;
                let n = self.ci()?;
                self.list(n, 4, |w| {
                    w.private()?;
                    w.ci()?;
                    w.ci()?;
                    Ok(())
                })?;
                self.private()?;
            }
            42 => {
                self.fixed(&[4, 4, 1])?;
                let n = self.ci()?;
                self.list(n, 6, |w| w.fixed(&[2, 2, 2]))?;
                let n = self.ci()?;
                self.list(n, 12, |w| {
                    let kind = w.short()?;
                    w.status(Some(kind))
                })?;
                let n = self.ci()?;
                self.list(n, 1, |w| w.ci().map(|_| ()))?;
            }
            10 => {
                self.fixed(&[4, 4, 4, 2])?;
                let n = self.short()?;
                self.list(i32::from(n), 10, |w| w.status(None))?;
            }
            12 => {
                self.fixed(&[2, 4, 4, 4, 4, 4, 2, 4])?;
                if self.reader.remaining() > 0 {
                    self.number(1)?;
                }
                if self.reader.remaining() > 0 {
                    self.number(1)?;
                }
                if self.reader.remaining() > 0 {
                    self.number(4)?;
                }
            }
            35 => {
                self.fixed(&[2, 4, 1, 4, 4, 4, 2])?;
                if self.reader.remaining() > 0 {
                    self.fixed(&[1, 4])?;
                }
            }
            92 => {
                self.fixed(&[4, 4])?;
                for _ in 0..3 {
                    self.private()?;
                }
                self.fixed(&[4, 4, 4, 1, 1, 1, 2, 4])?;
                self.private()?;
                self.fixed(&[2, 2, 1, 4])?;
                self.private()?;
                self.number(2)?;
                if self.reader.remaining() >= 8 {
                    self.fixed(&[4, 4])?;
                }
            }
            67 => self.notification()?,
            82 => {
                self.number(4)?;
                let n = self.ci()?;
                if !(0..=64).contains(&n) {
                    self.safe_end = 0;
                    return Err(ProtocolError::Definition);
                }
                self.list(n, 1, |w| w.ci().map(|_| ()))?;
            }
            169 => {
                self.number(4)?;
            }
            _ => return Err(ProtocolError::Definition),
        }
        Ok(())
    }
}

pub fn explain_failure(
    id: u8,
    body: &[u8],
    error: ProtocolError,
    alignment_verified: bool,
) -> FailureDetails {
    let mut walk = Walk::new(body);
    let result = walk.packet(id);
    // If the schema ever diverges, suppress the preview rather than treating
    // unclassified bytes as safe. Tests cover every typed packet's prefixes.
    let end = if result == Err(error) {
        walk.safe_end.min(body.len())
    } else {
        0
    };
    let position = walk.reader.position();
    // UPDATE/NEWTICK use only the prefix window: a later numeric failure must
    // not reopen a second hex window after the first string boundary.
    let next_hex = if !alignment_verified || matches!(id, 42 | 10) {
        String::new()
    } else {
        body.get(position..end.min(position.saturating_add(16)))
            .map(crate::lower_hex)
            .unwrap_or_default()
    };
    let mut prefix_hex = String::new();
    if alignment_verified && matches!(id, 42 | 10) && result == Err(error) {
        let mut preview = Walk::new(body);
        // Ignore only the vector's eager size precheck so numeric fields beyond
        // that failure can be classified. Every actual field read stays checked.
        preview.preview = true;
        let _ = preview.packet(id);
        let end = preview.prefix_end.min(position.saturating_add(64));
        prefix_hex = crate::lower_hex(&body[..end]);
    }
    let details = match id {
        67 if error == ProtocolError::Definition => {
            let mut details = if alignment_verified {
                format!(
                    " effectByte={} extraByte={} remainingLength={}",
                    body.first().map_or_else(|| "missing".into(), u8::to_string),
                    body.get(1).map_or_else(|| "missing".into(), u8::to_string),
                    body.len().saturating_sub(2),
                )
            } else {
                format!(" remainingLength={}", body.len().saturating_sub(2))
            };
            if alignment_verified {
                details.push_str(" remainingHex=");
                details.push_str(&if body.len().saturating_sub(2) <= 8 {
                    crate::lower_hex(body.get(2..).unwrap_or_default())
                } else {
                    "redacted".into()
                });
            }
            details
        }
        101 | 114 => numeric_trace(id, body, alignment_verified),
        _ => String::new(),
    };
    FailureDetails {
        id,
        alignment_verified,
        framed_length: body.len() + 5,
        offset: position + 5,
        error,
        stats: walk.stats,
        next_hex,
        prefix_hex,
        details,
    }
}

fn numeric_trace(id: u8, body: &[u8], alignment_verified: bool) -> String {
    use std::fmt::Write;
    let mut reader = Reader::new(body);
    let mut out = String::from(" traceBody=[");
    let fields: &[(&str, usize)] = if id == 101 {
        &[("objectId", 4), ("charId", 4), ("str.length", 2)]
    } else {
        &[
            ("objType", 2),
            ("dexterityProgress", 0),
            ("speedProgress", 0),
            ("vitalityProgress", 0),
            ("wisdomProgress", 0),
            ("defenseProgress", 0),
            ("attackProgress", 0),
            ("manaProgress", 0),
            ("healthProgress", 0),
        ]
    };
    for &(name, width) in fields {
        let start = reader.position();
        let remaining = reader.remaining();
        let value = match width {
            4 => reader.i32(),
            2 if name == "str.length" => reader.u16().map(i32::from),
            2 => reader.i16().map(i32::from),
            _ => reader.ci(),
        };
        let consumed = reader.position() - start;
        let _ = write!(out, "{name}@{start}:width={consumed}");
        match value {
            Ok(value) => {
                if alignment_verified || name == "str.length" {
                    let _ = write!(out, ":value={value};");
                } else {
                    out.push(';');
                }
                if name == "str.length" {
                    let length = value as usize;
                    let _ = write!(
                        out,
                        "str@{}:declared={length}:available={}:contents=redacted;",
                        reader.position(),
                        reader.remaining()
                    );
                    if let Ok(bytes) = reader.take(length) {
                        out.push(']');
                        out.push_str(&pcstats::shape_with_trust(bytes, alignment_verified));
                        return out;
                    }
                    let _ = write!(
                        out,
                        "base64Length={length}:decodedLength=unknown:error=Truncated;"
                    );
                }
            }
            Err(error) => {
                let _ = write!(
                    out,
                    ":expected={}:available={remaining}:error={error:?};",
                    if width == 0 {
                        "ci".to_owned()
                    } else {
                        width.to_string()
                    }
                );
                break;
            }
        }
    }
    out.push(']');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{decode_packet, direction::packet_direction};

    // Existing numeric-preview fixtures model a verified stream.
    fn explain_failure(id: u8, body: &[u8], error: ProtocolError) -> FailureDetails {
        super::explain_failure(id, body, error, true)
    }

    fn compare(id: u8, body: &[u8]) {
        let dir = packet_direction(id).unwrap();
        let actual = decode_packet(id, dir, body).map(|(_, left)| left);
        let mut walk = Walk::new(body);
        let structural = walk.packet(id).map(|_| walk.reader.remaining());
        assert_eq!(structural, actual, "id={id} len={}", body.len());
    }

    #[test]
    fn structural_walk_matches_all_typed_decoders_and_notification_branches() {
        for id in [
            105, 90, 0, 25, 74, 101, 44, 30, 49, 75, 64, 103, 45, 114, 46, 42, 10, 12, 35, 92, 67,
            82, 169,
        ] {
            for byte in [0, 1, 31, 63, 64, 127, 128, 255] {
                let body = [byte; 160];
                for len in 0..=body.len() {
                    compare(id, &body[..len]);
                }
            }
        }
        for code in 0..=255u8 {
            for extra in [0, 1, 2, 4, 255] {
                let mut body = vec![code, extra, 0, 5];
                body.extend(b"dummy");
                body.extend([0; 16]);
                for len in 0..=body.len() {
                    compare(67, &body[..len]);
                }
            }
        }
    }

    #[test]
    fn update_reports_shapes_without_string_contents_and_pins_frame_offsets() {
        let mut body = vec![0; 9]; // position, levelType
        body.extend([0, 1, 0x12, 0x34, 1]); // tiles, objects, objectType, objectId
        body.extend([0; 8]); // position
        body.extend([2, 20, 1, 0, 31, 0, 80]); // two stats, attack then a truncated name
        body.extend(b"SYNTHETIC_SECRET");
        let error = decode_packet(42, maxed_types::Direction::ServerToClient, &body)
            .err()
            .unwrap();
        let failure = explain_failure(42, &body, error);
        assert_eq!(failure.offset, 34);
        assert_eq!(failure.error, ProtocolError::Truncated);
        assert_eq!(
            failure.stats,
            vec![
                StatShape {
                    object_type: Some(0x1234),
                    stat_type_num: 20,
                    value_kind: "int"
                },
                StatShape {
                    object_type: Some(0x1234),
                    stat_type_num: 31,
                    value_kind: "string"
                },
            ]
        );
        assert!(failure.next_hex.is_empty());
        assert_eq!(failure.prefix_hex, crate::lower_hex(&body[..27]));
        assert_eq!(failure.to_string(), format!("name=UPDATE id=42 len={} offset=34 error=Truncated stats=[(4660,20,int),(4660,31,string)] nextHex= bodyOffset=29 prefixHex={} prefixEnd=27 prefixSchema=current", body.len() + 5, crate::lower_hex(&body[..27])));
        for end in 0..=body.len() {
            compare(42, &body[..end]);
        }
    }

    #[test]
    fn newtick_has_no_wire_object_type_and_compressed_failure_does_not_print_values() {
        let mut body = vec![0; 14];
        body.extend([0, 1, 1]);
        body.extend([0; 8]);
        body.extend([1, 20, 255, 255, 255, 255, 255, 42]);
        let error = decode_packet(10, maxed_types::Direction::ServerToClient, &body)
            .err()
            .unwrap();
        assert_eq!(error, ProtocolError::CompressedOverflow);
        let failure = explain_failure(10, &body, error);
        assert_eq!(failure.offset, 37);
        assert_eq!(
            failure.stats,
            vec![StatShape {
                object_type: None,
                stat_type_num: 20,
                value_kind: "int"
            }]
        );
        assert!(failure.next_hex.is_empty());
        for end in 0..=body.len() {
            compare(10, &body[..end]);
        }
    }

    #[test]
    fn notification_strings_and_unclassified_tails_never_appear_even_as_hex() {
        for body in [
            b"\x06\x00\x00\x40SYNTHETIC_SECRET".to_vec(),
            b"\xffSYNTHETIC_SECRET".to_vec(),
        ] {
            let error = decode_packet(67, maxed_types::Direction::ServerToClient, &body)
                .err()
                .unwrap();
            let failure = explain_failure(67, &body, error);
            assert!(failure.next_hex.is_empty());
            assert!(!failure.to_string().contains("SYNTHETIC"));
            assert!(!failure
                .to_string()
                .contains(&crate::lower_hex(b"SYNTHETIC_SECRET")));
        }
        // A partial fixed-width numeric field is safe; do not read beyond it.
        let failure = explain_failure(67, &[5, 0, 0x12, 0x34], ProtocolError::Truncated);
        assert_eq!(failure.offset, 7);
        assert_eq!(failure.next_hex, "1234");
    }

    #[test]
    fn preview_crosses_eager_list_check_but_stops_before_strings_and_unknown_tails() {
        let mut body = vec![0; 9];
        body.push(0x7f); // negative tile count; nothing after it has a known boundary
        body.extend(b"SECRET");
        let failure = explain_failure(42, &body, ProtocolError::Length);
        assert_eq!(failure.prefix_hex, crate::lower_hex(&body[..10]));

        let mut body = vec![0; 9];
        body.push(63); // eager list rejects, but each complete tile is numeric
        body.extend([0x12; 100]);
        let failure = explain_failure(42, &body, ProtocolError::Length);
        assert_eq!(failure.offset, 15);
        assert_eq!(failure.prefix_hex, crate::lower_hex(&body[..74]));

        // A stats count precheck fails before reaching a truncated string.
        let mut body = vec![0; 16];
        body[15] = 1;
        body.push(1); // objectId
        body.extend([0; 8]);
        body.extend([63, 31, 0, 6]);
        body.extend(b"SECRET");
        let failure = explain_failure(10, &body, ProtocolError::Length);
        assert_eq!(failure.prefix_hex, crate::lower_hex(&body[..27]));
        assert!(!failure.to_string().contains(&crate::lower_hex(b"SECRET")));
    }

    #[test]
    fn preview_never_resumes_after_a_complete_string() {
        let mut body = vec![0; 9];
        body.extend([0, 1, 0x12, 0x34, 1]);
        body.extend([0; 8]);
        body.extend([2, 31, 0, 6]);
        body.extend(b"SECRET");
        body.extend([0, 20, 0xff, 0xff, 0xff, 0xff, 0xff]);
        let failure = explain_failure(42, &body, ProtocolError::CompressedOverflow);
        assert_eq!(failure.prefix_hex, crate::lower_hex(&body[..24]));
        assert!(!failure.to_string().contains(&crate::lower_hex(b"SECRET")));
    }

    #[test]
    fn every_string_stat_kind_closes_the_numeric_window() {
        for id in [
            6, 31, 38, 54, 62, 71, 72, 80, 82, 115, 121, 127, 128, 147, 155,
        ] {
            let mut body = vec![0; 16];
            body[15] = 1;
            body.push(1);
            body.extend([0; 8]);
            body.extend([1, id, 0, 80]);
            body.extend(b"SYNTHETIC_SECRET");
            let error = decode_packet(10, maxed_types::Direction::ServerToClient, &body)
                .err()
                .unwrap();
            let failure = explain_failure(10, &body, error);
            assert_eq!(failure.prefix_hex, crate::lower_hex(&body[..27]));
            assert!(failure.next_hex.is_empty());
        }
    }

    #[test]
    fn notification_effect_and_extra_are_unsigned_even_for_unknown_effects() {
        let failure = explain_failure(67, &[255, 254, 0, 0], ProtocolError::Definition);
        assert!(failure.to_string().contains("len=9"));
        assert!(failure.to_string().contains("effectByte=255 extraByte=254"));
        assert!(explain_failure(67, &[255], ProtocolError::Definition)
            .to_string()
            .contains("effectByte=255 extraByte=missing"));
    }

    #[test]
    fn unknown_notification_tail_is_reported_only_through_eight_bytes() {
        for len in 0..=9 {
            let mut body = vec![24, 32];
            body.extend(vec![0xab; len]);
            let text = explain_failure(67, &body, ProtocolError::Definition).to_string();
            assert!(text.contains(&format!("remainingLength={len}")));
            if len <= 8 {
                assert!(text.ends_with(&format!("remainingHex={}", "ab".repeat(len))));
            } else {
                assert!(text.ends_with("remainingHex=redacted"));
                assert!(!text.contains("abab"));
            }
        }
    }

    #[test]
    fn truncation_trace_names_fields_and_widths_without_string_values() {
        let mut body = 123i32.to_be_bytes().to_vec();
        body.extend(456i32.to_be_bytes());
        body.extend([0, 80]);
        body.extend(b"SYNTHETIC_SECRET");
        let text = explain_failure(101, &body, ProtocolError::Truncated).to_string();
        assert!(text.contains("objectId@0:width=4:value=123"));
        assert!(text.contains("charId@4:width=4:value=456"));
        assert!(text.contains("str.length@8:width=2:value=80"));
        assert!(text.contains("base64Length=80:decodedLength=unknown:error=Truncated"));
        assert!(!text.contains("SYNTHETIC_SECRET"));
        assert!(!text.contains(&crate::lower_hex(b"SYNTHETIC_SECRET")));
        let body = [0x12, 0x34, 0x81, 1, 2, 3, 4, 5, 6, 0x80];
        let text = explain_failure(114, &body, ProtocolError::Truncated).to_string();
        assert!(text.contains("objType@0:width=2:value=4660"));
        assert!(text.contains("dexterityProgress@2:width=2:value=65"));
        assert!(text.contains("manaProgress@9:width=1:expected=ci:available=1:error=Truncated"));
        assert!(!text.contains("healthProgress@"));
    }
}
