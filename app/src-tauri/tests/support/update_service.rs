use super::*;
use std::sync::mpsc;

fn service() -> UpdateService {
    let mut service = UpdateService::new();
    service.factory = Arc::new(|| {
        Err(Error::Verification(
            super::super::verification::VerificationError::Key,
        ))
    });
    service
        .commit_channel(UpdateChannel::Stable, || Ok(()))
        .unwrap();
    service
}
fn offer(service: &UpdateService, phase: UpdatePhase) {
    let mut state = service.state.lock().unwrap();
    state.generation = 7;
    state.status.phase = phase;
    state.status.offer = Some(
        UpdateOffer::new(
            "7".into(),
            "2.0.0".into(),
            UpdateChannel::Stable,
            "Synthetic release".into(),
        )
        .unwrap(),
    );
}
fn join(service: &UpdateService) {
    let worker = service.state.lock().unwrap().worker.take();
    if let Some(worker) = worker {
        worker.join().unwrap();
    }
}

fn scripted_service(
    replies: Vec<Vec<u8>>,
) -> (
    UpdateService,
    Arc<super::super::transport::scripted::Script>,
) {
    let script = Arc::new(super::super::transport::scripted::Script::default());
    script.replies.lock().unwrap().extend(replies);
    let mut native = super::super::tests::verified_owner();
    native.http.script = Some(script.clone());
    native.set_channel(Channel::Stable).unwrap();
    let service = service();
    service.state.lock().unwrap().updater = Some(native);
    (service, script)
}

#[test]
fn manual_state_machine_requires_verified_download_and_explicit_install() {
    let (metadata, bytes) = super::super::tests::signed_reply();
    let mut tampered = bytes.clone();
    tampered[3] ^= 1;
    let (service, script) = scripted_service(vec![metadata.clone(), tampered, metadata, bytes]);
    assert_eq!(script.requests.load(Ordering::Acquire), 0);
    assert_eq!(service.check().phase, UpdatePhase::Checking);
    join(&service);
    assert_eq!(service.status().phase, UpdatePhase::Available);
    let first = service.status().offer.unwrap().request_id;
    assert_eq!(
        service.install(&first).unwrap().failure,
        Some(UpdateFailure::StaleOffer)
    );
    assert_eq!(
        service.download(&first).unwrap().phase,
        UpdatePhase::Downloading
    );
    join(&service);
    assert_eq!(
        service.status().failure,
        Some(UpdateFailure::InvalidSignature)
    );
    assert!(service
        .state
        .lock()
        .unwrap()
        .updater
        .as_ref()
        .unwrap()
        .verified
        .is_none());
    service.check();
    join(&service);
    let second = service.status().offer.unwrap().request_id;
    assert_ne!(first, second);
    assert_eq!(
        service.download(&first).unwrap().failure,
        Some(UpdateFailure::StaleOffer)
    );
    service.download(&second).unwrap();
    join(&service);
    assert_eq!(service.status().phase, UpdatePhase::Ready);
    assert_eq!(
        service.download(&second).unwrap().failure,
        Some(UpdateFailure::StaleOffer)
    );
    assert_eq!(script.requests.load(Ordering::Acquire), 4);
    let launched = Arc::new(AtomicBool::new(false));
    let flag = launched.clone();
    service.configure_installer(Arc::new(move |native| {
        native.install_with(
            tempfile::tempdir().unwrap().path(),
            || Ok(()),
            |_| {
                flag.store(true, Ordering::Release);
                Ok(())
            },
        )
    }));
    assert!(!launched.load(Ordering::Acquire));
    service.install(&second).unwrap();
    join(&service);
    assert!(launched.load(Ordering::Acquire));
    assert_eq!(service.status(), UpdateStatus::idle(UpdateChannel::Stable));
}

#[test]
fn delayed_check_and_download_cannot_survive_channel_change_or_cancel() {
    for downloading in [false, true] {
        for cancel in [false, true] {
            let (metadata, bytes) = super::super::tests::signed_reply();
            let (service, script) = scripted_service(vec![metadata, bytes]);
            if downloading {
                service.check();
                join(&service);
            }
            let gate = Arc::new(tokio::sync::Semaphore::new(0));
            *script.gate.lock().unwrap() = Some(gate.clone());
            if downloading {
                service
                    .download(&service.status().offer.unwrap().request_id)
                    .unwrap();
            } else {
                service.check();
            }
            let expected = if downloading { 2 } else { 1 };
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while script.requests.load(Ordering::Acquire) != expected {
                assert!(std::time::Instant::now() < until);
                std::thread::yield_now();
            }
            if cancel {
                service.cancel();
            } else {
                service
                    .commit_channel(UpdateChannel::Beta, || Ok(()))
                    .unwrap();
            }
            gate.add_permits(1);
            join(&service);
            assert_eq!(
                service.status(),
                UpdateStatus::idle(if cancel {
                    UpdateChannel::Stable
                } else {
                    UpdateChannel::Beta
                })
            );
            let state = service.state.lock().unwrap();
            assert!(state.updater.as_ref().unwrap().verified.is_none());
            assert!(state.updater.as_ref().unwrap().offer.is_none());
        }
    }
}

#[test]
fn no_newer_version_is_up_to_date_without_an_artifact_request() {
    let (metadata, _) = super::super::tests::signed_reply();
    let (service, script) = scripted_service(vec![metadata]);
    service
        .state
        .lock()
        .unwrap()
        .updater
        .as_mut()
        .unwrap()
        .current = semver::Version::parse("2.0.0").unwrap();
    service.check();
    join(&service);
    assert_eq!(service.status().phase, UpdatePhase::UpToDate);
    assert!(service.status().offer.is_none());
    assert_eq!(script.requests.load(Ordering::Acquire), 1);
}

#[test]
fn startup_never_constructs_network_owner_and_unresolved_configuration_is_safe() {
    let service = service();
    assert_eq!(service.status().phase, UpdatePhase::Idle);
    assert!(service.state.lock().unwrap().worker.is_none());
    assert_eq!(service.check().phase, UpdatePhase::Checking);
    join(&service);
    assert_eq!(service.status().failure, Some(UpdateFailure::Unavailable));
    assert_eq!(service.status().offer, None);
}

#[test]
fn failed_settings_commit_preserves_offer_and_success_retires_it() {
    let service = service();
    offer(&service, UpdatePhase::Ready);
    let before = service.status();
    assert!(service
        .commit_channel(UpdateChannel::Beta, || Err::<(), _>(
            "Synthetic write failure".into()
        ))
        .is_err());
    assert_eq!(service.status(), before);
    service
        .commit_channel(UpdateChannel::Beta, || Ok(()))
        .unwrap();
    assert_eq!(service.status(), UpdateStatus::idle(UpdateChannel::Beta));
    assert_eq!(
        service.download("7").unwrap().failure,
        Some(UpdateFailure::StaleOffer)
    );
    assert_eq!(
        service.install("7").unwrap().failure,
        Some(UpdateFailure::StaleOffer)
    );
}

#[test]
fn installing_freezes_cancel_channel_and_exit_without_running_commit() {
    let service = service();
    offer(&service, UpdatePhase::Installing);
    let called = AtomicBool::new(false);
    assert!(service
        .commit_channel(UpdateChannel::Beta, || {
            called.store(true, Ordering::Release);
            Ok(())
        })
        .is_err());
    assert!(!called.load(Ordering::Acquire));
    assert_eq!(service.cancel().failure, Some(UpdateFailure::Busy));
    assert_eq!(service.status().phase, UpdatePhase::Installing);
    assert!(service.shutdown().is_err());
    service.state.lock().unwrap().status.phase = UpdatePhase::Failed;
}

#[test]
fn slow_settings_write_does_not_block_status_or_enqueue_update_work() {
    let service = Arc::new(service());
    let (entered, waiting) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let owner = service.clone();
    let worker = std::thread::spawn(move || {
        owner.commit_channel(UpdateChannel::Beta, || {
            entered.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        })
    });
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(service.status().channel, UpdateChannel::Stable);
    assert_eq!(service.check().failure, Some(UpdateFailure::Busy));
    assert!(service.state.lock().unwrap().worker.is_none());
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert_eq!(service.status().channel, UpdateChannel::Beta);
}

#[test]
fn channel_change_retires_inflight_check() {
    let mut service = service();
    let (entered, waiting) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let released = Mutex::new(released);
    service.factory = Arc::new(move || {
        entered.send(()).unwrap();
        released
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        Err(Error::Network)
    });
    service.check();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(service.check().failure, Some(UpdateFailure::Busy));
    assert!(service
        .commit_channel(UpdateChannel::Beta, || Err::<(), _>(
            "Synthetic persistence failure".into()
        ))
        .is_err());
    assert_eq!(service.status().phase, UpdatePhase::Checking);
    service
        .commit_channel(UpdateChannel::Beta, || Ok(()))
        .unwrap();
    release.send(()).unwrap();
    join(&service);
    assert_eq!(service.status(), UpdateStatus::idle(UpdateChannel::Beta));
    let generation = service.state.lock().unwrap().generation;
    service
        .commit_channel(UpdateChannel::Stable, || Ok(()))
        .unwrap();
    assert!(service.state.lock().unwrap().generation > generation);
    assert_eq!(service.status(), UpdateStatus::idle(UpdateChannel::Stable));
}

#[test]
fn malformed_and_stale_ids_fail_before_owner_creation() {
    let service = service();
    for value in [
        "",
        "-1",
        "https://example.invalid",
        "１２",
        "123456789012345678901",
    ] {
        assert_eq!(
            service.download(value).unwrap_err(),
            "Invalid update request."
        );
        assert_eq!(
            service.install(value).unwrap_err(),
            "Invalid update request."
        );
    }
    assert_eq!(
        service.install("1").unwrap().failure,
        Some(UpdateFailure::StaleOffer)
    );
    assert!(service.state.lock().unwrap().worker.is_none());
    offer(&service, UpdatePhase::Available);
    service.state.lock().unwrap().generation = u64::MAX;
    assert_eq!(service.check().failure, Some(UpdateFailure::Unavailable));
    assert!(service.state.lock().unwrap().worker.is_none());
    assert!(service.status().offer.is_none());
}

#[test]
fn cancellation_drops_pending_network_future_before_completion() {
    struct Pending(Arc<AtomicBool>);
    impl Future for Pending {
        type Output = ();
        fn poll(self: std::pin::Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<()> {
            Poll::Pending
        }
    }
    impl Drop for Pending {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let cancel = AtomicBool::new(false);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let future = cancellable(&cancel, Pending(dropped.clone()));
        let mut future = std::pin::pin!(future);
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        cancel.store(true, Ordering::Release);
        assert_eq!(future.await, None);
    });
    assert!(dropped.load(Ordering::Acquire));
}

#[test]
fn cancelled_exit_retires_pending_status_after_join() {
    let mut service = service();
    let (entered, waiting) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let released = Mutex::new(released);
    service.factory = Arc::new(move || {
        entered.send(()).unwrap();
        released
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        Err(Error::Network)
    });
    let service = Arc::new(service);
    service.check();
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    let owner = service.clone();
    let shutdown = std::thread::spawn(move || owner.shutdown());
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while !service.state.lock().unwrap().cancel.load(Ordering::Acquire) {
        assert!(std::time::Instant::now() < until);
        std::thread::yield_now();
    }
    release.send(()).unwrap();
    shutdown.join().unwrap().unwrap();
    service.resume();
    assert_eq!(service.status(), UpdateStatus::idle(UpdateChannel::Stable));
    assert!(service.state.lock().unwrap().worker.is_none());
}

#[test]
fn native_metadata_parser_bounds_notes_and_distinguishes_unsupported_target() {
    use super::super::{parse_offer, policy, TARGET};
    let current = semver::Version::parse("1.0.0").unwrap();
    let next = semver::Version::parse("2.0.0").unwrap();
    let mut value = serde_json::json!({"version":"2.0.0","notes":"é".repeat(4096),"platforms":{TARGET:{"url":policy::artifact(&next),"signature":"synthetic"}}});
    assert!(parse_offer(
        &serde_json::to_vec(&value).unwrap(),
        &current,
        Channel::Stable,
        1
    )
    .is_ok());
    value["notes"] = serde_json::json!("é".repeat(4097));
    assert!(matches!(
        parse_offer(
            &serde_json::to_vec(&value).unwrap(),
            &current,
            Channel::Stable,
            1
        ),
        Err(Error::Manifest)
    ));
    value["notes"] = "".into();
    value["platforms"] = serde_json::json!({});
    assert!(matches!(
        parse_offer(
            &serde_json::to_vec(&value).unwrap(),
            &current,
            Channel::Stable,
            1
        ),
        Err(Error::Target)
    ));
}

fn ready_service() -> UpdateService {
    let service = service();
    offer(&service, UpdatePhase::Ready);
    service.state.lock().unwrap().updater = Some(super::super::tests::verified_owner());
    service
}

#[test]
fn post_shutdown_verification_failure_reports_install_failed_and_retires_offer() {
    let service = ready_service();
    service
        .state
        .lock()
        .unwrap()
        .updater
        .as_mut()
        .unwrap()
        .verified
        .as_mut()
        .unwrap()[4] ^= 1;
    let stopped = Arc::new(AtomicBool::new(false));
    let receipt = stopped.clone();
    service.configure_installer(Arc::new(move |native| {
        native.install_with(
            tempfile::tempdir().unwrap().path(),
            || {
                receipt.store(true, Ordering::Release);
                Ok(())
            },
            |_| panic!("changed bytes reached launch"),
        )
    }));
    service.install("7").unwrap();
    join(&service);
    assert!(stopped.load(Ordering::Acquire));
    assert_eq!(service.status().failure, Some(UpdateFailure::InstallFailed));
    assert!(service.status().offer.is_none());
}

#[test]
fn review_install_launch_and_wait_failure_retire_offer_for_a_fresh_check() {
    for fail_during_launch in [true, false] {
        let service = ready_service();
        service.configure_installer(Arc::new(move |native| {
            native.install_with(
                tempfile::tempdir().unwrap().path(),
                || Ok(()),
                |(directory, path)| {
                    assert!(path.is_file());
                    assert!(directory.path().is_dir());
                    if fail_during_launch {
                        Err(Error::Install)
                    } else {
                        Ok(())
                    }
                },
            )?;
            // Simulated child wait failure after a successful launch handoff.
            Err(Error::Install)
        }));
        service.install("7").unwrap();
        join(&service);
        assert_eq!(service.status().failure, Some(UpdateFailure::InstallFailed));
        assert!(service
            .state
            .lock()
            .unwrap()
            .updater
            .as_ref()
            .unwrap()
            .offer
            .is_none());
        assert!(
            service.status().offer.is_none(),
            "UI must offer a fresh check, not a stale download"
        );
    }
}

#[test]
fn review_successful_installer_exit_is_idle_until_the_actual_binary_restarts() {
    let service = ready_service();
    service.configure_installer(Arc::new(|native| {
        native.install_with(
            tempfile::tempdir().unwrap().path(),
            || Ok(()),
            |(_, path)| {
                assert!(path.is_file());
                Ok(())
            },
        )
    }));
    service.install("7").unwrap();
    join(&service);
    assert_eq!(service.status(), UpdateStatus::idle(UpdateChannel::Stable));
    assert_eq!(
        service
            .state
            .lock()
            .unwrap()
            .updater
            .as_ref()
            .unwrap()
            .current
            .to_string(),
        "1.0.0"
    );
}

#[test]
fn review_unresolved_coordinates_fail_as_unavailable_before_client_construction() {
    use super::super::{resolved_coordinate, validate_coordinate, REPOSITORY};
    for coordinate in [
        "",
        "/",
        "owner/",
        "/repo",
        "PLACEHOLDER/repo",
        "owner/unresolved",
        "owner/repo/extra",
        "owner/re?po",
        "owner/re po",
        "owner/répo",
        "https://github.com/owner/repo",
    ] {
        assert!(!resolved_coordinate(coordinate), "{coordinate}");
        assert_eq!(
            failed(validate_coordinate(coordinate).unwrap_err()),
            UpdateFailure::Unavailable,
            "{coordinate}"
        );
    }
    for coordinate in [REPOSITORY, "example-team/repo.name_1"] {
        assert!(resolved_coordinate(coordinate));
        assert!(validate_coordinate(coordinate).is_ok());
    }
}

#[derive(Default)]
struct GateLifecycle {
    stops: std::sync::atomic::AtomicUsize,
    unsaved: AtomicBool,
    on_stop: Mutex<Option<Box<dyn Fn() + Send>>>,
}
impl crate::setup::Lifecycle for GateLifecycle {
    fn running(&self) -> bool {
        false
    }
    fn live(&self) -> bool {
        false
    }
    fn stop(&self) -> Result<(), String> {
        self.stops.fetch_add(1, Ordering::AcqRel);
        if let Some(callback) = self.on_stop.lock().unwrap().as_ref() {
            callback();
        }
        if self.unsaved.load(Ordering::Acquire) {
            Err("Synthetic unsaved summaries".into())
        } else {
            Ok(())
        }
    }
    fn start(
        &self,
        _: Option<maxed_assets::CachedCatalog>,
        _: maxed_capture::CaptureHandle,
    ) -> Result<(), String> {
        Err("Capture is outside this test".into())
    }
    fn demo(&self, _: bool) -> Result<(), String> {
        Err("Demo is outside this test".into())
    }
}

fn setup_gate() -> (
    tempfile::TempDir,
    Arc<crate::setup::SetupService>,
    Arc<GateLifecycle>,
    Arc<AtomicBool>,
) {
    let root = tempfile::tempdir().unwrap();
    let lifecycle = Arc::new(GateLifecycle::default());
    let setup = Arc::new(
        crate::setup::SetupService::with_discovery_roots(
            root.path().into(),
            Arc::new(crate::setup::NativeCapture),
            lifecycle.clone(),
            Arc::new(crate::assets_bridge::AssetBridge::default()),
            vec![],
        )
        .unwrap(),
    );
    setup.initialize_updates().unwrap();
    offer(&setup.updates, UpdatePhase::Ready);
    setup.updates.state.lock().unwrap().updater = Some(super::super::tests::verified_owner());
    let launched = Arc::new(AtomicBool::new(false));
    let receipt = launched.clone();
    let weak = Arc::downgrade(&setup);
    setup.updates.configure_installer(Arc::new(move |native| {
        weak.upgrade()
            .unwrap()
            .install_update_with(native, |(_, path)| {
                assert!(path.is_file());
                receipt.store(true, Ordering::Release);
                Ok(())
            })
    }));
    (root, setup, lifecycle, launched)
}

#[test]
fn review_setup_rejects_stale_committed_channel_before_stop_or_launch() {
    let (_root, setup, lifecycle, launched) = setup_gate();
    // Simulate a committed on-disk channel different from the in-memory offer.
    setup
        .settings
        .put(&maxed_types::AppSettings {
            update_channel: UpdateChannel::Beta,
            ..Default::default()
        })
        .unwrap();
    setup.updates.install("7").unwrap();
    join(&setup.updates);
    assert_eq!(
        setup.updates.status().failure,
        Some(UpdateFailure::StaleOffer)
    );
    assert_eq!(lifecycle.stops.load(Ordering::Acquire), 0);
    assert!(!launched.load(Ordering::Acquire));
    assert!(setup
        .updates
        .state
        .lock()
        .unwrap()
        .updater
        .as_ref()
        .unwrap()
        .verified
        .is_some());
}

#[test]
fn review_setup_rejects_closing_before_stop_or_launch() {
    let (_root, setup, lifecycle, launched) = setup_gate();
    setup.cancel_pending(true);
    setup.updates.install("7").unwrap();
    join(&setup.updates);
    assert_eq!(
        setup.updates.status().failure,
        Some(UpdateFailure::ShutdownBlocked)
    );
    assert_eq!(lifecycle.stops.load(Ordering::Acquire), 0);
    assert!(!launched.load(Ordering::Acquire));
    assert!(setup.updates.status().offer.is_some());
    assert!(setup
        .updates
        .state
        .lock()
        .unwrap()
        .updater
        .as_ref()
        .unwrap()
        .offer
        .is_some());
}

#[test]
fn review_setup_unsaved_summaries_block_launch_and_keep_native_and_wire_offer() {
    let (_root, setup, lifecycle, launched) = setup_gate();
    lifecycle.unsaved.store(true, Ordering::Release);
    setup.updates.install("7").unwrap();
    join(&setup.updates);
    assert_eq!(
        setup.updates.status().failure,
        Some(UpdateFailure::ShutdownBlocked)
    );
    assert_eq!(lifecycle.stops.load(Ordering::Acquire), 1);
    assert!(!launched.load(Ordering::Acquire));
    assert!(setup.updates.status().offer.is_some());
    let state = setup.updates.state.lock().unwrap();
    let native = state.updater.as_ref().unwrap();
    assert!(native.offer.is_some());
    assert!(
        native.verified.is_some(),
        "a blocked shutdown retains verified bytes for explicit Retry"
    );
}

#[test]
fn review_setup_install_lease_freezes_cancel_and_channel_immediately_before_launch() {
    let (_root, setup, lifecycle, launched) = setup_gate();
    let weak = Arc::downgrade(&setup);
    *lifecycle.on_stop.lock().unwrap() = Some(Box::new(move || {
        let setup = weak.upgrade().unwrap();
        assert_eq!(setup.updates.cancel().failure, Some(UpdateFailure::Busy));
        assert!(setup
            .updates
            .commit_channel::<()>(UpdateChannel::Beta, || {
                panic!("channel persistence must not run during installation")
            })
            .is_err());
        assert_eq!(
            setup.settings.get().unwrap().update_channel,
            UpdateChannel::Stable
        );
        assert_eq!(setup.updates.status().phase, UpdatePhase::Installing);
    }));
    setup.updates.install("7").unwrap();
    join(&setup.updates);
    assert!(
        launched.load(Ordering::Acquire),
        "positive control must reach launch after durable stop"
    );
    assert_eq!(lifecycle.stops.load(Ordering::Acquire), 1);
}

struct RecordingSyncCount(Arc<std::sync::atomic::AtomicUsize>);
impl maxed_recording::RecordingDisk for RecordingSyncCount {
    fn sync(&self, file: &std::fs::File) -> std::io::Result<()> {
        self.0.fetch_add(1, Ordering::AcqRel);
        file.sync_all()
    }
}

#[test]
fn stop_update_exit_finish_recording_once() {
    for unsaved in [false, true] {
        let (root, setup, lifecycle, launched) = setup_gate();
        let recording = Arc::new(crate::recording::RecordingRuntime::default());
        let syncs = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        recording
            .configure_with_disk(
                root.path().join("recordings"),
                true,
                1000,
                true,
                Arc::new(RecordingSyncCount(syncs.clone())),
            )
            .unwrap();
        let frame = maxed_types::RawFrame {
            index: 0,
            at: maxed_types::Timestamp(1),
            original_len: 1,
            bytes: vec![42],
        };
        assert_eq!(
            recording.try_record(&frame),
            crate::recording::RecordAdmission::Accepted
        );
        let retire = recording.clone();
        let synced = syncs.clone();
        *lifecycle.on_stop.lock().unwrap() = Some(Box::new(move || {
            retire.retire();
            assert_eq!(synced.load(Ordering::Acquire), 1);
        }));
        lifecycle.unsaved.store(unsaved, Ordering::Release);
        setup.updates.install("7").unwrap();
        join(&setup.updates);
        assert_eq!(launched.load(Ordering::Acquire), !unsaved);
        assert_eq!(syncs.load(Ordering::Acquire), 1);
        setup.cancel_pending(true);
        setup.resume_after_cancelled_exit();
        let _ = setup.stop();
        let _ = setup.stop();
        assert_eq!(syncs.load(Ordering::Acquire), 1);
        let paths: Vec<_> = std::fs::read_dir(root.path().join("recordings"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "pcap"))
            .collect();
        assert_eq!(paths.len(), 1);
        let mut reader =
            maxed_recording::PcapReader::new(std::fs::File::open(&paths[0]).unwrap()).unwrap();
        assert_eq!(reader.next_frame().unwrap().unwrap().bytes, frame.bytes);
        assert!(reader.next_frame().unwrap().is_none());
    }
}

#[test]
fn blocked_shutdown_retries_verified_offer_and_cancel_retires_it() {
    let (_root, setup, lifecycle, launched) = setup_gate();
    lifecycle.unsaved.store(true, Ordering::Release);
    setup.updates.install("7").unwrap();
    join(&setup.updates);
    assert_eq!(
        setup.updates.status().failure,
        Some(UpdateFailure::ShutdownBlocked)
    );
    assert!(!launched.load(Ordering::Acquire));
    lifecycle.unsaved.store(false, Ordering::Release);
    assert_eq!(
        setup.updates.install("7").unwrap().phase,
        UpdatePhase::Installing
    );
    join(&setup.updates);
    assert!(launched.load(Ordering::Acquire));
    assert_eq!(setup.updates.status().phase, UpdatePhase::Idle);

    let (_root, setup, lifecycle, _) = setup_gate();
    lifecycle.unsaved.store(true, Ordering::Release);
    setup.updates.install("7").unwrap();
    join(&setup.updates);
    assert_eq!(
        setup.updates.cancel(),
        UpdateStatus::idle(UpdateChannel::Stable)
    );
    assert!(setup
        .updates
        .state
        .lock()
        .unwrap()
        .updater
        .as_ref()
        .unwrap()
        .verified
        .is_none());
    assert_eq!(
        setup.updates.install("7").unwrap().failure,
        Some(UpdateFailure::StaleOffer)
    );
}

#[test]
fn retry_check_allocates_a_new_generation() {
    let service = service();
    service.check();
    join(&service);
    let first = service.state.lock().unwrap().generation;
    service.check();
    join(&service);
    assert_eq!(service.state.lock().unwrap().generation, first + 1);
}
