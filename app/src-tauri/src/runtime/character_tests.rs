//! Synthetic native-boundary tests. Windows executes these; macOS only links them.
use super::character::{CharacterInputs, CharacterViewState};
use super::*;
use maxed_engine::character::input_revision;
use maxed_types::*;

fn inputs() -> CharacterInputs {
    let mut snapshot = maxed_types::demo::demo_snapshot();
    let mut catalog = maxed_types::demo::demo_catalog();
    catalog.client_fingerprint = "a".repeat(64);
    let character = snapshot.character.as_mut().unwrap();
    character.died_at = None;
    character.stats = vec![
        StatView {
            stat_id: 20,
            value: 75,
        },
        StatView {
            stat_id: 28,
            value: 75,
        },
        StatView {
            stat_id: 29,
            value: 0,
        },
    ];
    let weapon = &mut catalog.items[0];
    weapon.projectiles = vec![ProjectileDef {
        index: 0,
        min_damage: 100,
        max_damage: 100,
        armor_piercing: false,
        lifetime_millis: None,
    }];
    weapon.weapon_firing = Some(WeaponFiringDef {
        subattack_projectile_indices: None,
        support: CatalogSupport::Supported,
        omitted_projectile_indices: vec![],
        reasons: vec![],
        groups: vec![WeaponAttackGroup {
            group_id: "normal".into(),
            projectile_index: 0,
            count: 1,
            rate_factor: 1.0,
            schedule: WeaponSchedule::Simultaneous,
            reasons: vec![],
        }],
    });
    CharacterInputs::new(
        Arc::new(()),
        "synthetic-character-stream".into(),
        snapshot.character,
        Some(Arc::new(catalog)),
        CaptureStatus {
            state: CaptureState::Live,
            ..Default::default()
        },
    )
}

fn runtime(input: CharacterInputs) -> Arc<CaptureRuntime> {
    let runtime = Arc::new(CaptureRuntime::default());
    let mut view = runtime.shared.views.character.lock().unwrap();
    view.capture_changed(CaptureState::Live);
    view.observe(CharacterInputs::new(
        input.generation,
        input.stream_id,
        input.character,
        input.catalog,
        input.capture,
    ));
    let input = view.input.clone().unwrap();
    drop(view);
    cache_snapshot(&runtime, &input);
    runtime
}

fn cache_snapshot(runtime: &CaptureRuntime, input: &CharacterInputs) {
    let mut snapshot = runtime.shared.views.snapshot.lock().unwrap();
    snapshot.stream_id = input.stream_id.clone();
    snapshot.character = input.character.clone();
    snapshot.capture = input.capture.clone();
}

fn request(runtime: &CaptureRuntime) -> CharacterCalculationRequest {
    CharacterCalculationRequest {
        defense: 50,
        comparison_item_types: vec![],
        expected_revision: runtime
            .shared
            .views
            .character
            .lock()
            .unwrap()
            .input
            .as_ref()
            .unwrap()
            .character
            .as_ref()
            .unwrap()
            .calculation_revision
            .clone()
            .unwrap(),
    }
}

fn change(runtime: &CaptureRuntime, change: impl FnOnce(&mut CharacterInputs)) {
    let mut view = runtime.shared.views.character.lock().unwrap();
    let mut input = (**view.input.as_ref().unwrap()).clone();
    change(&mut input);
    view.observe(CharacterInputs::new(
        input.generation,
        input.stream_id,
        input.character,
        input.catalog,
        input.capture,
    ));
    let input = view.input.clone().unwrap();
    drop(view);
    cache_snapshot(runtime, &input);
}

#[test]
fn local_inputs_and_partial_evidence_survive_finite_serialization() {
    let mut input = inputs();
    let local = input.character.as_mut().unwrap();
    local.identity_verified = false;
    local.issues = vec![CoverageIssue::MissingCatalog];
    let runtime = runtime(input);
    let request = request(&runtime);
    let result = runtime.get_character_calculation(request.clone()).unwrap();
    assert_eq!(result.revision, request.expected_revision);
    assert_eq!(result.applied_defense, 50);
    assert_eq!(result.weapons[0].applied_att, Some(75));
    assert_eq!(result.weapons[0].dps_total.value, Some(1200.0));
    assert_eq!(result.issues, vec![CoverageIssue::MissingCatalog]);
    assert_eq!(result.freshness, CharacterFreshness::Live);
    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(
        serde_json::from_value::<CharacterCalculation>(json).unwrap(),
        result
    );
    assert!(runtime.shared.snapshot_requests.lock().unwrap().is_none());
    assert_eq!(runtime.store_status().state, StoreState::Closed);
}

#[test]
fn no_local_character_is_unavailable() {
    let runtime = runtime(inputs());
    let request = request(&runtime);
    change(&runtime, |input| input.character = None);
    assert_eq!(
        runtime.get_character_calculation(request).unwrap_err(),
        "no_local_character"
    );
}

#[test]
fn validates_controls_and_never_accepts_ui_player_inputs() {
    let runtime = runtime(inputs());
    let valid = request(&runtime);
    for comparisons in [vec![1, 2, 3], vec![1, 1]] {
        let mut invalid = valid.clone();
        invalid.comparison_item_types = comparisons;
        assert_eq!(
            runtime.get_character_calculation(invalid).unwrap_err(),
            "invalid_character_comparisons"
        );
    }
    let mut invalid = valid.clone();
    invalid.defense = 10_001;
    assert_eq!(
        runtime.get_character_calculation(invalid).unwrap_err(),
        "invalid_character_defense"
    );
    for field in ["stats", "equipment", "character", "player_id"] {
        let mut json = serde_json::to_value(&valid).unwrap();
        json[field] = serde_json::json!([]);
        assert!(serde_json::from_value::<CharacterCalculationRequest>(json).is_err());
    }
    for defense in [
        serde_json::json!(-1),
        serde_json::json!(1.5),
        serde_json::json!(65_536),
    ] {
        let mut json = serde_json::to_value(&valid).unwrap();
        json["defense"] = defense;
        assert!(serde_json::from_value::<CharacterCalculationRequest>(json).is_err());
    }
}

#[test]
fn comparisons_reject_only_confirmed_non_weapons() {
    for slot in [
        None,
        Some(1),
        Some(2),
        Some(3),
        Some(8),
        Some(17),
        Some(24),
        Some(4),
    ] {
        let mut input = inputs();
        let catalog = Arc::make_mut(input.catalog.as_mut().unwrap());
        catalog.items[0].slot_type = slot;
        let item_type = catalog.items[0].object_type;
        let runtime = runtime(input);
        let mut request = request(&runtime);
        request.comparison_item_types = vec![item_type];
        let result = runtime.get_character_calculation(request);
        if slot == Some(4) {
            assert_eq!(
                result.unwrap_err(),
                "invalid_character_comparison_selection"
            );
        } else {
            assert_eq!(result.unwrap().weapons.len(), 2);
        }
    }
    let runtime = runtime(inputs());
    let mut request = request(&runtime);
    request.comparison_item_types = vec![u16::MAX];
    let result = runtime.get_character_calculation(request).unwrap();
    assert_eq!(
        result.weapons[1].dps_total.completeness,
        CalculationCompleteness::Unavailable
    );
}

#[test]
fn missing_catalog_and_empty_weapon_are_distinct() {
    let mut input = inputs();
    input.catalog = None;
    input.character.as_mut().unwrap().equipment = vec![EquipmentView {
        slot: 0,
        item_type: None,
        observed_empty: Some(true),
        enchant: EnchantView {
            state: EnchantState::RecordedEmpty,
            slots: Some(0),
            applied: Some(0),
            ids: vec![],
        },
    }];
    let runtime = runtime(input);
    let result = runtime
        .get_character_calculation(request(&runtime))
        .unwrap();
    assert_eq!(result.catalog_fingerprint, None);
    assert!(result.weapons[0]
        .dps_total
        .reasons
        .iter()
        .any(|r| r == "empty_weapon_slot"));
    assert!(!result.weapons[0]
        .dps_total
        .reasons
        .iter()
        .any(|r| r == "missing_catalog"));
}

#[test]
fn same_time_stat_identity_stream_and_catalog_changes_reject_old_revisions() {
    for transition in 0..4 {
        let runtime = runtime(inputs());
        let old = request(&runtime);
        change(&runtime, |input| match transition {
            0 => input.character.as_mut().unwrap().stats.push(StatView {
                stat_id: 20,
                value: 999,
            }),
            1 => {
                let c = input.character.as_mut().unwrap();
                c.id = "merged-local".into();
                c.identity_verified = true;
            }
            2 => {
                input.stream_id = "new-stream".into();
                input.generation = Arc::new(());
            }
            _ => Arc::make_mut(input.catalog.as_mut().unwrap()).client_fingerprint = "b".repeat(64),
        });
        assert_ne!(request(&runtime).expected_revision, old.expected_revision);
        assert_eq!(
            runtime.get_character_calculation(old).unwrap_err(),
            "stale_character_revision"
        );
    }
}

#[test]
fn freshness_tracks_stop_recovery_disconnect_failure_and_death() {
    let runtime = runtime(inputs());
    for state in [
        CaptureState::Stopped,
        CaptureState::Recovering,
        CaptureState::WaitingForGame,
        CaptureState::Failed,
        CaptureState::Live,
    ] {
        let sink: StatusSink = Arc::new(|_| {});
        runtime.shared.publish(
            CaptureStatus {
                state: state.clone(),
                ..Default::default()
            },
            &sink,
        );
        let result = runtime
            .get_character_calculation(request(&runtime))
            .unwrap();
        assert_eq!(
            result.freshness,
            if state == CaptureState::Live {
                CharacterFreshness::Live
            } else {
                CharacterFreshness::LastSeen
            }
        );
    }
    change(&runtime, |input| {
        input.capture.state = CaptureState::Recovering
    });
    assert_eq!(
        runtime
            .get_character_calculation(request(&runtime))
            .unwrap()
            .freshness,
        CharacterFreshness::LastSeen
    );
    change(&runtime, |input| {
        input.character.as_mut().unwrap().died_at = Some(Timestamp(1))
    });
    assert_eq!(
        runtime
            .get_character_calculation(request(&runtime))
            .unwrap()
            .freshness,
        CharacterFreshness::Died
    );
}

#[test]
fn transitions_during_calculation_never_escape_as_current_even_after_aba() {
    for transition in 0..5 {
        let runtime = runtime(inputs());
        let request = request(&runtime);
        let changing = runtime.clone();
        *runtime.shared.after_character_calculation.lock().unwrap() = Some(Box::new(move || {
            change(&changing, |input| match transition {
                0 => input.character.as_mut().unwrap().id = "next-character".into(),
                1 => input.catalog = None,
                2 => {
                    input.generation = Arc::new(());
                    input.stream_id = "next-stream".into();
                }
                // Same fingerprint replacement is still another immutable generation.
                3 => input.catalog = input.catalog.as_ref().map(|c| Arc::new((**c).clone())),
                _ => input.character.as_mut().unwrap().stats.push(StatView {
                    stat_id: 28,
                    value: 123,
                }),
            });
            if transition == 4 {
                change(&changing, |input| {
                    input.character.as_mut().unwrap().stats.pop();
                });
            }
        }));
        assert_eq!(
            runtime.get_character_calculation(request).unwrap_err(),
            "stale_character_revision"
        );
    }
}

#[test]
fn unrelated_snapshot_updates_do_not_invalidate_the_input_hash() {
    let runtime = runtime(inputs());
    let before = request(&runtime);
    let updating = runtime.clone();
    *runtime.shared.after_character_calculation.lock().unwrap() = Some(Box::new(move || {
        change(&updating, |input| {
            let character = input.character.as_mut().unwrap();
            character.observed_at = Timestamp(999);
            character
                .stats
                .iter_mut()
                .find(|s| s.stat_id == 29)
                .unwrap()
                .value ^= 0x8000;
        });
        let mut snapshot = updating.shared.views.snapshot.lock().unwrap();
        snapshot.sequence = Sequence(Timestamp(999));
        snapshot.fights = maxed_types::demo::demo_snapshot().fights;
    }));
    assert_eq!(
        runtime
            .get_character_calculation(before.clone())
            .unwrap()
            .revision,
        before.expected_revision
    );
}

#[test]
fn stop_does_not_wait_for_calculation_or_relabel_its_reply_live() {
    let runtime = runtime(inputs());
    let request = request(&runtime);
    let (entered, waiting) = bounded(1);
    let (release, released) = bounded(1);
    *runtime.shared.after_character_calculation.lock().unwrap() = Some(Box::new(move || {
        entered.send(()).unwrap();
        released.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let calculating = runtime.clone();
    let worker = thread::spawn(move || calculating.get_character_calculation(request));
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let stopping = runtime.clone();
    let (done, stopped) = bounded(1);
    let stopper = thread::spawn(move || {
        stopping.stop().unwrap();
        done.send(()).unwrap();
    });
    stopped
        .recv_timeout(Duration::from_secs(5))
        .expect("Stop cannot need the calculation's locks");
    release.send(()).unwrap();
    assert_eq!(
        worker.join().unwrap().unwrap_err(),
        "stale_character_revision"
    );
    stopper.join().unwrap();
}

#[test]
fn command_is_registered_with_the_typed_request() {
    let source = include_str!("../lib.rs");
    let registration = source
        .split("tauri::generate_handler![")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap();
    assert!(registration
        .lines()
        .any(|line| line.trim() == "get_character_calculation,"));
    // The generated command wrapper also has to type-check in the Windows build.
    let _command = crate::get_character_calculation;
}

#[test]
fn observation_revision_is_derived_from_its_own_catalog() {
    let input = inputs();
    let expected = input_revision(
        &input.stream_id,
        input.character.as_ref().unwrap(),
        input.catalog.as_deref(),
    );
    let mut state = CharacterViewState::default();
    state.observe(input);
    assert_eq!(
        state
            .input
            .unwrap()
            .character
            .as_ref()
            .unwrap()
            .calculation_revision,
        Some(expected)
    );
}

#[test]
fn adapter_full_events_remain_byte_identical_and_replay_after_finalization() {
    use super::history_replay_tests::reconstruction as packets;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("canonical.sqlite3");
    let (send, delivered) = crossbeam_channel::unbounded();
    let mut adapter = Adapter::open(
        path.clone(),
        Arc::new(packets::catalog()),
        maxed_engine::EngineConfig::default(),
        maxed_store::WriterConfig {
            stream_id: "engine".into(),
            ..Default::default()
        },
        false,
        Arc::new(Views::default()),
        Bridge {
            event: Arc::new(move |event| send.send(event).unwrap()),
            ..Default::default()
        },
    )
    .unwrap();
    let mut originals = vec![];
    for (at, packet) in [
        (0, packets::map("The Void")),
        (1000, packets::create(1, 1)),
        (2000, packets::update(vec![packets::player()], vec![])),
        (300_000, packets::tick(vec![])),
    ] {
        let events = packets::apply(&mut adapter.engine, at, packet);
        originals.extend(events.clone());
        adapter.events(events);
    }
    assert!(originals
        .iter()
        .any(|event| matches!(event, EngineEvent::CharacterUpdated(_))));
    adapter
        .finish(Timestamp(500_000), FightEnd::CaptureStopped)
        .unwrap();
    adapter.writer_flush_for_test();
    let delivered: Vec<_> = delivered.try_iter().collect();
    let mut actual = rusqlite::Connection::open(&path).unwrap();
    let before = super::receipt_tests::rows(&actual);
    // Replay exactly the callback payload, without stripping presentation fields.
    // An annotated duplicate conflicts with the writer's canonical receipt digest.
    maxed_store::apply_batch(&mut actual, &delivered).unwrap();
    assert_eq!(super::receipt_tests::rows(&actual), before);
    assert!(!serde_json::to_string(&delivered)
        .unwrap()
        .contains("calculation_revision"));
    for (envelope, original) in delivered.iter().zip(originals) {
        assert_eq!(
            serde_json::to_vec(&envelope.event).unwrap(),
            serde_json::to_vec(&original).unwrap()
        );
    }
    let mut oracle = rusqlite::Connection::open_in_memory().unwrap();
    maxed_store::migrate(&mut oracle).unwrap();
    maxed_store::apply_batch(&mut oracle, &delivered).unwrap();
    assert_eq!(super::receipt_tests::rows(&oracle), before);
    adapter.close().unwrap();
}

#[test]
fn adapter_annotates_only_presentation_and_keeps_damage_and_receipts_unchanged() {
    use super::history_replay_tests::reconstruction as packets;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("actual.sqlite3");
    let runtime = Arc::new(CaptureRuntime::default());
    let (send, delivered) = crossbeam_channel::unbounded();
    let mut catalog = packets::catalog();
    catalog.client_fingerprint = "c".repeat(64);
    let catalog = Arc::new(catalog);
    let mut adapter = Adapter::open(
        path.clone(),
        catalog.clone(),
        maxed_engine::EngineConfig::default(),
        maxed_store::WriterConfig {
            stream_id: "engine".into(),
            ..Default::default()
        },
        false,
        runtime.shared.views.clone(),
        Bridge {
            batch: Some(Arc::new(move |events| {
                for event in events {
                    send.send(event).unwrap();
                }
            })),
            ..Default::default()
        },
    )
    .unwrap();
    let mut originals = vec![];
    for (at, packet) in [
        (0, packets::map("The Void")),
        (1000, packets::create(1, 1)),
        (
            2000,
            packets::update(vec![packets::player(), packets::target()], vec![]),
        ),
        (3000, packets::damage(1, 1, 100)),
        (300_000, packets::tick(vec![])),
    ] {
        let events = packets::apply(&mut adapter.engine, at, packet);
        originals.extend(events.clone());
        adapter.events(events);
    }
    let snapshot = adapter.read_snapshot();
    let character = snapshot.character.as_ref().unwrap();
    assert_eq!(
        character.calculation_revision,
        Some(input_revision("engine", character, Some(&catalog)))
    );
    let engine_before = adapter.engine.snapshot();
    assert!(engine_before
        .character
        .as_ref()
        .unwrap()
        .calculation_revision
        .is_none());
    let delivered: Vec<_> = delivered.try_iter().collect();
    assert_eq!(delivered.len(), originals.len());
    let mut durable = delivered.clone();
    let mut annotated = 0;
    for ((copy, wire), original) in durable.iter_mut().zip(&delivered).zip(originals) {
        if let EngineEvent::CharacterUpdated(character) = &wire.event {
            assert!(character.calculation_revision.is_some());
            annotated += 1;
        }
        copy.event = original;
        assert!(!serde_json::to_string(copy)
            .unwrap()
            .contains("calculation_revision"));
    }
    assert!(annotated > 0);
    adapter.drain().unwrap();
    adapter.writer_flush_for_test();
    let mut oracle = rusqlite::Connection::open(directory.path().join("oracle.sqlite3")).unwrap();
    maxed_store::migrate(&mut oracle).unwrap();
    maxed_store::apply_batch(&mut oracle, &durable).unwrap();
    let actual = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        super::receipt_tests::rows(&actual),
        super::receipt_tests::rows(&oracle)
    );
    let before_receipts = super::receipt_tests::rows(&actual);
    runtime
        .get_character_calculation(request(&runtime))
        .unwrap();
    assert_eq!(adapter.engine.snapshot(), engine_before);
    assert_eq!(super::receipt_tests::rows(&actual), before_receipts);
    assert_eq!(adapter.read_snapshot(), snapshot);
    adapter.close().unwrap();
}

#[test]
fn owner_character_wait_releases_locks_so_stop_can_join() {
    let runtime = runtime(inputs());
    let request = request(&runtime);
    let (requests, reads) = bounded::<CharacterRequest>(0);
    *runtime.shared.character_requests.lock().unwrap() = Some(requests);
    let (stop, stopped) = bounded(1);
    let (claimed, waiting) = bounded(1);
    let worker = thread::spawn(move || {
        let reply = reads.recv_timeout(Duration::from_secs(5)).unwrap();
        claimed.send(()).unwrap();
        stopped.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(reply);
    });
    *runtime.session.lock().unwrap() = Some(Session {
        stop,
        cancel: Arc::new(AtomicBool::new(false)),
        worker,
        sink: Arc::new(|_| {}),
    });
    let calculating = runtime.clone();
    let (done, finished) = bounded(1);
    let calculation = thread::spawn(move || {
        done.send(calculating.get_character_calculation(request))
            .unwrap()
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let stopping = runtime.clone();
    let (done, stops) = bounded(1);
    let stopper = thread::spawn(move || done.send(stopping.stop()).unwrap());
    stops
        .recv_timeout(Duration::from_secs(5))
        .expect("Stop joins while the owner read is waiting")
        .unwrap();
    let result = finished.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(result.is_err() || result.unwrap().freshness == CharacterFreshness::LastSeen);
    calculation.join().unwrap();
    stopper.join().unwrap();
}

#[test]
fn reset_snapshot_cannot_reuse_retired_character_inputs() {
    let runtime = runtime(inputs());
    let request = request(&runtime);
    {
        let mut snapshot = runtime.shared.views.snapshot.lock().unwrap();
        snapshot.stream_id = "stopped-new-namespace".into();
        snapshot.character = None;
    }
    assert_eq!(
        runtime.get_character_calculation(request).unwrap_err(),
        "no_local_character"
    );
}

#[test]
fn snapshot_from_another_stream_cannot_borrow_the_retained_catalog() {
    let runtime = runtime(inputs());
    let request = request(&runtime);
    runtime.shared.views.snapshot.lock().unwrap().stream_id = "other-stream".into();
    assert_eq!(
        runtime.get_character_calculation(request).unwrap_err(),
        "stale_character_revision"
    );
}

#[test]
fn replacement_adapter_publishes_its_catalog_revision_on_the_next_snapshot() {
    use super::history_replay_tests::reconstruction as packets;
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(CaptureRuntime::default());
    let mut previous = None;
    for (stream, fingerprint, missing) in [
        ("first-stream", "d", false),
        ("second-stream", "e", false),
        ("missing-stream", "f", true),
    ] {
        let mut catalog = packets::catalog();
        catalog.client_fingerprint = fingerprint.repeat(64);
        let mut adapter = Adapter::open(
            directory.path().join(stream),
            Arc::new(catalog),
            maxed_engine::EngineConfig {
                stream_id: stream.into(),
                ..Default::default()
            },
            maxed_store::WriterConfig {
                stream_id: stream.into(),
                ..Default::default()
            },
            missing,
            runtime.shared.views.clone(),
            Bridge::default(),
        )
        .unwrap();
        for (at, packet) in [
            (0, packets::map("The Void")),
            (1000, packets::create(1, 1)),
            (2000, packets::update(vec![packets::player()], vec![])),
            (300_000, packets::tick(vec![])),
        ] {
            let events = packets::apply(&mut adapter.engine, at, packet);
            adapter.events(events);
        }
        adapter.publish_snapshot();
        let snapshot = runtime.snapshot();
        let character = snapshot.character.unwrap();
        let revision = character.calculation_revision.unwrap();
        assert_eq!(revision.stream_id, stream);
        assert_eq!(
            revision.catalog_fingerprint,
            (!missing).then(|| fingerprint.repeat(64))
        );
        if missing {
            assert!(character.issues.contains(&CoverageIssue::MissingCatalog));
        }
        if let Some(old) = previous {
            assert_eq!(
                runtime.get_character_calculation(old).unwrap_err(),
                "stale_character_revision"
            );
        }
        let current = request(&runtime);
        assert_eq!(current.expected_revision, revision);
        assert_eq!(
            runtime
                .get_character_calculation(current.clone())
                .unwrap()
                .revision,
            revision
        );
        previous = Some(current);
        adapter.close().unwrap();
    }
}

#[test]
fn failed_workers_and_bridge_delivery_cannot_leave_a_live_result() {
    for bridge_failure in [false, true] {
        let runtime = runtime(inputs());
        if bridge_failure {
            runtime.shared.bridge_failed.store(true, Ordering::Release);
        } else {
            runtime.shared.failed.store(true, Ordering::Release);
        }
        assert_eq!(
            runtime
                .get_character_calculation(request(&runtime))
                .unwrap()
                .freshness,
            CharacterFreshness::LastSeen
        );
    }
}

fn finalization_adapter(
    runtime: &CaptureRuntime,
    path: std::path::PathBuf,
    stream: &str,
    fingerprint: char,
) -> Adapter {
    let mut catalog = super::history_replay_tests::reconstruction::catalog();
    catalog.client_fingerprint = fingerprint.to_string().repeat(64);
    Adapter::open(
        path,
        Arc::new(catalog),
        maxed_engine::EngineConfig {
            stream_id: stream.into(),
            ..Default::default()
        },
        maxed_store::WriterConfig {
            stream_id: stream.into(),
            ..Default::default()
        },
        false,
        runtime.shared.views.clone(),
        Bridge::default(),
    )
    .unwrap()
}

fn finalization_packet(adapter: &mut Adapter, at: u64, packet: Packet) {
    adapter
        .decoded(PipelineEvent::Decoded(DecodeEvent::Packet(
            maxed_protocol::DecodedPacket {
                conn: ConnId(1),
                dir: Direction::ServerToClient,
                at: Timestamp(at),
                id: 0,
                len: 0,
                alignment_verified: true,
                packet,
            },
        )))
        .unwrap();
}

fn observe_before_finalization(runtime: &CaptureRuntime, adapter: &mut Adapter) {
    use super::history_replay_tests::reconstruction as packets;
    let live = CaptureStatus {
        state: CaptureState::Live,
        ..Default::default()
    };
    let sink: StatusSink = Arc::new(|_| {});
    runtime.shared.publish(live.clone(), &sink);
    adapter.capture(live).unwrap();
    for (at, packet) in [
        (0, packets::map("The Void")),
        (1000, packets::create(1, 1)),
        (2000, packets::update(vec![packets::player()], vec![])),
        (300_000, packets::tick(vec![])),
    ] {
        finalization_packet(adapter, at, packet);
    }
    assert!(adapter.read_snapshot().character.is_some());
}

fn assert_real_finalization_retains_character(died: bool) {
    let directory = tempfile::tempdir().unwrap();
    let runtime = CaptureRuntime::default();
    let mut adapter =
        finalization_adapter(&runtime, directory.path().join("retiring"), "retiring", 'a');
    observe_before_finalization(&runtime, &mut adapter);
    if died {
        finalization_packet(
            &mut adapter,
            400_000,
            super::history_replay_tests::reconstruction::death(),
        );
    }
    let before = adapter.read_snapshot().character.unwrap();
    assert_eq!(before.died_at.is_some(), died);
    adapter
        .finish(Timestamp(500_000), FightEnd::CaptureStopped)
        .unwrap();
    assert!(
        adapter.engine.snapshot().character.is_none(),
        "engine finalization still clears the character"
    );
    let stopped = adapter
        .read_snapshot()
        .character
        .expect("presentation retains the retiring observation");
    assert_eq!(stopped.id, before.id);
    assert_eq!(stopped.stats, before.stats);
    assert_eq!(stopped.equipment, before.equipment);
    assert_eq!(stopped.died_at, before.died_at);
    let retained = request(&runtime);
    let expected = if died {
        CharacterFreshness::Died
    } else {
        CharacterFreshness::LastSeen
    };
    // The owner finalizes before native Stopped delivery. It must already be terminal.
    assert_eq!(runtime.status().state, CaptureState::Live);
    assert_eq!(
        runtime
            .get_character_calculation(retained.clone())
            .unwrap()
            .freshness,
        expected
    );
    adapter.capture(CaptureStatus::default()).unwrap();
    adapter.publish_snapshot();
    adapter
        .finish(Timestamp(500_000), FightEnd::CaptureStopped)
        .unwrap();
    adapter.close().unwrap();
    drop(adapter);
    let result = runtime.get_character_calculation(retained).unwrap();
    assert_eq!(result.freshness, expected);
    assert_eq!(result.catalog_fingerprint, Some("a".repeat(64)));
    assert_eq!(runtime.snapshot().character.unwrap().id, before.id);
}

#[test]
fn real_adapter_stop_retains_living_character_as_last_seen() {
    assert_real_finalization_retains_character(false);
}

#[test]
fn real_adapter_stop_retains_dead_character_as_died() {
    assert_real_finalization_retains_character(true);
}

#[test]
fn replacement_generation_clears_finalized_character_before_observing_new_inputs() {
    for replacement_stream in ["retiring", "replacement"] {
        let directory = tempfile::tempdir().unwrap();
        let runtime = CaptureRuntime::default();
        let mut retiring =
            finalization_adapter(&runtime, directory.path().join("old"), "retiring", 'a');
        observe_before_finalization(&runtime, &mut retiring);
        retiring
            .finish(Timestamp(500_000), FightEnd::CaptureStopped)
            .unwrap();
        let old = request(&runtime);
        retiring.close().unwrap();
        drop(retiring);
        let mut replacement = finalization_adapter(
            &runtime,
            directory.path().join("new"),
            replacement_stream,
            'b',
        );
        assert!(runtime.snapshot().character.is_none());
        {
            let view = runtime.shared.views.character.lock().unwrap();
            let input = view.input.as_ref().unwrap();
            assert!(input.character.is_none());
            assert_eq!(input.stream_id, replacement_stream);
            assert_eq!(
                input.catalog.as_ref().unwrap().client_fingerprint,
                "b".repeat(64)
            );
        }
        assert_eq!(
            runtime.get_character_calculation(old.clone()).unwrap_err(),
            "no_local_character"
        );
        observe_before_finalization(&runtime, &mut replacement);
        assert_eq!(
            runtime.get_character_calculation(old).unwrap_err(),
            "stale_character_revision"
        );
        let result = runtime
            .get_character_calculation(request(&runtime))
            .unwrap();
        assert_eq!(result.revision.stream_id, replacement_stream);
        assert_eq!(result.catalog_fingerprint, Some("b".repeat(64)));
        replacement.close().unwrap();
    }
}

#[test]
fn calculation_queries_owner_identity_twice_and_rejects_stream_changes() {
    for changed_stream in [false, true] {
        let runtime = runtime(inputs());
        let request = request(&runtime);
        let (requests, reads) = bounded::<CharacterRequest>(0);
        *runtime.shared.character_requests.lock().unwrap() = Some(requests);
        let owner = runtime.clone();
        let worker = thread::spawn(move || {
            for read in 0..2 {
                let reply = reads.recv_timeout(Duration::from_secs(5)).unwrap();
                let mut snapshot = owner.shared.views.snapshot.lock().unwrap();
                if changed_stream && read == 1 {
                    snapshot.stream_id = "replacement-stream".into();
                }
                reply
                    .send(super::character::CharacterIdentity::from_snapshot(
                        &snapshot,
                    ))
                    .unwrap();
            }
        });
        let result = runtime.get_character_calculation(request);
        worker.join().unwrap();
        if changed_stream {
            assert_eq!(result.unwrap_err(), "stale_character_revision");
        } else {
            assert!(result.is_ok());
        }
    }
}

#[test]
fn modeled_condition_changes_during_calculation_reject_the_reply() {
    for mask in [0x4, 0x40000] {
        let runtime = runtime(inputs());
        let request = request(&runtime);
        let updating = runtime.clone();
        *runtime.shared.after_character_calculation.lock().unwrap() = Some(Box::new(move || {
            change(&updating, |input| {
                input
                    .character
                    .as_mut()
                    .unwrap()
                    .stats
                    .iter_mut()
                    .find(|s| s.stat_id == 29)
                    .unwrap()
                    .value = mask;
            });
        }));
        assert_eq!(
            runtime.get_character_calculation(request).unwrap_err(),
            "stale_character_revision"
        );
    }
}

#[test]
fn real_worker_answers_character_queries_across_restart() {
    use maxed_capture::{
        backend::{start_with_backend, Backend, Device},
        AdapterInfo,
    };

    // Keep the real capture/owner loops alive without opening a native device.
    struct IdleBackend;
    struct IdleDevice;
    impl Device for IdleDevice {
        fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError> {
            Ok(None)
        }
    }
    impl Backend for IdleBackend {
        fn discover(&mut self) -> Result<Vec<AdapterInfo>, CaptureError> {
            Ok(vec![AdapterInfo {
                id: "synthetic".into(),
                description: "synthetic".into(),
            }])
        }
        fn open(&mut self, _: &AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
            Ok(Box::new(IdleDevice))
        }
    }

    let runtime = CaptureRuntime::default();
    let timeout = Duration::from_secs(5);
    let mut previous = None;
    for _ in 0..2 {
        runtime
            .start_with(Arc::new(|_| {}), || {
                start_with_backend(CaptureConfig::default(), || Ok(IdleBackend))
            })
            .unwrap();
        let requests = runtime
            .shared
            .character_requests
            .lock()
            .unwrap()
            .clone()
            .unwrap();
        // Direct channel replies prove the run_worker branch answered: no cached
        // fallback can make this pass if the branch is missing or disconnected.
        for _ in 0..2 {
            let (reply, response) = bounded(1);
            requests.send_timeout(reply, timeout).unwrap();
            let identity = response.recv_timeout(timeout).unwrap();
            let expected = super::character::CharacterIdentity::from_snapshot(
                &runtime.shared.views.snapshot.lock().unwrap(),
            );
            assert_eq!(identity, expected);
            if let Some(retired) = &previous {
                assert_ne!(&identity, retired);
            }
        }
        runtime.stop_and_flush().unwrap();
        assert!(requests.send_timeout(bounded(1).0, timeout).is_err());
        previous = Some(super::character::CharacterIdentity::from_snapshot(
            &runtime.shared.views.snapshot.lock().unwrap(),
        ));
    }
}
