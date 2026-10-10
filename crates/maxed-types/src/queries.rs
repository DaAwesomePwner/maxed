//! Query contracts. Unknown values remain explicit nulls.

use crate::domain::*;
use crate::Timestamp;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
/// Half-open interval: start is included and end is excluded.
pub struct TimeRange {
    pub start: Timestamp,
    pub end: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HomeScope {
    Session { id: String },
    Today { range: TimeRange },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ScopeLootItem {
    pub item_type: u16,
    pub tier: Option<String>,
    /// Retained (bag, slot, item type) appearances, not stack sizes or pickups.
    pub count: u32,
    pub best_enchant: Option<EnchantView>,
    pub last_at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ScopeLootPage {
    pub items: Vec<ScopeLootItem>,
    pub next: Option<String>,
    pub total_bags: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RunSort {
    Newest,
    Oldest,
    DurationAscending,
    DurationDescending,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunCursor {
    pub sort_value: String,
    pub id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunFilter {
    #[serde(default)]
    #[ts(optional = nullable)]
    pub character_id: Option<String>,
    pub dungeon: Option<String>,
    pub outcome: Option<Outcome>,
    pub range: Option<TimeRange>,
    pub sort: RunSort,
    pub cursor: Option<RunCursor>,
    pub limit: u16,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunPage {
    pub rows: Vec<RunView>,
    pub next: Option<RunCursor>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub details: Option<Vec<RunRowDetail>>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub summary: Option<RunListSummary>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunRowDetail {
    pub run_id: String,
    pub character_name: Option<String>,
    pub class_id: Option<u16>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub local_share: Option<f64>,
    pub share_partial: bool,
    pub fame_delta: Option<i32>,
    pub loot_preview: Vec<RunLootPreview>,
    pub observed_drop_count: u32,
    #[ts(as = "Option<f64>")]
    pub clear_micros: Option<u64>,
    #[ts(as = "Option<f64>")]
    pub best_clear_micros: Option<u64>,
    /// Fastest qualifying clear of the same dungeon among runs that started before
    /// this run (by started, id), using the same clear rule as best_clear_micros
    /// (dungeon_bests.sql); None when there was no earlier qualifying clear.
    #[serde(default)]
    #[ts(optional = nullable)]
    #[ts(as = "Option<f64>")]
    pub previous_best_clear_micros: Option<u64>,
    pub is_personal_best: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunLootPreview {
    pub drop_id: String,
    pub bag_type: u16,
    pub bag_rank: Option<u8>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunListSummary {
    /// Counts ignore the outcome filter. Includes in-progress runs, so
    /// cleared + died + left + unknown can be less than runs.
    pub runs: u32,
    pub cleared: u32,
    pub died: u32,
    pub left: u32,
    /// Explicit unknown outcomes only; in-progress runs have no outcome.
    pub unknown: u32,
    #[ts(as = "f64")]
    pub time_in_runs_micros: u64,
    pub personal_bests: u32,
    #[ts(as = "Option<f64>")]
    pub fame_delta: Option<i64>,
    pub fame_observed_runs: u32,
    pub first_started: Option<Timestamp>,
    pub total_runs_all_time: u32,
    /// Newest death within dungeon/range/character scope, ignoring the outcome filter.
    pub last_death: Option<RunDeathRef>,
    pub characters: Vec<RunListCharacter>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunListCharacter {
    pub id: String,
    pub name: Option<String>,
    pub class_id: Option<u16>,
    pub runs: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunDeathRef {
    pub run_id: String,
    pub dungeon: String,
    pub at: Timestamp,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TimelineKind {
    Entered,
    Fight,
    Drop,
    Completion,
    Death,
    Exit,
    CaptureIssue,
    PartyDeath,
    PlayerLeft,
    PortalOpened,
    Kill,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct TimelineEntry {
    pub id: String,
    pub at: Timestamp,
    pub kind: TimelineKind,
    pub related_id: Option<String>,
    pub explanation: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct FamePoint {
    pub at: Timestamp,
    pub character_id: String,
    pub fame: i32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ClearComparison {
    pub previous_clears: u32,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub mean_micros: f64,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub change_fraction: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunStory {
    pub run: RunView,
    pub character: Option<CharacterView>,
    pub fights: Vec<RunFightSummary>,
    pub drops: Vec<DropView>,
    pub timeline: Vec<TimelineEntry>,
    pub comparison: Option<ClearComparison>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub totals: Option<RunTotals>,
    /// Boss-tier fights ordered by (started, id), capped at 64 and the available byte budget.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub boss_fights: Option<Vec<FightView>>,
    /// True when boss fights were omitted due to the cap, size budget, or unreadable detail.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub boss_fights_limited: Option<bool>,
    /// Instance events for this run ordered by (at, id), capped at 2000 and the available byte budget.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub events: Option<Vec<InstanceEventView>>,
    /// True when run events were omitted due to the row cap, size budget or unreadable detail.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub events_limited: Option<bool>,
    /// Previous run by (started, id) across all runs, regardless of list filters.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub older_run_id: Option<String>,
    /// Next run by (started, id) across all runs, regardless of list filters.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub newer_run_id: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HistoryReply<T> {
    Ready { value: T },
    Superseded,
    Busy,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunFightSummary {
    pub id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub target_kind: Option<TargetKind>,
    pub target_type: Option<u16>,
    pub target_name: Option<String>,
    pub boss_type: Option<u16>,
    pub ordinal: u32,
    pub started: Timestamp,
    pub ended: Option<Timestamp>,
    pub first_hit: Option<Timestamp>,
    pub last_hit: Option<Timestamp>,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub total_damage: f64,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub local_damage: Option<f64>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub local_dps: Option<f64>,
    pub local_rank: Option<u32>,
    pub player_count: u32,
    pub partial: bool,
    pub estimated: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunFightStripRow {
    pub fight_id: String,
    pub target_type: Option<u16>,
    pub target_name: Option<String>,
    pub boss_type: Option<u16>,
    pub started: Timestamp,
    pub ended: Option<Timestamp>,
    pub killed: bool,
    pub player_count: u32,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub damage: Option<f64>,
    pub rank: Option<u32>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub share: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct HomeRunDetail {
    pub run_id: String,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub local_share: Option<f64>,
    pub share_partial: bool,
    pub observed_drop_count: u32,
    pub loot: Vec<DropView>,
    pub loot_preview_limited: bool,
    pub issues: Vec<CoverageIssue>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct HomeSummary {
    pub runs: u32,
    pub clears: u32,
    pub deaths: u32,
    pub fame_delta: Option<i32>,
    pub fame: Vec<FamePoint>,
    pub best_drop: Option<DropView>,
    pub top_fight: Option<FightView>,
    pub recent_runs: Vec<RunView>,
    pub issues: Vec<CoverageIssue>,
    pub recent_details: Vec<HomeRunDetail>,
    pub fame_missing_baselines: u32,
    pub fame_sampled: bool,
    pub unranked_drop_count: u32,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub dungeon_stats: Option<Vec<DungeonStatsRow>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StoreState {
    Ready,
    Backpressured,
    Failed,
    Closing,
    Closed,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct StoreStatus {
    pub state: StoreState,
    pub queued: u32,
    pub durable: Option<Sequence>,
    pub stream_id: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunPlayerTotal {
    pub player_id: String,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub name: Option<String>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub class_id: Option<u16>,
    pub is_local: bool,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub damage: f64,
    pub hits: u32,
    pub deaths: u32,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub share: Option<f64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub rank: Option<u32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub dps: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct RunTotals {
    pub fights: u32,
    pub kills: u32,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub first_hit: Option<Timestamp>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub last_hit: Option<Timestamp>,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub total_damage: f64,
    #[serde(with = "crate::finite::f64_value")]
    #[ts(as = "f64")]
    pub unattributed_damage: f64,
    pub players: Vec<RunPlayerTotal>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub local: Option<RunPlayerTotal>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub fame_delta: Option<i32>,
    pub partial: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DungeonStatsRow {
    pub dungeon: String,
    pub runs: u32,
    pub clears: u32,
    pub deaths: u32,
    pub observed_clears: u32,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub mean_clear_micros: Option<f64>,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[ts(as = "Option<f64>")]
    pub best_clear_micros: Option<u64>,
    pub last_started: Timestamp,
    #[serde(default)]
    #[ts(optional = nullable)]
    #[ts(as = "Option<f64>")]
    pub all_time_best_clear_micros: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DungeonBest {
    pub dungeon: String,
    #[ts(as = "f64")]
    pub best_clear_micros: u64,
}
