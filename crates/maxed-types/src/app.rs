//! Application contracts. Local settings never belong in domain events.

use crate::{BagDef, SpriteRef};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NpcapCheck {
    Unchecked,
    Compatible,
    Missing,
    Incompatible,
    PermissionDenied,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CaptureAccess {
    Unknown,
    Allowed,
    Denied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AssetCheck {
    Missing,
    Finding,
    Extracting,
    Ready,
    Unsupported,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionStage {
    Fingerprinting,
    Reading,
    Catalog,
    Sprites,
    Publishing,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SetupStatus {
    pub npcap: NpcapCheck,
    pub access: CaptureAccess,
    pub assets: AssetCheck,
    pub extraction_stage: Option<ExtractionStage>,
    pub client_found: bool,
    pub live_confirmed: bool,
    pub demo: bool,
    pub message: String,
}

impl SetupStatus {
    /// Only the native owner supplies decoded live-traffic evidence. Asset
    /// readiness and synthetic playback can never supply that evidence.
    pub fn confirm_live(&mut self, decoded_live_traffic: bool) {
        self.live_confirmed = decoded_live_traffic && !self.demo;
    }
}

impl Default for SetupStatus {
    fn default() -> Self {
        Self {
            npcap: NpcapCheck::Unchecked,
            access: CaptureAccess::Unknown,
            assets: AssetCheck::Missing,
            extraction_stage: None,
            client_found: false,
            live_confirmed: false,
            demo: false,
            message: "Setup has not been checked".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AdapterInfo {
    pub id: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Stable,
    Beta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default, deny_unknown_fields)]
pub struct RecapImageSettings {
    pub names: bool,
    pub loot: bool,
    pub fights: bool,
}

impl Default for RecapImageSettings {
    fn default() -> Self {
        Self {
            names: true,
            loot: true,
            fights: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AppSettings {
    #[serde(default)]
    pub alerts: crate::AlertPreferences,
    #[serde(default)]
    pub loot: crate::LootSettings,
    #[serde(default)]
    pub chat: crate::ChatSettings,
    #[serde(default)]
    pub party: crate::PartySettings,
    #[serde(default)]
    pub recap_image: RecapImageSettings,
    #[serde(deserialize_with = "settings_version")]
    pub version: u32,
    pub adapter_id: Option<String>,
    pub assets_path: Option<String>,
    pub save_recordings: bool,
    #[serde(default)]
    pub hardware_acceleration: bool,
    #[serde(deserialize_with = "recording_limit")]
    pub recording_limit_bytes: u32,
    pub update_channel: UpdateChannel,
}

fn settings_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(deserializer)?;
    if version != 1 {
        return Err(serde::de::Error::custom("Unsupported settings version"));
    }
    Ok(version)
}

fn recording_limit<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let limit = u32::deserialize(deserializer)?;
    if !(1..=1_000_000_000).contains(&limit) {
        return Err(serde::de::Error::custom("Invalid recording limit"));
    }
    Ok(limit)
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            alerts: crate::AlertPreferences::default(),
            loot: crate::LootSettings::default(),
            chat: crate::ChatSettings::default(),
            party: crate::PartySettings::default(),
            recap_image: RecapImageSettings::default(),
            version: 1,
            adapter_id: None,
            assets_path: None,
            save_recordings: false,
            hardware_acceleration: false,
            recording_limit_bytes: 1_000_000_000,
            update_channel: UpdateChannel::Stable,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ObjectVisual {
    pub object_type: u16,
    pub name: Option<String>,
    pub sprite: Option<SpriteRef>,
    pub bag: Option<BagDef>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub item_tier: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct VisualPage {
    pub client_fingerprint: String,
    pub objects: Vec<ObjectVisual>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ItemDetail {
    pub object_type: u16,
    pub name: Option<String>,
    pub tier: Option<String>,
    pub slot_type: Option<u8>,
    pub slot_label: Option<String>,
    pub soulbound: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EnchantInfo {
    pub id: i32,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ItemDetailPage {
    pub client_fingerprint: String,
    pub items: Vec<ItemDetail>,
    pub enchants: Vec<EnchantInfo>,
}
