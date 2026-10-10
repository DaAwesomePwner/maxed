//! Bounded, path-free native update contracts.
use crate::UpdateChannel;
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

pub const UPDATE_ARTIFACT_LIMIT: u32 = 20_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    Available,
    Downloading,
    Ready,
    Installing,
    UpToDate,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UpdateFailure {
    Offline,
    Unavailable,
    InvalidMetadata,
    InvalidSignature,
    UnsupportedTarget,
    TooLarge,
    Busy,
    StaleOffer,
    ShutdownBlocked,
    InstallFailed,
}

#[derive(Debug, thiserror::Error)]
#[error("Update contract is invalid.")]
pub struct UpdateContractError;

pub fn valid_update_request_id(value: &str) -> bool {
    (1..=20).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit())
}
fn request_id<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    if !valid_update_request_id(&value) {
        return Err(serde::de::Error::custom(UpdateContractError));
    }
    Ok(value)
}
fn version<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    if value.len() > 128 || semver::Version::parse(&value).is_err() {
        return Err(serde::de::Error::custom(UpdateContractError));
    }
    Ok(value)
}
fn notes<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    if value.len() > 8192 {
        return Err(serde::de::Error::custom(UpdateContractError));
    }
    Ok(value)
}
fn bytes<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let value = u32::deserialize(d)?;
    if value > UPDATE_ARTIFACT_LIMIT {
        return Err(serde::de::Error::custom(UpdateContractError));
    }
    Ok(value)
}
fn total<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
    let value = Option::<u32>::deserialize(d)?;
    if value.is_some_and(|v| v > UPDATE_ARTIFACT_LIMIT) {
        return Err(serde::de::Error::custom(UpdateContractError));
    }
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UpdateOffer {
    #[serde(deserialize_with = "request_id")]
    pub request_id: String,
    #[serde(deserialize_with = "version")]
    pub version: String,
    pub channel: UpdateChannel,
    #[serde(deserialize_with = "notes")]
    pub notes: String,
}
impl UpdateOffer {
    pub fn new(
        request_id: String,
        version: String,
        channel: UpdateChannel,
        notes: String,
    ) -> Result<Self, UpdateContractError> {
        if !valid_update_request_id(&request_id)
            || version.len() > 128
            || semver::Version::parse(&version).is_err()
            || notes.len() > 8192
        {
            return Err(UpdateContractError);
        }
        Ok(Self {
            request_id,
            version,
            channel,
            notes,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UpdateStatus {
    pub phase: UpdatePhase,
    pub channel: UpdateChannel,
    pub offer: Option<UpdateOffer>,
    #[serde(deserialize_with = "bytes")]
    pub downloaded_bytes: u32,
    #[serde(deserialize_with = "total")]
    pub total_bytes: Option<u32>,
    pub failure: Option<UpdateFailure>,
}
impl UpdateStatus {
    pub fn idle(channel: UpdateChannel) -> Self {
        Self {
            phase: UpdatePhase::Idle,
            channel,
            offer: None,
            downloaded_bytes: 0,
            total_bytes: None,
            failure: None,
        }
    }
}
