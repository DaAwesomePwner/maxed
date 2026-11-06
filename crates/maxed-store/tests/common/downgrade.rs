use crate::common::receipts;
use rusqlite::Connection;

/// Remove only maintenance additions, preserving installed loot indexes.
/// Keep child tables before their referenced parents.
pub fn remove_maintenance_schema(db: &Connection) {
    receipts::copy_to_legacy(db);
    db.execute_batch(
        "DROP TABLE event_receipts_v2;
         DROP TABLE event_streams;
         DROP TABLE maintenance_jobs;
         DROP TABLE backup_generations;",
    )
    .unwrap();
}

/// Remove shipped 015 indexes when constructing a v14 or earlier fixture.
pub fn remove_loot_stats_indexes(db: &Connection) {
    db.execute_batch(
        "DROP INDEX runs_loot_stats; DROP INDEX runs_id_dungeon; DROP INDEX drops_white_runs;",
    )
    .unwrap();
}
