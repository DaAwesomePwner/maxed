use crate::{AlertKind, AlertView, TimeRange, Timestamp};
use serde::{Deserialize, Deserializer, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AlertLoudness {
    Off,
    QuietLogOnly,
    Toast,
    Banner,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AlertPreferences {
    pub white_bag: AlertLoudness,
    pub personal_best: AlertLoudness,
    #[serde(default = "key_pop_off")]
    pub key_popped: AlertLoudness,
    // Missing legacy filters still mean all dungeons; retain explicitly saved delivery choices.
    #[serde(default)]
    pub key_pop_dungeons: Option<Vec<String>>,
    #[serde(default = "chat_toast")]
    pub whisper: AlertLoudness,
    #[serde(default = "chat_quiet")]
    pub guild_message: AlertLoudness,
    #[serde(default = "chat_quiet")]
    pub party_message: AlertLoudness,
    #[serde(default)]
    pub key_pop_excluded_dungeons: Vec<String>,
    #[serde(deserialize_with = "flawless_off")]
    pub flawless_run: AlertLoudness,
    // Legacy preference retained for saved settings; ignored by delivery.
    #[serde(default = "legacy_death_off")]
    pub death: AlertLoudness,
    pub muted: bool,
    pub windows_notifications: bool,
    #[serde(deserialize_with = "volume")]
    pub volume_percent: u8,
}
fn chat_toast() -> AlertLoudness {
    AlertLoudness::Toast
}
fn chat_quiet() -> AlertLoudness {
    AlertLoudness::QuietLogOnly
}
fn key_pop_off() -> AlertLoudness {
    AlertLoudness::Off
}
fn legacy_death_off() -> AlertLoudness {
    AlertLoudness::Off
}
fn volume<'de, D: Deserializer<'de>>(d: D) -> Result<u8, D::Error> {
    let v = u8::deserialize(d)?;
    if v > 100 {
        return Err(serde::de::Error::custom("Volume must be 0..=100"));
    }
    Ok(v)
}
fn flawless_off<'de, D: Deserializer<'de>>(d: D) -> Result<AlertLoudness, D::Error> {
    let v = AlertLoudness::deserialize(d)?;
    if v != AlertLoudness::Off {
        return Err(serde::de::Error::custom("Flawless is unavailable"));
    }
    Ok(v)
}
impl Default for AlertPreferences {
    fn default() -> Self {
        Self {
            white_bag: AlertLoudness::Banner,
            personal_best: AlertLoudness::Banner,
            key_popped: AlertLoudness::Off,
            key_pop_dungeons: Some(Vec::new()),
            whisper: AlertLoudness::Toast,
            guild_message: AlertLoudness::QuietLogOnly,
            party_message: AlertLoudness::QuietLogOnly,
            key_pop_excluded_dungeons: Vec::new(),
            flawless_run: AlertLoudness::Off,
            death: AlertLoudness::Off,
            muted: false,
            windows_notifications: false,
            volume_percent: 70,
        }
    }
}
impl AlertPreferences {
    pub fn disabled() -> Self {
        Self {
            white_bag: AlertLoudness::Off,
            personal_best: AlertLoudness::Off,
            key_popped: AlertLoudness::Off,
            whisper: AlertLoudness::Off,
            guild_message: AlertLoudness::Off,
            party_message: AlertLoudness::Off,
            death: AlertLoudness::Off,
            ..Self::default()
        }
    }
    pub fn valid(&self) -> bool {
        self.volume_percent <= 100
            && self.flawless_run == AlertLoudness::Off
            && self
                .key_pop_dungeons
                .as_ref()
                .is_none_or(|names| valid_dungeon_names(names))
            && valid_dungeon_names(&self.key_pop_excluded_dungeons)
            && self.key_pop_dungeons.as_ref().map_or(0, Vec::len)
                + self.key_pop_excluded_dungeons.len()
                <= 256
    }
    pub fn includes_key_pop_dungeon(&self, dungeon: &str) -> bool {
        match &self.key_pop_dungeons {
            Some(names) => names.iter().any(|name| name == dungeon),
            None => !self
                .key_pop_excluded_dungeons
                .iter()
                .any(|name| name == dungeon),
        }
    }
    pub fn allows_alert(&self, alert: &AlertView) -> bool {
        alert.kind != AlertKind::KeyPopped
            || matches!(&alert.details, Some(AlertDetails::KeyPopped { dungeon }) if self.includes_key_pop_dungeon(dungeon))
    }
    pub fn loudness(&self, kind: &AlertKind) -> AlertLoudness {
        match kind {
            AlertKind::WhiteBag => self.white_bag,
            AlertKind::PersonalBest => self.personal_best,
            AlertKind::KeyPopped => self.key_popped,
            AlertKind::Whisper => self.whisper,
            AlertKind::GuildMessage => self.guild_message,
            AlertKind::PartyMessage => self.party_message,
            AlertKind::Death => AlertLoudness::Off,
            AlertKind::FlawlessRun => AlertLoudness::Off,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AlertDetails {
    WhiteBag {
        drop_id: String,
        bag_type: u16,
        ordinal: Option<u32>,
        window: Option<TimeRange>,
        source_enemy: Option<u16>,
        source_estimated: bool,
        contents_known: bool,
    },
    PersonalBest {
        fight_id: String,
        character_id: String,
        boss_type: u16,
        #[serde(with = "crate::finite::f64_value")]
        #[ts(as = "f64")]
        dps: f64,
        #[serde(with = "crate::finite::f64_value")]
        #[ts(as = "f64")]
        previous_dps: f64,
    },
    RunTimePersonalBest {
        dungeon: String,
        #[ts(as = "f64")]
        duration_micros: u64,
        #[ts(as = "f64")]
        previous_micros: u64,
    },
    KeyPopped {
        dungeon: String,
    },
    Chat {
        channel: crate::ChatChannel,
        sender: String,
        excerpt: String,
    },
    Death {
        character_id: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AlertCursor {
    pub at: Timestamp,
    pub id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AlertFilter {
    pub kind: Option<AlertKind>,
    #[serde(default)]
    #[ts(as = "Option<Vec<AlertKind>>", optional)]
    pub kinds: Vec<AlertKind>,
    #[serde(default)]
    #[ts(as = "Option<Vec<AlertKind>>", optional)]
    pub exclude_kinds: Vec<AlertKind>,
    pub range: Option<TimeRange>,
    pub query: String,
    pub cursor: Option<AlertCursor>,
    pub limit: u16,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AlertPage {
    pub rows: Vec<AlertView>,
    pub next: Option<AlertCursor>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPermission {
    Unknown,
    Granted,
    Denied,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AlertDeliveryStatus {
    pub permission: NotificationPermission,
    pub sound_available: bool,
    pub message: Option<String>,
    pub flawless_unavailable: String,
}

fn valid_dungeon_names(names: &[String]) -> bool {
    names.len() <= 256
        && names
            .iter()
            .all(|name| !name.is_empty() && name.len() <= 64 && !name.chars().any(char::is_control))
}

/// KPI counts reflect the selected range, independently of alert-list search and kind filters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AlertSummaryRequest {
    #[serde(default)]
    pub range: Option<TimeRange>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AlertKindCount {
    pub kind: AlertKind,
    pub count: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct AlertLastWhite {
    pub at: Timestamp,
    pub run_id: Option<String>,
    pub dungeon: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct AlertSummary {
    pub total: u32,
    pub by_kind: Vec<AlertKindCount>,
    pub last_white: Option<AlertLastWhite>,
    pub first_at: Option<Timestamp>,
}
