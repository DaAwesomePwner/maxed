use crate::common::receipts;
use rusqlite::Connection;

/// Remove only maintenance additions, preserving installed loot indexes.
/// Keep child tables before their referenced parents.
pub fn remove_maintenance_schema(db: &Connection) {
    let triggers: Vec<String> = db
        .prepare("SELECT name FROM sqlite_schema WHERE type='trigger' AND name LIKE 'unresolved_%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for name in triggers {
        db.execute_batch(&format!("DROP TRIGGER {name}")).unwrap();
    }
    db.execute_batch(
        "DROP TABLE IF EXISTS crucible_definitions; DROP TABLE IF EXISTS unresolved_observations",
    )
    .unwrap();
    if db.prepare("SELECT account_id FROM characters").is_ok() {
        db.execute_batch("DROP VIEW death_accounts; DROP VIEW fame_accounts; DROP VIEW drop_accounts; DROP VIEW fight_accounts; DROP VIEW run_accounts; DROP VIEW character_accounts;
        DROP TABLE identity_spans; DROP TABLE observation_characters; DROP TABLE account_recovery; DROP TABLE current_account;
        DROP INDEX characters_account; ALTER TABLE characters DROP COLUMN account_id; DROP TABLE accounts;
        DROP INDEX IF EXISTS runs_character; DELETE FROM schema_migrations WHERE version>=17;").unwrap();
    }
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
