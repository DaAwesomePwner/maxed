use maxed_store::{
    backup::BackupUsage,
    maintenance::{Estimates, JobStatus, PauseReason, Phase},
    storage_usage::*,
};
use maxed_types::*;
use rusqlite::Connection;
use std::fs;

fn directory_files(dir: &std::path::Path) -> Vec<std::ffi::OsString> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    files.sort();
    files
}

#[test]
fn existing_reader_reports_live_logical_pages_without_scanning_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
        CREATE TABLE synthetic(bytes); INSERT INTO synthetic VALUES(zeroblob(65536));
        DELETE FROM synthetic;",
    )
    .unwrap();
    let reader = maxed_store::open_reader(&path).unwrap();
    let before = directory_files(dir.path());
    let changes = db.total_changes();
    assert_logical_unknown(&measure_database(&path));
    let logical = reader.logical_storage().unwrap();
    assert!(logical.freelist_count.unwrap().0 > 0);
    assert_eq!(
        logical.page_count.unwrap().0,
        db.pragma_query_value(None, "page_count", |r| r.get::<_, i64>(0))
            .unwrap() as u64
    );
    assert_eq!(
        logical.reusable_bytes,
        page_bytes(logical.page_size, logical.freelist_count.map(|n| n.0))
    );
    assert_eq!(logical.auto_vacuum, Some(StorageAutoVacuum::None));
    assert_eq!(db.total_changes(), changes);
    assert_eq!(directory_files(dir.path()), before);
    assert!(reader.maintenance_statuses().unwrap().is_empty());
}

fn wal_writer(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE synthetic(n); INSERT INTO synthetic VALUES(zeroblob(8192));").unwrap();
    conn
}

#[test]
fn wal_measurement_after_clean_close_preserves_files_and_main_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    wal_writer(&path).close().unwrap();
    let files = directory_files(dir.path());
    assert_eq!(files, vec![std::ffi::OsString::from("live.sqlite3")]);
    let main = fs::read(&path).unwrap();
    let usage = measure_database(&path);
    assert_eq!(directory_files(dir.path()), files);
    assert_eq!(fs::read(&path).unwrap(), main);
    assert_eq!(usage.journal_mode, Some(StorageJournalMode::Wal));
    assert_eq!(usage.allocated_bytes, Some(StorageCount(main.len() as u64)));
    assert!(usage.page_count.unwrap().0 > 0);
}

#[test]
fn wal_measurement_with_writer_preserves_lengths_and_main_wal_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let _writer = wal_writer(&path);
    let files = directory_files(dir.path());
    assert_eq!(files.len(), 3);
    let main = fs::read(&path).unwrap();
    let wal_path = dir.path().join("live.sqlite3-wal");
    let wal = fs::read(&wal_path).unwrap();
    let shm_path = dir.path().join("live.sqlite3-shm");
    let shm_len = fs::metadata(&shm_path).unwrap().len();
    assert!(wal.len() > 32);
    let usage = measure_database(&path);
    assert_eq!(usage.main_bytes, Some(StorageCount(main.len() as u64)));
    assert_eq!(usage.wal_bytes, Some(StorageCount(wal.len() as u64)));
    assert_eq!(usage.shm_bytes, Some(StorageCount(shm_len)));
    assert_eq!(directory_files(dir.path()), files);
    assert_eq!(fs::read(&path).unwrap(), main);
    assert_eq!(fs::read(&wal_path).unwrap(), wal);
    assert_eq!(fs::metadata(&shm_path).unwrap().len(), shm_len);
    assert_logical_unknown(&usage);
}

#[test]
fn wal_measurement_of_crash_copy_preserves_files_and_main_wal_bytes() {
    let source = tempfile::tempdir().unwrap();
    let _writer = wal_writer(&source.path().join("live.sqlite3"));
    let copy = tempfile::tempdir().unwrap();
    let files = directory_files(source.path());
    assert_eq!(files.len(), 3);
    for name in &files {
        fs::copy(source.path().join(name), copy.path().join(name)).unwrap();
    }
    let path = copy.path().join("live.sqlite3");
    let main = fs::read(&path).unwrap();
    let wal_path = copy.path().join("live.sqlite3-wal");
    let wal = fs::read(&wal_path).unwrap();
    assert!(wal.len() > 32);
    let usage = measure_database(&path);
    assert_eq!(directory_files(copy.path()), files);
    assert_eq!(fs::read(&path).unwrap(), main);
    assert_eq!(fs::read(&wal_path).unwrap(), wal);
    assert_logical_unknown(&usage);
}

fn assert_logical_unknown(usage: &DatabaseStorage) {
    assert_eq!(usage.page_size, None);
    assert_eq!(usage.page_count, None);
    assert_eq!(usage.freelist_count, None);
    assert_eq!(usage.allocated_bytes, None);
    assert_eq!(usage.reusable_bytes, None);
    assert_eq!(usage.auto_vacuum, None);
    assert_eq!(usage.journal_mode, None);
}

#[test]
fn missing_empty_and_error_measurements_do_not_create_or_migrate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let missing = measure_database(&path);
    assert_eq!(missing.state, StorageDatabaseState::Absent);
    assert_eq!(missing.main_bytes, None);
    assert_eq!(missing.reusable_bytes, None);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    fs::write(&path, []).unwrap();
    let empty = measure_database(&path);
    assert_eq!(empty.state, StorageDatabaseState::Present);
    assert_eq!(empty.main_bytes, Some(StorageCount(0)));
    assert_logical_unknown(&empty);
    assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    fs::write(&path, b"invalid synthetic database").unwrap();
    let invalid = measure_database(&path);
    assert_eq!(invalid.main_bytes, Some(StorageCount(26)));
    assert_eq!(invalid.page_count, None);
    assert_eq!(invalid.reusable_bytes, None);
    assert_eq!(invalid.auto_vacuum, None);
    let inaccessible = measure_database(dir.path());
    assert_eq!(inaccessible.state, StorageDatabaseState::Unavailable);
    assert_eq!(inaccessible.main_bytes, None);
    assert_eq!(inaccessible.page_size, None);
}

#[test]
fn cheap_read_measures_none_and_incremental_without_migrating_or_scanning_tables() {
    for (mode, expected) in [
        (0, StorageAutoVacuum::None),
        (2, StorageAutoVacuum::Incremental),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite3");
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "auto_vacuum", mode).unwrap();
        conn.execute_batch("PRAGMA user_version=12; CREATE TABLE synthetic(bytes BLOB); INSERT INTO synthetic VALUES(zeroblob(131072)); DELETE FROM synthetic;
            CREATE VIEW event_receipts AS SELECT * FROM nonexistent;").unwrap();
        let before = fs::read(&path).unwrap();
        let usage = measure_database(&path);
        assert_eq!(usage.state, StorageDatabaseState::Present);
        assert_eq!(usage.auto_vacuum, Some(expected));
        assert_eq!(usage.journal_mode, None);
        assert_eq!(usage.main_bytes, Some(StorageCount(before.len() as u64)));
        assert_eq!(usage.wal_bytes, Some(StorageCount(0)));
        assert_eq!(usage.shm_bytes, Some(StorageCount(0)));
        let pages = conn
            .query_row("PRAGMA page_count", [], |r| r.get::<_, i64>(0))
            .unwrap() as u64;
        let free = conn
            .query_row("PRAGMA freelist_count", [], |r| r.get::<_, i64>(0))
            .unwrap() as u64;
        assert!(free > 0);
        assert_eq!(usage.page_count, Some(StorageCount(pages)));
        assert_eq!(usage.freelist_count, Some(StorageCount(free)));
        assert_eq!(
            usage.allocated_bytes,
            page_bytes(usage.page_size, Some(pages))
        );
        assert_eq!(
            usage.reusable_bytes,
            page_bytes(usage.page_size, Some(free))
        );
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            12
        );
    }
}

#[test]
fn wal_and_shm_are_separate_file_lengths_and_metadata_errors_stay_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE synthetic(n); INSERT INTO synthetic VALUES(1);").unwrap();
    let wal = fs::metadata(dir.path().join("live.sqlite3-wal"))
        .unwrap()
        .len();
    let shm = fs::metadata(dir.path().join("live.sqlite3-shm"))
        .unwrap()
        .len();
    assert_ne!(wal, shm);
    let usage = measure_database(&path);
    assert_eq!(usage.wal_bytes, Some(StorageCount(wal)));
    assert_eq!(usage.shm_bytes, Some(StorageCount(shm)));
    assert_logical_unknown(&usage);
    drop(conn);
    fs::create_dir(dir.path().join("live.sqlite3-wal")).unwrap();
    assert_eq!(measure_database(&path).wal_bytes, None);
}

#[test]
fn page_products_are_checked_and_preserve_large_exact_values() {
    assert_eq!(page_bytes(Some(4096), Some(0)), Some(StorageCount(0)));
    assert_eq!(
        page_bytes(Some(4096), Some(1_u64 << 42)),
        Some(StorageCount(1_u64 << 54))
    );
    assert_eq!(page_bytes(Some(65536), Some(u64::MAX)), None);
    assert_eq!(page_bytes(None, Some(1)), None);
    assert_eq!(page_bytes(Some(4096), None), None);
}

#[test]
fn header_measures_full_auto_vacuum_and_65536_byte_pages() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA page_size=65536; PRAGMA auto_vacuum=FULL; CREATE TABLE synthetic(n);",
    )
    .unwrap();
    let pages: u64 = conn
        .query_row("PRAGMA page_count", [], |r| r.get::<_, u32>(0))
        .unwrap()
        .into();
    drop(conn);
    let usage = measure_database(&path);
    assert_eq!(usage.page_size, Some(65536));
    assert_eq!(usage.page_count, Some(StorageCount(pages)));
    assert_eq!(usage.auto_vacuum, Some(StorageAutoVacuum::Full));
    assert_eq!(usage.reusable_bytes, Some(StorageCount(0)));
}

#[test]
fn header_with_sidecars_or_invalid_allocation_is_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE synthetic(n);").unwrap();
    drop(conn);
    let valid = fs::read(&path).unwrap();
    assert!(measure_database(&path).page_count.is_some());
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = dir.path().join(format!("live.sqlite3{suffix}"));
        fs::write(&sidecar, []).unwrap();
        assert_logical_unknown(&measure_database(&path));
        fs::remove_file(&sidecar).unwrap();
    }
    for (offset, bytes) in [
        (0, &b"invalid"[..]),
        (16, &0_u16.to_be_bytes()[..]),
        (18, &[3, 3][..]),
        (21, &[0, 0, 0][..]),
        (28, &0_u32.to_be_bytes()[..]),
        (28, &u32::MAX.to_be_bytes()[..]),
        (36, &u32::MAX.to_be_bytes()[..]),
        (52, &u32::MAX.to_be_bytes()[..]),
        (92, &u32::MAX.to_be_bytes()[..]),
    ] {
        let mut invalid = valid.clone();
        invalid[offset..offset + bytes.len()].copy_from_slice(bytes);
        fs::write(&path, &invalid).unwrap();
        assert_logical_unknown(&measure_database(&path));
        assert_eq!(fs::read(&path).unwrap(), invalid);
    }
    fs::write(&path, &valid[..100]).unwrap();
    assert_logical_unknown(&measure_database(&path));
}

#[cfg(windows)]
#[test]
fn locked_database_reports_unavailable_pragmas_without_inventing_zeroes() {
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE synthetic(n);").unwrap();
    drop(conn);
    let _lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&path)
        .unwrap();
    let usage = measure_database(&path);
    assert_eq!(usage.page_count, None);
    assert_eq!(usage.freelist_count, None);
    assert_eq!(usage.allocated_bytes, None);
    assert_eq!(usage.reusable_bytes, None);
}

#[test]
fn estimate_runs_on_the_existing_read_only_history_reader() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut conn = Connection::open(&path).unwrap();
    maxed_store::migrate(&mut conn).unwrap();
    conn.execute(
        "INSERT INTO event_receipts VALUES(?1,1,?2)",
        ["synthetic", &"a".repeat(64)],
    )
    .unwrap();
    drop(conn);
    let before = fs::read(&path).unwrap();
    let reader = maxed_store::open_reader(&path).unwrap();
    let estimate = reader.estimate_legacy_receipt_bytes(Timestamp(30));
    assert_eq!(estimate.bytes, Some(StorageCount(81)));
    assert_eq!(estimate.measured_at, Some(Timestamp(30)));
    drop(reader);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn receipt_estimate_is_separate_timestamped_logical_bytes_with_unknown_on_error() {
    let conn = Connection::open_in_memory().unwrap();
    assert_eq!(StorageEstimate::default().bytes, None);
    assert_eq!(StorageEstimate::default().measured_at, None);
    let failed = estimate_legacy_receipt_bytes(&conn, Timestamp(10));
    assert_eq!(failed.bytes, None);
    assert_eq!(failed.measured_at, Some(Timestamp(10)));
    conn.execute_batch("CREATE TABLE event_receipts(stream_id TEXT,sequence INTEGER,digest TEXT);")
        .unwrap();
    assert_eq!(
        estimate_legacy_receipt_bytes(&conn, Timestamp(11)).bytes,
        Some(StorageCount(0))
    );
    conn.execute(
        "INSERT INTO event_receipts VALUES(?1,1,?2)",
        ["é", &"a".repeat(64)],
    )
    .unwrap();
    conn.pragma_update(None, "query_only", true).unwrap();
    let estimate = estimate_legacy_receipt_bytes(&conn, Timestamp(12));
    assert_eq!(estimate.bytes, Some(StorageCount(74)));
    assert_eq!(
        estimate.basis,
        StorageEstimateBasis::LegacyReceiptLogicalBytes
    );
    assert_eq!(estimate.measured_at, Some(Timestamp(12)));
}

fn job(phase: Phase, reason: Option<PauseReason>) -> JobStatus {
    JobStatus {
        id: 1,
        phase,
        paused: false,
        reason,
        requested_at: 10,
        updated_at: 20,
        processed_records: 3,
        processed_bytes: 300,
        estimates: Estimates {
            eligible_records: Some(8),
            eligible_bytes: Some(800),
            blocked_records: Some(2),
            blocked_bytes: None,
        },
    }
}

#[test]
fn maintenance_maps_phases_reasons_and_keeps_paused_blocked_and_poisoned_visible() {
    for (phase, expected) in [
        (
            Phase::AwaitingBackup,
            StorageMaintenancePhase::AwaitingBackup,
        ),
        (
            Phase::CopyingReceipts,
            StorageMaintenancePhase::CopyingReceipts,
        ),
        (Phase::Verifying, StorageMaintenancePhase::Verifying),
        (Phase::Deleting, StorageMaintenancePhase::Deleting),
        (Phase::Reclaimable, StorageMaintenancePhase::Reclaimable),
    ] {
        assert_eq!(
            maintenance_usage(&[job(phase, None)]).jobs[0].phase,
            expected
        );
    }
    for (reason, expected) in [
        (PauseReason::Cancelled, StorageMaintenanceReason::Cancelled),
        (
            PauseReason::AwaitingBackup,
            StorageMaintenanceReason::AwaitingBackup,
        ),
        (PauseReason::Capture, StorageMaintenanceReason::Capture),
        (PauseReason::Budget, StorageMaintenanceReason::Budget),
        (PauseReason::Wal, StorageMaintenanceReason::Wal),
        (PauseReason::Busy, StorageMaintenanceReason::Busy),
        (PauseReason::Full, StorageMaintenanceReason::Full),
        (PauseReason::Io, StorageMaintenanceReason::Io),
        (
            PauseReason::InvalidUnit,
            StorageMaintenanceReason::InvalidUnit,
        ),
        (
            PauseReason::StoppedRequired,
            StorageMaintenanceReason::StoppedRequired,
        ),
        (
            PauseReason::BlockedStream,
            StorageMaintenanceReason::BlockedStream,
        ),
        (
            PauseReason::Superseded,
            StorageMaintenanceReason::Superseded,
        ),
        (
            PauseReason::BackupPoisoned,
            StorageMaintenanceReason::BackupPoisoned,
        ),
    ] {
        let mut status = job(Phase::CopyingReceipts, Some(reason));
        status.paused = true;
        let usage = maintenance_usage(&[status]);
        assert!(usage.jobs[0].paused);
        assert_eq!(usage.jobs[0].reason, Some(expected));
        assert_eq!(usage.jobs[0].counters.blocked_bytes, None);
        assert_eq!(usage.last_success_at, None);
    }
}

#[test]
fn maintenance_totals_exclude_superseded_preserve_unknown_and_check_overflow() {
    let mut old = job(Phase::CopyingReceipts, Some(PauseReason::Superseded));
    old.processed_bytes = u64::MAX;
    old.estimates = Estimates::default();
    old.updated_at = 100;
    let done = job(Phase::Reclaimable, None);
    let usage = maintenance_usage(&[old, done]);
    assert_eq!(usage.jobs.len(), 2);
    assert_eq!(usage.totals.processed_bytes, Some(StorageCount(300)));
    assert_eq!(usage.totals.eligible_records, Some(StorageCount(8)));
    assert_eq!(usage.totals.blocked_bytes, None);
    assert_eq!(usage.last_success_at, Some(Timestamp(20)));
    let mut unknown = job(Phase::CopyingReceipts, None);
    unknown.estimates = Estimates::default();
    unknown.processed_bytes = u64::MAX;
    let usage = maintenance_usage(&[unknown, job(Phase::Verifying, None)]);
    assert_eq!(usage.totals.processed_bytes, None);
    assert_eq!(usage.totals.processed_records, Some(StorageCount(6)));
    assert_eq!(usage.totals.eligible_records, None);
    assert_eq!(usage.totals.eligible_bytes, None);
    assert_eq!(usage.totals.blocked_records, None);
    assert_eq!(
        maintenance_usage(&[]).totals.processed_bytes,
        Some(StorageCount(0))
    );
    assert_eq!(maintenance_usage(&[]).last_success_at, None);
}

#[test]
fn resume_requirement_depends_on_reason_regardless_of_paused() {
    for reason in [
        None,
        Some(PauseReason::Cancelled),
        Some(PauseReason::BlockedStream),
        Some(PauseReason::BackupPoisoned),
        Some(PauseReason::Capture),
        Some(PauseReason::Superseded),
    ] {
        for paused in [false, true] {
            let mut status = job(Phase::CopyingReceipts, reason);
            status.paused = paused;
            let usage = maintenance_usage(&[status]);
            let value = serde_json::to_value(&usage.jobs[0]).unwrap();
            assert_eq!(
                value.get("needs_resume"),
                Some(&serde_json::json!(matches!(
                    reason,
                    Some(
                        PauseReason::Cancelled
                            | PauseReason::BlockedStream
                            | PauseReason::BackupPoisoned
                    )
                )))
            );
            assert_eq!(usage.jobs[0].paused, paused);
        }
    }
}

#[test]
fn totals_include_finished_and_cancelled_jobs_when_supplied() {
    let done = job(Phase::Reclaimable, None);
    let cancelled = job(Phase::CopyingReceipts, Some(PauseReason::Cancelled));
    let usage = maintenance_usage(&[done, cancelled]);
    assert_eq!(usage.totals.processed_records, Some(StorageCount(6)));
    assert_eq!(usage.totals.processed_bytes, Some(StorageCount(600)));
}

#[test]
fn backup_measurements_preserve_unknown_and_temporary_bytes() {
    let usage = backup_usage(BackupUsage {
        retained_bytes: None,
        temporary_bytes: Some(u64::MAX),
    });
    assert_eq!(usage.retained_bytes, None);
    assert_eq!(usage.temporary_bytes, Some(StorageCount(u64::MAX)));
}

#[test]
fn exact_installed_schemas_are_measured_without_upgrading_or_creating_files() {
    for version in [12, 14, 15] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite3");
        let db = Connection::open(&path).unwrap();
        crate::common::installed_schema::install(&db, version);
        crate::common::installed_schema::assert_installed(&db, version);
        drop(db);
        let before = fs::read(&path).unwrap();
        let files = directory_files(dir.path());
        let usage = measure_database(&path);
        assert_eq!(usage.state, StorageDatabaseState::Present);
        assert_eq!(usage.main_bytes, Some(StorageCount(before.len() as u64)));
        assert!(usage.page_count.unwrap().0 > 0);
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(directory_files(dir.path()), files);
        let db = Connection::open(&path).unwrap();
        crate::common::installed_schema::assert_installed(&db, version);
    }
}
