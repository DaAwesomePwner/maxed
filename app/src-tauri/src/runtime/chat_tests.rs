use super::*;
use maxed_types::*;

#[test]
fn history_keys_validate_bounds() {
    let filter = ChatFilter {
        channel: None,
        sender: None,
        query: String::new(),
        run_id: None,
        show_npc: false,
        show_hidden: false,
        cursor: None,
        limit: 100,
    };
    assert!(history::HistoryKey::Chat(filter.clone(), vec![], vec![])
        .validate_for_tests()
        .is_ok());
    for invalid in [
        ChatFilter {
            limit: 0,
            ..filter.clone()
        },
        ChatFilter {
            limit: 101,
            ..filter.clone()
        },
        ChatFilter {
            query: "x".repeat(129),
            ..filter.clone()
        },
        ChatFilter {
            query: "a\0".into(),
            ..filter.clone()
        },
        ChatFilter {
            sender: Some(String::new()),
            ..filter.clone()
        },
        ChatFilter {
            sender: Some("x".repeat(257)),
            ..filter.clone()
        },
        ChatFilter {
            run_id: Some("invalid\nrun".into()),
            ..filter.clone()
        },
        ChatFilter {
            cursor: Some(ChatCursor {
                at: Timestamp(u64::MAX),
                id: "chat-id".into(),
            }),
            ..filter.clone()
        },
        ChatFilter {
            cursor: Some(ChatCursor {
                at: Timestamp(0),
                id: String::new(),
            }),
            ..filter.clone()
        },
    ] {
        assert!(history::HistoryKey::Chat(invalid, vec![], vec![])
            .validate_for_tests()
            .is_err());
    }
    assert!(history::HistoryKey::Chat(
        ChatFilter {
            query: "🦀".repeat(128),
            ..filter.clone()
        },
        vec![],
        vec![],
    )
    .validate_for_tests()
    .is_ok());
    assert!(history::HistoryKey::Chat(
        filter.clone(),
        vec!["SynthAlpha".into(); MAX_HIDDEN_SENDERS + 1],
        vec![],
    )
    .validate_for_tests()
    .is_err());
    assert!(history::HistoryKey::Chat(
        filter,
        vec![],
        vec!["synthetic".into(); MAX_HIDDEN_KEYWORDS + 1],
    )
    .validate_for_tests()
    .is_err());
    let events = EventFilter {
        range: None,
        query: String::new(),
        opener_local: None,
        kinds: vec![],
        run_id: None,
        dungeon: None,
        cursor: None,
        limit: 50,
    };
    assert!(history::HistoryKey::Events(events.clone())
        .validate_for_tests()
        .is_ok());
    for invalid in [
        EventFilter {
            limit: 0,
            ..events.clone()
        },
        EventFilter {
            limit: 101,
            ..events.clone()
        },
        EventFilter {
            range: None,
            query: String::new(),
            opener_local: None,
            kinds: vec![InstanceEventTag::PlayerLeft; 5],
            ..events.clone()
        },
        EventFilter {
            dungeon: Some("x".repeat(257)),
            ..events.clone()
        },
        EventFilter {
            dungeon: Some(String::new()),
            ..events.clone()
        },
        EventFilter {
            run_id: Some(String::new()),
            ..events.clone()
        },
        EventFilter {
            cursor: Some(EventCursor {
                at: Timestamp(u64::MAX),
                id: "event-id".into(),
            }),
            ..events.clone()
        },
        EventFilter {
            cursor: Some(EventCursor {
                at: Timestamp(0),
                id: String::new(),
            }),
            ..events.clone()
        },
    ] {
        assert!(history::HistoryKey::Events(invalid)
            .validate_for_tests()
            .is_err());
    }
}

#[test]
fn retention_cutoff_saturates() {
    assert_eq!(
        chat::cutoff(Timestamp(5 * 86_400_000_000), 90),
        Timestamp(0)
    );
    assert_eq!(
        chat::cutoff(Timestamp(100 * 86_400_000_000), 90),
        Timestamp(10 * 86_400_000_000)
    );
}

fn chat_filter() -> ChatFilter {
    ChatFilter {
        channel: None,
        sender: None,
        query: String::new(),
        run_id: None,
        show_npc: false,
        show_hidden: false,
        cursor: None,
        limit: 100,
    }
}

fn seed_chat(path: &std::path::Path, count: u64, at: Timestamp) {
    let mut db = rusqlite::Connection::open(path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let session = maxed_types::demo::demo_snapshot().session.unwrap();
    maxed_store::apply_batch(
        &mut db,
        &[EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic-chat".into(),
            sequence: Sequence(Timestamp(1)),
            at,
            event: EngineEvent::SessionUpdated(session.clone()),
        }],
    )
    .unwrap();
    let tx = db.transaction().unwrap();
    for n in 0..count {
        let message = ChatMessageView {
            id: format!("synthetic-chat:chat:{n}"),
            session_id: session.id.clone(),
            run_id: None,
            map: None,
            at,
            channel: ChatChannel::Guild,
            direction: None,
            sender_kind: ChatSenderKind::Player,
            sender: "SynthAlpha".into(),
            counterpart: None,
            is_local: false,
            stars: None,
            supporter: false,
            text: "synthetic message".into(),
            truncated: false,
        };
        tx.execute("INSERT INTO chat_messages(id,session_id,at,channel,sender_kind,sender,is_local,text,json)
            VALUES(?1,?2,?3,'guild','player','SynthAlpha',0,'synthetic message',?4)",
            rusqlite::params![message.id, session.id, at.0 as i64, serde_json::to_string(&message).unwrap()]).unwrap();
    }
    tx.commit().unwrap();
}

fn chat_count(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .unwrap()
        .query_row("SELECT count(*) FROM chat_messages", [], |row| row.get(0))
        .unwrap()
}

// Keep a real session worker alive without draining, as when capture stalls.
// The normal runtime shutdown sends stop and joins this worker.
fn paused_capture_session(runtime: &CaptureRuntime) {
    let (stop, stopped) = bounded(1);
    let worker = std::thread::spawn(move || {
        let _ = stopped.recv();
    });
    *runtime.session.lock().unwrap() = Some(Session {
        stop,
        cancel: Arc::new(AtomicBool::new(false)),
        worker,
        sink: Arc::new(|_| {}),
    });
}

fn wait_for_chat_clear(runtime: &CaptureRuntime) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.shared.views.chat_clear.lock().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "clear did not reach the live adapter"
        );
        std::thread::yield_now();
    }
}

#[test]
fn timed_out_clear_is_withdrawn_and_next_clear_can_succeed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_chat(&path, 1, chat::wall_now());
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let mut adapter = RuntimeOptions::live(root.path().into(), None)
        .open_catalog(
            &AtomicBool::new(false),
            runtime.shared.views.clone(),
            Bridge::default(),
            None,
        )
        .unwrap();
    paused_capture_session(&runtime);

    assert_eq!(
        runtime
            .clear_chat_history_with_timeout(Duration::from_millis(10))
            .unwrap_err(),
        "Chat history could not be cleared."
    );
    assert!(runtime.shared.views.chat_clear.lock().unwrap().is_none());
    adapter.drain().unwrap();
    assert_eq!(
        chat_count(&path),
        1,
        "a failed clear must not run on a later drain"
    );

    let clearing = runtime.clone();
    let clear = std::thread::spawn(move || clearing.clear_chat_history());
    wait_for_chat_clear(&runtime);
    adapter.drain().unwrap();
    clear.join().unwrap().unwrap();
    assert_eq!(chat_count(&path), 0);
    adapter.close().unwrap();
}

#[test]
fn claimed_clear_waits_for_result_or_disconnect_and_preserves_a_different_request() {
    for (replace_slot, disconnect) in [(false, false), (true, false), (false, true)] {
        let root = tempfile::tempdir().unwrap();
        let runtime = Arc::new(CaptureRuntime::default());
        runtime.initialize_home(root.path().into()).unwrap();
        tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
        let _lease = storage::namespace_lease(root.path(), Namespace::Live).unwrap();
        paused_capture_session(&runtime);
        let clearing = runtime.clone();
        let (done, result) = bounded(1);
        let clear = std::thread::spawn(move || {
            done.send(clearing.clear_chat_history_with_timeout(Duration::from_millis(100)))
                .unwrap();
        });
        // Claim and replace under the same lock as timeout cleanup.
        wait_for_chat_clear(&runtime);
        let mut pending = runtime.shared.views.chat_clear.lock().unwrap();
        let claimed = pending.take().unwrap();
        let (other, _other_result) = bounded(1);
        if replace_slot {
            *pending = Some(other.clone());
        }
        drop(pending);
        assert!(matches!(
            result.recv_timeout(Duration::from_millis(200)),
            Err(crossbeam_channel::RecvTimeoutError::Timeout)
        ));
        if disconnect {
            drop(claimed);
        } else {
            claimed.send(Ok(())).unwrap();
        }
        assert_eq!(
            result.recv_timeout(Duration::from_secs(5)).unwrap(),
            if disconnect {
                Err("Chat history could not be cleared.".into())
            } else {
                Ok(())
            }
        );
        clear.join().unwrap();
        let pending = runtime.shared.views.chat_clear.lock().unwrap().take();
        if replace_slot {
            assert!(pending.unwrap().same_channel(&other));
        } else {
            assert!(pending.is_none());
        }
    }
}

#[test]
fn unavailable_lease_without_running_capture_fails_without_queuing() {
    for parked in [None, Some(false), Some(true)] {
        let root = tempfile::tempdir().unwrap();
        let runtime = Arc::new(CaptureRuntime::default());
        runtime.initialize_home(root.path().into()).unwrap();
        tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
        let mut lease = None;
        if let Some(recovery) = parked {
            let adapter = RuntimeOptions::live(root.path().into(), None)
                .open_catalog(
                    &AtomicBool::new(false),
                    runtime.shared.views.clone(),
                    Bridge::default(),
                    None,
                )
                .unwrap();
            if recovery {
                *runtime.shared.recovery.lock().unwrap() = Some(adapter);
            } else {
                *runtime.shared.completed.lock().unwrap() = Some(adapter);
            }
            paused_capture_session(&runtime);
            runtime
                .session
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .stop
                .send(())
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while runtime.is_running() {
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
        } else {
            // The lease alone models cold preparation owning the live file.
            lease = Some(storage::namespace_lease(root.path(), Namespace::Live).unwrap());
        }
        let clearing = runtime.clone();
        let (done, result) = bounded(1);
        let clear = std::thread::spawn(move || {
            done.send(clearing.clear_chat_history()).unwrap();
        });
        assert_eq!(
            result
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap_err(),
            "Chat history could not be cleared."
        );
        clear.join().unwrap();
        assert!(runtime.shared.views.chat_clear.lock().unwrap().is_none());
        drop(lease);
        if let Some(mut adapter) = runtime.shared.recovery.lock().unwrap().take() {
            adapter.close().unwrap();
        };
    }
}

#[test]
fn chat_reads_use_current_hidden_policy_and_clear_cold_live_history() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_chat(&path, 2, Timestamp(1));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let read = |filter| match tauri::async_runtime::block_on(runtime.list_chat(filter)).unwrap() {
        HistoryReply::Ready { value } => value.rows.len(),
        _ => panic!("expected ready chat history"),
    };
    assert_eq!(read(chat_filter()), 2);
    runtime
        .shared
        .views
        .chat_policy
        .lock()
        .unwrap()
        .hidden_senders = vec!["sYnThAlPhA".into()];
    assert_eq!(read(chat_filter()), 0);
    assert_eq!(
        read(ChatFilter {
            show_hidden: true,
            ..chat_filter()
        }),
        2
    );
    for namespace in [Namespace::Demo, Namespace::Replay] {
        runtime.shared.alerts.lock().unwrap().namespace = namespace;
        assert!(runtime.clear_chat_history().is_err());
        assert_eq!(chat_count(&path), 2);
    }
    runtime.shared.alerts.lock().unwrap().namespace = Namespace::Live;
    runtime.clear_chat_history().unwrap();
    assert_eq!(chat_count(&path), 0);
}

#[test]
fn live_drain_prunes_two_batches_then_continues_and_services_clear() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_chat(&path, 2501, Timestamp(1));
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let mut adapter = RuntimeOptions::live(root.path().into(), None)
        .open_catalog(
            &AtomicBool::new(false),
            runtime.shared.views.clone(),
            Bridge::default(),
            None,
        )
        .unwrap();
    adapter.drain().unwrap();
    assert_eq!(chat_count(&path), 501);
    adapter.drain().unwrap();
    assert_eq!(chat_count(&path), 0);
    // A completed pass schedules the next prune for an hour later.
    seed_chat(&path, 5001, Timestamp(1));
    adapter.drain().unwrap();
    assert_eq!(chat_count(&path), 5001);
    paused_capture_session(&runtime);
    let clearing = runtime.clone();
    let clear = std::thread::spawn(move || clearing.clear_chat_history());
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.shared.views.chat_clear.lock().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "clear did not reach the live adapter"
        );
        std::thread::yield_now();
    }
    // Playback must neither consume the live request nor prune its own rows.
    adapter.live = false;
    adapter.drain().unwrap();
    assert_eq!(chat_count(&path), 5001);
    assert!(runtime.shared.views.chat_clear.lock().unwrap().is_some());
    adapter.live = true;
    adapter.drain().unwrap();
    clear.join().unwrap().unwrap();
    assert_eq!(chat_count(&path), 0);
    adapter.close().unwrap();
}

#[test]
fn invalid_chat_settings_are_rejected_before_any_write() {
    let runtime = CaptureRuntime::default();
    let bad = ChatSettings {
        retention_days: 0,
        ..ChatSettings::default()
    };
    assert_eq!(
        runtime.update_chat_settings(bad).unwrap_err(),
        crate::settings::SETTINGS_INVALID
    );
}

#[test]
fn chat_context_reflects_policy() {
    let policy = ChatSettings {
        store: ChatStorage {
            guild: false,
            ..ChatStorage::default()
        },
        hidden_senders: vec!["SynthAlpha".into()],
        ..ChatSettings::default()
    };
    let guild = |sender: &str| ChatMessageView {
        id: "live-1:chat:1".into(),
        session_id: "s".into(),
        run_id: None,
        map: None,
        at: Timestamp(1),
        channel: ChatChannel::Guild,
        direction: None,
        sender_kind: ChatSenderKind::Player,
        sender: sender.into(),
        counterpart: None,
        is_local: false,
        stars: None,
        supporter: false,
        text: "x".into(),
        truncated: false,
    };
    for sender in ["SynthAlpha", "sYnThAlPhA"] {
        assert_eq!(
            chat::context(&policy, &guild(sender), true),
            maxed_store::ChatContext {
                store: false,
                hidden: true,
                alertable: true,
            }
        );
    }
    assert_eq!(
        chat::context(&policy, &guild("SynthBravo"), false),
        maxed_store::ChatContext {
            store: false,
            hidden: false,
            alertable: false,
        }
    );
}

#[test]
fn chat_settings_publish_only_after_a_successful_live_save() {
    let root = tempfile::tempdir().unwrap();
    let runtime = CaptureRuntime::default();
    let store = Arc::new(crate::settings::SettingsStore::new(root.path().into()).unwrap());
    runtime.configure_alert_settings(store.clone()).unwrap();
    let settings = ChatSettings {
        retention_days: 30,
        ..Default::default()
    };
    runtime.update_chat_settings(settings.clone()).unwrap();
    assert_eq!(store.get().unwrap().chat, settings);
    assert_eq!(*runtime.shared.views.chat_policy.lock().unwrap(), settings);

    for namespace in [Namespace::Demo, Namespace::Replay] {
        runtime.shared.alerts.lock().unwrap().namespace = namespace;
        assert_eq!(
            runtime
                .update_chat_settings(ChatSettings::default())
                .unwrap_err(),
            "Chat settings cannot be changed during preview or replay."
        );
        assert_eq!(store.get().unwrap().chat, settings);
        assert_eq!(*runtime.shared.views.chat_policy.lock().unwrap(), settings);
    }
    runtime.shared.alerts.lock().unwrap().namespace = Namespace::Live;
    std::fs::write(root.path().join("settings.json"), b"invalid settings").unwrap();
    assert_eq!(
        runtime
            .update_chat_settings(ChatSettings::default())
            .unwrap_err(),
        crate::settings::SETTINGS_ERROR
    );
    assert_eq!(*runtime.shared.views.chat_policy.lock().unwrap(), settings);
}

#[test]
fn cold_chat_reads_load_saved_hide_lists_before_any_adapter() {
    for corrupt in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("live.sqlite3");
        seed_chat(&path, 2, chat::wall_now());
        let settings = Arc::new(crate::settings::SettingsStore::new(root.path().into()).unwrap());
        settings
            .put_chat(ChatSettings {
                hidden_senders: vec!["sYnThAlPhA".into()],
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
        assert!(!runtime.is_running());
        let HistoryReply::Ready { value } =
            tauri::async_runtime::block_on(runtime.list_chat(chat_filter())).unwrap()
        else {
            panic!("expected ready chat history");
        };
        assert_eq!(value.rows.len(), if corrupt { 2 } else { 0 });
        let HistoryReply::Ready { value } =
            tauri::async_runtime::block_on(runtime.list_chat(ChatFilter {
                show_hidden: true,
                ..chat_filter()
            }))
            .unwrap()
        else {
            panic!("expected ready chat history");
        };
        assert_eq!(value.rows.len(), 2);
        assert!(value.rows.iter().all(|row| row.hidden != corrupt));
    }
}

#[test]
fn capture_tick_services_clear_before_the_first_frame() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_chat(&path, 1, chat::wall_now());
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let mut adapter = RuntimeOptions::live(root.path().into(), None)
        .open_catalog(
            &AtomicBool::new(false),
            runtime.shared.views.clone(),
            Bridge::default(),
            None,
        )
        .unwrap();
    paused_capture_session(&runtime);
    let clearing = runtime.clone();
    let clear = std::thread::spawn(move || clearing.clear_chat_history());
    wait_for_chat_clear(&runtime);
    let clock = CaptureClock::default();
    assert!(clock.tick(Instant::now()).is_none());
    let at = adapter.at();
    clock.service_tick(&mut adapter, Instant::now()).unwrap();
    clear.join().unwrap().unwrap();
    assert_eq!(chat_count(&path), 0);
    assert_eq!(
        adapter.at(),
        at,
        "maintenance must not advance capture time"
    );
    adapter.close().unwrap();
}

#[test]
fn cold_prune_stops_after_twenty_batches_and_live_maintenance_finishes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_chat(&path, 20_001, Timestamp(1));
    let settings = Arc::new(crate::settings::SettingsStore::new(root.path().into()).unwrap());
    settings.put_chat(ChatSettings::default()).unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.configure_alert_settings(settings).unwrap();
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    assert_eq!(chat_count(&path), 1);
    let mut adapter = RuntimeOptions::live(root.path().into(), None)
        .open_catalog(
            &AtomicBool::new(false),
            runtime.shared.views.clone(),
            Bridge::default(),
            None,
        )
        .unwrap();
    adapter.drain().unwrap();
    assert_eq!(chat_count(&path), 0);
    adapter.close().unwrap();
}

#[test]
fn chat_summary_keys_repeat_store_bounds() {
    use history::HistoryKey;
    for name in [
        String::new(),
        "x".repeat(33),
        "bad\nname".into(),
        "é".repeat(17),
    ] {
        assert!(HistoryKey::ChatSender(ChatSenderRequest { name })
            .validate_for_tests()
            .is_err());
    }
    assert!(HistoryKey::ChatSender(ChatSenderRequest {
        name: "x".repeat(32)
    })
    .validate_for_tests()
    .is_ok());
    for run_id in [String::new(), "x".repeat(257), "bad\nrun".into()] {
        assert!(HistoryKey::ChatOverview(ChatOverviewRequest {
            run_id: Some(run_id)
        })
        .validate_for_tests()
        .is_err());
    }
    assert!(
        HistoryKey::ChatOverview(ChatOverviewRequest { run_id: None })
            .validate_for_tests()
            .is_ok()
    );
    let base = WhisperPartnerFilter {
        query: String::new(),
        show_hidden: false,
        limit: 200,
    };
    for limit in [0, 201] {
        assert!(HistoryKey::WhisperPartners(
            WhisperPartnerFilter {
                limit,
                ..base.clone()
            },
            vec![],
            vec![]
        )
        .validate_for_tests()
        .is_err());
    }
    for query in ["x".repeat(129), "🦀".repeat(129), "bad\0query".into()] {
        assert!(HistoryKey::WhisperPartners(
            WhisperPartnerFilter {
                query,
                ..base.clone()
            },
            vec![],
            vec![]
        )
        .validate_for_tests()
        .is_err());
    }
    assert!(HistoryKey::WhisperPartners(
        WhisperPartnerFilter {
            query: "🦀".repeat(128),
            ..base.clone()
        },
        vec![],
        vec![]
    )
    .validate_for_tests()
    .is_ok());
    for (senders, keywords) in [
        (vec!["x".into(); MAX_HIDDEN_SENDERS + 1], vec![]),
        (vec!["x".repeat(33)], vec![]),
        (vec![], vec!["x".into(); MAX_HIDDEN_KEYWORDS + 1]),
        (vec![], vec!["bad\nkeyword".into()]),
    ] {
        assert!(HistoryKey::WhisperPartners(base.clone(), senders, keywords)
            .validate_for_tests()
            .is_err());
    }
}

#[test]
fn chat_summary_reads_use_replay_namespace_and_current_hidden_policy() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let mut options = RuntimeOptions::live(root.path().into(), None);
    options.namespace = Namespace::Replay;
    runtime
        .shared
        .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
        .unwrap()
        .close()
        .unwrap();
    seed_chat(&options.database(), 2, Timestamp(1));
    let db = rusqlite::Connection::open(options.database()).unwrap();
    db.execute(
        "UPDATE chat_messages SET channel='whisper',json=json_set(json,'$.channel','whisper')",
        [],
    )
    .unwrap();
    drop(db);
    tauri::async_runtime::block_on(async {
        let HistoryReply::Ready { value } = runtime
            .get_chat_overview(ChatOverviewRequest { run_id: None })
            .await
            .unwrap()
        else {
            panic!("expected ready overview");
        };
        assert_eq!(value.lines, 2);
        assert_eq!(value.by_channel.whisper, 2);
        let HistoryReply::Ready { value } = runtime
            .get_chat_sender(ChatSenderRequest {
                name: "synthalpha".into(),
            })
            .await
            .unwrap()
        else {
            panic!("expected ready sender");
        };
        assert_eq!(value.unwrap().lines, 2);
        let filter = WhisperPartnerFilter {
            query: String::new(),
            show_hidden: false,
            limit: 200,
        };
        let HistoryReply::Ready { value } =
            runtime.list_whisper_partners(filter.clone()).await.unwrap()
        else {
            panic!("expected ready partners");
        };
        assert_eq!((value.people, value.lines), (1, 2));
        runtime
            .shared
            .views
            .chat_policy
            .lock()
            .unwrap()
            .hidden_senders = vec!["SYNTHALPHA".into()];
        let HistoryReply::Ready { value } =
            runtime.list_whisper_partners(filter.clone()).await.unwrap()
        else {
            panic!("expected ready hidden partners");
        };
        assert!(value.rows.is_empty());
        let HistoryReply::Ready { value } = runtime
            .list_whisper_partners(WhisperPartnerFilter {
                show_hidden: true,
                ..filter.clone()
            })
            .await
            .unwrap()
        else {
            panic!("expected ready shown partners");
        };
        assert_eq!(value.lines, 2);
        assert_eq!(
            runtime
                .get_chat_sender(ChatSenderRequest {
                    name: "x".repeat(33)
                })
                .await
                .unwrap_err(),
            history::INVALID
        );
        assert_eq!(
            runtime
                .get_chat_overview(ChatOverviewRequest {
                    run_id: Some(String::new())
                })
                .await
                .unwrap_err(),
            history::INVALID
        );
        assert_eq!(
            runtime
                .list_whisper_partners(WhisperPartnerFilter {
                    limit: 201,
                    ..filter
                })
                .await
                .unwrap_err(),
            history::INVALID
        );
    });
}

fn event_filter() -> EventFilter {
    EventFilter {
        range: None,
        query: String::new(),
        opener_local: None,
        kinds: vec![],
        run_id: None,
        dungeon: None,
        cursor: None,
        limit: 100,
    }
}

#[test]
fn event_history_keys_validate_scope_and_summary_ignores_presentation() {
    for filter in [
        EventFilter {
            query: "x".repeat(129),
            ..event_filter()
        },
        EventFilter {
            query: "x\0".into(),
            ..event_filter()
        },
        EventFilter {
            query: "🦀".repeat(129),
            ..event_filter()
        },
        EventFilter {
            range: Some(TimeRange {
                start: Timestamp(2),
                end: Timestamp(2),
            }),
            ..event_filter()
        },
        EventFilter {
            range: Some(TimeRange {
                start: Timestamp(0),
                end: Timestamp(u64::MAX),
            }),
            ..event_filter()
        },
        EventFilter {
            run_id: Some("bad\nrun".into()),
            ..event_filter()
        },
    ] {
        assert!(history::HistoryKey::Events(filter.clone())
            .validate_for_tests()
            .is_err());
        assert!(
            history::HistoryKey::EventSummary(EventSummaryRequest { filter })
                .validate_for_tests()
                .is_err()
        );
    }
    assert!(history::HistoryKey::EventSummary(EventSummaryRequest {
        filter: EventFilter {
            query: "🦀".repeat(128),
            limit: 0,
            kinds: vec![InstanceEventTag::PlayerDeath; 5],
            dungeon: Some(String::new()),
            cursor: Some(EventCursor {
                at: Timestamp(u64::MAX),
                id: String::new()
            }),
            ..event_filter()
        }
    })
    .validate_for_tests()
    .is_ok());
}

#[test]
fn event_reads_use_replay_namespace_and_return_flags_summary_and_matches() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    let mut options = RuntimeOptions::live(root.path().into(), None);
    options.namespace = Namespace::Replay;
    runtime
        .shared
        .open_home(&options, &AtomicBool::new(false), Bridge::default(), None)
        .unwrap()
        .close()
        .unwrap();
    seed_chat(&options.database(), 0, Timestamp(1));
    let mut db = rusqlite::Connection::open(options.database()).unwrap();
    let mut run = demo::demo_run_story().run;
    run.character_id = None;
    run.started = Timestamp(180_000_000);
    run.ended = None;
    run.completed_at = None;
    run.died_at = None;
    run.outcome = None;
    run.evidence.clear();
    let portal = InstanceEventView {
        id: "replay-portal".into(),
        session_id: run.session_id.clone(),
        run_id: None,
        map: Some("Nexus".into()),
        at: Timestamp(10),
        provisional: true,
        kind: InstanceEventKind::PortalOpened {
            opener: Some("Vexlin".into()),
            opener_is_local: Some(true),
            dungeon: Some(run.dungeon.clone()),
            portal_type: None,
        },
    };
    let events = [
        EngineEvent::SessionUpdated(demo::demo_snapshot().session.unwrap()),
        EngineEvent::RunStarted(run.clone()),
        EngineEvent::InstanceEvent(portal.clone()),
    ];
    let envelopes: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(n, event)| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic-events".into(),
            sequence: Sequence(Timestamp(n as u64 + 1)),
            at: Timestamp(180_000_000),
            event,
        })
        .collect();
    maxed_store::apply_batch(&mut db, &envelopes).unwrap();
    drop(db);
    tauri::async_runtime::block_on(async {
        let filter = EventFilter {
            range: Some(TimeRange {
                start: Timestamp(10),
                end: Timestamp(11),
            }),
            query: "vex".into(),
            opener_local: Some(true),
            ..event_filter()
        };
        let HistoryReply::Ready { value: page } =
            runtime.list_instance_events(filter.clone()).await.unwrap()
        else {
            panic!("ready events")
        };
        assert_eq!(page.rows, [portal]);
        assert_eq!(page.entered_runs.len(), 1);
        assert_eq!(page.entered_runs[0].run_id, run.id);
        let HistoryReply::Ready { value } = runtime
            .get_event_summary(EventSummaryRequest { filter })
            .await
            .unwrap()
        else {
            panic!("ready summary")
        };
        assert_eq!(value.total, 1);
        assert_eq!(value.local_pops, 1);
        assert_eq!(value.flagged_since, Some(Timestamp(10)));
        assert_eq!(value.last_local_pop.unwrap().dungeon, run.dungeon);
        assert_eq!(
            runtime
                .get_event_summary(EventSummaryRequest {
                    filter: EventFilter {
                        query: "x".repeat(129),
                        ..event_filter()
                    }
                })
                .await
                .unwrap_err(),
            history::INVALID
        );
    });
}

#[test]
fn stopped_chat_clear_waits_for_maintenance_lease() {
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_chat(&path, 1, chat::wall_now());
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    super::chat_tests::with_stopped_maintenance(&runtime, || {
        let timeout = Duration::from_secs(5);
        let started = Instant::now();
        std::thread::scope(|scope| {
            let (send, recv) = crossbeam_channel::bounded(1);
            let runtime = &runtime;
            scope.spawn(move || {
                send.send(runtime.clear_chat_history_with_timeout(timeout))
                    .unwrap();
            });
            assert_eq!(
                recv.recv_timeout(Duration::from_millis(50)),
                Err(crossbeam_channel::RecvTimeoutError::Timeout)
            );
            runtime.shared.views.maintenance.handoff_background();
            recv.recv_timeout(timeout).unwrap().unwrap();
        });
        assert!(started.elapsed() < timeout);
    });
    assert_eq!(chat_count(&path), 0);
    assert!(runtime.shared.views.chat_clear.lock().unwrap().is_none());
}

#[test]
fn stopped_chat_clear_lease_wait_expires() {
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_chat(&path, 1, chat::wall_now());
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    super::chat_tests::with_stopped_maintenance(&runtime, || {
        let timeout = Duration::from_millis(100);
        let started = Instant::now();
        assert_eq!(
            runtime.clear_chat_history_with_timeout(timeout),
            Err("Chat history could not be cleared.".into())
        );
        assert!(started.elapsed() >= timeout);
        assert!(started.elapsed() < Duration::from_secs(2));
    });
    assert_eq!(chat_count(&path), 1);
    assert!(runtime.shared.views.chat_clear.lock().unwrap().is_none());
}

// Hold the actual background owner before it services Compact. The estimate
// gate is try-locked by maintenance, so handoff remains able to stop the worker.
pub(super) fn with_stopped_maintenance(runtime: &Arc<CaptureRuntime>, test: impl FnOnce()) {
    struct Stop<'a>(&'a maintenance::StorageService);
    impl Drop for Stop<'_> {
        fn drop(&mut self) {
            self.0.handoff_background();
            self.0.defer_requests();
        }
    }
    // Cold history initialization deliberately leaves a missing database absent.
    let path = runtime.shared.home.live_database().unwrap();
    let mut db = rusqlite::Connection::open(path).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    drop(db);
    let service = &runtime.shared.views.maintenance;
    let _gate = service.estimate_gate.lock().unwrap();
    std::thread::scope(|scope| {
        let command = scope.spawn(|| runtime.compact_storage());
        let _stop = Stop(service);
        super::maintenance_tests::eventually(|| service.owns_stopped_namespace());
        test();
        service.handoff_background();
        service.defer_requests();
        command.join().unwrap().unwrap();
    });
}

#[test]
fn chat_clear_does_not_wait_for_maintenance_waiting_behind_another_owner() {
    let root = tempfile::tempdir().unwrap();
    let runtime = super::maintenance_tests::initialized(root.path());
    let _lease = storage::namespace_lease(root.path(), Namespace::Live).unwrap();
    assert_eq!(
        runtime.compact_storage().unwrap(),
        StorageCommandOutcome::Queued
    );
    let service = &runtime.shared.views.maintenance;
    assert!(service.has_background());
    assert!(!service.owns_stopped_namespace());
    let started = Instant::now();
    let result = runtime.clear_chat_history();
    service.handoff_background();
    assert_eq!(result, Err("Chat history could not be cleared.".into()));
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn chat_clear_queues_when_capture_starts_during_maintenance_wait() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("live.sqlite3");
    seed_chat(&path, 1, chat::wall_now());
    let runtime = Arc::new(CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    with_stopped_maintenance(&runtime, || {
        std::thread::scope(|scope| {
            let (send, recv) = bounded(1);
            let runtime = &runtime;
            scope.spawn(move || send.send(runtime.clear_chat_history()).unwrap());
            assert_eq!(
                recv.recv_timeout(Duration::from_millis(50)),
                Err(crossbeam_channel::RecvTimeoutError::Timeout)
            );
            paused_capture_session(runtime);
            wait_for_chat_clear(runtime);
            runtime.shared.views.maintenance.handoff_background();
            let mut adapter = RuntimeOptions::live(root.path().into(), None)
                .open_catalog(
                    &AtomicBool::new(false),
                    runtime.shared.views.clone(),
                    Bridge::default(),
                    None,
                )
                .unwrap();
            adapter.drain().unwrap();
            recv.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
            adapter.close().unwrap();
        });
    });
    assert_eq!(chat_count(&path), 0);
}

#[test]
fn stopped_chat_clear_does_not_retry_permanent_directory_failure() {
    let root = tempfile::tempdir().unwrap();
    let runtime = super::maintenance_tests::initialized(root.path());
    with_stopped_maintenance(&runtime, || {
        let file = root.path().join("not-a-directory");
        std::fs::write(&file, b"synthetic").unwrap();
        let started = Instant::now();
        let mut timeout = Duration::from_secs(60);
        assert!(matches!(
            chat::history_write_lease(&runtime, &file, &mut timeout),
            Err(storage::LeaseError::Permanent(
                "Could not prepare local summary storage"
            ))
        ));
        assert!(started.elapsed() < Duration::from_secs(5));
    });
}
