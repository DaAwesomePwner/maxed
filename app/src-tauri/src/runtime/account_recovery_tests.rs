use super::tests::context;
use super::*;
use maxed_types::*;
use std::time::Duration;

#[test]
fn account_scope_cancel_after_full_batch_notifies_and_resumes() {
    for retired in [None, Some(Namespace::Demo), Some(Namespace::Replay)] {
        cancel_and_resume(retired);
    }
}

fn cancel_and_resume(retired: Option<Namespace>) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let account = format!("account-{}", "a".repeat(64));
    let key = [7; 32];
    let mut db = rusqlite::Connection::open(&path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let mut events = vec![EngineEvent::CaptureStateChanged(CaptureStatus::default())];
    for number in [0, 4096, 8192, 65535] {
        let mut character = demo::demo_snapshot().character.unwrap();
        character.id = maxed_engine::character_id_for(&key, &account, number).unwrap();
        events.push(EngineEvent::CharacterUpdated(character));
    }
    let events: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(i, event)| EventEnvelope {
            event,
            stream_id: "seed".into(),
            sequence: Sequence(Timestamp(i as u64 + 1)),
            at: Timestamp(1),
            fight_chart_omitted: false,
        })
        .collect();
    maxed_store::apply_batch(&mut db, &events).unwrap();
    drop(db);
    let writer = Arc::new(
        maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()).unwrap(),
    );
    let home = Arc::new(HomeReads::default());
    *home.context.lock().unwrap() = Some(context(path.clone(), Namespace::Live));
    let (batch, reached) = crossbeam_channel::bounded(1);
    let (release, held) = crossbeam_channel::bounded(1);
    *home.on_recovery_batch.lock().unwrap() = Some(Arc::new(move |end| {
        batch.send(end).unwrap();
        held.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    let (notice, notices) = crossbeam_channel::unbounded();
    let sink: Arc<dyn Fn(AccountHistoryChanged) + Send + Sync> = Arc::new(move |value| {
        notice.send(value).unwrap();
    });
    let mut recovery = Recovery::start(
        home.clone(),
        writer.clone(),
        path.clone(),
        key,
        sink.clone(),
    )
    .unwrap();
    assert!(recovery.observe(&account));
    assert_eq!(reached.recv_timeout(Duration::from_secs(10)).unwrap(), 4095);
    if let Some(namespace) = retired {
        home.generation.fetch_add(1, Ordering::AcqRel);
        let mut playback = context(root.path().join("playback.sqlite3"), namespace);
        playback.generation = 1;
        *home.context.lock().unwrap() = Some(playback);
    }
    recovery.cancel.store(true, Ordering::Release);
    release.send(()).unwrap();
    recovery.stop();
    if retired.is_none() {
        assert!(matches!(
            notices.recv_timeout(Duration::from_secs(5)).unwrap().reason,
            AccountHistoryChangeReason::LegacyRecovered
        ));
    } else {
        assert_eq!(home.account_revision.load(Ordering::Acquire), 0);
        assert_eq!(
            home.context
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .account_revision,
            0
        );
    }
    assert!(notices.try_recv().is_err());
    assert_eq!(
        maxed_store::open_reader(&path)
            .unwrap()
            .recovery_progress(&account)
            .unwrap(),
        Some(4095)
    );
    *home.on_recovery_batch.lock().unwrap() = None;
    let (batch, reached) = crossbeam_channel::unbounded();
    *home.on_recovery_batch.lock().unwrap() = Some(Arc::new(move |end| {
        batch.send(end).unwrap();
    }));
    if retired.is_some() {
        let mut restored = context(path.clone(), Namespace::Live);
        restored.generation = home.generation.load(Ordering::Acquire);
        restored.account_revision = home.account_revision.load(Ordering::Acquire);
        *home.context.lock().unwrap() = Some(restored);
    }
    let mut resumed =
        Recovery::start(home.clone(), writer.clone(), path.clone(), key, sink).unwrap();
    assert!(resumed.observe(&account));
    assert!(matches!(
        notices
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .reason,
        AccountHistoryChangeReason::LegacyRecovered
    ));
    resumed.stop();
    let batches: Vec<_> = reached.try_iter().collect();
    assert_eq!(batches.first(), Some(&8191));
    assert_eq!(batches.last(), Some(&65535));
    assert_eq!(batches.len(), 15);
    assert_eq!(
        maxed_store::open_reader(&path)
            .unwrap()
            .recovery_progress(&account)
            .unwrap(),
        Some(65535)
    );
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM characters WHERE account_id=?1",
            [&account],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        4
    );
    drop(db);
    Arc::try_unwrap(writer).ok().unwrap().shutdown().unwrap();
    home.shutdown_reads();
}

#[test]
fn account_scope_failed_recovery_retries_only_on_account_change() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    let writer = Arc::new(
        maxed_store::start_writer(path.clone(), maxed_store::WriterConfig::default()).unwrap(),
    );
    let event = EventEnvelope {
        event: EngineEvent::CaptureStateChanged(CaptureStatus::default()),
        stream_id: maxed_store::WriterConfig::default().stream_id,
        sequence: Sequence(Timestamp(2)),
        at: Timestamp(1),
        fight_chart_omitted: false,
    };
    loop {
        match writer.try_submit(event.clone()) {
            Ok(()) => break,
            Err(maxed_store::SubmitError::Full(_)) => std::thread::yield_now(),
            Err(_) => panic!("unexpected submission failure"),
        }
    }
    assert!(writer.flush().is_err()); // Deliberately missing sequence 1.
    let home = Arc::new(HomeReads::default());
    *home.context.lock().unwrap() = Some(context(path.clone(), Namespace::Live));
    let (attempt, attempts) = crossbeam_channel::unbounded();
    *home.on_recovery_attempt.lock().unwrap() = Some(Arc::new(move || {
        attempt.send(()).unwrap();
    }));
    let mut recovery = Recovery::start(
        home.clone(),
        writer.clone(),
        path,
        [7; 32],
        Arc::new(|_| panic!("failed sweep attached nothing")),
    )
    .unwrap();
    let a = format!("account-{}", "a".repeat(64));
    let b = format!("account-{}", "b".repeat(64));
    assert!(recovery.observe(&a));
    attempts.recv_timeout(Duration::from_secs(5)).unwrap();
    for _ in 0..100 {
        assert!(!recovery.observe(&a));
    }
    assert!(attempts.try_recv().is_err());
    assert!(recovery.observe(&b));
    attempts.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(recovery.observe(&a));
    attempts.recv_timeout(Duration::from_secs(5)).unwrap();
    recovery.stop();
    assert!(Arc::try_unwrap(writer).ok().unwrap().shutdown().is_err());
    home.shutdown_reads();
}

#[test]
fn account_scope_retired_recovery_does_not_supersede_playback_or_new_live_reads() {
    for (namespace, generation) in [
        (Namespace::Demo, 0),
        (Namespace::Replay, 0),
        (Namespace::Live, 1),
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("current.sqlite3");
        maxed_store::migrate(&mut rusqlite::Connection::open(&path).unwrap()).unwrap();
        let runtime = Arc::new(CaptureRuntime::default());
        let home = runtime.shared.home.clone();
        home.preparation.lock().unwrap().ready = true;
        home.generation.store(generation, Ordering::Release);
        let mut current = context(path, namespace);
        current.generation = generation;
        *home.context.lock().unwrap() = Some(current);
        let (entered, waiting) = crossbeam_channel::bounded(1);
        let (release, held) = crossbeam_channel::bounded(1);
        *home.on_reader.lock().unwrap() = Some(Arc::new(move || {
            entered.send(()).unwrap();
            held.recv_timeout(Duration::from_secs(10)).unwrap();
        }));
        let reading = runtime.clone();
        let reader = std::thread::spawn(move || {
            tauri::async_runtime::block_on(reading.home_summary(HomeScope::Today {
                range: TimeRange {
                    start: Timestamp(0),
                    end: Timestamp(100),
                },
            }))
        });
        waiting.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(home.recovered(0).is_none());
        assert_eq!(home.account_revision.load(Ordering::Acquire), 0);
        release.send(()).unwrap();
        assert!(matches!(
            reader.join().unwrap().unwrap(),
            HistoryReply::Ready { .. }
        ));
        home.shutdown_reads();
    }
}
