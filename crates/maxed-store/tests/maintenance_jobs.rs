use maxed_store::migrate;
use maxed_store::{maintenance::*, StoreError};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

#[test]
fn phase1_process_exit_at_receipt_checkpoints_preserves_proofs_and_newer_capture() {
    use maxed_store::backup::{BackupEnvironment, BackupStore};
    use maxed_types::*;
    use std::time::Instant;

    struct Space;
    impl BackupEnvironment for Space {
        fn available_space(&self, _: &Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            200
        }
    }
    const CHILD: &str = "MAXED_SYNTHETIC_RECEIPT_CHECKPOINT";
    const DATABASE: &str = "MAXED_SYNTHETIC_RECEIPT_DATABASE";
    let events = || {
        ["synthetic-a", "synthetic-b"]
            .into_iter()
            .flat_map(|stream| {
                (1..=7).map(move |n| {
                    let mut event = crate::common::envelope(
                        n,
                        EngineEvent::CaptureStateChanged(CaptureStatus::default()),
                    );
                    event.stream_id = stream.into();
                    event
                })
            })
            .collect::<Vec<_>>()
    };
    let run_until = |owner: &mut StoppedMaintenance, checkpoint: &str| {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let step = owner.step().unwrap();
            if let Step::Committed(status) = &step {
                let reached = match checkpoint {
                    // Copy units stay within one stream, so seven receipts in
                    // each of two streams leave a partial-copy checkpoint.
                    "copying" => status.processed_records > 0 && status.processed_records < 14,
                    "verifying" => status.phase == Phase::Verifying,
                    "deleting" => status.phase == Phase::Deleting,
                    "reclaimable" => status.phase == Phase::Reclaimable,
                    _ => panic!("unknown synthetic checkpoint"),
                };
                if reached {
                    break;
                }
            }
            assert!(
                Instant::now() < deadline,
                "receipt checkpoint {checkpoint} was not reached: {step:?}"
            );
            std::thread::yield_now();
        }
    };
    if let Ok(checkpoint) = std::env::var(CHILD) {
        let path = std::path::PathBuf::from(std::env::var_os(DATABASE).unwrap());
        let mut owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
        owner.request_receipt_conversion(100).unwrap();
        if checkpoint != "awaiting_backup" {
            owner
                .backup_receipt_conversion(&Space, &AtomicBool::new(false), &mut || {})
                .unwrap();
        }
        if !matches!(checkpoint.as_str(), "awaiting_backup" | "backed_up") {
            run_until(&mut owner, &checkpoint);
        }
        // Abrupt termination intentionally skips SQLite/owner destructors.
        std::process::exit(79);
    }
    for version in [12, 14, 15] {
        for checkpoint in [
            "awaiting_backup",
            "backed_up",
            "copying",
            "verifying",
            "deleting",
            "reclaimable",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("live.sqlite3");
            let mut db = Connection::open(&path).unwrap();
            crate::common::installed_schema::install(&db, version);
            let mut originals = events();
            crate::common::apply_legacy_batch(&mut db, &originals);
            crate::common::installed_schema::assert_installed(&db, version);
            drop(db);
            let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "maintenance_jobs::phase1_process_exit_at_receipt_checkpoints_preserves_proofs_and_newer_capture"])
            .env(CHILD, checkpoint).env(DATABASE, &path)
            .output().unwrap();
            assert_eq!(
                child.status.code(),
                Some(79),
                "child must reach its durable checkpoint"
            );
            let mut db = Connection::open(&path).unwrap();
            let (phase, moved): (String, i64) = db
                .query_row(
                    "SELECT phase,processed_records FROM maintenance_jobs",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            match checkpoint {
                "awaiting_backup" | "backed_up" => {
                    assert_eq!(phase, "awaiting_backup");
                    assert_eq!(moved, 0);
                }
                "copying" => assert!(
                    (1..14).contains(&moved),
                    "the process must exit after a partial durable receipt copy"
                ),
                expected => {
                    assert_eq!(phase, expected);
                    assert_eq!(moved, 14);
                }
            }
            assert_eq!(
                BackupStore::read_inventory(&path)
                    .unwrap()
                    .0
                    .iter()
                    .filter(|b| b.ready)
                    .count(),
                usize::from(checkpoint != "awaiting_backup")
            );
            let changes = db.total_changes();
            maxed_store::apply_batch(&mut db, &originals).unwrap();
            assert_eq!(
                db.total_changes(),
                changes,
                "all old retries must survive process exit"
            );
            let mut newer = originals[0].clone();
            newer.sequence = Sequence(Timestamp(8));
            maxed_store::apply_batch(&mut db, &[newer.clone()]).unwrap();
            originals.push(newer);
            let mut owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
            let request = owner.request_receipt_conversion(300).unwrap();
            let id = request.status.id;
            if request.status.phase != Phase::Reclaimable {
                owner
                    .backup_receipt_conversion(&Space, &AtomicBool::new(false), &mut || {})
                    .unwrap();
                // Inject at the five non-complete checkpoints; reclaimable has
                // no next unit. Newer capture must survive the rejected transaction.
                let before = owner.control(Control::Status(id)).unwrap().status;
                let counts = || {
                    ["event_receipts_v2", "event_receipts"].map(|table| {
                        db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                            r.get::<_, i64>(0)
                        })
                        .unwrap()
                    })
                };
                let before_counts = counts();
                db.execute_batch("CREATE TRIGGER reject_phase1_unit BEFORE UPDATE ON maintenance_jobs BEGIN SELECT RAISE(ABORT,'synthetic transaction failure'); END;").unwrap();
                assert!(owner.step().is_err());
                assert_eq!(owner.control(Control::Status(id)).unwrap().status, before);
                assert_eq!(counts(), before_counts);
                db.execute_batch("DROP TRIGGER reject_phase1_unit").unwrap();
                drop(owner);
                owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
                owner
                    .backup_receipt_conversion(&Space, &AtomicBool::new(false), &mut || {})
                    .unwrap();
                run_until(&mut owner, "reclaimable");
            }
            let complete = owner.control(Control::Status(id)).unwrap().status;
            assert_eq!(complete.processed_records, 14);
            drop(owner);
            let changes = db.total_changes();
            maxed_store::apply_batch(&mut db, &originals).unwrap();
            assert_eq!(db.total_changes(), changes);
            let mut changed = originals[0].clone();
            changed.at.0 += 1;
            assert!(matches!(
                maxed_store::apply_batch(&mut db, &[changed]),
                Err(StoreError::InvalidEvent)
            ));
            assert_eq!(
                db.query_row("SELECT count(*) FROM event_receipts_v2", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                15
            );
            assert_eq!(db.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('event_receipts','stream_heads')", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
            assert_eq!(BackupStore::read_inventory(&path).unwrap().0.len(), 1);
            crate::common::installed_schema::assert_integrity(&db);
        }
    }
}

#[derive(Clone, Default)]
struct Clock(Arc<AtomicU64>);
impl Environment for Clock {
    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
    fn timestamp(&self) -> u64 {
        100 + self.0.load(Ordering::SeqCst)
    }
}

struct Synthetic {
    total: u64,
    bytes: u64,
    record: Record,
    clock: Clock,
    millis_per_record: u64,
    fail: Arc<AtomicU64>,
}
impl JobExecutor for Synthetic {
    fn execute(
        &mut self,
        tx: &rusqlite::Transaction<'_>,
        job: &Job,
        budget: &mut UnitBudget<'_>,
    ) -> Result<UnitOutcome, StoreError> {
        assert_eq!(
            tx.query_row("PRAGMA synchronous", [], |r| r.get::<_, u32>(0))?,
            2
        );
        assert_eq!(
            tx.query_row("PRAGMA busy_timeout", [], |r| r.get::<_, u32>(0))?,
            1
        );
        let mut cursor: u64 = job.cursor.as_deref().unwrap_or("0").parse().unwrap();
        if job.status.phase != Phase::CopyingReceipts {
            return Ok(UnitOutcome {
                phase_complete: true,
                ..Default::default()
            });
        }
        while cursor < self.total && budget.reserve(self.record, self.bytes) {
            cursor += 1;
            tx.execute(
                "INSERT INTO synthetic_work VALUES(?1,?2)",
                rusqlite::params![job.status.id, cursor as i64],
            )?;
            self.clock
                .0
                .fetch_add(self.millis_per_record, Ordering::SeqCst);
            let code = self.fail.swap(0, Ordering::SeqCst);
            if code != 0 {
                return Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(code as i32),
                    None,
                )
                .into());
            }
        }
        Ok(UnitOutcome {
            cursor: Some(cursor.to_string()),
            phase_complete: cursor == self.total,
            ..Default::default()
        })
    }
}
fn request() -> JobRequest {
    JobRequest {
        kind: "synthetic".into(),
        key: "scope".into(),
        format_version: 1,
        input_generation: "generation".into(),
        input_revision: 7,
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    path: std::path::PathBuf,
    clock: Clock,
    authorized: Arc<AtomicBool>,
    fail: Arc<AtomicU64>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobs.sqlite3");
        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn).unwrap();
        conn.execute_batch(
            "CREATE TABLE synthetic_work(job INTEGER, n INTEGER, PRIMARY KEY(job,n))",
        )
        .unwrap();
        Self {
            _dir: dir,
            path,
            clock: Clock::default(),
            authorized: Arc::new(AtomicBool::new(true)),
            fail: Arc::new(AtomicU64::new(0)),
        }
    }
    fn service(
        &self,
        total: u64,
        bytes: u64,
        record: Record,
        millis: u64,
        limits: Limits,
    ) -> MaintenanceService {
        let authorized = self.authorized.clone();
        MaintenanceService::new(
            Box::new(Synthetic {
                total,
                bytes,
                record,
                clock: self.clock.clone(),
                millis_per_record: millis,
                fail: self.fail.clone(),
            }),
            Box::new(move |_, _| Ok(authorized.load(Ordering::SeqCst))),
            Box::new(self.clock.clone()),
            limits,
        )
        .unwrap()
    }
    fn open(&self, total: u64) -> StoppedMaintenance {
        StoppedMaintenance::open(
            &self.path,
            self.service(total, 1, Record::Receipt, 0, Limits::default()),
        )
        .unwrap()
    }
    fn count(&self) -> u64 {
        Connection::open(&self.path)
            .unwrap()
            .query_row("SELECT count(*) FROM synthetic_work", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap() as u64
    }
    fn cursor(&self) -> Option<String> {
        Connection::open(&self.path)
            .unwrap()
            .query_row("SELECT keyset_cursor FROM maintenance_jobs", [], |r| {
                r.get(0)
            })
            .unwrap()
    }
}

#[test]
fn stopped_owner_without_conversion_has_empty_inventory_and_unknown_usage() {
    let fixture = Fixture::new();
    let owner = fixture.open(1);
    assert!(owner.list_backups().unwrap().is_empty());
    assert_eq!(owner.backup_usage().unwrap().retained_bytes, None);
    assert_eq!(owner.backup_usage().unwrap().temporary_bytes, None);
    assert!(!fixture.path.parent().unwrap().join("backups").exists());
}
fn committed(step: Step) -> JobStatus {
    match step {
        Step::Committed(s) => s,
        other => panic!("expected commit, got {other:?}"),
    }
}

#[test]
fn duplicate_requests_join_and_completed_jobs_are_not_restarted() {
    let f = Fixture::new();
    let mut owner = f.open(1);
    assert_eq!(owner.step().unwrap(), Step::Idle);
    let created = owner.control(Control::Request(request())).unwrap();
    assert_eq!(created.request, Some(RequestDisposition::Created));
    let first = created.status;
    f.clock.0.store(50, Ordering::SeqCst);
    let duplicate = owner.control(Control::Request(request())).unwrap();
    assert_eq!(duplicate.request, Some(RequestDisposition::Joined));
    assert_eq!(first, duplicate.status);
    assert_eq!(first.estimates.eligible_records, None);
    for phase in [
        Phase::CopyingReceipts,
        Phase::Verifying,
        Phase::Deleting,
        Phase::Reclaimable,
    ] {
        assert_eq!(committed(owner.step().unwrap()).phase, phase);
    }
    let done = owner.control(Control::Status(first.id)).unwrap().status;
    let complete = owner.control(Control::Request(request())).unwrap();
    assert_eq!(complete.request, Some(RequestDisposition::AlreadyComplete));
    assert_eq!(complete.status, done);
    assert_eq!(
        owner.control(Control::Resume(first.id)).unwrap().status,
        done
    );
    assert_eq!(owner.step().unwrap(), Step::Idle);
    assert_eq!(f.count(), 1);
    let mut renewal = request();
    renewal.key = "scope:next-generation".into();
    renewal.input_generation = "next-generation".into();
    let renewed = owner.control(Control::Request(renewal)).unwrap();
    assert_eq!(renewed.request, Some(RequestDisposition::Created));
    assert_ne!(renewed.status.id, first.id);
}

#[test]
fn duplicate_requests_reject_changed_generation_revision_or_format() {
    let f = Fixture::new();
    let mut owner = f.open(1);
    let first = owner.control(Control::Request(request())).unwrap().status;
    for field in 0..3 {
        let mut changed = request();
        match field {
            0 => changed.input_generation = "next-generation".into(),
            1 => changed.input_revision += 1,
            _ => changed.format_version += 1,
        }
        assert!(matches!(
            owner.control(Control::Request(changed)),
            Err(StoreError::MaintenanceRequestMismatch)
        ));
        assert_eq!(
            owner.control(Control::Status(first.id)).unwrap().status,
            first
        );
    }
}

#[test]
fn slow_authorization_still_admits_the_first_record() {
    let f = Fixture::new();
    let clock = f.clock.clone();
    let service = MaintenanceService::new(
        Box::new(Synthetic {
            total: 10,
            bytes: 600 * 1024,
            record: Record::Receipt,
            clock: f.clock.clone(),
            millis_per_record: 0,
            fail: f.fail.clone(),
        }),
        Box::new(move |_, _| {
            clock.0.fetch_add(10, Ordering::SeqCst);
            Ok(true)
        }),
        Box::new(f.clock.clone()),
        Limits::default(),
    )
    .unwrap();
    let mut owner = StoppedMaintenance::open(&f.path, service).unwrap();
    owner.control(Control::Request(request())).unwrap();
    committed(owner.step().unwrap());
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1);
}

#[test]
fn authorization_is_required_and_rechecked_before_each_work_unit() {
    let f = Fixture::new();
    f.authorized.store(false, Ordering::SeqCst);
    let mut owner = f.open(1001);
    let job = owner.control(Control::Request(request())).unwrap().status;
    assert_eq!(
        owner.step().unwrap(),
        Step::Deferred(PauseReason::AwaitingBackup)
    );
    assert_eq!(
        owner.control(Control::Status(job.id)).unwrap().status.phase,
        Phase::AwaitingBackup
    );
    assert_eq!(f.count(), 0);
    f.authorized.store(true, Ordering::SeqCst);
    assert_eq!(owner.step().unwrap(), Step::Idle);
    owner
        .control(Control::AuthorizationChanged(job.id))
        .unwrap();
    committed(owner.step().unwrap());
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1000);
    f.authorized.store(false, Ordering::SeqCst);
    assert_eq!(
        owner.step().unwrap(),
        Step::Deferred(PauseReason::AwaitingBackup)
    );
    assert_eq!(f.count(), 1000);
    assert_eq!(
        owner.control(Control::Status(job.id)).unwrap().status.phase,
        Phase::CopyingReceipts
    );
}

#[test]
fn authorization_wait_does_not_starve_other_jobs_and_change_wakes_the_waiter() {
    let f = Fixture::new();
    f.authorized.store(false, Ordering::SeqCst);
    let authorized = f.authorized.clone();
    let service = MaintenanceService::new(
        Box::new(Synthetic {
            total: 1,
            bytes: 1,
            record: Record::Receipt,
            clock: f.clock.clone(),
            millis_per_record: 0,
            fail: f.fail.clone(),
        }),
        Box::new(move |_, job| {
            Ok(job.request.key != "waiting" || authorized.load(Ordering::SeqCst))
        }),
        Box::new(f.clock.clone()),
        Limits::default(),
    )
    .unwrap();
    let mut owner = StoppedMaintenance::open(&f.path, service).unwrap();
    let mut waiting = request();
    waiting.key = "waiting".into();
    let waiter = owner.control(Control::Request(waiting)).unwrap().status.id;
    let ready = owner
        .control(Control::Request(request()))
        .unwrap()
        .status
        .id;
    assert_eq!(
        owner.step().unwrap(),
        Step::Deferred(PauseReason::AwaitingBackup)
    );
    for _ in 0..4 {
        assert_eq!(committed(owner.step().unwrap()).id, ready);
    }
    assert_eq!(owner.step().unwrap(), Step::Idle);
    assert_eq!(
        owner
            .control(Control::Status(waiter))
            .unwrap()
            .status
            .reason,
        Some(PauseReason::AwaitingBackup)
    );
    f.authorized.store(true, Ordering::SeqCst);
    owner
        .control(Control::AuthorizationChanged(waiter))
        .unwrap();
    assert_eq!(committed(owner.step().unwrap()).id, waiter);
}

#[test]
fn cancellation_survives_restart_and_only_explicit_resume_continues_cursor() {
    let f = Fixture::new();
    let mut owner = f.open(2001);
    let id = owner
        .control(Control::Request(request()))
        .unwrap()
        .status
        .id;
    committed(owner.step().unwrap());
    committed(owner.step().unwrap());
    assert_eq!(f.cursor().as_deref(), Some("1000"));
    let paused = owner.control(Control::Pause(id)).unwrap().status;
    assert!(paused.paused);
    assert_eq!(paused.phase, Phase::CopyingReceipts);
    owner.control(Control::AuthorizationChanged(id)).unwrap();
    assert_eq!(owner.step().unwrap(), Step::Idle);
    drop(owner);
    let mut owner = f.open(2001);
    assert_eq!(
        owner.control(Control::Request(request())).unwrap().status,
        paused
    );
    assert_eq!(owner.step().unwrap(), Step::Idle);
    let resumed = owner.control(Control::Resume(id)).unwrap().status;
    assert_eq!(owner.control(Control::Resume(id)).unwrap().status, resumed);
    assert_eq!(committed(owner.step().unwrap()).processed_records, 2000);
    assert_eq!(f.count(), 2000);
}

#[test]
fn failed_unit_rolls_back_work_and_cursor_and_restart_uses_last_commit() {
    for code in [
        rusqlite::ffi::SQLITE_BUSY,
        rusqlite::ffi::SQLITE_FULL,
        rusqlite::ffi::SQLITE_IOERR,
    ] {
        let f = Fixture::new();
        let mut owner = f.open(2001);
        let id = owner
            .control(Control::Request(request()))
            .unwrap()
            .status
            .id;
        committed(owner.step().unwrap());
        committed(owner.step().unwrap());
        f.fail.store(code as u64, Ordering::SeqCst);
        if code == rusqlite::ffi::SQLITE_BUSY {
            assert_eq!(owner.step().unwrap(), Step::Deferred(PauseReason::Busy));
        } else {
            assert!(owner.step().is_err());
        }
        assert_eq!(f.count(), 1000);
        assert_eq!(f.cursor().as_deref(), Some("1000"));
        let failed = owner.control(Control::Status(id)).unwrap().status;
        assert!(
            !failed.paused,
            "only user cancellation needs explicit resume"
        );
        drop(owner);
        let mut owner = f.open(2001);
        if code != rusqlite::ffi::SQLITE_BUSY {
            assert_eq!(owner.control(Control::Status(id)).unwrap().status, failed);
        }
        assert_eq!(committed(owner.step().unwrap()).processed_records, 2000);
        drop(owner);
        let mut owner = f.open(2001);
        assert_eq!(committed(owner.step().unwrap()).processed_records, 2001);
        assert_eq!(f.count(), 2001);
    }
}

#[test]
fn record_byte_and_clock_budgets_are_independent_and_adapt_after_overrun() {
    for (record, bytes, millis, expected) in [
        (Record::Receipt, 1, 0, 1000),
        (Record::Fight, 1, 0, 32),
        (Record::Receipt, 524288, 0, 2),
        (Record::Receipt, 1, 6, 2),
    ] {
        let f = Fixture::new();
        let mut owner = StoppedMaintenance::open(
            &f.path,
            f.service(4000, bytes, record, millis, Limits::default()),
        )
        .unwrap();
        owner.control(Control::Request(request())).unwrap();
        committed(owner.step().unwrap());
        assert_eq!(committed(owner.step().unwrap()).processed_records, expected);
        assert_eq!(f.count(), expected);
    }
    let f = Fixture::new();
    let limits = Limits {
        receipts: 4,
        ..Default::default()
    };
    let mut owner =
        StoppedMaintenance::open(&f.path, f.service(50, 1, Record::Receipt, 11, limits)).unwrap();
    owner.control(Control::Request(request())).unwrap();
    committed(owner.step().unwrap());
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1);
    // Clock overrun halves the next count/byte ceilings; zero SQL-time advance
    // in another executor isn't needed: two overruns reduce four to one.
    assert_eq!(committed(owner.step().unwrap()).processed_records, 2);
}

#[test]
fn oversized_record_runs_alone_and_notices_coalesce_at_one_per_second() {
    let f = Fixture::new();
    let mut owner = StoppedMaintenance::open(
        &f.path,
        f.service(10, 2 * 1024 * 1024, Record::Receipt, 0, Limits::default()),
    )
    .unwrap();
    let id = owner
        .control(Control::Request(request()))
        .unwrap()
        .status
        .id;
    assert!(owner.take_notice().is_some());
    committed(owner.step().unwrap());
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1);
    assert!(owner.take_notice().is_none());
    owner.control(Control::Pause(id)).unwrap();
    f.clock.0.store(999, Ordering::SeqCst);
    assert!(owner.take_notice().is_none());
    f.clock.0.store(1000, Ordering::SeqCst);
    assert_eq!(
        owner.take_notice().unwrap().reason,
        Some(PauseReason::Cancelled)
    );
    assert!(owner.take_notice().is_none());
}

#[test]
fn long_reader_defers_wal_growth_until_passive_checkpoint_can_finish() {
    let f = Fixture::new();
    let limits = Limits {
        wal_bytes: 1,
        ..Default::default()
    };
    let mut owner =
        StoppedMaintenance::open(&f.path, f.service(10, 1, Record::Receipt, 0, limits)).unwrap();
    let id = owner
        .control(Control::Request(request()))
        .unwrap()
        .status
        .id;
    let reader = Connection::open(&f.path).unwrap();
    reader
        .execute_batch("BEGIN; SELECT * FROM maintenance_jobs;")
        .unwrap();
    committed(owner.step().unwrap());
    assert_eq!(owner.step().unwrap(), Step::Deferred(PauseReason::Wal));
    assert_eq!(
        owner.control(Control::Status(id)).unwrap().status.reason,
        Some(PauseReason::Wal)
    );
    assert_eq!(f.count(), 0);
    assert_eq!(
        owner.control(Control::Status(id)).unwrap().status.phase,
        Phase::CopyingReceipts
    );
    reader.execute_batch("ROLLBACK").unwrap();
    assert_eq!(committed(owner.step().unwrap()).processed_records, 10);
}

#[test]
fn wal_size_seam_defers_at_exact_default_budget_and_unknown_size_fails_closed() {
    struct Wal {
        bytes: Arc<AtomicU64>,
        checks: Arc<AtomicU64>,
    }
    impl Environment for Wal {
        fn monotonic(&self) -> Duration {
            Duration::ZERO
        }
        fn timestamp(&self) -> u64 {
            100
        }
        fn wal_size(&self, _: &Connection) -> Result<u64, StoreError> {
            match self.bytes.load(Ordering::SeqCst) {
                u64::MAX => Err(StoreError::Io),
                bytes => Ok(bytes),
            }
        }
        fn checkpoint(&self, _: &Connection) -> Result<bool, StoreError> {
            self.checks.fetch_add(1, Ordering::SeqCst);
            Ok(false)
        }
    }
    let f = Fixture::new();
    let bytes = Arc::new(AtomicU64::new(32 * 1024 * 1024));
    let checks = Arc::new(AtomicU64::new(0));
    let service = MaintenanceService::new(
        Box::new(Synthetic {
            total: 2001,
            bytes: 1,
            record: Record::Receipt,
            clock: f.clock.clone(),
            millis_per_record: 0,
            fail: f.fail.clone(),
        }),
        Box::new(|_, _| Ok(true)),
        Box::new(Wal {
            bytes: bytes.clone(),
            checks: checks.clone(),
        }),
        Limits::default(),
    )
    .unwrap();
    let mut owner = StoppedMaintenance::open(&f.path, service).unwrap();
    let id = owner
        .control(Control::Request(request()))
        .unwrap()
        .status
        .id;
    assert_eq!(owner.step().unwrap(), Step::Deferred(PauseReason::Wal));
    assert_eq!(checks.load(Ordering::SeqCst), 1);
    bytes.store(32 * 1024 * 1024 - 1, Ordering::SeqCst);
    committed(owner.step().unwrap());
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1000);
    assert_eq!(checks.load(Ordering::SeqCst), 1);
    bytes.store(u64::MAX, Ordering::SeqCst);
    assert!(matches!(owner.step(), Err(StoreError::Io)));
    let failed = owner.control(Control::Status(id)).unwrap().status;
    assert!(!failed.paused);
    assert_eq!(failed.reason, Some(PauseReason::Io));
    assert_eq!(f.count(), 1000);
}

#[test]
fn sqlite_commit_failure_rolls_back_the_already_written_cursor_and_work() {
    struct FailCommit;
    impl JobExecutor for FailCommit {
        fn execute(
            &mut self,
            tx: &rusqlite::Transaction<'_>,
            job: &Job,
            budget: &mut UnitBudget<'_>,
        ) -> Result<UnitOutcome, StoreError> {
            assert!(budget.reserve(Record::Receipt, 1));
            tx.execute("INSERT INTO synthetic_work VALUES(?1,1)", [job.status.id])?;
            tx.execute_batch("PRAGMA defer_foreign_keys=ON; INSERT INTO event_receipts_v2 VALUES(999,1,zeroblob(32));")?;
            Ok(UnitOutcome {
                cursor: Some("1".into()),
                ..Default::default()
            })
        }
    }
    let f = Fixture::new();
    let service = MaintenanceService::new(
        Box::new(FailCommit),
        Box::new(|_, _| Ok(true)),
        Box::<Clock>::default(),
        Limits::default(),
    )
    .unwrap();
    let mut owner = StoppedMaintenance::open(&f.path, service).unwrap();
    let id = owner
        .control(Control::Request(request()))
        .unwrap()
        .status
        .id;
    committed(owner.step().unwrap());
    assert!(owner.step().is_err());
    assert_eq!(f.count(), 0);
    assert_eq!(f.cursor(), None);
    assert_eq!(
        owner
            .control(Control::Status(id))
            .unwrap()
            .status
            .processed_records,
        0
    );
    drop(owner);
    let mut owner = f.open(2001);
    assert_eq!(
        owner.control(Control::Status(id)).unwrap().status.reason,
        Some(PauseReason::InvalidUnit)
    );
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1000);
}

#[test]
fn live_and_stopped_owners_exclude_each_other_before_opening_sqlite() {
    let f = Fixture::new();
    let writer = maxed_store::start_writer(f.path.clone(), Default::default()).unwrap();
    assert!(matches!(
        StoppedMaintenance::open(&f.path, MaintenanceService::default()),
        Err(StoreError::Busy)
    ));
    writer.shutdown().unwrap();
    let owner = f.open(1);
    assert!(matches!(
        maxed_store::start_writer(f.path.clone(), Default::default()),
        Err(StoreError::Busy)
    ));
    assert!(matches!(
        StoppedMaintenance::open(&f.path, MaintenanceService::default()),
        Err(StoreError::Busy)
    ));
    drop(owner);
    maxed_store::start_writer(f.path.clone(), Default::default())
        .unwrap()
        .shutdown()
        .unwrap();
}

#[test]
fn cursor_update_failure_rolls_back_executor_writes() {
    let f = Fixture::new();
    let mut owner = f.open(2001);
    let id = owner
        .control(Control::Request(request()))
        .unwrap()
        .status
        .id;
    committed(owner.step().unwrap());
    Connection::open(&f.path).unwrap().execute_batch("CREATE TRIGGER reject_cursor BEFORE UPDATE OF keyset_cursor ON maintenance_jobs WHEN NEW.keyset_cursor IS NOT NULL BEGIN SELECT RAISE(ABORT,'synthetic cursor failure'); END;").unwrap();
    assert!(owner.step().is_err());
    assert_eq!(f.count(), 0);
    assert_eq!(f.cursor(), None);
    Connection::open(&f.path)
        .unwrap()
        .execute_batch("DROP TRIGGER reject_cursor")
        .unwrap();
    assert_eq!(
        owner.control(Control::Status(id)).unwrap().status.reason,
        Some(PauseReason::InvalidUnit)
    );
    assert_eq!(owner.step().unwrap(), Step::Idle);
    f.clock.0.fetch_add(5_000, Ordering::SeqCst);
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1000);
}

#[test]
fn actual_busy_lock_defers_and_retries_without_resume_before_or_after_restart() {
    let f = Fixture::new();
    let mut owner = f.open(2001);
    let id = owner
        .control(Control::Request(request()))
        .unwrap()
        .status
        .id;
    committed(owner.step().unwrap());
    let blocker = Connection::open(&f.path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(owner.step().unwrap(), Step::Deferred(PauseReason::Busy));
    let failed = owner.control(Control::Status(id)).unwrap().status;
    assert!(!failed.paused);
    assert_eq!(failed.reason, Some(PauseReason::Busy));
    assert_eq!(owner.step().unwrap(), Step::Deferred(PauseReason::Busy));
    blocker.execute_batch("ROLLBACK").unwrap();
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1000);
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(owner.step().unwrap(), Step::Deferred(PauseReason::Busy));
    drop(owner);
    blocker.execute_batch("ROLLBACK").unwrap();
    let mut owner = f.open(2001);
    assert_eq!(committed(owner.step().unwrap()).processed_records, 2000);
}

#[test]
fn unsaved_failure_annotation_has_no_memory_only_pause_or_global_block() {
    for code in [rusqlite::ffi::SQLITE_FULL, rusqlite::ffi::SQLITE_IOERR] {
        let f = Fixture::new();
        let mut owner = f.open(2001);
        let id = owner
            .control(Control::Request(request()))
            .unwrap()
            .status
            .id;
        committed(owner.step().unwrap());
        let before = owner.control(Control::Status(id)).unwrap().status;
        Connection::open(&f.path).unwrap().execute_batch("CREATE TRIGGER reject_error BEFORE UPDATE OF safe_error ON maintenance_jobs WHEN NEW.safe_error IS NOT NULL BEGIN SELECT RAISE(ABORT,'synthetic annotation failure'); END;").unwrap();
        f.fail.store(code as u64, Ordering::SeqCst);
        assert!(owner.step().is_err());
        assert_eq!(owner.control(Control::Status(id)).unwrap().status, before);
        drop(owner);
        let mut owner = f.open(2001);
        assert_eq!(owner.control(Control::Status(id)).unwrap().status, before);
        f.fail.store(code as u64, Ordering::SeqCst);
        assert!(owner.step().is_err());
        let mut other = request();
        other.key = "other".into();
        let other_id = owner.control(Control::Request(other)).unwrap().status.id;
        assert_eq!(committed(owner.step().unwrap()).id, other_id);
        assert_eq!(committed(owner.step().unwrap()).processed_records, 1000);
    }
}

#[test]
fn process_exit_before_and_after_commit_resumes_only_durable_cursor() {
    const CHILD: &str = "MAXED_SYNTHETIC_MAINTENANCE_CRASH";
    if let Some(path) = std::env::var_os(CHILD) {
        struct ExitUnit {
            before: bool,
        }
        impl JobExecutor for ExitUnit {
            fn execute(
                &mut self,
                tx: &rusqlite::Transaction<'_>,
                job: &Job,
                budget: &mut UnitBudget<'_>,
            ) -> Result<UnitOutcome, StoreError> {
                let mut cursor: i64 = job.cursor.as_ref().unwrap().parse().unwrap();
                while budget.reserve(Record::Receipt, 1) {
                    cursor += 1;
                    tx.execute(
                        "INSERT INTO synthetic_work VALUES(?1,?2)",
                        rusqlite::params![job.status.id, cursor],
                    )?;
                }
                if self.before {
                    std::process::exit(73);
                }
                Ok(UnitOutcome {
                    cursor: Some(cursor.to_string()),
                    ..Default::default()
                })
            }
        }
        let before = std::env::var("MAXED_SYNTHETIC_CRASH_BEFORE").unwrap() == "1";
        let service = MaintenanceService::new(
            Box::new(ExitUnit { before }),
            Box::new(|_, _| Ok(true)),
            Box::<Clock>::default(),
            Limits::default(),
        )
        .unwrap();
        let mut owner = StoppedMaintenance::open(Path::new(&path), service).unwrap();
        committed(owner.step().unwrap());
        std::process::exit(73);
    }
    let f = Fixture::new();
    let mut owner = f.open(5000);
    let id = owner
        .control(Control::Request(request()))
        .unwrap()
        .status
        .id;
    committed(owner.step().unwrap());
    committed(owner.step().unwrap());
    drop(owner);
    for (before, expected) in [(true, 1000), (false, 2000)] {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact","maintenance_jobs::process_exit_before_and_after_commit_resumes_only_durable_cursor","--nocapture"])
            .env(CHILD,&f.path).env("MAXED_SYNTHETIC_CRASH_BEFORE",if before {"1"} else {"0"})
            .output().unwrap();
        assert_eq!(
            child.status.code(),
            Some(73),
            "child must exit at the injected boundary"
        );
        assert_eq!(f.count(), expected);
        assert_eq!(f.cursor(), Some(expected.to_string()));
    }
    let mut owner = f.open(5000);
    assert!(!owner.control(Control::Status(id)).unwrap().status.paused);
    assert_eq!(committed(owner.step().unwrap()).processed_records, 3000);
    assert_eq!(f.count(), 3000);
}

#[test]
fn overrun_reduces_next_batch_count_without_wall_clock_assertions() {
    struct Overrun {
        clock: Clock,
        calls: usize,
    }
    impl JobExecutor for Overrun {
        fn execute(
            &mut self,
            _: &rusqlite::Transaction<'_>,
            job: &Job,
            budget: &mut UnitBudget<'_>,
        ) -> Result<UnitOutcome, StoreError> {
            let mut n: u64 = job.cursor.as_deref().unwrap_or("0").parse().unwrap();
            while budget.reserve(Record::Receipt, 1) {
                n += 1;
            }
            if self.calls == 0 {
                self.clock.0.fetch_add(11, Ordering::SeqCst);
            }
            self.calls += 1;
            Ok(UnitOutcome {
                cursor: Some(n.to_string()),
                ..Default::default()
            })
        }
    }
    let f = Fixture::new();
    let service = MaintenanceService::new(
        Box::new(Overrun {
            clock: f.clock.clone(),
            calls: 0,
        }),
        Box::new(|_, _| Ok(true)),
        Box::new(f.clock.clone()),
        Limits::default(),
    )
    .unwrap();
    let mut owner = StoppedMaintenance::open(&f.path, service).unwrap();
    owner.control(Control::Request(request())).unwrap();
    committed(owner.step().unwrap());
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1000);
    assert_eq!(committed(owner.step().unwrap()).processed_records, 1500);
    assert_eq!(committed(owner.step().unwrap()).processed_records, 2500);
}

#[test]
fn pause_is_separate_from_phase_and_unknown_estimates_remain_null() {
    let mut db = Connection::open_in_memory().unwrap();
    migrate(&mut db).unwrap();
    let columns: Vec<String> = db
        .prepare("SELECT name FROM pragma_table_info('maintenance_jobs')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        columns.iter().any(|c| c == "paused"),
        "durable pause must not replace the resumable phase"
    );
    db.execute_batch("INSERT INTO maintenance_jobs(kind,job_key,format_version,input_generation,input_revision) VALUES('synthetic','scope',1,'generation',0)").unwrap();
    let state: (bool, i64, Option<i64>, Option<i64>) = db
        .query_row(
            "SELECT paused,processed_records,eligible_records,eligible_bytes FROM maintenance_jobs",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(state, (false, 0, None, None));
    for column in [
        "requested_at",
        "updated_at",
        "processed_records",
        "processed_bytes",
        "eligible_records",
        "eligible_bytes",
        "blocked_records",
        "blocked_bytes",
    ] {
        assert!(db
            .execute(&format!("UPDATE maintenance_jobs SET {column}=-1"), [])
            .is_err());
    }
}
