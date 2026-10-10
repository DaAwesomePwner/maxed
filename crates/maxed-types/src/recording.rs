//! Recording status contains no paths or frame content.
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RecordingState {
    Disabled,
    Armed,
    Writing,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RecordingFailure {
    InUse,
    Quota,
    QueueFull,
    Disk,
    InvalidFrame,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecordingStatus {
    pub state: RecordingState,
    #[serde(deserialize_with = "limit")]
    pub limit_bytes: u32,
    #[serde(deserialize_with = "retained")]
    pub retained_bytes: u32,
    // Frames skipped under queue pressure or during live recording cap swaps.
    // Monotonic across cap swaps; retained after stop, reset for the next recording.
    #[serde(default)]
    #[ts(type = "number")]
    pub dropped_frames: u64,
    pub failure: Option<RecordingFailure>,
}
fn limit<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(d)?;
    if !(1..=1_000_000_000).contains(&value) {
        return Err(serde::de::Error::custom("Invalid recording limit."));
    }
    Ok(value)
}
fn retained<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(d)?;
    if value > 1_000_000_000 {
        return Err(serde::de::Error::custom("Invalid recording usage."));
    }
    Ok(value)
}
