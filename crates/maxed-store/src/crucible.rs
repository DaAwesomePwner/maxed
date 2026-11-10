use crate::StoreError;
use maxed_types::{CrucibleDef, Timestamp};
use rusqlite::{params, Connection, TransactionBehavior};

const MAX_DEFINITIONS: usize = 4096;

fn valid(def: &CrucibleDef) -> bool {
    !def.id.is_empty()
        && def.id.len() <= 64
        && def.damage_multiplier.is_finite()
        && def.damage_multiplier > 0.0
        && def.damage_multiplier <= 100.0
}

/// Read game configuration without migrating or recovering the database.
/// Missing/pre-020 tables yield no definitions; malformed rows are skipped.
/// SQLite operational errors still propagate to the caller.
pub fn read_crucible_definitions(conn: &Connection) -> Result<Vec<CrucibleDef>, StoreError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version < 20
        || !conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='crucible_definitions')",
            [],
            |r| r.get::<_, bool>(0),
        )?
    {
        return Ok(Vec::new());
    }
    let mut stmt =
        conn.prepare("SELECT id,damage_multiplier FROM crucible_definitions ORDER BY id")?;
    let mut rows = stmt.query([])?;
    let mut definitions = Vec::new();
    while let Some(row) = rows.next()? {
        if let (Ok(id), Ok(damage_multiplier)) = (row.get(0), row.get(1)) {
            let def = CrucibleDef {
                id,
                damage_multiplier,
            };
            if valid(&def) {
                definitions.push(def);
                if definitions.len() == MAX_DEFINITIONS {
                    break;
                }
            }
        }
    }
    Ok(definitions)
}

/// Atomically save valid observations on the durable writer's own connection,
/// outside an event-projection transaction. Returns accepted upserts (including
/// repeated IDs), even if a later observation evicts them from the bounded cache.
/// Invalid definitions are skipped; an unrepresentable timestamp rejects the batch.
pub fn upsert_crucible_definitions(
    conn: &mut Connection,
    definitions: &[CrucibleDef],
    observed_at: Timestamp,
) -> Result<usize, StoreError> {
    let at = i64::try_from(observed_at.0).map_err(|_| StoreError::InvalidEvent)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut written = 0;
    for def in definitions.iter().filter(|def| valid(def)) {
        tx.execute(
            "INSERT INTO crucible_definitions VALUES(?1,?2,?3,?3)
             ON CONFLICT(id) DO UPDATE SET damage_multiplier=excluded.damage_multiplier,
             last_observed_at=excluded.last_observed_at",
            params![def.id, def.damage_multiplier, at],
        )?;
        tx.execute(
            "DELETE FROM crucible_definitions WHERE id IN (
             SELECT id FROM crucible_definitions ORDER BY last_observed_at,id
             LIMIT max(0,(SELECT count(*) FROM crucible_definitions)-4096))",
            [],
        )?;
        written += 1;
    }
    tx.commit()?;
    Ok(written)
}
