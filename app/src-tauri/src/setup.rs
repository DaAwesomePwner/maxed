//! Blocking-worker orchestration. The transition lock owns all engine changes.
use crate::{
    assets_bridge::{resources_path, AssetBridge, Operation},
    runtime::{CaptureRuntime, RuntimeOptions},
    settings::{validate, SettingsStore},
};
use maxed_assets::CachedCatalog;
use maxed_capture::{CaptureConfig, CaptureError, CaptureHandle};
use maxed_types::{AdapterInfo, AppSettings, CaptureAccess, CaptureState, NpcapCheck, SetupStatus};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime},
};

const CAPTURE_ERROR: &str =
    "Capture could not start. Check Npcap and the selected adapter, then retry.";
pub trait CaptureBackend: Send + Sync {
    fn probe(&self) -> Result<(), CaptureError>;
    fn adapters(&self) -> Result<Vec<maxed_capture::AdapterInfo>, CaptureError>;
    fn start(&self, config: CaptureConfig) -> Result<CaptureHandle, CaptureError>;
}
pub struct NativeCapture;
impl CaptureBackend for NativeCapture {
    fn probe(&self) -> Result<(), CaptureError> {
        maxed_capture::probe_npcap()
    }
    fn adapters(&self) -> Result<Vec<maxed_capture::AdapterInfo>, CaptureError> {
        maxed_capture::list_adapters()
    }
    fn start(&self, config: CaptureConfig) -> Result<CaptureHandle, CaptureError> {
        maxed_capture::start_capture(config)
    }
}
pub trait Lifecycle: Send + Sync {
    fn history_upgrade_status(&self) -> Result<maxed_types::HistoryUpgradeStatus, String> {
        Err("History upgrade status is unavailable.".into())
    }
    fn backup_and_upgrade_history(&self) -> Result<maxed_types::HistoryUpgradeStatus, String> {
        Err("History upgrade is unavailable.".into())
    }
    fn configure_recording(&self, enabled: bool, _limit: u32, _live: bool) -> Result<(), String> {
        if enabled {
            Err("Recording is unavailable.".into())
        } else {
            Ok(())
        }
    }
    fn recording_status(&self, limit_bytes: u32) -> maxed_types::RecordingStatus {
        maxed_types::RecordingStatus {
            state: maxed_types::RecordingState::Disabled,
            limit_bytes,
            retained_bytes: 0,
            dropped_frames: 0,
            failure: None,
        }
    }
    fn running(&self) -> bool;
    fn live(&self) -> bool;
    fn stop(&self) -> Result<(), String>;
    fn prepare_update(&self) -> Result<(), String> {
        self.stop()
    }
    fn finish_update(&self) {}
    fn start(&self, cache: Option<CachedCatalog>, handle: CaptureHandle) -> Result<(), String>;
    fn demo(&self, enabled: bool) -> Result<(), String>;
}
pub struct NativeLifecycle {
    pub runtime: Arc<CaptureRuntime>,
    pub app: tauri::AppHandle,
    pub app_data: PathBuf,
}
impl Lifecycle for NativeLifecycle {
    fn history_upgrade_status(&self) -> Result<maxed_types::HistoryUpgradeStatus, String> {
        self.runtime.history_upgrade_status()
    }
    fn backup_and_upgrade_history(&self) -> Result<maxed_types::HistoryUpgradeStatus, String> {
        self.runtime.backup_and_upgrade_history()
    }
    fn configure_recording(&self, enabled: bool, limit: u32, live: bool) -> Result<(), String> {
        self.runtime
            .configure_recording(self.app_data.clone(), enabled, limit, live)
    }
    fn recording_status(&self, limit_bytes: u32) -> maxed_types::RecordingStatus {
        self.runtime.recording_status(limit_bytes)
    }
    fn running(&self) -> bool {
        self.runtime.is_running()
    }
    fn live(&self) -> bool {
        self.runtime.status().state == CaptureState::Live
    }
    fn stop(&self) -> Result<(), String> {
        self.runtime.stop_and_flush()
    }
    fn prepare_update(&self) -> Result<(), String> {
        self.runtime.prepare_update()
    }
    fn finish_update(&self) {
        self.runtime.finish_update();
    }
    fn start(&self, cache: Option<CachedCatalog>, handle: CaptureHandle) -> Result<(), String> {
        let mut options = RuntimeOptions::live(self.app_data.clone(), None);
        options.prepared_catalog = cache;
        self.runtime
            .start_prepared(self.app.clone(), options, handle)
    }
    fn demo(&self, enabled: bool) -> Result<(), String> {
        if enabled {
            self.runtime.demo(self.app.clone())?;
            self.runtime.flush_bridge()
        } else {
            let snapshot = self.runtime.reset_preview()?;
            self.runtime.publish_preview(self.app.clone(), snapshot)
        }
    }
}

/// Metadata only: checking an unsuccessful candidate must never fingerprint it.
#[derive(PartialEq, Eq)]
struct FailedCandidate {
    path: PathBuf,
    metadata: Option<(u64, SystemTime)>,
}
impl FailedCandidate {
    fn read(path: &Path) -> Self {
        let metadata = resources_path(path)
            .ok()
            .and_then(|path| std::fs::metadata(path).ok())
            .and_then(|metadata| Some((metadata.len(), metadata.modified().ok()?)));
        Self {
            path: path.to_owned(),
            metadata,
        }
    }
}

pub struct SetupService {
    pub updates: Arc<crate::updater::service::UpdateService>,
    pub assets: Arc<AssetBridge>,
    pub settings: Arc<SettingsStore>,
    root: PathBuf,
    backend: Arc<dyn CaptureBackend>,
    lifecycle: Arc<dyn Lifecycle>,
    serial: Mutex<()>,
    probe: Mutex<(NpcapCheck, CaptureAccess)>,
    probe_checked: Mutex<Option<Instant>>,
    refresh_worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    discovery_roots: Vec<PathBuf>,
    launcher_root: Option<PathBuf>,
    settings_present_at_start: bool,
    gui_smoke: bool,
    cancel: AtomicBool,
    closing: AtomicBool,
    demo: AtomicBool,
    discovered: AtomicBool,
    failed_candidate: Mutex<Option<FailedCandidate>>,
}
impl SetupService {
    pub fn search_items(
        &self,
        query: &str,
        limit: u8,
    ) -> Result<Vec<maxed_types::ItemMatch>, String> {
        if self.demo.load(Ordering::Acquire) {
            crate::assets_bridge::search_items(&demo_catalog()?, query, limit)
        } else {
            self.assets.search_items(query, limit)
        }
    }

    pub fn catalog_query_data(&self) -> maxed_store::CatalogQueryData {
        if self.demo.load(Ordering::Acquire) {
            static DEMO: std::sync::LazyLock<maxed_store::CatalogQueryData> =
                std::sync::LazyLock::new(|| {
                    demo_catalog()
                        .map(|catalog| maxed_store::CatalogQueryData::from_catalog(&catalog))
                        .unwrap_or_default()
                });
            DEMO.clone()
        } else {
            self.assets.catalog_query_data()
        }
    }

    pub fn new(
        root: PathBuf,
        backend: Arc<dyn CaptureBackend>,
        lifecycle: Arc<dyn Lifecycle>,
        assets: Arc<AssetBridge>,
    ) -> Result<Self, String> {
        let roots = if crate::gui_smoke::enabled() {
            Vec::new()
        } else {
            steam_roots()
        };
        Self::with_discovery_roots(root, backend, lifecycle, assets, roots)
    }
    pub fn with_discovery_roots(
        root: PathBuf,
        backend: Arc<dyn CaptureBackend>,
        lifecycle: Arc<dyn Lifecycle>,
        assets: Arc<AssetBridge>,
        discovery_roots: Vec<PathBuf>,
    ) -> Result<Self, String> {
        Ok(Self {
            updates: Arc::new(crate::updater::service::UpdateService::new()),
            settings_present_at_start: root.join("settings.json").is_file(),
            gui_smoke: crate::gui_smoke::enabled(),
            settings: Arc::new(SettingsStore::new(root.clone())?),
            root,
            backend,
            lifecycle,
            assets,
            serial: Mutex::new(()),
            probe: Mutex::new((NpcapCheck::Unchecked, CaptureAccess::Unknown)),
            probe_checked: Mutex::new(None),
            refresh_worker: Mutex::new(None),
            discovery_roots,
            launcher_root: None,
            cancel: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            demo: AtomicBool::new(false),
            discovered: AtomicBool::new(false),
            failed_candidate: Mutex::new(None),
        })
    }
    /// LocalAppData comes from Tauri's known-folder resolver; tests inject a temp root.
    pub fn with_launcher_root(mut self, root: Option<PathBuf>) -> Self {
        if !self.gui_smoke {
            self.launcher_root = root;
        }
        self
    }
    pub fn catalog_available(&self) -> bool {
        self.demo.load(Ordering::Acquire) || self.assets.cache().is_some()
    }
    /// Routing hint only: never probe, discover, hash, extract or join a worker.
    pub fn startup_setup_required(&self) -> bool {
        if self.gui_smoke {
            return true;
        }
        // Explicit synthetic demo startup has its own catalog and never needs
        // a live installation or Npcap. Leaving demo still exposes normal Setup.
        if self.demo.load(Ordering::Acquire) {
            return false;
        }
        if !self.settings_present_at_start {
            return true;
        }
        !self
            .settings
            .get()
            .ok()
            .and_then(|settings| settings.assets_path)
            .is_some_and(|path| crate::startup_cache::recorded(&self.root, Path::new(&path)))
    }
    fn begin_transition(&self) -> Result<(), String> {
        if self.gui_smoke {
            return Err(CAPTURE_ERROR.into());
        }
        if self.updates.installing() {
            return Err("Update operation is busy.".into());
        }
        if self.closing.load(Ordering::Acquire) {
            return Err("Maxed is closing.".into());
        }
        self.cancel.store(false, Ordering::Release);
        Ok(())
    }
    pub fn adapters(&self) -> Result<Vec<AdapterInfo>, String> {
        if self.gui_smoke {
            return Err(CAPTURE_ERROR.into());
        }
        self.backend
            .adapters()
            .map(|rows| {
                rows.into_iter()
                    .map(|row| AdapterInfo {
                        id: row.id,
                        description: row.description,
                    })
                    .collect()
            })
            .map_err(|_| CAPTURE_ERROR.into())
    }
    fn opened(&self, id: Option<String>, cancel: &AtomicBool) -> Result<CaptureHandle, String> {
        if self.gui_smoke {
            return Err(CAPTURE_ERROR.into());
        }
        let result = (|| {
            self.backend.probe()?;
            *self.probe.lock().unwrap_or_else(|p| p.into_inner()) =
                (NpcapCheck::Compatible, CaptureAccess::Unknown);
            let adapters = self.backend.adapters()?;
            if adapters.is_empty()
                || id
                    .as_ref()
                    .is_some_and(|id| !adapters.iter().any(|a| &a.id == id))
            {
                return Err(CaptureError::NoAdapters);
            }
            let handle = self.backend.start(CaptureConfig {
                adapter_id: id,
                ..Default::default()
            })?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if cancel.load(Ordering::Acquire)
                    || self.cancel.load(Ordering::Acquire)
                    || self.closing.load(Ordering::Acquire)
                {
                    return Err(CaptureError::Stopped);
                }
                if let Some(result) = handle.readiness() {
                    result?;
                    return Ok(handle);
                }
                if Instant::now() >= deadline {
                    return Err(CaptureError::OpenFailed);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        })();
        *self.probe.lock().unwrap_or_else(|p| p.into_inner()) =
            probe_result(result.as_ref().map(|_| ()).map_err(|e| *e));
        *self.probe_checked.lock().unwrap_or_else(|p| p.into_inner()) =
            if matches!(&result, Err(CaptureError::Stopped)) {
                None
            } else {
                Some(Instant::now())
            };
        result.map_err(|error| {
            match error {
                CaptureError::Missing => "Npcap is missing. Install Npcap and retry.",
                CaptureError::Incompatible => {
                    "Npcap is incompatible. Repair or update Npcap and retry."
                }
                CaptureError::PermissionDenied => {
                    "This Windows user cannot capture. Repair Npcap access settings and retry."
                }
                CaptureError::NoAdapters => {
                    "No eligible adapter was found. Reconnect or select another adapter."
                }
                CaptureError::Stopped => "Capture setup was cancelled.",
                _ => CAPTURE_ERROR,
            }
            .into()
        })
    }
    pub fn status(&self) -> SetupStatus {
        let mut status = self.assets.status();
        let (npcap, access) = self.probe.lock().unwrap_or_else(|p| p.into_inner()).clone();
        status.npcap = npcap;
        status.access = access;
        status.demo = self.demo.load(Ordering::Acquire);
        status.confirm_live(self.lifecycle.live());
        status
    }
    pub fn visuals(&self, object_types: &[u16]) -> Result<maxed_types::VisualPage, String> {
        if self.demo.load(Ordering::Acquire) {
            crate::assets_bridge::visuals(&demo_catalog()?, object_types)
        } else {
            self.assets.visuals(object_types)
        }
    }
    pub fn item_details(
        &self,
        object_types: &[u16],
        enchant_ids: &[i32],
    ) -> Result<maxed_types::ItemDetailPage, String> {
        if self.demo.load(Ordering::Acquire) {
            crate::assets_bridge::item_details(&demo_catalog()?, object_types, enchant_ids)
        } else {
            self.assets.item_details(object_types, enchant_ids)
        }
    }
    pub fn catalog_dungeons(&self) -> Result<Vec<String>, String> {
        if self.demo.load(Ordering::Acquire) {
            Ok(crate::assets_bridge::catalog_dungeons(&demo_catalog()?))
        } else {
            self.assets.catalog_dungeons()
        }
    }
    pub fn catalog_dungeon_options(&self) -> Result<Vec<maxed_types::DungeonDef>, String> {
        if self.demo.load(Ordering::Acquire) {
            Ok(crate::assets_bridge::catalog_dungeon_options(
                &demo_catalog()?,
            ))
        } else {
            self.assets.catalog_dungeon_options()
        }
    }
    pub fn dungeon_portal_type(&self, dungeon: &str) -> Result<Option<u16>, String> {
        if self.demo.load(Ordering::Acquire) {
            Ok(crate::assets_bridge::dungeon_portal_type(
                &demo_catalog()?,
                dungeon,
            ))
        } else {
            self.assets.dungeon_portal_type(dungeon)
        }
    }
    fn probe_recent(&self) -> bool {
        self.probe_checked
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some_and(|at| at.elapsed() < Duration::from_secs(10))
    }
    fn failed_candidate_unchanged(&self) -> bool {
        self.failed_candidate
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .is_some_and(|failed| *failed == FailedCandidate::read(&failed.path))
    }
    /// Reserve visible progress before returning the first query. Filesystem,
    /// probe and extraction work belongs to one joinable blocking worker.
    pub fn query(self: &Arc<Self>) -> SetupStatus {
        self.query_with_retry(false)
    }
    /// Explicit Retry checks may retry unchanged failures, including while live.
    pub fn query_with_retry(self: &Arc<Self>, retry: bool) -> SetupStatus {
        if self.gui_smoke {
            return self.status();
        }
        let Ok(_serial) = self.serial.try_lock() else {
            return self.status();
        };
        let Ok(mut worker) = self.refresh_worker.try_lock() else {
            return self.status();
        };
        if self.closing.load(Ordering::Acquire)
            || self.demo.load(Ordering::Acquire)
            || (self.lifecycle.running() && !retry)
            || worker.as_ref().is_some_and(|w| !w.is_finished())
        {
            return self.status();
        }
        if matches!(
            self.assets.status().assets,
            maxed_types::AssetCheck::Finding | maxed_types::AssetCheck::Extracting
        ) {
            return self.status();
        }
        if retry {
            self.discovered.store(false, Ordering::Release);
            *self
                .failed_candidate
                .lock()
                .unwrap_or_else(|p| p.into_inner()) = None;
            *self.probe_checked.lock().unwrap_or_else(|p| p.into_inner()) = None;
        }
        let discovery =
            !self.discovered.load(Ordering::Acquire) && !self.failed_candidate_unchanged();
        if !discovery && (self.lifecycle.running() || self.probe_recent()) {
            return self.status();
        }
        if self.begin_transition().is_err() {
            return self.status();
        }
        let operation = if discovery {
            match self.assets.begin() {
                Ok(op) => Some(op),
                Err(_) => return self.status(),
            }
        } else {
            None
        };
        if let Some(previous) = worker.take() {
            let _ = previous.join();
        }
        let service = self.clone();
        let pending = operation.clone();
        match std::thread::Builder::new()
            .name("maxed-setup-check".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    service.refresh(pending.as_ref(), retry)
                }));
                if result.is_err() {
                    if let Some(op) = pending {
                        service.assets.failed(&op);
                    }
                }
            }) {
            Ok(handle) => *worker = Some(handle),
            Err(_) => {
                if let Some(op) = operation {
                    self.assets.failed(&op);
                }
            }
        }
        self.status()
    }
    fn refresh(&self, operation: Option<&Operation>, retry: bool) {
        let _serial = self.serial.lock().unwrap_or_else(|p| p.into_inner());
        // Start can acquire the transition lock after a query reserved work but
        // before this worker runs. Automatic discovery must remain idle then.
        if self.demo.load(Ordering::Acquire) || (self.lifecycle.running() && !retry) {
            if let Some(op) = operation {
                self.assets.abandon_if_pending(op);
            }
            return;
        }
        if self.closing.load(Ordering::Acquire)
            || self.cancel.load(Ordering::Acquire)
            || operation.is_some_and(|op| !self.assets.current(op))
        {
            return;
        }
        let Ok(settings) = self.settings.get() else {
            if let Some(op) = operation {
                self.assets.finish_discovery(op, Some("Saved settings are unavailable. Restore a supported settings file and retry."));
            }
            return;
        };
        if !self.lifecycle.running() && !self.probe_recent() {
            if let Ok(handle) = self.opened(settings.adapter_id.clone(), &self.cancel) {
                let _ = handle.stop();
            }
        }
        let Some(op) = operation else {
            return;
        };
        if !self.assets.current(op) {
            return;
        }
        match discover_with_launcher(
            settings.assets_path.as_deref().map(Path::new),
            &self.discovery_roots,
            self.launcher_root.as_deref(),
        ) {
            Discovery::Found(path) => {
                let candidate = FailedCandidate::read(&path);
                let mut desired = settings;
                desired.assets_path = Some(path.to_string_lossy().into_owned());
                if self.apply_locked(desired, Some(op.clone()), false).is_ok() {
                    self.discovered.store(true, Ordering::Release);
                    *self
                        .failed_candidate
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = None;
                } else if self.assets.current(op) {
                    *self
                        .failed_candidate
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = Some(candidate);
                }
            }
            outcome => {
                self.assets.finish_discovery(
                    op,
                    Some(match outcome {
                        Discovery::Multiple => {
                            "Multiple game clients found. Select the resources.assets file to use."
                        }
                        _ => "Find your game client by selecting its resources.assets file.",
                    }),
                );
            }
        }
    }
    pub fn set_adapter(&self, id: Option<String>) -> Result<(), String> {
        let _serial = self.serial.lock().map_err(|_| CAPTURE_ERROR)?;
        self.begin_transition()?;
        let mut settings = self.settings.get()?;
        settings.adapter_id = id;
        self.apply_locked(settings, None, true)
    }
    pub fn set_assets(&self, path: String) -> Result<(), String> {
        let op = self.reserve_assets()?;
        self.set_assets_reserved(path, op)
    }
    /// Reserve at IPC receipt, before dispatch to the blocking pool. Worker
    /// scheduling must never invert two successive user selections.
    pub fn reserve_assets(&self) -> Result<Operation, String> {
        if self.gui_smoke {
            return Err(crate::assets_bridge::ASSET_ERROR.into());
        }
        self.discovered.store(true, Ordering::Release);
        *self
            .failed_candidate
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
        self.assets.begin()
    }
    pub fn set_assets_reserved(&self, path: String, op: Operation) -> Result<(), String> {
        let result = (|| {
            let _serial = self.serial.lock().map_err(|_| CAPTURE_ERROR)?;
            self.begin_transition()?;
            self.discovered.store(true, Ordering::Release);
            let mut settings = self.settings.get()?;
            settings.assets_path = Some(path);
            self.apply_locked(settings, Some(op.clone()), false)
        })();
        if result.is_err() {
            self.assets.abandon_if_pending(&op);
        }
        result
    }
    pub fn update(&self, settings: AppSettings) -> Result<(), String> {
        let op = self.reserve_update(&settings)?;
        self.update_reserved(settings, op)
    }
    pub fn reserve_update(&self, settings: &AppSettings) -> Result<Option<Operation>, String> {
        validate(settings)?;
        if settings.assets_path.is_some()
            && settings.assets_path != self.settings.get()?.assets_path
        {
            self.reserve_assets().map(Some)
        } else {
            Ok(None)
        }
    }
    pub fn update_reserved(
        &self,
        settings: AppSettings,
        op: Option<Operation>,
    ) -> Result<(), String> {
        let _serial = self.serial.lock().map_err(|_| CAPTURE_ERROR)?;
        self.begin_transition()?;
        let previous = self.settings.get()?;
        let op = if op.is_none()
            && settings.assets_path.is_some()
            && settings.assets_path != previous.assets_path
        {
            Some(self.reserve_assets()?)
        } else {
            op
        };
        self.apply_locked(settings, op, false)
    }
    fn apply_locked(
        &self,
        mut desired: AppSettings,
        op: Option<Operation>,
        verify_adapter: bool,
    ) -> Result<(), String> {
        let result = self.apply_inner(&mut desired, op.as_ref(), verify_adapter);
        if result.is_err() {
            if let Some(op) = &op {
                self.assets.abandon_if_pending(op);
            }
        }
        result
    }
    fn apply_inner(
        &self,
        desired: &mut AppSettings,
        op: Option<&Operation>,
        verify_adapter: bool,
    ) -> Result<(), String> {
        validate(desired)?;
        let previous = self.settings.get()?;
        let demo = self.demo.load(Ordering::Acquire);
        if demo && desired.save_recordings && !previous.save_recordings {
            return Err("Leave demo before enabling recordings.".into());
        }
        if demo && (verify_adapter || previous.adapter_id != desired.adapter_id) {
            return Err("Leave demo before changing the live capture adapter.".into());
        }
        if desired.assets_path != previous.assets_path && op.is_none() {
            return Err("Asset selection must finish before saving settings.".into());
        }
        if previous.assets_path.is_some() && desired.assets_path.is_none() {
            return Err(
                "Select a replacement assets file before removing the current selection.".into(),
            );
        }
        let old_cache = self.assets.cache();
        let source_stamp = desired
            .assets_path
            .as_deref()
            .and_then(|path| resources_path(Path::new(path)).ok())
            .and_then(|path| crate::startup_cache::Stamp::read(&path));
        let prepared = if let Some(op) = op {
            let path = desired
                .assets_path
                .as_ref()
                .ok_or(crate::assets_bridge::ASSET_ERROR)?;
            let (path, cache) = self.assets.prepare(Path::new(path), &self.root, op)?;
            // Persist a normal drive path, not the Windows canonical device prefix.
            let path = path.to_string_lossy();
            desired.assets_path = Some(path.strip_prefix("\\\\?\\").unwrap_or(&path).to_owned());
            Some(cache)
        } else {
            None
        };
        let catalog_changed = prepared.as_ref().is_some_and(|next| {
            old_cache
                .as_ref()
                .is_none_or(|old| old.catalog.client_fingerprint != next.catalog.client_fingerprint)
        });
        let recording_changed = previous.save_recordings != desired.save_recordings
            || previous.recording_limit_bytes != desired.recording_limit_bytes;
        let changed = previous.adapter_id != desired.adapter_id || catalog_changed;
        let live = !demo && self.lifecycle.running();
        let running = changed && live;
        if running {
            self.lifecycle.stop()?;
        }
        let result = (|| {
            if recording_changed || running {
                self.lifecycle.configure_recording(
                    desired.save_recordings && !demo,
                    desired.recording_limit_bytes,
                    live,
                )?;
            }
            if !demo && (running || verify_adapter || previous.adapter_id != desired.adapter_id) {
                let cancel = op.as_ref().map_or(&self.cancel, |v| v.cancel.as_ref());
                let handle = self.opened(desired.adapter_id.clone(), cancel)?;
                if running {
                    self.lifecycle
                        .start(prepared.clone().or_else(|| old_cache.clone()), handle)?;
                } else {
                    handle.stop().map_err(|_| CAPTURE_ERROR)?;
                }
            }
            if let (Some(op), Some(cache)) = (op, prepared) {
                let receipt_cache = cache.clone();
                self.assets.commit(op, cache, || {
                    self.updates
                        .commit_channel(desired.update_channel.clone(), || {
                            self.settings.put(desired)
                        })?;
                    crate::startup_cache::record(&self.root, source_stamp, &receipt_cache);
                    Ok(())
                })
            } else {
                self.updates
                    .commit_channel(desired.update_channel.clone(), || {
                        self.settings.put(desired)
                    })
            }
        })();
        if result.is_err() && (recording_changed || running) {
            // Retire failed admission before restoring the previously committed choice.
            let _ = self.lifecycle.configure_recording(
                previous.save_recordings && !demo,
                previous.recording_limit_bytes,
                live && !running,
            );
        }
        if result.is_err() && running {
            self.lifecycle.stop()?;
            if !self.closing.load(Ordering::Acquire) && !self.cancel.load(Ordering::Acquire) {
                let handle = self.opened(previous.adapter_id, &self.cancel)?;
                let _ = self.lifecycle.configure_recording(
                    previous.save_recordings,
                    previous.recording_limit_bytes,
                    true,
                );
                self.lifecycle.start(old_cache, handle).map_err(|_| {
                    "Selection failed and the previous capture could not restart. Retry capture."
                })?;
            }
        }
        result
    }
    pub fn start(&self) -> Result<(), String> {
        let _serial = self.serial.lock().map_err(|_| CAPTURE_ERROR)?;
        self.begin_transition()?;
        if self.demo.load(Ordering::Acquire) {
            self.lifecycle.stop()?;
            self.lifecycle.demo(false)?;
            self.demo.store(false, Ordering::Release);
        }
        if self.lifecycle.running() {
            return Err("Capture is already running.".into());
        }
        let settings = self.settings.get()?;
        if let Some(path) = settings
            .assets_path
            .as_deref()
            .filter(|_| self.assets.cache().is_none() && !self.failed_candidate_unchanged())
        {
            let candidate = FailedCandidate::read(Path::new(path));
            let source_stamp = resources_path(Path::new(path))
                .ok()
                .and_then(|path| crate::startup_cache::Stamp::read(&path));
            let op = self.assets.begin()?;
            // The path is already persisted. Loading it needs no settings write.
            let prepared = self
                .assets
                .prepare(Path::new(path), &self.root, &op)
                .and_then(|(_, cache)| {
                    let receipt_cache = cache.clone();
                    self.assets.commit(&op, cache, || {
                        crate::startup_cache::record(&self.root, source_stamp, &receipt_cache);
                        Ok(())
                    })
                });
            if let Err(error) = prepared {
                if !self.assets.current(&op) || error == crate::assets_bridge::CANCELLED {
                    return Err(crate::assets_bridge::CANCELLED.into());
                }
                self.assets.failed(&op);
                *self
                    .failed_candidate
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = Some(candidate);
            } else {
                self.discovered.store(true, Ordering::Release);
                *self
                    .failed_candidate
                    .lock()
                    .unwrap_or_else(|p| p.into_inner()) = None;
            }
        }
        let handle = self.opened(settings.adapter_id, &self.cancel)?;
        self.demo.store(false, Ordering::Release);
        // A saved preference cannot take capture or summaries down when storage fails.
        let _ = self.lifecycle.configure_recording(
            settings.save_recordings,
            settings.recording_limit_bytes,
            true,
        );
        let result = self.lifecycle.start(self.assets.cache(), handle);
        if result.is_err() {
            let _ =
                self.lifecycle
                    .configure_recording(false, settings.recording_limit_bytes, false);
        }
        result
    }
    pub fn stop(&self) -> Result<(), String> {
        self.cancel_pending(false);
        let _serial = self.serial.lock().map_err(|_| CAPTURE_ERROR)?;
        self.lifecycle.stop()
    }
    pub fn set_demo(&self, enabled: bool) -> Result<(), String> {
        self.cancel_pending(false);
        let _serial = self.serial.lock().map_err(|_| CAPTURE_ERROR)?;
        self.begin_transition()?;
        self.lifecycle.stop()?;
        // Set the marker before a demo worker can publish decoded traffic.
        if enabled {
            self.demo.store(true, Ordering::Release);
        }
        if let Err(error) = self.lifecycle.demo(enabled) {
            // No old source is running. Clear its presentation before returning
            // a safe resource/start error; never leave demo data labelled live.
            // Publication can fail after launch, so join that worker as well.
            self.lifecycle.stop()?;
            self.lifecycle.demo(false)?;
            self.demo.store(false, Ordering::Release);
            return Err(error);
        }
        self.demo.store(enabled, Ordering::Release);
        Ok(())
    }
    pub fn cancel_pending(&self, closing: bool) {
        self.cancel.store(true, Ordering::Release);
        if closing {
            self.closing.store(true, Ordering::Release);
        }
        self.assets.cancel(closing);
    }
    pub fn join_pending(&self) {
        let worker = self
            .refresh_worker
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
        drop(self.serial.lock().unwrap_or_else(|p| p.into_inner()));
    }
    pub fn resume_after_cancelled_exit(&self) {
        self.updates.resume();
        self.closing.store(false, Ordering::Release);
        self.assets.resume();
    }

    /// Read committed settings off the UI thread, under the same transition owner.
    pub fn initialize_updates(&self) -> Result<(), String> {
        let _serial = self.serial.lock().map_err(|_| CAPTURE_ERROR)?;
        let settings = self.settings.get()?;
        self.updates
            .commit_channel(settings.update_channel, || Ok(()))
    }

    pub fn database_size(&self) -> Result<u64, String> {
        crate::storage_usage::database_size(&self.root)
    }

    pub fn history_upgrade_status(&self) -> Result<maxed_types::HistoryUpgradeStatus, String> {
        self.lifecycle.history_upgrade_status()
    }
    pub fn backup_and_upgrade_history(&self) -> Result<maxed_types::HistoryUpgradeStatus, String> {
        {
            let _serial = self
                .serial
                .lock()
                .map_err(|_| "History upgrade is unavailable.")?;
            if self.demo.load(Ordering::Acquire)
                || self.closing.load(Ordering::Acquire)
                || self.updates.installing()
            {
                return Err("History upgrade is unavailable during playback or shutdown.".into());
            }
        }
        self.lifecycle.backup_and_upgrade_history()
    }
    pub fn storage_status(&self) -> Result<maxed_types::StorageStatus, String> {
        let settings = self.settings.get()?;
        Ok(crate::storage_usage::status(
            &self.root,
            self.lifecycle
                .recording_status(settings.recording_limit_bytes),
        ))
    }
    /// Local Settings only; never include this location in a notice or log.
    pub fn storage_backup_location(&self) -> Result<String, String> {
        self.root
            .join("backups/live")
            .into_os_string()
            .into_string()
            .map_err(|_| "Backup location is unavailable.".into())
    }

    pub fn recording_status(&self) -> Result<maxed_types::RecordingStatus, String> {
        let settings = self.settings.get()?;
        let mut status = self
            .lifecycle
            .recording_status(settings.recording_limit_bytes);
        if settings.save_recordings && status.state == maxed_types::RecordingState::Disabled {
            status.state = maxed_types::RecordingState::Armed;
        }
        Ok(status)
    }

    /// Installing freezes all setup transitions through this existing lease.
    /// Staging precedes the durability gate; the worker owns staging until exit.
    pub fn install_update(
        &self,
        updater: &mut crate::updater::NativeUpdater,
    ) -> crate::updater::Result<crate::updater::RunningInstaller> {
        self.install_update_with(updater, crate::updater::RunningInstaller::launch)
    }

    /// Internal launch seam; production and tests share the same transition and
    /// durability gate. Only the final process launch is substituted in tests.
    pub(crate) fn install_update_with<T>(
        &self,
        updater: &mut crate::updater::NativeUpdater,
        launch: impl FnOnce((crate::updater::staging::Staged, PathBuf)) -> crate::updater::Result<T>,
    ) -> crate::updater::Result<T> {
        let _serial = self
            .serial
            .lock()
            .map_err(|_| crate::updater::Error::Shutdown)?;
        if self.closing.load(Ordering::Acquire) || !self.updates.installing() {
            return Err(crate::updater::Error::Shutdown);
        }
        let committed = self
            .settings
            .get()
            .map_err(|_| crate::updater::Error::Shutdown)?;
        if self.updates.status().channel != committed.update_channel {
            return Err(crate::updater::Error::Stale);
        }
        let result = updater.install_with(
            &self.root,
            || {
                self.lifecycle
                    .prepare_update()
                    .map_err(|_| crate::updater::Error::Shutdown)
            },
            launch,
        );
        self.lifecycle.finish_update();
        result
    }
}

fn demo_catalog() -> Result<maxed_types::Catalog, String> {
    serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
    ))
    .map_err(|_| "Synthetic demo catalog is unavailable.".into())
}

pub fn probe_result(result: Result<(), CaptureError>) -> (NpcapCheck, CaptureAccess) {
    match result {
        Ok(()) => (NpcapCheck::Compatible, CaptureAccess::Allowed),
        Err(CaptureError::Missing) => (NpcapCheck::Missing, CaptureAccess::Unknown),
        Err(CaptureError::Incompatible) => (NpcapCheck::Incompatible, CaptureAccess::Unknown),
        Err(CaptureError::PermissionDenied) => {
            (NpcapCheck::PermissionDenied, CaptureAccess::Denied)
        }
        Err(_) => (NpcapCheck::Failed, CaptureAccess::Unknown),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Discovery {
    Found(PathBuf),
    Multiple,
    Missing,
}
pub(crate) fn steam_roots() -> Vec<PathBuf> {
    ["ProgramFiles(x86)", "ProgramFiles"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join("Steam"))
        .collect()
}
fn manifest(path: &Path) -> Option<String> {
    let path = crate::assets_bridge::local_path(path).ok()?;
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(65537)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 65536 {
        return None;
    }
    String::from_utf8(bytes).ok()
}
// Steam KeyValues strings only; no scripting or arbitrary file includes.
fn values(text: &str, key: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '"' {
            continue;
        }
        let mut token = String::new();
        while let Some(ch) = chars.next() {
            if ch == '"' {
                break;
            }
            if ch == '\\' {
                if let Some(escaped) = chars.next() {
                    token.push(escaped);
                }
            } else {
                token.push(ch);
            }
        }
        tokens.push(token);
    }
    tokens
        .windows(2)
        .filter(|pair| pair[0] == key)
        .map(|pair| pair[1].clone())
        .collect()
}
pub fn discover(saved: Option<&Path>, roots: &[PathBuf]) -> Discovery {
    discover_with_launcher(saved, roots, None)
}
pub fn discover_with_launcher(
    saved: Option<&Path>,
    roots: &[PathBuf],
    local_data: Option<&Path>,
) -> Discovery {
    if let Some(path) = saved.and_then(|p| resources_path(p).ok()) {
        return Discovery::Found(path);
    }
    let mut found = std::collections::BTreeSet::new();
    if let Some(path) = local_data.and_then(|root| {
        resources_path(&root.join("RealmOfTheMadGod/Production/RotMG Exalt_Data/resources.assets"))
            .ok()
    }) {
        found.insert(path);
    }
    let mut libraries = std::collections::BTreeSet::new();
    for root in roots.iter().take(8) {
        if !root.is_absolute() {
            continue;
        }
        libraries.insert(root.clone());
        if let Some(text) = manifest(&root.join("steamapps/libraryfolders.vdf")) {
            for path in values(&text, "path").into_iter().take(16) {
                let path = PathBuf::from(path);
                if path.is_absolute() && !path.to_string_lossy().starts_with("\\\\") {
                    libraries.insert(path);
                }
            }
        }
    }
    for library in libraries.into_iter().take(32) {
        let Some(text) = manifest(&library.join("steamapps/appmanifest_200210.acf")) else {
            continue;
        };
        if values(&text, "appid").first().map(String::as_str) != Some("200210") {
            continue;
        }
        for directory in values(&text, "installdir").into_iter().take(1) {
            if directory.is_empty()
                || directory.contains(['/', '\\', ':'])
                || directory == "."
                || directory == ".."
            {
                continue;
            }
            if let Ok(path) = resources_path(
                &library
                    .join("steamapps/common")
                    .join(directory)
                    .join("RotMG Exalt_Data/resources.assets"),
            ) {
                found.insert(path);
            }
        }
    }
    match found.len() {
        0 => Discovery::Missing,
        1 => Discovery::Found(found.into_iter().next().unwrap()),
        _ => Discovery::Multiple,
    }
}

#[cfg(test)]
#[path = "../tests/support/recording_setup.rs"]
mod recording_tests;

#[cfg(all(test, debug_assertions))]
mod gui_smoke_tests {
    use super::*;

    struct NoCapture;
    impl CaptureBackend for NoCapture {
        fn probe(&self) -> Result<(), CaptureError> {
            panic!("smoke touched Npcap")
        }
        fn adapters(&self) -> Result<Vec<maxed_capture::AdapterInfo>, CaptureError> {
            panic!("smoke enumerated adapters")
        }
        fn start(&self, _: CaptureConfig) -> Result<CaptureHandle, CaptureError> {
            panic!("smoke started capture")
        }
    }
    struct NoLifecycle;
    impl Lifecycle for NoLifecycle {
        fn running(&self) -> bool {
            false
        }
        fn live(&self) -> bool {
            false
        }
        fn stop(&self) -> Result<(), String> {
            Ok(())
        }
        fn start(&self, _: Option<CachedCatalog>, _: CaptureHandle) -> Result<(), String> {
            panic!("smoke started the engine")
        }
        fn demo(&self, _: bool) -> Result<(), String> {
            panic!("smoke started demo")
        }
    }

    #[test]
    fn demo_history_uses_shared_demo_classifications() {
        let temp = tempfile::tempdir().unwrap();
        let service = SetupService::with_discovery_roots(
            temp.path().to_owned(),
            Arc::new(NoCapture),
            Arc::new(NoLifecycle),
            Arc::new(AssetBridge::default()),
            Vec::new(),
        )
        .unwrap();
        let live = service.catalog_query_data();
        service.demo.store(true, Ordering::Release);
        let data = service.catalog_query_data();
        let catalog = demo_catalog().unwrap();
        assert_eq!(
            data.chase_types(),
            catalog
                .items
                .iter()
                .filter_map(|item| matches!(item.tier.as_deref(), Some("UT" | "ST"))
                    .then_some(item.object_type))
                .collect::<Vec<_>>()
        );
        // Pin the actual embedded demo contract: no potion classifications.
        // Checking every stat below also catches accidental live-data leakage.
        let demo_potions: Vec<_> = catalog
            .items
            .iter()
            .filter_map(|item| item.potion_stat.map(|stat| (item.object_type, stat)))
            .collect();
        assert_eq!(demo_potions, vec![]);
        use maxed_types::PotionStat;
        for stat in [
            PotionStat::Life,
            PotionStat::Mana,
            PotionStat::Attack,
            PotionStat::Defense,
            PotionStat::Speed,
            PotionStat::Dexterity,
            PotionStat::Vitality,
            PotionStat::Wisdom,
        ] {
            assert_eq!(data.potion_type(stat), None);
        }
        assert!(data.shares_instance(&service.catalog_query_data()));
        assert_ne!(live.identity(), data.identity());
        service.demo.store(false, Ordering::Release);
        assert!(live.shares_instance(&service.catalog_query_data()));
    }

    #[test]
    fn smoke_refuses_setup_work_and_keeps_normal_unavailable_state() {
        let temp = tempfile::tempdir().unwrap();
        let mut service = SetupService::with_discovery_roots(
            temp.path().to_owned(),
            Arc::new(NoCapture),
            Arc::new(NoLifecycle),
            Arc::new(AssetBridge::default()),
            Vec::new(),
        )
        .unwrap();
        service.gui_smoke = true;
        let service = Arc::new(service.with_launcher_root(Some(temp.path().join("launcher"))));
        assert!(service.launcher_root.is_none());
        assert!(service.startup_setup_required());
        for retry in [false, true] {
            let status = service.query_with_retry(retry);
            assert_eq!(status.npcap, NpcapCheck::Unchecked);
            assert_eq!(status.access, CaptureAccess::Unknown);
            assert_eq!(status.assets, maxed_types::AssetCheck::Missing);
            assert!(!status.client_found);
            assert!(!status.demo);
            assert!(service.refresh_worker.lock().unwrap().is_none());
        }
        assert!(service.adapters().is_err());
        assert!(service.opened(None, &AtomicBool::new(false)).is_err());
        assert!(service.start().is_err());
        assert!(service.set_adapter(None).is_err());
        assert!(service.reserve_assets().is_err());
        assert!(service
            .set_assets("unavailable/resources.assets".into())
            .is_err());
        let operation = service.assets.begin().unwrap();
        assert!(service
            .set_assets_reserved("unavailable/resources.assets".into(), operation)
            .is_err());
        let mut settings = service.settings.get().unwrap();
        settings.assets_path = Some("unavailable/resources.assets".into());
        assert!(service.update(settings.clone()).is_err());
        assert!(service.update_reserved(settings, None).is_err());
        assert!(service.set_demo(true).is_err());
        assert!(!service.catalog_available());
        assert!(service.assets.cache().is_none());
    }
}
