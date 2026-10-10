use maxed_types::{ConnId, Direction, Timestamp};
pub struct DecodedPacket {
    pub conn: ConnId,
    pub dir: Direction,
    pub at: Timestamp,
    pub id: u8,
    pub len: usize,
    /// Cipher alignment has SYN provenance or consecutive-tick evidence in
    /// this direction. False packets are retained for Java export parity;
    /// consumers must not treat them as reliable gameplay evidence.
    pub alignment_verified: bool,
    pub packet: crate::Packet,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diagnostic {
    /// Numeric context accompanying an existing aggregate diagnostic. Keeping
    /// the original event preserves packet-export and adapter totals.
    Packet {
        id: u8,
        problem: PacketDiagnostic,
        alignment_verified: bool,
    },
    Recovering,
    Recovered,
    BadFrame,
    BadFields,
    TrailingBytes,
    UnknownId,
    WrongDirection,
    Closed,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketDiagnostic {
    UnknownId,
    BadFields,
    TrailingBytes,
    WrongDirection,
}
// Packets are the hot path and diagnostics are rare; boxing every packet would add an allocation per frame.
#[allow(clippy::large_enum_variant)]
pub enum DecodeEvent {
    Packet(DecodedPacket),
    Diagnostic {
        conn: ConnId,
        dir: Direction,
        at: Timestamp,
        kind: Diagnostic,
    },
}

use crate::explanation::{ReportContext, StreamReportEvent, StructuralReport};
use crate::{decode_packet, Alignment, Framer};
use maxed_types::StreamEvent;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
struct StreamState {
    framer: Framer,
    alignment: Alignment,
    gate: bool,
    packets_since_recovered: Option<u64>,
    alignment_verified: bool,
    verification_tick: Option<i32>,
}
impl StreamState {
    fn lose_verification(&mut self) {
        self.alignment_verified = false;
        self.verification_tick = None;
    }

    fn observe_verification_tick(&mut self, id: u8, dir: Direction, body: &[u8]) {
        let expected = match dir {
            Direction::ServerToClient => 10, // NEWTICK
            Direction::ClientToServer => 62, // MOVE
        };
        if id != expected || self.alignment_verified {
            return;
        }
        if let Some(head) = body.get(..4) {
            let tick = i32::from_be_bytes([head[0], head[1], head[2], head[3]]);
            self.alignment_verified = self
                .verification_tick
                .is_some_and(|previous| previous.wrapping_add(1) == tick);
            self.verification_tick = Some(tick);
        } else {
            self.verification_tick = None;
        }
    }
}
pub struct Decoder {
    streams: HashMap<(ConnId, Direction), StreamState>,
    cancel: Arc<AtomicBool>,
    terminated: bool,
    explain: bool,
    failures: Vec<crate::explanation::StructuralFailure>,
    packet_ordinal: u64,
    last_newticks: HashMap<ConnId, i32>,
}
impl Default for Decoder {
    fn default() -> Self {
        Self::with_cancellation(Arc::new(AtomicBool::new(false)))
    }
}
impl Decoder {
    pub fn with_cancellation(cancel: Arc<AtomicBool>) -> Self {
        Self {
            streams: HashMap::new(),
            cancel,
            terminated: false,
            explain: false,
            failures: Vec::new(),
            packet_ordinal: 0,
            last_newticks: HashMap::new(),
        }
    }
    pub fn explain_failures(&mut self, enabled: bool) {
        self.explain = enabled;
    }
    pub fn take_failures(&mut self) -> Vec<crate::explanation::StructuralFailure> {
        std::mem::take(&mut self.failures)
    }
    pub fn push(&mut self, event: StreamEvent) -> Vec<DecodeEvent> {
        let (conn, dir, at) = match &event {
            StreamEvent::Opened { conn, dir, at, .. }
            | StreamEvent::Gap { conn, dir, at, .. }
            | StreamEvent::Closed { conn, dir, at, .. } => (*conn, *dir, *at),
            StreamEvent::Data(c) => (c.conn, c.dir, c.at),
        };
        let diagnostic = |kind| DecodeEvent::Diagnostic {
            conn,
            dir,
            at,
            kind,
        };
        let context = |packet_ordinal, last_newtick, packets_since_recovered| ReportContext {
            packet_ordinal,
            conn,
            dir,
            at,
            last_newtick,
            packets_since_recovered,
        };
        if self.terminated {
            return vec![];
        }
        if self.cancel.load(Ordering::Relaxed) {
            self.terminated = true;
            self.streams.clear();
            self.last_newticks.clear();
            return vec![diagnostic(Diagnostic::Cancelled)];
        }
        match event {
            StreamEvent::Opened {
                fresh, syn_proven, ..
            } => {
                if self.streams.contains_key(&(conn, dir)) {
                    self.last_newticks.remove(&conn);
                }
                match Alignment::with_cancellation(dir, fresh, self.cancel.clone()) {
                    Ok(alignment) => {
                        self.streams.insert(
                            (conn, dir),
                            StreamState {
                                framer: Framer::default(),
                                alignment,
                                gate: !fresh,
                                packets_since_recovered: None,
                                alignment_verified: fresh && syn_proven,
                                verification_tick: None,
                            },
                        );
                    }
                    Err(_) => return vec![diagnostic(Diagnostic::BadFields)],
                }
                if self.explain {
                    let context = context(
                        self.packet_ordinal,
                        self.last_newticks.get(&conn).copied(),
                        None,
                    );
                    self.failures.push(StructuralReport::Stream {
                        context,
                        event: StreamReportEvent::Opened { fresh },
                    });
                    if !fresh {
                        self.failures.push(StructuralReport::Stream {
                            context,
                            event: StreamReportEvent::Recovering,
                        });
                    }
                }
                if fresh {
                    vec![]
                } else {
                    vec![diagnostic(Diagnostic::Recovering)]
                }
            }
            StreamEvent::Closed { .. } => {
                if self.explain {
                    self.failures.push(StructuralReport::Stream {
                        context: context(
                            self.packet_ordinal,
                            self.last_newticks.get(&conn).copied(),
                            None,
                        ),
                        event: StreamReportEvent::Closed,
                    });
                }
                self.streams.remove(&(conn, dir));
                if !self.streams.keys().any(|(id, _)| *id == conn) {
                    self.last_newticks.remove(&conn);
                }
                vec![diagnostic(Diagnostic::Closed)]
            }
            StreamEvent::Gap { .. } => {
                if let Some(s) = self.streams.get_mut(&(conn, dir)) {
                    s.framer.reset();
                    s.gate = true;
                    s.packets_since_recovered = None;
                    s.lose_verification();
                    if let Ok(a) = Alignment::with_cancellation(dir, false, self.cancel.clone()) {
                        s.alignment = a;
                    }
                }
                if self.explain {
                    self.failures.push(StructuralReport::Stream {
                        context: context(
                            self.packet_ordinal,
                            self.last_newticks.get(&conn).copied(),
                            None,
                        ),
                        event: StreamReportEvent::Recovering,
                    });
                }
                vec![diagnostic(Diagnostic::Recovering)]
            }
            StreamEvent::Data(chunk) => {
                let Some(s) = self.streams.get_mut(&(conn, dir)) else {
                    return vec![];
                };
                if s.gate {
                    if chunk.bytes.len() < 1460 {
                        s.gate = false;
                    }
                    return vec![];
                }
                let framed = s.framer.push(&chunk.bytes);
                let mut out = Vec::new();
                for f in framed.frames {
                    let packet_ordinal = self.packet_ordinal;
                    self.packet_ordinal = self.packet_ordinal.saturating_add(1);
                    if let Some(count) = &mut s.packets_since_recovered {
                        *count = count.saturating_add(1);
                    }
                    let was_synced = s.alignment.is_synced();
                    let id = f[4];
                    let plain = s.alignment.consume(id, &f[5..]);
                    if self.cancel.load(Ordering::Relaxed) {
                        self.terminated = true;
                        out.push(diagnostic(Diagnostic::Cancelled));
                        break;
                    }
                    let synced = s.alignment.is_synced();
                    if was_synced && !synced {
                        s.lose_verification();
                        s.packets_since_recovered = None;
                        if self.explain {
                            self.failures.push(StructuralReport::Stream {
                                context: context(
                                    packet_ordinal,
                                    self.last_newticks.get(&conn).copied(),
                                    None,
                                ),
                                event: StreamReportEvent::Recovering,
                            });
                        }
                        out.push(diagnostic(Diagnostic::Recovering));
                    }
                    if !was_synced && synced {
                        s.alignment_verified = false;
                        // A search match is only a candidate. Require the next
                        // consecutive directional tick before exposing trust.
                        s.verification_tick = s.alignment.current_tick();
                        s.packets_since_recovered = Some(0);
                        if self.explain {
                            self.failures.push(StructuralReport::Stream {
                                context: context(
                                    packet_ordinal,
                                    self.last_newticks.get(&conn).copied(),
                                    Some(0),
                                ),
                                event: StreamReportEvent::Recovered,
                            });
                        }
                        out.push(diagnostic(Diagnostic::Recovered));
                    }
                    match plain {
                        Ok(Some(body)) => {
                            s.observe_verification_tick(id, dir, &body);
                            match decode_packet(id, dir, &body) {
                                Ok((packet, left)) => {
                                    if let crate::Packet::NewTick(tick) = &packet {
                                        self.last_newticks.insert(conn, tick.tickId);
                                    }
                                    if let crate::Packet::CreateSuccess(value) = &packet {
                                        if self.explain {
                                            self.failures.push(StructuralReport::CreateSuccess {
                                                context: context(
                                                    packet_ordinal,
                                                    self.last_newticks.get(&conn).copied(),
                                                    s.packets_since_recovered,
                                                ),
                                                details: crate::explanation::explain_create_success(
                                                    &value.str,
                                                    f.len(),
                                                    s.alignment_verified && synced,
                                                ),
                                            });
                                        }
                                    }
                                    // Canonical JSON rules belong to private export, not live decoding.
                                    let classification =
                                        match crate::direction::packet_direction(id) {
                                            None => Some(Diagnostic::UnknownId),
                                            Some(expected) if expected != dir => {
                                                Some(Diagnostic::WrongDirection)
                                            }
                                            _ => None,
                                        };
                                    out.push(DecodeEvent::Packet(DecodedPacket {
                                        conn,
                                        dir,
                                        at,
                                        id,
                                        len: f.len(),
                                        alignment_verified: s.alignment_verified,
                                        packet,
                                    }));
                                    if let Some(kind) = classification {
                                        out.push(diagnostic(kind));
                                        out.push(diagnostic(Diagnostic::Packet {
                                            id,
                                            problem: if kind == Diagnostic::UnknownId {
                                                PacketDiagnostic::UnknownId
                                            } else {
                                                PacketDiagnostic::WrongDirection
                                            },
                                            alignment_verified: s.alignment_verified,
                                        }));
                                    }
                                    if left > 0 {
                                        out.push(diagnostic(Diagnostic::TrailingBytes));
                                        out.push(diagnostic(Diagnostic::Packet {
                                            id,
                                            problem: PacketDiagnostic::TrailingBytes,
                                            alignment_verified: s.alignment_verified,
                                        }));
                                    }
                                }
                                Err(error) => {
                                    if self.explain {
                                        self.failures.push(StructuralReport::Failure {
                                            context: context(
                                                packet_ordinal,
                                                self.last_newticks.get(&conn).copied(),
                                                s.packets_since_recovered,
                                            ),
                                            failure: crate::explanation::explain_failure(
                                                id,
                                                &body,
                                                error,
                                                s.alignment_verified && synced,
                                            ),
                                        });
                                    }
                                    out.push(diagnostic(Diagnostic::BadFields));
                                    out.push(diagnostic(Diagnostic::Packet {
                                        id,
                                        problem: PacketDiagnostic::BadFields,
                                        alignment_verified: s.alignment_verified,
                                    }));
                                }
                            }
                        }
                        Ok(None) => {}
                        Err(_) => out.push(diagnostic(Diagnostic::BadFields)),
                    }
                }
                if framed.error.is_some() && !self.terminated {
                    // Preserve completed packets and their cipher advancement before recovery.
                    s.framer.reset();
                    s.gate = true;
                    s.packets_since_recovered = None;
                    s.lose_verification();
                    if let Ok(a) = Alignment::with_cancellation(dir, false, self.cancel.clone()) {
                        s.alignment = a;
                    }
                    out.push(diagnostic(Diagnostic::BadFrame));
                    out.push(diagnostic(Diagnostic::Recovering));
                    if self.explain {
                        self.failures.push(StructuralReport::Stream {
                            context: context(
                                self.packet_ordinal,
                                self.last_newticks.get(&conn).copied(),
                                None,
                            ),
                            event: StreamReportEvent::Recovering,
                        });
                    }
                }
                if self.terminated {
                    self.streams.clear();
                    self.last_newticks.clear();
                }
                out
            }
        }
    }
}
