// Explicit target: keep fixture paths beside their original test modules.
#[path = "../common/mod.rs"]
mod common;
#[path = "../../../maxed-engine/tests/common/mod.rs"]
mod engine_common;

#[path = "../account_scope_store.rs"]
mod account_scope_store;
#[path = "../alerts.rs"]
mod alerts;
#[path = "../boss_history.rs"]
mod boss_history;
#[path = "../capture_prefix.rs"]
mod capture_prefix;
#[path = "../chat.rs"]
mod chat;
#[path = "../chat_alerts.rs"]
mod chat_alerts;
#[path = "../chat_migration.rs"]
mod chat_migration;
#[path = "../chat_reads.rs"]
mod chat_reads;
#[path = "../chat_retention.rs"]
mod chat_retention;
#[path = "../d_alerts_killed.rs"]
mod d_alerts_killed;
#[path = "../d_home.rs"]
mod d_home;
#[path = "../d_instance.rs"]
mod d_instance;
#[path = "../d_migration.rs"]
mod d_migration;
#[path = "../d_totals.rs"]
mod d_totals;
#[path = "../dungeon_profile.rs"]
mod dungeon_profile;
#[path = "../event_fts.rs"]
mod event_fts;
#[path = "../event_rows.rs"]
mod event_rows;
#[path = "../home.rs"]
mod home;
#[path = "../home_hud.rs"]
mod home_hud;
#[path = "../home_read_equivalence.rs"]
mod home_read_equivalence;
#[path = "../home_streak.rs"]
mod home_streak;
#[path = "../journey.rs"]
mod journey;
#[path = "../loot.rs"]
mod loot;
#[path = "../loot_migration.rs"]
mod loot_migration;
#[path = "../loot_retention.rs"]
mod loot_retention;
#[path = "../loot_stats_migration.rs"]
mod loot_stats_migration;
#[path = "../maintenance_backup.rs"]
mod maintenance_backup;
#[path = "../maintenance_jobs.rs"]
mod maintenance_jobs;
#[path = "../migrations.rs"]
mod migrations;
#[path = "../party_average.rs"]
mod party_average;
#[path = "../party_migration.rs"]
mod party_migration;
#[path = "../party_projection.rs"]
mod party_projection;
#[path = "../party_reads.rs"]
mod party_reads;
#[path = "../pb_index_migration.rs"]
mod pb_index_migration;
#[path = "../player_profiles.rs"]
mod player_profiles;
#[path = "../queries.rs"]
mod queries;
#[path = "../receipt_conversion.rs"]
mod receipt_conversion;
#[path = "../reopen.rs"]
mod reopen;
#[path = "../retention_migration.rs"]
mod retention_migration;
#[path = "../run_recap.rs"]
mod run_recap;
#[path = "../run_rows.rs"]
mod run_rows;
#[path = "../run_screen.rs"]
mod run_screen;
#[path = "../run_strip.rs"]
mod run_strip;
#[path = "../runs.rs"]
mod runs;
#[path = "../scope_loot.rs"]
mod scope_loot;
#[path = "../stats_dungeons.rs"]
mod stats_dungeons;
#[path = "../stats_maps.rs"]
mod stats_maps;
#[path = "../stats_migration.rs"]
mod stats_migration;
#[path = "../stats_sessions.rs"]
mod stats_sessions;
#[path = "../stats_visits.rs"]
mod stats_visits;
#[path = "../storage_usage.rs"]
mod storage_usage;
#[path = "../summary_migration.rs"]
mod summary_migration;
#[path = "../visit_recap.rs"]
mod visit_recap;
#[path = "../writer.rs"]
mod writer;

#[path = "../account_scope_readers.rs"]
mod account_scope_readers;
