use super::party::{resolve_preset, PartyCatalog, PartyRoster, RosterContext};
use maxed_types::*;
use std::sync::Arc;

#[test]
fn realmeye_url_validates_names() {
    assert_eq!(
        super::party::realmeye_url("SynthAlpha").as_deref(),
        Some("https://www.realmeye.com/player/SynthAlpha")
    );
    for name in [
        "Synth Alpha",
        "SynthAlpha,fe14",
        "SynthAlphaa",
        "",
        "../x",
        "SynthÄ",
        "Synth1",
    ] {
        assert_eq!(super::party::realmeye_url(name), None);
    }
}

fn command_runtime() -> (tempfile::TempDir, Arc<super::CaptureRuntime>) {
    let root = tempfile::tempdir().unwrap();
    let runtime = Arc::new(super::CaptureRuntime::default());
    runtime.initialize_home(root.path().into()).unwrap();
    runtime
        .configure_alert_settings(Arc::new(
            crate::settings::SettingsStore::new(root.path().into()).unwrap(),
        ))
        .unwrap();
    runtime.update_party_settings(settings()).unwrap();
    tauri::async_runtime::block_on(runtime.ensure_history_ready()).unwrap();
    (root, runtime)
}

fn ready_party<T>(reply: HistoryReply<T>) -> T {
    match reply {
        HistoryReply::Ready { value } => value,
        _ => panic!("history not ready"),
    }
}

#[test]
fn review_party_writes_before_first_frame() {
    use super::party::{PartyWrite, PartyWriteRequest};
    use std::time::{Duration, Instant};
    let (root, runtime) = command_runtime();
    let mut adapter = super::RuntimeOptions::live(root.path().into(), None)
        .open(
            &Default::default(),
            runtime.shared.views.clone(),
            super::Bridge::default(),
        )
        .unwrap();
    let clock = super::CaptureClock::default();
    let at = adapter.at();
    for operation in [
        PartyWrite::Mark {
            key: "synthalpha".into(),
            name: "SynthAlpha".into(),
            mark: Some(PartyMark::Watched),
            note: None,
        },
        PartyWrite::Clear(ClearPartyHistory {
            marks_and_notes: true,
        }),
    ] {
        let clearing = matches!(operation, PartyWrite::Clear(_));
        let (reply, receive) = crossbeam_channel::bounded(1);
        *runtime.shared.views.party_write.lock().unwrap() =
            Some(PartyWriteRequest { operation, reply });
        clock.service_tick(&mut adapter, Instant::now()).unwrap();
        receive
            .recv_timeout(Duration::from_millis(300))
            .unwrap()
            .unwrap();
        let player = maxed_store::open_reader(&root.path().join("live.sqlite3"))
            .unwrap()
            .get_party_player("synthalpha", None)
            .unwrap();
        assert_eq!(player.is_none(), clearing);
        assert_eq!(adapter.at(), at);
    }
    adapter.close().unwrap();
}

#[test]
fn cold_party_write_rejects_pending_adapter_request() {
    use super::party::{PartyWrite, PartyWriteRequest, WRITE_FAILED};
    let (root, runtime) = command_runtime();
    let (reply, receive) = crossbeam_channel::bounded(1);
    *runtime.shared.views.party_write.lock().unwrap() = Some(PartyWriteRequest {
        operation: PartyWrite::Clear(ClearPartyHistory {
            marks_and_notes: true,
        }),
        reply,
    });
    runtime
        .set_party_mark(
            "synthalpha".into(),
            "SynthAlpha".into(),
            Some(PartyMark::Watched),
            None,
        )
        .unwrap();
    assert_eq!(
        receive
            .recv_timeout(std::time::Duration::from_millis(100))
            .unwrap(),
        Err(WRITE_FAILED.into())
    );
    assert!(runtime.shared.views.party_write.lock().unwrap().is_none());
    let mut adapter = super::RuntimeOptions::live(root.path().into(), None)
        .open(
            &Default::default(),
            runtime.shared.views.clone(),
            super::Bridge::default(),
        )
        .unwrap();
    adapter.drain().unwrap();
    let player = maxed_store::open_reader(&root.path().join("live.sqlite3"))
        .unwrap()
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert_eq!(player.row.mark, Some(PartyMark::Watched));
    adapter.close().unwrap();
}

#[test]
fn adapter_reuses_prepared_party_catalog_and_clears_missing_catalog() {
    let (root, runtime) = command_runtime();
    let source = Arc::new(catalog());
    let open = |index, missing| {
        super::join::Adapter::open(
            root.path()
                .join(format!("synthetic-catalog-{index}.sqlite3")),
            source.clone(),
            maxed_engine::EngineConfig::default(),
            maxed_store::WriterConfig {
                stream_id: "engine".into(),
                ..Default::default()
            },
            missing,
            runtime.shared.views.clone(),
            super::Bridge::default(),
        )
        .unwrap()
    };
    let mut first = open(1, false);
    let shared = runtime
        .shared
        .views
        .party_catalog
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    assert!(Arc::ptr_eq(&shared.catalog, &source));
    first.close().unwrap();
    let mut second = open(2, false);
    assert!(Arc::ptr_eq(
        &shared,
        runtime
            .shared
            .views
            .party_catalog
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
    ));
    assert!(
        ready_party(tauri::async_runtime::block_on(runtime.get_party_live()).unwrap())
            .assets_available
    );
    second.close().unwrap();
    let mut missing = open(3, true);
    assert!(runtime.shared.views.party_catalog.lock().unwrap().is_none());
    assert!(
        !ready_party(tauri::async_runtime::block_on(runtime.get_party_live()).unwrap())
            .assets_available
    );
    missing.close().unwrap();
}

#[test]
fn review_demo_party_has_catalog() {
    let (root, runtime) = command_runtime();
    let mut options = super::RuntimeOptions::live(root.path().into(), None);
    options.namespace = super::Namespace::Demo;
    let mut adapter = options
        .open(
            &Default::default(),
            runtime.shared.views.clone(),
            super::Bridge::default(),
        )
        .unwrap();
    let view = ready_party(tauri::async_runtime::block_on(runtime.get_party_live()).unwrap());
    assert!(view.assets_available);
    adapter.close().unwrap();
}

#[test]
fn review_party_async_reads_yield_before_history_admission_while_lifecycle_locks_are_held() {
    use std::{
        future::Future,
        task::{Context, Waker},
        time::Duration,
    };
    fn poll_and_hold(
        future: impl Future,
        send: crossbeam_channel::Sender<bool>,
        released: crossbeam_channel::Receiver<()>,
    ) {
        let mut future = Box::pin(future);
        send.send(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending(),
        )
        .unwrap();
        released.recv().unwrap();
    }
    for (saved, settings_lock) in [(false, true), (false, false), (true, true)] {
        let (_root, runtime) = command_runtime();
        let alerts = settings_lock.then(|| runtime.shared.alerts.lock().unwrap());
        let session = (!settings_lock).then(|| runtime.session.lock().unwrap());
        let reading = runtime.clone();
        let (send, receive) = crossbeam_channel::bounded(1);
        let (release, released) = crossbeam_channel::bounded(1);
        let worker = std::thread::spawn(move || {
            if saved {
                poll_and_hold(
                    reading.get_party_run("synthetic-run".into()),
                    send,
                    released,
                );
            } else {
                poll_and_hold(reading.get_party_live(), send, released);
            }
        });
        let result = receive.recv_timeout(Duration::from_millis(300));
        let (active, pending, _, _) = runtime.shared.home.scheduler.lock().unwrap().counts();
        drop(session);
        drop(alerts);
        release.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(result, Ok(true));
        // Namespace transitions hold these locks while waiting for history to
        // become idle. The prelude must finish before a read owns that lease.
        assert_eq!((active, pending), (0, 0));
    }
}

#[test]
fn review_party_publication_releases_publisher_lock_before_backpressure() {
    use std::time::{Duration, Instant};
    let (_root, runtime) = command_runtime();
    let (entered, wait) = crossbeam_channel::bounded(1);
    let (release, released) = crossbeam_channel::bounded(1);
    let mut first = true;
    let publisher = crate::bridge::Publisher::new(
        move |_| {
            if first {
                first = false;
                entered.send(()).unwrap();
                released.recv().unwrap();
            }
            Ok(())
        },
        Arc::new(|| panic!("delivery failed")),
    )
    .unwrap();
    publisher.publish(crate::bridge::Publication::Capture(Default::default()));
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    for _ in 0..8 {
        publisher.publish(crate::bridge::Publication::Capture(Default::default()));
    }
    *runtime.publisher.lock().unwrap() = Some(publisher.clone());
    let publishing = runtime.clone();
    let worker = std::thread::spawn(move || publishing.publish_party_changed());
    let deadline = Instant::now() + Duration::from_secs(5);
    // take_changed has consumed the notice before the blocked enqueue.
    loop {
        let mut party = runtime.shared.views.party.lock().unwrap();
        if party.take_changed().is_none() {
            break;
        }
        party.mark_dirty();
        drop(party);
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let unlocked = runtime.publisher.try_lock().is_ok();
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    publisher.flush().unwrap();
    assert!(unlocked);
}

fn paused_party_capture(runtime: &super::CaptureRuntime) {
    let (stop, stopped) = crossbeam_channel::bounded(1);
    *runtime.session.lock().unwrap() = Some(super::Session {
        stop,
        cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        worker: std::thread::spawn(move || {
            let _ = stopped.recv();
        }),
        sink: Arc::new(|_| {}),
    });
}

fn open_party_events() -> (RunView, Vec<EngineEvent>) {
    let snapshot = demo::demo_snapshot();
    let mut run = snapshot.run.unwrap();
    run.ended = None;
    let mut session = snapshot.session.unwrap();
    session.ended = None;
    let mut alpha = profile(1, Some("SynthAlpha"));
    alpha.scope_id = run.id.clone();
    alpha.run_id = Some(run.id.clone());
    alpha.first_seen = run.started;
    alpha.last_seen = run.started;
    let mut fight = snapshot
        .fights
        .into_iter()
        .find(|fight| fight.run_id.is_some())
        .unwrap();
    fight.players.truncate(1);
    fight.players[0].name = alpha.name.clone();
    fight.players[0].player_id = alpha.player_id.clone();
    fight.players[0].object_id = alpha.object_id;
    fight.players[0].is_local = false;
    let events = vec![
        EngineEvent::SessionUpdated(session),
        EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
        EngineEvent::RunStarted(run.clone()),
        EngineEvent::PlayerObserved(alpha),
        EngineEvent::FightEnded(fight),
    ];
    (run, events)
}

#[test]
fn review_legacy_party_run_does_not_parse_unrecorded_gear() {
    let (root, runtime) = command_runtime();
    let (run, events) = open_party_events();
    let mut db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let envelopes: Vec<_> = events
        .into_iter()
        .filter(|event| !matches!(event, EngineEvent::PlayerObserved(_)))
        .enumerate()
        .map(|(i, event)| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "legacy-party".into(),
            sequence: Sequence(Timestamp(i as u64 + 1)),
            at: run.started,
            event,
        })
        .collect();
    maxed_store::apply_batch(&mut db, &envelopes).unwrap();
    *runtime.shared.views.party_catalog.lock().unwrap() =
        Some(Arc::new(PartyCatalog::new(Arc::new(catalog()))));
    let view = ready_party(tauri::async_runtime::block_on(runtime.get_party_run(run.id)).unwrap())
        .unwrap();
    assert!(view.assets_available && !view.gear_recorded);
    assert_eq!(view.members.len(), 1);
    assert!(view.members[0].parse.is_none());
    assert_eq!(
        (
            view.counts.pass,
            view.counts.fail,
            view.counts.check,
            view.counts.not_seen
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn review_cold_party_writes_do_not_recover_open_runs() {
    let (root, runtime) = command_runtime();
    let (run, events) = open_party_events();
    let mut db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let envelopes: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(i, event)| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "cold-party".into(),
            sequence: Sequence(Timestamp(i as u64 + 1)),
            at: run.started,
            event,
        })
        .collect();
    maxed_store::apply_batch(&mut db, &envelopes).unwrap();
    let read_run = || {
        db.query_row("SELECT json FROM runs WHERE id=?1", [&run.id], |row| {
            row.get::<_, String>(0)
        })
        .unwrap()
    };
    let before = read_run();
    runtime
        .set_party_mark(
            "synthalpha".into(),
            "SynthAlpha".into(),
            Some(PartyMark::Watched),
            None,
        )
        .unwrap();
    assert_eq!(read_run(), before);
    runtime
        .clear_party_history(ClearPartyHistory {
            marks_and_notes: false,
        })
        .unwrap();
    assert_eq!(read_run(), before);
}

#[test]
fn review_adapter_clear_during_open_run_rebuilds_members_and_keeps_marks() {
    use super::party::{PartyWrite, PartyWriteRequest};
    let (root, runtime) = command_runtime();
    runtime
        .set_party_mark(
            "synthalpha".into(),
            "SynthAlpha".into(),
            Some(PartyMark::Watched),
            Some("Synthetic note".into()),
        )
        .unwrap();
    let mut adapter = super::RuntimeOptions::live(root.path().into(), None)
        .open(
            &Default::default(),
            runtime.shared.views.clone(),
            super::Bridge::default(),
        )
        .unwrap();
    let (mut run, events) = open_party_events();
    adapter.events(events);
    let (reply, receive) = crossbeam_channel::bounded(1);
    *runtime.shared.views.party_write.lock().unwrap() = Some(PartyWriteRequest {
        operation: PartyWrite::Clear(ClearPartyHistory {
            marks_and_notes: false,
        }),
        reply,
    });
    adapter.drain().unwrap();
    receive.recv().unwrap().unwrap();
    let reader = maxed_store::open_reader(&root.path().join("live.sqlite3")).unwrap();
    let cleared = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert_eq!(cleared.row.runs_together, 0);
    assert!(cleared.last_profile.is_none());
    run.ended = Some(Timestamp(run.started.0 + 60_000_000));
    adapter.events(vec![EngineEvent::RunEnded(run.clone())]);
    adapter.drain().unwrap();
    adapter.writer_flush_for_test();
    let rebuilt = reader
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert_eq!(rebuilt.row.runs_together, 1);
    assert_eq!(rebuilt.row.mark, Some(PartyMark::Watched));
    assert_eq!(rebuilt.row.note.as_deref(), Some("Synthetic note"));
    assert_eq!(rebuilt.runs[0].run_id, run.id);
    adapter.close().unwrap();
}

#[test]
fn party_commands_read_annotations_totals_presets_and_actual_capture_state() {
    let (_root, runtime) = command_runtime();
    runtime
        .set_party_mark(
            "synthalpha".into(),
            "SynthAlpha".into(),
            Some(PartyMark::Watched),
            Some("Synthetic note".into()),
        )
        .unwrap();
    let mut settings = settings();
    settings.presets[1].auto_apply = vec!["Synthetic Dungeon".into()];
    runtime.update_party_settings(settings).unwrap();
    *runtime.shared.views.party.lock().unwrap() = roster(&[profile(1, Some("SynthAlpha"))]);
    *runtime.shared.views.party_catalog.lock().unwrap() =
        Some(Arc::new(PartyCatalog::new(Arc::new(catalog()))));
    let mut snapshot = demo::demo_snapshot();
    snapshot.run.as_mut().unwrap().dungeon = "Synthetic Dungeon".into();
    snapshot.map = Some("Synthetic Map".into());
    snapshot.run_totals = Some(totals(&[(1, 100.)]));
    *runtime.shared.views.snapshot.lock().unwrap() = snapshot;
    *runtime.shared.status.lock().unwrap() = CaptureStatus {
        state: CaptureState::Live,
        ..Default::default()
    };
    let read = || ready_party(tauri::async_runtime::block_on(runtime.get_party_live()).unwrap());
    let view = read();
    assert!(
        !view.live,
        "a stale Live status does not mean capture is running"
    );
    assert_eq!(view.preset.unwrap().source, PresetSource::Auto);
    assert_eq!(view.members[0].damage, Some(100.));
    assert_eq!(view.members[0].mark, Some(PartyMark::Watched));
    assert_eq!(
        view.members[0].parse.as_ref().unwrap().result,
        ParseResult::Pass
    );
    paused_party_capture(&runtime);
    assert!(read().live);
    for namespace in [super::Namespace::Demo, super::Namespace::Replay] {
        runtime.shared.alerts.lock().unwrap().namespace = namespace;
        assert!(!read().live);
    }
    runtime.shared.alerts.lock().unwrap().namespace = super::Namespace::Live;
    runtime.shared.status.lock().unwrap().state = CaptureState::Stopped;
    assert!(!read().live);
    runtime
        .set_party_preset_pick(Some("default".into()))
        .unwrap();
    assert_eq!(read().preset.unwrap().source, PresetSource::Picked);
    assert!(runtime
        .set_party_preset_pick(Some("missing".into()))
        .is_err());
    runtime.set_party_preset_pick(None).unwrap();
    runtime.shared.views.snapshot.lock().unwrap().run = None;
    assert_eq!(read().preset.unwrap().source, PresetSource::Default);
    runtime
        .set_party_mark(
            "synthalpha".into(),
            "SynthAlpha".into(),
            Some(PartyMark::Banned),
            None,
        )
        .unwrap();
    assert_eq!(
        read().members[0].mark,
        Some(PartyMark::Banned),
        "annotations are read again on every call"
    );
    let page = ready_party(
        tauri::async_runtime::block_on(runtime.get_party_players(
            "Synth".into(),
            PartyPlayerFilter::Banned,
            PartyPlayerSort::LastSeen,
            None,
        ))
        .unwrap(),
    );
    assert_eq!(page.rows.len(), 1);
    let player = ready_party(
        tauri::async_runtime::block_on(runtime.get_party_player("synthalpha".into(), None))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(player.row.mark, Some(PartyMark::Banned));
}

#[test]
fn party_writes_refuse_preview_and_replay_and_clear_through_cold_file() {
    let (_root, runtime) = command_runtime();
    runtime
        .set_party_mark(
            "synthalpha".into(),
            "SynthAlpha".into(),
            Some(PartyMark::Watched),
            Some("Synthetic note".into()),
        )
        .unwrap();
    for namespace in [super::Namespace::Demo, super::Namespace::Replay] {
        runtime.shared.alerts.lock().unwrap().namespace = namespace;
        assert!(runtime
            .set_party_mark("synthalpha".into(), "SynthAlpha".into(), None, None)
            .is_err());
        assert!(runtime
            .clear_party_history(ClearPartyHistory {
                marks_and_notes: true
            })
            .is_err());
    }
    runtime.shared.alerts.lock().unwrap().namespace = super::Namespace::Live;
    runtime
        .clear_party_history(ClearPartyHistory {
            marks_and_notes: false,
        })
        .unwrap();
    let read = || {
        ready_party(
            tauri::async_runtime::block_on(runtime.get_party_player("synthalpha".into(), None))
                .unwrap(),
        )
    };
    assert_eq!(read().unwrap().row.mark, Some(PartyMark::Watched));
    runtime
        .clear_party_history(ClearPartyHistory {
            marks_and_notes: true,
        })
        .unwrap();
    assert!(read().is_none());
}

#[test]
fn party_write_timeout_withdraws_request_before_adapter_can_apply_it() {
    use super::party::PartyWrite;
    use std::time::{Duration, Instant};
    let (root, runtime) = command_runtime();
    runtime
        .set_party_mark(
            "synthalpha".into(),
            "SynthAlpha".into(),
            Some(PartyMark::Watched),
            None,
        )
        .unwrap();
    let mut adapter = super::RuntimeOptions::live(root.path().into(), None)
        .open_catalog(
            &std::sync::atomic::AtomicBool::new(false),
            runtime.shared.views.clone(),
            super::Bridge::default(),
            None,
        )
        .unwrap();
    let clear = || {
        PartyWrite::Clear(ClearPartyHistory {
            marks_and_notes: true,
        })
    };
    assert!(
        runtime
            .write_party_with_timeout(clear(), Duration::from_millis(10))
            .is_err(),
        "a parked adapter must refuse the write"
    );
    paused_party_capture(&runtime);
    for operation in [
        clear(),
        PartyWrite::Mark {
            key: "synthalpha".into(),
            name: "SynthAlpha".into(),
            mark: Some(PartyMark::Banned),
            note: None,
        },
    ] {
        assert!(runtime
            .write_party_with_timeout(operation, Duration::from_millis(10))
            .is_err());
        assert!(runtime.shared.views.party_write.lock().unwrap().is_none());
        adapter.drain().unwrap();
    }
    let player = maxed_store::open_reader(&root.path().join("live.sqlite3"))
        .unwrap()
        .get_party_player("synthalpha", None)
        .unwrap()
        .unwrap();
    assert_eq!(player.row.mark, Some(PartyMark::Watched));
    let writing = runtime.clone();
    let write = std::thread::spawn(move || {
        writing.clear_party_history(ClearPartyHistory {
            marks_and_notes: true,
        })
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime.shared.views.party_write.lock().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    adapter.drain().unwrap();
    write.join().unwrap().unwrap();
    assert!(maxed_store::open_reader(&root.path().join("live.sqlite3"))
        .unwrap()
        .get_party_player("synthalpha", None)
        .unwrap()
        .is_none());
    adapter.close().unwrap();
}

#[test]
fn party_run_command_adds_parse_without_using_live_pick() {
    let (root, runtime) = command_runtime();
    let mut db = rusqlite::Connection::open(root.path().join("live.sqlite3")).unwrap();
    maxed_store::migrate(&mut db).unwrap();
    let snapshot = demo::demo_snapshot();
    let run = snapshot.run.unwrap();
    let mut alpha = profile(1, Some("SynthAlpha"));
    alpha.scope_id = run.id.clone();
    alpha.run_id = Some(run.id.clone());
    alpha.first_seen = run.started;
    alpha.last_seen = run.started;
    let events = [
        EngineEvent::SessionUpdated(snapshot.session.unwrap()),
        EngineEvent::CharacterUpdated(snapshot.character.unwrap()),
        EngineEvent::RunStarted(run.clone()),
        EngineEvent::PlayerObserved(alpha),
    ];
    let envelopes: Vec<_> = events
        .into_iter()
        .enumerate()
        .map(|(i, event)| EventEnvelope {
            fight_chart_omitted: false,
            stream_id: "synthetic-party".into(),
            sequence: Sequence(Timestamp(i as u64 + 1)),
            at: run.started,
            event,
        })
        .collect();
    maxed_store::apply_batch(&mut db, &envelopes).unwrap();
    *runtime.shared.views.party_catalog.lock().unwrap() =
        Some(Arc::new(PartyCatalog::new(Arc::new(catalog()))));
    runtime
        .set_party_preset_pick(Some("picked".into()))
        .unwrap();
    let read = || {
        ready_party(tauri::async_runtime::block_on(runtime.get_party_run(run.id.clone())).unwrap())
            .unwrap()
    };
    let view = read();
    assert!(!view.live);
    assert_eq!(view.preset.unwrap().source, PresetSource::Default);
    assert_eq!(
        view.members[0].parse.as_ref().unwrap().result,
        ParseResult::Pass
    );
    assert_eq!(view.counts.pass, 1);
    assert_eq!(view.members[0].present, None);
    *runtime.shared.views.party_catalog.lock().unwrap() = None;
    let view = read();
    assert!(!view.assets_available);
    assert!(view.members[0].parse.is_none());
    assert_eq!(view.counts.pass, 0);
}

fn profile(id: i32, name: Option<&str>) -> PlayerProfileView {
    PlayerProfileView {
        maxed_gated: false,
        scope_id: "scope-a".into(),
        run_id: Some("run-a".into()),
        player_id: format!("player-{id}"),
        object_id: id,
        is_local: false,
        name: name.map(str::to_owned),
        class_id: Some(100),
        level: Some(20),
        stars: Some(50),
        fame: None,
        skin_id: None,
        guild: Some("Synthetic Guild".into()),
        stats: vec![],
        maxed: Some(8),
        maxed_stats: vec![],
        equipment: vec![EquipmentView {
            observed_empty: None,
            slot: 0,
            item_type: Some(200),
            enchant: EnchantView {
                state: EnchantState::Missing,
                slots: None,
                applied: None,
                ids: vec![],
            },
        }],
        first_seen: Timestamp(10),
        last_seen: Timestamp(20),
    }
}

fn presence(profile: &PlayerProfileView, present: bool, at: u64) -> PlayerPresence {
    PlayerPresence {
        scope_id: profile.scope_id.clone(),
        run_id: profile.run_id.clone(),
        player_id: profile.player_id.clone(),
        object_id: profile.object_id,
        present,
        at: Timestamp(at),
    }
}

fn map(name: &str, kind: AreaKind) -> EngineEvent {
    EngineEvent::MapEntered {
        realm_instance_key: None,
        at: Timestamp(1),
        name: name.into(),
        kind,
    }
}

fn roster(profiles: &[PlayerProfileView]) -> PartyRoster {
    let mut roster = PartyRoster::default();
    roster.apply(&map("Synthetic Sanctum", AreaKind::Dungeon));
    for profile in profiles {
        roster.apply(&EngineEvent::PlayerObserved(profile.clone()));
    }
    roster
}

fn preset(id: &str) -> RequirementPreset {
    RequirementPreset {
        id: id.into(),
        name: format!("Synthetic {id}"),
        min_maxed: Some(6),
        min_tier: SlotTiers {
            weapon: Some(11),
            ability: None,
            armor: None,
            ring: None,
        },
        ut_st_meets: true,
        auto_apply: vec![],
    }
}

fn settings() -> PartySettings {
    PartySettings {
        presets: vec![preset("default"), preset("picked")],
        default_preset_id: Some("default".into()),
    }
}

fn catalog() -> Catalog {
    Catalog {
        items: vec![ItemDef {
            weapon_firing: None,
            potion_stat: None,
            object_type: 200,
            name: "Synthetic Weapon".into(),
            tier: Some("12".into()),
            slot_type: None,
            projectiles: vec![],
            scaling: None,
            sprite_key: None,
            soulbound: None,
            stat_bonuses: None,
        }],
        ..(*super::join::unavailable_catalog()).clone()
    }
}

fn context(settings: &PartySettings) -> RosterContext<'_> {
    RosterContext {
        live: true,
        totals: None,
        dungeon: Some("Synthetic Sanctum"),
        settings,
        catalog: None,
        annotations: &[],
    }
}

fn totals(players: &[(i32, f64)]) -> RunTotals {
    let total: f64 = players.iter().map(|(_, damage)| damage).sum();
    RunTotals {
        fights: 1,
        kills: 0,
        first_hit: None,
        last_hit: None,
        total_damage: total,
        unattributed_damage: 900.0,
        players: players
            .iter()
            .map(|(id, damage)| RunPlayerTotal {
                player_id: format!("player-{id}"),
                name: None,
                class_id: None,
                is_local: false,
                damage: *damage,
                hits: 1,
                deaths: 0,
                share: (total > 0.0).then_some(damage / total),
                rank: None,
                dps: None,
            })
            .collect(),
        local: None,
        fame_delta: None,
        partial: false,
    }
}

fn fight(id: &str, boss: bool, players: &[(i32, f64)]) -> FightView {
    FightView {
        id: id.into(),
        scope_id: Some("scope-a".into()),
        run_id: Some("run-a".into()),
        session_id: "session-a".into(),
        target_id: 900,
        target_generation: 0,
        boss_type: boss.then_some(300),
        target_type: Some(300),
        max_hp: None,
        started: Timestamp(30),
        ended: None,
        first_hit: None,
        last_hit: None,
        end_reason: None,
        players: players
            .iter()
            .map(|(id, damage)| PlayerDamage {
                object_id: *id,
                player_id: format!("player-{id}"),
                name: None,
                class_id: None,
                is_local: false,
                damage: *damage,
                hits: 1,
                biggest_hit: None,
                deaths: 0,
                dps: None,
                share: None,
                rank: None,
                sources: vec![],
            })
            .collect(),
        dps_basis: DpsBasis::PlayerActiveWindowV1,
        total_damage: players.iter().map(|(_, damage)| damage).sum(),
        unattributed_damage: 900.0,
        buckets: vec![],
        complete_capture: true,
        pending_local_reconciliation: false,
        issues: vec![],
        kill: None,
        target_hp: None,
        target_kind: None,
        is_current_quest: false,
        local_last_hit: None,
        local_active_micros: None,
        player_series: None,
    }
}

#[test]
fn map_change_resets_roster_and_pick() {
    let settings = settings();
    let ctx = context(&settings);
    let alpha = profile(1, Some("SynthAlpha"));
    let mut roster = roster(std::slice::from_ref(&alpha));
    roster.pick(Some("picked".into()));
    assert_eq!(
        roster.view(&ctx).preset.unwrap().source,
        PresetSource::Picked
    );
    roster.apply(&EngineEvent::FightStarted(fight(
        "boss",
        true,
        &[(1, 50.0)],
    )));
    roster.apply(&map("Synthetic Nexus", AreaKind::Nexus));
    let view = roster.view(&ctx);
    assert!(view.members.is_empty());
    assert!(roster.keys().is_empty());
    assert_eq!(view.scope_id, None);
    assert_eq!(view.run_id, None);
    assert_eq!(view.map.as_deref(), Some("Synthetic Nexus"));
    assert_eq!(view.area_kind, AreaKind::Nexus);
    assert_eq!(view.preset.unwrap().source, PresetSource::Default);
    assert!(view.live && view.gear_recorded);
    roster.apply(&EngineEvent::PlayerObserved(alpha));
    assert_eq!(roster.view(&ctx).members[0].boss_share, None);

    // Both profile-first and presence-first scope changes clear prior state.
    for presence_first in [false, true] {
        roster.pick(Some("picked".into()));
        let mut bravo = profile(2, Some("SynthBravo"));
        bravo.scope_id = format!("scope-{presence_first}");
        bravo.run_id = Some(format!("run-{presence_first}"));
        if presence_first {
            roster.observe_presence(&presence(&bravo, true, 40));
        } else {
            roster.apply(&EngineEvent::PlayerObserved(bravo.clone()));
        }
        let view = roster.view(&ctx);
        assert_eq!(view.members.len(), 1);
        assert_eq!(view.scope_id.as_ref(), Some(&bravo.scope_id));
        assert_eq!(view.run_id, bravo.run_id);
        assert_eq!(view.preset.unwrap().source, PresetSource::Default);
    }
    roster.pick(Some("picked".into()));
    roster.pick(None);
    assert_eq!(
        roster.view(&ctx).preset.unwrap().source,
        PresetSource::Default
    );
}

#[test]
fn rejoin_merges_by_name_key() {
    let settings = settings();
    let catalog = PartyCatalog::new(Arc::new(catalog()));
    let run_totals = totals(&[(1, 20.0), (2, 30.0), (3, 50.0)]);
    let mut ctx = context(&settings);
    ctx.catalog = Some(&catalog);
    ctx.totals = Some(&run_totals);
    let mut older = profile(1, Some("SynthAlpha,fe14"));
    older.maxed = Some(4);
    let mut latest = profile(2, Some("SYNTHalpha"));
    latest.first_seen = Timestamp(50);
    latest.last_seen = Timestamp(60);
    latest.class_id = Some(101);
    latest.is_local = true;
    latest.maxed_gated = true;
    // Insert the older profile last so selection must use last_seen.
    let mut roster = roster(&[latest.clone(), older.clone()]);
    roster.observe_presence(&presence(&latest, true, 50));
    roster.observe_presence(&presence(&older, false, 55));
    roster.apply(&EngineEvent::FightEnded(FightView {
        ended: Some(Timestamp(70)),
        ..fight("boss", true, &[(1, 10.0), (2, 30.0), (3, 60.0)])
    }));
    let view = roster.view(&ctx);
    assert_eq!(roster.keys(), vec!["synthalpha"]);
    assert_eq!(view.members.len(), 1);
    let row = &view.members[0];
    assert_eq!(row.player_id, latest.player_id);
    assert_eq!(row.name, latest.name);
    assert_eq!(row.class_id, latest.class_id);
    assert_eq!(row.guild, latest.guild);
    assert_eq!(row.equipment, latest.equipment);
    assert!(row.is_local);
    assert_eq!(row.present, Some(true));
    assert_eq!(row.out_of_view_since, None);
    assert_eq!(row.damage, Some(50.0));
    assert_eq!(row.share, Some(0.5));
    assert_eq!(row.boss_share, Some(0.4));
    assert_eq!(row.parse.as_ref().unwrap().result, ParseResult::Pass);
    assert_eq!(view.counts.in_view, 1);
    assert_eq!(
        view.classes,
        vec![ClassCount {
            class_id: 101,
            count: 1
        }]
    );

    let zero = totals(&[(1, 0.0), (2, 0.0)]);
    ctx.totals = Some(&zero);
    assert!(roster.view(&ctx).members[0]
        .flags
        .contains(&ContributionFlag::NoDamageSeen));
}

#[test]
fn presence_sets_out_of_view_since() {
    let settings = PartySettings::default();
    let ctx = context(&settings);
    let alpha = profile(1, Some("SynthAlpha"));
    let mut roster = roster(&[]);
    roster.observe_presence(&presence(&alpha, true, 10));
    assert_eq!(roster.view(&ctx).members[0].present, Some(true));
    roster.observe_presence(&presence(&alpha, false, 30));
    roster.apply(&EngineEvent::PlayerObserved(alpha.clone()));
    let view = roster.view(&ctx);
    assert_eq!(view.members[0].out_of_view_since, Some(Timestamp(30)));
    assert_eq!(view.members[0].present, Some(false));
    roster.observe_presence(&presence(&alpha, true, 40));
    let view = roster.view(&ctx);
    assert_eq!(view.members[0].out_of_view_since, None);
    assert_eq!(view.members[0].present, Some(true));
}

#[test]
fn departure_sets_departed_and_left_early_flag() {
    let settings = PartySettings::default();
    let ctx = context(&settings);
    let mut roster = roster(&[
        profile(1, Some("SynthAlpha")),
        profile(2, Some("SynthBravo")),
    ]);
    roster.apply(&EngineEvent::InstanceEvent(InstanceEventView {
        id: "left-a".into(),
        session_id: "session-a".into(),
        run_id: Some("run-a".into()),
        map: Some("Synthetic Sanctum".into()),
        at: Timestamp(40),
        kind: InstanceEventKind::PlayerLeft {
            object_id: 1,
            player_name: None,
            class_id: None,
        },
        provisional: false,
    }));
    let view = roster.view(&ctx);
    assert_eq!(view.members[0].departed_at, Some(Timestamp(40)));
    assert_eq!(view.members[0].flags, vec![ContributionFlag::LeftEarly]);
    assert_eq!(view.members[1].departed_at, None);
    assert!(view.members[1].flags.is_empty());
    assert_eq!(view.counts.left_early, 1);
}

#[test]
fn boss_share_from_boss_fights_only() {
    let settings = PartySettings::default();
    let ctx = context(&settings);
    let mut roster = roster(&[profile(1, Some("SynthAlpha"))]);
    roster.apply(&EngineEvent::FightEnded(fight(
        "trash",
        false,
        &[(1, 500.0)],
    )));
    roster.apply(&EngineEvent::FightStarted(fight("boss-a", true, &[])));
    assert_eq!(roster.view(&ctx).members[0].boss_share, None);
    roster.apply(&EngineEvent::FightUpdated(fight(
        "boss-a",
        true,
        &[(1, 10.0), (2, 90.0)],
    )));
    assert_eq!(roster.view(&ctx).members[0].boss_share, Some(0.1));
    let ended = FightView {
        ended: Some(Timestamp(60)),
        ..fight("boss-a", true, &[(1, 50.0), (2, 50.0)])
    };
    roster.apply(&EngineEvent::FightEnded(ended.clone()));
    roster.apply(&EngineEvent::FightEnded(ended));
    roster.apply(&EngineEvent::FightStarted(fight(
        "boss-b",
        true,
        &[(2, 100.0)],
    )));
    assert_eq!(roster.view(&ctx).members[0].boss_share, Some(0.25));
}

#[test]
fn boss_share_zero_for_player_without_boss_damage() {
    let settings = PartySettings::default();
    let totals = totals(&[(1, 50.0), (2, 950.0)]);
    let mut ctx = context(&settings);
    ctx.totals = Some(&totals);
    let mut roster = roster(&[profile(1, Some("SynthAlpha"))]);
    roster.apply(&EngineEvent::FightStarted(fight(
        "boss",
        true,
        &[(2, 100.0)],
    )));
    let view = roster.view(&ctx);
    assert_eq!(view.members[0].boss_share, Some(0.0));
    assert!(!view.members[0]
        .flags
        .contains(&ContributionFlag::MostlyTrash));
    roster.apply(&EngineEvent::FightEnded(FightView {
        ended: Some(Timestamp(60)),
        ..fight("boss", true, &[(2, 100.0)])
    }));
    assert_eq!(
        roster.view(&ctx).members[0].flags,
        vec![ContributionFlag::MostlyTrash]
    );
}

#[test]
fn no_damage_seen_after_boss_start() {
    let settings = PartySettings::default();
    let totals = totals(&[(1, 0.0), (2, 0.0)]);
    let mut ctx = context(&settings);
    ctx.totals = Some(&totals);
    let mut late = profile(2, Some("SynthBravo"));
    late.first_seen = Timestamp(40);
    let mut roster = roster(&[profile(1, Some("SynthAlpha")), late]);
    assert!(roster
        .view(&ctx)
        .members
        .iter()
        .all(|row| row.flags.is_empty()));
    roster.apply(&EngineEvent::FightStarted(fight("first", true, &[])));
    roster.apply(&EngineEvent::FightStarted(FightView {
        started: Timestamp(50),
        ..fight("second", true, &[])
    }));
    let view = roster.view(&ctx);
    assert_eq!(view.members[0].flags, vec![ContributionFlag::NoDamageSeen]);
    assert_eq!(view.members[1].flags, vec![ContributionFlag::NoDamageSeen]);
    assert_eq!(view.counts.no_damage_seen, 2);
    roster.apply(&map("Synthetic Realm", AreaKind::Realm));
    roster.apply(&EngineEvent::PlayerObserved(profile(1, Some("SynthAlpha"))));
    roster.apply(&EngineEvent::FightStarted(fight("realm-boss", true, &[])));
    assert!(roster.view(&ctx).members[0].flags.is_empty());
}

#[test]
fn resolve_preset_order() {
    let mut settings = settings();
    let mut auto = preset("auto");
    auto.auto_apply.push("Synthetic Sanctum".into());
    settings.presets.push(auto);
    for (pick, dungeon, expected) in [
        (
            Some("picked"),
            Some("Synthetic Sanctum"),
            Some(("picked", PresetSource::Picked)),
        ),
        (
            Some("deleted"),
            Some("Synthetic Sanctum"),
            Some(("auto", PresetSource::Auto)),
        ),
        (
            None,
            Some("Synthetic Sanctum"),
            Some(("auto", PresetSource::Auto)),
        ),
        (
            None,
            Some("Synthetic Other"),
            Some(("default", PresetSource::Default)),
        ),
        (
            Some("deleted"),
            None,
            Some(("default", PresetSource::Default)),
        ),
    ] {
        assert_eq!(
            resolve_preset(&settings, pick, dungeon)
                .map(|(preset, source)| (preset.id.as_str(), source)),
            expected
        );
    }
    settings.default_preset_id = None;
    assert!(resolve_preset(&settings, None, None).is_none());
    settings.default_preset_id = Some("deleted".into());
    assert!(resolve_preset(&settings, Some("deleted"), None).is_none());
}

#[test]
fn no_catalog_gives_no_parse_and_assets_unavailable() {
    let settings = settings();
    let view = roster(&[profile(1, Some("SynthAlpha"))]).view(&context(&settings));
    assert!(!view.assets_available);
    assert!(view.preset.is_some());
    assert_eq!(view.members[0].parse, None);
    assert_eq!(view.members[0].damage, None);
    assert_eq!(view.members[0].share, None);
    assert_eq!(
        (
            view.counts.pass,
            view.counts.fail,
            view.counts.check,
            view.counts.not_seen
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn unknown_item_gives_check() {
    let settings = settings();
    let mut catalog = Arc::new(catalog());
    let mut alpha = profile(1, Some("SynthAlpha"));
    alpha.equipment[0].item_type = Some(201);
    let roster = roster(&[alpha]);
    for missing_definition in [true, false] {
        if !missing_definition {
            Arc::make_mut(&mut catalog).items[0].object_type = 201;
            Arc::make_mut(&mut catalog).items[0].tier = None;
        }
        let mut ctx = context(&settings);
        let indexed = PartyCatalog::new(catalog.clone());
        ctx.catalog = Some(&indexed);
        let view = roster.view(&ctx);
        assert!(view.assets_available);
        let parse = view.members[0].parse.as_ref().unwrap();
        assert_eq!(parse.result, ParseResult::Check);
        assert_eq!(parse.reasons[0].kind, ParseReasonKind::NoTierData);
    }
}

#[test]
fn annotations_apply_marks_and_counts() {
    let settings = PartySettings::default();
    let annotations = [PartyAnnotation {
        key: "synthalpha".into(),
        mark: Some(PartyMark::Watched),
        note: Some("Synthetic note".into()),
        runs_together: 7,
    }];
    let mut ctx = context(&settings);
    ctx.annotations = &annotations;
    let roster = roster(&[
        profile(1, Some("SynthAlpha,fe14")),
        profile(2, Some("SynthBravo")),
    ]);
    let view = roster.view(&ctx);
    assert_eq!(view.members[0].name_key.as_deref(), Some("synthalpha"));
    assert_eq!(view.members[0].mark, Some(PartyMark::Watched));
    assert_eq!(view.members[0].runs_together, 7);
    assert_eq!(view.members[1].mark, None);
    assert_eq!(view.members[1].runs_together, 0);
    ctx.annotations = &[];
    assert_eq!(roster.view(&ctx).members[0].mark, None);
}

#[test]
fn nameless_member_has_no_history() {
    let settings = PartySettings::default();
    let annotations = [PartyAnnotation {
        key: String::new(),
        mark: Some(PartyMark::Banned),
        note: None,
        runs_together: 9,
    }];
    let mut ctx = context(&settings);
    ctx.annotations = &annotations;
    let roster = roster(&[profile(1, None), profile(2, None), profile(3, Some(""))]);
    assert!(roster.keys().is_empty());
    let view = roster.view(&ctx);
    assert_eq!(view.members.len(), 3);
    for row in view.members {
        assert_eq!(row.name_key, None);
        assert_eq!(row.mark, None);
        assert_eq!(row.runs_together, 0);
    }
}

#[test]
fn sort_and_counts() {
    let settings = settings();
    let catalog = PartyCatalog::new(Arc::new(catalog()));
    let totals = totals(&[
        (1, 10.0),
        (2, 20.0),
        (3, 20.0),
        (4, 30.0),
        (5, 40.0),
        (6, 50.0),
    ]);
    let mut profiles: Vec<_> = [
        "SynthCharlie",
        "SynthBravo",
        "SynthAlpha",
        "SynthDelta",
        "SynthEcho",
        "SynthFoxtrot",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, name)| profile(index as i32 + 1, Some(name)))
    .collect();
    for profile in &mut profiles[..3] {
        profile.maxed = Some(5);
    }
    profiles[3].maxed = None;
    profiles[4].level = None;
    profiles[4].maxed = None;
    profiles[5].class_id = Some(101);
    let mut roster = roster(&profiles);
    roster.observe_presence(&presence(&profiles[0], true, 10));
    roster.observe_presence(&presence(&profiles[1], false, 20));
    let mut ctx = context(&settings);
    ctx.catalog = Some(&catalog);
    ctx.totals = Some(&totals);
    let view = roster.view(&ctx);
    assert_eq!(
        view.members
            .iter()
            .map(|row| row.player_id.as_str())
            .collect::<Vec<_>>(),
        vec!["player-3", "player-2", "player-1", "player-4", "player-5", "player-6"]
    );
    assert_eq!(
        view.counts,
        PartyCounts {
            in_view: 1,
            pass: 1,
            fail: 3,
            check: 1,
            not_seen: 1,
            no_damage_seen: 0,
            left_early: 0
        }
    );
    assert_eq!(
        view.classes,
        vec![
            ClassCount {
                class_id: 100,
                count: 5
            },
            ClassCount {
                class_id: 101,
                count: 1
            }
        ]
    );
    let empty_settings = PartySettings::default();
    ctx.settings = &empty_settings;
    let view = roster.view(&ctx);
    assert!(view.preset.is_none());
    assert!(view.members.iter().all(|row| row.parse.is_none()));
    assert_eq!(
        view.members
            .iter()
            .map(|row| row.player_id.as_str())
            .collect::<Vec<_>>(),
        vec!["player-6", "player-5", "player-4", "player-3", "player-2", "player-1"]
    );
}

#[test]
fn cap_matches_engine_profile_limit() {
    use maxed_engine::PROFILE_LIMIT;
    let settings = PartySettings::default();
    let ctx = context(&settings);
    for presence_first in [false, true] {
        let mut roster = roster(&[]);
        for id in 0..PROFILE_LIMIT as i32 + 2 {
            let profile = profile(id, Some(&format!("Synth{id}")));
            if presence_first {
                roster.observe_presence(&presence(&profile, true, 10));
            }
            roster.apply(&EngineEvent::PlayerObserved(profile));
        }
        assert_eq!(roster.view(&ctx).members.len(), PROFILE_LIMIT);
        assert_eq!(roster.keys().len(), PROFILE_LIMIT);
        let mut updated = profile(0, Some("SynthUpdated"));
        updated.last_seen = Timestamp(60);
        roster.apply(&EngineEvent::PlayerObserved(updated));
        assert!(roster.keys().contains(&"synthupdated".into()));
        assert!(!roster.keys().contains(&format!("synth{PROFILE_LIMIT}")));
    }
}

#[test]
fn truncated_totals_leave_missing_and_partially_known_rejoins_unknown() {
    let settings = PartySettings::default();
    let players: Vec<_> = (1..=66).map(|id| (id, 100.0 / f64::from(id))).collect();
    let run_totals = maxed_engine::run_totals(&[fight("trash", false, &players)], None);
    assert_eq!(
        run_totals.players.len(),
        maxed_engine::RUN_TOTAL_PLAYER_LIMIT
    );
    let mut ctx = context(&settings);
    ctx.totals = Some(&run_totals);
    let profiles: Vec<_> = (1..=66)
        .map(|id| profile(id, Some(&format!("Synth{id}"))))
        .collect();
    let mut roster = roster(&profiles);
    roster.apply(&EngineEvent::FightStarted(fight("boss", true, &[])));
    for id in [65, 66] {
        let view = roster.view(&ctx);
        let member = view
            .members
            .iter()
            .find(|p| p.player_id == format!("player-{id}"))
            .unwrap();
        assert_eq!(member.damage, None);
        assert_eq!(member.share, None);
        assert!(!member.flags.contains(&ContributionFlag::NoDamageSeen));
    }
    roster.apply(&EngineEvent::PlayerObserved(profile(65, Some("Synth1"))));
    let view = roster.view(&ctx);
    let member = view
        .members
        .iter()
        .find(|p| p.name_key.as_deref() == Some("synth1"))
        .unwrap();
    assert_eq!(member.damage, None);
    assert_eq!(member.share, None);
    assert!(!member.flags.contains(&ContributionFlag::NoDamageSeen));
}

#[test]
fn boss_target_kinds_count_without_boss_type() {
    let settings = PartySettings::default();
    for kind in [TargetKind::Boss, TargetKind::EventBoss] {
        let mut roster = roster(&[profile(1, Some("SynthAlpha"))]);
        roster.apply(&EngineEvent::FightStarted(FightView {
            target_kind: Some(kind),
            ..fight("boss", false, &[(1, 20.0), (2, 80.0)])
        }));
        assert_eq!(
            roster.view(&context(&settings)).members[0].boss_share,
            Some(0.2)
        );
    }
}

#[test]
fn character_merge_rekeys_presence_and_boss_damage() {
    let settings = PartySettings::default();
    let alpha = profile(1, Some("SynthAlpha"));
    let mut verified = alpha.clone();
    verified.player_id = "verified".into();
    let mut roster = roster(std::slice::from_ref(&alpha));
    roster.observe_presence(&presence(&alpha, true, 10));
    roster.apply(&EngineEvent::FightStarted(fight(
        "boss",
        true,
        &[(1, 20.0), (2, 80.0)],
    )));
    roster.apply(&EngineEvent::CharacterMerged {
        provisional_id: alpha.player_id,
        verified_id: verified.player_id.clone(),
    });
    roster.apply(&EngineEvent::PlayerObserved(verified.clone()));
    roster.observe_presence(&presence(&verified, false, 50));
    let view = roster.view(&context(&settings));
    assert_eq!(view.members.len(), 1);
    assert_eq!(view.members[0].player_id, "verified");
    assert_eq!(view.members[0].present, Some(false));
    assert_eq!(view.members[0].out_of_view_since, Some(Timestamp(50)));
    assert_eq!(view.members[0].boss_share, Some(0.2));
}

fn departure(object_id: i32) -> EngineEvent {
    EngineEvent::InstanceEvent(InstanceEventView {
        id: "left-a".into(),
        session_id: "session-a".into(),
        run_id: Some("run-a".into()),
        map: Some("Synthetic Sanctum".into()),
        at: Timestamp(40),
        kind: InstanceEventKind::PlayerLeft {
            object_id,
            player_name: Some("SynthAlpha".into()),
            class_id: None,
        },
        provisional: false,
    })
}

#[test]
fn departure_on_one_rejoined_id_keeps_other_id_present() {
    let settings = PartySettings::default();
    let older = profile(1, Some("SynthAlpha"));
    let latest = profile(2, Some("SYNTHalpha"));
    let mut roster = roster(&[older.clone(), latest.clone()]);
    roster.observe_presence(&presence(&older, false, 20));
    roster.observe_presence(&presence(&latest, true, 30));
    roster.apply(&departure(1));
    let view = roster.view(&context(&settings));
    assert_eq!(view.members.len(), 1);
    assert_eq!(view.members[0].present, Some(true));
    assert_eq!(view.members[0].out_of_view_since, None);
    assert_eq!(view.members[0].departed_at, Some(Timestamp(40)));
}

#[test]
fn unknown_departure_does_not_guess_from_name() {
    let settings = PartySettings::default();
    let mut roster = roster(&[profile(1, Some("SynthAlpha"))]);
    roster.apply(&departure(99));
    let view = roster.view(&context(&settings));
    assert_eq!(view.members[0].departed_at, None);
    assert!(view.members[0].flags.is_empty());
}

#[test]
fn out_of_view_since_uses_latest_exit_across_rejoin() {
    let settings = PartySettings::default();
    let older = profile(1, Some("SynthAlpha"));
    let latest = profile(2, Some("SYNTHalpha"));
    let mut roster = roster(&[older.clone(), latest.clone()]);
    roster.observe_presence(&presence(&older, false, 20));
    roster.observe_presence(&presence(&latest, true, 30));
    assert_eq!(
        roster.view(&context(&settings)).members[0].out_of_view_since,
        None
    );
    roster.observe_presence(&presence(&latest, false, 50));
    assert_eq!(
        roster.view(&context(&settings)).members[0].out_of_view_since,
        Some(Timestamp(50))
    );
}

#[test]
fn roster_live_status_comes_from_context() {
    let settings = PartySettings::default();
    let roster = roster(&[profile(1, Some("SynthAlpha"))]);
    let mut ctx = context(&settings);
    for live in [false, true] {
        ctx.live = live;
        assert_eq!(roster.view(&ctx).live, live);
    }
}

#[test]
fn reset_preview_clears_party_roster_and_pick() {
    let runtime = super::CaptureRuntime::default();
    let settings = settings();
    let ctx = context(&settings);
    let alpha = profile(1, Some("SynthAlpha"));
    {
        let mut party = runtime.shared.views.party.lock().unwrap();
        *party = roster(std::slice::from_ref(&alpha));
        party.observe_presence(&presence(&alpha, true, 10));
        party.pick(Some("picked".into()));
    }
    runtime.reset_preview().unwrap();
    let view = runtime.shared.views.party.lock().unwrap().view(&ctx);
    assert!(view.members.is_empty());
    assert_eq!(view.counts.in_view, 0);
    assert_eq!(view.scope_id, None);
    assert_eq!(view.map, None);
    assert_eq!(view.preset.unwrap().source, PresetSource::Default);
}

#[test]
fn review_catalog_tiers_are_shared_across_cloned_roster_reads() {
    let (_root, runtime) = command_runtime();
    let source = Arc::new(catalog());
    let indexed = Arc::new(PartyCatalog::new(source.clone()));
    *runtime.shared.views.party_catalog.lock().unwrap() = Some(indexed.clone());
    *runtime.shared.views.party.lock().unwrap() = roster(&[profile(1, Some("SynthAlpha"))]);
    for _ in 0..3 {
        let view = ready_party(tauri::async_runtime::block_on(runtime.get_party_live()).unwrap());
        assert_eq!(
            view.members[0].parse.as_ref().unwrap().result,
            ParseResult::Pass
        );
        assert!(Arc::ptr_eq(
            runtime
                .shared
                .views
                .party_catalog
                .lock()
                .unwrap()
                .as_ref()
                .unwrap(),
            &indexed
        ));
        assert_eq!(Arc::strong_count(&source), 2);
    }
    let mut replacement = (*source).clone();
    replacement.items[0].tier = Some("1".into());
    *runtime.shared.views.party_catalog.lock().unwrap() =
        Some(Arc::new(PartyCatalog::new(Arc::new(replacement))));
    let view = ready_party(tauri::async_runtime::block_on(runtime.get_party_live()).unwrap());
    assert_eq!(
        view.members[0].parse.as_ref().unwrap().result,
        ParseResult::Fail
    );
}

#[test]
fn boss_updates_replace_damage_without_retaining_removed_players() {
    let settings = PartySettings::default();
    let mut roster = roster(&[profile(1, Some("SynthAlpha"))]);
    roster.apply(&EngineEvent::FightStarted(fight(
        "boss",
        true,
        &[(1, 20.0), (2, 80.0)],
    )));
    roster.apply(&EngineEvent::FightUpdated(fight(
        "boss",
        true,
        &[(1, 30.0)],
    )));
    assert_eq!(
        roster.view(&context(&settings)).members[0].boss_share,
        Some(1.0)
    );
}

#[test]
fn party_settings_command_patches_live_settings_only() {
    let root = tempfile::tempdir().unwrap();
    let runtime = super::CaptureRuntime::default();
    let store = Arc::new(crate::settings::SettingsStore::new(root.path().into()).unwrap());
    runtime.configure_alert_settings(store.clone()).unwrap();
    runtime.update_party_settings(settings()).unwrap();
    assert_eq!(store.get().unwrap().party, settings());
    runtime.shared.alerts.lock().unwrap().namespace = super::Namespace::Demo;
    assert!(runtime
        .update_party_settings(PartySettings::default())
        .is_err());
    assert_eq!(store.get().unwrap().party, settings());
}

#[test]
fn adapter_feeds_party_before_live_and_replay_publication() {
    use super::join::{Adapter, Bridge, Views};
    use std::sync::{Arc, Mutex};

    for live in [false, true] {
        for batched in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let views = Arc::new(Views::default());
            let received = Arc::new(Mutex::new(Vec::new()));
            let sink_views = views.clone();
            let sink_received = received.clone();
            let observe = move |events: Vec<EventEnvelope>| {
                // The publication callback must see the complete batch in Party,
                // and must be able to lock it without the adapter holding it.
                let settings = PartySettings::default();
                let view = sink_views.party.lock().unwrap().view(&context(&settings));
                assert_eq!(view.map.as_deref(), Some("Synthetic Sanctum"));
                assert_eq!(view.scope_id.as_deref(), Some("scope-a"));
                assert_eq!(view.members.len(), 1);
                assert_eq!(view.members[0].present, None);
                assert_eq!(view.members[0].boss_share, Some(1.0));
                sink_received
                    .lock()
                    .unwrap()
                    .extend(events.into_iter().map(|event| event.event));
            };
            let bridge = if batched {
                Bridge {
                    batch: Some(Arc::new(observe)),
                    ..Bridge::default()
                }
            } else {
                Bridge {
                    event: Arc::new(move |event| observe(vec![event])),
                    ..Bridge::default()
                }
            };
            let mut adapter = Adapter::open(
                directory.path().join("party.sqlite3"),
                Arc::new(catalog()),
                maxed_engine::EngineConfig::default(),
                maxed_store::WriterConfig {
                    stream_id: "engine".into(),
                    ..Default::default()
                },
                false,
                views,
                bridge,
            )
            .unwrap();
            adapter.live = live;
            let alpha = profile(1, Some("SynthAlpha"));
            let events = vec![
                map("Synthetic Sanctum", AreaKind::Dungeon),
                EngineEvent::PlayerObserved(alpha.clone()),
                EngineEvent::FightStarted(fight("boss", true, &[(1, 50.0)])),
            ];
            adapter.events(events.clone());
            assert_eq!(*received.lock().unwrap(), events);
            adapter.discard();
        }
    }
}

#[test]
fn adapter_applies_roster_presence_and_profiles_without_events_or_receipts() {
    use super::join::{Adapter, Bridge, Views};
    use super::pipeline::PipelineEvent;
    use maxed_protocol::{defs::*, DecodeEvent, DecodedPacket};
    use std::sync::Mutex;

    fn nexus_map(name: &str) -> Packet {
        Packet::MapInfo(MapInfo {
            width: 1,
            height: 1,
            name: name.into(),
            displayName: name.into(),
            realmName: String::new(),
            fp: 1,
            seed: 1,
            background: 0,
            difficulty: 0.,
            allowPlayerTeleport: false,
            noSave: false,
            showDisplays: false,
            maxPlayerCount: 85,
            gameOpenedTime: 0,
            versionNumber: String::new(),
            unknown1: 0,
            viewDistance: 0,
            unknown2: false,
            unknownInt: 0,
            modifiers: Default::default(),
            dungeonModifiers: None,
            dungeonModifiers2: None,
            dungeonModifiers3: None,
            dungeonModifiers4: None,
            dungeonGrade: None,
            bgColor: 0,
            has_scores: false,
            maxRealmScore: 0,
            currentRealmScore: 0,
        })
    }

    for live in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let views = Arc::new(Views::default());
        *views.party.lock().unwrap() = roster(&[profile(9, Some("SynthPrevious"))]);
        let mut catalog = catalog();
        catalog.objects.push(ObjectDef {
            object_type: 100,
            name: "Synthetic Player".into(),
            is_player: true,
            is_boss: false,
            is_miniboss: false,
            class_name: None,
            dungeon_name: None,
            event_boss: None,
            realm_event_key: None,
            sprite_key: None,
        });
        catalog.dungeons.push(DungeonDef {
            name: "Nexus".into(),
            kind: AreaKind::Nexus,
            aliases: vec![],
            completion_index: None,
            portal_type: None,
            difficulty: None,
            exalt: false,
            exalt_gateway: false,
            category: None,
        });
        let seen = Arc::new(Mutex::new(Vec::new()));
        let received = seen.clone();
        let notices = Arc::new(Mutex::new(Vec::new()));
        let received_notices = notices.clone();
        let mut adapter = Adapter::open(
            root.path().join("party.sqlite3"),
            Arc::new(catalog),
            maxed_engine::EngineConfig::default(),
            maxed_store::WriterConfig {
                stream_id: "engine".into(),
                ..Default::default()
            },
            false,
            views.clone(),
            Bridge {
                event: Arc::new(move |event| received.lock().unwrap().push(event.event)),
                party_changed: Arc::new(move |notice| {
                    received_notices.lock().unwrap().push(notice)
                }),
                ..Default::default()
            },
        )
        .unwrap();
        adapter.live = live;
        let settings = PartySettings::default();
        assert!(views
            .party
            .lock()
            .unwrap()
            .view(&context(&settings))
            .members
            .is_empty());
        let packet = |at, packet| {
            PipelineEvent::Decoded(DecodeEvent::Packet(DecodedPacket {
                conn: ConnId(1),
                dir: Direction::ServerToClient,
                at: Timestamp(at),
                id: 0,
                len: 0,
                alignment_verified: true,
                packet,
            }))
        };
        let stat = |id, value, name: Option<&str>| StatData {
            statTypeNum: id,
            statType: None,
            statValue: value,
            stringStatValue: name.map(str::to_owned),
            statValueTwo: 0,
        };
        adapter.decoded(packet(0, nexus_map("Nexus"))).unwrap();
        adapter.writer_flush_for_test();
        let receipt_count = || {
            super::receipt_tests::count(
                &rusqlite::Connection::open(root.path().join("party.sqlite3")).unwrap(),
            )
        };
        let initial_receipts = receipt_count();
        seen.lock().unwrap().clear();
        adapter
            .decoded(packet(
                1000,
                Packet::Update(Update {
                    pos: WorldPosData { x: 0.0, y: 0.0 },
                    levelType: 0,
                    tiles: vec![],
                    drops: vec![],
                    newObjects: vec![ObjectData {
                        objectType: 100,
                        status: ObjectStatusData {
                            objectId: 3,
                            pos: WorldPosData { x: 0.0, y: 0.0 },
                            stats: vec![
                                stat(31, 0, Some("SynthAlpha")),
                                stat(7, 20, None),
                                stat(8, 200, None),
                            ],
                        },
                    }],
                }),
            ))
            .unwrap();
        adapter.tick(Timestamp(250_000)).unwrap();
        let view = views.party.lock().unwrap().view(&context(&settings));
        assert_eq!(view.members.len(), 1);
        assert_eq!(view.area_kind, AreaKind::Nexus);
        assert_eq!(view.members[0].name.as_deref(), Some("SynthAlpha"));
        assert_eq!(view.members[0].equipment[0].item_type, Some(200));
        assert_eq!(view.members[0].present, Some(true));
        assert!(
            seen.lock().unwrap().is_empty(),
            "bystander entry has no engine events"
        );
        adapter.publish_due(std::time::Duration::from_secs(1));
        notices.lock().unwrap().clear();
        seen.lock().unwrap().clear();
        adapter
            .decoded(packet(
                300_000,
                Packet::NewTick(NewTick {
                    tickId: 1,
                    tickTime: 200,
                    serverRealTimeMS: 0,
                    serverLastTimeRTTMS: 0,
                    status: vec![ObjectStatusData {
                        objectId: 3,
                        pos: WorldPosData { x: 0.0, y: 0.0 },
                        stats: vec![stat(8, 201, None)],
                    }],
                }),
            ))
            .unwrap();
        adapter.tick(Timestamp(500_000)).unwrap();
        adapter.publish_due(std::time::Duration::from_secs(2));
        adapter.publish_due(std::time::Duration::from_secs(3));
        assert_eq!(notices.lock().unwrap().len(), 1);
        assert_eq!(notices.lock().unwrap()[0].scope_id, view.scope_id);
        assert!(
            seen.lock().unwrap().is_empty(),
            "weapon-only change has no engine events"
        );
        assert_eq!(
            views
                .party
                .lock()
                .unwrap()
                .view(&context(&settings))
                .members[0]
                .equipment[0]
                .item_type,
            Some(201)
        );
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .all(|event| !matches!(event, EngineEvent::PlayerObserved(_))));
        adapter.writer_flush_for_test();
        let saved = maxed_store::open_reader(&root.path().join("party.sqlite3"))
            .unwrap()
            .list_party_players("", PartyPlayerFilter::All, PartyPlayerSort::LastSeen, None)
            .unwrap();
        assert!(
            saved.rows.is_empty(),
            "bystander profile and notice must not enter the writer"
        );
        let update = |new_objects, drops| {
            Packet::Update(Update {
                pos: WorldPosData { x: 0.0, y: 0.0 },
                levelType: 0,
                tiles: vec![],
                drops,
                newObjects: new_objects,
            })
        };
        adapter
            .decoded(packet(600_000, update(vec![], vec![3])))
            .unwrap();
        let absent = views.party.lock().unwrap().view(&context(&settings));
        assert_eq!(absent.members[0].present, Some(false));
        assert_eq!(
            absent.members[0].out_of_view_since,
            Some(Timestamp(600_000))
        );
        let object = |id, stats| ObjectData {
            objectType: 100,
            status: ObjectStatusData {
                objectId: id,
                pos: WorldPosData { x: 0.0, y: 0.0 },
                stats,
            },
        };
        adapter
            .decoded(packet(700_000, update(vec![object(3, vec![])], vec![])))
            .unwrap();
        let present = views.party.lock().unwrap().view(&context(&settings));
        assert_eq!(present.members.len(), 1);
        assert_eq!(present.members[0].present, Some(true));
        assert_eq!(present.members[0].player_id, view.members[0].player_id);
        assert!(
            seen.lock().unwrap().is_empty(),
            "presence changes have no engine events"
        );
        adapter.writer_flush_for_test();
        assert_eq!(
            receipt_count(),
            initial_receipts,
            "roster updates create no receipts"
        );

        // Old-scope absences must precede MapEntered, even without a new profile
        // to clear an accidentally restored old scope afterwards.
        adapter
            .decoded(packet(800_000, nexus_map("Nexus")))
            .unwrap();
        let next = views.party.lock().unwrap().view(&context(&settings));
        assert!(next.members.is_empty());
        assert_eq!(next.scope_id, None);

        // A single packet can publish provisional presence before its identity
        // merge. Applying presence after all events would leave a phantom row.
        adapter
            .decoded(packet(
                900_000,
                Packet::CreateSuccess(CreateSuccess {
                    objectId: 1,
                    charId: 1,
                    str: String::new(),
                }),
            ))
            .unwrap();
        adapter
            .decoded(packet(
                1_000_000,
                update(
                    vec![
                        object(1, vec![stat(31, 0, Some("SynthLocal"))]),
                        object(1, vec![stat(38, 0, Some("synthetic-account"))]),
                    ],
                    vec![],
                ),
            ))
            .unwrap();
        let local = views.party.lock().unwrap().view(&context(&settings));
        assert_eq!(local.members.len(), 1);
        assert!(local.members[0].is_local);
        assert_eq!(local.members[0].present, Some(true));
        assert_ne!(local.scope_id, view.scope_id);
        adapter
            .finish(Timestamp(1_100_000), FightEnd::CaptureStopped)
            .unwrap();
        let stopped = views.party.lock().unwrap().view(&context(&settings));
        assert_eq!(stopped.members.len(), 1);
        assert_eq!(stopped.members[0].present, Some(false));
        adapter.discard();
    }
}

#[test]
fn live_and_saved_party_respect_selected_profile_provenance() {
    let settings = settings();
    let catalog = PartyCatalog::new(Arc::new(catalog()));
    let mut ctx = context(&settings);
    ctx.catalog = Some(&catalog);
    for (gated, count, expected, reason) in [
        (
            false,
            8,
            ParseResult::Check,
            Some(ParseReasonKind::StatsNotSeen),
        ),
        (true, 8, ParseResult::Pass, None),
        (
            true,
            4,
            ParseResult::Fail,
            Some(ParseReasonKind::BelowMaxed),
        ),
    ] {
        let mut older = profile(1, Some("SynthAlpha"));
        older.is_local = true;
        older.maxed_gated = true;
        let mut latest = profile(2, Some("SynthAlpha"));
        latest.last_seen.0 += 1;
        latest.is_local = gated;
        latest.maxed_gated = gated;
        latest.maxed = Some(count);
        let mut view = roster(&[older, latest]).view(&ctx);
        let member = &view.members[0];
        assert!(member.is_local);
        assert_eq!(member.maxed_gated, gated);
        assert_eq!(member.maxed, if gated { Some(count) } else { None });
        let parse = member.parse.as_ref().unwrap();
        assert_eq!(parse.result, expected);
        assert_eq!(parse.reasons.first().map(|r| r.kind.clone()), reason);
        super::party::parse_saved_party(&mut view, &settings, Some(&catalog));
        let parse = view.members[0].parse.as_ref().unwrap();
        assert_eq!(parse.result, expected);
        assert_eq!(parse.reasons.first().map(|r| r.kind.clone()), reason);
    }
}

#[test]
fn saved_party_clears_legacy_local_count_before_parsing() {
    let settings = settings();
    let catalog = PartyCatalog::new(Arc::new(catalog()));
    // Deserialize a saved DTO directly, bypassing live roster normalization.
    // The missing provenance marker represents a legacy saved member.
    let mut view: PartyView = serde_json::from_value(serde_json::json!({
        "scope_id": "synthetic-scope",
        "run_id": "synthetic-run",
        "area_kind": "dungeon",
        "live": false,
        "counts": {
            "in_view": 0, "pass": 0, "fail": 0, "check": 0,
            "not_seen": 0, "no_damage_seen": 0, "left_early": 0
        },
        "classes": [],
        "members": [{
            "player_id": "synthetic-local",
            "is_local": true,
            "level": 20,
            "maxed": 8,
            "equipment": profile(1, Some("SynthAlpha")).equipment,
            "flags": [],
            "runs_together": 0
        }],
        "assets_available": true,
        "gear_recorded": true
    }))
    .unwrap();
    assert!(!view.live);
    assert!(!view.members[0].maxed_gated);
    assert_eq!(view.members[0].maxed, Some(8));

    super::party::parse_saved_party(&mut view, &settings, Some(&catalog));

    let member = &view.members[0];
    assert_eq!(member.maxed, None);
    let parse = member.parse.as_ref().unwrap();
    assert_eq!(parse.result, ParseResult::Check);
    assert_eq!(parse.reasons.len(), 1);
    assert_eq!(parse.reasons[0].kind, ParseReasonKind::StatsNotSeen);
    assert_eq!(view.counts.check, 1);
    assert_eq!(view.counts.fail, 0);
    assert_eq!(view.counts.pass, 0);
}
