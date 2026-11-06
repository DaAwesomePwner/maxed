//! Reconstruct installed v4 projections, without ever installing v5 triggers.
use crate::common::receipts;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::Path;

pub fn copy_v4(source: &Path, db: &Connection) {
    for (i, script) in [
        include_str!("../../migrations/001_initial.sql"),
        include_str!("../../migrations/002_fight_summaries.sql"),
        include_str!("../../migrations/003_loot_and_sessions.sql"),
        include_str!("../../migrations/004_alerts.sql"),
    ]
    .iter()
    .enumerate()
    {
        db.execute_batch(script).unwrap();
        db.execute(
            "INSERT INTO schema_migrations VALUES(?1,?2)",
            params![
                i as i64 + 1,
                format!("{:x}", Sha256::digest(script.as_bytes()))
            ],
        )
        .unwrap();
    }
    db.pragma_update(None, "user_version", 4).unwrap();
    db.execute("ATTACH DATABASE ?1 AS seed", [source.to_str().unwrap()])
        .unwrap();
    let tables = db
        .prepare(
            "SELECT name FROM main.sqlite_schema WHERE type='table' AND name<>'schema_migrations'",
        )
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    db.execute_batch("BEGIN").unwrap();
    for table in tables {
        if matches!(table.as_str(), "event_receipts" | "stream_heads") {
            continue;
        }
        let columns = if table == "drops" {
            "id,session_id,run_id,at,bag_type,bag_rank,json"
        } else {
            "*"
        };
        db.execute_batch(&format!(
            "INSERT INTO main.\"{table}\" SELECT {columns} FROM seed.\"{table}\""
        ))
        .unwrap();
    }
    let source =
        Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    receipts::copy_into_legacy(&source, db);
    assert_eq!(receipts::rows(db), receipts::rows(&source));
    assert_eq!(receipts::heads(db), receipts::heads(&source));
    db.execute_batch("UPDATE fight_summaries SET json=json_remove(json,'$.kill_count','$.unattributed_damage'); COMMIT; DETACH DATABASE seed;")
        .unwrap();
}
