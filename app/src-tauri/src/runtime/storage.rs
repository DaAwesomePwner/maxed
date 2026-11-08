use super::{
    join::{unavailable_catalog, Adapter, Bridge, Views},
    CaptureRuntime,
};
use maxed_assets::{load_or_extract, AssetPaths};
use maxed_engine::EngineConfig;
use maxed_store::WriterConfig;
use maxed_types::Catalog;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace {
    Live,
    Demo,
    Replay,
}
impl Namespace {
    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Demo => "demo",
            Self::Replay => "replay",
        }
    }
}

/// Native orchestration options. No private paths are included in bridge DTOs.
pub struct RuntimeOptions {
    pub app_data: PathBuf,
    /// None explicitly selects partial, unavailable-catalog mode. Some(path)
    /// is validated against that client's content fingerprint. Failure uses an
    /// explicitly reported partial mode, never a stale cache.
    pub resources: Option<PathBuf>,
    pub namespace: Namespace,
    pub prepared_catalog: Option<maxed_assets::CachedCatalog>,
    #[cfg(test)]
    pub(super) temporary: Option<tempfile::TempDir>,
}

impl RuntimeOptions {
    /// App-owned private history images; command clients use manifest IDs only.
    pub fn backup_directory(&self) -> PathBuf {
        self.app_data.join("backups").join(self.namespace.label())
    }
    pub fn live(app_data: PathBuf, resources: Option<PathBuf>) -> Self {
        Self {
            app_data,
            resources,
            namespace: Namespace::Live,
            prepared_catalog: None,
            #[cfg(test)]
            temporary: None,
        }
    }
    pub(super) fn database(&self) -> PathBuf {
        self.app_data
            .join(format!("{}.sqlite3", self.namespace.label()))
    }
    #[cfg(test)]
    pub(super) fn open(
        &self,
        cancel: &AtomicBool,
        views: Arc<Views>,
        bridge: Bridge,
    ) -> Result<Adapter, String> {
        self.open_catalog(cancel, views, bridge, None)
    }
    pub(super) fn open_catalog(
        &self,
        cancel: &AtomicBool,
        views: Arc<Views>,
        bridge: Bridge,
        demo_catalog: Option<Arc<Catalog>>,
    ) -> Result<Adapter, String> {
        let lease = namespace_lease(&self.app_data, self.namespace)?;
        // Synthetic identities never create or read the owner's live identity key.
        let identity_key = if self.namespace == Namespace::Demo {
            [7; 32]
        } else {
            identity_key(&self.app_data)?
        };
        let (catalog, cached, missing, notice) = if self.namespace == Namespace::Demo {
            let catalog = match demo_catalog {
                Some(catalog) => catalog,
                None => Arc::new(
                    serde_json::from_slice::<Catalog>(include_bytes!(
                        "../../../../tests/fixtures/scrubbed/maxed-demo/catalog.json"
                    ))
                    .map_err(|_| "Synthetic demo catalog is invalid")?,
                ),
            };
            (catalog, None, false, None)
        } else if let Some(cached) = &self.prepared_catalog {
            (cached.catalog.clone(), Some(cached.clone()), false, None)
        } else if let Some(resources) = &self.resources {
            match load_or_extract(&AssetPaths { resources: resources.clone(), app_data: self.app_data.clone() }, cancel, &mut |_| {}) {
                Ok(cached) => {
                    cached.prune_in_background();
                    (cached.catalog.clone(), Some(cached), false, None)
                },
                Err(maxed_assets::AssetError::Cancelled) => (unavailable_catalog(), None, true,
                    Some("Client catalog extraction cancelled; unavailable-catalog partial mode.")),
                Err(_) => (unavailable_catalog(), None, true,
                    Some("Client catalog validation failed; capturing in unavailable-catalog partial mode.")),
            }
        } else {
            (
                unavailable_catalog(),
                None,
                true,
                Some("Client catalog unavailable; capture coverage is partial."),
            )
        };
        let stream_id = format!("{}-{}", self.namespace.label(), random_id()?);
        // The namespace lease excludes another playback/recovery owner. Reuse
        // one bounded scratch path; live history never participates in cleanup.
        let database = self.database();
        if self.namespace != Namespace::Live {
            super::join::remove_playback_database(&database)?;
        }
        let mut adapter = Adapter::open(
            database,
            catalog,
            EngineConfig {
                stream_id: stream_id.clone(),
                identity_key,
                ..Default::default()
            },
            WriterConfig {
                stream_id,
                ..Default::default()
            },
            missing,
            views,
            bridge,
        )?;
        adapter.lease = Some(lease);
        adapter.catalog_lease = cached;
        adapter.catalog_notice = notice;
        adapter.ephemeral = self.namespace != Namespace::Live;
        adapter.live = self.namespace == Namespace::Live;
        adapter.refresh_store();
        Ok(adapter)
    }
}

/// Shared by capture and cold-history preparation. Directory creation must
/// precede lease acquisition, including on a fresh installation.
pub(super) fn namespace_lease(root: &Path, namespace: Namespace) -> Result<File, String> {
    fs::create_dir_all(root).map_err(|_| "Could not prepare local summary storage")?;
    exclusive(&root.join(format!("{}.lock", namespace.label())))
}

fn exclusive(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    #[cfg(not(windows))]
    return Err("Summary namespace ownership requires Windows".into());
    #[cfg(windows)]
    options
        .open(path)
        .map_err(|_| "Summary storage is already in use or unavailable".into())
}

fn identity_key(root: &Path) -> Result<[u8; 32], String> {
    let _lease = exclusive(&root.join("identity.lock"))?;
    let path = root.join("identity.key");
    let mut key = [0; 32];
    let existing = match File::open(&path) {
        Ok(file) => Some(file),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("Could not read local identity key".into()),
    };
    if let Some(mut file) = existing {
        let len = file
            .metadata()
            .map_err(|_| "Could not read local identity key")?
            .len();
        if len == 32 {
            file.read_exact(&mut key)
                .map_err(|_| "Could not read local identity key")?;
            return Ok(key);
        }
        if len != 0 {
            return Err("Local identity key is invalid".into());
        }
        // An empty key could never have been used to hash account history.
        drop(file);
        fs::remove_file(&path).map_err(|_| "Could not save local identity key")?;
    }
    getrandom::fill(&mut key).map_err(|_| "Could not create local identity key")?;
    let staging = root.join("identity.key.pending");
    match fs::remove_file(&staging) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Could not save local identity key".into()),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)
        .map_err(|_| "Could not save local identity key")?;
    let result = (|| {
        file.write_all(&key)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Could not save local identity key")?;
        drop(file);
        // Publish only a complete, synced key while holding the identity lease.
        publish_identity_key(&staging, &path).map_err(|_| "Could not save local identity key")
    })();
    if result.is_err() {
        let _ = fs::remove_file(staging);
    }
    result?;
    Ok(key)
}

fn publish_identity_key(staging: &Path, path: &Path) -> std::io::Result<()> {
    publish_identity_key_with(staging, path, |from, to| fs::hard_link(from, to))
}

fn publish_identity_key_with(
    staging: &Path,
    path: &Path,
    link: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> std::io::Result<()> {
    // Unlike rename on Windows, hard_link never replaces an existing key.
    if let Err(error) = link(staging, path) {
        if error.kind() != std::io::ErrorKind::Unsupported
            && !(cfg!(windows) && matches!(error.raw_os_error(), Some(1 | 50)))
        {
            return Err(error);
        }
        // Redirected app data may live on a filesystem without hard links.
        // The caller still holds the identity lease. Fail closed on read errors
        // and use create_new to protect against a destination appearing later.
        match File::open(path) {
            Ok(_) => return Err(std::io::ErrorKind::AlreadyExists.into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let key = fs::read(staging)?;
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        file.write_all(&key)?;
        file.sync_all()?;
        return fs::remove_file(staging);
    }
    // Flush the published file before removing the staging name. On failure,
    // leave the complete published key intact so the next attempt reuses it.
    OpenOptions::new().write(true).open(path)?.sync_all()?;
    fs::remove_file(staging)
}

#[cfg(test)]
#[path = "identity_key_tests.rs"]
mod identity_key_tests;

pub(super) fn random_id() -> Result<String, String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Could not create capture stream identity")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Discovery is local-only. Selection/fingerprinting stays on the worker.
pub fn discover_resources() -> Option<PathBuf> {
    match crate::setup::discover(None, &crate::setup::steam_roots()) {
        crate::setup::Discovery::Found(path) => Some(path),
        _ => None,
    }
}

impl CaptureRuntime {
    pub fn snapshot(&self) -> maxed_types::LiveSnapshot {
        // Read on the engine owner, independently of WebView publication cadence.
        // Never hold a lifecycle/view lock while waiting: Stop must remain able
        // to join the worker. Disconnection falls back to its final cached view.
        let requests = self
            .shared
            .snapshot_requests
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(requests) = requests {
            let (reply, response) = crossbeam_channel::bounded(1);
            if requests.send(reply).is_ok() {
                if let Ok(snapshot) = response.recv() {
                    return snapshot;
                }
            }
        }
        self.shared
            .views
            .snapshot
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn store_status(&self) -> maxed_types::StoreStatus {
        // Finished capture workers no longer poll. Refresh their writer status
        // without waiting for an in-progress retry/close lifecycle operation.
        if let Ok(completed) = self.shared.completed.try_lock() {
            if let Some(adapter) = completed.as_ref() {
                adapter.refresh_store();
            }
        }
        if let Ok(recovery) = self.shared.recovery.try_lock() {
            if let Some(adapter) = recovery.as_ref() {
                adapter.refresh_store();
            }
        }
        self.shared
            .views
            .store
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    pub fn retry_store(&self) -> Result<(), String> {
        if !self.has_unsaved_summaries() {
            let status = self.store_status();
            if status.state != maxed_types::StoreState::Failed || status.queued == 0 {
                return Ok(());
            }
        }
        // Retire the live/completed owner before retrying its retained writer.
        // A storage error from stop is precisely what this operation recovers.
        let _ = self.stop();
        let _lifecycle = self.session.lock().map_err(|_| super::WORKER_FAILED)?;
        let mut recovery = self
            .shared
            .recovery
            .lock()
            .map_err(|_| super::WORKER_FAILED)?;
        if let Some(adapter) = recovery.as_mut() {
            let result = adapter.retry_and_close();
            adapter.refresh_store();
            result?;
            *recovery = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maxed_types::{CoverageIssue, FightEnd, Timestamp};

    #[test]
    fn namespaces_use_distinct_streams_databases_and_exclusive_ownership() {
        let root = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        let mut options = RuntimeOptions::live(root.path().into(), None);
        let mut ids = std::collections::HashSet::new();
        for namespace in [
            Namespace::Live,
            Namespace::Demo,
            Namespace::Replay,
            Namespace::Demo,
            Namespace::Replay,
        ] {
            options.namespace = namespace;
            let views = Arc::new(Views::default());
            let mut adapter = options
                .open(&cancel, views.clone(), Bridge::default())
                .unwrap();
            let snapshot = views.snapshot.lock().unwrap().clone();
            assert!(snapshot.stream_id.starts_with(namespace.label()));
            assert!(ids.insert(snapshot.stream_id.clone()));
            assert_eq!(
                snapshot.issues.contains(&CoverageIssue::MissingCatalog),
                namespace != Namespace::Demo
            );
            assert!(options
                .open(&cancel, Arc::new(Views::default()), Bridge::default())
                .is_err());
            adapter
                .finish(Timestamp(0), FightEnd::CaptureStopped)
                .unwrap();
            adapter.close().unwrap();
            let name = if namespace == Namespace::Live {
                "live.sqlite3".into()
            } else {
                format!("{}.sqlite3", namespace.label())
            };
            assert!(root.path().join(name).exists());
        }
        let databases: Vec<_> = std::fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "sqlite3"))
            .collect();
        assert_eq!(databases.len(), 3);
        for namespace in ["live", "demo", "replay"] {
            assert!(databases.contains(&root.path().join(format!("{namespace}.sqlite3"))));
        }
        let key = identity_key(root.path()).unwrap();
        assert_ne!(key, [0; 32]);
        assert_eq!(identity_key(root.path()).unwrap(), key);
    }

    #[test]
    fn invalid_or_cancelled_catalog_opens_honest_partial_capture() {
        for cancelled in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let options = RuntimeOptions::live(
                root.path().into(),
                Some(root.path().join("missing-resources.assets")),
            );
            let views = Arc::new(Views::default());
            let mut adapter = options
                .open(
                    &AtomicBool::new(cancelled),
                    views.clone(),
                    Bridge::default(),
                )
                .unwrap();
            assert!(root.path().join("live.sqlite3").exists());
            let status = views.store.lock().unwrap().clone();
            assert_eq!(status.state, maxed_types::StoreState::Ready);
            assert!(status.message.contains(if cancelled {
                "extraction cancelled"
            } else {
                "validation failed"
            }));
            assert!(!status.message.contains("Npcap"));
            assert!(views
                .snapshot
                .lock()
                .unwrap()
                .issues
                .contains(&CoverageIssue::MissingCatalog));
            adapter.traffic(Timestamp(1)).unwrap();
            adapter
                .finish(Timestamp(1), FightEnd::CaptureStopped)
                .unwrap();
            adapter.close().unwrap();
            assert!(views.store.lock().unwrap().message.contains(if cancelled {
                "extraction cancelled"
            } else {
                "validation failed"
            }));
        }
    }
}
