//! Deterministic engine boundary. Shared contracts live in `maxed-types`.

#![forbid(unsafe_code)]

pub mod alerts;
pub mod character;
mod character_stats;
pub mod chat;
mod clock;
mod completion;
mod enchants;
mod encoding;
mod math;
pub mod names;
mod reconstruction;
pub use character_stats::{decode_completion_counts, CompletionCounts, CompletionError};
pub use clock::{dps, idle_deadline, share};
pub use completion::{completion_rule, valid_counter_step};
pub use enchants::parse_enchant;
pub use math::damage_with_defense;
pub use reconstruction::ReconstructionReason;
mod characters;
mod combat;
mod drops;
mod engine;
mod fights;
mod identity;
mod live;
pub mod party;
mod players;
mod runs;
mod world;
pub use live::{compact_live_fight, LIVE_CHART_BUCKETS};
pub use players::{RosterPresence, PROFILE_LIMIT};
#[cfg(any(test, feature = "perf-live"))]
pub mod perf;
pub use characters::{personal_best_eligible, FLAWLESS_UNSUPPORTED};
pub use engine::{run_totals, RUN_TOTAL_PLAYER_LIMIT, SNAPSHOT_SERIES_LIMIT};
pub use engine::{Engine, EngineConfig, EngineError};

#[cfg(test)]
mod maintenance_tests;

pub use identity::character_id_for;
