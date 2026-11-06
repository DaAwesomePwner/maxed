//! Logical receipt assertions shared by store and native runtime tests.
#![allow(dead_code)]
use rusqlite::{params, Connection};
use std::collections::BTreeMap;

pub fn rows(db: &Connection) -> Vec<(String, i64, Vec<u8>)> {
    let mut receipts = BTreeMap::new();
    let mut insert = |stream, sequence, digest: Vec<u8>| {
        assert_eq!(digest.len(), 32);
        if let Some(previous) = receipts.insert((stream, sequence), digest.clone()) {
            assert_eq!(previous, digest, "dual receipt copies must agree");
        }
    };
    if exists(db, "event_receipts") {
        let mut stmt = db
            .prepare("SELECT stream_id,sequence,digest FROM event_receipts")
            .unwrap();
        for row in stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .unwrap()
        {
            let (stream, sequence, hex) = row.unwrap();
            assert_eq!(hex.len(), 64);
            assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
            let digest = hex
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            insert(stream, sequence, digest);
        }
    }
    if exists(db, "event_receipts_v2") {
        let mut stmt = db.prepare("SELECT s.stream_id,r.sequence,r.digest FROM event_receipts_v2 r LEFT JOIN event_streams s USING(stream_key)").unwrap();
        for row in stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            })
            .unwrap()
        {
            let (stream, sequence, digest) = row.unwrap();
            insert(stream, sequence, digest);
        }
    }
    receipts
        .into_iter()
        .map(|((stream, sequence), digest)| (stream, sequence, digest))
        .collect()
}

pub fn count(db: &Connection) -> i64 {
    rows(db).len() as i64
}

pub fn count_stream(db: &Connection, stream: &str) -> i64 {
    rows(db).iter().filter(|(id, _, _)| id == stream).count() as i64
}

pub fn lookup(db: &Connection, stream: &str, sequence: i64) -> Option<Vec<u8>> {
    rows(db)
        .into_iter()
        .find(|(id, seq, _)| id == stream && *seq == sequence)
        .map(|(_, _, digest)| digest)
}

pub fn heads(db: &Connection) -> BTreeMap<String, i64> {
    let mut heads: BTreeMap<String, i64> = BTreeMap::new();
    for (table, column) in [
        ("stream_heads", "sequence"),
        ("event_streams", "durable_sequence"),
    ] {
        if exists(db, table) {
            let mut stmt = db
                .prepare(&format!(
                    "SELECT stream_id,{column} FROM {table} WHERE {column} IS NOT NULL"
                ))
                .unwrap();
            for row in stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .unwrap()
            {
                let (stream, head) = row.unwrap();
                assert!(head > 0);
                heads
                    .entry(stream)
                    .and_modify(|previous| *previous = (*previous).max(head))
                    .or_insert(head);
            }
        }
    }
    heads
}

pub fn head(db: &Connection, stream: &str) -> Option<i64> {
    heads(db).get(stream).copied()
}

fn exists(db: &Connection, table: &str) -> bool {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    )
    .unwrap()
}

/// Preserve logical proofs when a synthetic current fixture is downgraded into
/// an installed legacy schema. This is test setup, never a production writer.
pub fn copy_to_legacy(db: &Connection) {
    copy_into_legacy(db, db);
}

pub fn copy_into_legacy(source: &Connection, db: &Connection) {
    let receipts = rows(source);
    let heads = heads(source);
    for (stream, sequence, digest) in receipts {
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        db.execute("INSERT INTO event_receipts VALUES(?1,?2,?3) ON CONFLICT(stream_id,sequence) DO NOTHING", params![stream, sequence, hex]).unwrap();
    }
    for (stream, sequence) in heads {
        db.execute("INSERT INTO stream_heads VALUES(?1,?2) ON CONFLICT(stream_id) DO UPDATE SET sequence=max(sequence,excluded.sequence)", params![stream, sequence]).unwrap();
    }
}
