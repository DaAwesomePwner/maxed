use crate::common;
use maxed_store::{apply_batch, migrate, start_writer, StoreError, WriterConfig};
use maxed_store::{backup::*, maintenance::*};
use maxed_types::*;
use rusqlite::{params, types::Value, Connection};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct BackupSpace;
impl BackupEnvironment for BackupSpace {
    fn available_space(&self, _: &Path) -> Option<u64> {
        Some(u64::MAX)
    }
    fn timestamp(&self) -> u64 {
        200
    }
}

#[derive(Default)]
struct BudgetClock {
    ticks: AtomicU64,
    tick_millis: u64,
}
impl Environment for BudgetClock {
    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.ticks.fetch_add(self.tick_millis, Ordering::Relaxed))
    }
    fn timestamp(&self) -> u64 {
        200
    }
}

fn conversion_owner(path: &Path) -> StoppedMaintenance {
    // These are semantic tests, not fsync-throughput gates. Keep real SQLite,
    // backup authorization, and record/byte caps, but don't shrink batch sizes
    // based on contention from unrelated tests. Slow budgets are covered below.
    StoppedMaintenance::receipt_conversion_with_environment_for_tests(
        path,
        Box::<BudgetClock>::default(),
    )
    .unwrap()
}

fn legacy_file(
    streams: usize,
    rows: u64,
) -> (tempfile::TempDir, std::path::PathBuf, Vec<EventEnvelope>) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let tx = db.transaction().unwrap();
    let mut events = Vec::new();
    for stream in 0..streams {
        for sequence in 1..=rows {
            let mut event = capture(sequence);
            event.stream_id = format!("synthetic-{stream}");
            tx.execute(
                "INSERT INTO event_receipts VALUES(?1,?2,?3)",
                params![event.stream_id, sequence as i64, legacy_digest(&event)],
            )
            .unwrap();
            events.push(event);
        }
        tx.execute(
            "INSERT INTO stream_heads VALUES(?1,?2)",
            params![format!("synthetic-{stream}"), rows as i64],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    (dir, path, events)
}

fn exists(db: &Connection, table: &str) -> bool {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name=?1 AND type='table')",
        [table],
        |r| r.get(0),
    )
    .unwrap()
}

fn assert_retries(db: &mut Connection, events: &[EventEnvelope]) {
    let changes = db.total_changes();
    apply_batch(db, events).unwrap();
    assert_eq!(
        db.total_changes(),
        changes,
        "every exact retry remains a no-op"
    );
    for event in events {
        let mut changed = event.clone();
        changed.at = Timestamp(9876);
        assert!(matches!(
            apply_batch(db, &[changed]),
            Err(StoreError::InvalidEvent)
        ));
    }
}

fn step_until(owner: &mut StoppedMaintenance, mut reached: impl FnMut(&Step) -> bool) {
    // A hang watchdog, not a throughput assertion: unit sizes use an injected
    // budget clock so parallel test load cannot force thousands of tiny commits.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let step = owner.step().unwrap();
        if reached(&step) {
            return;
        }
        assert!(Instant::now() < deadline, "conversion timed out: {step:?}");
        if matches!(step, Step::Idle | Step::Deferred(_)) {
            std::thread::sleep(Duration::from_millis(1));
        } else {
            std::thread::yield_now();
        }
    }
}

fn reclaimed(step: &Step) -> bool {
    matches!(step, Step::Committed(status) if status.phase == Phase::Reclaimable)
}

fn finish_conversion(owner: &mut StoppedMaintenance, id: i64) -> JobStatus {
    step_until(owner, reclaimed);
    owner.control(Control::Status(id)).unwrap().status
}

#[test]
fn conversion_slow_units_advance_validation_before_public_progress() {
    let (_dir, path, events) = legacy_file(1, 32);
    let mut db = Connection::open(&path).unwrap();
    let mut owner = StoppedMaintenance::receipt_conversion_with_environment_for_tests(
        &path,
        Box::new(BudgetClock {
            ticks: AtomicU64::new(0),
            tick_millis: 20,
        }),
    )
    .unwrap();
    let id = owner.request_receipt_conversion(100).unwrap().status.id;
    owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    assert!(matches!(
        owner.step().unwrap(),
        Step::Committed(status) if status.phase == Phase::CopyingReceipts
    ));
    // Every clock tick exceeds the production 10 ms target. Even after adaptive
    // reduction reaches one record per unit, validation must keep advancing.
    for scanned in 1..=32 {
        let step = owner.step().unwrap();
        assert!(
            matches!(step, Step::Committed(ref status)
            if status.phase == Phase::CopyingReceipts && status.processed_records == 0),
            "validation unit {scanned}: {step:?}"
        );
        assert_eq!(
            db.query_row("SELECT legacy_scanned FROM event_streams", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            scanned
        );
        assert_eq!(count(&db, "event_receipts"), 32);
        assert_eq!(count(&db, "event_receipts_v2"), 0);
    }
    // Copying is independently bounded and advances the public count only once.
    for copied in 1..=32 {
        let step = owner.step().unwrap();
        assert!(
            matches!(step, Step::Committed(ref status)
            if status.processed_records == copied),
            "copy unit {copied}: {step:?}"
        );
        assert_eq!(count(&db, "event_receipts_v2"), copied as i64);
    }
    let status = finish_conversion(&mut owner, id);
    assert_eq!(status.processed_records, 32);
    assert!(!exists(&db, "event_receipts"));
    assert_retries(&mut db, &events);
}

#[test]
fn conversion_public_progress_counts_only_converted_rows_across_streams_and_units() {
    let (_dir, path, events) = legacy_file(2, 1001);
    let mut owner = conversion_owner(&path);
    let id = owner.request_receipt_conversion(100).unwrap().status.id;
    owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    let status = finish_conversion(&mut owner, id);
    assert_eq!(status.processed_records, 2002);
    assert_eq!(
        status.estimates.eligible_records,
        Some(status.processed_records)
    );
    let bytes = events
        .iter()
        .map(|e| e.stream_id.len() as u64 + 8 + 64)
        .sum::<u64>();
    assert_eq!(status.processed_bytes, bytes);
    assert_eq!(status.estimates.eligible_bytes, Some(bytes));
}

#[test]
fn conversion_renews_deleted_backup_and_supersedes_old_job_without_recopying() {
    for mid_copy in [false, true] {
        let (_dir, path, events) = legacy_file(1, 1001);
        let mut db = Connection::open(&path).unwrap();
        let mut owner = conversion_owner(&path);
        let old = owner.request_receipt_conversion(100).unwrap().status.id;
        let backup = owner
            .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
            .unwrap();
        if mid_copy {
            step_until(&mut owner, |_| count(&db, "event_receipts") < 1001);
        }
        let remaining = count(&db, "event_receipts");
        if mid_copy {
            assert!((1..1001).contains(&remaining));
        }
        owner.control(Control::Pause(old)).unwrap();
        drop(owner);
        let mut backups = BackupStore::open(&path).unwrap();
        backups.delete(&db, backup.id, &BackupSpace).unwrap();
        drop(backups);
        let mut owner = conversion_owner(&path);
        if mid_copy {
            // The manifest renewal is already durable if the writer transaction
            // fails. Retrying after restart must join that renewal, atomically
            // retiring the predecessor and inheriting its committed stream work.
            db.execute_batch(
                "CREATE TRIGGER fail_renewal BEFORE UPDATE ON maintenance_jobs
                WHEN NEW.safe_error='superseded' BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
            )
            .unwrap();
            assert!(owner.request_receipt_conversion(300).is_err());
            assert_eq!(count(&db, "maintenance_jobs"), 1);
            assert_eq!(
                owner.control(Control::Status(old)).unwrap().status.reason,
                Some(PauseReason::Cancelled)
            );
            db.execute_batch("DROP TRIGGER fail_renewal").unwrap();
            drop(owner);
            owner = conversion_owner(&path);
        }
        let renewed = owner.request_receipt_conversion(300).unwrap();
        assert_eq!(renewed.request, Some(RequestDisposition::Created));
        let id = renewed.status.id;
        assert_ne!(id, old);
        assert_eq!(renewed.status.processed_records, (1001 - remaining) as u64);
        assert!(
            renewed.status.paused,
            "renewal must preserve explicit cancellation"
        );
        let retired = owner.control(Control::Resume(old)).unwrap().status;
        assert!(retired.paused, "a superseded job can never run again");
        owner.control(Control::Resume(id)).unwrap();
        assert_eq!(
            owner.step().unwrap(),
            Step::Deferred(PauseReason::AwaitingBackup)
        );
        assert_eq!(count(&db, "event_receipts"), remaining);
        let new_backup = owner
            .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
            .unwrap();
        assert_ne!(new_backup.id, backup.id);
        assert_eq!(
            new_backup.slot,
            if mid_copy {
                Slot::Rolling
            } else {
                Slot::Initial
            }
        );
        let manifest =
            Connection::open(path.parent().unwrap().join("backups/live/manifest.sqlite3")).unwrap();
        assert!(manifest
            .query_row(
                "SELECT completed_order>requested_order FROM backups b
            JOIN requests r ON r.id=b.request_id WHERE b.id=?1",
                [new_backup.id.0],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
        drop(manifest);
        let status = finish_conversion(&mut owner, id);
        assert_eq!(status.processed_records, 1001);
        assert_eq!(status.estimates.eligible_records, Some(1001));
        assert_eq!(
            status.estimates.eligible_bytes,
            Some(status.processed_bytes)
        );
        assert_eq!(
            db.query_row("SELECT legacy_inserted FROM event_streams", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1001
        );
        assert_eq!(owner.control(Control::Status(old)).unwrap().status, retired);
        assert_retries(&mut db, &events);
    }
}

#[test]
fn conversion_completed_request_stays_complete_after_backup_deletion() {
    let (_dir, path, _) = legacy_file(1, 3);
    let db = Connection::open(&path).unwrap();
    let mut owner = conversion_owner(&path);
    let id = owner.request_receipt_conversion(100).unwrap().status.id;
    let backup = owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    let completed = finish_conversion(&mut owner, id);
    drop(owner);
    let mut backups = BackupStore::open(&path).unwrap();
    backups.delete(&db, backup.id, &BackupSpace).unwrap();
    drop(backups);
    let mut owner = conversion_owner(&path);
    let result = owner.request_receipt_conversion(300).unwrap();
    assert_eq!(result.request, Some(RequestDisposition::AlreadyComplete));
    assert_eq!(result.status, completed);
    assert_eq!(count(&db, "maintenance_jobs"), 1);
}

#[test]
fn conversion_head_advance_between_count_and_copy_and_mid_copy_preserves_all_retries() {
    for mid_copy in [false, true] {
        let (_dir, path, mut events) = legacy_file(1, 1001);
        let mut db = Connection::open(&path).unwrap();
        let mut owner = conversion_owner(&path);
        let id = owner.request_receipt_conversion(100).unwrap().status.id;
        owner
            .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
            .unwrap();
        step_until(&mut owner, |_| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM event_streams
                WHERE legacy_state='ready' AND legacy_count=1001 AND
                CASE WHEN ?1 THEN legacy_moved>0 AND legacy_moved<1001 ELSE legacy_moved=0 END)",
                [mid_copy],
                |r| r.get(0),
            )
            .unwrap()
        });
        for n in 1002..=1003 {
            let mut next = events[0].clone();
            next.sequence = Sequence(Timestamp(n));
            apply_batch(&mut db, &[next.clone()]).unwrap();
            events.push(next);
        }
        assert_retries(&mut db, &events);
        step_until(&mut owner, |step| {
            assert_eq!(
                db.query_row("SELECT durable_sequence FROM event_streams", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1003
            );
            reclaimed(step)
        });
        assert_eq!(count(&db, "event_receipts_v2"), 1003);
        assert_retries(&mut db, &events);
        let status = owner.control(Control::Status(id)).unwrap().status;
        assert_eq!(status.processed_records, 1001);
        assert_eq!(status.estimates.eligible_records, Some(1001));
    }
}

#[test]
fn conversion_mid_copy_block_counts_remaining_rows_and_survives_restart_without_backup() {
    let (_dir, path, events) = legacy_file(1, 1001);
    let db = Connection::open(&path).unwrap();
    let mut owner = conversion_owner(&path);
    let id = owner.request_receipt_conversion(100).unwrap().status.id;
    let backup = owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    step_until(&mut owner, |_| count(&db, "event_receipts") < 1001);
    let remaining = count(&db, "event_receipts");
    assert!((1..1001).contains(&remaining));
    let first_remaining = 1002 - remaining;
    db.execute(
        "UPDATE event_receipts SET digest='malformed' WHERE sequence=?1",
        [first_remaining],
    )
    .unwrap();
    step_until(&mut owner, |step| {
        *step == Step::Deferred(PauseReason::BlockedStream)
    });
    let blocked = owner.control(Control::Status(id)).unwrap().status;
    assert_eq!(blocked.reason, Some(PauseReason::BlockedStream));
    assert_eq!(blocked.estimates.blocked_records, Some(remaining as u64));
    assert_eq!(blocked.processed_records, (1001 - remaining) as u64);
    drop(owner);
    let mut owner = conversion_owner(&path);
    assert_eq!(owner.control(Control::Status(id)).unwrap().status, blocked);
    assert_eq!(owner.step().unwrap(), Step::Idle);
    drop(owner);
    let mut backups = BackupStore::open(&path).unwrap();
    backups.delete(&db, backup.id, &BackupSpace).unwrap();
    drop(backups);
    db.execute(
        "UPDATE event_receipts SET digest=?1 WHERE sequence=?2",
        params![
            legacy_digest(&events[first_remaining as usize - 1]),
            first_remaining
        ],
    )
    .unwrap();
    let mut owner = conversion_owner(&path);
    let new = owner.request_receipt_conversion(300).unwrap().status.id;
    assert_ne!(new, id);
    assert_eq!(
        owner.step().unwrap(),
        Step::Deferred(PauseReason::AwaitingBackup)
    );
    owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    assert_eq!(finish_conversion(&mut owner, new).processed_records, 1001);
}

#[test]
fn conversion_own_backup_atomic_ranges_and_final_reopen_keep_every_retry() {
    let (_dir, path, events) = legacy_file(2, 7);
    let mut db = Connection::open(&path).unwrap();
    // A matching partial copy is accounted once, without overwriting its digest.
    seed_v2(&db, &events[0]);
    let mut owner = conversion_owner(&path);
    let requested = owner.request_receipt_conversion(100).unwrap();
    assert_eq!(requested.request, Some(RequestDisposition::Created));
    let id = requested.status.id;
    assert_eq!(owner.request_receipt_conversion(150).unwrap().status.id, id);
    assert_eq!(
        owner.step().unwrap(),
        Step::Deferred(PauseReason::AwaitingBackup)
    );
    assert_eq!(count(&db, "event_receipts"), 14);
    let backup = owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    assert_eq!(backup.slot, Slot::Initial);
    let mut advanced = false;
    step_until(&mut owner, |step| {
        assert_retries(&mut db, &events);
        if !advanced {
            let mut next = events[0].clone();
            next.sequence = Sequence(Timestamp(8));
            apply_batch(&mut db, &[next]).unwrap();
            advanced = true;
        }
        reclaimed(step)
    });
    assert_eq!(
        owner.control(Control::Status(id)).unwrap().status.phase,
        Phase::Reclaimable
    );
    assert!(!exists(&db, "event_receipts"));
    assert!(!exists(&db, "stream_heads"));
    let totals: (i64, i64, i64, i64) = db.query_row("SELECT sum(legacy_count),sum(legacy_moved),sum(legacy_verified),sum(legacy_inserted) FROM event_streams", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
    assert_eq!(totals, (14, 14, 14, 13));
    assert_eq!(count(&db, "event_receipts_v2"), 15);
    assert_eq!(
        db.query_row(
            "SELECT receipt_format FROM maintenance_jobs WHERE job_id=?1",
            [id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        owner.request_receipt_conversion(160).unwrap().request,
        Some(RequestDisposition::AlreadyComplete)
    );
    drop(owner);
    drop(db);
    let writer = start_writer(
        path.clone(),
        WriterConfig {
            stream_id: events[0].stream_id.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(writer.flush().unwrap().sequence.0 .0, 8);
    writer.shutdown().unwrap();
    let mut db = Connection::open(&path).unwrap();
    assert_retries(&mut db, &events);
    assert_eq!(
        db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(db.query_row("SELECT count(*) FROM event_streams WHERE lifecycle<>'resumable' OR receipt_floor<>0 OR sealed_at IS NOT NULL", [], |r| r.get::<_,i64>(0)).unwrap(), 0);
}

#[test]
fn conversion_bad_tail_blocks_only_its_stream_without_deleting_any_of_it() {
    let (_dir, path, events) = legacy_file(3, 1001);
    let db = Connection::open(&path).unwrap();
    db.execute(
        "UPDATE event_receipts SET digest='malformed' WHERE stream_id=?1 AND sequence=1001",
        [&events[0].stream_id],
    )
    .unwrap();
    let mut changed = events[2001].clone();
    changed.at = Timestamp(999);
    seed_v2(&db, &changed);
    let mut owner = conversion_owner(&path);
    let id = owner.request_receipt_conversion(100).unwrap().status.id;
    owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    let mut before = count(&db, "event_receipts");
    step_until(&mut owner, |step| {
        let after = count(&db, "event_receipts");
        assert!(
            before - after <= 1000,
            "one unit exceeds the receipt ceiling"
        );
        before = after;
        *step == Step::Deferred(PauseReason::BlockedStream)
    });
    let status = owner.control(Control::Status(id)).unwrap().status;
    assert_eq!(status.reason, Some(PauseReason::BlockedStream));
    assert_eq!(status.estimates.blocked_records, Some(2002));
    assert_eq!(count(&db, "event_receipts"), 2002);
    for stream in [&events[0].stream_id, &events[1001].stream_id] {
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM event_receipts WHERE stream_id=?1",
                [stream],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1001
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT sum(legacy_count),sum(legacy_moved) FROM event_streams",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        )
        .unwrap(),
        (3003, 1001)
    );
}

#[test]
fn conversion_legacy_upgrade_backup_cannot_authorize_and_new_backup_is_rolling() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let db = Connection::open(&path).unwrap();
    common::installed_schema::install(&db, 10);
    seed_legacy(&db, &capture(1));
    drop(db);
    HistoryUpgrade::open(&path)
        .unwrap()
        .confirm_and_upgrade(&BackupSpace, &AtomicBool::new(false))
        .unwrap();
    let mut owner = conversion_owner(&path);
    owner.request_receipt_conversion(300).unwrap();
    assert_eq!(
        owner.step().unwrap(),
        Step::Deferred(PauseReason::AwaitingBackup)
    );
    let db = Connection::open(&path).unwrap();
    assert_eq!(count(&db, "event_receipts"), 1);
    let backup = owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    assert_eq!(backup.slot, Slot::Rolling);
    step_until(&mut owner, reclaimed);
    assert!(!exists(&db, "event_receipts"));
}

#[test]
fn conversion_schema_records_lossless_progress_without_startup_work() {
    let db = database();
    let columns: Vec<String> = db
        .prepare("PRAGMA table_info(event_streams)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        columns.iter().any(|c| c == "legacy_count"),
        "conversion must retain its original bounded count"
    );
    assert!(
        columns.iter().any(|c| c == "legacy_moved"),
        "conversion must atomically account for retired rows"
    );
    assert_eq!(count(&db, "maintenance_jobs"), 0);
}

#[test]
fn conversion_accepts_v2_only_keys_inside_a_sparse_legacy_range() {
    let (_dir, path, events) = legacy_file(1, 5);
    let mut db = Connection::open(&path).unwrap();
    seed_v2(&db, &events[0]);
    db.execute("DELETE FROM event_receipts WHERE sequence=1", [])
        .unwrap();
    let mut owner = conversion_owner(&path);
    owner.request_receipt_conversion(100).unwrap();
    owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    step_until(&mut owner, reclaimed);
    assert!(!exists(&db, "event_receipts"));
    assert_eq!(count(&db, "event_receipts_v2"), 5);
    assert_retries(&mut db, &events);
}

#[test]
fn conversion_failures_roll_back_copy_verification_delete_cursor_and_commit() {
    for boundary in [
        "copy",
        "verify",
        "old_verify",
        "delete",
        "cursor",
        "job",
        "commit",
        "retire",
    ] {
        let (_dir, path, events) = legacy_file(1, 7);
        let mut owner = conversion_owner(&path);
        let id = owner.request_receipt_conversion(100).unwrap().status.id;
        owner
            .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
            .unwrap();
        let mut db = Connection::open(&path).unwrap();
        step_until(&mut owner, |step| {
            let ready: bool = db
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM event_streams WHERE legacy_state='ready')",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let deleting =
                matches!(step, Step::Committed(status) if status.phase == Phase::Deleting);
            boundary == "retire" && deleting || boundary != "retire" && ready
        });
        let before = owner.control(Control::Status(id)).unwrap().status;
        let old = count(&db, "event_receipts");
        let new = count(&db, "event_receipts_v2");
        let progress: (i64, i64, i64) = db
            .query_row(
                "SELECT legacy_cursor,legacy_moved,legacy_verified FROM event_streams",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let trigger = match boundary {
            "copy" => "CREATE TRIGGER fault AFTER INSERT ON event_receipts_v2 BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
            "verify" => "CREATE TRIGGER fault AFTER INSERT ON event_receipts_v2 BEGIN UPDATE event_receipts_v2 SET digest=zeroblob(32) WHERE stream_key=NEW.stream_key AND sequence=NEW.sequence; END;",
            "old_verify" => "CREATE TRIGGER fault AFTER INSERT ON event_receipts_v2 BEGIN UPDATE event_receipts SET digest=lower(hex(zeroblob(32))) WHERE sequence=NEW.sequence; END;",
            "delete" => "CREATE TRIGGER fault AFTER DELETE ON event_receipts BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
            "cursor" => "CREATE TRIGGER fault AFTER UPDATE OF legacy_moved ON event_streams BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
            "job" => "CREATE TRIGGER fault AFTER UPDATE OF keyset_cursor ON maintenance_jobs BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
            "retire" => "CREATE TRIGGER fault BEFORE UPDATE OF receipt_format ON maintenance_jobs BEGIN SELECT RAISE(ABORT,'synthetic'); END;",
            _ => "CREATE TABLE synthetic_parent(id INTEGER PRIMARY KEY); CREATE TABLE synthetic_child(id INTEGER REFERENCES synthetic_parent DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER fault AFTER DELETE ON event_receipts BEGIN INSERT INTO synthetic_child VALUES(99); END;",
        };
        db.execute_batch(trigger).unwrap();
        assert!(owner.step().is_err(), "{boundary} must reject the unit");
        assert!(exists(&db, "event_receipts") && exists(&db, "stream_heads"));
        assert_eq!(count(&db, "event_receipts"), old);
        assert_eq!(count(&db, "event_receipts_v2"), new);
        assert_eq!(
            db.query_row(
                "SELECT legacy_cursor,legacy_moved,legacy_verified FROM event_streams",
                [],
                |r| Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?
                ))
            )
            .unwrap(),
            progress
        );
        let after = owner.control(Control::Status(id)).unwrap().status;
        assert_eq!(after.phase, before.phase);
        assert_eq!(after.processed_records, before.processed_records);
        assert_retries(&mut db, &events);
        db.execute_batch("DROP TRIGGER fault").unwrap();
        owner.control(Control::Resume(id)).unwrap();
        step_until(&mut owner, |step| {
            assert_retries(&mut db, &events);
            reclaimed(step)
        });
        assert!(!exists(&db, "event_receipts"));
    }
}

#[test]
fn conversion_cancel_reopen_and_duplicate_request_keep_committed_progress() {
    let (_dir, path, events) = legacy_file(1, 2001);
    let mut owner = conversion_owner(&path);
    let id = owner.request_receipt_conversion(100).unwrap().status.id;
    owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    let db = Connection::open(&path).unwrap();
    step_until(&mut owner, |_| count(&db, "event_receipts") < 2001);
    let remaining = count(&db, "event_receipts");
    assert!((1..2001).contains(&remaining));
    let before = owner.control(Control::Pause(id)).unwrap().status;
    drop(owner);
    let mut owner = conversion_owner(&path);
    owner
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
        .unwrap();
    assert_eq!(
        owner.request_receipt_conversion(150).unwrap().status,
        before
    );
    assert_eq!(owner.step().unwrap(), Step::Idle);
    assert_eq!(count(&db, "event_receipts"), remaining);
    owner.control(Control::Resume(id)).unwrap();
    step_until(&mut owner, reclaimed);
    assert!(!exists(&db, "event_receipts"));
    let mut db = db;
    assert_retries(&mut db, &events);
    assert_eq!(count(&db, "event_receipts_v2"), 2001);
}

#[test]
fn conversion_empty_legacy_and_headless_rows_are_not_permission_to_erase_receipts() {
    for orphan in [false, true] {
        let (_dir, path, _) = legacy_file(0, 0);
        let db = Connection::open(&path).unwrap();
        if orphan {
            db.execute(
                "INSERT INTO event_receipts VALUES('synthetic-orphan',1,?1)",
                [legacy_digest(&capture(1))],
            )
            .unwrap();
        }
        let mut owner = conversion_owner(&path);
        let id = owner.request_receipt_conversion(100).unwrap().status.id;
        owner
            .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {})
            .unwrap();
        step_until(&mut owner, |step| {
            *step == Step::Deferred(PauseReason::BlockedStream) || reclaimed(step)
        });
        assert_eq!(exists(&db, "event_receipts"), orphan);
        assert_eq!(
            owner.control(Control::Status(id)).unwrap().status.reason,
            if orphan {
                Some(PauseReason::BlockedStream)
            } else {
                None
            }
        );
        if orphan {
            assert_eq!(count(&db, "event_receipts"), 1);
            assert_eq!(
                owner
                    .control(Control::Status(id))
                    .unwrap()
                    .status
                    .estimates
                    .eligible_records,
                None
            );
        }
    }
}

#[test]
fn conversion_default_writer_api_runs_backup_outside_capture_and_enqueues_nothing_on_open() {
    let (dir, path, events) = legacy_file(1, 23);
    let writer = start_writer(
        path.clone(),
        WriterConfig {
            stream_id: events[0].stream_id.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    let db = Connection::open(&path).unwrap();
    assert_eq!(count(&db, "maintenance_jobs"), 0);
    assert!(!dir.path().join("backups").exists());
    let id = writer.request_receipt_conversion(100).unwrap().status.id;
    writer.maintenance(Control::Pause(id)).unwrap();
    let mut next = events[0].clone();
    next.sequence = Sequence(Timestamp(24));
    let mut during_backup = Some(next);
    writer
        .backup_receipt_conversion(&BackupSpace, &AtomicBool::new(false), &mut || {
            if let Some(event) = during_backup.take() {
                crate::common::submit(&writer, event);
            }
            writer.flush().unwrap();
        })
        .unwrap();
    assert!(
        during_backup.is_none(),
        "backup must yield while capture commits"
    );
    assert!(
        writer
            .maintenance(Control::Status(id))
            .unwrap()
            .status
            .paused
    );
    assert_eq!(writer.flush().unwrap().sequence.0 .0, 24);
    writer.maintenance(Control::Resume(id)).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while writer
        .maintenance(Control::Status(id))
        .unwrap()
        .status
        .phase
        != Phase::Reclaimable
    {
        assert!(
            std::time::Instant::now() < deadline,
            "writer conversion did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!exists(&db, "event_receipts"));
    // The same live connection has executed retirement DDL and must accept its
    // next sequence without preparing SQL against the absent legacy tables.
    let mut next = events[0].clone();
    next.sequence = Sequence(Timestamp(25));
    crate::common::submit(&writer, next.clone());
    assert_eq!(writer.flush().unwrap().sequence.0 .0, 25);
    writer.shutdown().unwrap();
    let mut db = db;
    assert_retries(&mut db, &events);
    assert_retries(&mut db, &[next]);
    assert_eq!(count(&db, "event_receipts_v2"), 25);
}

fn database() -> Connection {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    db
}

fn capture(sequence: u64) -> EventEnvelope {
    common::envelope(
        sequence,
        EngineEvent::CaptureStateChanged(CaptureStatus::default()),
    )
}

fn legacy_digest(event: &EventEnvelope) -> String {
    let mut value = serde_json::to_value(event).unwrap();
    value.sort_all_objects();
    format!("{:x}", Sha256::digest(serde_json::to_vec(&value).unwrap()))
}

fn bytes(event: &EventEnvelope) -> Vec<u8> {
    legacy_digest(event)
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn registry_head(db: &Connection) -> Option<i64> {
    db.query_row(
        "SELECT durable_sequence FROM event_streams WHERE stream_id='test-stream'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn seed_legacy(db: &Connection, event: &EventEnvelope) {
    db.execute(
        "INSERT INTO event_receipts VALUES(?1,?2,?3)",
        params![
            event.stream_id,
            event.sequence.0 .0 as i64,
            legacy_digest(event)
        ],
    )
    .unwrap();
    db.execute(
        "INSERT INTO stream_heads VALUES(?1,?2)",
        params![event.stream_id, event.sequence.0 .0 as i64],
    )
    .unwrap();
}

fn seed_v2(db: &Connection, event: &EventEnvelope) {
    db.execute(
        "INSERT INTO event_streams(stream_id,durable_sequence) VALUES(?1,?2)",
        params![event.stream_id, event.sequence.0 .0 as i64],
    )
    .unwrap();
    db.execute(
        "INSERT INTO event_receipts_v2 VALUES(?1,?2,?3)",
        params![
            db.last_insert_rowid(),
            event.sequence.0 .0 as i64,
            bytes(event)
        ],
    )
    .unwrap();
}

#[test]
fn mixed_new_receipts_are_binary_canonical_digests_without_legacy_writes() {
    let mut db = database();
    let events = common::story_events();
    apply_batch(&mut db, &events).unwrap();
    assert_eq!(count(&db, "event_receipts_v2"), events.len() as i64);
    assert_eq!(count(&db, "event_receipts"), 0);
    assert_eq!(count(&db, "stream_heads"), 0);
    assert_eq!(count(&db, "event_streams"), 1);
    assert_eq!(registry_head(&db), Some(events.len() as i64));
    for event in &events {
        let (kind, stored): (String, Vec<u8>) = db
            .query_row(
                "SELECT typeof(digest),digest FROM event_receipts_v2 WHERE sequence=?1",
                [event.sequence.0 .0 as i64],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let hex = legacy_digest(event);
        assert_eq!(hex.len(), 64);
        assert!(hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        assert_eq!(kind, "blob");
        assert_eq!(stored.len(), 32);
        assert_eq!(stored, bytes(event));
    }
}

#[test]
fn mixed_exact_changed_retries_and_gaps_in_old_new_and_dual_tables() {
    for (old, new) in [(true, false), (false, true), (true, true)] {
        let mut db = database();
        let event = capture(1);
        if old {
            seed_legacy(&db, &event);
        }
        if new {
            seed_v2(&db, &event);
        }
        let changes = db.total_changes();
        apply_batch(&mut db, std::slice::from_ref(&event)).unwrap();
        assert_eq!(
            db.total_changes(),
            changes,
            "an exact retry must be a no-op"
        );
        let mut changed = event;
        changed.at = Timestamp(7);
        assert!(matches!(
            apply_batch(&mut db, &[changed]),
            Err(StoreError::InvalidEvent)
        ));
        assert!(matches!(
            apply_batch(&mut db, &[capture(3)]),
            Err(StoreError::SequenceGap)
        ));
        apply_batch(&mut db, &[capture(2)]).unwrap();
        assert_eq!(count(&db, "event_receipts"), i64::from(old));
        assert_eq!(count(&db, "stream_heads"), i64::from(old));
        assert_eq!(count(&db, "event_receipts_v2"), i64::from(new) + 1);
        assert_eq!(count(&db, "event_streams"), 1);
        assert_eq!(registry_head(&db), Some(2));
        if old {
            assert_eq!(
                db.query_row("SELECT sequence FROM stream_heads", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }
}

#[test]
fn mixed_disagreeing_dual_copies_fail_even_when_either_matches() {
    let mut db = database();
    let event = capture(1);
    seed_legacy(&db, &event);
    let mut changed = event.clone();
    changed.at = Timestamp(7);
    seed_v2(&db, &changed);
    for retry in [event, changed] {
        assert!(matches!(
            apply_batch(&mut db, &[capture(2), retry]),
            Err(StoreError::InvalidEvent)
        ));
        assert_eq!(count(&db, "event_receipts_v2"), 1);
        assert_eq!(registry_head(&db), Some(1));
    }
}

#[test]
fn mixed_malformed_legacy_digest_length_hex_and_sqlite_type_fail_closed() {
    let event = capture(1);
    let hex = legacy_digest(&event);
    for bad in [
        Value::Text(hex.to_ascii_uppercase()),
        Value::Text(hex[..63].into()),
        Value::Text(format!("{hex}0")),
        Value::Text(format!("g{}", &hex[1..])),
        Value::Text(format!(" {}", &hex[1..])),
        Value::Blob(hex.as_bytes().to_vec()),
        Value::Integer(42),
    ] {
        for dual in [false, true] {
            let mut db = database();
            seed_legacy(&db, &event);
            db.execute("UPDATE event_receipts SET digest=?1", [&bad])
                .unwrap();
            if dual {
                seed_v2(&db, &event);
            }
            assert!(matches!(
                apply_batch(&mut db, std::slice::from_ref(&event)),
                Err(StoreError::InvalidEvent)
            ));
            assert_eq!(count(&db, "event_receipts"), 1);
            assert_eq!(count(&db, "event_receipts_v2"), i64::from(dual));
        }
    }
}

#[test]
fn mixed_malformed_v2_digest_length_and_sqlite_type_fail_closed() {
    for bad in [
        Value::Blob(vec![0; 31]),
        Value::Blob(vec![0; 33]),
        Value::Text("0".repeat(32)),
    ] {
        let mut db = database();
        let event = capture(1);
        seed_legacy(&db, &event);
        seed_v2(&db, &event);
        db.pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        db.execute("UPDATE event_receipts_v2 SET digest=?1", [&bad])
            .unwrap();
        assert!(matches!(
            apply_batch(&mut db, &[event]),
            Err(StoreError::InvalidEvent)
        ));
        assert_eq!(registry_head(&db), Some(1));
    }
}

#[test]
fn mixed_null_initial_head_accepts_only_one_and_capture_prefix_has_no_projection() {
    let mut db = database();
    db.execute(
        "INSERT INTO event_streams(stream_id) VALUES('test-stream')",
        [],
    )
    .unwrap();
    assert_eq!(registry_head(&db), None);
    assert!(matches!(
        apply_batch(&mut db, &[capture(2)]),
        Err(StoreError::SequenceGap)
    ));
    assert_eq!(registry_head(&db), None);
    apply_batch(&mut db, &[capture(1)]).unwrap();
    assert_eq!(registry_head(&db), Some(1));
    assert_eq!(count(&db, "play_sessions"), 0);
    assert_eq!(count(&db, "runs"), 0);
    assert_eq!(count(&db, "event_receipts_v2"), 1);
}

#[test]
fn mixed_legacy_head_initializes_before_acceptance_without_receipt_scan() {
    for initial in [None, Some(2), Some(7)] {
        let mut db = database();
        db.execute("INSERT INTO stream_heads VALUES('test-stream',5)", [])
            .unwrap();
        db.execute(
            "INSERT INTO event_streams(stream_id,durable_sequence) VALUES('test-stream',?1)",
            [initial],
        )
        .unwrap();
        let expected = initial.unwrap_or(0).max(5);
        assert!(matches!(
            apply_batch(&mut db, &[capture(expected as u64 + 2)]),
            Err(StoreError::SequenceGap)
        ));
        assert_eq!(registry_head(&db), initial);
        apply_batch(&mut db, &[capture(expected as u64 + 1)]).unwrap();
        assert_eq!(registry_head(&db), Some(expected + 1));
        assert_eq!(count(&db, "event_receipts_v2"), 1);
        assert_eq!(count(&db, "event_receipts"), 0);
    }
}

#[test]
fn mixed_projection_and_commit_failure_roll_back_receipt_registry_and_head() {
    for legacy in [false, true] {
        let mut db = database();
        if legacy {
            seed_legacy(&db, &capture(1));
        }
        let first = if legacy { 2 } else { 1 };
        let mut bad = common::story_events()[4].clone();
        bad.sequence = Sequence(Timestamp(first + 1));
        assert!(apply_batch(&mut db, &[capture(first), bad]).is_err());
        assert_eq!(count(&db, "event_streams"), 0);
        assert_eq!(count(&db, "event_receipts_v2"), 0);
        // A deferred foreign key fails at COMMIT, after projection and receipt/head writes.
        db.execute_batch("CREATE TABLE commit_fault(parent INTEGER REFERENCES event_streams(stream_key) DEFERRABLE INITIALLY DEFERRED);
            CREATE TRIGGER fail_commit AFTER INSERT ON event_receipts_v2 BEGIN INSERT INTO commit_fault VALUES(-1); END;").unwrap();
        assert!(apply_batch(&mut db, &[capture(first)]).is_err());
        assert_eq!(count(&db, "event_streams"), 0);
        assert_eq!(count(&db, "event_receipts_v2"), 0);
        assert_eq!(count(&db, "commit_fault"), 0);
        assert_eq!(count(&db, "event_receipts"), i64::from(legacy));
        db.execute_batch("DROP TRIGGER fail_commit").unwrap();
        apply_batch(&mut db, &[capture(first)]).unwrap();
        assert_eq!(registry_head(&db), Some(first as i64));
    }
}

#[test]
fn mixed_writer_reopen_uses_legacy_then_advanced_registry_head() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    seed_legacy(&db, &capture(1));
    let config = WriterConfig {
        stream_id: "test-stream".into(),
        ..Default::default()
    };
    let writer = start_writer(path.clone(), config.clone()).unwrap();
    assert_eq!(writer.status().durable, Some(Sequence(Timestamp(1))));
    common::submit(&writer, capture(1));
    common::submit(&writer, capture(2));
    assert_eq!(writer.shutdown().unwrap().sequence.0 .0, 2);
    let writer = start_writer(path, config).unwrap();
    assert_eq!(writer.status().durable, Some(Sequence(Timestamp(2))));
    common::submit(&writer, capture(1));
    common::submit(&writer, capture(2));
    assert_eq!(writer.flush().unwrap().sequence.0 .0, 2);
    common::submit(&writer, capture(3));
    assert_eq!(writer.shutdown().unwrap().sequence.0 .0, 3);
    assert_eq!(registry_head(&db), Some(3));
    assert_eq!(count(&db, "event_receipts"), 1);
    assert_eq!(count(&db, "event_receipts_v2"), 2);
}

#[test]
fn mixed_empty_writer_keeps_null_head_and_resumes_after_legacy_tables_retire() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    db.execute(
        "INSERT INTO event_streams(stream_id) VALUES('test-stream')",
        [],
    )
    .unwrap();
    let config = WriterConfig {
        stream_id: "test-stream".into(),
        ..Default::default()
    };
    let writer = start_writer(path.clone(), config.clone()).unwrap();
    assert_eq!(writer.status().durable, None);
    assert_eq!(writer.shutdown().unwrap().sequence.0 .0, 0);
    assert_eq!(registry_head(&db), None);
    apply_batch(&mut db, &[capture(1)]).unwrap();
    db.execute_batch("DROP TABLE event_receipts; DROP TABLE stream_heads;")
        .unwrap();
    let writer = start_writer(path, config).unwrap();
    assert_eq!(writer.status().durable, Some(Sequence(Timestamp(1))));
    common::submit(&writer, capture(1));
    common::submit(&writer, capture(2));
    assert_eq!(writer.shutdown().unwrap().sequence.0 .0, 2);
    assert_eq!(registry_head(&db), Some(2));
    assert_eq!(count(&db, "event_receipts_v2"), 2);
}

#[test]
fn mixed_table_discovery_is_per_transaction_and_refreshes_after_retirement() {
    use rusqlite::trace::{TraceEvent, TraceEventCodes};
    use std::cell::Cell;
    thread_local! { static DISCOVERIES: Cell<usize> = const { Cell::new(0) }; }
    fn trace(event: TraceEvent<'_>) {
        if let TraceEvent::Stmt(statement, _) = event {
            if statement.sql().contains("table_list") {
                DISCOVERIES.with(|count| count.set(count.get() + 1));
            }
        }
    }
    let mut db = database();
    db.trace_v2(TraceEventCodes::SQLITE_TRACE_STMT, Some(trace));
    apply_batch(&mut db, &[capture(1), capture(2), capture(3)]).unwrap();
    assert_eq!(DISCOVERIES.with(|count| count.replace(0)), 1);
    db.execute_batch("DROP TABLE event_receipts; DROP TABLE stream_heads;")
        .unwrap();
    apply_batch(&mut db, &[capture(3), capture(4), capture(5)]).unwrap();
    assert_eq!(DISCOVERIES.with(|count| count.replace(0)), 1);
    assert_eq!(registry_head(&db), Some(5));
}

#[test]
fn mixed_late_party_failure_rolls_back_profiles_receipts_and_head() {
    let mut db = database();
    apply_batch(&mut db, &common::story_events()).unwrap();
    let profile: PlayerProfileView = serde_json::from_value(serde_json::json!({
        "scope_id": "demo-run", "run_id": "demo-run", "player_id": "synthetic-player",
        "object_id": 101, "is_local": false, "name": "Synthetic", "stats": [], "equipment": [],
        "first_seen": Timestamp(100), "last_seen": Timestamp(200)
    }))
    .unwrap();
    let events = [
        common::envelope(8, EngineEvent::PlayerObserved(profile)),
        common::envelope(9, EngineEvent::RunEnded(demo::demo_run_story().run)),
    ];
    db.execute_batch("CREATE TRIGGER fail_party BEFORE INSERT ON party_run_members BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    assert!(apply_batch(&mut db, &events).is_err());
    assert_eq!(registry_head(&db), Some(7));
    assert_eq!(count(&db, "event_receipts_v2"), 7);
    for table in ["player_profiles", "party_players", "party_run_members"] {
        assert_eq!(count(&db, table), 0);
    }
    db.execute_batch("DROP TRIGGER fail_party").unwrap();
    apply_batch(&mut db, &events).unwrap();
    assert_eq!(registry_head(&db), Some(9));
    assert_eq!(count(&db, "event_receipts_v2"), 9);
    assert_eq!(count(&db, "party_run_members"), 1);
}
