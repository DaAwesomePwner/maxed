//! Last-good catalog leases, bounded presentation lookup and local PNG delivery.
use maxed_assets::{AssetError, AssetPaths, AssetProgress, CachedCatalog};
use maxed_types::{
    AssetCheck, Catalog, ExtractionStage, ItemMatch, ObjectVisual, SetupStatus, VisualPage,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

pub use maxed_assets::sprite_delivery::{
    dungeon_portal_type, read_sprite, sprite_location, ASSET_ERROR, SPRITE_ORIGIN,
};
pub const CANCELLED: &str = "Asset selection was cancelled.";

pub trait AssetLoader: Send + Sync {
    fn load(
        &self,
        paths: &AssetPaths,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(AssetProgress),
    ) -> Result<CachedCatalog, AssetError>;
}
pub struct NativeAssets;
impl AssetLoader for NativeAssets {
    fn load(
        &self,
        paths: &AssetPaths,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(AssetProgress),
    ) -> Result<CachedCatalog, AssetError> {
        maxed_assets::load_or_extract(paths, cancel, &mut |stage| progress(stage))
    }
}

pub fn resources_path(path: &Path) -> Result<PathBuf, String> {
    let path = local_path(path)?;
    let path = if path.is_dir() {
        path.join("resources.assets")
    } else {
        path
    };
    if path.file_name().and_then(|v| v.to_str()) != Some("resources.assets") {
        return Err(ASSET_ERROR.into());
    }
    let path = local_path(&path)?;
    if !path.is_file() {
        return Err(ASSET_ERROR.into());
    }
    Ok(path)
}

pub(crate) fn local_path(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(ASSET_ERROR.into());
    }
    let text = path.to_string_lossy();
    // UNC/network and device namespace selections are outside local discovery.
    if text.starts_with("\\\\") || text.starts_with("//") {
        return Err(ASSET_ERROR.into());
    }
    // Reject junctions/symlinks before descending into them, including links
    // which redirect an apparently local selection to a network share.
    reject_links(path)?;
    let path = path.canonicalize().map_err(|_| ASSET_ERROR)?;
    let text = path.to_string_lossy();
    let local = text.strip_prefix("\\\\?\\").unwrap_or(&text);
    if local.starts_with("UNC\\") || local.starts_with("\\\\") {
        return Err(ASSET_ERROR.into());
    }
    Ok(PathBuf::from(local))
}

fn reject_links(path: &Path) -> Result<(), String> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if !matches!(component, std::path::Component::Normal(_)) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&current).map_err(|_| ASSET_ERROR)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(ASSET_ERROR.into());
            }
        }
        if metadata.file_type().is_symlink() {
            return Err(ASSET_ERROR.into());
        }
    }
    Ok(())
}

struct State {
    generation: u64,
    cancel: Arc<AtomicBool>,
    cache: Option<CachedCatalog>,
    status: SetupStatus,
    closed: bool,
}
pub struct AssetBridge {
    state: Mutex<State>,
    loader: Arc<dyn AssetLoader>,
}
#[derive(Clone)]
pub struct Operation {
    generation: u64,
    pub cancel: Arc<AtomicBool>,
    previous_status: SetupStatus,
}
impl Default for AssetBridge {
    fn default() -> Self {
        Self::new(Arc::new(NativeAssets))
    }
}
impl AssetBridge {
    pub fn scope_loot_tiers(&self) -> Vec<(u16, Option<String>)> {
        self.cache()
            .map(|cache| {
                cache
                    .catalog
                    .items
                    .iter()
                    .map(|item| (item.object_type, item.tier.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn new(loader: Arc<dyn AssetLoader>) -> Self {
        Self {
            state: Mutex::new(State {
                generation: 0,
                cancel: Arc::new(AtomicBool::new(false)),
                cache: None,
                status: SetupStatus::default(),
                closed: false,
            }),
            loader,
        }
    }
    pub fn begin(&self) -> Result<Operation, String> {
        let mut state = self.state.lock().map_err(|_| ASSET_ERROR)?;
        if state.closed {
            return Err(CANCELLED.into());
        }
        state.cancel.store(true, Ordering::Release);
        state.generation = state.generation.checked_add(1).ok_or(ASSET_ERROR)?;
        state.cancel = Arc::new(AtomicBool::new(false));
        let mut previous_status = state.status.clone();
        if matches!(
            previous_status.assets,
            AssetCheck::Finding | AssetCheck::Extracting
        ) {
            previous_status.assets = if state.cache.is_some() {
                AssetCheck::Ready
            } else {
                AssetCheck::Missing
            };
            previous_status.extraction_stage = None;
            previous_status.message = "Previous asset selection retained.".into();
        }
        state.status.assets = AssetCheck::Finding;
        state.status.extraction_stage = None;
        state.status.message = "Checking the selected game client.".into();
        Ok(Operation {
            generation: state.generation,
            cancel: state.cancel.clone(),
            previous_status,
        })
    }
    pub fn cancel(&self, closing: bool) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.cancel.store(true, Ordering::Release);
        state.closed |= closing;
        if !matches!(
            state.status.assets,
            AssetCheck::Finding | AssetCheck::Extracting
        ) {
            return;
        }
        state.status.assets = if state.cache.is_some() {
            AssetCheck::Ready
        } else {
            AssetCheck::Missing
        };
        state.status.extraction_stage = None;
        state.status.message = "Asset selection cancelled; previous assets retained.".into();
    }
    /// Non-asset failures must not replace a valid asset diagnosis with an
    /// extraction error. Only this still-pending generation can restore it.
    pub fn abandon_if_pending(&self, op: &Operation) {
        self.finish_discovery(op, None);
    }
    pub fn finish_discovery(&self, op: &Operation, message: Option<&'static str>) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.generation == op.generation
            && !op.cancel.load(Ordering::Acquire)
            && !state.closed
            && matches!(
                state.status.assets,
                AssetCheck::Finding | AssetCheck::Extracting
            )
        {
            state.status = op.previous_status.clone();
            if let Some(message) = message {
                state.status.message = message.into();
            }
            op.cancel.store(true, Ordering::Release);
        }
    }
    pub fn resume(&self) {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).closed = false;
    }
    pub fn failed(&self, op: &Operation) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.generation == op.generation && !op.cancel.load(Ordering::Acquire) && !state.closed
        {
            if state.status.assets != AssetCheck::Unsupported {
                state.status.assets = AssetCheck::Failed;
            }
            state.status.extraction_stage = None;
            state.status.message = ASSET_ERROR.into();
        }
    }
    pub fn current(&self, op: &Operation) -> bool {
        let state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        !state.closed && state.generation == op.generation && !op.cancel.load(Ordering::Acquire)
    }
    pub fn status(&self) -> SetupStatus {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .status
            .clone()
    }
    pub fn cache(&self) -> Option<CachedCatalog> {
        self.state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .cache
            .clone()
    }
    pub fn prepare(
        &self,
        path: &Path,
        app_data: &Path,
        op: &Operation,
    ) -> Result<(PathBuf, CachedCatalog), String> {
        if !self.current(op) {
            return Err(CANCELLED.into());
        }
        let result = (|| {
            let path = resources_path(path).map_err(|_| AssetError::Io)?;
            let cache = self.loader.load(
                &AssetPaths {
                    resources: path.clone(),
                    app_data: app_data.to_owned(),
                },
                &op.cancel,
                &mut |stage| {
                    let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
                    if state.generation == op.generation
                        && !op.cancel.load(Ordering::Acquire)
                        && !state.closed
                    {
                        state.status.client_found = true;
                        state.status.assets = AssetCheck::Extracting;
                        state.status.extraction_stage = Some(match stage {
                            AssetProgress::Fingerprinting => ExtractionStage::Fingerprinting,
                            AssetProgress::Reading => ExtractionStage::Reading,
                            AssetProgress::Catalog => ExtractionStage::Catalog,
                            AssetProgress::Sprites => ExtractionStage::Sprites,
                            AssetProgress::Publishing => ExtractionStage::Publishing,
                        });
                        state.status.message = "Preparing local game assets.".into();
                    }
                },
            )?;
            Ok((path, cache))
        })();
        if !self.current(op) {
            return Err(CANCELLED.into());
        }
        result.map_err(|error| {
            let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
            if state.generation == op.generation {
                state.status.assets = match error {
                    AssetError::UnsupportedVersion | AssetError::UnsupportedTexture => {
                        AssetCheck::Unsupported
                    }
                    _ => AssetCheck::Failed,
                };
                state.status.extraction_stage = None;
                state.status.message = ASSET_ERROR.into();
            }
            ASSET_ERROR.into()
        })
    }
    /// Commit under the generation lock so replacement/cancellation cannot
    /// interleave between persisting the selection and publishing its cache.
    pub fn commit(
        &self,
        op: &Operation,
        cache: CachedCatalog,
        persist: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| ASSET_ERROR)?;
        if state.closed || state.generation != op.generation || op.cancel.load(Ordering::Acquire) {
            return Err(CANCELLED.into());
        }
        persist()?;
        state.cache = Some(cache.clone());
        cache.prune_in_background();
        state.status.assets = AssetCheck::Ready;
        state.status.client_found = true;
        state.status.extraction_stage = None;
        state.status.message = "Client assets ready. Start the game and enter a realm.".into();
        Ok(())
    }
    pub fn discovery_message(&self, message: &'static str) {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if !matches!(
            state.status.assets,
            AssetCheck::Finding | AssetCheck::Extracting
        ) {
            state.status.message = message.into();
        }
    }
    pub fn visuals(&self, types: &[u16]) -> Result<VisualPage, String> {
        let cache = self.cache().ok_or("Client assets are not available yet.")?;
        visuals(&cache.catalog, types)
    }
    pub fn search_items(&self, query: &str, limit: u8) -> Result<Vec<ItemMatch>, String> {
        let cache = self.cache().ok_or("Client assets are not available yet.")?;
        search_items(&cache.catalog, query, limit)
    }
    pub fn catalog_dungeons(&self) -> Result<Vec<String>, String> {
        let cache = self.cache().ok_or("Client assets are not available yet.")?;
        Ok(catalog_dungeons(&cache.catalog))
    }
    pub fn catalog_dungeon_options(&self) -> Result<Vec<maxed_types::DungeonDef>, String> {
        let cache = self.cache().ok_or("Client assets are not available yet.")?;
        Ok(catalog_dungeon_options(&cache.catalog))
    }
    pub fn dungeon_portal_type(&self, dungeon: &str) -> Result<Option<u16>, String> {
        let cache = self.cache().ok_or("Client assets are not available yet.")?;
        Ok(dungeon_portal_type(&cache.catalog, dungeon))
    }
    pub fn sprite(&self, uri: &str) -> Result<Vec<u8>, String> {
        let (fingerprint, tile) = sprite_location(uri)?;
        let cache = self.cache().ok_or(ASSET_ERROR)?;
        if cache.catalog.client_fingerprint != fingerprint {
            return Err(ASSET_ERROR.into());
        }
        let bytes = read_sprite(&cache, tile)?;
        if self
            .cache()
            .is_none_or(|current| current.catalog.client_fingerprint != fingerprint)
        {
            return Err(ASSET_ERROR.into());
        }
        Ok(bytes)
    }

    pub fn sprite_response(
        &self,
        main_window: bool,
        method: &tauri::http::Method,
        uri: &str,
    ) -> tauri::http::Response<Vec<u8>> {
        let result = if main_window && method == tauri::http::Method::GET {
            self.sprite(uri)
        } else {
            Err("Unsupported sprite request.".into())
        };
        let (status, bytes) = match result {
            Ok(bytes) => (200, bytes),
            Err(_) => (404, Vec::new()),
        };
        tauri::http::Response::builder()
            .status(status)
            .header("Content-Type", "image/png")
            .header("X-Content-Type-Options", "nosniff")
            .header(
                "Cache-Control",
                if status == 200 {
                    // The URL includes the validated content fingerprint. A
                    // catalog replacement gets a new URL, including on Home.
                    "public, max-age=31536000, immutable"
                } else {
                    "no-store"
                },
            )
            .body(bytes)
            .expect("Static sprite response headers")
    }
}

pub fn visuals(catalog: &Catalog, types: &[u16]) -> Result<VisualPage, String> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for object_type in types {
        if seen.insert(*object_type) {
            unique.push(*object_type);
        }
        if unique.len() > 128 {
            return Err("Request at most 128 unique object types.".into());
        }
    }
    let objects = unique
        .into_iter()
        .map(|object_type| {
            let item = catalog.items.iter().find(|v| v.object_type == object_type);
            let object = catalog
                .objects
                .iter()
                .find(|v| v.object_type == object_type);
            let bag = catalog
                .bags
                .iter()
                .find(|v| v.object_type == object_type)
                .cloned();
            // Classes share their numeric object definition; they do not invent a name.
            let name = item
                .map(|v| v.name.clone())
                .or_else(|| object.map(|v| v.name.clone()))
                .or_else(|| bag.as_ref().map(|v| v.name.clone()));
            let key = item
                .and_then(|v| v.sprite_key.as_ref())
                .or_else(|| object.and_then(|v| v.sprite_key.as_ref()));
            let sprite = key
                .and_then(|key| catalog.sprites.iter().find(|v| &v.key == key))
                .cloned();
            ObjectVisual {
                object_type,
                name,
                sprite,
                bag,
                item_tier: item.and_then(|v| v.tier.clone()),
            }
        })
        .collect();
    Ok(VisualPage {
        client_fingerprint: catalog.client_fingerprint.clone(),
        objects,
    })
}

pub fn search_items(catalog: &Catalog, query: &str, limit: u8) -> Result<Vec<ItemMatch>, String> {
    let query = query.trim();
    if query.is_empty()
        || query.chars().count() > 64
        || query.chars().any(char::is_control)
        || !(1..=25).contains(&limit)
    {
        return Err("Invalid search.".into());
    }
    let query = query.to_lowercase();
    let mut matches: Vec<_> = catalog
        .items
        .iter()
        .filter_map(|item| {
            let name = item.name.to_lowercase();
            name.contains(&query)
                .then(|| (item, name.starts_with(&query)))
        })
        .collect();
    matches.sort_by(|(a, a_prefix), (b, b_prefix)| {
        b_prefix
            .cmp(a_prefix)
            .then_with(|| a.name.len().cmp(&b.name.len()))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.object_type.cmp(&b.object_type))
    });
    Ok(matches
        .into_iter()
        .take(usize::from(limit))
        .map(|(item, _)| ItemMatch {
            object_type: item.object_type,
            name: item.name.clone(),
            tier: item.tier.clone(),
        })
        .collect())
}

pub fn catalog_dungeons(catalog: &Catalog) -> Vec<String> {
    let mut names: Vec<_> = catalog
        .dungeons
        .iter()
        .filter(|d| d.kind == maxed_types::AreaKind::Dungeon)
        .map(|d| d.name.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

pub fn catalog_dungeon_options(catalog: &Catalog) -> Vec<maxed_types::DungeonDef> {
    let mut rows: Vec<_> = catalog
        .dungeons
        .iter()
        .filter(|d| d.kind == maxed_types::AreaKind::Dungeon)
        .cloned()
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows.dedup_by(|a, b| a.name == b.name);
    rows
}
