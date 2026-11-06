//! Included by runtime::maintenance as unit tests exercising the actual owners.
use super::super::{
    maintenance_tests::{current_fixture, initialized, Space},
    Adapter, Bridge, RuntimeOptions, Session,
};
use super::*;

fn fixture(resume: bool) -> (tempfile::TempDir, Arc<CaptureRuntime>, rusqlite::Connection) {
    let root = current_fixture();
    let path = root.path().join("live.sqlite3");
    let jobs = if resume {
        let mut owner = StoppedMaintenance::receipt_conversion(&path).unwrap();
        let id = owner.request_receipt_conversion(100).unwrap().status.id;
        vec![owner.control(Control::Pause(id)).unwrap().status]
    } else {
        drop(maxed_store::backup::BackupStore::open(&path).unwrap());
        vec![]
    };
    let runtime = initialized(root.path());
    let service = &runtime.shared.views.maintenance;
    service.observe(jobs);
    service.set_environment(Arc::new(Space(None)));
    let manifest =
        rusqlite::Connection::open(root.path().join("backups/live/manifest.sqlite3")).unwrap();
    manifest.execute_batch("BEGIN IMMEDIATE").unwrap();
    (root, runtime, manifest)
}

fn open(runtime: &CaptureRuntime, root: &Path, namespace: Namespace) -> Adapter {
    let mut options = RuntimeOptions::live(root.into(), None);
    options.namespace = namespace;
    runtime
        .shared
        .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
        .unwrap()
}

fn assert_persisted(root: &Path, paused: bool) {
    let db = rusqlite::Connection::open(root.join("live.sqlite3")).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM maintenance_jobs WHERE phase='awaiting_backup' AND paused=?1",
            [paused],
            |row| row.get::<_, i64>(0),
        )
        .unwrap(),
        1
    );
}

#[test]
fn queued_live_request_survives_stop_and_restarts_without_a_runnable_job() {
    for resume in [false, true] {
        let (root, runtime, manifest) = fixture(resume);
        let service = &runtime.shared.views.maintenance;
        let mut adapter = open(&runtime, root.path(), Namespace::Live);
        let (send, receive) = crossbeam_channel::bounded(1);
        service.enqueue(
            if resume {
                Operation::Resume
            } else {
                Operation::Compact
            },
            Some(send),
        );
        adapter.service_maintenance();
        assert!(matches!(receive.try_recv(), Ok(Ok(Reply::Queued))));
        service.enqueue(Operation::Cancel, None);

        // Model the joined capture worker's completed adapter. Stop itself owns
        // closing the writer and transferring its pending maintenance requests.
        *runtime.shared.completed.lock().unwrap() = Some(adapter);
        let (stop, _) = crossbeam_channel::bounded(1);
        *runtime.session.lock().unwrap() = Some(Session {
            stop,
            cancel: Arc::new(AtomicBool::new(false)),
            worker: std::thread::spawn(|| {}),
            sink: Arc::new(|_| {}),
        });
        // Prevent the new stopped owner from consuming requests before the
        // assertion. try_lock admission makes handoff/join safe under this gate.
        let gate = service.estimate_gate.lock().unwrap();
        runtime.stop().unwrap();
        assert!(service.has_background(), "Stop must restart volatile work");
        assert!(matches!(
            service
                .state
                .lock()
                .unwrap()
                .requests
                .front()
                .map(|r| &r.operation),
            Some(Operation::Compact | Operation::Resume)
        ));
        service.handoff_background();
        drop(gate);
        manifest.execute_batch("ROLLBACK").unwrap();
        drop(manifest);

        let mut adapter = open(&runtime, root.path(), Namespace::Live);
        adapter.service_maintenance();
        assert_persisted(root.path(), false);
        adapter.service_maintenance();
        assert_persisted(root.path(), true);
        adapter.close().unwrap();
        drop(adapter);
        runtime.shutdown().unwrap();
    }
}

#[test]
fn queued_stopped_request_waits_through_demo_and_replay_for_live_owner() {
    for namespace in [Namespace::Demo, Namespace::Replay] {
        for resume in [false, true] {
            let (root, runtime, manifest) = fixture(resume);
            let service = &runtime.shared.views.maintenance;
            let result = if resume {
                runtime.resume_storage_maintenance()
            } else {
                runtime.compact_storage()
            };
            assert_eq!(result, Ok(StorageCommandOutcome::Queued));
            service.enqueue(Operation::Cancel, None);
            let mut playback = open(&runtime, root.path(), namespace);
            manifest.execute_batch("ROLLBACK").unwrap();
            drop(manifest);
            playback.service_maintenance();
            runtime.resume_cached_storage();
            assert!(!service.has_background());
            assert!(service.halted.load(Ordering::Acquire));
            assert_eq!(service.state.lock().unwrap().requests.len(), 2);
            let db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM maintenance_jobs WHERE paused=0",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0,
                "playback must not run pending live maintenance"
            );
            drop(db);
            playback.close().unwrap();
            drop(playback);
            let mut live = open(&runtime, root.path(), Namespace::Live);
            live.service_maintenance();
            assert_persisted(root.path(), false);
            live.service_maintenance();
            assert_persisted(root.path(), true);
            live.close().unwrap();
            drop(live);
            runtime.shutdown().unwrap();
        }
    }
}

#[test]
fn leaving_preview_restarts_queued_stopped_work_and_preserves_cancel() {
    for namespace in [Namespace::Demo, Namespace::Replay] {
        let (root, runtime, manifest) = fixture(false);
        let service = &runtime.shared.views.maintenance;
        assert_eq!(runtime.compact_storage(), Ok(StorageCommandOutcome::Queued));
        service.enqueue(Operation::Cancel, None);
        let mut playback = open(&runtime, root.path(), namespace);
        playback.close().unwrap();
        drop(playback);
        runtime.stop().unwrap();
        assert!(!service.has_background());
        assert_eq!(service.state.lock().unwrap().requests.len(), 2);
        manifest.execute_batch("ROLLBACK").unwrap();
        drop(manifest);

        // Keep the new owner from consuming the FIFO until its admission has
        // been asserted. Release the gate, then join its natural idle exit;
        // neither assertion depends on backup/fsync speed or a polling deadline.
        let gate = service.estimate_gate.lock().unwrap();
        runtime.reset_preview().unwrap();
        assert!(
            service.has_background(),
            "leaving preview must start an owner"
        );
        assert_eq!(service.state.lock().unwrap().requests.len(), 2);
        drop(gate);
        service
            .worker
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .join()
            .unwrap();
        assert_persisted(root.path(), true);
        assert!(service.state.lock().unwrap().requests.is_empty());
        runtime.shutdown().unwrap();
    }
}

#[test]
fn playback_handoff_answers_waiters_without_losing_queued_controls() {
    for namespace in [Namespace::Demo, Namespace::Replay] {
        let root = current_fixture();
        let runtime = initialized(root.path());
        let service = &runtime.shared.views.maintenance;
        let (compact, compact_reply) = crossbeam_channel::bounded(1);
        service.enqueue(Operation::Compact, Some(compact));
        // A duplicate has already received Queued even though the original
        // caller is still waiting. Removing the original would lose its work.
        let (duplicate, duplicate_reply) = crossbeam_channel::bounded(1);
        service.enqueue(Operation::Compact, Some(duplicate));
        assert!(matches!(duplicate_reply.try_recv(), Ok(Ok(Reply::Queued))));
        let (cancel, cancel_reply) = crossbeam_channel::bounded(1);
        service.enqueue(Operation::Cancel, Some(cancel));
        let (resume, resume_reply) = crossbeam_channel::bounded(1);
        service.enqueue(Operation::Resume, Some(resume));
        let (delete, delete_reply) = crossbeam_channel::bounded(1);
        service.enqueue(Operation::Delete(BackupId(999)), Some(delete));

        let mut playback = open(&runtime, root.path(), namespace);
        for reply in [compact_reply, cancel_reply, resume_reply] {
            assert!(matches!(reply.try_recv(), Ok(Ok(Reply::Queued))));
        }
        assert!(matches!(delete_reply.try_recv(), Ok(Err(_))));
        playback.close().unwrap();
        drop(playback);
        runtime.stop().unwrap();
        {
            let state = service.state.lock().unwrap();
            assert_eq!(state.requests.len(), 3);
            assert!(matches!(state.requests[0].operation, Operation::Compact));
            assert!(matches!(state.requests[1].operation, Operation::Cancel));
            assert!(matches!(state.requests[2].operation, Operation::Resume));
            assert_eq!(state.failure, None);
        }
        runtime.shutdown().unwrap();
    }
}

#[test]
fn rejecting_internal_restores_does_not_create_or_replace_a_failure() {
    for previous in [None, Some(StorageFailure::Full)] {
        let service = StorageService::default();
        service.fail(previous);
        service.enqueue(Operation::Restore(vec![]), None);
        service.stop_background();
        assert!(service.state.lock().unwrap().requests.is_empty());
        assert_eq!(service.state.lock().unwrap().failure, previous);
    }
}

#[test]
fn rejected_queued_request_publishes_failure_even_after_a_recent_notice() {
    for terminal in ["shutdown", "upgrade", "unavailable"] {
        let root = current_fixture();
        let runtime = initialized(root.path());
        let service = &runtime.shared.views.maintenance;
        let notices = Arc::new(Mutex::new(Vec::new()));
        runtime.set_storage_notice_sink({
            let notices = notices.clone();
            Arc::new(move |notice| notices.lock().unwrap().push(notice))
        });
        // Freeze the throttle at the first publication; no wall-clock delay can
        // hide a terminal notice which was marked pending but never delivered.
        let now = Instant::now();
        *service.clock.lock().unwrap() = Some(Arc::new(move || now));
        service.pump(now);
        service.enqueue(Operation::Compact, None);
        let (send, receive) = crossbeam_channel::bounded(1);
        service.enqueue(Operation::Compact, Some(send));
        assert!(matches!(receive.try_recv(), Ok(Ok(Reply::Queued))));
        match terminal {
            "upgrade" => runtime
                .backup_and_upgrade_with(&Space(None), &AtomicBool::new(false))
                .unwrap(),
            "unavailable" => service.run_stopped(root.path().join("absent.sqlite3")),
            _ => runtime.shutdown().unwrap(),
        }
        assert!(service.state.lock().unwrap().requests.is_empty());
        let notices = notices.lock().unwrap();
        assert_eq!(notices.len(), 2);
        assert_eq!(notices[1].failure, Some(StorageFailure::Unavailable));
    }
}
