//! Party presence, requirement presets and player history contracts.

use crate::{AreaKind, EquipmentView, Outcome, PlayerProfileView, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Matches display names before the first comma, ignoring ASCII case.
pub fn name_key(name: &str) -> String {
    name.split(',')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayerPresence {
    pub scope_id: String,
    #[serde(default)]
    pub run_id: Option<String>,
    pub player_id: String,
    pub object_id: i32,
    pub present: bool,
    pub at: Timestamp,
}

/// Equipment slots in weapon, ability, armor, ring order (0 through 3).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum EquipSlot {
    Weapon,
    Ability,
    Armor,
    Ring,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SlotTiers {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub weapon: Option<u8>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub ability: Option<u8>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub armor: Option<u8>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub ring: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RequirementPreset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub min_maxed: Option<u8>,
    pub min_tier: SlotTiers,
    pub ut_st_meets: bool,
    pub auto_apply: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PartySettings {
    #[serde(default)]
    pub presets: Vec<RequirementPreset>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub default_preset_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ParseResult {
    Pass,
    Fail,
    Check,
    NotSeen,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ParseReasonKind {
    BelowMaxed,
    StatsNotSeen,
    SlotEmpty,
    BelowTier,
    UtStNotAccepted,
    NoTierData,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ParseReason {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub slot: Option<EquipSlot>,
    pub kind: ParseReasonKind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ParseOutcome {
    pub result: ParseResult,
    pub reasons: Vec<ParseReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ContributionFlag {
    NoDamageSeen,
    LeftEarly,
    MostlyTrash,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PartyMark {
    Watched,
    Banned,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PresetSource {
    Picked,
    Auto,
    Default,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ResolvedPreset {
    pub id: String,
    pub name: String,
    pub source: PresetSource,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyCounts {
    pub in_view: u32,
    pub pass: u32,
    pub fail: u32,
    pub check: u32,
    pub not_seen: u32,
    pub no_damage_seen: u32,
    pub left_early: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ClassCount {
    pub class_id: u16,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyMember {
    pub player_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub name_key: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub class_id: Option<u16>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub level: Option<u8>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub stars: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub guild: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub maxed: Option<u8>,
    pub equipment: Vec<EquipmentView>,
    pub is_local: bool,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub present: Option<bool>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub out_of_view_since: Option<Timestamp>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub departed_at: Option<Timestamp>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub parse: Option<ParseOutcome>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub damage: Option<f64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub share: Option<f64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub boss_share: Option<f64>,
    pub flags: Vec<ContributionFlag>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub mark: Option<PartyMark>,
    pub runs_together: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyView {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub scope_id: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub run_id: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub map: Option<String>,
    pub area_kind: AreaKind,
    pub live: bool,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub preset: Option<ResolvedPreset>,
    pub counts: PartyCounts,
    pub classes: Vec<ClassCount>,
    pub members: Vec<PartyMember>,
    pub assets_available: bool,
    pub gear_recorded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyAnnotation {
    pub key: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub mark: Option<PartyMark>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub note: Option<String>,
    pub runs_together: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyPlayerRow {
    pub key: String,
    pub name: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub last_class_id: Option<u16>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub last_maxed: Option<u8>,
    pub runs_together: u32,
    pub deaths: u32,
    pub left_early: u32,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub avg_share: Option<f64>,
    pub last_seen: Timestamp,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub last_map: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub mark: Option<PartyMark>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PartyPlayerFilter {
    All,
    Watched,
    Banned,
    WithNotes,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PartyPlayerSort {
    LastSeen,
    RunsTogether,
    Deaths,
    LeftEarly,
    AvgShare,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyPlayersCursor {
    // Exact f64 sort value: `to_bits()` encoded as a decimal u64 string.
    pub sort_value: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyPlayersPage {
    pub rows: Vec<PartyPlayerRow>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub next: Option<PartyPlayersCursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyRunRow {
    pub run_id: String,
    pub dungeon: String,
    pub started: Timestamp,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub outcome: Option<Outcome>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub share: Option<f64>,
    pub died: bool,
    pub left_early: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyRunsCursor {
    pub started: Timestamp,
    pub run_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartyPlayerView {
    pub row: PartyPlayerRow,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub guild: Option<String>,
    pub first_seen: Timestamp,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub last_profile: Option<PlayerProfileView>,
    pub runs: Vec<PartyRunRow>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub next: Option<PartyRunsCursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ClearPartyHistory {
    pub marks_and_notes: bool,
}
