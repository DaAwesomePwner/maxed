//! Inferred journey steps for saved-run recaps.
use crate::{DropView, FightView, InstanceEventView, RunFightSummary, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChainStepKind {
    Realm,
    Castle,
    Chamber,
    Sanctuary,
}

/// A journey step. `local_dps` is duration-weighted over fights with local DPS
/// and valid hit spans; `party_dps` weights party averages over those fights.
/// `item_types` contains up to four distinct types, ordered by bag rank, time,
/// then slot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ChainStep {
    pub visit_id: String,
    pub kind: ChainStepKind,
    pub name: String,
    pub run_id: Option<String>,
    pub started: Timestamp,
    pub ended: Option<Timestamp>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub local_dps: Option<f64>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub party_dps: Option<f64>,
    pub bags: u32,
    pub whites: u32,
    pub item_types: Vec<u16>,
}

/// Always inferred from adjacent visits; consumers must label it as inferred.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RecapChain {
    pub steps: Vec<ChainStep>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RunRecapRequest {
    pub run_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PbClear {
    #[ts(as = "f64")]
    pub clear_micros: u64,
    #[ts(as = "f64")]
    pub previous_best_micros: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DryStreakEnd {
    pub runs: u32,
    pub had_previous_white: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct BossDpsRecord {
    pub fight_id: String,
    pub boss_type: u16,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub dps: f64,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub previous_best_dps: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecapMilestone {
    FirstClear,
    ClearCount { count: u32 },
    FirstWhite,
    NewItems { item_types: Vec<u16>, more: u32 },
}

/// Records belong to the run that earned them, using earlier stored observations.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunRecap {
    pub run_id: String,
    pub clear_number: Option<u32>,
    pub in_a_row: u32,
    pub chain: Option<RecapChain>,
    pub pb_clear: Option<PbClear>,
    pub dry_streak_end: Option<DryStreakEnd>,
    pub best_dps: Vec<BossDpsRecord>,
    pub milestones: Vec<RecapMilestone>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct VisitRecapRequest {
    pub visit_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct VisitPeriod {
    pub visit_id: String,
    pub started: Timestamp,
    pub ended: Option<Timestamp>,
}

/// Saved realm contents across re-entries into the same instance in one session.
/// The title uses the realm name from a directly following Castle run, otherwise
/// the requested visit's map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct VisitRecap {
    pub visit_id: String,
    pub session_id: String,
    pub character_id: Option<String>,
    pub title: String,
    pub periods: Vec<VisitPeriod>,
    pub fights: Vec<RunFightSummary>,
    pub boss_fights: Vec<FightView>,
    pub boss_fights_limited: bool,
    pub announcements: Vec<InstanceEventView>,
    pub announcements_limited: bool,
    pub drops: Vec<DropView>,
    pub drops_limited: bool,
    #[ts(as = "Option<f64>")]
    pub fame_delta: Option<i64>,
    pub best_dps: Vec<BossDpsRecord>,
    pub chain: Option<RecapChain>,
}
