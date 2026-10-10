use super::*;
use maxed_types::{HistoryReply, HomeScope, HomeSummary, TimeRange};

fn home(runtime: &Arc<CaptureRuntime>, scope: HomeScope) -> Result<HomeSummary, String> {
    match tauri::async_runtime::block_on(runtime.home_summary(scope))? {
        HistoryReply::Ready { value } => Ok(value),
        other => panic!("expected ready history, got {other:?}"),
    }
}

fn today() -> HomeScope {
    HomeScope::Today {
        range: TimeRange {
            start: Timestamp(0),
            end: Timestamp(10_000_000),
        },
    }
}

fn seed(path: &std::path::Path) {
    use maxed_types::{demo, EngineEvent, EventEnvelope, Sequence};
    let mut db = rusqlite::Connection::open(path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let snapshot = demo::demo_snapshot();
    let events: Vec<_> = [
        EngineEvent::SessionUpdated(snapshot.session.unwrap()),
        EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
        EngineEvent::RunStarted(snapshot.run.unwrap()),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, event)| EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "invented-home-test".into(),
        sequence: Sequence(Timestamp(i as u64 + 1)),
        at: Timestamp(1_000_000),
        event,
    })
    .collect();
    maxed_store::apply_batch(&mut db, &events).unwrap();
}

fn open(runtime: &CaptureRuntime, root: &std::path::Path, namespace: Namespace) -> Adapter {
    let mut options = RuntimeOptions::live(root.into(), None);
    options.namespace = namespace;
    runtime
        .shared
        .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
        .unwrap()
}

fn pause_reader(runtime: &CaptureRuntime) -> (Receiver<()>, Sender<()>) {
    let (entered, wait_entered) = bounded(1);
    let (release, wait_release) = bounded(1);
    *runtime.shared.home.on_reader.lock().unwrap() = Some(Arc::new(move || {
        entered.send(()).unwrap();
        wait_release.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    (wait_entered, release)
}

fn wait_for_invalidation(runtime: &CaptureRuntime) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.shared.home.context.lock().unwrap().is_some() {
        assert!(
            Instant::now() < deadline,
            "transition did not invalidate Home"
        );
        thread::yield_now();
    }
}

#[test]
fn namespace_switch_rejects_inflight_home_read() {
    let root = tempfile::tempdir().unwrap();
    seed(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    open(&runtime, root.path(), Namespace::Demo)
        .close()
        .unwrap();
    let (entered, release) = pause_reader(&runtime);
    let reading = runtime.clone();
    let read = thread::spawn(move || tauri::async_runtime::block_on(reading.home_summary(today())));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let switching = runtime.clone();
    let reset = thread::spawn(move || switching.reset_preview());
    wait_for_invalidation(&runtime);
    assert!(
        !reset.is_finished(),
        "reset must wait for the active reader"
    );
    release.send(()).unwrap();
    assert_eq!(read.join().unwrap().unwrap(), HistoryReply::Superseded);
    reset.join().unwrap().unwrap();
    *runtime.shared.home.on_reader.lock().unwrap() = None;
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
}

#[test]
fn reader_does_not_hold_capture_lock() {
    let root = tempfile::tempdir().unwrap();
    seed(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let (entered, release) = pause_reader(&runtime);
    let reading = runtime.clone();
    let read = thread::spawn(move || tauri::async_runtime::block_on(reading.home_summary(today())));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(runtime.session.try_lock().is_ok());
    assert!(runtime.shared.recovery.try_lock().is_ok());
    assert!(runtime.shared.completed.try_lock().is_ok());
    assert!(runtime.shared.views.snapshot.try_lock().is_ok());
    assert!(runtime.shared.views.store.try_lock().is_ok());
    // Stop and capture publication remain usable while the reader is paused.
    runtime.stop().unwrap();
    let sink: StatusSink = Arc::new(|_| {});
    runtime.shared.publish(CaptureStatus::default(), &sink);
    release.send(()).unwrap();
    assert!(
        matches!(read.join().unwrap().unwrap(), HistoryReply::Ready { value } if value.runs == 1)
    );
}

#[test]
fn playback_replacement_waits_for_reader_and_rejects_same_namespace_generation() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    open(&runtime, root.path(), Namespace::Replay)
        .close()
        .unwrap();
    let database = root.path().join("replay.sqlite3");
    seed(&database);
    let first = runtime.shared.home.context.lock().unwrap().clone().unwrap();
    let (entered, release) = pause_reader(&runtime);
    let reading = runtime.clone();
    let read = thread::spawn(move || tauri::async_runtime::block_on(reading.home_summary(today())));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let switching = runtime.clone();
    let app_data = root.path().to_path_buf();
    let replace = thread::spawn(move || open(&switching, &app_data, Namespace::Replay).close());
    wait_for_invalidation(&runtime);
    assert!(!replace.is_finished());
    assert_eq!(
        maxed_store::open_reader(&database)
            .unwrap()
            .home_summary(today())
            .unwrap()
            .runs,
        1
    );
    release.send(()).unwrap();
    assert_eq!(read.join().unwrap().unwrap(), HistoryReply::Superseded);
    replace.join().unwrap().unwrap();
    *runtime.shared.home.on_reader.lock().unwrap() = None;
    let second = runtime.shared.home.context.lock().unwrap().clone().unwrap();
    assert!(second.generation > first.generation);
    assert_ne!(second.stream_id, first.stream_id);
    assert_eq!(home(&runtime, today()).unwrap().runs, 0);
}

#[test]
fn home_read_after_stop() {
    let root = tempfile::tempdir().unwrap();
    seed(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime
        .launch(
            Arc::new(|_| {}),
            Bridge::default(),
            RuntimeOptions::live(root.path().into(), None),
            || Err(CaptureError::Missing),
        )
        .unwrap();
    runtime.stop().unwrap();
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
    assert_eq!(
        runtime
            .shared
            .home
            .context
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .namespace,
        Namespace::Live
    );
}

#[test]
fn scope_validation_runs_before_io_and_releases_admission() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    for scope in [
        HomeScope::Session { id: String::new() },
        HomeScope::Session {
            id: "\u{e9}".repeat(129),
        },
        HomeScope::Today {
            range: TimeRange {
                start: Timestamp(5),
                end: Timestamp(5),
            },
        },
        HomeScope::Today {
            range: TimeRange {
                start: Timestamp(6),
                end: Timestamp(5),
            },
        },
        HomeScope::Today {
            range: TimeRange {
                start: Timestamp(0),
                end: Timestamp(u64::MAX),
            },
        },
    ] {
        assert_eq!(
            home(&runtime, scope).unwrap_err(),
            "Home history scope is invalid; select a session or day again"
        );
    }
    assert_eq!(
        home(
            &runtime,
            HomeScope::Session {
                id: "a".repeat(256)
            }
        )
        .unwrap()
        .runs,
        0
    );
    assert_eq!(home(&runtime, today()).unwrap().fame_delta, None);
    assert!(
        !root.path().join("live.sqlite3").exists(),
        "reads cannot create a database"
    );
}

#[test]
fn unavailable_and_missing_playback_never_fall_back_to_live() {
    let root = tempfile::tempdir().unwrap();
    seed(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    // Deletes between reads; close each reader so Windows releases the database.
    *runtime.shared.home.reader_idle.lock().unwrap() = Duration::ZERO;
    assert_eq!(home(&runtime, today()).unwrap_err(), home::UNAVAILABLE);
    for namespace in [Namespace::Demo, Namespace::Replay] {
        open(&runtime, root.path(), namespace).close().unwrap();
        assert_eq!(home(&runtime, today()).unwrap().runs, 0);
        std::fs::remove_file(root.path().join(format!("{}.sqlite3", namespace.label()))).unwrap();
        assert_eq!(home(&runtime, today()).unwrap_err(), home::READ_FAILED);
    }
}

#[test]
fn read_failure_preserves_failed_writer_status() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    std::fs::write(
        root.path().join("live.sqlite3"),
        b"private malformed database",
    )
    .unwrap();
    let before = maxed_types::StoreStatus {
        state: maxed_types::StoreState::Failed,
        queued: 3,
        durable: None,
        stream_id: Some("invented-stream".into()),
        message: "Retained changes".into(),
    };
    *runtime.shared.views.store.lock().unwrap() = before.clone();
    assert_eq!(home(&runtime, today()).unwrap_err(), home::SCHEMA_FAILED);
    assert_eq!(runtime.store_status(), before);
}

#[test]
fn generation_exhaustion_invalidates_without_wrapping() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    runtime
        .shared
        .home
        .generation
        .store(u64::MAX, Ordering::Release);
    assert_eq!(
        runtime.shared.home.invalidate_and_wait().unwrap_err(),
        home::UNAVAILABLE
    );
    assert!(runtime.shared.home.context.lock().unwrap().is_none());
    assert_eq!(
        runtime.shared.home.generation.load(Ordering::Acquire),
        u64::MAX
    );
}

#[test]
fn unreadable_existing_database_is_not_empty_history() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("live.sqlite3");
    seed(&database);
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&database)
        .unwrap();
    assert_eq!(home(&runtime, today()).unwrap_err(), home::READ_FAILED);
    drop(locked);
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
}

#[test]
fn sql_and_stored_json_errors_are_sanitized_and_release_the_lease() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("live.sqlite3");
    seed(&database);
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let db = rusqlite::Connection::open(&database).unwrap();
    db.execute("UPDATE runs SET json='private-invalid-json'", [])
        .unwrap();
    assert_eq!(home(&runtime, today()).unwrap_err(), home::READ_FAILED);
    db.execute("DROP TABLE fame_samples", []).unwrap();
    assert_eq!(home(&runtime, today()).unwrap_err(), home::READ_FAILED);
}

#[test]
fn failed_playback_open_retires_previous_context() {
    let root = tempfile::tempdir().unwrap();
    seed(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    std::fs::create_dir(root.path().join("demo.sqlite3")).unwrap();
    let mut options = RuntimeOptions::live(root.path().into(), None);
    options.namespace = Namespace::Demo;
    assert!(runtime
        .shared
        .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
        .is_err());
    assert_eq!(home(&runtime, today()).unwrap_err(), home::UNAVAILABLE);
    runtime.reset_preview().unwrap();
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
}

#[test]
fn playback_with_another_root_preserves_remembered_live_history() {
    let live_root = tempfile::tempdir().unwrap();
    let playback_root = tempfile::tempdir().unwrap();
    let live_database = live_root.path().join("live.sqlite3");
    seed(&live_database);
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(live_root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let expected = home(&runtime, today()).unwrap();
    assert_eq!(expected.runs, 1);

    for namespace in [Namespace::Demo, Namespace::Replay] {
        open(&runtime, playback_root.path(), namespace)
            .close()
            .unwrap();
        assert_eq!(home(&runtime, today()).unwrap().runs, 0);
        runtime.reset_preview().unwrap();
        assert_eq!(home(&runtime, today()).unwrap(), expected);
        let context = runtime.shared.home.context.lock().unwrap().clone().unwrap();
        assert_eq!(context.database, live_database);
        assert_eq!(context.namespace, Namespace::Live);
    }
    assert!(!playback_root.path().join("live.sqlite3").exists());
}

#[test]
fn failed_live_open_restores_readable_history_with_a_fresh_generation() {
    let root = tempfile::tempdir().unwrap();
    seed(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    open(&runtime, root.path(), Namespace::Live)
        .close()
        .unwrap();
    let expected = home(&runtime, today()).unwrap();
    assert_eq!(expected.runs, 1);
    let previous = runtime.shared.home.context.lock().unwrap().clone().unwrap();
    assert!(previous.stream_id.is_some());
    std::fs::write(root.path().join("identity.key"), b"invalid").unwrap();
    let options = RuntimeOptions::live(root.path().into(), None);
    let error = runtime
        .shared
        .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
        .err()
        .expect("invalid identity key must reject capture startup");
    assert_eq!(error, "Local identity key is invalid");
    assert_eq!(home(&runtime, today()).unwrap(), expected);
    let restored = runtime.shared.home.context.lock().unwrap().clone().unwrap();
    assert_eq!(restored.database, previous.database);
    assert_eq!(restored.namespace, Namespace::Live);
    assert_eq!(restored.generation, previous.generation + 1);
    assert_eq!(restored.stream_id, None);
}

#[test]
fn playback_discard_waits_for_reader_before_deleting() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let mut adapter = open(&runtime, root.path(), Namespace::Demo);
    adapter.writer_flush_for_test();
    let (entered, release) = pause_reader(&runtime);
    let reading = runtime.clone();
    let read = thread::spawn(move || tauri::async_runtime::block_on(reading.home_summary(today())));
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let discard = thread::spawn(move || adapter.discard());
    wait_for_invalidation(&runtime);
    assert!(root.path().join("demo.sqlite3").exists());
    assert!(!discard.is_finished());
    release.send(()).unwrap();
    assert_eq!(read.join().unwrap().unwrap(), HistoryReply::Superseded);
    discard.join().unwrap();
    assert!(!root.path().join("demo.sqlite3").exists());
    assert_eq!(home(&runtime, today()).unwrap_err(), home::UNAVAILABLE);
}

#[test]
fn unwinding_reader_releases_admission() {
    let root = tempfile::tempdir().unwrap();
    seed(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    *runtime.shared.home.on_reader.lock().unwrap() =
        Some(Arc::new(|| panic!("synthetic worker failure")));
    assert_eq!(home(&runtime, today()).unwrap_err(), super::history::WORKER);
    *runtime.shared.home.on_reader.lock().unwrap() = None;
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
}

#[test]
fn missing_database_differs_from_failed_read() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    assert_eq!(home(&runtime, today()).unwrap().runs, 0);
    std::fs::write(root.path().join("live.sqlite3"), b"private corrupt data").unwrap();
    assert_eq!(home(&runtime, today()).unwrap_err(), home::SCHEMA_FAILED);
    // Failure must release the one-job lease.
    assert_eq!(home(&runtime, today()).unwrap_err(), home::SCHEMA_FAILED);
}

#[test]
fn fresh_install_creates_directory_then_cold_read_is_empty() {
    let root = tempfile::tempdir().unwrap();
    let app_data = root.path().join("fresh");
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(app_data.clone()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    assert_eq!(home(&runtime, today()).unwrap().runs, 0);
    assert!(app_data.is_dir());
    assert!(app_data.join("live.lock").is_file());
    assert!(!app_data.join("live.sqlite3").exists());
    assert!(!app_data.join("identity.key").exists());
}

fn seed_v1(path: &std::path::Path) {
    seed_legacy(path, 1);
}

fn seed_legacy(path: &std::path::Path, version: i64) {
    assert!((1..=5).contains(&version));
    // Generate representative rows separately, then copy only legacy tables into
    // the actual historical DDL. Never downgrade a current database by relabeling it.
    let source_dir = tempfile::tempdir().unwrap();
    let source_path = source_dir.path().join("source.sqlite3");
    seed(&source_path);
    let mut source = rusqlite::Connection::open(&source_path).unwrap();
    maxed_store::apply_batch(
        &mut source,
        &[maxed_types::EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "invented-home-test".into(),
            sequence: maxed_types::Sequence(Timestamp(4)),
            at: Timestamp(2_000_000),
            event: maxed_types::EngineEvent::FightStarted(
                maxed_types::demo::demo_snapshot().fights.remove(0),
            ),
        }],
    )
    .unwrap();
    drop(source);
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch(include_str!(
        "../../../../crates/maxed-store/migrations/001_initial.sql"
    ))
    .unwrap();
    if version >= 2 {
        db.execute_batch(include_str!(
            "../../../../crates/maxed-store/migrations/002_fight_summaries.sql"
        ))
        .unwrap();
    }
    if version >= 3 {
        db.execute_batch(include_str!(
            "../../../../crates/maxed-store/migrations/003_loot_and_sessions.sql"
        ))
        .unwrap();
    }
    if version >= 4 {
        db.execute_batch(include_str!(
            "../../../../crates/maxed-store/migrations/004_alerts.sql"
        ))
        .unwrap();
    }
    if version >= 5 {
        db.execute_batch(include_str!(
            "../../../../crates/maxed-store/migrations/005_instance.sql"
        ))
        .unwrap();
    }
    db.execute(
        "ATTACH DATABASE ?1 AS source",
        [source_path.to_str().unwrap()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO schema_migrations SELECT * FROM source.schema_migrations WHERE version<=?1",
        [version],
    )
    .unwrap();
    for table in [
        "event_receipts",
        "stream_heads",
        "characters",
        "play_sessions",
        "runs",
        "run_evidence",
        "fights",
        "fight_players",
        "damage_sources",
        "timeline",
    ] {
        db.execute_batch(&format!("INSERT INTO {table} SELECT * FROM source.{table}"))
            .unwrap();
    }
    if version >= 2 {
        db.execute_batch("INSERT INTO fight_summaries SELECT * FROM source.fight_summaries")
            .unwrap();
    }
    db.execute_batch(
        "DETACH DATABASE source; UPDATE play_sessions SET json=json_remove(json,'$.issues');",
    )
    .unwrap();
    db.pragma_update(None, "user_version", version).unwrap();
    assert!(!db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .exists([])
        .unwrap());
    assert_eq!(schema_version(path), version);
}

fn schema_version(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn cold_v1_upgrades_at_startup_without_capture() {
    cold_legacy_upgrades_at_startup_without_capture(1);
}

#[test]
fn cold_v2_upgrades_at_startup_without_capture() {
    cold_legacy_upgrades_at_startup_without_capture(2);
}

#[test]
fn cold_v3_upgrades_at_startup_without_capture() {
    cold_legacy_upgrades_at_startup_without_capture(3);
}

#[test]
fn cold_v4_upgrades_at_startup_without_capture() {
    cold_legacy_upgrades_at_startup_without_capture(4);
}

#[test]
fn cold_v5_upgrades_at_startup_without_capture() {
    cold_legacy_upgrades_at_startup_without_capture(5);
}

fn cold_legacy_upgrades_at_startup_without_capture(version: i64) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_legacy(&path, version);
    assert!(maxed_store::open_reader(&path).is_err());
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    assert_eq!(schema_version(&path), 12);
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
    let reader = maxed_store::open_reader(&path).unwrap();
    assert_eq!(
        reader
            .list_runs(maxed_types::RunFilter {
                character_id: None,
                dungeon: None,
                outcome: None,
                range: None,
                sort: maxed_types::RunSort::Newest,
                cursor: None,
                limit: 50
            })
            .unwrap()
            .rows
            .len(),
        1
    );
    assert_eq!(reader.get_run("demo-run").unwrap().unwrap().fights.len(), 1);
    assert!(reader.get_fight("demo-fight").unwrap().is_some());
    assert!(reader
        .get_player_breakdown("demo-fight", 1)
        .unwrap()
        .is_some());
    assert!(!root.path().join("identity.key").exists());
}

#[test]
fn read_during_backfill_waits_for_v3() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_v1(&path);
    let runtime = Arc::new(CaptureRuntime::default());
    let (entered, waiting) = bounded(1);
    let (release, resume) = bounded(1);
    *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = reads.clone();
    *runtime.shared.home.on_reader.lock().unwrap() = Some(Arc::new(move || {
        observed.fetch_add(1, Ordering::SeqCst);
    }));
    runtime.initialize_home(root.path().into()).unwrap();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let reader_runtime = runtime.clone();
    let read =
        thread::spawn(move || tauri::async_runtime::block_on(reader_runtime.home_summary(today())));
    assert!(runtime.shared.home.context.lock().unwrap().is_none());
    assert!(!read.is_finished());
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert!(maxed_store::open_reader(&path).is_err());
    assert_eq!(schema_version(&path), 1);
    release.send(()).unwrap();
    assert!(
        matches!(read.join().unwrap().unwrap(), HistoryReply::Ready { value } if value.runs == 1)
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(schema_version(&path), 12);
}

#[test]
fn failed_startup_upgrade_is_retryable_schema_error() {
    for failure in ["directory", "lease", "corrupt-row"] {
        let root = tempfile::tempdir().unwrap();
        let app_data = root.path().join("app");
        let mut lease = None;
        if failure == "directory" {
            std::fs::write(&app_data, b"not a directory").unwrap();
        } else {
            std::fs::create_dir(&app_data).unwrap();
            seed_v1(&app_data.join("live.sqlite3"));
            if failure == "lease" {
                lease = Some(super::storage::namespace_lease(&app_data, Namespace::Live).unwrap());
            } else {
                rusqlite::Connection::open(app_data.join("live.sqlite3"))
                    .unwrap()
                    .execute("UPDATE fights SET json='bad'", [])
                    .unwrap();
            }
        }
        let runtime = Arc::new(CaptureRuntime::default());
        runtime.initialize_home(app_data).unwrap();
        assert_eq!(
            tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap_err(),
            home::SCHEMA_FAILED
        );
        assert!(runtime.shared.home.context.lock().unwrap().is_none());
        assert_eq!(home(&runtime, today()).unwrap_err(), home::SCHEMA_FAILED);
        drop(lease);
    }
}

#[test]
fn cold_preparation_prunes_chat_only_with_readable_saved_settings() {
    for corrupt in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("live.sqlite3");
        seed(&path);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            for (id, days) in [("old", 200), ("new", 10)] {
                db.execute(
                    "INSERT INTO chat_messages(id,session_id,at,channel,sender_kind,sender,is_local,text,json)
                     SELECT ?1,id,?2,'guild','player','SynthAlpha',0,'synthetic','{}' FROM play_sessions LIMIT 1",
                    rusqlite::params![id, (now - days * 86_400_000_000) as i64],
                ).unwrap();
            }
        }
        let settings = Arc::new(crate::settings::SettingsStore::new(root.path().into()).unwrap());
        settings
            .put_chat(maxed_types::ChatSettings {
                retention_days: 90,
                ..Default::default()
            })
            .unwrap();
        if corrupt {
            std::fs::write(root.path().join("settings.json"), b"invalid settings").unwrap();
        }
        let runtime = Arc::new(CaptureRuntime::default());
        runtime.configure_alert_settings(settings).unwrap();
        runtime.initialize_home(root.path().into()).unwrap();
        tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        let ids: Vec<String> = db
            .prepare("SELECT id FROM chat_messages ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            ids,
            if corrupt {
                vec!["new", "old"]
            } else {
                vec!["new"]
            }
        );
    }
}

#[test]
fn newer_schema_is_refused_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_v1(&path);
    rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", 13)
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap_err(),
        home::SCHEMA_NEWER
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(runtime.shared.home.context.lock().unwrap().is_none());
}

#[test]
fn capture_start_failure_preserves_upgraded_cold_history() {
    let root = tempfile::tempdir().unwrap();
    seed_v1(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    // Hold the lease so capture's writer cannot open; upgraded history remains readable.
    let lease = super::storage::namespace_lease(root.path(), Namespace::Live).unwrap();
    assert!(runtime
        .shared
        .open_home(
            &RuntimeOptions::live(root.path().into(), None),
            &AtomicBool::new(false),
            Bridge::default(),
            None
        )
        .is_err());
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
    drop(lease);
}

#[test]
fn unprepared_live_context_can_reset_then_retry_history() {
    let root = tempfile::tempdir().unwrap();
    seed_v1(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    let seconds = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let clock = seconds.clone();
    let base = Instant::now();
    *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || {
        base + Duration::from_secs(clock.load(Ordering::SeqCst))
    }));
    let lease = super::storage::namespace_lease(root.path(), Namespace::Live).unwrap();
    // A capture open can fail before any startup preparation has run.
    assert!(runtime
        .shared
        .open_home(
            &RuntimeOptions::live(root.path().into(), None),
            &AtomicBool::new(false),
            Bridge::default(),
            None,
        )
        .is_err());
    runtime.reset_preview().unwrap();
    assert!(
        matches!(runtime.shared.home.context.lock().unwrap().as_ref(),
        Some(context) if context.namespace == Namespace::Live)
    );
    assert_eq!(home(&runtime, today()).unwrap_err(), home::SCHEMA_FAILED);
    drop(lease);
    assert_eq!(home(&runtime, today()).unwrap_err(), home::SCHEMA_FAILED);
    seconds.store(5, Ordering::SeqCst);
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
}

#[test]
fn failed_preparation_retries_from_any_submission_once_per_five_seconds() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    std::fs::write(&path, b"not a database").unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let start = Instant::now();
    let seconds = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let clock = seconds.clone();
    *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || {
        start + Duration::from_secs(clock.load(Ordering::SeqCst))
    }));
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = attempts.clone();
    let (entered, waiting) = bounded(1);
    let (release, resume) = bounded(1);
    *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
        if count.fetch_add(1, Ordering::SeqCst) == 1 {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(10)).unwrap();
        }
    }));
    runtime.initialize_home(root.path().into()).unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap_err(),
        home::SCHEMA_FAILED
    );
    runtime.reset_preview().unwrap();
    assert!(matches!(
        runtime.shared.home.context.lock().unwrap().as_ref(),
        Some(context) if context.namespace == Namespace::Live
    ));
    assert_eq!(home(&runtime, today()).unwrap_err(), home::SCHEMA_FAILED);
    // Exercise each production command against the shared preparation gate.
    for operation in 0..12 {
        assert_eq!(
            tauri::async_runtime::block_on(super::history_tests::command(&runtime, operation))
                .unwrap_err(),
            home::SCHEMA_FAILED
        );
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    seconds.store(5, Ordering::SeqCst);
    let barrier = Arc::new(std::sync::Barrier::new(12));
    let (completed, responses) = bounded(12);
    let submissions: Vec<_> = (0..12)
        .map(|operation| {
            let runtime = runtime.clone();
            let barrier = barrier.clone();
            let completed = completed.clone();
            thread::spawn(move || {
                barrier.wait();
                completed
                    .send(tauri::async_runtime::block_on(
                        super::history_tests::command(&runtime, operation),
                    ))
                    .unwrap();
            })
        })
        .collect();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    for second in [5, 11] {
        seconds.store(second, Ordering::SeqCst);
        for operation in 0..12 {
            assert_eq!(
                tauri::async_runtime::block_on(super::history_tests::command(&runtime, operation))
                    .unwrap_err(),
                home::SCHEMA_FAILED
            );
        }
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
    for _ in 0..11 {
        assert_eq!(
            responses
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap_err(),
            home::SCHEMA_FAILED
        );
    }
    assert!(responses.try_recv().is_err());
    release.send(()).unwrap();
    assert_eq!(
        responses
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap_err(),
        home::SCHEMA_FAILED
    );
    for submission in submissions {
        submission.join().unwrap();
    }
    // A newly failed attempt at t=11 establishes its own five-second interval.
    assert_eq!(
        tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap_err(),
        home::SCHEMA_FAILED
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    std::fs::remove_file(&path).unwrap();
    seed_v1(&path);
    seconds.store(15, Ordering::SeqCst);
    assert_eq!(
        tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap_err(),
        home::SCHEMA_FAILED
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    seconds.store(16, Ordering::SeqCst);
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
    assert_eq!(attempts.load(Ordering::SeqCst), 4);
}

#[test]
fn once_observed_existing_database_cannot_disappear_into_empty_history() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let runtime = Arc::new(CaptureRuntime::default());
    // Deletes between reads; close each reader so Windows releases the database.
    *runtime.shared.home.reader_idle.lock().unwrap() = Duration::ZERO;
    runtime.initialize_home(root.path().into()).unwrap();
    assert_eq!(home(&runtime, today()).unwrap().runs, 0);
    seed(&path);
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(home(&runtime, today()).unwrap_err(), home::READ_FAILED);
}

#[test]
fn namespace_replacement_drains_upgrade_and_retires_its_waiters() {
    use std::future::Future;
    let root = tempfile::tempdir().unwrap();
    seed_v1(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    let (entered, waiting) = bounded(1);
    let (release, resume) = bounded(1);
    *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    runtime.initialize_home(root.path().into()).unwrap();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(super::storage::namespace_lease(root.path(), Namespace::Live).is_err());
    let mut waiter = Box::pin(runtime.ensure_history_ready());
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    assert!(waiter.as_mut().poll(&mut context).is_pending());
    let old = runtime.shared.home.generation.load(Ordering::Acquire);
    let switching = runtime.clone();
    let path = root.path().to_path_buf();
    let transition =
        thread::spawn(move || open(&switching, &path, Namespace::Demo).close().unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.shared.home.generation.load(Ordering::Acquire) == old {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(!transition.is_finished());
    assert!(!root.path().join("demo.sqlite3").exists());
    release.send(()).unwrap();
    transition.join().unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(waiter).unwrap_err(),
        home::CHANGED
    );
    assert_eq!(
        runtime
            .shared
            .home
            .context
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .namespace,
        Namespace::Demo
    );
    runtime.reset_preview().unwrap();
    assert_eq!(home(&runtime, today()).unwrap().runs, 1);
}

#[test]
fn namespace_switch_supersedes_scope_loot_read() {
    let root = tempfile::tempdir().unwrap();
    seed(&root.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    open(&runtime, root.path(), Namespace::Demo)
        .close()
        .unwrap();
    let (entered, release) = pause_reader(&runtime);
    let reading = runtime.clone();
    let read = thread::spawn(move || {
        tauri::async_runtime::block_on(reading.scope_loot(today(), None, 20, vec![]))
    });
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let switching = runtime.clone();
    let reset = thread::spawn(move || switching.reset_preview());
    wait_for_invalidation(&runtime);
    release.send(()).unwrap();
    assert_eq!(read.join().unwrap().unwrap(), HistoryReply::Superseded);
    reset.join().unwrap().unwrap();
    *runtime.shared.home.on_reader.lock().unwrap() = None;
}
