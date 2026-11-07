use super::*;
use maxed_store::backup::{BackupEnvironment, BackupError, BackupStore, DestructiveClass, Slot};
use maxed_store::maintenance::{Control, PauseReason, Phase};
use maxed_types::*;
use std::future::Future;
use std::path::Path;

pub(super) fn current_fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let mut db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    db.execute(
        "INSERT INTO stream_heads VALUES('synthetic-retention',3)",
        [],
    )
    .unwrap();
    for seq in 1..=3 {
        db.execute(
            "INSERT INTO event_receipts VALUES('synthetic-retention',?1,?2)",
            rusqlite::params![seq, "ab".repeat(32)],
        )
        .unwrap();
    }
    root
}
pub(super) fn initialized(root: &Path) -> Arc<CaptureRuntime> {
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.into()).unwrap();
    tauri::async_runtime::block_on(runtime.wait_history_preparation());
    runtime
}
fn observed(runtime: &Arc<CaptureRuntime>, root: &Path, estimate: bool) -> StorageStatus {
    let base = crate::storage_usage::status(root, runtime.recording_status(1_000_000_000));
    tauri::async_runtime::block_on(runtime.storage_status(base, estimate))
}
#[track_caller]
pub(super) fn eventually(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "synthetic maintenance did not reach the expected state"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn poll_pending<F: std::future::Future>(future: std::pin::Pin<&mut F>) {
    assert!(future
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
        .is_pending());
}

fn home_scope() -> HomeScope {
    HomeScope::Today {
        range: TimeRange {
            start: Timestamp(0),
            end: Timestamp(100),
        },
    }
}

struct HeldCopy {
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
    once: AtomicBool,
}
impl BackupEnvironment for HeldCopy {
    fn available_space(&self, _: &Path) -> Option<u64> {
        Some(u64::MAX)
    }
    fn timestamp(&self) -> u64 {
        100
    }
    fn boundary(&self, at: maxed_store::backup::Boundary) -> Result<(), BackupError> {
        if at == maxed_store::backup::Boundary::Copy && !self.once.swap(true, Ordering::AcqRel) {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        Ok(())
    }
}

#[test]
fn stop_and_setup_demo_refuse_an_active_confirmed_upgrade() {
    struct Lifecycle(Arc<CaptureRuntime>);
    impl crate::setup::Lifecycle for Lifecycle {
        fn running(&self) -> bool {
            self.0.is_running()
        }
        fn live(&self) -> bool {
            false
        }
        fn stop(&self) -> Result<(), String> {
            self.0.stop_and_flush()
        }
        fn start(
            &self,
            _: Option<maxed_assets::CachedCatalog>,
            _: maxed_capture::CaptureHandle,
        ) -> Result<(), String> {
            panic!("no capture in upgrade test")
        }
        fn demo(&self, _: bool) -> Result<(), String> {
            panic!("demo must be refused during upgrade")
        }
    }
    for demo in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("live.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        installed::install(&db, 6);
        drop(db);
        let runtime = initialized(root.path());
        let service = crate::setup::SetupService::with_discovery_roots(
            root.path().into(),
            Arc::new(crate::setup::NativeCapture),
            Arc::new(Lifecycle(runtime.clone())),
            Arc::new(crate::assets_bridge::AssetBridge::new(Arc::new(
                crate::assets_bridge::NativeAssets,
            ))),
            vec![],
        )
        .unwrap();
        let (entered, waiting) = crossbeam_channel::bounded(1);
        let (release, held) = crossbeam_channel::bounded(1);
        let upgrade = std::thread::spawn({
            let runtime = runtime.clone();
            move || {
                runtime.backup_and_upgrade_with(
                    &HeldCopy {
                        entered,
                        release: held,
                        once: AtomicBool::new(false),
                    },
                    &AtomicBool::new(false),
                )
            }
        });
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        let (send, receive) = crossbeam_channel::bounded(1);
        let action = std::thread::spawn(move || {
            send.send(if demo {
                service.set_demo(true)
            } else {
                service.stop()
            })
            .unwrap()
        });
        let result = receive.recv_timeout(Duration::from_secs(1));
        let phase = runtime.history_upgrade_status().unwrap().phase;
        release.send(()).unwrap();
        action.join().unwrap();
        let upgraded = upgrade.join().unwrap();
        assert_eq!(
            result,
            Ok(Err(
                "History upgrade is running; wait for it to finish.".into()
            ))
        );
        assert_eq!(phase, HistoryUpgradePhase::BackingUp);
        upgraded.unwrap();
        assert_eq!(
            runtime.history_upgrade_status().unwrap().phase,
            HistoryUpgradePhase::Ready
        );
    }
}

#[test]
fn update_cancels_an_active_legacy_upgrade_and_releases_ownership() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    installed::install(&db, 6);
    drop(db);
    let runtime = initialized(root.path());
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    let upgrading = std::thread::spawn({
        let runtime = runtime.clone();
        move || {
            runtime.backup_and_upgrade_with(
                &HeldCopy {
                    entered,
                    release: held,
                    once: AtomicBool::new(false),
                },
                &AtomicBool::new(false),
            )
        }
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let update = std::thread::spawn({
        let runtime = runtime.clone();
        move || runtime.prepare_update()
    });
    eventually(|| {
        runtime
            .shared
            .views
            .maintenance
            .update_paused
            .load(Ordering::Acquire)
    });
    release.send(()).unwrap();
    assert!(matches!(
        upgrading.join().unwrap(),
        Err(maintenance::HistoryUpgradeError::Backup(
            BackupError::Cancelled
        ))
    ));
    update.join().unwrap().unwrap();
    assert!(storage::namespace_lease(root.path(), Namespace::Live).is_ok());
    assert_eq!(
        installed::user_version(&rusqlite::Connection::open(&path).unwrap()),
        6
    );
    runtime.finish_update();
}

#[test]
fn capture_start_joins_an_actual_stopped_backup_copy_before_acquiring_live() {
    let root = current_fixture();
    let runtime = initialized(root.path());
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    runtime
        .shared
        .views
        .maintenance
        .set_environment(Arc::new(HeldCopy {
            entered,
            release: held,
            once: AtomicBool::new(false),
        }));
    runtime.compact_storage().unwrap();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let (started, observed_start) = crossbeam_channel::bounded(1);
    runtime
        .launch(
            Arc::new(|_| {}),
            Bridge::default(),
            RuntimeOptions::live(root.path().into(), None),
            move || {
                started.send(()).unwrap();
                Err(maxed_capture::CaptureError::Missing)
            },
        )
        .unwrap();
    assert!(observed_start
        .recv_timeout(Duration::from_millis(100))
        .is_err());
    assert!(storage::namespace_lease(root.path(), Namespace::Live).is_err());
    release.send(()).unwrap();
    observed_start.recv_timeout(Duration::from_secs(5)).unwrap();
    runtime.shutdown().unwrap();
    assert!(storage::namespace_lease(root.path(), Namespace::Live).is_ok());
}

#[test]
fn storage_reads_and_pending_page_reads_never_replace_each_other() {
    for key in [
        history::HistoryKey::Home(home_scope()),
        history::HistoryKey::HomeHud(home_scope(), vec![], vec![]),
        history::HistoryKey::BossHistory(maxed_types::BossHistoryRequest {
            range: None,
            character_id: None,
            target_types: None,
        }),
        history::HistoryKey::DungeonProfile(maxed_types::DungeonProfileRequest {
            dungeon: "Synthetic Dungeon".into(),
            character_id: None,
            range: maxed_types::TimeRange {
                start: maxed_types::Timestamp(0),
                end: maxed_types::Timestamp(100),
            },
        }),
    ] {
        for storage_first in [false, true] {
            let root = current_fixture();
            let runtime = initialized(root.path());
            let (entered, waiting) = crossbeam_channel::bounded(1);
            let (release, held) = crossbeam_channel::bounded(1);
            let mut active = Box::pin(runtime.submit_history(
                history::HistoryCaller::NativeTest,
                history::HistoryKey::Run("synthetic".into()),
                move |_| {
                    entered.send(()).unwrap();
                    held.recv_timeout(Duration::from_secs(10)).unwrap();
                    Ok(())
                },
            ));
            poll_pending(active.as_mut());
            waiting.recv_timeout(Duration::from_secs(5)).unwrap();
            let reader = runtime.clone();
            let mut page = Box::pin(runtime.submit_history(
                history::HistoryCaller::MainWindow,
                key.clone(),
                move |job| reader.read_history(job, || (), |_| Ok(())),
            ));
            let mut storage = Box::pin(runtime.submit_history(
                history::HistoryCaller::MainWindow,
                history::HistoryKey::MaintenanceJobs,
                |_| Ok(()),
            ));
            if storage_first {
                poll_pending(storage.as_mut());
                poll_pending(page.as_mut());
            } else {
                poll_pending(page.as_mut());
                poll_pending(storage.as_mut());
            }
            let mut poll = Box::pin(runtime.storage_status(
                crate::storage_usage::status(root.path(), runtime.recording_status(1_000_000)),
                false,
            ));
            poll_pending(poll.as_mut());
            release.send(()).unwrap();
            assert!(matches!(
                tauri::async_runtime::block_on(active).unwrap(),
                HistoryReply::Ready { .. }
            ));
            assert!(matches!(
                tauri::async_runtime::block_on(page).unwrap(),
                HistoryReply::Ready { .. }
            ));
            assert!(matches!(
                tauri::async_runtime::block_on(storage).unwrap(),
                HistoryReply::Ready { .. }
            ));
            tauri::async_runtime::block_on(poll);
        }
    }
}

#[test]
fn history_storage_reads_wait_for_preparation_with_context() {
    let root = current_fixture();
    let runtime = Arc::new(CaptureRuntime::default());
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    runtime.initialize_home(root.path().into()).unwrap();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut storage = Box::pin(runtime.submit_history(
        history::HistoryCaller::Storage,
        history::HistoryKey::MaintenanceJobs,
        |_| Ok(17),
    ));
    let mut page = Box::pin(runtime.home_summary(home_scope()));
    poll_pending(page.as_mut());
    // Retain the result so a RED assertion cannot strand the preparation hook.
    let result = storage
        .as_mut()
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()));
    release.send(()).unwrap();
    assert!(
        result.is_pending(),
        "storage must wait for preparation: {result:?}"
    );
    assert_eq!(
        tauri::async_runtime::block_on(storage).unwrap(),
        HistoryReply::Ready { value: 17 }
    );
    assert!(matches!(
        tauri::async_runtime::block_on(page).unwrap(),
        HistoryReply::Ready { .. }
    ));
}

#[test]
fn history_startup_restore_recovers_after_refused_read() {
    let root = current_fixture();
    let path = root.path().join("live.sqlite3");
    let mut owner =
        maxed_store::maintenance::StoppedMaintenance::receipt_conversion(&path).unwrap();
    owner.request_receipt_conversion(100).unwrap();
    drop(owner);
    let runtime = Arc::new(CaptureRuntime::default());
    let (refused, waiting) = crossbeam_channel::bounded(1);
    *runtime
        .shared
        .views
        .maintenance
        .on_jobs_retry
        .lock()
        .unwrap() = Some(Arc::new(move || {
        let _ = refused.try_send(());
    }));
    runtime.shared.home.shutdown_reads();
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.wait_history_preparation());
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    runtime.shared.home.resume_reads();
    eventually(|| {
        observed(&runtime, root.path(), false)
            .maintenance
            .is_some_and(|m| {
                m.jobs
                    .iter()
                    .any(|j| j.phase == StorageMaintenancePhase::Reclaimable)
            })
    });
}

#[test]
fn storage_queue_is_bounded_and_reports_busy_without_displacing_accepted_reads() {
    let root = current_fixture();
    let runtime = initialized(root.path());
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    let mut active = Box::pin(runtime.submit_history(
        history::HistoryCaller::NativeTest,
        history::HistoryKey::Run("synthetic".into()),
        move |_| {
            entered.send(()).unwrap();
            held.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        },
    ));
    poll_pending(active.as_mut());
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut reads = Vec::new();
    for _ in 0..8 {
        let mut read = Box::pin(runtime.submit_history(
            history::HistoryCaller::Storage,
            history::HistoryKey::MaintenanceJobs,
            |_| Ok(()),
        ));
        poll_pending(read.as_mut());
        reads.push(read);
    }
    for _ in 0..40 {
        assert_eq!(
            tauri::async_runtime::block_on(runtime.submit_history(
                history::HistoryCaller::Storage,
                history::HistoryKey::MaintenanceJobs,
                |_| Ok(()),
            ))
            .unwrap(),
            HistoryReply::Busy
        );
    }
    let mut page = Box::pin(runtime.home_summary(home_scope()));
    poll_pending(page.as_mut());
    release.send(()).unwrap();
    assert!(matches!(
        tauri::async_runtime::block_on(active).unwrap(),
        HistoryReply::Ready { .. }
    ));
    for read in reads {
        assert!(matches!(
            tauri::async_runtime::block_on(read).unwrap(),
            HistoryReply::Ready { .. }
        ));
    }
    assert!(matches!(
        tauri::async_runtime::block_on(page).unwrap(),
        HistoryReply::Ready { .. }
    ));
}

#[test]
fn startup_manifest_restore_does_not_supersede_first_home_read() {
    let root = current_fixture();
    let path = root.path().join("live.sqlite3");
    let mut owner =
        maxed_store::maintenance::StoppedMaintenance::receipt_conversion(&path).unwrap();
    let id = owner.request_receipt_conversion(100).unwrap().status.id;
    owner.control(Control::Pause(id)).unwrap();
    drop(owner);
    let runtime = Arc::new(CaptureRuntime::default());
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
        entered.send(()).unwrap();
        held.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    runtime.initialize_home(root.path().into()).unwrap();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut home = Box::pin(runtime.home_hud(home_scope(), vec![], vec![]));
    poll_pending(home.as_mut());
    release.send(()).unwrap();
    assert!(matches!(
        tauri::async_runtime::block_on(home).unwrap(),
        HistoryReply::Ready { .. }
    ));
    eventually(|| {
        observed(&runtime, root.path(), false)
            .maintenance
            .is_some_and(|m| m.jobs.len() == 1)
    });
}

#[test]
fn probes_cover_absent_legacy_and_compatible_schemas_without_requesting_work() {
    for version in [
        None,
        Some(0),
        Some(2),
        Some(6),
        Some(10),
        Some(12),
        Some(14),
        Some(15),
        Some(16),
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("live.sqlite3");
        if let Some(version) = version {
            let mut db = rusqlite::Connection::open(&path).unwrap();
            if version == 16 {
                maxed_store::migrate(&mut db).unwrap();
            } else if version != 0 {
                installed::install(&db, version);
            }
        }
        // Set only the owner-resolved path; no normal preparation/migration.
        let runtime = Arc::new(CaptureRuntime::default());
        runtime.register_storage_path_for_test(&path);
        let before = std::fs::read(&path).ok();
        for free in [None, Some(0), Some(u64::MAX)] {
            let status = runtime.history_upgrade_status_with(&Space(free)).unwrap();
            assert_eq!(status.schema_version, version.map(|v| v as u32));
            let legacy = version.is_some_and(|v| (1..11).contains(&v));
            assert_eq!(
                status.phase,
                if !legacy {
                    HistoryUpgradePhase::Ready
                } else if free == Some(u64::MAX) {
                    HistoryUpgradePhase::Required
                } else {
                    HistoryUpgradePhase::Blocked
                }
            );
            let _ = observed(&runtime, root.path(), false);
        }
        assert_eq!(std::fs::read(&path).ok(), before);
        assert!(!root.path().join("backups").exists());
    }
}

#[test]
fn fresh_start_and_storage_polling_create_no_backup_job_or_estimate() {
    let root = current_fixture();
    let runtime = initialized(root.path());
    for _ in 0..3 {
        let status = observed(&runtime, root.path(), false);
        assert_eq!(status.receipt_estimate.measured_at, None);
    }
    let db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM maintenance_jobs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM event_receipts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert!(!root.path().join("backups").exists());
}

#[test]
fn conversion_rechecks_space_coalesces_requests_and_deletes_only_managed_completed_backups() {
    use std::sync::atomic::AtomicUsize;
    struct SpaceChanges {
        free: Mutex<Option<u64>>,
        calls: AtomicUsize,
    }
    impl BackupEnvironment for SpaceChanges {
        fn available_space(&self, _: &Path) -> Option<u64> {
            self.calls.fetch_add(1, Ordering::AcqRel);
            *self.free.lock().unwrap()
        }
        fn timestamp(&self) -> u64 {
            100
        }
    }
    let root = current_fixture();
    let runtime = initialized(root.path());
    let space = Arc::new(SpaceChanges {
        free: Mutex::new(None),
        calls: AtomicUsize::new(0),
    });
    runtime
        .shared
        .views
        .maintenance
        .set_environment(space.clone());
    runtime.compact_storage().unwrap();
    eventually(|| {
        observed(&runtime, root.path(), false).failure == Some(StorageFailure::SpaceUnknown)
    });
    *space.free.lock().unwrap() = Some(0);
    runtime.compact_storage().unwrap();
    eventually(|| {
        observed(&runtime, root.path(), false).failure == Some(StorageFailure::InsufficientSpace)
    });
    assert!(runtime.list_storage_backups().unwrap().is_empty());
    *space.free.lock().unwrap() = Some(u64::MAX);
    runtime.compact_storage().unwrap();
    runtime.compact_storage().unwrap();
    eventually(|| {
        observed(&runtime, root.path(), false)
            .maintenance
            .is_some_and(|m| {
                m.jobs
                    .last()
                    .is_some_and(|j| j.phase == StorageMaintenancePhase::Reclaimable)
            })
    });
    assert!(space.calls.load(Ordering::Acquire) >= 3);
    let db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM maintenance_jobs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(receipt_tests::count(&db), 3);
    let backups = runtime.list_storage_backups().unwrap();
    assert_eq!(backups.len(), 1);
    assert!(backups[0].ready && !backups[0].in_use);
    assert!(backups[0].completed_at.is_some());
    for forged in [
        "../live.sqlite3",
        "0",
        "01",
        "-1",
        "9223372036854775808",
        "999",
    ] {
        assert!(runtime.delete_storage_backup(forged).is_err());
    }
    runtime.delete_storage_backup(&backups[0].id).unwrap();
    assert!(runtime.list_storage_backups().unwrap().is_empty());
    let manifest =
        rusqlite::Connection::open(root.path().join("backups/live/manifest.sqlite3")).unwrap();
    assert_eq!(
        manifest
            .query_row(
                "SELECT invalidated FROM requests ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    drop(manifest);
    drop(db);
    runtime.shutdown().unwrap();
    assert!(storage::namespace_lease(root.path(), Namespace::Live).is_ok());
    assert!(BackupStore::open(&root.path().join("live.sqlite3")).is_ok());
}

#[test]
fn durable_requested_jobs_resume_but_cancelled_jobs_wait_for_explicit_resume() {
    for cancelled in [false, true] {
        let root = current_fixture();
        let path = root.path().join("live.sqlite3");
        let mut owner =
            maxed_store::maintenance::StoppedMaintenance::receipt_conversion(&path).unwrap();
        let id = owner.request_receipt_conversion(100).unwrap().status.id;
        if cancelled {
            owner.control(Control::Pause(id)).unwrap();
        }
        drop(owner);
        let runtime = initialized(root.path());
        if cancelled {
            eventually(|| {
                observed(&runtime, root.path(), false)
                    .maintenance
                    .is_some_and(|m| m.jobs.iter().any(|j| j.needs_resume))
            });
            assert!(runtime.list_storage_backups().unwrap().is_empty());
            let db = rusqlite::Connection::open(&path).unwrap();
            assert_eq!(
                db.query_row("SELECT count(*) FROM event_receipts", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                3
            );
            drop(db);
            runtime.resume_storage_maintenance().unwrap();
        }
        eventually(|| {
            observed(&runtime, root.path(), false)
                .maintenance
                .is_some_and(|m| {
                    m.jobs
                        .iter()
                        .any(|j| j.phase == StorageMaintenancePhase::Reclaimable)
                })
        });
        assert_eq!(runtime.list_storage_backups().unwrap().len(), 1);
        runtime.shutdown().unwrap();
    }
}

#[test]
fn stopped_lease_defers_and_pending_backup_does_not_prevent_compatible_capture() {
    let root = current_fixture();
    let runtime = initialized(root.path());
    runtime
        .shared
        .views
        .maintenance
        .set_environment(Arc::new(Space(None)));
    let lease = storage::namespace_lease(root.path(), Namespace::Live).unwrap();
    for _ in 0..3 {
        assert_eq!(
            runtime.compact_storage(),
            Ok(maxed_types::StorageCommandOutcome::Queued)
        );
    }
    assert_eq!(
        observed(&runtime, root.path(), false).failure,
        Some(StorageFailure::Busy)
    );
    drop(lease);
    eventually(|| {
        observed(&runtime, root.path(), false).failure == Some(StorageFailure::SpaceUnknown)
    });
    let db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM maintenance_jobs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(db);
    let mut adapter = runtime
        .shared
        .open_home(
            &RuntimeOptions::live(root.path().into(), None),
            &AtomicBool::new(false),
            Bridge::default(),
            None,
        )
        .unwrap();
    adapter.service_maintenance();
    adapter.close().unwrap();
    drop(adapter);
    runtime.shutdown().unwrap();
    assert!(storage::namespace_lease(root.path(), Namespace::Live).is_ok());
}

#[test]
fn retained_unsaved_events_block_units_and_draining_releases_the_block() {
    let root = current_fixture();
    let runtime = initialized(root.path());
    let mut adapter = runtime
        .shared
        .open_home(
            &RuntimeOptions::live(root.path().into(), None),
            &AtomicBool::new(false),
            Bridge::default(),
            None,
        )
        .unwrap();
    adapter.events(vec![EngineEvent::CaptureStateChanged(
        CaptureStatus::default(),
    )]);
    let writer = adapter.maintenance_writer_for_test();
    let id = writer.request_receipt_conversion(100).unwrap().status.id;
    writer
        .backup_receipt_conversion(&Space(Some(u64::MAX)), &AtomicBool::new(false), &mut || {})
        .unwrap();
    eventually(|| {
        writer
            .maintenance(Control::Status(id))
            .unwrap()
            .status
            .reason
            == Some(PauseReason::Capture)
    });
    assert_eq!(
        writer
            .maintenance(Control::Status(id))
            .unwrap()
            .status
            .processed_records,
        0
    );
    adapter.drain().unwrap();
    eventually(|| {
        adapter
            .maintenance_writer_for_test()
            .maintenance(Control::Status(id))
            .unwrap()
            .status
            .phase
            == Phase::Reclaimable
    });
    adapter.close().unwrap();
}

#[test]
fn demo_and_replay_actions_never_maintain_live_history() {
    for namespace in [Namespace::Demo, Namespace::Replay] {
        let root = current_fixture();
        let runtime = initialized(root.path());
        let mut options = RuntimeOptions::live(root.path().into(), None);
        options.namespace = namespace;
        let mut adapter = runtime
            .shared
            .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
            .unwrap();
        assert!(runtime.compact_storage().is_err());
        assert!(runtime.backup_and_upgrade_history().is_err());
        adapter.service_maintenance();
        assert!(!root.path().join("backups").exists());
        adapter.close().unwrap();
    }
}

#[test]
fn failed_shutdown_and_update_do_not_create_maintenance_work() {
    let root = current_fixture();
    let runtime = initialized(root.path());
    let mut adapter = runtime
        .shared
        .open_home(
            &RuntimeOptions::live(root.path().into(), None),
            &AtomicBool::new(false),
            Bridge::default(),
            None,
        )
        .unwrap();
    adapter.events(vec![EngineEvent::CaptureStateChanged(
        CaptureStatus::default(),
    )]);
    *runtime.shared.recovery.lock().unwrap() = Some(adapter);
    assert!(runtime.compact_storage().is_err());
    assert!(runtime.shutdown().is_err());
    assert!(runtime.prepare_update().is_err());
    assert!(!root.path().join("backups").exists());
    let db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM maintenance_jobs", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(db);
    runtime.exit_without_saving();
}

#[test]
fn blocked_unpaused_job_renews_deleted_authorization_on_explicit_resume() {
    struct Hold(
        crossbeam_channel::Sender<()>,
        crossbeam_channel::Receiver<()>,
    );
    impl BackupEnvironment for Hold {
        fn available_space(&self, _: &Path) -> Option<u64> {
            self.0.send(()).unwrap();
            self.1.recv().unwrap();
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            100
        }
    }
    let root = current_fixture();
    let path = root.path().join("live.sqlite3");
    let mut owner =
        maxed_store::maintenance::StoppedMaintenance::receipt_conversion(&path).unwrap();
    owner.request_receipt_conversion(100).unwrap();
    let backup = owner
        .backup_receipt_conversion(&Space(Some(u64::MAX)), &AtomicBool::new(false), &mut || {})
        .unwrap();
    owner.step().unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE maintenance_jobs SET safe_error='blocked_stream',paused=0",
        [],
    )
    .unwrap();
    drop(db);
    drop(owner);
    let runtime = initialized(root.path());
    eventually(|| {
        observed(&runtime, root.path(), false)
            .maintenance
            .is_some_and(|m| m.jobs.iter().any(|j| j.needs_resume && !j.paused))
    });
    assert_eq!(
        serde_json::to_value(observed(&runtime, root.path(), false).failure).unwrap(),
        "blocked_stream"
    );
    assert!(!runtime.list_storage_backups().unwrap()[0].in_use);
    runtime
        .delete_storage_backup(&backup.id.0.to_string())
        .unwrap();
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    runtime
        .shared
        .views
        .maintenance
        .set_environment(Arc::new(Hold(entered, held)));
    runtime.resume_storage_maintenance().unwrap();
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    let resumed_failure = observed(&runtime, root.path(), false).failure;
    release.send(()).unwrap();
    assert_eq!(
        serde_json::to_value(resumed_failure).unwrap(),
        "blocked_stream"
    );
    eventually(|| {
        observed(&runtime, root.path(), false)
            .maintenance
            .is_some_and(|m| {
                m.jobs.len() == 1 && m.jobs[0].phase == StorageMaintenancePhase::Reclaimable
            })
    });
    assert_ne!(
        runtime.list_storage_backups().unwrap()[0].id,
        backup.id.0.to_string()
    );
    let status = observed(&runtime, root.path(), false);
    assert_eq!(
        status.maintenance.unwrap().totals.processed_records,
        Some(StorageCount(3))
    );
}

#[test]
fn update_barrier_rejects_new_backup_work_until_released() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    installed::install(&db, 6);
    drop(db);
    let runtime = initialized(root.path());
    runtime.prepare_update().unwrap();
    assert!(runtime.compact_storage().is_err());
    assert!(matches!(
        runtime.backup_and_upgrade_with(&Space(Some(u64::MAX)), &AtomicBool::new(false)),
        Err(maintenance::HistoryUpgradeError::StoppedRequired)
    ));
    assert!(!root.path().join("backups").exists());
    assert!(storage::namespace_lease(root.path(), Namespace::Live).is_ok());
    runtime.finish_update();
    runtime
        .backup_and_upgrade_with(&Space(Some(u64::MAX)), &AtomicBool::new(false))
        .unwrap();
}

#[test]
fn native_delete_refuses_a_backup_authorizing_unfinished_work() {
    let root = current_fixture();
    let path = root.path().join("live.sqlite3");
    let mut owner =
        maxed_store::maintenance::StoppedMaintenance::receipt_conversion(&path).unwrap();
    owner.request_receipt_conversion(100).unwrap();
    let backup = owner
        .backup_receipt_conversion(&Space(Some(u64::MAX)), &AtomicBool::new(false), &mut || {})
        .unwrap();
    owner.step().unwrap();
    drop(owner);
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.register_storage_path_for_test(&path);
    assert!(runtime.list_storage_backups().unwrap()[0].in_use);
    assert_eq!(
        runtime.delete_storage_backup(&backup.id.0.to_string()),
        Err("History backup is in use".into())
    );
    assert_eq!(runtime.list_storage_backups().unwrap().len(), 1);
}

#[test]
fn stopped_completion_and_status_polls_release_chat_and_party_writes() {
    let root = current_fixture();
    let runtime = initialized(root.path());
    runtime.compact_storage().unwrap();
    eventually(|| {
        observed(&runtime, root.path(), false)
            .maintenance
            .is_some_and(|m| {
                m.jobs
                    .iter()
                    .any(|j| j.phase == StorageMaintenancePhase::Reclaimable)
            })
    });
    // Allow the completing service turn to drop its owner before public writes.
    let deadline = Instant::now() + Duration::from_secs(2);
    while storage::namespace_lease(root.path(), Namespace::Live).is_err()
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(10));
    }
    runtime.clear_chat_history().unwrap();
    runtime
        .set_party_mark(
            "synthetic".into(),
            "Synthetic".into(),
            Some(PartyMark::Watched),
            None,
        )
        .unwrap();
    let lease = storage::namespace_lease(root.path(), Namespace::Live).unwrap();
    // A status observation must work even while another stopped operation owns Live.
    assert_eq!(runtime.list_storage_backups().unwrap().len(), 1);
    assert!(observed(&runtime, root.path(), false)
        .backups
        .retained_bytes
        .is_some());
    drop(lease);
    for _ in 0..3 {
        let _ = observed(&runtime, root.path(), false);
        runtime.clear_chat_history().unwrap();
        runtime
            .set_party_mark("synthetic".into(), "Synthetic".into(), None, None)
            .unwrap();
    }
}

#[test]
fn empty_cancel_and_resume_do_not_acquire_stopped_ownership() {
    let root = current_fixture();
    let runtime = initialized(root.path());
    // Include an existing empty manifest and another namespace owner: control
    // no-ops must decide from reader state without starting a stopped writer.
    drop(BackupStore::open(&root.path().join("live.sqlite3")).unwrap());
    let lease = storage::namespace_lease(root.path(), Namespace::Live).unwrap();
    runtime.cancel_storage_maintenance().unwrap();
    runtime.resume_storage_maintenance().unwrap();
    drop(lease);
    runtime.stop().unwrap();
    runtime.clear_chat_history().unwrap();
    assert!(storage::namespace_lease(root.path(), Namespace::Live).is_ok());
    assert!(runtime.list_storage_backups().unwrap().is_empty());
}

#[test]
fn refused_upgrade_can_be_probed_with_fresh_space_and_retried() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    installed::install(&db, 6);
    drop(db);
    let runtime = initialized(root.path());
    assert!(runtime
        .backup_and_upgrade_with(&Space(Some(0)), &AtomicBool::new(false))
        .is_err());
    let blocked = runtime
        .history_upgrade_status_with(&Space(Some(0)))
        .unwrap();
    assert_eq!(blocked.failure, Some(StorageFailure::InsufficientSpace));
    let fresh = runtime
        .history_upgrade_status_with(&Space(Some(u64::MAX)))
        .unwrap();
    assert_eq!(fresh.phase, HistoryUpgradePhase::Required);
    assert_eq!(fresh.failure, None);
    assert_eq!(fresh.last_failure, Some(StorageFailure::InsufficientSpace));
    runtime
        .backup_and_upgrade_with(&Space(Some(u64::MAX)), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        runtime.history_upgrade_status().unwrap().phase,
        HistoryUpgradePhase::Ready
    );
}

#[test]
fn storage_uses_existing_reader_counters_and_estimates_only_on_explicit_idle_request() {
    let root = current_fixture();
    let runtime = initialized(root.path());
    let db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE synthetic_pages(bytes); INSERT INTO synthetic_pages VALUES(zeroblob(65536)); DELETE FROM synthetic_pages;").unwrap();
    assert_eq!(
        observed(&runtime, root.path(), false).database.page_count,
        None
    );
    let read = runtime.clone();
    let result = tauri::async_runtime::block_on(runtime.submit_history(
        history::HistoryCaller::NativeTest,
        history::HistoryKey::MaintenanceJobs,
        move |job| read.read_history(job, Vec::new, |r| r.maintenance_statuses()),
    ))
    .unwrap();
    assert!(matches!(result, HistoryReply::Ready { .. }));
    let status = observed(&runtime, root.path(), false);
    assert!(status.database.reusable_bytes.unwrap().0 > 0);
    assert!(status.database.shm_bytes.unwrap().0 > 0);
    assert_eq!(status.receipt_estimate.measured_at, None);
    let gate = runtime
        .shared
        .views
        .maintenance
        .estimate_gate
        .lock()
        .unwrap();
    let busy_estimate = observed(&runtime, root.path(), true);
    assert!(busy_estimate.database.page_count.is_some());
    assert!(busy_estimate.maintenance.is_some());
    assert_eq!(busy_estimate.receipt_estimate.measured_at, None);
    drop(gate);
    let estimated = observed(&runtime, root.path(), true);
    assert!(estimated.receipt_estimate.bytes.unwrap().0 > 0);
    assert!(estimated.receipt_estimate.measured_at.is_some());
    assert_eq!(
        observed(&runtime, root.path(), false).receipt_estimate,
        estimated.receipt_estimate
    );
    runtime.shared.home.close_idle_reader();
    assert_eq!(
        observed(&runtime, root.path(), false).database.page_count,
        None
    );
}

#[test]
fn notices_throttle_to_one_per_second_and_keep_latest_progress_after_navigation() {
    let runtime = Arc::new(CaptureRuntime::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    runtime.set_storage_notice_sink({
        let seen = seen.clone();
        Arc::new(move |notice| seen.lock().unwrap().push(notice))
    });
    let service = &runtime.shared.views.maintenance;
    let mut job = maxed_store::maintenance::JobStatus {
        id: 77,
        phase: Phase::CopyingReceipts,
        paused: false,
        reason: None,
        requested_at: 1,
        updated_at: 2,
        processed_records: 0,
        processed_bytes: 0,
        estimates: Default::default(),
    };
    let start = Instant::now();
    service.observe(vec![job.clone()]);
    service.pump(start);
    for count in 1..=100 {
        job.processed_records = count;
        service.observe(vec![job.clone()]);
        service.pump(start + Duration::from_millis(999));
    }
    assert_eq!(seen.lock().unwrap().len(), 1);
    service.pump(start + Duration::from_secs(1));
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[1]
            .maintenance
            .as_ref()
            .unwrap()
            .totals
            .processed_records,
        Some(StorageCount(100))
    );
    let json = serde_json::to_value(&seen[1]).unwrap();
    assert!(json.get("id").is_none());
    assert!(json["maintenance"]["jobs"][0].get("id").is_none());
    assert_eq!(json["maintenance"]["jobs"][0]["phase"], "copying_receipts");
}

#[test]
fn app_close_cancels_upgrade_repeated_clicks_join_and_capture_is_refused() {
    struct Hold(
        crossbeam_channel::Sender<()>,
        crossbeam_channel::Receiver<()>,
        AtomicBool,
    );
    impl BackupEnvironment for Hold {
        fn available_space(&self, _: &Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            100
        }
        fn boundary(&self, at: maxed_store::backup::Boundary) -> Result<(), BackupError> {
            if at == maxed_store::backup::Boundary::Copy && !self.2.swap(true, Ordering::AcqRel) {
                self.0.send(()).unwrap();
                self.1.recv().unwrap();
            }
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    installed::install(&db, 6);
    drop(db);
    let runtime = initialized(root.path());
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    let worker = std::thread::spawn({
        let runtime = runtime.clone();
        move || {
            runtime.backup_and_upgrade_with(
                &Hold(entered, held, AtomicBool::new(false)),
                &AtomicBool::new(false),
            )
        }
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        runtime.history_upgrade_status().unwrap().phase,
        HistoryUpgradePhase::BackingUp
    );
    runtime
        .backup_and_upgrade_with(&Space(Some(u64::MAX)), &AtomicBool::new(false))
        .unwrap();
    assert!(runtime
        .launch(
            Arc::new(|_| {}),
            Bridge::default(),
            RuntimeOptions::live(root.path().into(), None),
            || panic!("capture factory must not run during upgrade")
        )
        .is_err());
    let closing = std::thread::spawn({
        let runtime = runtime.clone();
        move || runtime.shutdown()
    });
    eventually(|| runtime.closing.load(Ordering::Acquire));
    release.send(()).unwrap();
    assert!(matches!(
        worker.join().unwrap(),
        Err(maintenance::HistoryUpgradeError::Backup(
            BackupError::Cancelled
        ))
    ));
    closing.join().unwrap().unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(installed::user_version(&db), 6);
    assert!(storage::namespace_lease(root.path(), Namespace::Live).is_ok());
}

pub(super) struct Space(pub Option<u64>);

#[test]
fn upgrade_copy_does_not_hold_the_session_mutex() {
    use maxed_store::backup::Boundary;
    struct Hold {
        entered: crossbeam_channel::Sender<()>,
        release: crossbeam_channel::Receiver<()>,
        once: AtomicBool,
    }
    impl BackupEnvironment for Hold {
        fn available_space(&self, _: &Path) -> Option<u64> {
            Some(u64::MAX)
        }
        fn timestamp(&self) -> u64 {
            100
        }
        fn boundary(&self, at: Boundary) -> Result<(), BackupError> {
            if at == Boundary::Copy && !self.once.swap(true, Ordering::AcqRel) {
                self.entered.send(()).unwrap();
                self.release.recv().unwrap();
            }
            Ok(())
        }
    }
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    installed::install(&db, 6);
    drop(db);
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.wait_history_preparation());
    let (entered, waiting) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    let worker = std::thread::spawn({
        let runtime = runtime.clone();
        move || {
            runtime.backup_and_upgrade_with(
                &Hold {
                    entered,
                    release: held,
                    once: AtomicBool::new(false),
                },
                &AtomicBool::new(false),
            )
        }
    });
    waiting.recv_timeout(Duration::from_secs(10)).unwrap();
    let (reply, received) = crossbeam_channel::bounded(1);
    let query = std::thread::spawn({
        let runtime = runtime.clone();
        move || reply.send(runtime.is_running()).unwrap()
    });
    let responsive = received.recv_timeout(Duration::from_millis(200));
    release.send(()).unwrap();
    query.join().unwrap();
    worker.join().unwrap().unwrap();
    assert_eq!(
        responsive,
        Ok(false),
        "status must remain responsive throughout backup"
    );
}

impl BackupEnvironment for Space {
    fn available_space(&self, _: &Path) -> Option<u64> {
        self.0
    }
    fn timestamp(&self) -> u64 {
        100
    }
}

#[allow(dead_code)]
#[path = "../../../../crates/maxed-store/tests/common/installed_schema.rs"]
mod installed;

#[test]
fn legacy_chat_clear_and_party_writes_refuse_without_changing_history() {
    for version in [2, 6, 10] {
        for party in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("live.sqlite3");
            let db = rusqlite::Connection::open(&path).unwrap();
            installed::install(&db, version);
            drop(db);
            let before = std::fs::read(&path).unwrap();
            let runtime = Arc::new(CaptureRuntime::default());
            runtime.initialize_home(root.path().into()).unwrap();
            assert!(tauri::async_runtime::block_on(runtime.ensure_history_ready()).is_err());
            let result = if party {
                runtime.set_party_mark("synthetic-key".into(), "Synthetic".into(), None, None)
            } else {
                runtime.clear_chat_history()
            };
            assert!(result.is_err(), "legacy mutation must be refused");
            assert!(std::fs::read(&path).unwrap() == before);
            let db = rusqlite::Connection::open(&path).unwrap();
            assert_eq!(installed::user_version(&db), version as i64);
            assert!(!root.path().join("backups").exists());
        }
    }
}

#[test]
fn maintenance_tests_legacy_history_requires_confirmation_at_both_entry_points() {
    for version in [2, 6, 10] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("live.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        installed::install(&db, version);
        drop(db);
        assert!(matches!(
            maxed_store::upgrade_history(&path),
            Err(maxed_store::StoreError::HistoryUpgradeRequired)
        ));
        assert!(matches!(
            maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()),
            Err(maxed_store::StoreError::HistoryUpgradeRequired)
        ));
        let runtime = Arc::new(CaptureRuntime::default());
        runtime.initialize_home(root.path().into()).unwrap();
        let result = tauri::async_runtime::block_on(runtime.ensure_history_ready());
        assert_eq!(result, Err(home::HISTORY_UPGRADE_REQUIRED.into()));
        let options = RuntimeOptions::live(root.path().into(), None);
        let result = options.open(
            &AtomicBool::new(false),
            Arc::new(Views::default()),
            Bridge::default(),
        );
        assert!(matches!(result, Err(ref error) if error == home::HISTORY_UPGRADE_REQUIRED));
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(installed::user_version(&db), version as i64);
        assert!(!root.path().join("backups").exists());
    }
}

#[test]
fn confirmed_exact_legacy_upgrade_keeps_initial_and_later_conversion_uses_rolling() {
    for version in [2, 6, 10] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("live.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        installed::install(&db, version);
        db.execute_batch("INSERT INTO play_sessions(id,started,last_traffic,ended,json) VALUES('synthetic-session',1,2,2,'{}');
            INSERT INTO drops(id,session_id,at,bag_type,json) VALUES('synthetic-drop','synthetic-session',1,1280,'{}');
            INSERT INTO drop_items(drop_id,slot,item_type,quantity,json) VALUES('synthetic-drop',0,1,1,'{}');").unwrap();
        drop(db);
        let runtime = Arc::new(CaptureRuntime::default());
        runtime.initialize_home(root.path().into()).unwrap();
        assert!(tauri::async_runtime::block_on(runtime.ensure_history_ready()).is_err());
        runtime
            .backup_and_upgrade_with(&Space(Some(u64::MAX)), &AtomicBool::new(false))
            .unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(installed::user_version(&db), maxed_store::SCHEMA_VERSION);
        assert_eq!(
            db.query_row("SELECT count(*) FROM drops", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(tauri::async_runtime::block_on(runtime.ensure_history_ready()).is_ok());
        let mut backups = BackupStore::open(&path).unwrap();
        let initial = backups.list().unwrap();
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].schema, version as i64);
        assert_eq!(initial[0].slot, Slot::Initial);
        let copy = rusqlite::Connection::open_with_flags(
            root.path()
                .join("backups/live")
                .join(format!("backup-{:020}.sqlite3", initial[0].id.0)),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert_eq!(installed::user_version(&copy), version as i64);
        assert_eq!(
            copy.query_row("SELECT count(*) FROM drops", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            copy.query_row("SELECT count(*) FROM drop_items", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        drop(copy);
        let request = backups
            .request(DestructiveClass::ReceiptConversion, 101)
            .unwrap();
        assert!(backups.prepare_authorization(request).is_err());
        let rolling = backups
            .create(
                &db,
                request,
                &Space(Some(u64::MAX)),
                &AtomicBool::new(false),
                &mut || {},
            )
            .unwrap();
        assert_eq!(rolling.slot, Slot::Rolling);
        assert_eq!(backups.list().unwrap().len(), 2);
        drop(backups);
        drop(db);
        let mut adapter = RuntimeOptions::live(root.path().into(), None)
            .open(
                &AtomicBool::new(false),
                Arc::new(Views::default()),
                Bridge::default(),
            )
            .unwrap();
        adapter.close().unwrap();
    }
}

#[test]
fn native_compatible_versions_and_empty_zero_bypass_backup_at_both_entry_points() {
    for version in [0, 11, 12, 14, 15] {
        for writer in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("live.sqlite3");
            let db = rusqlite::Connection::open(&path).unwrap();
            if version != 0 {
                installed::install(&db, version);
            }
            drop(db);
            if writer {
                let mut adapter = RuntimeOptions::live(root.path().into(), None)
                    .open(
                        &AtomicBool::new(false),
                        Arc::new(Views::default()),
                        Bridge::default(),
                    )
                    .unwrap();
                adapter.close().unwrap();
            } else {
                let runtime = Arc::new(CaptureRuntime::default());
                runtime.initialize_home(root.path().into()).unwrap();
                tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
            }
            let db = rusqlite::Connection::open(&path).unwrap();
            assert_eq!(installed::user_version(&db), maxed_store::SCHEMA_VERSION);
            assert!(!root.path().join("backups").exists());
        }
    }
}

#[test]
fn native_unrecognized_zero_and_tampered_checksums_fail_closed() {
    for tampered in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("live.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        if tampered {
            installed::install(&db, 6);
            db.execute(
                "UPDATE schema_migrations SET checksum='invalid' WHERE version=6",
                [],
            )
            .unwrap();
        } else {
            db.execute_batch("CREATE TABLE unknown_history(value TEXT)")
                .unwrap();
        }
        drop(db);
        let original = std::fs::read(&path).unwrap();
        assert!(matches!(
            maxed_store::upgrade_history(&path),
            Err(maxed_store::StoreError::Migration)
        ));
        assert!(matches!(
            maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()),
            Err(maxed_store::StoreError::Migration)
        ));
        assert!(std::fs::read(&path).unwrap() == original);
    }
}

#[test]
fn confirmed_upgrade_unknown_insufficient_cancelled_and_busy_preserve_legacy() {
    for reason in ["unknown", "insufficient", "cancelled", "busy"] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("live.sqlite3");
        let db = rusqlite::Connection::open(&path).unwrap();
        installed::install(&db, 6);
        drop(db);
        let original = std::fs::read(&path).unwrap();
        let runtime = Arc::new(CaptureRuntime::default());
        runtime.initialize_home(root.path().into()).unwrap();
        assert!(tauri::async_runtime::block_on(runtime.ensure_history_ready()).is_err());
        let lease = if reason == "busy" {
            Some(storage::namespace_lease(root.path(), Namespace::Live).unwrap())
        } else {
            None
        };
        let space = match reason {
            "unknown" => None,
            "insufficient" => Some(0),
            _ => Some(u64::MAX),
        };
        let result =
            runtime.backup_and_upgrade_with(&Space(space), &AtomicBool::new(reason == "cancelled"));
        match (reason, result) {
            (
                "unknown",
                Err(maintenance::HistoryUpgradeError::Backup(BackupError::SpaceUnknown)),
            )
            | (
                "insufficient",
                Err(maintenance::HistoryUpgradeError::Backup(BackupError::InsufficientSpace)),
            )
            | (
                "cancelled",
                Err(maintenance::HistoryUpgradeError::Backup(BackupError::Cancelled)),
            )
            | ("busy", Err(maintenance::HistoryUpgradeError::StoppedRequired)) => (),
            (_, result) => panic!("unexpected safe result {result:?}"),
        }
        assert!(std::fs::read(&path).unwrap() == original);
        drop(lease);
        assert!(matches!(
            maxed_store::upgrade_history(&path),
            Err(maxed_store::StoreError::HistoryUpgradeRequired)
        ));
        runtime
            .backup_and_upgrade_with(&Space(Some(u64::MAX)), &AtomicBool::new(false))
            .unwrap();
    }
}

#[test]
fn native_available_space_uses_the_backup_directory_volume() {
    let root = tempfile::tempdir().unwrap();
    let long = root
        .path()
        .join("synthetic-long-directory".repeat(8))
        .join("synthetic-child".repeat(8));
    std::fs::create_dir_all(&long).unwrap();
    let canonical = std::fs::canonicalize(&long).unwrap();
    assert!(canonical.as_os_str().len() > 260);
    assert!(maintenance::SystemBackupEnvironment
        .available_space(&canonical)
        .is_some_and(|bytes| bytes > 0));
    assert_eq!(
        maintenance::SystemBackupEnvironment.available_space(Path::new(
            r"\\?\Volume{00000000-0000-0000-0000-000000000000}\"
        )),
        None
    );
}

#[test]
fn retry_rechecks_space_for_a_previously_verified_legacy_backup() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    installed::install(&db, 6);
    let mut backups = BackupStore::open(&path).unwrap();
    let request = backups
        .request(DestructiveClass::LegacyUpgrade, 100)
        .unwrap();
    backups
        .create(
            &db,
            request,
            &Space(Some(u64::MAX)),
            &AtomicBool::new(false),
            &mut || {},
        )
        .unwrap();
    drop(backups);
    drop(db);
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    assert!(tauri::async_runtime::block_on(runtime.ensure_history_ready()).is_err());
    assert!(matches!(
        runtime.backup_and_upgrade_with(&Space(None), &AtomicBool::new(false)),
        Err(maintenance::HistoryUpgradeError::Backup(
            BackupError::SpaceUnknown
        ))
    ));
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(installed::user_version(&db), 6);
    drop(db);
    runtime
        .backup_and_upgrade_with(&Space(Some(u64::MAX)), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(BackupStore::open(&path).unwrap().list().unwrap().len(), 1);
}
