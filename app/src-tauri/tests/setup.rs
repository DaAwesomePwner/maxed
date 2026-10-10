use maxed_app::{
    assets_bridge::{AssetBridge, AssetLoader},
    setup::*,
};
use maxed_assets::{AssetError, AssetPaths, AssetProgress, CachedCatalog};
use maxed_capture::{
    backend::{start_with_backend, Backend, Device},
    CaptureConfig, CaptureError, CaptureHandle,
};
use maxed_types::{AppSettings, CaptureAccess, NpcapCheck, RawFrame};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
mod fixture {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/maxed-assets/tests/common/mod.rs"
    ));
}

#[derive(Default)]
struct FakeCapture {
    probe_error: Mutex<Option<CaptureError>>,
    enumeration_error: Mutex<Option<CaptureError>>,
    empty: AtomicBool,
    open_error: Mutex<Option<CaptureError>>,
    rejected: Mutex<Option<String>>,
    opened: Mutex<Vec<Option<String>>>,
}
struct Quiet;
impl Device for Quiet {
    fn poll(&mut self) -> Result<Option<RawFrame>, CaptureError> {
        Ok(None)
    }
}
struct Opening(Option<CaptureError>);
impl Backend for Opening {
    fn discover(&mut self) -> Result<Vec<maxed_capture::AdapterInfo>, CaptureError> {
        Ok(vec![maxed_capture::AdapterInfo {
            id: "selected".into(),
            description: "Synthetic".into(),
        }])
    }
    fn open(&mut self, _: &maxed_capture::AdapterInfo) -> Result<Box<dyn Device>, CaptureError> {
        match self.0 {
            Some(error) => Err(error),
            None => Ok(Box::new(Quiet)),
        }
    }
}
impl CaptureBackend for FakeCapture {
    fn probe(&self) -> Result<(), CaptureError> {
        self.probe_error.lock().unwrap().map_or(Ok(()), Err)
    }
    fn adapters(&self) -> Result<Vec<maxed_capture::AdapterInfo>, CaptureError> {
        if let Some(error) = *self.enumeration_error.lock().unwrap() {
            return Err(error);
        }
        Ok(if self.empty.load(Ordering::Acquire) {
            vec![]
        } else {
            ["one", "two"]
                .map(|id| maxed_capture::AdapterInfo {
                    id: id.into(),
                    description: "Synthetic".into(),
                })
                .to_vec()
        })
    }
    fn start(&self, config: CaptureConfig) -> Result<CaptureHandle, CaptureError> {
        let rejected = self.rejected.lock().unwrap().clone();
        let error = if rejected.is_some() && config.adapter_id == rejected {
            Some(CaptureError::PermissionDenied)
        } else {
            *self.open_error.lock().unwrap()
        };
        self.opened.lock().unwrap().push(config.adapter_id);
        start_with_backend(CaptureConfig::default(), move || Ok(Opening(error)))
    }
}
#[derive(Default)]
struct FakeLifecycle {
    running: AtomicBool,
    live: AtomicBool,
    log: Mutex<Vec<&'static str>>,
    fail_next: AtomicBool,
    cached_starts: Mutex<Vec<Option<String>>>,
}
impl Lifecycle for FakeLifecycle {
    fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
    fn live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
    fn stop(&self) -> Result<(), String> {
        self.log.lock().unwrap().push("stop/finalize");
        self.running.store(false, Ordering::Release);
        self.live.store(false, Ordering::Release);
        Ok(())
    }
    fn start(&self, cache: Option<CachedCatalog>, handle: CaptureHandle) -> Result<(), String> {
        self.cached_starts
            .lock()
            .unwrap()
            .push(cache.map(|cache| cache.catalog.client_fingerprint.clone()));
        assert_eq!(handle.readiness(), Some(Ok(())));
        assert!(
            !self.running.swap(true, Ordering::AcqRel),
            "concurrent engine"
        );
        self.log.lock().unwrap().push("start");
        handle.stop().unwrap();
        if self.fail_next.swap(false, Ordering::AcqRel) {
            return Err("Synthetic startup failure".into());
        }
        Ok(())
    }
    fn demo(&self, _: bool) -> Result<(), String> {
        self.log.lock().unwrap().push("demo");
        self.live.store(true, Ordering::Release);
        Ok(())
    }
}
fn harness() -> (
    tempfile::TempDir,
    Arc<FakeCapture>,
    Arc<FakeLifecycle>,
    Arc<SetupService>,
) {
    let dir = tempfile::tempdir().unwrap();
    let capture = Arc::new(FakeCapture::default());
    let lifecycle = Arc::new(FakeLifecycle::default());
    let service = Arc::new(
        SetupService::with_discovery_roots(
            dir.path().into(),
            capture.clone(),
            lifecycle.clone(),
            Arc::new(AssetBridge::default()),
            vec![],
        )
        .unwrap(),
    );
    (dir, capture, lifecycle, service)
}

#[test]
fn committed_settings_drive_update_channel_and_failed_save_keeps_it() {
    let (dir, _, _, service) = harness();
    service.initialize_updates().unwrap();
    let desired = AppSettings {
        update_channel: maxed_types::UpdateChannel::Beta,
        recording_limit_bytes: 65_575,
        ..Default::default()
    };
    service.update(desired.clone()).unwrap();
    assert_eq!(
        service.updates.status().channel,
        maxed_types::UpdateChannel::Beta
    );
    assert_eq!(service.recording_status().unwrap().limit_bytes, 65_575);
    // An actual blocked staging destination must preserve the committed channel.
    fs::create_dir(dir.path().join("settings.pending")).unwrap();
    assert!(service
        .update(AppSettings {
            update_channel: maxed_types::UpdateChannel::Stable,
            ..desired.clone()
        })
        .is_err());
    assert_eq!(service.settings.get().unwrap(), desired);
    assert_eq!(
        service.updates.status().channel,
        maxed_types::UpdateChannel::Beta
    );
    assert_eq!(
        service.updates.status().phase,
        maxed_types::UpdatePhase::Idle
    );
}

#[test]
fn stable_default_and_beta_opt_in() {
    let (dir, capture, lifecycle, service) = harness();
    service.initialize_updates().unwrap();
    assert_eq!(
        service.updates.status(),
        maxed_types::UpdateStatus::idle(maxed_types::UpdateChannel::Stable)
    );
    assert_eq!(
        service.settings.get().unwrap().update_channel,
        maxed_types::UpdateChannel::Stable
    );
    service
        .settings
        .put(&AppSettings {
            update_channel: maxed_types::UpdateChannel::Beta,
            ..Default::default()
        })
        .unwrap();
    let reopened = SetupService::with_discovery_roots(
        dir.path().into(),
        capture,
        lifecycle,
        Arc::new(AssetBridge::default()),
        vec![],
    )
    .unwrap();
    reopened.initialize_updates().unwrap();
    assert_eq!(
        reopened.updates.status(),
        maxed_types::UpdateStatus::idle(maxed_types::UpdateChannel::Beta)
    );
}

#[test]
fn auto_explicit_removed_empty_and_enumeration_failure_keep_selection_honest() {
    let (_dir, capture, _, service) = harness();
    service.set_adapter(None).unwrap();
    service.set_adapter(Some("one".into())).unwrap();
    assert_eq!(
        service.settings.get().unwrap().adapter_id.as_deref(),
        Some("one")
    );
    assert!(service.set_adapter(Some("removed".into())).is_err());
    capture.empty.store(true, Ordering::Release);
    assert!(service.set_adapter(None).is_err());
    assert!(service.adapters().unwrap().is_empty());
    capture.empty.store(false, Ordering::Release);
    *capture.enumeration_error.lock().unwrap() = Some(CaptureError::OpenFailed);
    assert!(service.adapters().is_err());
    assert!(service.set_adapter(None).is_err());
    assert_eq!(
        service.settings.get().unwrap().adapter_id.as_deref(),
        Some("one")
    );
}

#[test]
fn missing_incompatible_denied_and_generic_failures_are_distinct_and_safe() {
    for (error, npcap, access) in [
        (
            CaptureError::Missing,
            NpcapCheck::Missing,
            CaptureAccess::Unknown,
        ),
        (
            CaptureError::Incompatible,
            NpcapCheck::Incompatible,
            CaptureAccess::Unknown,
        ),
        (
            CaptureError::PermissionDenied,
            NpcapCheck::PermissionDenied,
            CaptureAccess::Denied,
        ),
        (
            CaptureError::OpenFailed,
            NpcapCheck::Failed,
            CaptureAccess::Unknown,
        ),
    ] {
        let (_dir, capture, _, service) = harness();
        if matches!(error, CaptureError::Missing | CaptureError::Incompatible) {
            *capture.probe_error.lock().unwrap() = Some(error);
        } else {
            *capture.open_error.lock().unwrap() = Some(error);
        }
        assert!(service.set_adapter(None).is_err());
        assert_eq!(service.status().npcap, npcap);
        assert_eq!(service.status().access, access);
        assert!(!service.status().live_confirmed);
        assert_eq!(service.settings.get().unwrap(), AppSettings::default());
    }
}

struct PendingOpen {
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
}
impl CaptureBackend for PendingOpen {
    fn probe(&self) -> Result<(), CaptureError> {
        Ok(())
    }
    fn adapters(&self) -> Result<Vec<maxed_capture::AdapterInfo>, CaptureError> {
        FakeCapture::default().adapters()
    }
    fn start(&self, _: CaptureConfig) -> Result<CaptureHandle, CaptureError> {
        let entered = self.entered.clone();
        let release = self.release.clone();
        start_with_backend(CaptureConfig::default(), move || {
            entered.send(()).unwrap();
            release.recv().unwrap();
            Ok(Opening(None))
        })
    }
}
#[test]
fn compatible_dll_does_not_imply_access_until_open_completes() {
    let dir = tempfile::tempdir().unwrap();
    let (entered, wait) = crossbeam_channel::bounded(1);
    let (release, gate) = crossbeam_channel::bounded(1);
    let service = Arc::new(
        SetupService::new(
            dir.path().into(),
            Arc::new(PendingOpen {
                entered,
                release: gate,
            }),
            Arc::new(FakeLifecycle::default()),
            Arc::new(AssetBridge::default()),
        )
        .unwrap(),
    );
    let worker_service = service.clone();
    let worker = std::thread::spawn(move || worker_service.set_adapter(None));
    wait.recv().unwrap();
    assert_eq!(service.status().npcap, NpcapCheck::Compatible);
    assert_eq!(service.status().access, CaptureAccess::Unknown);
    assert!(!service.query().live_confirmed); // polling stays responsive while open is pending
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert_eq!(service.status().access, CaptureAccess::Allowed);
    assert!(!service.status().live_confirmed);
}

#[test]
fn running_adapter_changes_finalize_restart_and_roll_back_both_open_and_save_failure() {
    let (dir, capture, lifecycle, service) = harness();
    service.set_adapter(Some("one".into())).unwrap();
    service.start().unwrap();
    lifecycle.log.lock().unwrap().clear();
    service.set_adapter(Some("two".into())).unwrap();
    assert_eq!(*lifecycle.log.lock().unwrap(), ["stop/finalize", "start"]);
    *capture.rejected.lock().unwrap() = Some("one".into());
    lifecycle.log.lock().unwrap().clear();
    assert!(service.set_adapter(Some("one".into())).is_err());
    assert_eq!(
        *lifecycle.log.lock().unwrap(),
        ["stop/finalize", "stop/finalize", "start"]
    );
    assert_eq!(
        service.settings.get().unwrap().adapter_id.as_deref(),
        Some("two")
    );
    *capture.rejected.lock().unwrap() = None;
    fs::create_dir(dir.path().join("settings.pending")).unwrap();
    lifecycle.log.lock().unwrap().clear();
    assert!(service.set_adapter(Some("one".into())).is_err());
    assert_eq!(
        *lifecycle.log.lock().unwrap(),
        ["stop/finalize", "start", "stop/finalize", "start"]
    );
    assert_eq!(
        service.settings.get().unwrap().adapter_id.as_deref(),
        Some("two")
    );
    assert!(lifecycle.running());
}

#[test]
fn failed_engine_start_restores_prior_capture_and_selection() {
    let (_dir, _, lifecycle, service) = harness();
    service.set_adapter(Some("one".into())).unwrap();
    service.start().unwrap();
    lifecycle.fail_next.store(true, Ordering::Release);
    assert!(service.set_adapter(Some("two".into())).is_err());
    assert_eq!(
        service.settings.get().unwrap().adapter_id.as_deref(),
        Some("one")
    );
    assert!(lifecycle.running());
}

#[test]
fn whole_settings_update_cannot_bypass_adapter_path_or_recording_validation() {
    let (_dir, _, _, service) = harness();
    for next in [
        AppSettings {
            adapter_id: Some("removed".into()),
            ..Default::default()
        },
        AppSettings {
            assets_path: Some("C:/missing/resources.assets".into()),
            ..Default::default()
        },
        AppSettings {
            save_recordings: true,
            ..Default::default()
        },
    ] {
        assert!(service.update(next).is_err());
        assert_eq!(service.settings.get().unwrap(), AppSettings::default());
    }
}

#[test]
fn concurrent_adapter_and_demo_transitions_never_overlap_engines() {
    let (_dir, _, lifecycle, service) = harness();
    service.start().unwrap();
    let workers: Vec<_> = (0..8)
        .map(|i| {
            let service = service.clone();
            std::thread::spawn(move || {
                if i % 2 == 0 {
                    if let Err(error) = service.set_adapter(Some("one".into())) {
                        // Demo either cancels an opening adapter or wins the
                        // transition lock and prevents a physical adapter change.
                        assert!(matches!(
                            error.as_str(),
                            "Capture setup was cancelled."
                                | "Leave demo before changing the live capture adapter."
                        ));
                    }
                } else {
                    service.set_demo(true).unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    service.set_demo(true).unwrap();
    assert!(lifecycle.live());
    assert!(!service.status().live_confirmed);
    service.set_demo(false).unwrap();
    service.stop().unwrap();
    assert!(!service.status().live_confirmed);
}

fn installation(root: &std::path::Path, name: &str) -> PathBuf {
    let resources = root
        .join("steamapps/common")
        .join(name)
        .join("RotMG Exalt_Data/resources.assets");
    fs::create_dir_all(resources.parent().unwrap()).unwrap();
    fs::write(&resources, fixture::fixture()).unwrap();
    fs::write(
        root.join("steamapps/appmanifest_200210.acf"),
        format!("\"AppState\" {{ \"appid\" \"200210\" \"installdir\" \"{name}\" }}"),
    )
    .unwrap();
    resources
}
#[test]
fn launcher_discovery_is_bounded_and_requires_selection_with_steam() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("Local");
    let launcher = local.join("RealmOfTheMadGod/Production/RotMG Exalt_Data/resources.assets");
    assert_eq!(
        discover_with_launcher(None, &[], Some(&local)),
        Discovery::Missing
    );
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    fs::write(&launcher, fixture::fixture()).unwrap();
    let expected = maxed_app::assets_bridge::resources_path(&launcher).unwrap();
    assert_eq!(
        discover_with_launcher(None, &[], Some(&local)),
        Discovery::Found(expected.clone())
    );
    let steam = dir.path().join("Steam");
    installation(&steam, "Realm");
    assert_eq!(
        discover_with_launcher(None, std::slice::from_ref(&steam), Some(&local)),
        Discovery::Multiple
    );
    assert_eq!(
        discover_with_launcher(Some(&launcher), &[steam], Some(&local)),
        Discovery::Found(expected)
    );
}

#[test]
fn startup_routes_on_saved_metadata_without_loading_or_probing() {
    let (dir, capture, _, first) = harness();
    let assets = dir.path().join("resources.assets");
    fs::write(&assets, fixture::fixture()).unwrap();
    assert!(!first.catalog_available());
    first.set_assets(assets.to_string_lossy().into()).unwrap();
    assert!(first.catalog_available());
    // Auto-discovery/selection cannot erase the fact that this was a first run.
    assert!(first.startup_setup_required());
    let reopen = || {
        Arc::new(
            SetupService::with_discovery_roots(
                dir.path().into(),
                capture.clone(),
                Arc::new(FakeLifecycle::default()),
                Arc::new(AssetBridge::default()),
                vec![],
            )
            .unwrap(),
        )
    };
    let healthy = reopen();
    assert!(!healthy.startup_setup_required());
    assert!(
        !healthy.catalog_available(),
        "routing does not load the catalog"
    );
    assert_eq!(healthy.status().npcap, NpcapCheck::Unchecked);
    assert!(capture.opened.lock().unwrap().is_empty());
    healthy.query();
    healthy.join_pending(); // Normal background status work still prepares assets.
    assert!(healthy.catalog_available());
    let unsupported = dir.path().join("unsupported/resources.assets");
    fs::create_dir_all(unsupported.parent().unwrap()).unwrap();
    fs::write(&unsupported, b"invalid").unwrap();
    assert!(healthy
        .set_assets(unsupported.to_string_lossy().into())
        .is_err());
    assert!(
        healthy.catalog_available(),
        "replacement failure retains the previous catalog"
    );
    *capture.probe_error.lock().unwrap() = Some(CaptureError::Incompatible);
    assert!(
        !reopen().startup_setup_required(),
        "Npcap is checked in the background, not for routing"
    );
    *capture.probe_error.lock().unwrap() = None;
    fs::remove_file(&assets).unwrap();
    let missing = reopen();
    assert!(missing.startup_setup_required());
    assert!(!missing.catalog_available());
    missing.set_demo(true).unwrap();
    assert!(missing.catalog_available());
    assert!(!missing.startup_setup_required());
}

#[test]
fn first_run_routes_without_probe_or_extraction_even_with_discoverable_assets() {
    let dir = tempfile::tempdir().unwrap();
    let steam = dir.path().join("Steam");
    installation(&steam, "Realm");
    let loads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let capture = Arc::new(FakeCapture::default());
    let service = Arc::new(
        SetupService::with_discovery_roots(
            dir.path().join("app"),
            capture.clone(),
            Arc::new(FakeLifecycle::default()),
            Arc::new(AssetBridge::new(Arc::new(CountedAssets(loads.clone())))),
            vec![steam],
        )
        .unwrap(),
    );
    assert!(service.startup_setup_required());
    assert_eq!(loads.load(Ordering::Acquire), 0);
    assert_eq!(service.status().npcap, NpcapCheck::Unchecked);
    assert!(capture.opened.lock().unwrap().is_empty());
    assert!(service.settings.get().unwrap().assets_path.is_none());
    service.query();
    service.join_pending();
    assert_eq!(
        loads.load(Ordering::Acquire),
        1,
        "positive control: the background query loads the candidate"
    );
    assert!(!capture.opened.lock().unwrap().is_empty());
}

#[test]
fn startup_receipt_requires_unchanged_saved_path_and_cache_metadata() {
    let (dir, _, _, service) = harness();
    let assets = dir.path().join("resources.assets");
    fs::write(&assets, fixture::fixture()).unwrap();
    service.set_assets(assets.to_string_lossy().into()).unwrap();
    let reopen = || {
        SetupService::with_discovery_roots(
            dir.path().into(),
            Arc::new(FakeCapture::default()),
            Arc::new(FakeLifecycle::default()),
            Arc::new(AssetBridge::default()),
            vec![],
        )
        .unwrap()
    };
    assert!(!reopen().startup_setup_required());
    fs::write(&assets, b"changed input").unwrap();
    assert!(reopen().startup_setup_required());
    fs::write(&assets, fixture::fixture()).unwrap();
    service.set_assets(assets.to_string_lossy().into()).unwrap();
    assert!(!reopen().startup_setup_required());
    let cache = service.assets.cache().unwrap();
    fs::remove_file(cache.root.join("manifest.json")).unwrap();
    assert!(reopen().startup_setup_required());
    fs::write(dir.path().join("startup-assets.json"), b"invalid receipt").unwrap();
    assert!(reopen().startup_setup_required());
}

#[test]
fn discovery_saved_first_then_bounded_manifest_candidates_and_ambiguity() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Steam");
    let first = installation(&root, "Realm");
    assert!(matches!(
        discover(None, std::slice::from_ref(&root)),
        Discovery::Found(_)
    ));
    let other = dir.path().join("Library");
    let saved = installation(&other, "Other");
    let escaped = other.to_string_lossy().replace('\\', "\\\\");
    fs::write(
        root.join("steamapps/libraryfolders.vdf"),
        format!("\"libraryfolders\" {{ \"1\" {{ \"path\" \"{escaped}\" }} }}"),
    )
    .unwrap();
    assert_eq!(
        discover(None, std::slice::from_ref(&root)),
        Discovery::Multiple
    );
    let expected = maxed_app::assets_bridge::resources_path(&saved).unwrap();
    assert_eq!(
        discover(Some(&saved), std::slice::from_ref(&root)),
        Discovery::Found(expected)
    );
    fs::remove_file(saved).unwrap();
    fs::remove_file(first).unwrap();
    assert_eq!(discover(None, &[root]), Discovery::Missing);
}

#[test]
fn discovery_does_not_follow_unmanifested_installations_or_manifest_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Steam");
    let path = installation(&root, "Realm");
    let manifest = root.join("steamapps/appmanifest_200210.acf");
    fs::remove_file(&manifest).unwrap();
    assert_eq!(
        discover(None, std::slice::from_ref(&root)),
        Discovery::Missing
    );
    fs::write(
        &manifest,
        "\"appid\" \"200210\" \"installdir\" \"../Realm\"",
    )
    .unwrap();
    assert_eq!(
        discover(None, std::slice::from_ref(&root)),
        Discovery::Missing
    );
    assert!(path.is_file());
}

#[test]
fn assets_ready_without_npcap_is_not_live_and_running_replacement_has_a_boundary() {
    let (dir, capture, lifecycle, service) = harness();
    let path = dir.path().join("resources.assets");
    fs::write(&path, fixture::fixture()).unwrap();
    *capture.probe_error.lock().unwrap() = Some(CaptureError::Missing);
    service.set_assets(path.to_string_lossy().into()).unwrap();
    assert_eq!(service.status().assets, maxed_types::AssetCheck::Ready);
    assert!(!service.status().live_confirmed);
    *capture.probe_error.lock().unwrap() = None;
    service.start().unwrap();
    lifecycle.log.lock().unwrap().clear();
    service.set_assets(path.to_string_lossy().into()).unwrap();
    assert!(lifecycle.log.lock().unwrap().is_empty());
    // Only a genuinely changed catalog creates the stop/finalize boundary.
    let mut changed = fixture::fixture();
    let at = changed
        .windows(10)
        .position(|v| v == b"<Objects/>")
        .unwrap();
    changed[at..at + 10].copy_from_slice(b"<Grounds/>");
    fs::write(&path, changed).unwrap();
    service.set_assets(path.to_string_lossy().into()).unwrap();
    assert_eq!(*lifecycle.log.lock().unwrap(), ["stop/finalize", "start"]);
    let persisted = service.settings.get().unwrap();
    assert!(service
        .set_assets(
            dir.path()
                .join("missing/resources.assets")
                .to_string_lossy()
                .into()
        )
        .is_err());
    assert_eq!(service.settings.get().unwrap(), persisted);
    assert!(service.assets.cache().is_some());
}

struct Slow {
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
}
impl AssetLoader for Slow {
    fn load(
        &self,
        _: &AssetPaths,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(AssetProgress),
    ) -> Result<CachedCatalog, AssetError> {
        self.entered.send(()).unwrap();
        self.release.recv().unwrap();
        progress(AssetProgress::Publishing);
        assert!(cancel.load(Ordering::Acquire));
        Err(AssetError::Cancelled)
    }
}
#[test]
fn stop_cancels_extraction_before_join_and_no_selection_is_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    fs::write(&path, b"fixture").unwrap();
    let (entered, wait) = crossbeam_channel::bounded(1);
    let (release, gate) = crossbeam_channel::bounded(1);
    let assets = Arc::new(AssetBridge::new(Arc::new(Slow {
        entered,
        release: gate,
    })));
    let service = Arc::new(
        SetupService::new(
            dir.path().into(),
            Arc::new(FakeCapture::default()),
            Arc::new(FakeLifecycle::default()),
            assets,
        )
        .unwrap(),
    );
    let worker_service = service.clone();
    let worker =
        std::thread::spawn(move || worker_service.set_assets(path.to_string_lossy().into()));
    wait.recv().unwrap();
    service.cancel_pending(false);
    release.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    service.stop().unwrap();
    assert!(service.settings.get().unwrap().assets_path.is_none());
    assert!(service.assets.cache().is_none());
}

#[test]
fn worker_scheduling_cannot_reverse_successive_asset_selections() {
    let (dir, _, _, service) = harness();
    let first_path = dir.path().join("first/resources.assets");
    let next_path = dir.path().join("next/resources.assets");
    for path in [&first_path, &next_path] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, fixture::fixture()).unwrap();
    }
    let first = service.reserve_assets().unwrap();
    let next = service.reserve_assets().unwrap();
    service
        .set_assets_reserved(next_path.to_string_lossy().into(), next)
        .unwrap();
    let persisted = service.settings.get().unwrap();
    assert!(service
        .set_assets_reserved(first_path.to_string_lossy().into(), first)
        .is_err());
    assert_eq!(service.settings.get().unwrap(), persisted);
    assert_eq!(service.status().assets, maxed_types::AssetCheck::Ready);
}

#[test]
fn unreadable_settings_do_not_leave_an_asset_action_permanently_pending() {
    let (dir, _, _, service) = harness();
    fs::write(
        dir.path().join("settings.json"),
        b"invalid private settings",
    )
    .unwrap();
    assert!(service
        .set_assets(dir.path().join("resources.assets").to_string_lossy().into())
        .is_err());
    assert_eq!(service.status().assets, maxed_types::AssetCheck::Missing);
    assert!(service.status().extraction_stage.is_none());
    assert_eq!(
        fs::read(dir.path().join("settings.json")).unwrap(),
        b"invalid private settings"
    );
}

#[test]
fn demo_visuals_use_synthetic_catalog_and_never_fall_through_to_live_assets() {
    let (dir, _, _, service) = harness();
    let path = dir.path().join("resources.assets");
    fs::write(&path, fixture::fixture()).unwrap();
    service.set_assets(path.to_string_lossy().into()).unwrap();
    let live = service.visuals(&[]).unwrap().client_fingerprint;
    service.set_demo(true).unwrap();
    let synthetic = service.visuals(&[]).unwrap().client_fingerprint;
    assert_ne!(live, synthetic);
    assert!(!service.status().live_confirmed);
    service.set_demo(false).unwrap();
    assert_eq!(service.visuals(&[]).unwrap().client_fingerprint, live);
}

struct CountedAssets(Arc<std::sync::atomic::AtomicUsize>);
impl AssetLoader for CountedAssets {
    fn load(
        &self,
        paths: &AssetPaths,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(AssetProgress),
    ) -> Result<CachedCatalog, AssetError> {
        self.0.fetch_add(1, Ordering::AcqRel);
        maxed_assets::load_or_extract(paths, cancel, &mut |stage| progress(stage))
    }
}
#[test]
fn unchanged_asset_path_settings_apply_preserves_run_cache_and_asset_status() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    fs::write(&path, fixture::fixture()).unwrap();
    let loads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let lifecycle = Arc::new(FakeLifecycle::default());
    let service = SetupService::with_discovery_roots(
        dir.path().into(),
        Arc::new(FakeCapture::default()),
        lifecycle.clone(),
        Arc::new(AssetBridge::new(Arc::new(CountedAssets(loads.clone())))),
        vec![],
    )
    .unwrap();
    service.set_assets(path.to_string_lossy().into()).unwrap();
    service.start().unwrap();
    lifecycle.log.lock().unwrap().clear();
    let before = service.assets.status();
    let mut changed = service.settings.get().unwrap();
    changed.update_channel = maxed_types::UpdateChannel::Beta;
    service.update(changed.clone()).unwrap();
    assert!(lifecycle.log.lock().unwrap().is_empty());
    assert_eq!(loads.load(Ordering::Acquire), 1);
    assert_eq!(service.assets.status(), before);
    assert_eq!(service.settings.get().unwrap(), changed);

    changed.adapter_id = Some("removed".into());
    assert!(service.update(changed).is_err());
    assert_eq!(service.assets.status(), before);
    assert_eq!(loads.load(Ordering::Acquire), 1);
    // Reserving an unrelated full settings update does not cancel a selection
    // that is already in flight.
    let pending = service.reserve_assets().unwrap();
    assert!(service
        .reserve_update(&service.settings.get().unwrap())
        .unwrap()
        .is_none());
    assert!(!pending.cancel.load(Ordering::Acquire));
    service.assets.cancel(false);
}

#[test]
fn different_path_with_identical_fingerprint_does_not_restart_capture() {
    let (dir, _, lifecycle, service) = harness();
    let first = dir.path().join("first/resources.assets");
    let second = dir.path().join("second/resources.assets");
    for path in [&first, &second] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, fixture::fixture()).unwrap();
    }
    service.set_assets(first.to_string_lossy().into()).unwrap();
    service.start().unwrap();
    lifecycle.log.lock().unwrap().clear();
    let before = service
        .assets
        .cache()
        .unwrap()
        .catalog
        .client_fingerprint
        .clone();
    service.set_assets(second.to_string_lossy().into()).unwrap();
    assert!(lifecycle.log.lock().unwrap().is_empty());
    assert_eq!(
        service.assets.cache().unwrap().catalog.client_fingerprint,
        before
    );
    assert_eq!(
        service.settings.get().unwrap().assets_path.as_deref(),
        maxed_app::assets_bridge::resources_path(&second)
            .unwrap()
            .to_str()
    );
}

#[test]
fn shell_start_prepares_saved_assets_without_opening_setup_first() {
    let (dir, _, lifecycle, service) = harness();
    let path = dir.path().join("resources.assets");
    fs::write(&path, fixture::fixture()).unwrap();
    service
        .settings
        .put(&AppSettings {
            assets_path: Some(path.to_string_lossy().into()),
            ..Default::default()
        })
        .unwrap();
    assert!(service.assets.cache().is_none());
    service.start().unwrap();
    let reopened = SetupService::with_discovery_roots(
        dir.path().into(),
        Arc::new(FakeCapture::default()),
        Arc::new(FakeLifecycle::default()),
        Arc::new(AssetBridge::default()),
        vec![],
    )
    .unwrap();
    assert!(
        !reopened.startup_setup_required(),
        "Start also records the validated generation"
    );
    let cache = service.assets.cache().unwrap();
    assert_eq!(
        *lifecycle.cached_starts.lock().unwrap(),
        [Some(cache.catalog.client_fingerprint.clone())]
    );
    assert_eq!(
        service.assets.status().assets,
        maxed_types::AssetCheck::Ready
    );
}

struct GatedAssets {
    entered: crossbeam_channel::Sender<()>,
    release: crossbeam_channel::Receiver<()>,
}

#[test]
fn shell_start_keeps_missing_or_unsupported_asset_diagnosis_and_uses_partial_mode() {
    for unsupported in [false, true] {
        let (dir, _, lifecycle, service) = harness();
        let path = dir.path().join("resources.assets");
        if unsupported {
            let mut bytes = fixture::fixture();
            bytes[8..12].copy_from_slice(&99u32.to_be_bytes());
            fs::write(&path, bytes).unwrap();
        }
        service
            .settings
            .put(&AppSettings {
                assets_path: Some(path.to_string_lossy().into()),
                ..Default::default()
            })
            .unwrap();
        service.start().unwrap();
        assert_eq!(*lifecycle.cached_starts.lock().unwrap(), [None]);
        let diagnosis = service.assets.status();
        assert_eq!(
            diagnosis.assets,
            if unsupported {
                maxed_types::AssetCheck::Unsupported
            } else {
                maxed_types::AssetCheck::Failed
            }
        );
        assert_eq!(diagnosis.message, maxed_app::assets_bridge::ASSET_ERROR);
        assert_eq!(service.query().assets, diagnosis.assets);
        service.join_pending();
        assert_eq!(service.assets.status(), diagnosis);
        service.stop().unwrap();
        assert_eq!(service.query().assets, diagnosis.assets);
        service.join_pending();
        assert_eq!(service.assets.status(), diagnosis);
    }
}

#[test]
fn shell_start_asset_cancellation_does_not_launch_partial_capture() {
    for closing in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resources.assets");
        fs::write(&path, fixture::fixture()).unwrap();
        let (entered, wait) = crossbeam_channel::bounded(1);
        let (release, gate) = crossbeam_channel::bounded(1);
        let lifecycle = Arc::new(FakeLifecycle::default());
        let service = Arc::new(
            SetupService::with_discovery_roots(
                dir.path().into(),
                Arc::new(FakeCapture::default()),
                lifecycle.clone(),
                Arc::new(AssetBridge::new(Arc::new(GatedAssets {
                    entered,
                    release: gate,
                }))),
                vec![],
            )
            .unwrap(),
        );
        service
            .settings
            .put(&AppSettings {
                assets_path: Some(path.to_string_lossy().into()),
                ..Default::default()
            })
            .unwrap();
        let starting = service.clone();
        let worker = std::thread::spawn(move || starting.start());
        wait.recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        service.cancel_pending(closing);
        release.send(()).unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            maxed_app::assets_bridge::CANCELLED
        );
        assert!(lifecycle.cached_starts.lock().unwrap().is_empty());
    }
}

#[test]
fn failed_discovery_skips_unchanged_files_and_defers_changed_files_until_capture_stops() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Steam");
    installation(&root, "Realm");
    let path = root.join("steamapps/common/Realm/RotMG Exalt_Data/resources.assets");
    let mut bytes = fixture::fixture();
    bytes[8..12].copy_from_slice(&99u32.to_be_bytes());
    fs::write(&path, &bytes).unwrap();
    let loads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let service = Arc::new(
        SetupService::with_discovery_roots(
            dir.path().join("app"),
            Arc::new(FakeCapture::default()),
            Arc::new(FakeLifecycle::default()),
            Arc::new(AssetBridge::new(Arc::new(CountedAssets(loads.clone())))),
            vec![root],
        )
        .unwrap(),
    );
    service.query();
    service.join_pending();
    let failure = service.assets.status();
    assert_eq!(failure.assets, maxed_types::AssetCheck::Unsupported);
    for _ in 0..3 {
        assert_eq!(service.query().assets, failure.assets);
        service.join_pending();
        assert_eq!(service.assets.status(), failure);
    }
    assert_eq!(loads.load(Ordering::Acquire), 1);
    service.start().unwrap();
    // A different size is a deterministic metadata change, independent of the
    // filesystem's timestamp precision. Even this must not trigger live hashing.
    bytes.push(0);
    fs::write(&path, bytes).unwrap();
    assert_eq!(service.query().assets, failure.assets);
    service.join_pending();
    assert_eq!(loads.load(Ordering::Acquire), 1);
    service.stop().unwrap();
    service.query();
    service.join_pending();
    assert_eq!(loads.load(Ordering::Acquire), 2);
    assert_eq!(
        service.assets.status().assets,
        maxed_types::AssetCheck::Unsupported
    );
    // A new explicit selection may retry the same unsuccessful candidate.
    assert!(service.set_assets(path.to_string_lossy().into()).is_err());
    assert_eq!(loads.load(Ordering::Acquire), 3);
}
impl AssetLoader for GatedAssets {
    fn load(
        &self,
        paths: &AssetPaths,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(AssetProgress),
    ) -> Result<CachedCatalog, AssetError> {
        progress(AssetProgress::Reading);
        self.entered.send(()).map_err(|_| AssetError::Cancelled)?;
        self.release.recv().map_err(|_| AssetError::Cancelled)?;
        maxed_assets::load_or_extract(paths, cancel, &mut |stage| progress(stage))
    }
}
#[test]
fn discovery_returns_progress_before_extraction_finishes_and_reuses_recent_probe() {
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    fs::write(&path, fixture::fixture()).unwrap();
    let (entered, wait) = crossbeam_channel::bounded(1);
    let (release, gate) = crossbeam_channel::bounded(1);
    let capture = Arc::new(FakeCapture::default());
    let service = Arc::new(
        SetupService::with_discovery_roots(
            dir.path().into(),
            capture.clone(),
            Arc::new(FakeLifecycle::default()),
            Arc::new(AssetBridge::new(Arc::new(GatedAssets {
                entered,
                release: gate,
            }))),
            vec![],
        )
        .unwrap(),
    );
    service
        .settings
        .put(&AppSettings {
            assets_path: Some(path.to_string_lossy().into()),
            ..Default::default()
        })
        .unwrap();
    let (answer, receive) = crossbeam_channel::bounded(1);
    let query_service = service.clone();
    let caller = std::thread::spawn(move || answer.send(query_service.query()).unwrap());
    assert_eq!(
        receive.recv_timeout(Duration::from_secs(2)).unwrap().assets,
        maxed_types::AssetCheck::Finding
    );
    caller.join().unwrap();
    wait.recv_timeout(Duration::from_secs(2)).unwrap();
    let progress = service.query();
    assert_eq!(progress.assets, maxed_types::AssetCheck::Extracting);
    assert_eq!(
        progress.extraction_stage,
        Some(maxed_types::ExtractionStage::Reading)
    );
    assert_eq!(capture.opened.lock().unwrap().len(), 1);
    release.send(()).unwrap();
    service.join_pending();
    assert_eq!(service.status().assets, maxed_types::AssetCheck::Ready);
    service.stop().unwrap();
    service.query();
    service.join_pending();
    assert_eq!(
        capture.opened.lock().unwrap().len(),
        1,
        "idle query must not open adapters again within ten seconds"
    );
    service.start().unwrap();
    assert_eq!(
        capture.opened.lock().unwrap().len(),
        2,
        "Start opens only its actual capture"
    );
}

#[test]
fn saved_failure_retries_only_on_explicit_retry_or_modified_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("resources.assets");
    let mut bytes = fixture::fixture();
    bytes[8..12].copy_from_slice(&99u32.to_be_bytes());
    fs::write(&path, bytes).unwrap();
    let loads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let lifecycle = Arc::new(FakeLifecycle::default());
    let service = Arc::new(
        SetupService::with_discovery_roots(
            dir.path().into(),
            Arc::new(FakeCapture::default()),
            lifecycle.clone(),
            Arc::new(AssetBridge::new(Arc::new(CountedAssets(loads.clone())))),
            vec![],
        )
        .unwrap(),
    );
    service
        .settings
        .put(&AppSettings {
            assets_path: Some(path.to_string_lossy().into()),
            ..Default::default()
        })
        .unwrap();
    service.start().unwrap();
    assert_eq!(loads.load(Ordering::Acquire), 1);
    service.stop().unwrap();
    service.start().unwrap();
    assert_eq!(
        loads.load(Ordering::Acquire),
        1,
        "Start retains the known failure"
    );
    service.query_with_retry(true);
    service.join_pending();
    assert_eq!(
        loads.load(Ordering::Acquire),
        2,
        "explicit Retry is allowed while live"
    );
    assert_eq!(
        service.status().assets,
        maxed_types::AssetCheck::Unsupported
    );
    assert!(lifecycle.running());
    service.query();
    service.join_pending();
    assert_eq!(loads.load(Ordering::Acquire), 2);
    service.stop().unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified + std::time::Duration::from_secs(5)))
        .unwrap();
    service.query();
    service.join_pending();
    assert_eq!(
        loads.load(Ordering::Acquire),
        3,
        "mtime changes retry without reading content first"
    );
}

#[test]
fn stop_and_demo_preserve_diagnoses_and_allow_missing_client_discovery_retry() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Steam");
    let service = Arc::new(
        SetupService::with_discovery_roots(
            dir.path().join("app"),
            Arc::new(FakeCapture::default()),
            Arc::new(FakeLifecycle::default()),
            Arc::new(AssetBridge::default()),
            vec![root.clone()],
        )
        .unwrap(),
    );
    service.query();
    service.join_pending();
    let missing = service.assets.status();
    assert_eq!(missing.assets, maxed_types::AssetCheck::Missing);
    service.stop().unwrap();
    service.set_demo(true).unwrap();
    service.set_demo(false).unwrap();
    assert_eq!(service.assets.status(), missing);
    installation(&root, "Realm");
    assert_eq!(service.query().assets, maxed_types::AssetCheck::Finding);
    service.join_pending();
    assert_eq!(service.status().assets, maxed_types::AssetCheck::Ready);

    let unsupported = dir.path().join("resources.assets");
    let mut bytes = fixture::fixture();
    bytes[8..12].copy_from_slice(&99u32.to_be_bytes());
    fs::write(&unsupported, bytes).unwrap();
    assert!(service
        .set_assets(unsupported.to_string_lossy().into())
        .is_err());
    let failure = service.assets.status();
    assert_eq!(failure.assets, maxed_types::AssetCheck::Unsupported);
    service.stop().unwrap();
    service.set_demo(true).unwrap();
    assert_eq!(service.assets.status(), failure);
}
