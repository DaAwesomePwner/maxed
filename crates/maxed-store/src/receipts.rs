use crate::maintenance::{
    sql_count, Estimates, Job, JobExecutor, PauseReason, Phase, Processed, Record, UnitBudget,
    UnitOutcome,
};
use crate::StoreError;
use rusqlite::{params, types::ValueRef, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};

pub(crate) struct ConversionExecutor;
impl JobExecutor for ConversionExecutor {
    fn execute(
        &mut self,
        tx: &Transaction<'_>,
        job: &Job,
        budget: &mut UnitBudget<'_>,
    ) -> Result<UnitOutcome, StoreError> {
        if job.request.kind != "receipt_conversion" || job.request.format_version != 1 {
            return Err(StoreError::Maintenance);
        }
        let tables = Tables::read(tx)?;
        let mut outcome = UnitOutcome {
            processed: Some(Processed::default()),
            ..Default::default()
        };
        match job.status.phase {
            Phase::CopyingReceipts if tables.legacy_heads && tables.legacy_receipts => {
                // Discovery touches only the tiny heads and registry, never DISTINCT receipts.
                let stream: Option<String> = tx.prepare_cached(
                    "SELECT h.stream_id FROM stream_heads h LEFT JOIN event_streams s USING(stream_id)
                     WHERE s.stream_key IS NULL OR s.legacy_state IN ('pending','scanning','ready')
                     ORDER BY h.stream_id LIMIT 1"
                )?.query_row([], |r| r.get(0)).optional()?;
                if let Some(stream) = stream {
                    outcome.processed = Some(convert_stream(tx, &tables, &stream, budget)?);
                    // Reflect committed stream work, so a stuck unit cannot defeat
                    // the scheduler's no-progress guard. No observation text.
                    outcome.cursor = Some(tx.query_row(
                        "SELECT stream_key,legacy_state,legacy_cursor,legacy_scanned,legacy_moved
                         FROM event_streams WHERE stream_id=?1",
                        [&stream],
                        |r| {
                            Ok(format!(
                                "{}:{}:{}:{}:{}",
                                r.get::<_, i64>(0)?,
                                r.get::<_, String>(1)?,
                                r.get::<_, i64>(2)?,
                                r.get::<_, i64>(3)?,
                                r.get::<_, i64>(4)?
                            ))
                        },
                    )?);
                } else {
                    outcome.phase_complete = true;
                }
            }
            Phase::CopyingReceipts => outcome.phase_complete = true,
            Phase::Verifying | Phase::Deleting => {
                let blocked: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM event_streams WHERE legacy_error IS NOT NULL
                     OR legacy_count IS NOT NULL AND (legacy_state<>'done' OR legacy_count<>legacy_moved
                     OR legacy_moved<>legacy_verified OR legacy_inserted>legacy_moved))", [], |r| r.get(0))?;
                let remaining = tables.legacy_receipts
                    && tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM event_receipts LIMIT 1)",
                        [],
                        |r| r.get::<_, bool>(0),
                    )?;
                let missing_heads = tables.legacy_heads && tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM stream_heads h LEFT JOIN event_streams s USING(stream_id)
                     WHERE s.stream_key IS NULL OR s.legacy_state<>'done'
                     OR s.durable_sequence IS NULL OR s.durable_sequence<h.sequence)", [], |r| r.get::<_,bool>(0))?;
                if blocked || remaining || missing_heads {
                    outcome.deferred = Some(PauseReason::BlockedStream);
                } else {
                    if job.status.phase == Phase::Deleting {
                        // This phase is its own transaction. Do not reuse Tables after DROP.
                        if tables.legacy_receipts {
                            tx.execute_batch("DROP TABLE event_receipts")?;
                        }
                        if tables.legacy_heads {
                            tx.execute_batch("DROP TABLE stream_heads")?;
                        }
                        // Audit-only marker for this job. Tables schema introspection
                        // is the authority for every receipt reader and writer.
                        tx.execute(
                            "UPDATE maintenance_jobs SET receipt_format=2 WHERE job_id=?1",
                            [job.status.id],
                        )?;
                    }
                    outcome.phase_complete = true;
                }
            }
            _ => return Err(StoreError::Maintenance),
        }
        outcome.estimates = conversion_estimates(
            tx,
            tables.legacy_heads && job.status.phase != Phase::Deleting,
            tables.legacy_receipts && job.status.phase != Phase::Deleting,
        )?;
        Ok(outcome)
    }
}

fn conversion_estimates(
    tx: &Transaction<'_>,
    legacy_heads: bool,
    legacy_receipts: bool,
) -> Result<Estimates, StoreError> {
    let mut unknown = legacy_heads
        && tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM stream_heads h LEFT JOIN event_streams s USING(stream_id)
         WHERE s.legacy_count IS NULL)",
            [],
            |r| r.get::<_, bool>(0),
        )?;
    let (total, blocked, remaining, bytes, invalid): (u64,u64,i64,u64,bool) = tx.query_row(
        "SELECT coalesce(sum(legacy_count),0),
         coalesce(sum(CASE WHEN legacy_error IS NOT NULL THEN legacy_count-legacy_moved ELSE 0 END),0),
         coalesce(sum(legacy_count-legacy_moved),0),
         coalesce(sum(legacy_count*(length(CAST(stream_id AS BLOB))+72)),0),
         coalesce(max(legacy_error IS NOT NULL),0) FROM event_streams",
        [], |r| Ok((sql_count(r,0)?,sql_count(r,1)?,r.get(2)?,sql_count(r,3)?,r.get(4)?)))?;
    // Unaccounted/headless receipts prevent retirement and have unknown counts.
    if legacy_receipts
        && remaining == 0
        && tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM event_receipts LIMIT 1)",
            [],
            |r| r.get::<_, bool>(0),
        )?
    {
        unknown = true;
    }
    Ok(Estimates {
        eligible_records: (!unknown).then_some(total),
        // Valid legacy input is exactly 64 digest bytes + 8 sequence bytes +
        // UTF-8 stream ID bytes, matching reservation and converted-byte units.
        // Malformed input may have another size, so keep that estimate unknown.
        eligible_bytes: (!unknown && !invalid).then_some(bytes),
        blocked_records: (!unknown).then_some(blocked),
        ..Default::default()
    })
}

fn convert_stream(
    tx: &Transaction<'_>,
    tables: &Tables,
    stream: &str,
    budget: &mut UnitBudget<'_>,
) -> Result<Processed, StoreError> {
    let (key, head) = tables.intern_stream(tx, stream)?;
    let (state, start, mut error): (String, i64, Option<String>) = tx.query_row(
        "SELECT legacy_state,legacy_cursor,legacy_error FROM event_streams WHERE stream_key=?1",
        [key],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let copying = state == "ready";
    let mut cursor = start;
    let mut records = Vec::new();
    let mut scanned = 0_i64;
    let mut exhausted = false;
    loop {
        // Inspect only the next key/size before admission; never allocate a large
        // corrupt digest. The composite primary key supports every range seek.
        let next: Option<(i64, i64)> = tx
            .prepare_cached(
                "SELECT sequence,length(CAST(digest AS BLOB)) FROM event_receipts
             WHERE stream_id=?1 AND sequence>?2 ORDER BY sequence LIMIT 1",
            )?
            .query_row(params![stream, cursor], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let Some((sequence, bytes)) = next else {
            if scanned == 0 && !budget.reserve(Record::Receipt, stream.len() as u64 + 8) {
                return Ok(Processed::default());
            }
            exhausted = true;
            break;
        };
        if !budget.reserve(Record::Receipt, stream.len() as u64 + 8 + bytes as u64) {
            break;
        }
        let decoded = tx
            .prepare_cached("SELECT digest FROM event_receipts WHERE stream_id=?1 AND sequence=?2")?
            .query_row(params![stream, sequence], |r| {
                Ok(decode_legacy(r.get_ref(0)?))
            })?;
        let digest = match decoded {
            Ok(value) if head.is_some_and(|head| sequence <= head) => {
                let new = tx
                    .prepare_cached(
                        "SELECT digest FROM event_receipts_v2 WHERE stream_key=?1 AND sequence=?2",
                    )?
                    .query_row(params![key, sequence], |r| Ok(decode_binary(r.get_ref(0)?)))
                    .optional();
                match new {
                    Ok(Some(Ok(new))) if new != value => {
                        error = Some("receipt_mismatch".into());
                    }
                    Ok(Some(Err(_))) => {
                        error = Some("invalid_receipt".into());
                    }
                    Err(e) => return Err(e.into()),
                    _ => {}
                }
                Some(value)
            }
            _ => {
                error = Some("invalid_receipt".into());
                None
            }
        };
        scanned += 1;
        cursor = sequence;
        if copying {
            if let Some(digest) = digest {
                records.push((sequence, digest));
            }
            if error.is_some() {
                break;
            }
        }
    }
    if !copying {
        // Preflight all rows before deleting any in this stream, including a bad
        // tail beyond the first batch. Counting continues through malformed rows.
        tx.execute("UPDATE event_streams SET legacy_state=?2,legacy_cursor=?3,
            legacy_scanned=legacy_scanned+?4,legacy_count=CASE WHEN ?5 THEN legacy_scanned+?4 ELSE NULL END,
            legacy_error=?6 WHERE stream_key=?1",
            params![key,if exhausted {if error.is_some() {"blocked"} else {"ready"}} else {"scanning"},
                if exhausted {0} else {cursor},scanned,exhausted,error])?;
        return Ok(Processed::default());
    }
    if let Some(error) = error {
        tx.execute(
            "UPDATE event_streams SET legacy_state='blocked',legacy_error=?2 WHERE stream_key=?1",
            params![key, error],
        )?;
        return Ok(Processed::default());
    }
    let mut inserted = 0;
    for (sequence, digest) in &records {
        inserted += tx
            .prepare_cached(
                "INSERT INTO event_receipts_v2(stream_key,sequence,digest) VALUES(?1,?2,?3)
            ON CONFLICT(stream_key,sequence) DO NOTHING",
            )?
            .execute(params![key, sequence, digest])?;
    }
    // Verify the complete range: key order, digest bytes and exact count. Neither
    // INSERT OR IGNORE nor a checksum alone proves a pre-existing copy agrees.
    let mut statement = tx.prepare_cached(
        "SELECT v.sequence,v.digest FROM event_receipts l CROSS JOIN event_receipts_v2 v
        ON v.stream_key=?1 AND v.sequence=l.sequence
        WHERE l.stream_id=?4 AND l.sequence>?2 AND l.sequence<=?3 ORDER BY l.sequence",
    )?;
    let mut rows = statement.query(params![key, start, cursor, stream])?;
    let mut verified = 0;
    while let Some(row) = rows.next()? {
        let (sequence, digest) = records.get(verified).ok_or(StoreError::Maintenance)?;
        if row.get::<_, i64>(0)? != *sequence || decode_binary(row.get_ref(1)?)? != *digest {
            return Err(StoreError::Maintenance);
        }
        verified += 1;
    }
    if verified != records.len() {
        return Err(StoreError::Maintenance);
    }
    drop(rows);
    drop(statement);
    let mut statement = tx.prepare_cached(
        "SELECT sequence,digest FROM event_receipts
        WHERE stream_id=?1 AND sequence>?2 AND sequence<=?3 ORDER BY sequence",
    )?;
    let mut rows = statement.query(params![stream, start, cursor])?;
    for (sequence, digest) in &records {
        let row = rows.next()?.ok_or(StoreError::Maintenance)?;
        if row.get::<_, i64>(0)? != *sequence || decode_legacy(row.get_ref(1)?)? != *digest {
            return Err(StoreError::Maintenance);
        }
    }
    if rows.next()?.is_some() {
        return Err(StoreError::Maintenance);
    }
    drop(rows);
    drop(statement);
    let deleted = tx
        .prepare_cached(
            "DELETE FROM event_receipts WHERE stream_id=?1 AND sequence>?2 AND sequence<=?3",
        )?
        .execute(params![stream, start, cursor])?;
    if deleted != verified {
        return Err(StoreError::Maintenance);
    }
    tx.execute(
        "UPDATE event_streams SET legacy_cursor=?2,legacy_moved=legacy_moved+?3,
        legacy_verified=legacy_verified+?6,legacy_inserted=legacy_inserted+?4,legacy_state=?5
        WHERE stream_key=?1",
        params![
            key,
            cursor,
            deleted as i64,
            inserted as i64,
            if exhausted { "done" } else { "ready" },
            verified as i64
        ],
    )?;
    Ok(Processed {
        records: deleted as u64,
        bytes: deleted as u64 * (stream.len() as u64 + 72),
    })
}

/// Renewal inherits committed conversion totals from the tiny stream registry,
/// never by recounting receipts or adding historical job snapshots together.
pub(crate) fn conversion_progress(conn: &Connection) -> Result<Processed, StoreError> {
    Ok(conn.query_row(
        "SELECT coalesce(sum(legacy_moved),0),
         coalesce(sum(legacy_moved*(length(CAST(stream_id AS BLOB))+72)),0) FROM event_streams",
        [],
        |r| {
            Ok(Processed {
                records: sql_count(r, 0)?,
                bytes: sql_count(r, 1)?,
            })
        },
    )?)
}

pub(crate) type ReceiptDigest = [u8; 32];

pub(crate) fn digest(bytes: &[u8]) -> ReceiptDigest {
    Sha256::digest(bytes).into()
}

/// Decode the original hash bytes without trimming, coercion, or partial input.
pub(crate) fn decode_hex(hex: &[u8]) -> Result<ReceiptDigest, StoreError> {
    fn digit(byte: u8) -> Result<u8, StoreError> {
        match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            _ => Err(StoreError::InvalidEvent),
        }
    }
    if hex.len() != 64 {
        return Err(StoreError::InvalidEvent);
    }
    let mut bytes = [0; 32];
    for (out, pair) in bytes.iter_mut().zip(hex.as_chunks::<2>().0) {
        *out = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Ok(bytes)
}

pub(crate) fn decode_legacy(value: ValueRef<'_>) -> Result<ReceiptDigest, StoreError> {
    match value {
        ValueRef::Text(hex) => decode_hex(hex),
        _ => Err(StoreError::InvalidEvent),
    }
}

fn decode_binary(value: ValueRef<'_>) -> Result<ReceiptDigest, StoreError> {
    match value {
        ValueRef::Blob(bytes) => bytes.try_into().map_err(|_| StoreError::InvalidEvent),
        _ => Err(StoreError::InvalidEvent),
    }
}

/// Schema availability is read once per transaction. A DROP must use a separate
/// transaction or rebuild this value before any further receipt/head access.
/// This is the format authority; maintenance_jobs.receipt_format is audit-only.
#[derive(Default)]
pub(crate) struct Tables {
    registry: bool,
    legacy_receipts: bool,
    legacy_heads: bool,
}

impl Tables {
    pub(crate) fn read(conn: &Connection) -> Result<Self, StoreError> {
        let mut tables = Self::default();
        let mut statement = conn.prepare_cached("PRAGMA main.table_list")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            if row.get::<_, String>(2)? != "table" {
                continue;
            }
            match row.get::<_, String>(1)?.as_str() {
                "event_streams" => tables.registry = true,
                "event_receipts" => tables.legacy_receipts = true,
                "stream_heads" => tables.legacy_heads = true,
                _ => {}
            }
        }
        Ok(tables)
    }

    /// Check both copies before deciding a retry is exact. Also works after the
    /// verified conversion retires the legacy table; never infer absence from errors.
    /// Requires schema 016 or later (unlike the historical head helpers).
    pub(crate) fn lookup(
        &self,
        conn: &Connection,
        stream_id: &str,
        sequence: i64,
    ) -> Result<Option<ReceiptDigest>, StoreError> {
        let new = conn
            .prepare_cached(
                "SELECT r.digest FROM event_receipts_v2 r
             JOIN event_streams s USING(stream_key) WHERE s.stream_id=?1 AND r.sequence=?2",
            )?
            .query_row(params![stream_id, sequence], |r| {
                Ok(decode_binary(r.get_ref(0)?))
            })
            .optional()?
            .transpose()?;
        let old = if self.legacy_receipts {
            conn.prepare_cached(
                "SELECT digest FROM event_receipts WHERE stream_id=?1 AND sequence=?2",
            )?
            .query_row(params![stream_id, sequence], |r| {
                Ok(decode_legacy(r.get_ref(0)?))
            })
            .optional()?
            .transpose()?
        } else {
            None
        };
        if let (Some(old), Some(new)) = (old, new) {
            if old != new {
                return Err(StoreError::InvalidEvent);
            }
        }
        Ok(new.or(old))
    }

    /// Resolve the durable watermark without scanning receipts or regressing a
    /// live-advanced registry head while legacy heads are still being copied.
    pub(crate) fn head(
        &self,
        conn: &Connection,
        stream_id: &str,
    ) -> Result<Option<i64>, StoreError> {
        let new: Option<i64> = if self.registry {
            conn.prepare_cached("SELECT durable_sequence FROM event_streams WHERE stream_id=?1")?
                .query_row([stream_id], |r| r.get(0))
                .optional()?
                .flatten()
        } else {
            None
        };
        let old: Option<i64> = if self.legacy_heads {
            conn.prepare_cached("SELECT sequence FROM stream_heads WHERE stream_id=?1")?
                .query_row([stream_id], |r| r.get(0))
                .optional()?
        } else {
            None
        };
        if new.into_iter().chain(old).any(|sequence| sequence <= 0) {
            return Err(StoreError::InvalidEvent);
        }
        Ok(new.max(old))
    }

    /// Must run in the same transaction as acceptance or a conversion batch. The
    /// unique text ID allocates one key; repeated interning never lowers its head.
    pub(crate) fn intern_stream(
        &self,
        tx: &Transaction<'_>,
        stream_id: &str,
    ) -> Result<(i64, Option<i64>), StoreError> {
        let head = self.head(tx, stream_id)?;
        tx.prepare_cached(
            "INSERT INTO event_streams(stream_id,durable_sequence) VALUES(?1,?2)
         ON CONFLICT(stream_id) DO NOTHING",
        )?
        .execute(params![stream_id, head])?;
        let key = tx
            .prepare_cached("SELECT stream_key FROM event_streams WHERE stream_id=?1")?
            .query_row([stream_id], |r| r.get(0))?;
        if let Some(head) = head {
            advance_head(tx, key, head)?;
        }
        Ok((key, head))
    }
}

/// Resolve a single head for startup/reopen using the same rule as backups.
pub(crate) fn head(conn: &Connection, stream_id: &str) -> Result<Option<i64>, StoreError> {
    Tables::read(conn)?.head(conn, stream_id)
}

/// Enumerate only the small head/registry tables, never receipt rows. Historical
/// databases have no registry; converted databases have no legacy heads.
pub(crate) fn heads(conn: &Connection) -> Result<Vec<(String, i64)>, StoreError> {
    let tables = Tables::read(conn)?;
    let mut streams = std::collections::BTreeSet::<String>::new();
    for (exists, sql) in [
        (
            tables.registry,
            "SELECT stream_id FROM event_streams WHERE durable_sequence IS NOT NULL",
        ),
        (tables.legacy_heads, "SELECT stream_id FROM stream_heads"),
    ] {
        if exists {
            let mut statement = conn.prepare_cached(sql)?;
            for stream in statement.query_map([], |r| r.get(0))? {
                streams.insert(stream?);
            }
        }
    }
    streams
        .into_iter()
        .map(|stream| {
            let head = tables
                .head(conn, &stream)?
                .ok_or(StoreError::InvalidEvent)?;
            Ok((stream, head))
        })
        .collect()
}

pub(crate) fn advance_head(
    tx: &Transaction<'_>,
    stream_key: i64,
    sequence: i64,
) -> Result<(), StoreError> {
    tx.prepare_cached(
        "UPDATE event_streams SET durable_sequence=?2 WHERE stream_key=?1
         AND (durable_sequence IS NULL OR durable_sequence<?2)",
    )?
    .execute(params![stream_key, sequence])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_counts_original_rows_with_bounded_keyset_and_byte_admission() {
        use crate::maintenance::{Control, Environment, JobRequest, Limits, MaintenanceService};
        use std::time::Duration;
        struct Clock;
        impl Environment for Clock {
            fn monotonic(&self) -> Duration {
                Duration::ZERO
            }
            fn timestamp(&self) -> u64 {
                100
            }
            fn wal_size(&self, _: &Connection) -> Result<u64, StoreError> {
                Ok(0)
            }
        }
        for (receipts, bytes, expected) in [(3, 1024, 3), (1000, 170, 2)] {
            let mut db = Connection::open_in_memory().unwrap();
            crate::migrate(&mut db).unwrap();
            db.execute("INSERT INTO stream_heads VALUES('synthetic',9)", [])
                .unwrap();
            for n in 1..=9 {
                db.execute(
                    "INSERT INTO event_receipts VALUES('synthetic',?1,?2)",
                    params![n, "ab".repeat(32)],
                )
                .unwrap();
            }
            let mut service = MaintenanceService::new(
                Box::new(ConversionExecutor),
                Box::new(|_, _| Ok(true)),
                Box::new(Clock),
                Limits {
                    receipts,
                    bytes,
                    ..Default::default()
                },
            )
            .unwrap();
            let id = service
                .control(
                    &mut db,
                    Control::Request(JobRequest {
                        kind: "receipt_conversion".into(),
                        key: "synthetic".into(),
                        format_version: 1,
                        input_generation: "synthetic".into(),
                        input_revision: 0,
                    }),
                )
                .unwrap()
                .status
                .id;
            service.step(&mut db, true, &|| false).unwrap();
            for _ in 0..30 {
                let work_before: i64 = db
                    .query_row(
                        "SELECT coalesce(sum(legacy_scanned+legacy_moved),0) FROM event_streams",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                let before = service
                    .control(&mut db, Control::Status(id))
                    .unwrap()
                    .status;
                if before.phase == Phase::Reclaimable {
                    break;
                }
                service.step(&mut db, true, &|| false).unwrap();
                let after = service
                    .control(&mut db, Control::Status(id))
                    .unwrap()
                    .status;
                assert!(after.processed_records - before.processed_records <= expected);
                assert!(after.processed_bytes - before.processed_bytes <= bytes);
                let work_after: i64 = db
                    .query_row(
                        "SELECT coalesce(sum(legacy_scanned+legacy_moved),0) FROM event_streams",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert!(work_after - work_before <= expected as i64);
                if work_before == 0 {
                    assert_eq!(work_after, expected as i64);
                    assert_eq!(after.processed_records, 0);
                    assert_eq!(after.estimates.eligible_records, None);
                }
            }
            let after = service
                .control(&mut db, Control::Status(id))
                .unwrap()
                .status;
            assert_eq!(after.phase, Phase::Reclaimable);
            assert_eq!(after.estimates.eligible_records, Some(9));
            assert_eq!(after.processed_records, 9);
            assert_eq!(after.processed_bytes, 9 * ("synthetic".len() as u64 + 72));
            assert_eq!(after.estimates.eligible_bytes, Some(after.processed_bytes));
            assert_eq!(
                db.query_row(
                    "SELECT legacy_count,legacy_moved,legacy_verified FROM event_streams",
                    [],
                    |r| Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?
                    ))
                )
                .unwrap(),
                (9, 9, 9)
            );
        }
    }

    #[test]
    fn conversion_process_exit_rolls_back_each_boundary_or_reopens_committed_format() {
        use crate::backup::BackupEnvironment;
        use crate::maintenance::{Control, ConversionAccess, MaintenanceService, Step};
        use std::{cell::RefCell, path::Path, sync::atomic::AtomicBool};
        struct Space;
        impl BackupEnvironment for Space {
            fn available_space(&self, _: &Path) -> Option<u64> {
                Some(u64::MAX)
            }
            fn timestamp(&self) -> u64 {
                200
            }
        }
        fn owner(path: &Path) -> (Connection, MaintenanceService, i64) {
            let mut conn = crate::migrate::open_writer(path).unwrap();
            let access = ConversionAccess::new(path.to_path_buf());
            let request = access.request(100).unwrap();
            let mut service = access.service();
            let id = service
                .control(&mut conn, Control::Request(request))
                .unwrap()
                .status
                .id;
            access
                .backup(&Space, &AtomicBool::new(false), &mut || {})
                .unwrap();
            service
                .control(&mut conn, Control::AuthorizationChanged(id))
                .unwrap();
            (conn, service, id)
        }
        thread_local! { static CRASH: RefCell<String> = const { RefCell::new(String::new()) }; }
        fn trace(event: rusqlite::trace::TraceEvent<'_>) {
            let rusqlite::trace::TraceEvent::Stmt(stmt, _) = event else {
                return;
            };
            let sql = stmt.sql();
            let hit = CRASH.with_borrow(|at| match at.as_str() {
                "copy" => sql.starts_with("INSERT INTO event_receipts_v2"),
                "verify" => sql.starts_with("SELECT v.sequence,v.digest FROM event_receipts l"),
                "old_verify" => sql.starts_with("SELECT sequence,digest FROM event_receipts\n"),
                "delete" => sql.starts_with("DELETE FROM event_receipts WHERE"),
                "cursor" => sql.starts_with("UPDATE event_streams SET legacy_cursor"),
                "job" => sql.starts_with("UPDATE maintenance_jobs SET keyset_cursor"),
                "commit" | "retire_commit" => sql == "COMMIT",
                "heads" => sql == "DROP TABLE stream_heads",
                "marker" => sql.starts_with("UPDATE maintenance_jobs SET receipt_format"),
                _ => false,
            });
            if hit {
                std::process::exit(83);
            }
        }
        if let Ok(at) = std::env::var("MAXED_RECEIPT_CRASH_AT") {
            let path =
                std::path::PathBuf::from(std::env::var_os("MAXED_RECEIPT_CRASH_DB").unwrap());
            let (mut conn, mut service, _) = owner(&path);
            CRASH.set(at.clone());
            conn.trace_v2(
                rusqlite::trace::TraceEventCodes::SQLITE_TRACE_STMT,
                Some(trace),
            );
            assert!(matches!(
                service.step(&mut conn, true, &|| false).unwrap(),
                Step::Committed(_)
            ));
            if at == "committed" || at == "retired" {
                std::process::exit(83);
            }
            panic!("crash boundary was not reached");
        }
        for at in [
            "copy",
            "verify",
            "old_verify",
            "delete",
            "cursor",
            "job",
            "commit",
            "committed",
            "heads",
            "marker",
            "retire_commit",
            "retired",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("live.sqlite3");
            let mut db = crate::migrate::open_writer(&path).unwrap();
            let events: Vec<_> = (1..=5)
                .map(|n| maxed_types::EventEnvelope {
                    fight_chart_omitted: false,
                    stream_id: "synthetic".into(),
                    sequence: maxed_types::Sequence(maxed_types::Timestamp(n)),
                    at: maxed_types::Timestamp(0),
                    event: maxed_types::EngineEvent::CaptureStateChanged(Default::default()),
                })
                .collect();
            crate::apply_batch(&mut db, &events).unwrap();
            db.execute_batch("INSERT INTO event_receipts SELECT s.stream_id,r.sequence,lower(hex(r.digest)) FROM event_receipts_v2 r JOIN event_streams s USING(stream_key);
                INSERT INTO stream_heads SELECT stream_id,durable_sequence FROM event_streams;
                DELETE FROM event_receipts_v2; DELETE FROM event_streams;").unwrap();
            drop(db);
            let (mut db, mut service, id) = owner(&path);
            let retiring = matches!(at, "heads" | "marker" | "retire_commit" | "retired");
            for _ in 0..100 {
                service.step(&mut db, true, &|| false).unwrap();
                let ready: bool = db
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM event_streams WHERE legacy_state='ready')",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                let deleting = service
                    .control(&mut db, Control::Status(id))
                    .unwrap()
                    .status
                    .phase
                    == Phase::Deleting;
                if retiring && deleting || !retiring && ready {
                    break;
                }
            }
            let before = service
                .control(&mut db, Control::Status(id))
                .unwrap()
                .status;
            drop(service);
            drop(db);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "receipts::tests::conversion_process_exit_rolls_back_each_boundary_or_reopens_committed_format"])
                .env("MAXED_RECEIPT_CRASH_AT", at).env("MAXED_RECEIPT_CRASH_DB", &path)
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().unwrap();
            assert_eq!(status.code(), Some(83), "{at}");
            let (mut db, mut service, id) = owner(&path);
            let after = service
                .control(&mut db, Control::Status(id))
                .unwrap()
                .status;
            if at != "committed" && at != "retired" {
                assert_eq!(after, before, "uncommitted progress escaped at {at}");
            }
            assert_eq!(Tables::read(&db).unwrap().legacy_receipts, at != "retired");
            if !retiring {
                let moved: i64 = db
                    .query_row("SELECT legacy_moved FROM event_streams", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(moved, if at == "committed" { 5 } else { 0 });
            }
            for _ in 0..100 {
                let changes = db.total_changes();
                crate::apply_batch(&mut db, &events).unwrap();
                assert_eq!(db.total_changes(), changes);
                for event in &events {
                    let mut changed = event.clone();
                    changed.at = maxed_types::Timestamp(1);
                    assert!(matches!(
                        crate::apply_batch(&mut db, &[changed]),
                        Err(StoreError::InvalidEvent)
                    ));
                }
                if service
                    .control(&mut db, Control::Status(id))
                    .unwrap()
                    .status
                    .phase
                    == Phase::Reclaimable
                {
                    break;
                }
                service.step(&mut db, true, &|| false).unwrap();
            }
            assert!(!Tables::read(&db).unwrap().legacy_receipts);
            assert_eq!(
                db.query_row(
                    "SELECT receipt_format FROM maintenance_jobs WHERE job_id=?1",
                    [id],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                2
            );
        }
    }

    #[test]
    fn canonical_receipt_bytes_match_unchanged_legacy_digest() {
        let event = maxed_types::EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic".into(),
            sequence: maxed_types::Sequence(maxed_types::Timestamp(1)),
            at: maxed_types::Timestamp(0),
            event: maxed_types::EngineEvent::CaptureStateChanged(Default::default()),
        };
        let mut value = serde_json::to_value(&event).unwrap();
        value.sort_all_objects();
        let canonical = serde_json::to_vec(&value).unwrap();
        let legacy = crate::migrate::digest(&canonical);
        assert_eq!(legacy.len(), 64);
        assert_eq!(legacy, legacy.to_ascii_lowercase());
        assert_eq!(decode_hex(legacy.as_bytes()).unwrap(), digest(&canonical));
        assert_eq!(
            crate::project::validate_event(&event).unwrap().1,
            digest(&canonical)
        );
        assert!(matches!(
            decode_hex(legacy.to_ascii_uppercase().as_bytes()),
            Err(StoreError::InvalidEvent)
        ));
        for invalid in [
            ValueRef::Null,
            ValueRef::Integer(1),
            ValueRef::Real(1.0),
            ValueRef::Blob(legacy.as_bytes()),
        ] {
            assert!(matches!(
                decode_legacy(invalid),
                Err(StoreError::InvalidEvent)
            ));
        }
    }

    #[test]
    fn interning_and_late_head_copy_never_lower_a_live_head() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::migrate(&mut db).unwrap();
        db.execute("INSERT INTO stream_heads VALUES('synthetic',5)", [])
            .unwrap();
        let tx = db.transaction().unwrap();
        let tables = Tables::read(&tx).unwrap();
        let (key, head) = tables.intern_stream(&tx, "synthetic").unwrap();
        assert_eq!(head, Some(5));
        advance_head(&tx, key, 9).unwrap();
        advance_head(&tx, key, 5).unwrap();
        assert_eq!(
            tables.intern_stream(&tx, "synthetic").unwrap(),
            (key, Some(9))
        );
        tx.commit().unwrap();
        assert_eq!(super::head(&db, "synthetic").unwrap(), Some(9));
    }
}
