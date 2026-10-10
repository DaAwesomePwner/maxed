use crate::{DropView, EnchantState, TimeRange, Timestamp};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where a bag dropped: inside a run of a named dungeon, or outside any run (realm, nexus, etc.).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LootPlace {
    Dungeon { name: String },
    Outside,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DropCursor {
    pub at: Timestamp,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DropFilter {
    pub min_rank: Option<u8>,
    pub place: Option<LootPlace>,
    pub item_type: Option<u16>,
    pub enchanted_only: bool,
    pub range: Option<TimeRange>,
    pub cursor: Option<DropCursor>,
    pub limit: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct LootDropRow {
    pub drop: DropView,
    /// `runs.dungeon` of the bag's run; None when the bag has no run.
    pub dungeon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DropPage {
    pub rows: Vec<LootDropRow>,
    pub next: Option<DropCursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ItemHistoryRequest {
    pub item_type: u16,
    pub cursor: Option<DropCursor>,
    pub limit: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ItemTotals {
    /// Retained (bag, slot, item_type) appearances.
    pub appearances: u32,
    pub bags: u32,
    /// Appearances with a taken_observed_at; does not identify who took them.
    pub taken: u32,
    pub first_at: Option<Timestamp>,
    pub last_at: Option<Timestamp>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EnchantVariant {
    pub state: EnchantState,
    pub applied: Option<u8>,
    pub ids: Vec<i32>,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct PlaceCount {
    pub dungeon: Option<String>,
    pub count: u32,
    pub last_at: Timestamp,
    // Ended runs of this dungeon started after last_at; None outside a run.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub runs_since: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SourceCount {
    pub source_enemy: Option<u16>,
    pub estimated: bool,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ItemHistory {
    pub item_type: u16,
    pub totals: ItemTotals,
    pub enchant_variants: Vec<EnchantVariant>,
    pub by_dungeon: Vec<PlaceCount>,
    pub by_source: Vec<SourceCount>,
    pub bags: DropPage,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LootStatsFilter {
    pub range: Option<TimeRange>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct BagTierCount {
    pub bag_rank: Option<u8>,
    pub count: u32,
    /// Bag object type of the newest bag in the group.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub bag_type: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct BestBagRef {
    pub drop_id: String,
    pub run_id: Option<String>,
    pub bag_rank: u8,
    pub at: Timestamp,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub bag_type: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DungeonLootStats {
    pub dungeon: String,
    pub runs_ended: u32,
    pub clears: u32,
    pub bags: Vec<BagTierCount>,
    pub whites: u32,
    /// All-time: ended runs of this dungeon that started after the latest white. None = no white ever.
    pub runs_since_last_white: Option<u32>,
    pub best: Option<BestBagRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct SourceLootStats {
    pub source_enemy: u16,
    /// None for sources seen outside a run (realm bosses/events).
    pub dungeon: Option<String>,
    pub bags: Vec<BagTierCount>,
    pub whites: u32,
    /// Ended runs of `dungeon` in range; None when `dungeon` is None (counts only, no rate).
    pub runs_ended: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct LootStats {
    pub dungeons: Vec<DungeonLootStats>,
    pub sources: Vec<SourceLootStats>,
    pub totals: Vec<BagTierCount>,
    pub whites: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ChaseItem {
    pub item_type: u16,
    pub dungeon: Option<String>,
    pub pinned_at: Timestamp,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LootSettings {
    #[serde(default)]
    pub chase: Vec<ChaseItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CollectionRequest {
    pub dungeon: Option<String>,
    pub chase: Vec<ChaseItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct ChaseRow {
    pub item: ChaseItem,
    pub seen: u32,
    pub last_at: Option<Timestamp>,
    pub dry_runs: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct CollectionCell {
    pub item_type: u16,
    pub tier: String,
    pub count: u32,
    pub last_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Collection {
    pub chase: Vec<ChaseRow>,
    pub grid_dungeon: Option<String>,
    pub grid: Vec<CollectionCell>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ItemMatch {
    pub object_type: u16,
    pub name: String,
    pub tier: Option<String>,
}

pub const MAX_CHASE_ITEMS: usize = 100;
pub const MAX_LOOT_PAGE: u16 = 50;
pub const MAX_SUMMARY_ITEM_BAGS: u32 = 10_000;

/// Feed filters apply to bags; pagination is ignored. Days are half-open local-day ranges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DropSummaryRequest {
    pub filter: DropFilter,
    pub days: Vec<TimeRange>,
    #[serde(default = "summary_include_items")]
    #[ts(as = "Option<bool>", optional)]
    pub include_items: bool,
}

fn summary_include_items() -> bool {
    true
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct DropTotals {
    pub bags: u32,
    pub runs: u32,
    /// Ended runs in range, independent of bag filters; unavailable outside runs.
    pub runs_ended: Option<u32>,
    pub whites: u32,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub ut_items: Option<u32>,
    #[serde(default)]
    #[ts(optional = nullable)]
    pub st_items: Option<u32>,
    /// Recorded appearances with applied enchants greater than zero.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub enchanted_items: Option<u32>,
    /// False when item aggregation was disabled or the matching bag count exceeded the cap.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub items_counted: bool,
    /// Greatest recorded applied-enchant count among matching appearances.
    pub best_enchants: Option<u8>,
    pub last_white: Option<LastWhite>,
    pub first_at: Option<Timestamp>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct LastWhite {
    pub drop_id: String,
    pub run_id: Option<String>,
    pub dungeon: Option<String>,
    pub at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct DropSummary {
    pub totals: DropTotals,
    pub days: Vec<DropTotals>,
}
