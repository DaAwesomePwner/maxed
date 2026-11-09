#![forbid(unsafe_code)]

pub mod alerts;
pub mod app;
pub mod catalog;
mod chat;
pub mod demo;
pub mod domain;
pub mod dungeon_meta;
mod finite;
pub mod live;
mod loot;
pub mod party;
pub mod players;
pub mod queries;
mod recap;
pub mod recording;
mod stats;
mod storage;
pub mod updates;

pub use alerts::*;
pub use app::*;
pub use catalog::*;
pub use chat::*;
pub use domain::*;
pub use dungeon_meta::*;
pub use live::*;
pub use loot::*;
pub use party::*;
pub use players::*;
pub use queries::*;
pub use recap::*;
pub use recording::*;
pub use stats::*;
pub use storage::*;
pub use updates::*;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, TS)]
pub struct Timestamp(#[ts(type = "string")] pub u64);
impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse::<u64>()
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum Direction {
    ClientToServer,
    ServerToClient,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConnId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Endpoint {
    pub ip: [u8; 4],
    pub port: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FlowKey {
    pub client: Endpoint,
    pub server: Endpoint,
}
#[derive(Clone)]
pub struct RawFrame {
    pub index: u64,
    pub at: Timestamp,
    pub original_len: u32,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ByteOrigin {
    pub frame_index: u64,
    pub offset: usize,
    pub len: usize,
}
#[derive(Clone)]
pub struct TcpSegment {
    pub flow: FlowKey,
    pub dir: Direction,
    pub at: Timestamp,
    pub seq: u32,
    pub syn: bool,
    pub fin: bool,
    pub rst: bool,
    pub bytes: Vec<u8>,
    pub origin: ByteOrigin,
}
#[derive(Clone)]
pub struct StreamChunk {
    pub conn: ConnId,
    pub dir: Direction,
    pub at: Timestamp,
    pub bytes: Vec<u8>,
    pub origins: Vec<ByteOrigin>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GapReason {
    CaptureDrop,
    Truncated,
    ReorderLimit,
    ConflictingOverlap,
    InvalidFrame,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    Fin,
    Reset,
    Replaced,
    Stopped,
}
#[derive(Clone)]
pub enum StreamEvent {
    Opened {
        conn: ConnId,
        dir: Direction,
        at: Timestamp,
        /// Initialize a fresh directional cipher and bypass the startup gate:
        /// a SYN was seen, an earlier RST left a pending directional reset, or
        /// this flow replaces the previous connection in this direction.
        /// Matches Java TcpStreamBuilder's constructor resets; this does not
        /// by itself prove that the connection's SYN was captured.
        fresh: bool,
        /// A SYN was captured for this direction. Unlike `fresh`, reset or
        /// replacement assumptions do not establish this provenance.
        syn_proven: bool,
    },
    Data(StreamChunk),
    Gap {
        conn: ConnId,
        dir: Direction,
        at: Timestamp,
        reason: GapReason,
    },
    Closed {
        conn: ConnId,
        dir: Direction,
        at: Timestamp,
        reason: CloseReason,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum CaptureState {
    Stopped,
    NpcapMissing,
    NpcapIncompatible,
    PermissionDenied,
    WaitingForGame,
    Live,
    Recovering,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CaptureStatus {
    pub state: CaptureState,
    pub message: String,
    pub dropped_frames: u32,
    pub unknown_packets: u32,
    pub decode_failures: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<PacketDiagnosticCount>>", optional)]
    pub packet_diagnostics: Vec<PacketDiagnosticCount>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PacketDiagnosticCount {
    pub packet_id: u8,
    pub unknown_packets: u32,
    pub field_failures: u32,
}
impl Default for CaptureStatus {
    fn default() -> Self {
        Self {
            state: CaptureState::Stopped,
            message: "Capture stopped".into(),
            dropped_frames: 0,
            unknown_packets: 0,
            decode_failures: 0,
            packet_diagnostics: vec![],
        }
    }
}

mod account_history;
pub use account_history::*;
