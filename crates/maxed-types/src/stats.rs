//! Stats tab DTOs: map visits, per-map fame rates, sessions and lifetime dungeon records.
use crate::{AreaKind, RunView, TimeRange, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Rows per Maps or Dungeons read. One more row is read to report truncation.
pub const MAX_STATS_ROWS: usize = 200;
/// Sessions per page.
pub const MAX_STATS_SESSION_PAGE: u16 = 100;
/// Sessions per CSV export.
pub const MAX_STATS_EXPORT_SESSIONS: usize = 10_000;
/// Runs per session detail and per session export.
pub const MAX_STATS_SESSION_RUNS: usize = 500;
/// Characters offered by the filter.
pub const MAX_STATS_CHARACTERS: usize = 200;
/// Below five minutes in total, per-hour rates are not shown.
pub const MIN_STATS_RATE_MICROS: u64 = 300_000_000;

/// Fame per hour, or `None` when the row has under five minutes in total.
pub fn fame_per_hour(fame: i64, micros: u64) -> Option<f64> {
    (micros >= MIN_STATS_RATE_MICROS).then(|| fame as f64 * 3_600_000_000.0 / micros as f64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum VisitSource {
    Live,
    Backfill,
}

/// One stay in one map. Stored as `map_visits.json`; columns mirror these fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MapVisitView {
    pub id: String,
    pub stream_id: String,
    pub session_id: String,
    pub character_id: Option<String>,
    pub run_id: Option<String>,
    pub map: String,
    pub kind: AreaKind,
    pub realm_instance_key: Option<String>,
    pub started: Timestamp,
    pub ended: Option<Timestamp>,
    pub source: VisitSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct StatsFilter {
    /// Visits, sessions and runs whose start is within `[start, end)`; `None` is all time.
    pub range: Option<TimeRange>,
    pub character_id: Option<String>,
}

/// `range` is half-open over `fights.started`; `None` is all time.
/// `character_id` restricts local performance only; kills cover all characters.
/// `target_types` contains at most 64 types from fight JSON (`$.target_type`);
/// `None` includes all targets and an empty list includes none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct BossHistoryRequest {
    pub range: Option<TimeRange>,
    pub character_id: Option<String>,
    pub target_types: Option<Vec<u16>>,
}

/// Boss rows sorted by fights descending, then target type. Read 201 rows and keep 200.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct BossHistory {
    pub rows: Vec<BossHistoryRow>,
    pub truncated: bool,
}

/// Home's scalar boss baselines, using `BossHistoryRequest` and the same row order and limit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct HomeBossBaselines {
    pub rows: Vec<HomeBossBaseline>,
    pub truncated: bool,
}

/// Same definitions as the matching `BossHistoryRow` fields; `kill_samples` is `kills`.
/// Performance is character-specific; confirmed kill durations cover all characters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct HomeBossBaseline {
    pub target_type: u16,
    pub fights: u32,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub avg_dps: Option<f64>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub best_dps: Option<f64>,
    pub kill_samples: u32,
    #[ts(as = "Option<f64>")]
    pub median_kill_micros: Option<u64>,
}

/// `target_type` comes from fight JSON (`$.target_type`).
/// `fights` counts ended fights in range with non-null `fights.local_dps`,
/// restricted to the requested character resolved through the run or containing map visit.
/// `avg_dps`, `best_dps`, `best_fight_id` and the mean of non-null local ranks
/// (`avg_rank`) cover those eligible fights. `avg_party_dps` is the mean of the
/// shared `party_avg` CTE's per-fight averages over those fights; each party
/// average excludes the local player and unknown DPS.
///
/// `kills` and `median_kill_micros` cover confirmed kills in range across all
/// characters: `CAST(json_extract(f.json,'$.kill.at') AS INTEGER)` is non-null
/// and `json_extract(f.json,'$.kill.provisional')=0`.
/// The median uses `kill.at - started`; for an even count, floor the mean of
/// the two middle values. Unknown metrics, including a median with no kills, are `None`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct BossHistoryRow {
    pub target_type: u16,
    pub target_name: Option<String>,
    pub fights: u32,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub avg_dps: Option<f64>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub best_dps: Option<f64>,
    pub best_fight_id: Option<String>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub avg_rank: Option<f64>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub avg_party_dps: Option<f64>,
    pub kills: u32,
    #[ts(as = "Option<f64>")]
    pub median_kill_micros: Option<u64>,
}

/// Runs are scoped by dungeon and half-open start range. The character filter
/// applies to performance and clear-time baselines, not the learned boss order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DungeonProfileRequest {
    pub dungeon: String,
    pub character_id: Option<String>,
    pub range: TimeRange,
}

/// `clears` counts cleared runs across all characters. Bosses killed in at least
/// half those runs are ordered by median kill offset, then target type.
/// `usual_run_dps` is the mean established whole-run local DPS from ended runs
/// for the character, using run-story totals; at least three eligible runs are needed.
/// `usual_clear_micros` is the median `completed_at - started` for the character's
/// cleared runs. At least three non-negative durations are needed; even medians round down.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DungeonProfile {
    pub dungeon: String,
    pub clears: u32,
    pub bosses: Vec<ProfileBoss>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub usual_run_dps: Option<f64>,
    pub run_dps_samples: u32,
    #[ts(as = "Option<f64>")]
    pub usual_clear_micros: Option<u64>,
    pub clear_samples: u32,
}

/// Confirmed, non-provisional kills of `boss` or `event_boss` targets inside
/// cleared runs, across all characters. Inconsistent negative offsets/durations
/// are excluded. `seen_in` counts distinct runs, not repeated kills.
/// `median_kill_micros` is the median `kill.at - fight.started`, with at least three kills.
/// `best_dps` is the maximum established local DPS on this boss in ended fights
/// starting in range for the character, resolved through the run or containing map visit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ProfileBoss {
    pub target_type: u16,
    pub target_name: Option<String>,
    pub seen_in: u32,
    #[ts(as = "f64")]
    pub median_offset_micros: u64,
    #[ts(as = "Option<f64>")]
    pub median_kill_micros: Option<u64>,
    pub kill_samples: u32,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub best_dps: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatsCharacter {
    pub id: String,
    pub name: Option<String>,
    pub class_id: Option<u16>,
    pub last_seen: Timestamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StatsMapGroup {
    Dungeon,
    Realm,
    Hubs,
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct StatsMapRow {
    pub group: StatsMapGroup,
    /// The catalog dungeon name, or "Realm", "Hubs" or "Other".
    pub map: String,
    pub visits: u32,
    #[ts(as = "f64")]
    pub time_micros: u64,
    #[ts(as = "f64")]
    pub fame: i64,
    /// At least one visit had no earlier fame sample; show the fame as "≥".
    pub missing_baseline: bool,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub fame_per_hour: Option<f64>,
    /// Dungeon rows only.
    pub clears: Option<u32>,
    /// Dungeon rows only.
    pub deaths: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct StatsMaps {
    pub rows: Vec<StatsMapRow>,
    pub truncated: bool,
    /// The earliest live (non-backfilled) visit: when realm and hub tracking began on this install.
    pub tracking_since: Option<Timestamp>,
    pub characters: Vec<StatsCharacter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct StatsSessionCursor {
    pub started: Timestamp,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct StatsSessionFilter {
    pub range: Option<TimeRange>,
    pub character_id: Option<String>,
    /// Include sessions with no fame change, no runs, no deaths and no observed bags.
    pub include_empty: bool,
    pub cursor: Option<StatsSessionCursor>,
    pub limit: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct StatsSessionRow {
    pub id: String,
    pub started: Timestamp,
    /// `None` while the session is in progress.
    pub ended: Option<Timestamp>,
    #[ts(as = "f64")]
    pub duration_micros: u64,
    #[ts(as = "f64")]
    pub fame: i64,
    pub missing_baseline: bool,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub fame_per_hour: Option<f64>,
    // Observed bags, as in Loot Feed with "Any bag"; pickup and ownership are unknown.
    pub bags: u32,
    // Orange/ST and higher bag ranks (80+): orange, red and white, including boosted bags.
    pub notable_bags: u32,
    // Uses the same five-minute minimum as fame per hour.
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub bags_per_hour: Option<f64>,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub notable_bags_per_hour: Option<f64>,
    pub runs: u32,
    pub clears: u32,
    pub deaths: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct StatsSessionPage {
    pub rows: Vec<StatsSessionRow>,
    pub next: Option<StatsSessionCursor>,
    pub characters: Vec<StatsCharacter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct StatsSessionRequest {
    pub id: String,
    pub character_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct StatsSessionDetail {
    pub session: StatsSessionRow,
    pub maps: Vec<StatsMapRow>,
    pub maps_truncated: bool,
    /// Session fame minus the sum of its visits' fame, when positive.
    #[ts(as = "Option<f64>")]
    pub unrecorded_fame: Option<i64>,
    pub runs: Vec<RunView>,
    pub runs_truncated: bool,
    pub characters: Vec<StatsCharacter>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct StatsDungeonRow {
    pub dungeon: String,
    pub runs: u32,
    pub clears: u32,
    pub deaths: u32,
    pub observed_clears: u32,
    #[serde(with = "crate::finite::optional_f64")]
    #[ts(as = "Option<f64>")]
    pub mean_clear_micros: Option<f64>,
    #[ts(as = "Option<f64>")]
    pub best_clear_micros: Option<u64>,
    #[ts(as = "f64")]
    pub total_micros: u64,
    pub last_started: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct StatsDungeons {
    pub rows: Vec<StatsDungeonRow>,
    pub truncated: bool,
    pub characters: Vec<StatsCharacter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StatsExportKind {
    Maps,
    Sessions,
    Dungeons,
    SessionRuns { id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct StatsExportRequest {
    pub kind: StatsExportKind,
    pub filter: StatsFilter,
    /// Sessions export only.
    pub include_empty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StatsExportResult {
    Saved { rows: u32 },
    Cancelled,
}
