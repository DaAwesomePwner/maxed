//! Observed player profiles and damage timelines within a capture scope.

use crate::{
    EquipmentView, PlayerSeries, RunPlayerTotal, RunView, SourceDamage, StatView, Timestamp,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PlayerProfileView {
    pub scope_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub run_id: Option<String>,
    pub player_id: String,
    pub object_id: i32,
    pub is_local: bool,
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
    pub fame: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub skin_id: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub guild: Option<String>,
    pub stats: Vec<StatView>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub maxed: Option<u8>,
    // Stat IDs whose observed base value meets a known class cap.
    #[serde(default)]
    pub maxed_stats: Vec<u8>,
    pub equipment: Vec<EquipmentView>,
    pub first_seen: Timestamp,
    pub last_seen: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ScopeBucket {
    pub from: Timestamp,
    pub until: Timestamp,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub party_total: f64,
    pub party_active: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ScopeTimelineUpdate {
    pub scope_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub run_id: Option<String>,
    pub buckets: Vec<ScopeBucket>,
    pub players: Vec<PlayerSeries>,
    // Carries every player row; replace stored rows.
    pub full: bool,
    // The scope timeline has been compacted at least once.
    pub compacted: bool,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct GearChange {
    pub at: Timestamp,
    pub equipment: Vec<EquipmentView>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PlayerTimeline {
    pub buckets: Vec<ScopeBucket>,
    #[serde(with = "crate::finite::vec_f64")]
    #[ts(as = "Vec<f64>")]
    pub points: Vec<f64>,
    pub compacted: bool,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PlayerDeathAt {
    pub at: Timestamp,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub fight_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RosterEntry {
    pub player_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub class_id: Option<u16>,
    pub is_local: bool,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub damage: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PartySummary {
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub total_damage: f64,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub average_dps: Option<f64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub top_player_id: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub top_name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub top_dps: Option<f64>,
    pub players_with_damage: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunPlayerFight {
    pub fight_id: String,
    #[serde(default)]
    pub dps_basis: crate::DpsBasis,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub object_id: Option<i32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub target_type: Option<u16>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub target_name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub boss_type: Option<u16>,
    pub started: Timestamp,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub ended: Option<Timestamp>,
    pub killed: bool,
    pub player_count: u32,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub damage: Option<f64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub dps: Option<f64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub rank: Option<u32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub share: Option<f64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub deaths: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunPlayerView {
    pub run: RunView,
    pub scope_id: String,
    pub player_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub player: Option<RunPlayerTotal>,
    pub party: PartySummary,
    pub sources: Vec<SourceDamage>,
    pub fights: Vec<RunPlayerFight>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub profile: Option<PlayerProfileView>,
    pub gear_log: Vec<GearChange>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub timeline: Option<PlayerTimeline>,
    pub deaths: Vec<PlayerDeathAt>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub departed_at: Option<Timestamp>,
    pub roster: Vec<RosterEntry>,
    pub partial: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PlayerContextView {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub profile: Option<PlayerProfileView>,
    pub gear_log: Vec<GearChange>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub timeline: Option<PlayerTimeline>,
    pub deaths: Vec<PlayerDeathAt>,
}
