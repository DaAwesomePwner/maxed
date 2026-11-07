use crate::StoreError;
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{marker::PhantomData, path::Path, rc::Rc, time::Duration};

/// Latest schema supported by this build, matching SQLite's `user_version`.
pub const SCHEMA_VERSION: i64 = 17;
pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_millis(250);

const ACCOUNTS: &str = include_str!("../migrations/017_accounts.sql");

const MAINTENANCE_RECEIPTS: &str = include_str!("../migrations/016_maintenance_receipts.sql");

const LOOT_STATS_INDEXES: &str = include_str!("../migrations/015_loot_stats_indexes.sql");

const PARTY: &str = include_str!("../migrations/014_party.sql");

const PLAYER_PROFILES: &str = include_str!("../migrations/013_player_profiles.sql");

const EVENT_FTS: &str = include_str!("../migrations/012_event_fts.sql");

const EXCLUDE_JUNK_BAGS: &str = include_str!("../migrations/011_exclude_junk_bags.sql");

const MAP_VISITS: &str = include_str!("../migrations/010_map_visits.sql");

const CHAT: &str = include_str!("../migrations/009_chat.sql");

const LOOT_BROWSING: &str = include_str!("../migrations/008_loot_browsing.sql");

const PB_LOOKUPS: &str = include_str!("../migrations/007_pb_lookup_indexes.sql");

const TARGET_KINDS: &str = include_str!("../migrations/006_summary_target_kinds.sql");

const INSTANCE: &str = include_str!("../migrations/005_instance.sql");

const ALERTS: &str = include_str!("../migrations/004_alerts.sql");

const RETENTION: &str = include_str!("../migrations/003_loot_and_sessions.sql");

const SUMMARIES: &str = include_str!("../migrations/002_fight_summaries.sql");

const INITIAL: &str = include_str!("../migrations/001_initial.sql");

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn configure(conn: &Connection) -> Result<(), StoreError> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    if conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))? != 1 {
        return Err(StoreError::Migration);
    }
    Ok(())
}

pub(crate) fn check_schema(conn: &Connection) -> Result<i64, StoreError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StoreError::NewerSchema);
    }
    if !(0..=SCHEMA_VERSION).contains(&version) {
        return Err(StoreError::Migration);
    }
    if version == 0 {
        let objects: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if objects != 0 {
            return Err(StoreError::Migration);
        }
    }
    if version > 0 {
        let mut statement = conn
            .prepare("SELECT version,checksum FROM schema_migrations ORDER BY version LIMIT ?1")
            .map_err(|_| StoreError::Migration)?;
        let rows = statement
            .query_map([SCHEMA_VERSION + 1], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let expected: Vec<_> = [
            INITIAL,
            SUMMARIES,
            RETENTION,
            ALERTS,
            INSTANCE,
            TARGET_KINDS,
            PB_LOOKUPS,
            LOOT_BROWSING,
            CHAT,
            MAP_VISITS,
            EXCLUDE_JUNK_BAGS,
            EVENT_FTS,
            PLAYER_PROFILES,
            PARTY,
            LOOT_STATS_INDEXES,
            MAINTENANCE_RECEIPTS,
            ACCOUNTS,
        ]
        .iter()
        .take(version as usize)
        .enumerate()
        .map(|(i, script)| (i as i64 + 1, digest(script.as_bytes())))
        .collect();
        if rows != expected {
            return Err(StoreError::Migration);
        }
    }
    Ok(version)
}

/// Create the latest schema in an empty database or atomically upgrade an exact
/// known prior schema. Installed migration checksums are never rewritten.
/// File entry points must refuse v1–v10; only the confirmed HistoryUpgrade
/// owner may call this low-level primitive on installed legacy history.
pub fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    configure(conn)?;
    migrate_script(conn, INITIAL)
}

fn migrate_script(conn: &mut Connection, script: &str) -> Result<(), StoreError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version = check_schema(&tx)?;
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    if version == 0 {
        let objects: i64 = tx.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )?;
        if objects != 0 {
            return Err(StoreError::Migration);
        }
        tx.execute_batch(script)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(1,?1)",
            [digest(INITIAL.as_bytes())],
        )?;
    }
    if version < 2 {
        tx.execute_batch(SUMMARIES)
            .map_err(|_| StoreError::Migration)?;
        {
            // Stream a single legacy fight at a time; no retained detail collection.
            let mut statement = tx.prepare("SELECT length(CAST(json AS BLOB)),json FROM fights")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                if row.get::<_, i64>(0)? > 8 * 1024 * 1024 {
                    return Err(StoreError::Limit);
                }
                let fight: maxed_types::FightView =
                    serde_json::from_str(&row.get::<_, String>(1)?)?;
                crate::project::write_fight_summary(&tx, &fight)?;
            }
        }
        tx.execute(
            "INSERT INTO schema_migrations VALUES(2,?1)",
            [digest(SUMMARIES.as_bytes())],
        )?;
    }
    if version < 3 {
        tx.execute_batch(RETENTION)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(3,?1)",
            [digest(RETENTION.as_bytes())],
        )?;
    }
    if version < 4 {
        tx.execute_batch(ALERTS)
            .map_err(|_| StoreError::Migration)?;
        crate::alerts::seed(&tx)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(4,?1)",
            [digest(ALERTS.as_bytes())],
        )?;
    }
    if version < 5 {
        tx.execute_batch(INSTANCE)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(5,?1)",
            [digest(INSTANCE.as_bytes())],
        )?;
    }
    if version < 6 {
        tx.execute_batch(TARGET_KINDS)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(6,?1)",
            [digest(TARGET_KINDS.as_bytes())],
        )?;
    }
    if version < 7 {
        tx.execute_batch(PB_LOOKUPS)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(7,?1)",
            [digest(PB_LOOKUPS.as_bytes())],
        )?;
    }
    if version < 8 {
        tx.execute_batch(LOOT_BROWSING)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(8,?1)",
            [digest(LOOT_BROWSING.as_bytes())],
        )?;
    }
    if version < 9 {
        tx.execute_batch(CHAT).map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(9,?1)",
            [digest(CHAT.as_bytes())],
        )?;
    }
    if version < 10 {
        tx.execute_batch(MAP_VISITS)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(10,?1)",
            [digest(MAP_VISITS.as_bytes())],
        )?;
    }
    if version < 11 {
        tx.execute_batch(EXCLUDE_JUNK_BAGS)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(11,?1)",
            [digest(EXCLUDE_JUNK_BAGS.as_bytes())],
        )?;
    }
    if version < 12 {
        tx.execute_batch(EVENT_FTS)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(12,?1)",
            [digest(EVENT_FTS.as_bytes())],
        )?;
    }
    if version < 13 {
        tx.execute_batch(PLAYER_PROFILES)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(13,?1)",
            [digest(PLAYER_PROFILES.as_bytes())],
        )?;
    }
    if version < 14 {
        tx.execute_batch(PARTY).map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(14,?1)",
            [digest(PARTY.as_bytes())],
        )?;
    }
    if version < 15 {
        tx.execute_batch(LOOT_STATS_INDEXES)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(15,?1)",
            [digest(LOOT_STATS_INDEXES.as_bytes())],
        )?;
    }
    if version < 16 {
        tx.execute_batch(MAINTENANCE_RECEIPTS)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(16,?1)",
            [digest(MAINTENANCE_RECEIPTS.as_bytes())],
        )?;
    }
    if version < 17 {
        tx.execute_batch(ACCOUNTS)
            .map_err(|_| StoreError::Migration)?;
        tx.execute(
            "INSERT INTO schema_migrations VALUES(17,?1)",
            [digest(ACCOUNTS.as_bytes())],
        )?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}

/// Upgrade an existing history file under the caller's namespace lease.
/// This connection never creates a database or starts event recovery.
pub fn upgrade_history(path: &Path) -> Result<(), StoreError> {
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| StoreError::Open)?;
    require_compatible_history(&conn)?;
    migrate(&mut conn)
}

fn require_compatible_history(conn: &Connection) -> Result<(), StoreError> {
    if (1..11).contains(&check_schema(conn)?) {
        return Err(StoreError::HistoryUpgradeRequired);
    }
    Ok(())
}

/// Read-only native preflight. Missing/empty databases need no history backup;
/// only the explicit stopped upgrade owner may cross destructive legacy scripts.
pub fn history_preflight(path: &Path) -> Result<(), StoreError> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(StoreError::Open),
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(StoreError::Open)
        }
        _ => (),
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| StoreError::Open)?;
    configure(&conn)?;
    require_compatible_history(&conn)
}

pub(crate) fn open_writer(path: &Path) -> Result<Connection, StoreError> {
    if path.as_os_str().is_empty() || path == Path::new(":memory:") {
        return Err(StoreError::Open);
    }
    let mut conn = Connection::open(path).map_err(|_| StoreError::Open)?;
    configure(&conn)?;
    // Validate before changing the journal of an unrecognized database.
    require_compatible_history(&conn)?;
    migrate(&mut conn)?;
    let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(StoreError::Io);
    }
    conn.pragma_update(None, "synchronous", "FULL")?;
    if conn.query_row("PRAGMA synchronous", [], |r| r.get::<_, i64>(0))? != 2 {
        return Err(StoreError::Io);
    }
    Ok(conn)
}

/// A query-only connection intentionally confined to its owning thread.
pub struct StoreReader {
    pub(crate) conn: Connection,
    _thread_bound: PhantomData<Rc<()>>,
}

impl StoreReader {
    pub fn user_version(&self) -> Result<i64, StoreError> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }
}

pub fn open_reader(path: &Path) -> Result<StoreReader, StoreError> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| StoreError::Open)?;
    configure(&conn)?;
    if check_schema(&conn)? != SCHEMA_VERSION {
        return Err(StoreError::Migration);
    }
    conn.pragma_update(None, "query_only", true)?;
    Ok(StoreReader {
        conn,
        _thread_bound: PhantomData,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_reports_user_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        drop(open_writer(&path).unwrap());
        assert_eq!(
            open_reader(&path).unwrap().user_version().unwrap(),
            SCHEMA_VERSION
        );
    }

    #[test]
    fn failed_ddl_rolls_back_every_table_and_version() {
        let mut db = Connection::open_in_memory().unwrap();
        let script = format!("{INITIAL}\nINVALID INJECTED SQL;");
        assert!(migrate_script(&mut db, &script).is_err());
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_schema WHERE type='table'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        migrate(&mut db).unwrap();
    }

    #[test]
    fn connection_pragmas_and_read_only_are_enforced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("db");
        let writer = open_writer(&path).unwrap();
        assert_eq!(
            writer
                .query_row("PRAGMA synchronous", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            writer
                .query_row("PRAGMA busy_timeout", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            250
        );
        let reader = open_reader(&path).unwrap();
        assert_eq!(
            reader
                .conn
                .query_row("PRAGMA query_only", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            reader
                .conn
                .query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert!(reader.conn.execute("DELETE FROM runs", []).is_err());
    }
}
