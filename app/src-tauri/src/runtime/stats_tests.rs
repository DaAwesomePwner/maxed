//! Synthetic Stats history and CSV export through the shared native scheduler.
use super::history::{HistoryCaller, HistoryKey};
use super::stats::SaveDialog;
use super::*;
use maxed_types::*;
use std::{collections::HashMap, path::PathBuf, sync::Mutex};

fn all() -> StatsFilter {
    StatsFilter {
        range: None,
        character_id: None,
    }
}

fn setup() -> (tempfile::TempDir, Arc<CaptureRuntime>) {
    let dir = tempfile::tempdir().unwrap();
    let snapshot = demo::demo_snapshot();
    let session = snapshot.session.unwrap();
    let character = snapshot.character.unwrap();
    let start = session.started.0;
    let mut open = session.clone();
    open.ended = None;
    let mut closed = session;
    closed.last_traffic = Timestamp(start + 600_000_000);
    closed.ended = Some(closed.last_traffic);
    let events = [
        EngineEvent::SessionUpdated(open),
        EngineEvent::CharacterUpdated(character.clone()),
        EngineEvent::MapEntered {
            realm_instance_key: None,
            at: Timestamp(start),
            name: "Realm of the Mad God".into(),
            kind: AreaKind::Realm,
        },
        EngineEvent::FameChanged {
            at: Timestamp(start + 1),
            character_id: character.id.clone(),
            fame: 100,
        },
        EngineEvent::FameChanged {
            at: Timestamp(start + 300_000_000),
            character_id: character.id,
            fame: 160,
        },
        EngineEvent::SessionUpdated(closed),
    ];
    let events: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(i, event)| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic-stats".into(),
            sequence: Sequence(Timestamp(i as u64 + 1)),
            at: Timestamp(start),
            event,
        })
        .collect();
    let mut db = rusqlite::Connection::open(dir.path().join("live.sqlite3")).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    maxed_store::apply_batch(&mut db, &events).unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(dir.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    (dir, runtime)
}

fn ready<T: std::fmt::Debug>(reply: Result<HistoryReply<T>, String>) -> T {
    match reply.unwrap() {
        HistoryReply::Ready { value } => value,
        other => panic!("expected Ready, got {other:?}"),
    }
}

struct FakeDialog {
    choice: Option<PathBuf>,
    asked: Mutex<Vec<String>>,
}
impl SaveDialog for FakeDialog {
    fn choose(&self, suggested: String) -> tokio::sync::oneshot::Receiver<Option<PathBuf>> {
        self.asked.lock().unwrap().push(suggested);
        let (send, receive) = tokio::sync::oneshot::channel();
        let _ = send.send(self.choice.clone());
        receive
    }
}

fn export(
    runtime: &Arc<CaptureRuntime>,
    dialog: &FakeDialog,
    kind: StatsExportKind,
) -> Result<StatsExportResult, String> {
    let request = StatsExportRequest {
        kind,
        filter: all(),
        include_empty: true,
    };
    tauri::async_runtime::block_on(runtime.export_stats_csv(dialog, request, |_| {
        HashMap::from([(782, "Wizard".to_owned())])
    }))
}

#[test]
fn stats_reads_return_ready_on_seeded_store() {
    let (_dir, runtime) = setup();
    tauri::async_runtime::block_on(async {
        let maps = ready(runtime.stats_maps(all()).await);
        assert_eq!(
            (maps.rows[0].map.as_str(), maps.rows[0].fame),
            ("Realm", 60)
        );
        let sessions = ready(
            runtime
                .stats_sessions(StatsSessionFilter {
                    range: None,
                    character_id: None,
                    include_empty: false,
                    cursor: None,
                    limit: 50,
                })
                .await,
        );
        assert_eq!(sessions.rows.len(), 1);
        let id = sessions.rows[0].id.clone();
        let detail = ready(
            runtime
                .stats_session(StatsSessionRequest {
                    id,
                    character_id: None,
                })
                .await,
        );
        assert_eq!(detail.unwrap().maps.len(), 1);
        assert!(ready(runtime.stats_dungeons(all()).await).rows.is_empty());
    });
}

#[test]
fn stats_keys_reject_invalid_input() {
    let range = |start, end| {
        Some(TimeRange {
            start: Timestamp(start),
            end: Timestamp(end),
        })
    };
    let sessions = |limit, cursor| {
        HistoryKey::StatsSessions(StatsSessionFilter {
            range: None,
            character_id: None,
            include_empty: false,
            cursor,
            limit,
        })
    };
    let invalid = [
        sessions(0, None),
        sessions(101, None),
        sessions(
            50,
            Some(StatsSessionCursor {
                started: Timestamp(0),
                id: String::new(),
            }),
        ),
        sessions(
            50,
            Some(StatsSessionCursor {
                started: Timestamp(i64::MAX as u64 + 1),
                id: "s".into(),
            }),
        ),
        HistoryKey::StatsMaps(StatsFilter {
            range: range(2, 1),
            character_id: None,
        }),
        HistoryKey::StatsDungeons(StatsFilter {
            range: range(0, i64::MAX as u64 + 1),
            character_id: None,
        }),
        HistoryKey::StatsMaps(StatsFilter {
            range: None,
            character_id: Some(String::new()),
        }),
        HistoryKey::StatsSession(StatsSessionRequest {
            id: String::new(),
            character_id: None,
        }),
        HistoryKey::StatsExport(StatsExportRequest {
            kind: StatsExportKind::SessionRuns {
                id: "x\u{7}".into(),
            },
            filter: all(),
            include_empty: false,
        }),
    ];
    for key in invalid {
        assert!(key.validate_for_tests().is_err(), "{key:?}");
    }
    assert!(sessions(100, None).validate_for_tests().is_ok());
    assert!(HistoryKey::StatsMaps(StatsFilter {
        range: range(1, 2),
        character_id: Some("c".into())
    })
    .validate_for_tests()
    .is_ok());
}

#[test]
fn cancelled_export_writes_nothing() {
    let (dir, runtime) = setup();
    let dialog = FakeDialog {
        choice: None,
        asked: Mutex::new(vec![]),
    };
    assert_eq!(
        export(&runtime, &dialog, StatsExportKind::Maps),
        Ok(StatsExportResult::Cancelled)
    );
    let asked = dialog.asked.lock().unwrap();
    assert!(asked[0].starts_with("maxed-stats-maps-") && asked[0].ends_with(".csv"));
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|e| !e
        .unwrap()
        .path()
        .to_string_lossy()
        .ends_with(".csv")));
}

#[test]
fn saved_exports_report_rows_and_write_a_bom() {
    let (dir, runtime) = setup();
    for (kind, rows) in [
        (StatsExportKind::Maps, 1),
        (StatsExportKind::Sessions, 1),
        (StatsExportKind::Dungeons, 0),
    ] {
        let path = dir.path().join("export.csv");
        let dialog = FakeDialog {
            choice: Some(path.clone()),
            asked: Mutex::new(vec![]),
        };
        assert_eq!(
            export(&runtime, &dialog, kind),
            Ok(StatsExportResult::Saved { rows })
        );
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with("\u{feff}\"".as_bytes()));
        assert_eq!(
            String::from_utf8(bytes).unwrap().matches("\r\n").count(),
            rows as usize + 1
        );
    }
}

#[test]
fn invalid_export_fails_before_the_dialog_opens() {
    let (_dir, runtime) = setup();
    let dialog = FakeDialog {
        choice: None,
        asked: Mutex::new(vec![]),
    };
    assert!(export(
        &runtime,
        &dialog,
        StatsExportKind::SessionRuns { id: String::new() }
    )
    .is_err());
    assert!(dialog.asked.lock().unwrap().is_empty());
}

#[test]
fn unwritable_destination_returns_an_error_without_the_path() {
    let (dir, runtime) = setup();
    let path = dir.path().join("missing-folder").join("export.csv");
    let dialog = FakeDialog {
        choice: Some(path),
        asked: Mutex::new(vec![]),
    };
    let error = export(&runtime, &dialog, StatsExportKind::Maps).unwrap_err();
    assert!(!error.contains("missing-folder"), "{error}");
}

#[test]
fn webview_capabilities_are_unchanged() {
    let capabilities: serde_json::Value =
        serde_json::from_str(include_str!("../../capabilities/default.json")).unwrap();
    assert_eq!(
        capabilities["permissions"],
        serde_json::json!(["core:default"])
    );
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    assert_eq!(
        config["app"]["security"],
        serde_json::json!({"csp": "default-src 'self'; connect-src ipc: http://ipc.localhost; style-src 'self' 'unsafe-inline'; img-src 'self' data: http://maxed-sprite.localhost", "devCsp": "default-src 'self'; connect-src ipc: http://ipc.localhost http://127.0.0.1:1420 ws://127.0.0.1:1420; style-src 'self' 'unsafe-inline'; img-src 'self' data: http://maxed-sprite.localhost"})
    );
}

fn pending<F: std::future::Future>(future: std::pin::Pin<&mut F>) {
    assert!(future
        .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
        .is_pending());
}

#[test]
fn export_retries_superseded_reads_at_most_three_times() {
    for supersessions in [1, 2, 3] {
        let (dir, runtime) = setup();
        let path = dir.path().join("export.csv");
        let dialog = FakeDialog {
            choice: Some(path.clone()),
            asked: Mutex::new(vec![]),
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                let (release, wait) = crossbeam_channel::bounded(1);
                let mut active = Box::pin(runtime.submit_history(
                    HistoryCaller::NativeTest,
                    HistoryKey::Run("hold".into()),
                    move |_| {
                        wait.recv_timeout(std::time::Duration::from_secs(10))
                            .unwrap();
                        Ok(())
                    },
                ));
                pending(active.as_mut());
                let mut saving = Box::pin(runtime.export_stats_csv(
                    &dialog,
                    StatsExportRequest {
                        kind: StatsExportKind::Maps,
                        filter: all(),
                        include_empty: true,
                    },
                    |_| HashMap::new(),
                ));
                pending(saving.as_mut());
                let mut competing = None;
                for attempt in 0..supersessions {
                    let mut refresh = Box::pin(runtime.stats_dungeons(all()));
                    pending(refresh.as_mut());
                    competing = Some(refresh);
                    if attempt < 2 {
                        pending(saving.as_mut());
                        tokio::time::advance(std::time::Duration::from_millis(25)).await;
                        pending(saving.as_mut());
                    }
                }
                if supersessions == 3 {
                    assert_eq!(
                        saving.await.unwrap_err(),
                        "Saved history changed; export again."
                    );
                    assert!(!path.exists());
                } else {
                    release.send(()).unwrap();
                    ready(active.await);
                    assert_eq!(saving.await.unwrap(), StatsExportResult::Saved { rows: 1 });
                    let csv = std::fs::read_to_string(&path).unwrap();
                    assert!(csv.contains("Realm"));
                    assert_eq!(csv.matches("\r\n").count(), 2);
                    assert_eq!(dialog.asked.lock().unwrap().len(), 1);
                    return;
                }
                release.send(()).unwrap();
                ready(active.await);
                ready(competing.unwrap().await);
            });
    }
}

#[test]
fn export_retries_busy_reads_and_preserves_the_error_when_busy_persists() {
    for recovers in [false, true] {
        let (dir, runtime) = setup();
        let now = std::time::Instant::now();
        *runtime.shared.home.clock.lock().unwrap() = Some(Arc::new(move || now));
        for _ in 0..32 {
            ready(tauri::async_runtime::block_on(
                runtime.get_run("missing".into()),
            ));
        }
        let path = dir.path().join("export.csv");
        let dialog = FakeDialog {
            choice: Some(path.clone()),
            asked: Mutex::new(vec![]),
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
            .block_on(async {
                let mut saving = Box::pin(runtime.export_stats_csv(
                    &dialog,
                    StatsExportRequest {
                        kind: StatsExportKind::Maps,
                        filter: all(),
                        include_empty: true,
                    },
                    |_| HashMap::new(),
                ));
                pending(saving.as_mut());
                if recovers {
                    *runtime.shared.home.clock.lock().unwrap() =
                        Some(Arc::new(move || now + std::time::Duration::from_secs(1)));
                    assert_eq!(saving.await.unwrap(), StatsExportResult::Saved { rows: 1 });
                    assert!(path.exists());
                } else {
                    assert_eq!(
                        saving.await.unwrap_err(),
                        "Saved history is busy; export again."
                    );
                    assert!(!path.exists());
                }
                assert_eq!(dialog.asked.lock().unwrap().len(), 1);
            });
    }
}

#[test]
fn export_class_lookup_runs_off_the_async_thread() {
    let (dir, runtime) = setup();
    let dialog = FakeDialog {
        choice: Some(dir.path().join("party.csv")),
        asked: Mutex::new(vec![]),
    };
    let async_thread = std::thread::current().id();
    let (send, receive) = std::sync::mpsc::channel();
    let result = tauri::async_runtime::block_on(runtime.export_stats_csv(
        &dialog,
        StatsExportRequest {
            kind: StatsExportKind::SessionRuns {
                id: "missing".into(),
            },
            filter: all(),
            include_empty: true,
        },
        move |_| {
            send.send(std::thread::current().id()).unwrap();
            HashMap::new()
        },
    ));
    assert_eq!(result.unwrap(), StatsExportResult::Saved { rows: 0 });
    assert_ne!(receive.recv().unwrap(), async_thread);
}

#[test]
fn export_never_retries_after_history_invalidation() {
    for stopped in [false, true] {
        for during_backoff in [false, true] {
            let (dir, runtime) = setup();
            let path = dir.path().join("export.csv");
            let dialog = FakeDialog {
                choice: Some(path.clone()),
                asked: Mutex::new(vec![]),
            };
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .start_paused(true)
                .build()
                .unwrap()
                .block_on(async {
                    let (release, wait) = crossbeam_channel::bounded(1);
                    let mut active = Box::pin(runtime.submit_history(
                        HistoryCaller::NativeTest,
                        HistoryKey::Run("hold".into()),
                        move |_| {
                            wait.recv_timeout(std::time::Duration::from_secs(10))
                                .unwrap();
                            Ok(())
                        },
                    ));
                    pending(active.as_mut());
                    let mut saving = Box::pin(runtime.export_stats_csv(
                        &dialog,
                        StatsExportRequest {
                            kind: StatsExportKind::Maps,
                            filter: all(),
                            include_empty: true,
                        },
                        |_| panic!("invalidated export must not build a CSV"),
                    ));
                    pending(saving.as_mut());
                    let mut refresh = Box::pin(runtime.stats_dungeons(all()));
                    if during_backoff {
                        pending(refresh.as_mut());
                        pending(saving.as_mut());
                    }
                    let home = &runtime.shared.home;
                    let attempts = home.scheduler.lock().unwrap().counts().3;
                    let generation = home.generation.load(Ordering::Acquire);
                    let owner = runtime.clone();
                    let transition = std::thread::spawn(move || {
                        if stopped {
                            owner.shared.home.shutdown_reads();
                        } else {
                            owner.shared.home.invalidate_and_wait().unwrap();
                        }
                    });
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    loop {
                        let queue = home.scheduler.lock().unwrap();
                        if queue.stopped() || home.generation.load(Ordering::Acquire) != generation
                        {
                            break;
                        }
                        drop(queue);
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::yield_now();
                    }
                    if during_backoff {
                        tokio::time::advance(std::time::Duration::from_millis(25)).await;
                    }
                    assert_eq!(
                        std::future::Future::poll(
                            saving.as_mut(),
                            &mut std::task::Context::from_waker(std::task::Waker::noop())
                        ),
                        std::task::Poll::Ready(Err("Saved history changed; export again.".into()))
                    );
                    assert_eq!(home.scheduler.lock().unwrap().counts().3, attempts);
                    assert!(!path.exists());
                    release.send(()).unwrap();
                    assert_eq!(active.await.unwrap(), HistoryReply::Superseded);
                    transition.join().unwrap();
                });
        }
    }
}
