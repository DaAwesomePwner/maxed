use maxed_store::{backup::*, migrate};
use rusqlite::Connection;
use std::{path::Path, sync::atomic::AtomicBool};

#[test]
fn conversion_request_defers_while_backup_holds_owner() {
    use std::{sync::mpsc, time::Duration};

    let dir = tempfile::tempdir().unwrap();
    let writer =
        maxed_store::start_writer(dir.path().join("live.sqlite3"), Default::default()).unwrap();
    writer.request_receipt_conversion(100).unwrap();
    let task = writer.receipt_backup_task().unwrap();
    let (entered, waiting) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let copy = std::thread::spawn(move || {
        let mut first = true;
        task.run(&Space(Some(u64::MAX)), &AtomicBool::new(false), &mut || {
            if first {
                first = false;
                entered.send(()).unwrap();
                blocked.recv_timeout(Duration::from_secs(15)).unwrap();
            }
        })
    });
    waiting.recv_timeout(Duration::from_secs(15)).unwrap();
    let request = std::thread::scope(|scope| {
        let (send, receive) = mpsc::channel();
        let writer = &writer;
        scope.spawn(move || {
            let _ = send.send(writer.request_receipt_conversion(200));
        });
        let result = receive.recv_timeout(Duration::from_secs(1));
        let _ = release.send(());
        result
    });
    copy.join().unwrap().unwrap();
    writer.shutdown().unwrap();
    assert!(
        matches!(request, Ok(Err(BackupError::Busy))),
        "request must defer without waiting for the copy"
    );
}

#[test]
fn phase1_first_live_commit_precedes_blocked_backup_copy_for_exact_installed_schemas() {
    use maxed_store::{start_writer, WriterConfig};
    use maxed_types::*;
    use std::{cell::Cell, sync::mpsc, time::Duration};

    struct Barrier {
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        first: Cell<bool>,
    }
    impl BackupEnvironment for Barrier {
        fn available_space(&self, _: &Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            200
        }
        fn boundary(&self, boundary: Boundary) -> Result<(), BackupError> {
            if boundary == Boundary::Copy && self.first.replace(false) {
                self.entered.send(()).unwrap();
                self.release.recv_timeout(Duration::from_secs(15)).unwrap();
            }
            Ok(())
        }
    }
    for version in [12, 14, 15] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite3");
        let mut db = Connection::open(&path).unwrap();
        crate::common::installed_schema::install(&db, version);
        crate::common::apply_legacy_story(&mut db, &crate::common::story_events());
        crate::common::installed_schema::assert_installed(&db, version);
        drop(db);
        let writer = start_writer(
            path.clone(),
            WriterConfig {
                stream_id: "synthetic-live".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!dir.path().join("backups").exists());
        let id = writer.request_receipt_conversion(100).unwrap().status.id;
        let task = writer.receipt_backup_task().unwrap();
        let (entered, waiting) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let copy = std::thread::spawn(move || {
            task.run(
                &Barrier {
                    entered,
                    release: blocked,
                    first: Cell::new(true),
                },
                &AtomicBool::new(false),
                &mut || {},
            )
        });
        waiting.recv_timeout(Duration::from_secs(15)).unwrap();
        let mut event = crate::common::envelope(
            1,
            EngineEvent::CaptureStateChanged(CaptureStatus::default()),
        );
        event.stream_id = "synthetic-live".into();
        crate::common::submit(&writer, event);
        let durable = writer.flush();
        let still_blocked = !copy.is_finished();
        // Release even if the assertions below fail; never strand a helper thread.
        let _ = release.send(());
        let copied = copy.join().unwrap().unwrap();
        assert_eq!(durable.unwrap().sequence.0 .0, 1);
        assert!(
            still_blocked,
            "first durable capture must not wait for a backup step"
        );
        assert_eq!(copied.1, id);
        writer.shutdown().unwrap();
    }
}

#[test]
fn process_exit_during_rotation_recovers_only_published_slots() {
    struct Crash(Boundary);
    impl BackupEnvironment for Crash {
        fn available_space(&self, _: &Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            200
        }
        fn boundary(&self, at: Boundary) -> Result<(), BackupError> {
            if at == self.0 {
                std::process::exit(83);
            }
            Ok(())
        }
    }
    let boundary = |name: &str| match name {
        "verify" => Boundary::Verify,
        "publish" => Boundary::Publish,
        "cleanup" => Boundary::Cleanup,
        _ => Boundary::CleanupRemoved,
    };
    if let Ok(name) = std::env::var("MAXED_SYNTHETIC_BACKUP_CRASH") {
        let path =
            std::path::PathBuf::from(std::env::var_os("MAXED_SYNTHETIC_BACKUP_DATABASE").unwrap());
        let db = Connection::open(&path).unwrap();
        let mut backups = BackupStore::open(&path).unwrap();
        let request = backups
            .current_request(DestructiveClass::SealedReceiptPruning)
            .unwrap()
            .unwrap();
        backups
            .create(
                &db,
                request,
                &Crash(boundary(&name)),
                &AtomicBool::new(false),
                &mut || {},
            )
            .unwrap();
        panic!("injected process exit was not reached");
    }
    for name in ["verify", "publish", "cleanup", "removed"] {
        let (_dir, path, mut db, mut backups) = fixture();
        let (request, initial) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
        admit(&mut backups, &mut db, request, "first");
        db.execute("UPDATE maintenance_jobs SET phase='reclaimable'", [])
            .unwrap();
        let (_, rolling) = make(&mut backups, &db, DestructiveClass::DetailReplacement);
        backups
            .request(DestructiveClass::SealedReceiptPruning, 150)
            .unwrap();
        drop(backups);
        drop(db);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "maintenance_backup::process_exit_during_rotation_recovers_only_published_slots",
            ])
            .env("MAXED_SYNTHETIC_BACKUP_CRASH", name)
            .env("MAXED_SYNTHETIC_BACKUP_DATABASE", &path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(83));
        let mut backups = BackupStore::open(&path).unwrap();
        let ready = backups.list().unwrap();
        assert_eq!(ready.len(), 2);
        assert!(ready.contains(&initial));
        assert_eq!(
            ready.contains(&rolling),
            matches!(name, "verify" | "publish")
        );
        backups.recover(&Space(Some(u64::MAX))).unwrap();
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
        assert_eq!(backups.list().unwrap(), ready);
    }
}

struct Space(Option<u64>);

#[test]
fn read_only_preflight_matches_copy_budget_and_creates_no_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    assert_eq!(
        HistoryUpgrade::preflight(&path, &Space(None))
            .unwrap()
            .schema,
        None
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let probe = HistoryUpgrade::preflight(&path, &Space(None)).unwrap();
    assert_eq!(probe.schema, Some(maxed_store::SCHEMA_VERSION));
    assert_eq!(probe.space.check(), Err(BackupError::SpaceUnknown));
    assert!(!dir.path().join("backups").exists());
    let required = probe.space.required_bytes.unwrap();
    let mut backups = BackupStore::open(&path).unwrap();
    let request = backups
        .request(DestructiveClass::ReceiptConversion, 1)
        .unwrap();
    assert_eq!(
        backups.create(
            &db,
            request,
            &Space(Some(required - 1)),
            &AtomicBool::new(false),
            &mut || {}
        ),
        Err(BackupError::InsufficientSpace)
    );
    backups
        .create(
            &db,
            request,
            &Space(Some(required)),
            &AtomicBool::new(false),
            &mut || {},
        )
        .unwrap();
    let metadata = backups.list_managed().unwrap();
    assert_eq!(metadata.len(), 1);
    assert_eq!(metadata[0].completed_at, Some(50));
    assert!(metadata[0].ready);
    assert!(!metadata[0].in_use);
    assert!(metadata[0].bytes.unwrap() > 0);
}

#[test]
fn upgrade_boundary_is_verified_and_cancellable_before_migration() {
    use std::sync::atomic::Ordering;
    for version in [2, 6, 10] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite3");
        let db = Connection::open(&path).unwrap();
        crate::common::installed_schema::install(&db, version);
        drop(db);
        let original = std::fs::read(&path).unwrap();
        let mut owner = HistoryUpgrade::open(&path).unwrap();
        let cancel = AtomicBool::new(false);
        let mut called = false;
        assert_eq!(owner.confirm_and_upgrade_with(&Space(Some(u64::MAX)), &cancel, &mut || {
            called = true;
            let manifest = Connection::open(dir.path().join("backups/live/manifest.sqlite3")).unwrap();
            assert_eq!(manifest.query_row("SELECT count(*) FROM backups WHERE state='ready' AND completed_order IS NOT NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
            cancel.store(true, Ordering::Release);
        }), Err(BackupError::Cancelled));
        assert!(called);
        assert_eq!(std::fs::read(&path).unwrap(), original);
        cancel.store(false, Ordering::Release);
        owner
            .confirm_and_upgrade_with(&Space(Some(u64::MAX)), &cancel, &mut || {})
            .unwrap();
        drop(owner);
        assert_eq!(
            maxed_store::open_reader(&path)
                .unwrap()
                .user_version()
                .unwrap(),
            maxed_store::SCHEMA_VERSION
        );
    }
}

#[test]
fn managed_owner_delete_serializes_resume_and_invalidates_before_next_unit() {
    use maxed_store::maintenance::Control;
    struct Race<'a>(&'a maxed_store::StoreWriter, i64, std::cell::Cell<bool>);
    impl BackupEnvironment for Race<'_> {
        fn available_space(&self, _: &Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            200
        }
        fn boundary(&self, at: Boundary) -> Result<(), BackupError> {
            if at == Boundary::DeleteChecked {
                self.2.set(true);
                assert!(matches!(
                    self.0.maintenance(Control::Resume(self.1)),
                    Err(maxed_store::StoreError::Busy)
                ));
            }
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let writer = maxed_store::start_writer(path.clone(), Default::default()).unwrap();
    assert!(writer.list_backups().unwrap().is_empty());
    assert_eq!(writer.backup_usage().unwrap().retained_bytes, Some(0));
    assert!(!dir.path().join("backups").exists());
    let job = writer.request_receipt_conversion(10).unwrap().status.id;
    writer.maintenance(Control::Pause(job)).unwrap();
    let (backup, id) = std::thread::spawn({
        let task = writer.receipt_backup_task().unwrap();
        move || {
            task.run(&Space(Some(u64::MAX)), &AtomicBool::new(false), &mut || {})
                .unwrap()
        }
    })
    .join()
    .unwrap();
    assert_eq!(id, job);
    writer
        .maintenance(Control::AuthorizationChanged(id))
        .unwrap();
    assert_eq!(writer.list_backups().unwrap()[0].id, backup.id);
    assert!(!writer.list_backups().unwrap()[0].in_use);
    assert!(matches!(BackupStore::open(&path), Err(BackupError::InUse)));
    let race = Race(&writer, job, std::cell::Cell::new(false));
    writer.delete_backup(backup.id, &race).unwrap();
    assert!(race.2.get());
    writer.set_maintenance_blocked(true);
    writer.maintenance(Control::Resume(job)).unwrap();
    assert!(writer.list_backups().unwrap().is_empty());
    assert_eq!(
        writer.backup_receipt_conversion(
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {}
        ),
        Err(BackupError::Superseded)
    );
    assert_eq!(
        writer
            .maintenance(Control::Status(job))
            .unwrap()
            .status
            .processed_records,
        0
    );
    writer.set_maintenance_blocked(false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let status = writer.maintenance(Control::Status(job)).unwrap().status;
        if status.reason == Some(maxed_store::maintenance::PauseReason::AwaitingBackup) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "deleted authorization must defer after capture unblocks"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let db = Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT safe_error FROM maintenance_jobs WHERE job_id=?1",
            [job],
            |r| r.get::<_, Option<String>>(0)
        )
        .unwrap(),
        None
    );
    drop(db);
    writer.shutdown().unwrap();
    let mut owner =
        maxed_store::maintenance::StoppedMaintenance::receipt_conversion(&path).unwrap();
    assert!(owner.list_backups().unwrap().is_empty());
    assert_eq!(owner.backup_usage().unwrap().retained_bytes, Some(0));
    assert_eq!(
        owner.delete_backup(BackupId(0), &Space(Some(u64::MAX))),
        Err(BackupError::Invalid)
    );
    let job = owner.request_receipt_conversion(300).unwrap().status.id;
    owner.control(Control::Resume(job)).unwrap();
    let backup = owner
        .backup_receipt_conversion(&Space(Some(u64::MAX)), &AtomicBool::new(false), &mut || {})
        .unwrap();
    owner.step().unwrap();
    assert!(owner.list_backups().unwrap()[0].in_use);
    assert_eq!(
        owner.delete_backup(backup.id, &Space(Some(u64::MAX))),
        Err(BackupError::InUse)
    );

    // Exercise an actual unit at the check/invalidation boundary as well as
    // Resume. A newly authorized job has not yet persisted its backup binding.
    use maxed_store::maintenance::{PauseReason, Step, StoppedMaintenance};
    struct StepRace(
        std::cell::RefCell<StoppedMaintenance>,
        std::cell::Cell<bool>,
    );
    impl BackupEnvironment for StepRace {
        fn available_space(&self, _: &Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            200
        }
        fn boundary(&self, at: Boundary) -> Result<(), BackupError> {
            if at == Boundary::DeleteChecked {
                self.1.set(true);
                assert_eq!(
                    self.0.borrow_mut().step().unwrap(),
                    Step::Deferred(PauseReason::Busy)
                );
            }
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
    owner.request_receipt_conversion(10).unwrap();
    let backup = owner
        .backup_receipt_conversion(&Space(Some(u64::MAX)), &AtomicBool::new(false), &mut || {})
        .unwrap();
    drop(owner);
    let mut backups = BackupStore::open(&path).unwrap();
    let race = StepRace(
        std::cell::RefCell::new(StoppedMaintenance::receipt_conversion(&path).unwrap()),
        std::cell::Cell::new(false),
    );
    backups.delete_managed(backup.id, &race).unwrap();
    assert!(race.1.get());
    assert_eq!(
        race.0.borrow_mut().step().unwrap(),
        Step::Deferred(PauseReason::AwaitingBackup)
    );
}

#[test]
fn deleted_backup_defers_resumed_units_without_recording_a_failure() {
    use maxed_store::maintenance::{Control, PauseReason, Step, StoppedMaintenance};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
    let id = owner.request_receipt_conversion(10).unwrap().status.id;
    let backup = owner
        .backup_receipt_conversion(&Space(Some(u64::MAX)), &AtomicBool::new(false), &mut || {})
        .unwrap();
    owner.control(Control::Pause(id)).unwrap();
    owner
        .delete_backup(backup.id, &Space(Some(u64::MAX)))
        .unwrap();
    owner.control(Control::Resume(id)).unwrap();
    let db = Connection::open(&path).unwrap();
    let failures = || {
        db.query_row(
            "SELECT safe_error FROM maintenance_jobs WHERE job_id=?1",
            [id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap()
    };
    let before = failures();
    assert_eq!(
        owner.step().unwrap(),
        Step::Deferred(PauseReason::AwaitingBackup)
    );
    assert_eq!(failures(), before);
}

#[test]
fn readonly_inventory_shares_an_active_owner_and_rejects_unknown_slots() {
    let (_dir, path, db, mut backups) = fixture();
    let request = backups
        .request(DestructiveClass::ReceiptConversion, 10)
        .unwrap();
    let info = backups
        .create(
            &db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {},
        )
        .unwrap();
    let (inventory, usage) = BackupStore::read_inventory(&path).unwrap();
    assert_eq!(inventory[0].id, info.id);
    assert_eq!(usage.retained_bytes, Some(info.bytes));
    let manifest =
        Connection::open(path.parent().unwrap().join("backups/live/manifest.sqlite3")).unwrap();
    manifest
        .execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE backups SET slot='unknown'")
        .unwrap();
    assert_eq!(backups.list_managed(), Err(BackupError::Invalid));
    assert_eq!(backups.list(), Err(BackupError::Invalid));
    assert_eq!(
        BackupStore::read_inventory(&path),
        Err(BackupError::Invalid)
    );
}

#[test]
fn preflight_reports_a_locked_schema_as_busy() {
    let (_dir, path, db, _backups) = fixture();
    db.execute_batch("PRAGMA journal_mode=DELETE; BEGIN EXCLUSIVE")
        .unwrap();
    assert!(matches!(
        HistoryUpgrade::preflight(&path, &Space(Some(u64::MAX))),
        Err(BackupError::Busy)
    ));
}

#[test]
fn superseded_request_has_a_distinct_safe_reason() {
    let (_dir, _path, db, mut backups) = fixture();
    let old = backups
        .request(DestructiveClass::ReceiptConversion, 1)
        .unwrap();
    backups
        .request(DestructiveClass::ReceiptConversion, 2)
        .unwrap();
    let error = backups
        .create(
            &db,
            old,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {},
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "History backup request was superseded; request a new backup"
    );
}

impl BackupEnvironment for Space {
    fn available_space(&self, _: &Path) -> Option<u64> {
        self.0
    }
    fn timestamp(&self) -> u64 {
        50
    }
}

fn assert_recorded_heads(dir: &Path, info: &BackupInfo, expected: &[(&str, i64)]) {
    use sha2::{Digest, Sha256};
    let manifest = Connection::open(dir.join("backups/live/manifest.sqlite3")).unwrap();
    let mut statement = manifest
        .prepare("SELECT stream_id,sequence FROM heads WHERE backup_id=?1 ORDER BY stream_id")
        .unwrap();
    let heads: Vec<(String, i64)> = statement
        .query_map([info.id.0], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        heads,
        expected
            .iter()
            .map(|(s, n)| (s.to_string(), *n))
            .collect::<Vec<_>>()
    );
    let mut hash = Sha256::new();
    for (stream, sequence) in expected {
        hash.update((stream.len() as u64).to_le_bytes());
        hash.update(stream.as_bytes());
        hash.update(sequence.to_le_bytes());
    }
    let checksum: String = manifest
        .query_row(
            "SELECT heads_checksum FROM backups WHERE id=?1",
            [info.id.0],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(checksum, format!("{:x}", hash.finalize()));
}

#[test]
fn logical_heads_cover_registry_mixed_and_retired_legacy_tables() {
    for mode in ["registry", "mixed", "retired"] {
        let (dir, path, mut db, mut backups) = fixture();
        maxed_store::apply_batch(&mut db, &crate::common::story_events()).unwrap();
        db.execute("INSERT INTO event_streams(stream_id) VALUES('empty')", [])
            .unwrap();
        let mut expected = vec![("test-stream", 7)];
        match mode {
            "mixed" => {
                db.execute_batch("INSERT INTO stream_heads VALUES('test-stream',3),('legacy-only',5),('legacy-ahead',9);
                    INSERT INTO event_streams(stream_id,durable_sequence) VALUES('legacy-ahead',2);").unwrap();
                expected = vec![("legacy-ahead", 9), ("legacy-only", 5), ("test-stream", 7)];
            }
            "retired" => db
                .execute_batch("DROP TABLE event_receipts; DROP TABLE stream_heads;")
                .unwrap(),
            _ => {}
        }
        let (request, info) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
        assert_recorded_heads(dir.path(), &info, &expected);
        drop(backups);
        let mut backups = BackupStore::open(&path).unwrap();
        assert_eq!(backups.prepare_authorization(request).unwrap(), info.id);
    }
}

#[test]
fn logical_heads_preserve_v1_through_v10_legacy_backups() {
    for version in 1..=10 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite3");
        let db = Connection::open(&path).unwrap();
        crate::common::installed_schema::install(&db, version);
        db.execute("INSERT INTO stream_heads VALUES('legacy-only',5)", [])
            .unwrap();
        let mut backups = BackupStore::open(&path).unwrap();
        let (request, info) = make(&mut backups, &db, DestructiveClass::LegacyUpgrade);
        assert_recorded_heads(dir.path(), &info, &[("legacy-only", 5)]);
        drop(backups);
        let mut backups = BackupStore::open(&path).unwrap();
        assert_eq!(backups.prepare_authorization(request).unwrap(), info.id);
    }
}

#[test]
fn symlinked_live_database_is_rejected_before_manifest_creation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    drop(db);
    let link = dir.path().join("linked.sqlite3");
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(&path, &link) {
        eprintln!(
            "File symlink unavailable ({error}); exercising a junction to the database instead."
        );
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "synthetic junction creation failed"
        );
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(matches!(
        BackupStore::open(&link),
        Err(BackupError::Invalid)
    ));
    assert!(!dir.path().join("backups").exists());
}

#[test]
fn pre_copy_space_is_required_and_failure_never_publishes_ready() {
    for (space, expected) in [
        (None, BackupError::SpaceUnknown),
        (Some(0), BackupError::InsufficientSpace),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite3");
        let mut db = Connection::open(&path).unwrap();
        migrate(&mut db).unwrap();
        let changes = db.total_changes();
        let mut backups = BackupStore::open(&path).unwrap();
        let request = backups
            .request(DestructiveClass::ReceiptConversion, 100)
            .unwrap();
        assert_eq!(
            backups.create(
                &db,
                request,
                &Space(space),
                &AtomicBool::new(false),
                &mut || {}
            ),
            Err(expected)
        );
        assert!(backups.list().unwrap().is_empty());
        assert_eq!(db.total_changes(), changes);
    }
}

#[test]
fn committed_wal_is_backed_up_and_clock_regression_does_not_block_causal_completion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO stream_heads VALUES('synthetic-stream',7);").unwrap();
    assert!(
        std::fs::metadata(path.with_file_name("live.sqlite3-wal"))
            .unwrap()
            .len()
            > 0
    );
    let mut backups = BackupStore::open(&path).unwrap();
    let request = backups
        .request(DestructiveClass::ReceiptConversion, 100)
        .unwrap();
    let ready = backups
        .create(
            &db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {},
        )
        .unwrap();
    let copy = Connection::open_with_flags(
        dir.path()
            .join("backups/live")
            .join(format!("backup-{:020}.sqlite3", ready.id.0)),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        copy.query_row("SELECT sequence FROM stream_heads", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        7
    );
    drop(copy);
    assert_eq!(ready.slot, Slot::Initial);
    assert_eq!(backups.list().unwrap().len(), 1);
    drop(backups);
    assert_eq!(BackupStore::open(&path).unwrap().list().unwrap(), [ready]);
}

fn fixture() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    Connection,
    BackupStore,
) {
    let dir = tempfile::tempdir_in(long_temp_dir()).unwrap();
    let path = dir.path().join("live.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let backups = BackupStore::open(&path).unwrap();
    (dir, path, db, backups)
}

/// The temp directory without 8.3 short components. GitHub-hosted Windows
/// runners set TEMP with an 8.3 short profile component (`RUNNER~1`);
/// `canonicalize` expands that, so
/// the canonical-reopen tests would otherwise compare different spellings.
fn long_temp_dir() -> std::path::PathBuf {
    let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    #[cfg(windows)]
    if let Some(verbatim) = temp
        .to_str()
        .and_then(|s| s.strip_prefix(r"\\?\"))
        .filter(|rest| !rest.starts_with(r"UNC\"))
    {
        return std::path::PathBuf::from(verbatim);
    }
    temp
}
fn make(
    backups: &mut BackupStore,
    db: &Connection,
    class: DestructiveClass,
) -> (RequestId, BackupInfo) {
    let request = backups.request(class, 100).unwrap();
    let info = backups
        .create(
            db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {},
        )
        .unwrap();
    (request, info)
}
fn job(
    backups: &BackupStore,
    class: DestructiveClass,
    key: &str,
) -> maxed_store::maintenance::JobRequest {
    maxed_store::maintenance::JobRequest {
        kind: class.tag().into(),
        key: key.into(),
        format_version: 1,
        input_generation: backups.generation().into(),
        input_revision: 0,
    }
}
fn admit(backups: &mut BackupStore, db: &mut Connection, request: RequestId, key: &str) -> i64 {
    use maxed_store::maintenance::*;
    let input = job(backups, DestructiveClass::ReceiptConversion, key);
    backups.bind_job(request, &input).unwrap();
    backups.prepare_authorization(request).unwrap();
    db.execute("INSERT INTO maintenance_jobs(kind,job_key,format_version,input_generation,input_revision) VALUES(?1,?2,1,?3,0)",rusqlite::params![input.kind,input.key,input.input_generation]).unwrap();
    let id = db.last_insert_rowid();
    let tx = db.transaction().unwrap();
    let job = Job {
        request: input,
        cursor: None,
        backup_id: None,
        status: JobStatus {
            id,
            phase: Phase::Deleting,
            paused: false,
            reason: None,
            requested_at: 100,
            updated_at: 100,
            processed_records: 0,
            processed_bytes: 0,
            estimates: Estimates::default(),
        },
    };
    assert!(backups.authorize_job(&tx, &job).unwrap());
    tx.commit().unwrap();
    id
}

#[test]
fn class_request_and_lineage_associations_survive_restart_and_batches_reuse_one_file() {
    let (_dir, path, mut db, mut backups) = fixture();
    let (request, info) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    for n in 0..100 {
        admit(&mut backups, &mut db, request, &format!("batch-{n}"));
    }
    assert_eq!(backups.list().unwrap(), std::slice::from_ref(&info));
    assert_eq!(
        backups.delete(&db, info.id, &Space(Some(u64::MAX))),
        Err(BackupError::InUse)
    );
    drop(backups);
    let mut backups = BackupStore::open(&std::fs::canonicalize(&path).unwrap()).unwrap();
    assert_eq!(
        backups
            .current_request(DestructiveClass::ReceiptConversion)
            .unwrap(),
        Some(request)
    );
    assert_eq!(backups.prepare_authorization(request).unwrap(), info.id);
    admit(&mut backups, &mut db, request, "after-canonical-reopen");
    let wrong = job(&backups, DestructiveClass::DetailReplacement, "wrong-class");
    assert_eq!(backups.bind_job(request, &wrong), Err(BackupError::Invalid));
    assert_eq!(
        backups.request(DestructiveClass::ReceiptConversion, 1),
        Err(BackupError::InUse)
    );
    db.execute("UPDATE maintenance_jobs SET phase='reclaimable'", [])
        .unwrap();
    let renewal = backups
        .request(DestructiveClass::ReceiptConversion, 1)
        .unwrap();
    assert_eq!(
        backups.prepare_authorization(renewal),
        Err(BackupError::Invalid)
    );
    assert_eq!(
        backups.prepare_authorization(request),
        Err(BackupError::Superseded)
    );
    let ready = backups
        .create(
            &db,
            renewal,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {},
        )
        .unwrap();
    assert_eq!(ready.slot, Slot::Rolling);
    assert_eq!(backups.list().unwrap().len(), 2);
    assert_eq!(backups.prepare_authorization(renewal).unwrap(), ready.id);
}

struct Fail(Boundary);
impl BackupEnvironment for Fail {
    fn available_space(&self, _: &Path) -> Option<u64> {
        Some(u64::MAX)
    }
    fn timestamp(&self) -> u64 {
        100
    }
    fn boundary(&self, b: Boundary) -> Result<(), BackupError> {
        if b == self.0 {
            Err(BackupError::Io)
        } else {
            Ok(())
        }
    }
}

#[test]
fn interrupted_verification_flush_and_publish_never_authorize_a_candidate() {
    for boundary in [
        Boundary::Copy,
        Boundary::Verify,
        Boundary::FileFlush,
        Boundary::Publish,
    ] {
        let (_dir, path, db, mut backups) = fixture();
        let request = backups
            .request(DestructiveClass::ReceiptConversion, 100)
            .unwrap();
        assert_eq!(
            backups.create(
                &db,
                request,
                &Fail(boundary),
                &AtomicBool::new(false),
                &mut || {}
            ),
            Err(BackupError::Io)
        );
        assert!(backups.list().unwrap().is_empty());
        assert_eq!(
            backups.prepare_authorization(request),
            Err(BackupError::Invalid)
        );
        drop(backups);
        let mut backups = BackupStore::open(&path).unwrap();
        assert!(backups.list().unwrap().is_empty());
        backups.recover(&Space(Some(u64::MAX))).unwrap();
        assert!(backups
            .create(
                &db,
                request,
                &Space(Some(u64::MAX)),
                &AtomicBool::new(false),
                &mut || {}
            )
            .is_ok());
    }
}

#[test]
fn rolling_crashes_keep_initial_and_at_most_two_ready_images() {
    for boundary in [Boundary::Verify, Boundary::Publish, Boundary::Cleanup] {
        let (dir, path, mut db, mut backups) = fixture();
        let (request, initial) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
        admit(&mut backups, &mut db, request, "first");
        db.execute("UPDATE maintenance_jobs SET phase='reclaimable'", [])
            .unwrap();
        let (_, rolling) = make(&mut backups, &db, DestructiveClass::DetailReplacement);
        assert_eq!(rolling.slot, Slot::Rolling);
        let request = backups
            .request(DestructiveClass::SealedReceiptPruning, 200)
            .unwrap();
        assert_eq!(
            backups.create(
                &db,
                request,
                &Fail(boundary),
                &AtomicBool::new(false),
                &mut || {}
            ),
            Err(BackupError::Io)
        );
        let ready = backups.list().unwrap();
        assert_eq!(ready.len(), 2);
        assert!(ready.contains(&initial));
        assert_eq!(ready.contains(&rolling), boundary != Boundary::Cleanup);
        let usage = backups.usage().unwrap();
        assert_eq!(
            usage.retained_bytes,
            Some(ready.iter().map(|b| b.bytes).sum())
        );
        assert!(usage.temporary_bytes.is_some_and(|bytes| bytes > 0));
        let inventory = backups.list_managed().unwrap();
        assert_eq!(inventory.iter().filter(|b| b.ready).count(), 2);
        assert_eq!(inventory.iter().filter(|b| !b.ready).count(), 1);
        let count = std::fs::read_dir(dir.path().join("backups/live"))
            .unwrap()
            .filter(|p| {
                p.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("backup-")
                    && p.as_ref()
                        .unwrap()
                        .path()
                        .extension()
                        .is_some_and(|s| s == "sqlite3")
            })
            .count();
        assert_eq!(
            count, 3,
            "two ready images plus one unpublished or retired candidate"
        );
        drop(backups);
        let mut backups = BackupStore::open(&path).unwrap();
        assert_eq!(
            backups.recover(&Fail(Boundary::Cleanup)),
            Err(BackupError::Io)
        );
        assert_eq!(
            backups.create(
                &db,
                request,
                &Fail(Boundary::Cleanup),
                &AtomicBool::new(false),
                &mut || {}
            ),
            Err(BackupError::Io)
        );
        backups.recover(&Space(Some(u64::MAX))).unwrap();
        assert_eq!(backups.list().unwrap().len(), 2);
        assert_eq!(backups.usage().unwrap().temporary_bytes, Some(0));
    }
}

#[test]
fn committed_writes_during_large_copy_preserve_snapshot_and_recorded_heads() {
    let (dir, path, db, mut backups) = fixture();
    db.execute_batch("CREATE TABLE synthetic_padding(bytes BLOB); INSERT INTO synthetic_padding VALUES(zeroblob(67108864)); PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO stream_heads VALUES('synthetic-concurrent',7);").unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() >= 64 * 1024 * 1024);
    let writer = Connection::open(&path).unwrap();
    writer.busy_timeout(std::time::Duration::ZERO).unwrap();
    let request = backups
        .request(DestructiveClass::ReceiptConversion, 100)
        .unwrap();
    let mut commits = 0;
    let result = backups.create(
        &db,
        request,
        &Space(Some(u64::MAX)),
        &AtomicBool::new(false),
        &mut || {
            commits += 1;
            writer
                .execute("UPDATE stream_heads SET sequence=sequence+1", [])
                .unwrap();
        },
    );
    let info = result.expect("periodic capture commits must not restart the snapshot copy");
    assert!(commits >= 128);
    assert!(commits < 256, "copy must progress without restarting");
    let copy = Connection::open(
        dir.path()
            .join("backups/live")
            .join(format!("backup-{:020}.sqlite3", info.id.0)),
    )
    .unwrap();
    assert_eq!(
        copy.query_row("SELECT sequence FROM stream_heads", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        7
    );
    let manifest = Connection::open(dir.path().join("backups/live/manifest.sqlite3")).unwrap();
    assert_eq!(
        manifest
            .query_row(
                "SELECT sequence FROM heads WHERE backup_id=?1",
                [info.id.0],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        7
    );
    assert_eq!(
        db.query_row("SELECT sequence FROM stream_heads", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        7 + commits
    );
    drop(copy);
    drop(manifest);
    drop(backups);
    let mut backups = BackupStore::open(&path).unwrap();
    assert_eq!(backups.prepare_authorization(request).unwrap(), info.id);
}

#[test]
fn cancellation_leaves_original_authoritative() {
    let (_dir, _path, db, mut backups) = fixture();
    db.execute_batch("CREATE TABLE synthetic_padding(bytes BLOB); INSERT INTO synthetic_padding VALUES(zeroblob(2097152));").unwrap();
    let request = backups
        .request(DestructiveClass::ReceiptConversion, 100)
        .unwrap();
    let cancel = AtomicBool::new(false);
    assert_eq!(
        backups.create(&db, request, &Space(Some(u64::MAX)), &cancel, &mut || {
            cancel.store(true, std::sync::atomic::Ordering::Release);
        }),
        Err(BackupError::Cancelled)
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM stream_heads", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(backups
        .create(
            &db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {}
        )
        .is_ok());
}

#[test]
fn snapshot_ignores_an_uncommitted_source_transaction() {
    let (dir, _path, db, mut backups) = fixture();
    db.execute_batch("PRAGMA journal_mode=WAL; INSERT INTO stream_heads VALUES('synthetic',1); BEGIN IMMEDIATE; UPDATE stream_heads SET sequence=2;").unwrap();
    let (_, info) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    let copy = Connection::open(
        dir.path()
            .join("backups/live")
            .join(format!("backup-{:020}.sqlite3", info.id.0)),
    )
    .unwrap();
    assert_eq!(
        copy.query_row("SELECT sequence FROM stream_heads", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(!db.is_autocommit());
    db.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn wal_pressure_releases_snapshot_so_checkpoint_and_retry_can_progress() {
    let (_dir, path, db, mut backups) = fixture();
    db.execute_batch("CREATE TABLE synthetic_padding(bytes BLOB); INSERT INTO synthetic_padding VALUES(zeroblob(2097152)); PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;").unwrap();
    let writer = Connection::open(&path).unwrap();
    writer.busy_timeout(std::time::Duration::ZERO).unwrap();
    writer.execute_batch("PRAGMA wal_autocheckpoint=0").unwrap();
    let request = backups
        .request(DestructiveClass::ReceiptConversion, 100)
        .unwrap();
    let result = backups.create(
        &db,
        request,
        &Space(Some(u64::MAX)),
        &AtomicBool::new(false),
        &mut || {
            writer
                .execute(
                    "INSERT INTO synthetic_padding VALUES(zeroblob(33554432))",
                    [],
                )
                .unwrap();
        },
    );
    assert_eq!(result, Err(BackupError::WalPressure));
    assert!(backups.list().unwrap().is_empty());
    let checkpoint: (i64, i64, i64) = writer
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(
        checkpoint,
        (0, 0, 0),
        "aborted snapshot must release its WAL pin"
    );
    backups
        .create(
            &db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {},
        )
        .unwrap();
}

#[cfg(windows)]
#[test]
fn redirected_app_data_is_allowed_but_managed_backup_junctions_are_refused() {
    fn junction(link: &Path, target: &Path) {
        let result = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:MAXED_SYNTHETIC_LINK -Target $env:MAXED_SYNTHETIC_TARGET | Out-Null"])
            .env("MAXED_SYNTHETIC_LINK", link)
            .env("MAXED_SYNTHETIC_TARGET", target)
            .output().unwrap();
        assert!(
            result.status.success(),
            "synthetic junction creation failed"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let actual = dir.path().join("actual");
    std::fs::create_dir(&actual).unwrap();
    let redirected = dir.path().join("redirected");
    junction(&redirected, &actual);
    let path = redirected.join("live.sqlite3");
    let mut db = Connection::open(&path).unwrap();
    migrate(&mut db).unwrap();
    let mut backups =
        BackupStore::open(&path).expect("ancestor junctions are outside the managed root");
    make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    drop(backups);
    std::fs::rename(actual.join("backups"), actual.join("saved-backups")).unwrap();
    junction(&actual.join("backups"), &actual.join("saved-backups"));
    assert!(matches!(
        BackupStore::open(&path),
        Err(BackupError::Invalid)
    ));
    std::fs::remove_dir(actual.join("backups")).unwrap();
    std::fs::remove_dir(redirected).unwrap();
}

#[test]
fn unused_initial_can_be_replaced_after_cancellation_or_request_renewal() {
    for first_class in [
        DestructiveClass::DetailReplacement,
        DestructiveClass::ReceiptConversion,
    ] {
        let (_dir, _path, mut db, mut backups) = fixture();
        let (first_request, initial) = make(&mut backups, &db, first_class);
        backups.prepare_authorization(first_request).unwrap();
        let input = job(&backups, first_class, "first-request");
        backups.bind_job(first_request, &input).unwrap();
        let cancelled = first_class == DestructiveClass::DetailReplacement;
        db.execute("INSERT INTO maintenance_jobs(kind,job_key,format_version,input_generation,input_revision,paused) VALUES(?1,?2,1,?3,0,?4)", rusqlite::params![input.kind,input.key,input.input_generation,cancelled]).unwrap();
        let request = backups
            .request(DestructiveClass::ReceiptConversion, 101)
            .unwrap();
        assert_eq!(
            backups.create(
                &db,
                request,
                &Fail(Boundary::Publish),
                &AtomicBool::new(false),
                &mut || {}
            ),
            Err(BackupError::Io)
        );
        assert_eq!(backups.list().unwrap(), [initial]);
        let replacement = backups
            .create(
                &db,
                request,
                &Space(Some(u64::MAX)),
                &AtomicBool::new(false),
                &mut || {},
            )
            .unwrap();
        admit(&mut backups, &mut db, request, "first-destruction");
        assert_eq!(replacement.slot, Slot::Initial);
        assert_eq!(backups.list().unwrap(), [replacement]);
        assert_eq!(
            backups.prepare_authorization(first_request),
            Err(BackupError::Superseded)
        );
    }
}

#[test]
fn failed_legacy_initial_verification_can_be_retried_without_manual_deletion() {
    use crate::common::installed_schema;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("live.sqlite3");
    let db = Connection::open(&path).unwrap();
    installed_schema::install(&db, 6);
    let mut backups = BackupStore::open(&path).unwrap();
    let (request, initial) = make(&mut backups, &db, DestructiveClass::LegacyUpgrade);
    drop(backups);
    std::fs::write(
        dir.path()
            .join("backups/live")
            .join(format!("backup-{:020}.sqlite3", initial.id.0)),
        b"incomplete",
    )
    .unwrap();
    let mut backups = BackupStore::open(&path).unwrap();
    assert_eq!(
        backups.prepare_authorization(request),
        Err(BackupError::VerificationFailed)
    );
    drop(backups);
    drop(db);
    HistoryUpgrade::open(&path)
        .unwrap()
        .confirm_and_upgrade(&Space(Some(u64::MAX)), &AtomicBool::new(false))
        .expect("verification retry must replace the unused initial");
    let backups = BackupStore::open(&path).unwrap();
    let ready = backups.list().unwrap();
    assert_eq!(ready.len(), 1);
    assert_ne!(ready[0].id, initial.id);
    assert_eq!(ready[0].slot, Slot::Initial);
    assert_eq!(
        maxed_store::open_reader(&path)
            .unwrap()
            .user_version()
            .unwrap(),
        maxed_store::SCHEMA_VERSION
    );
}

#[test]
fn deletion_requires_renewal_and_missing_corrupt_or_other_lineage_backups_fail_closed() {
    let (dir, path, mut db, mut backups) = fixture();
    let (request, info) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    admit(&mut backups, &mut db, request, "first");
    db.execute("UPDATE maintenance_jobs SET phase='reclaimable'", [])
        .unwrap();
    backups
        .delete(&db, info.id, &Space(Some(u64::MAX)))
        .unwrap();
    assert_eq!(
        backups.prepare_authorization(request),
        Err(BackupError::Superseded)
    );
    let (renewal, rolling) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    assert_eq!(
        rolling.slot,
        Slot::Rolling,
        "deleting initial never resets the first-destruction marker"
    );
    drop(backups);
    let copy = dir
        .path()
        .join("backups/live")
        .join(format!("backup-{:020}.sqlite3", rolling.id.0));
    std::fs::write(&copy, b"incomplete").unwrap();
    let mut backups = BackupStore::open(&path).unwrap();
    assert_eq!(
        backups.prepare_authorization(renewal),
        Err(BackupError::VerificationFailed)
    );
    drop(backups);
    std::fs::remove_file(copy).unwrap();
    assert!(BackupStore::open(&path)
        .unwrap()
        .prepare_authorization(renewal)
        .is_err());
    let other = dir.path().join("other.sqlite3");
    let mut other_db = Connection::open(&other).unwrap();
    migrate(&mut other_db).unwrap();
    let other_store = BackupStore::open(&other).unwrap();
    assert_ne!(
        other_store.generation(),
        job(
            &BackupStore::open(&path).unwrap(),
            DestructiveClass::ReceiptConversion,
            "unused"
        )
        .input_generation
    );
}

#[test]
fn repeated_copy_joins_ready_and_deletion_cannot_recopy_an_old_request() {
    let (_dir, _path, mut db, mut backups) = fixture();
    let (request, initial) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    let duplicate = backups
        .create(
            &db,
            request,
            &Space(None),
            &AtomicBool::new(false),
            &mut || {},
        )
        .unwrap();
    assert_eq!(
        initial, duplicate,
        "joining a ready request never copies or consumes a new slot"
    );
    admit(&mut backups, &mut db, request, "first");
    db.execute("UPDATE maintenance_jobs SET phase='reclaimable'", [])
        .unwrap();
    backups
        .delete(&db, initial.id, &Space(Some(u64::MAX)))
        .unwrap();
    assert_eq!(
        backups.create(
            &db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {}
        ),
        Err(BackupError::Superseded),
        "invalidated authorization needs a new request"
    );
}

#[test]
fn readiness_rejects_completion_before_request_even_if_wall_clock_looks_newer() {
    let (dir, path, db, mut backups) = fixture();
    let (request, _) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    drop(backups);
    let manifest = Connection::open(dir.path().join("backups/live/manifest.sqlite3")).unwrap();
    manifest
        .execute(
            "UPDATE backups SET completed_order=0,completed_at=999999",
            [],
        )
        .unwrap();
    drop(manifest);
    assert_eq!(
        BackupStore::open(&path)
            .unwrap()
            .prepare_authorization(request),
        Err(BackupError::Invalid)
    );
}

#[test]
fn a_private_manifest_from_another_database_cannot_authorize_this_lineage() {
    let (dir, _path, db, mut backups) = fixture();
    make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    drop(backups);
    let other = dir.path().join("other.sqlite3");
    let mut db = Connection::open(&other).unwrap();
    migrate(&mut db).unwrap();
    drop(db);
    std::fs::create_dir(dir.path().join("backups/other")).unwrap();
    std::fs::copy(
        dir.path().join("backups/live/manifest.sqlite3"),
        dir.path().join("backups/other/manifest.sqlite3"),
    )
    .unwrap();
    assert!(matches!(
        BackupStore::open(&other),
        Err(BackupError::Invalid)
    ));
}

#[test]
fn managed_directories_refuse_redirection_and_verified_images_are_immutable() {
    let (dir, _path, db, mut backups) = fixture();
    let (request, info) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    backups.prepare_authorization(request).unwrap();
    let image = dir
        .path()
        .join("backups/live")
        .join(format!("backup-{:020}.sqlite3", info.id.0));
    #[cfg(windows)]
    {
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .open(&image)
            .is_err());
        assert!(std::fs::remove_file(&image).is_err());
        assert!(std::fs::rename(dir.path().join("backups"), dir.path().join("moved")).is_err());
    }
    assert_eq!(
        backups.delete(&db, BackupId(-1), &Space(Some(u64::MAX))),
        Err(BackupError::Invalid)
    );
}

#[test]
fn real_manifest_publish_failure_retains_original_and_never_marks_ready() {
    let (dir, _path, db, mut backups) = fixture();
    let manifest = Connection::open(dir.path().join("backups/live/manifest.sqlite3")).unwrap();
    manifest.execute_batch("CREATE TRIGGER reject_ready BEFORE UPDATE OF state ON backups WHEN NEW.state='ready' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    let request = backups
        .request(DestructiveClass::ReceiptConversion, 100)
        .unwrap();
    let before = db.total_changes();
    assert!(backups
        .create(
            &db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {}
        )
        .is_err());
    assert!(backups.list().unwrap().is_empty());
    assert_eq!(db.total_changes(), before);
    manifest.execute_batch("DROP TRIGGER reject_ready").unwrap();
    assert!(backups
        .create(
            &db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {}
        )
        .is_ok());
}

#[test]
fn backup_busy_full_io_and_readonly_failures_leave_capture_writable() {
    struct Fault {
        kind: &'static str,
        directory: std::path::PathBuf,
        permissions: std::cell::RefCell<Vec<(std::path::PathBuf, std::fs::Permissions)>>,
        locks: std::cell::RefCell<Vec<Connection>>,
    }
    impl BackupEnvironment for Fault {
        fn available_space(&self, _: &Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            100
        }
        fn boundary(&self, at: Boundary) -> Result<(), BackupError> {
            match (self.kind, at) {
                ("busy", Boundary::Copy) => {
                    for entry in std::fs::read_dir(&self.directory).unwrap() {
                        let path = entry.unwrap().path();
                        if path
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with("backup-")
                        {
                            let db = Connection::open(&path).unwrap();
                            db.execute_batch("BEGIN EXCLUSIVE").unwrap();
                            self.locks.borrow_mut().push(db);
                        }
                    }
                    Ok(())
                }
                ("full", Boundary::Copy) => Err(BackupError::InsufficientSpace),
                ("io", Boundary::Copy) => Err(BackupError::Io),
                ("readonly", Boundary::FileFlush) => {
                    for entry in std::fs::read_dir(&self.directory).unwrap() {
                        let path = entry.unwrap().path();
                        if path
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with("backup-")
                        {
                            let mut permissions = std::fs::metadata(&path).unwrap().permissions();
                            self.permissions
                                .borrow_mut()
                                .push((path.clone(), permissions.clone()));
                            permissions.set_readonly(true);
                            std::fs::set_permissions(path, permissions).unwrap();
                        }
                    }
                    Ok(())
                }
                _ => Ok(()),
            }
        }
    }
    for kind in ["busy", "full", "io", "readonly"] {
        let (dir, path, db, mut backups) = fixture();
        let events = crate::common::story_events();
        let writer = maxed_store::start_writer(
            path.clone(),
            maxed_store::WriterConfig {
                stream_id: events[0].stream_id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        let request = backups
            .request(DestructiveClass::ReceiptConversion, 100)
            .unwrap();
        let fault = Fault {
            kind,
            directory: dir.path().join("backups/live"),
            permissions: Default::default(),
            locks: Default::default(),
        };
        let result = backups.create(&db, request, &fault, &AtomicBool::new(false), &mut || {});
        let expected = match kind {
            "busy" => BackupError::Busy,
            "full" => BackupError::InsufficientSpace,
            _ => BackupError::Io,
        };
        assert_eq!(result, Err(expected));
        fault.locks.borrow_mut().clear();
        assert!(backups.list().unwrap().is_empty());
        for event in events {
            crate::common::submit(&writer, event);
        }
        writer.flush().unwrap();
        assert_eq!(writer.status().state, maxed_types::StoreState::Ready);
        writer.shutdown().unwrap();
        for (candidate, permissions) in fault.permissions.into_inner() {
            std::fs::set_permissions(candidate, permissions).unwrap();
        }
        backups.recover(&Space(Some(u64::MAX))).unwrap();
    }
}

#[test]
fn exact_pre_maintenance_backups_can_be_deleted_without_maintenance_tables() {
    for version in [12, 14, 15] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite3");
        let db = Connection::open(&path).unwrap();
        crate::common::installed_schema::install(&db, version);
        let mut backups = BackupStore::open(&path).unwrap();
        let (_, info) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
        assert!(
            backups.delete(&db, info.id, &Space(Some(u64::MAX))).is_ok(),
            "v{version} backup deletion must not query maintenance tables"
        );
        assert!(backups.list().unwrap().is_empty());
        crate::common::installed_schema::assert_installed(&db, version);
    }
}

#[test]
fn crucible_configuration_is_backed_up_and_integrity_checked() {
    let (dir, _path, mut db, mut backups) = fixture();
    let defs = vec![maxed_types::CrucibleDef {
        id: "synthetic".into(),
        damage_multiplier: 1.5,
    }];
    maxed_store::upsert_crucible_definitions(&mut db, &defs, maxed_types::Timestamp(10)).unwrap();
    let (_, info) = make(&mut backups, &db, DestructiveClass::ReceiptConversion);
    let image = dir
        .path()
        .join("backups/live")
        .join(format!("backup-{:020}.sqlite3", info.id.0));
    let restored =
        Connection::open_with_flags(&image, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(
        maxed_store::read_crucible_definitions(&restored).unwrap(),
        defs
    );
    crate::common::installed_schema::assert_integrity(&restored);
    // A new backup (rather than reuse of the verified initial image) must
    // reject invalid configuration through whole-database integrity checking.
    let (_invalid_dir, _invalid_path, mut db, mut backups) = fixture();
    maxed_store::upsert_crucible_definitions(&mut db, &defs, maxed_types::Timestamp(10)).unwrap();
    db.execute_batch(
        "PRAGMA ignore_check_constraints=ON;
        UPDATE crucible_definitions SET damage_multiplier=101; PRAGMA ignore_check_constraints=OFF;",
    )
    .unwrap();
    assert_eq!(
        db.query_row(
            "SELECT damage_multiplier FROM crucible_definitions",
            [],
            |r| r.get::<_, f64>(0)
        )
        .unwrap(),
        101.
    );
    assert_ne!(
        db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    let request = backups
        .request(DestructiveClass::DetailReplacement, 200)
        .unwrap();
    assert!(backups
        .create(
            &db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {}
        )
        .is_err());
}
