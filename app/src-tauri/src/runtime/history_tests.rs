//! Invented native history data; no recordings or capture DLL are used.
use super::history::{HistoryCaller, HistoryKey};
use super::*;
use maxed_types::*;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Waker},
};

fn scope() -> HomeScope {
    HomeScope::Today {
        range: TimeRange {
            start: Timestamp(0),
            end: Timestamp(i64::MAX as u64),
        },
    }
}
fn filter() -> RunFilter {
    RunFilter {
        character_id: None,
        dungeon: None,
        outcome: None,
        range: None,
        sort: RunSort::Newest,
        cursor: None,
        limit: 20,
    }
}
fn alert_filter() -> AlertFilter {
    AlertFilter {
        kinds: vec![],
        exclude_kinds: vec![],
        kind: None,
        range: None,
        query: String::new(),
        cursor: None,
        limit: 50,
    }
}

#[test]
fn alerts_history_validates_before_reader_and_sanitizes_legacy_failures() {
    let (dir, runtime) = setup();
    for query in ["x".repeat(129), "😀".repeat(129), "secret\0path".into()] {
        assert_eq!(
            tauri::async_runtime::block_on(runtime.list_alerts(AlertFilter {
                query,
                ..alert_filter()
            }))
            .unwrap_err(),
            history::INVALID
        );
    }
    let db = rusqlite::Connection::open(dir.path().join("live.sqlite3")).unwrap();
    db.execute(
        "INSERT INTO alerts VALUES('bad',NULL,1,'death','private malformed data')",
        [],
    )
    .unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(runtime.list_alerts(alert_filter())).unwrap_err(),
        home::READ_FAILED
    );
    db.execute(
        "UPDATE alerts SET json=?1",
        ["s".repeat(2 * 1024 * 1024 + 1)],
    )
    .unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(runtime.list_alerts(alert_filter())).unwrap_err(),
        history::LIMIT
    );
}

#[test]
fn alerts_history_shares_abuse_budget_without_evicting_pending_run() {
    let (_dir, runtime) = setup();
    let now = Instant::now();
    *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || now));
    let (entered, release) = pause(&runtime);
    let mut active = Box::pin(command(&runtime, 0));
    pending(active.as_mut());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    for _ in 0..30 {
        assert_eq!(
            tauri::async_runtime::block_on(runtime.list_alerts(AlertFilter {
                limit: 0,
                ..alert_filter()
            }))
            .unwrap_err(),
            history::INVALID
        );
    }
    let mut run = Box::pin(command(&runtime, 2));
    pending(run.as_mut());
    let mut alert = Box::pin(command(&runtime, 5));
    pending(alert.as_mut());
    assert_eq!(counts(&runtime), (1, 1, 1, 32));
    release.send(()).unwrap();
    ready(tauri::async_runtime::block_on(active));
    assert_eq!(
        tauri::async_runtime::block_on(alert).unwrap(),
        HistoryReply::Busy
    );
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    ready(tauri::async_runtime::block_on(run));
}

#[test]
fn alerts_history_trimmed_same_key_supersedes_active_read() {
    let (_dir, runtime) = setup();
    let (entered, release) = pause(&runtime);
    let mut active = Box::pin(runtime.list_alerts(AlertFilter {
        query: "  bag  ".into(),
        ..alert_filter()
    }));
    pending(active.as_mut());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut next = Box::pin(runtime.list_alerts(AlertFilter {
        query: "bag".into(),
        ..alert_filter()
    }));
    pending(next.as_mut());
    release.send(()).unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(active).unwrap(),
        HistoryReply::Superseded
    );
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    ready(tauri::async_runtime::block_on(next));
}
fn seed(path: &std::path::Path) {
    let mut connection = rusqlite::Connection::open(path).unwrap();
    maxed_store::migrate(&mut connection).unwrap();
    let snapshot = demo::demo_snapshot();
    let mut events = vec![
        EngineEvent::SessionUpdated(snapshot.session.unwrap()),
        EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
        EngineEvent::RunStarted(snapshot.run.unwrap()),
    ];
    events.extend(snapshot.fights.into_iter().map(EngineEvent::FightUpdated));
    let events: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(i, event)| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "invented-b2-history".into(),
            sequence: Sequence(Timestamp(i as u64 + 1)),
            at: Timestamp(1_000_000),
            event,
        })
        .collect();
    maxed_store::apply_batch(&mut connection, &events).unwrap();
}
fn setup() -> (tempfile::TempDir, Arc<CaptureRuntime>) {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(dir.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    (dir, runtime)
}

#[test]
fn reads_run_on_named_history_reader_thread() {
    let (_dir, runtime) = setup();
    let (send, receive) = std::sync::mpsc::channel();
    *runtime.shared.home.on_reader.lock().unwrap() = Some(Arc::new(move || {
        let thread = std::thread::current();
        send.send((thread.name().map(str::to_owned), thread.id()))
            .unwrap();
    }));
    for _ in 0..2 {
        assert!(matches!(
            tauri::async_runtime::block_on(runtime.list_runs(filter())).unwrap(),
            HistoryReply::Ready { .. }
        ));
    }
    let first = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    let second = receive.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first.0.as_deref(), Some("history-reader"));
    assert_eq!(second.0.as_deref(), Some("history-reader"));
    assert_eq!(first.1, second.1);
}

#[test]
fn reads_restart_a_disconnected_history_reader() {
    let (_dir, runtime) = setup();
    let (send, receive) = std::sync::mpsc::channel();
    drop(receive);
    *runtime.shared.home.reader.lock().unwrap() = Some((0, send));
    assert!(matches!(
        tauri::async_runtime::block_on(runtime.list_runs(filter())).unwrap(),
        HistoryReply::Ready { .. }
    ));
    assert_eq!(counts(&runtime).0, 0);
}

#[test]
fn update_retires_history_leases_before_launch_and_resumes_afterward() {
    let (_dir, runtime) = setup();
    let (entered, release) = pause(&runtime);
    let mut active = Box::pin(command(&runtime, 0));
    pending(active.as_mut());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let owner = runtime.clone();
    let (done, completion) = bounded(1);
    let worker = std::thread::spawn(move || done.send(owner.prepare_update()).unwrap());
    assert!(completion.recv_timeout(Duration::from_millis(50)).is_err());
    release.send(()).unwrap();
    completion
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(active).unwrap(),
        HistoryReply::Superseded
    );
    assert_eq!(
        tauri::async_runtime::block_on(command(&runtime, 0)).unwrap(),
        HistoryReply::Superseded
    );
    *runtime.shared.home.on_reader.lock().unwrap() = None;
    runtime.finish_update();
    ready(tauri::async_runtime::block_on(command(&runtime, 0)));
}

#[test]
fn update_waits_for_schema_preparation_and_blocks_new_recovery_until_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let (entered, waiting) = bounded(1);
    let (release, gate) = bounded(1);
    *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
        entered.send(()).unwrap();
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    runtime.initialize_home(dir.path().into()).unwrap();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let owner = runtime.clone();
    let (done, completion) = bounded(1);
    let worker = std::thread::spawn(move || done.send(owner.prepare_update()).unwrap());
    assert!(completion.recv_timeout(Duration::from_millis(50)).is_err());
    release.send(()).unwrap();
    completion
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert!(tauri::async_runtime::block_on(runtime.ensure_history_ready()).is_err());
    runtime.finish_update();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
}
fn ready<T: std::fmt::Debug>(reply: Result<HistoryReply<T>, String>) -> T {
    match reply.unwrap() {
        HistoryReply::Ready { value } => value,
        other => panic!("expected Ready, got {other:?}"),
    }
}
fn json<T: serde::Serialize>(reply: HistoryReply<T>) -> HistoryReply<serde_json::Value> {
    match reply {
        HistoryReply::Ready { value } => HistoryReply::Ready {
            value: serde_json::to_value(value).unwrap(),
        },
        HistoryReply::Superseded => HistoryReply::Superseded,
        HistoryReply::Busy => HistoryReply::Busy,
    }
}
pub(super) async fn command(
    runtime: &Arc<CaptureRuntime>,
    index: usize,
) -> Result<HistoryReply<serde_json::Value>, String> {
    let snapshot = demo::demo_snapshot();
    Ok(match index {
        0 => json(runtime.home_summary(scope()).await?),
        1 => json(runtime.list_runs(filter()).await?),
        2 => json(runtime.get_run(snapshot.run.unwrap().id).await?),
        3 => json(runtime.get_fight(snapshot.fights[0].id.clone()).await?),
        4 => json(
            runtime
                .get_player_breakdown(
                    snapshot.fights[0].id.clone(),
                    snapshot.fights[0].players[0].object_id,
                )
                .await?,
        ),
        5 => json(runtime.list_alerts(alert_filter()).await?),
        6 => json(runtime.scope_loot(scope(), None, 20, vec![]).await?),
        7 => json(runtime.dungeon_bests().await?),
        8 => json(
            runtime
                .list_drops(DropFilter {
                    min_rank: None,
                    place: None,
                    item_type: None,
                    enchanted_only: false,
                    range: None,
                    cursor: None,
                    limit: 1,
                })
                .await?,
        ),
        9 => json(
            runtime
                .get_item_history(ItemHistoryRequest {
                    item_type: 3180,
                    cursor: None,
                    limit: 1,
                })
                .await?,
        ),
        10 => json(
            runtime
                .get_loot_stats(LootStatsFilter { range: None })
                .await?,
        ),
        11 => json(
            runtime
                .get_collection(
                    CollectionRequest {
                        dungeon: None,
                        chase: vec![],
                    },
                    vec![],
                )
                .await?,
        ),
        _ => unreachable!(),
    })
}
fn pending<F: Future>(future: Pin<&mut F>) {
    assert!(future
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
}
fn pause(runtime: &CaptureRuntime) -> (Receiver<()>, Sender<()>) {
    let (send, entered) = bounded(1);
    let (release, wait) = bounded(1);
    *runtime.shared.home.on_reader.lock().unwrap() = Some(Arc::new(move || {
        send.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    (entered, release)
}
fn counts(runtime: &CaptureRuntime) -> (usize, usize, usize, usize) {
    runtime.shared.home.scheduler.lock().unwrap().counts()
}
fn wait_retired(runtime: &CaptureRuntime) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.shared.home.context.lock().unwrap().is_some() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
}

#[test]
fn commands_return_ready_payloads_and_missing_is_null() {
    let (_dir, runtime) = setup();
    for index in 0..12 {
        let value = ready(tauri::async_runtime::block_on(command(&runtime, index)));
        assert!(!value.is_null(), "seeded command {index} must find data");
    }
    assert!(ready(tauri::async_runtime::block_on(
        runtime.get_run("missing".into())
    ))
    .is_none());
    assert!(ready(tauri::async_runtime::block_on(
        runtime.get_fight("missing".into())
    ))
    .is_none());
    assert!(ready(tauri::async_runtime::block_on(
        runtime.get_player_breakdown("missing".into(), -9)
    ))
    .is_none());
}

#[test]
fn ordinary_navigation_never_returns_busy() {
    for first in [0, 2] {
        let (_dir, runtime) = setup();
        let (entered, release) = pause(&runtime);
        let mut active = Box::pin(command(&runtime, first));
        pending(active.as_mut());
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut displaced = Box::pin(command(&runtime, 1));
        pending(displaced.as_mut());
        let mut newest = Box::pin(command(&runtime, 3));
        pending(newest.as_mut());
        assert_eq!(
            tauri::async_runtime::block_on(displaced).unwrap(),
            HistoryReply::Superseded
        );
        assert_eq!((counts(&runtime).0, counts(&runtime).1), (1, 1));
        release.send(()).unwrap();
        ready(tauri::async_runtime::block_on(active));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        release.send(()).unwrap();
        ready(tauri::async_runtime::block_on(newest));
    }
}

#[test]
fn alerts_history_home_run_alert_competition_never_returns_busy() {
    for first in [0, 2, 5] {
        let (_dir, runtime) = setup();
        let (entered, release) = pause(&runtime);
        let mut active = Box::pin(command(&runtime, first));
        pending(active.as_mut());
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut displaced = Box::pin(command(&runtime, 1));
        pending(displaced.as_mut());
        let mut newest = Box::pin(command(&runtime, if first == 5 { 0 } else { 5 }));
        pending(newest.as_mut());
        assert_eq!(
            tauri::async_runtime::block_on(displaced).unwrap(),
            HistoryReply::Superseded
        );
        assert_eq!((counts(&runtime).0, counts(&runtime).1), (1, 1));
        release.send(()).unwrap();
        ready(tauri::async_runtime::block_on(active));
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        release.send(()).unwrap();
        ready(tauri::async_runtime::block_on(newest));
    }
}

#[test]
fn newer_same_key_supersedes_active_even_when_newest_pending_changes_again() {
    let (_dir, runtime) = setup();
    let (entered, release) = pause(&runtime);
    let mut active = Box::pin(command(&runtime, 2));
    pending(active.as_mut());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut refresh = Box::pin(command(&runtime, 2));
    pending(refresh.as_mut());
    let mut newest = Box::pin(command(&runtime, 4));
    pending(newest.as_mut());
    assert_eq!(
        tauri::async_runtime::block_on(refresh).unwrap(),
        HistoryReply::Superseded
    );
    release.send(()).unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(active).unwrap(),
        HistoryReply::Superseded
    );
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    ready(tauri::async_runtime::block_on(newest));
}

#[test]
fn all_history_commands_share_namespace_retirement() {
    for namespace in [Namespace::Live, Namespace::Demo, Namespace::Replay] {
        for index in 0..12 {
            let (dir, runtime) = setup();
            let mut options = RuntimeOptions::live(dir.path().into(), None);
            options.namespace = namespace;
            runtime
                .shared
                .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
                .unwrap()
                .close()
                .unwrap();
            let path = options.database();
            seed(&path);
            let (entered, release) = pause(&runtime);
            let mut active = Box::pin(command(&runtime, index));
            pending(active.as_mut());
            entered.recv_timeout(Duration::from_secs(5)).unwrap();
            let mut queued = Box::pin(command(&runtime, (index + 1) % 12));
            pending(queued.as_mut());
            let switching = runtime.clone();
            let transition = thread::spawn(move || {
                switching
                    .shared
                    .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
                    .unwrap()
                    .close()
                    .unwrap()
            });
            wait_retired(&runtime);
            assert_eq!(
                tauri::async_runtime::block_on(queued).unwrap(),
                HistoryReply::Superseded
            );
            assert!(
                !transition.is_finished(),
                "cleanup must wait for SQLite close"
            );
            assert_eq!(
                maxed_store::open_reader(&path)
                    .unwrap()
                    .list_runs(filter())
                    .unwrap()
                    .rows
                    .len(),
                1
            );
            release.send(()).unwrap();
            assert_eq!(
                tauri::async_runtime::block_on(active).unwrap(),
                HistoryReply::Superseded
            );
            transition.join().unwrap();
            *runtime.shared.home.on_reader.lock().unwrap() = None;
            assert_eq!(
                ready(tauri::async_runtime::block_on(runtime.list_runs(filter())))
                    .rows
                    .len(),
                usize::from(namespace == Namespace::Live),
            );
            runtime.reset_preview().unwrap();
            assert_eq!(
                ready(tauri::async_runtime::block_on(runtime.list_runs(filter())))
                    .rows
                    .len(),
                1
            );
        }
    }
}

fn native<'a>(
    runtime: &'a Arc<CaptureRuntime>,
    caller: HistoryCaller,
    id: &str,
) -> impl Future<Output = Result<HistoryReply<u32>, String>> + 'a {
    let reader = runtime.clone();
    runtime.submit_history(caller, HistoryKey::Run(id.into()), move |job| {
        reader.read_history(job, || 0, |_| Ok(1))
    })
}

#[test]
fn abuse_busy_retries_only_after_active_completion() {
    let (_dir, runtime) = setup();
    let now = Instant::now();
    let elapsed = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let clock = elapsed.clone();
    *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || {
        now + Duration::from_millis(clock.load(Ordering::Acquire))
    }));
    let (entered, release) = pause(&runtime);
    let mut active = Box::pin(native(&runtime, HistoryCaller::MainWindow, "active"));
    pending(active.as_mut());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut latest = None;
    for _ in 1..32 {
        let mut next = Box::pin(native(&runtime, HistoryCaller::MainWindow, "pending"));
        pending(next.as_mut());
        if let Some(previous) = latest.replace(next) {
            assert_eq!(
                tauri::async_runtime::block_on(previous).unwrap(),
                HistoryReply::Superseded
            );
        }
    }
    let mut busy = Box::pin(native(&runtime, HistoryCaller::MainWindow, "abuse"));
    pending(busy.as_mut());
    assert_eq!(counts(&runtime), (1, 1, 1, 32));
    let mut newest_notice = Box::pin(native(&runtime, HistoryCaller::MainWindow, "abuse"));
    pending(newest_notice.as_mut());
    assert_eq!(
        tauri::async_runtime::block_on(busy).unwrap(),
        HistoryReply::Superseded
    );
    pending(newest_notice.as_mut());
    assert_eq!(counts(&runtime), (1, 1, 1, 32));
    release.send(()).unwrap();
    assert_eq!(ready(tauri::async_runtime::block_on(active)), 1);
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    // Busy belongs to the old completion, even while the legitimate next read runs.
    assert_eq!(
        tauri::async_runtime::block_on(newest_notice).unwrap(),
        HistoryReply::Busy
    );
    elapsed.store(1000, Ordering::Release);
    let mut retry = Box::pin(native(&runtime, HistoryCaller::MainWindow, "retry"));
    pending(retry.as_mut());
    assert_eq!((counts(&runtime).0, counts(&runtime).1), (1, 1));
    release.send(()).unwrap();
    assert_eq!(ready(tauri::async_runtime::block_on(latest.unwrap())), 1);
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    release.send(()).unwrap();
    assert_eq!(ready(tauri::async_runtime::block_on(retry)), 1);
}

#[test]
fn caller_quotas_are_independent_and_count_invalid_attempts() {
    let (_dir, runtime) = setup();
    let now = Instant::now();
    *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || now));
    for _ in 0..32 {
        assert_eq!(
            tauri::async_runtime::block_on(native(&runtime, HistoryCaller::MainWindow, ""))
                .unwrap_err(),
            history::INVALID
        );
    }
    assert_eq!(
        tauri::async_runtime::block_on(native(&runtime, HistoryCaller::MainWindow, "valid"))
            .unwrap(),
        HistoryReply::Busy
    );
    assert_eq!(
        ready(tauri::async_runtime::block_on(native(
            &runtime,
            HistoryCaller::NativeTest,
            "valid"
        ))),
        1
    );
    assert_eq!(counts(&runtime).3, 33);
}

#[test]
fn history_read_does_not_block_capture_or_advance_watermark() {
    for index in 0..12 {
        let (_dir, runtime) = setup();
        let before = runtime.store_status();
        let (entered, release) = pause(&runtime);
        let mut read = Box::pin(command(&runtime, index));
        pending(read.as_mut());
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(runtime.session.try_lock().is_ok());
        assert!(runtime.shared.recovery.try_lock().is_ok());
        assert!(runtime.shared.completed.try_lock().is_ok());
        assert!(runtime.shared.views.store.try_lock().is_ok());
        runtime.shared.views.snapshot.try_lock().unwrap().at = Timestamp(55);
        let sink: StatusSink = Arc::new(|_| {});
        runtime.shared.publish(
            CaptureStatus {
                state: CaptureState::Live,
                ..Default::default()
            },
            &sink,
        );
        assert_eq!(runtime.snapshot().at, Timestamp(55));
        assert_eq!(runtime.status().state, CaptureState::Live);
        runtime.stop().unwrap();
        release.send(()).unwrap();
        ready(tauri::async_runtime::block_on(read));
        assert_eq!(runtime.store_status(), before);
    }
}

#[test]
fn dropped_pending_receiver_never_opens_a_reader_and_panic_recovers() {
    let (_dir, runtime) = setup();
    let (entered, release) = pause(&runtime);
    let mut active = Box::pin(command(&runtime, 0));
    pending(active.as_mut());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut abandoned = Box::pin(command(&runtime, 3));
    pending(abandoned.as_mut());
    drop(abandoned);
    release.send(()).unwrap();
    ready(tauri::async_runtime::block_on(active));
    assert_eq!((counts(&runtime).0, counts(&runtime).1), (0, 0));
    assert!(entered.try_recv().is_err());
    *runtime.shared.home.on_reader.lock().unwrap() =
        Some(Arc::new(|| panic!("private synthetic worker panic")));
    assert_eq!(
        tauri::async_runtime::block_on(command(&runtime, 3)).unwrap_err(),
        history::WORKER
    );
    *runtime.shared.home.on_reader.lock().unwrap() = None;
    ready(tauri::async_runtime::block_on(command(&runtime, 3)));
}

#[test]
fn startup_coalesces_before_any_reader_is_dispatched() {
    let dir = tempfile::tempdir().unwrap();
    seed(&dir.path().join("live.sqlite3"));
    let runtime = Arc::new(CaptureRuntime::default());
    let (send, entered) = bounded(1);
    let (release, wait) = bounded(1);
    *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
        send.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    runtime.initialize_home(dir.path().into()).unwrap();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut previous = None;
    for index in 0..12 {
        let mut next = Box::pin(command(&runtime, index));
        pending(next.as_mut());
        if let Some(old) = previous.replace(next) {
            assert_eq!(
                tauri::async_runtime::block_on(old).unwrap(),
                HistoryReply::Superseded
            );
        }
        assert_eq!((counts(&runtime).0, counts(&runtime).1), (0, 1));
    }
    release.send(()).unwrap();
    ready(tauri::async_runtime::block_on(previous.unwrap()));
}

#[test]
fn schema_gate_applies_to_every_command_and_absence_is_not_corruption() {
    for index in 0..12 {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(CaptureRuntime::default());
        runtime.initialize_home(dir.path().into()).unwrap();
        ready(tauri::async_runtime::block_on(command(&runtime, index)));
        assert!(!dir.path().join("live.sqlite3").exists());
        std::fs::write(dir.path().join("live.sqlite3"), b"private corrupt bytes").unwrap();
        assert_eq!(
            tauri::async_runtime::block_on(command(&runtime, index)).unwrap_err(),
            home::SCHEMA_FAILED
        );
        for next in 0..12 {
            assert_eq!(
                tauri::async_runtime::block_on(command(&runtime, next)).unwrap_err(),
                home::SCHEMA_FAILED
            );
        }
    }
}

#[test]
fn request_overflow_and_shutdown_resolve_waiters() {
    for overflow in [false, true] {
        let (_dir, runtime) = setup();
        let now = Instant::now();
        *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || now));
        let (entered, release) = pause(&runtime);
        let mut active = Box::pin(command(&runtime, 0));
        pending(active.as_mut());
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut queued = Box::pin(command(&runtime, 3));
        pending(queued.as_mut());
        for _ in 2..32 {
            assert!(tauri::async_runtime::block_on(runtime.get_run("".into())).is_err());
        }
        let mut notice = Box::pin(command(&runtime, 4));
        pending(notice.as_mut());
        let shutdown = if overflow {
            runtime
                .shared
                .home
                .scheduler
                .lock()
                .unwrap()
                .exhaust_requests();
            assert_eq!(
                tauri::async_runtime::block_on(native(
                    &runtime,
                    HistoryCaller::NativeTest,
                    "overflow"
                ))
                .unwrap_err(),
                home::UNAVAILABLE
            );
            None
        } else {
            let closing = runtime.clone();
            Some(thread::spawn(move || closing.shutdown()))
        };
        assert_eq!(
            tauri::async_runtime::block_on(queued).unwrap(),
            HistoryReply::Superseded
        );
        assert_eq!(
            tauri::async_runtime::block_on(notice).unwrap(),
            HistoryReply::Superseded
        );
        if let Some(shutdown) = &shutdown {
            assert!(!shutdown.is_finished());
        }
        release.send(()).unwrap();
        assert_eq!(
            tauri::async_runtime::block_on(active).unwrap(),
            HistoryReply::Superseded
        );
        if let Some(shutdown) = shutdown {
            shutdown.join().unwrap().unwrap();
            assert_eq!(
                tauri::async_runtime::block_on(command(&runtime, 1)).unwrap(),
                HistoryReply::Superseded
            );
        }
    }
}

#[test]
fn invalid_queries_are_sanitized_before_queueing_and_size_errors_are_fixed() {
    let (_dir, runtime) = setup();
    for id in ["", "private\nsecret", &"x".repeat(257)] {
        assert_eq!(
            tauri::async_runtime::block_on(runtime.get_run(id.into())).unwrap_err(),
            history::INVALID
        );
        assert_eq!(
            tauri::async_runtime::block_on(runtime.get_fight(id.into())).unwrap_err(),
            history::INVALID
        );
        assert_eq!(
            tauri::async_runtime::block_on(runtime.get_player_breakdown(id.into(), i32::MIN))
                .unwrap_err(),
            history::INVALID
        );
    }
    for sort_value in ["01", "+1", "-0", "-2", "9223372036854775808"] {
        let mut query = filter();
        query.cursor = Some(RunCursor {
            sort_value: sort_value.into(),
            id: "valid".into(),
        });
        assert_eq!(
            tauri::async_runtime::block_on(runtime.list_runs(query)).unwrap_err(),
            history::INVALID
        );
    }
    assert_eq!(counts(&runtime).0, 0);
    let reader = runtime.clone();
    assert_eq!(
        tauri::async_runtime::block_on(runtime.submit_history(
            HistoryCaller::NativeTest,
            HistoryKey::Run("oversize".into()),
            move |job| reader.read_history(job, || (), |_| Err(maxed_store::StoreError::Limit))
        ))
        .unwrap_err(),
        history::LIMIT
    );
    ready(tauri::async_runtime::block_on(command(&runtime, 0)));
}

#[test]
fn failed_live_open_and_missing_playback_apply_to_every_command() {
    let (dir, runtime) = setup();
    let mut now = Instant::now();
    *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || now));
    std::fs::write(dir.path().join("identity.key"), b"invalid").unwrap();
    assert!(runtime
        .shared
        .open_home(
            &RuntimeOptions::live(dir.path().into(), None),
            &AtomicBool::new(false),
            Bridge::default(),
            None
        )
        .is_err());
    for index in 0..12 {
        ready(tauri::async_runtime::block_on(command(&runtime, index)));
    }
    std::fs::remove_file(dir.path().join("identity.key")).unwrap();
    for namespace in [Namespace::Demo, Namespace::Replay] {
        // Each namespace is a separate burst; twelve commands would otherwise
        // exhaust the shared 32-attempt budget before testing read failures.
        now += Duration::from_secs(2);
        *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || now));
        let mut options = RuntimeOptions::live(dir.path().into(), None);
        options.namespace = namespace;
        std::fs::create_dir(options.database()).unwrap();
        assert!(runtime
            .shared
            .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
            .is_err());
        for index in 0..12 {
            assert_eq!(
                tauri::async_runtime::block_on(command(&runtime, index)).unwrap_err(),
                home::UNAVAILABLE
            );
        }
        std::fs::remove_dir(options.database()).unwrap();
        runtime
            .shared
            .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
            .unwrap()
            .close()
            .unwrap();
        std::fs::remove_file(options.database()).unwrap();
        for index in 0..12 {
            assert_eq!(
                tauri::async_runtime::block_on(command(&runtime, index)).unwrap_err(),
                home::READ_FAILED
            );
        }
        runtime.reset_preview().unwrap();
        // Use the independent native caller for the positive control after this burst.
        assert_eq!(
            ready(tauri::async_runtime::block_on(native(
                &runtime,
                HistoryCaller::NativeTest,
                "live"
            ))),
            1
        );
    }
}

#[test]
fn retired_abuse_notices_and_namespace_overflow_never_wait_for_a_new_source() {
    let (_dir, runtime) = setup();
    let (entered, release) = pause(&runtime);
    let now = Instant::now();
    *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || now));
    let mut active = Box::pin(command(&runtime, 0));
    pending(active.as_mut());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut queued = Box::pin(command(&runtime, 1));
    pending(queued.as_mut());
    for _ in 2..32 {
        assert!(tauri::async_runtime::block_on(runtime.get_run("".into())).is_err());
    }
    let mut notice = Box::pin(command(&runtime, 3));
    pending(notice.as_mut());
    runtime
        .shared
        .home
        .generation
        .store(u64::MAX, Ordering::Release);
    let switching = runtime.clone();
    let transition = thread::spawn(move || switching.shared.home.invalidate_and_wait());
    wait_retired(&runtime);
    assert_eq!(
        tauri::async_runtime::block_on(queued).unwrap(),
        HistoryReply::Superseded
    );
    assert_eq!(
        tauri::async_runtime::block_on(notice).unwrap(),
        HistoryReply::Superseded
    );
    assert!(!transition.is_finished());
    release.send(()).unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(active).unwrap(),
        HistoryReply::Superseded
    );
    assert_eq!(transition.join().unwrap().unwrap_err(), home::UNAVAILABLE);
    assert_eq!(
        runtime.shared.home.generation.load(Ordering::Acquire),
        u64::MAX
    );
}

#[test]
fn cancelling_all_callers_drops_runtime_after_active_reader_closes() {
    let (_dir, runtime) = setup();
    let weak = Arc::downgrade(&runtime);
    let (entered, release) = pause(&runtime);
    let mut active = Box::pin(command(&runtime, 0));
    pending(active.as_mut());
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut queued = Box::pin(command(&runtime, 1));
    pending(queued.as_mut());
    drop(active);
    drop(queued);
    drop(runtime);
    assert!(
        weak.upgrade().is_some(),
        "SQLite must not be interrupted by receiver drop"
    );
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while weak.upgrade().is_some() {
        assert!(
            Instant::now() < deadline,
            "runtime ownership leaked or worker deadlocked"
        );
        thread::yield_now();
    }
    assert!(
        entered.try_recv().is_err(),
        "cancelled pending read must not open SQLite"
    );
}

#[test]
fn queued_reads_recheck_schema_and_observed_database_absence() {
    for corrupt in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("live.sqlite3");
        let runtime = Arc::new(CaptureRuntime::default());
        // Deletes between reads; close each reader so Windows releases the database.
        *runtime.shared.home.reader_idle.lock().unwrap() = Duration::ZERO;
        runtime.initialize_home(dir.path().into()).unwrap();
        tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
        if corrupt {
            std::fs::write(&path, b"invented invalid history").unwrap();
        } else {
            seed(&path);
        }
        let (send, entered) = bounded(1);
        let (release, wait) = bounded(1);
        let reader = runtime.clone();
        let mut active = Box::pin(runtime.submit_history(
            HistoryCaller::NativeTest,
            HistoryKey::Run("first".into()),
            move |job| {
                send.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                let result = reader.read_history(job, || 0, |_| Ok(1));
                if !corrupt {
                    std::fs::remove_file(path).unwrap();
                }
                result
            },
        ));
        pending(active.as_mut());
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut queued = Box::pin(runtime.get_run("queued".into()));
        pending(queued.as_mut());
        release.send(()).unwrap();
        if corrupt {
            assert_eq!(
                tauri::async_runtime::block_on(active).unwrap_err(),
                home::SCHEMA_FAILED
            );
            assert_eq!(
                tauri::async_runtime::block_on(queued).unwrap_err(),
                home::SCHEMA_FAILED
            );
        } else {
            assert_eq!(ready(tauri::async_runtime::block_on(active)), 1);
            assert_eq!(
                tauri::async_runtime::block_on(queued).unwrap_err(),
                home::READ_FAILED
            );
        }
    }
}

#[test]
fn namespace_exhaustion_cannot_reinstall_a_retired_startup_context() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime
        .shared
        .home
        .generation
        .store(u64::MAX - 1, Ordering::Release);
    let (send, entered) = bounded(1);
    let (release, wait) = bounded(1);
    *runtime.shared.home.on_prepare.lock().unwrap() = Some(Arc::new(move || {
        send.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    runtime.initialize_home(dir.path().into()).unwrap();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut queued = Box::pin(command(&runtime, 1));
    pending(queued.as_mut());
    let switching = runtime.clone();
    let transition = thread::spawn(move || switching.shared.home.invalidate_and_wait());
    assert_eq!(
        tauri::async_runtime::block_on(queued).unwrap(),
        HistoryReply::Superseded
    );
    assert!(!transition.is_finished());
    release.send(()).unwrap();
    assert_eq!(transition.join().unwrap().unwrap_err(), home::UNAVAILABLE);
    assert!(runtime.shared.home.context.lock().unwrap().is_none());
    assert_eq!(
        runtime.shared.home.generation.load(Ordering::Acquire),
        u64::MAX
    );
}

#[test]
fn scope_loot_validates_before_read_and_returns_a_page() {
    let (_dir, runtime) = setup();
    for limit in [0, 101] {
        assert_eq!(
            tauri::async_runtime::block_on(runtime.scope_loot(scope(), None, limit, vec![]))
                .unwrap_err(),
            super::history::INVALID
        );
    }
    let page = ready(tauri::async_runtime::block_on(runtime.scope_loot(
        scope(),
        None,
        20,
        vec![],
    )));
    assert!(page.items.len() <= 20);
    // This runtime fixture seeds sessions/runs/fights, but deliberately no bags.
    assert_eq!(page.total_bags, 0);
}

#[test]
fn dungeon_bests_returns_scheduled_history() {
    let (_dir, runtime) = setup();
    let rows = ready(tauri::async_runtime::block_on(runtime.dungeon_bests()));
    assert!(rows.iter().all(|row| row.best_clear_micros > 0));
}

#[test]
fn alert_summary_and_kind_filters_read_replay_and_validate_requests() {
    let (dir, runtime) = setup();
    let mut options = RuntimeOptions::live(dir.path().into(), None);
    options.namespace = Namespace::Replay;
    runtime
        .shared
        .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
        .unwrap()
        .close()
        .unwrap();
    let mut db = rusqlite::Connection::open(options.database()).unwrap();
    maxed_store::apply_batch(
        &mut db,
        &[EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic-alert-replay".into(),
            sequence: Sequence(Timestamp(1)),
            at: Timestamp(0),
            event: EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
        }],
    )
    .unwrap();
    let events = [
        AlertKind::WhiteBag,
        AlertKind::Whisper,
        AlertKind::KeyPopped,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, kind)| EventEnvelope {
        fight_chart_omitted: false,
        stream_id: "synthetic-alert-replay".into(),
        sequence: Sequence(Timestamp(i as u64 + 2)),
        at: Timestamp(i as u64 + 1),
        event: EngineEvent::AlertFired(AlertView {
            id: format!("synthetic-alert-{i}"),
            at: Timestamp(i as u64 + 1),
            kind,
            run_id: None,
            explanation: "Synthetic replay".into(),
            details: None,
            presentation: None,
        }),
    })
    .collect::<Vec<_>>();
    maxed_store::apply_batch(&mut db, &events).unwrap();
    drop(db);
    let summary = ready(tauri::async_runtime::block_on(
        runtime.alert_summary(AlertSummaryRequest::default()),
    ));
    assert_eq!(summary.total, 3);
    assert_eq!(summary.first_at, Some(Timestamp(1)));
    assert_eq!(summary.last_white.unwrap().at, Timestamp(1));
    let mut filter = alert_filter();
    filter.kinds = vec![AlertKind::Whisper, AlertKind::KeyPopped];
    filter.exclude_kinds = vec![AlertKind::KeyPopped];
    let page = ready(tauri::async_runtime::block_on(
        runtime.list_alerts(filter.clone()),
    ));
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].kind, AlertKind::Whisper);
    filter.kind = Some(AlertKind::Whisper);
    assert_eq!(
        tauri::async_runtime::block_on(runtime.list_alerts(filter.clone())).unwrap_err(),
        history::INVALID
    );
    filter.kind = None;
    filter.kinds = vec![AlertKind::Whisper; 9];
    assert_eq!(
        tauri::async_runtime::block_on(runtime.list_alerts(filter.clone())).unwrap_err(),
        history::INVALID
    );
    filter.kinds.clear();
    filter.exclude_kinds = vec![AlertKind::KeyPopped; 2];
    assert_eq!(
        tauri::async_runtime::block_on(runtime.list_alerts(filter)).unwrap_err(),
        history::INVALID
    );
    for request in [
        AlertSummaryRequest {
            range: Some(TimeRange {
                start: Timestamp(2),
                end: Timestamp(1),
            }),
        },
        AlertSummaryRequest {
            range: Some(TimeRange {
                start: Timestamp(0),
                end: Timestamp(u64::MAX),
            }),
        },
    ] {
        assert_eq!(
            tauri::async_runtime::block_on(runtime.alert_summary(request)).unwrap_err(),
            history::INVALID
        );
    }
    runtime.reset_preview().unwrap();
    assert_eq!(
        ready(tauri::async_runtime::block_on(
            runtime.alert_summary(AlertSummaryRequest::default())
        ))
        .total,
        0
    );
}

fn count_opens(runtime: &CaptureRuntime) -> Arc<std::sync::atomic::AtomicUsize> {
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let opens = count.clone();
    *runtime.shared.home.on_open.lock().unwrap() = Some(Arc::new(move || {
        opens.fetch_add(1, Ordering::SeqCst);
    }));
    count
}

fn read_runs(runtime: &Arc<CaptureRuntime>) {
    assert!(matches!(
        tauri::async_runtime::block_on(runtime.list_runs(filter())).unwrap(),
        HistoryReply::Ready { .. }
    ));
}

#[test]
fn two_reads_in_one_generation_open_once() {
    let (_dir, runtime) = setup();
    let opens = count_opens(&runtime);
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = reads.clone();
    *runtime.shared.home.on_reader.lock().unwrap() = Some(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    read_runs(&runtime);
    read_runs(&runtime);
    assert_eq!(opens.load(Ordering::SeqCst), 1);
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[test]
fn invalidate_and_wait_closes_kept_connection() {
    let (_dir, runtime) = setup();
    read_runs(&runtime);
    assert!(runtime.shared.home.scheduler.lock().unwrap().open());
    runtime.shared.home.invalidate_and_wait().unwrap();
    assert!(!runtime.shared.home.scheduler.lock().unwrap().open());
}

#[test]
fn close_idle_reader_closes_without_retiring_reads() {
    let (_dir, runtime) = setup();
    let opens = count_opens(&runtime);
    read_runs(&runtime);
    assert!(runtime.shared.home.scheduler.lock().unwrap().open());
    runtime.shared.home.close_idle_reader();
    assert!(!runtime.shared.home.scheduler.lock().unwrap().open());
    read_runs(&runtime);
    assert_eq!(opens.load(Ordering::SeqCst), 2);
}

#[test]
fn shutdown_reads_closes_kept_connection() {
    let (_dir, runtime) = setup();
    read_runs(&runtime);
    assert!(runtime.shared.home.scheduler.lock().unwrap().open());
    runtime.shared.home.shutdown_reads();
    assert!(!runtime.shared.home.scheduler.lock().unwrap().open());
}

#[test]
fn playback_cleanup_can_remove_files_after_read() {
    let (dir, runtime) = setup();
    let path = dir.path().join("replay.sqlite3");
    seed(&path);
    {
        let mut context = runtime.shared.home.context.lock().unwrap();
        let context = context.as_mut().unwrap();
        context.namespace = Namespace::Replay;
        context.database = path.clone();
    }
    read_runs(&runtime);
    assert!(runtime.shared.home.scheduler.lock().unwrap().open());
    runtime.shared.home.invalidate_and_wait().unwrap();
    join::remove_playback_database(&path).unwrap();
    for suffix in ["", "-wal", "-shm"] {
        assert!(!dir.path().join(format!("replay.sqlite3{suffix}")).exists());
    }
}

#[test]
fn idle_close_after_reader_idle() {
    let (_dir, runtime) = setup();
    *runtime.shared.home.reader_idle.lock().unwrap() = Duration::from_millis(50);
    read_runs(&runtime);
    let until = Instant::now() + Duration::from_secs(2);
    while runtime.shared.home.scheduler.lock().unwrap().open() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn read_error_then_next_read_reopens() {
    let (_dir, runtime) = setup();
    let opens = count_opens(&runtime);
    read_runs(&runtime);
    *runtime.shared.home.on_reader.lock().unwrap() =
        Some(Arc::new(|| panic!("injected read panic")));
    assert_eq!(
        tauri::async_runtime::block_on(runtime.list_runs(filter())).unwrap_err(),
        history::WORKER
    );
    assert!(!runtime.shared.home.scheduler.lock().unwrap().open());
    *runtime.shared.home.on_reader.lock().unwrap() = None;
    read_runs(&runtime);
    assert_eq!(opens.load(Ordering::SeqCst), 2);
}

#[test]
fn newer_user_version_on_reuse_is_schema_newer() {
    let (dir, runtime) = setup();
    let opens = count_opens(&runtime);
    read_runs(&runtime);
    let db = rusqlite::Connection::open(dir.path().join("live.sqlite3")).unwrap();
    db.pragma_update(None, "user_version", 13).unwrap();
    assert_eq!(
        tauri::async_runtime::block_on(runtime.list_runs(filter())).unwrap_err(),
        home::SCHEMA_NEWER
    );
    assert_eq!(
        tauri::async_runtime::block_on(runtime.list_runs(filter())).unwrap_err(),
        home::SCHEMA_NEWER
    );
    assert_eq!(opens.load(Ordering::SeqCst), 1);
    assert!(!runtime.shared.home.scheduler.lock().unwrap().open());
}

#[test]
#[ignore]
fn reader_reuse_latency_bench() {
    for (mode, idle) in [
        ("reused", Duration::from_secs(60)),
        ("fresh", Duration::ZERO),
    ] {
        let (_dir, runtime) = setup();
        *runtime.shared.home.reader_idle.lock().unwrap() = idle;
        let tick = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let start = Instant::now();
        *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || {
            start + Duration::from_secs(tick.fetch_add(1, Ordering::SeqCst))
        }));
        let mut times = Vec::new();
        for _ in 0..200 {
            let at = Instant::now();
            read_runs(&runtime);
            times.push(at.elapsed());
        }
        times.sort();
        let median = (times[99] + times[100]) / 2;
        eprintln!("{mode}: median={median:?}, p95={:?}", times[189]);
        runtime.shared.home.shutdown_reads();
    }
}
