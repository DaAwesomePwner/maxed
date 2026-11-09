//! Presentation transport only. Query snapshots are always complete baselines.
/// Maximum ordered presentation events per IPC message.
pub const EVENT_BATCH_LIMIT: usize = 1024;

use crate::{LiveSnapshot, Sequence};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct LiveDelta {
    // The exact preceding snapshot in this stream, never an event cursor.
    pub base_sequence: Sequence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub base_revision: Option<Sequence>,
    // Metadata replaces the baseline; fights/drops contain upserts only.
    pub snapshot: LiveSnapshot,
    pub removed_fights: Vec<String>,
    pub removed_drops: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum LiveSnapshotUpdate {
    Delta(LiveDelta),
    Full(LiveSnapshot),
}
